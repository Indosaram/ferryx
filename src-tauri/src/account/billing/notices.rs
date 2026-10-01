use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::account::billing::entitlement::{EntitlementStatus, GRACE_SECS};
use crate::account::mailer::{BillingMessage, BillingNoticeType};
use crate::account::service::AccountState;
use crate::account::store::{AccountStore, BillingStateRecord};

pub const DAY_SECS: u64 = 86_400;

/// How often the commercial account service re-checks the notices it owes.
///
/// The grace and suspension notices are clock-driven and must fire while the entitlement state
/// stays unchanged, so they cannot ride on an entitlement request: the account service runs one
/// background sweeper on this interval.
pub const NOTICE_SWEEP_INTERVAL: Duration = Duration::from_secs(600);

fn active_dispatches() -> &'static Mutex<HashSet<String>> {
    static IN_FLIGHT: std::sync::OnceLock<Mutex<HashSet<String>>> = std::sync::OnceLock::new();
    IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

/// In-process guard that keeps two evaluations of one owner from dispatching one notice twice.
///
/// The guard cannot outlive the process, so the marker write re-checks the persisted marker
/// before it commits (see [`maybe_notify`]).
struct DispatchGuard {
    key: String,
}

impl Drop for DispatchGuard {
    fn drop(&mut self) {
        active_dispatches().lock().remove(&self.key);
    }
}

/// Runs one synchronous account-store operation on a blocking thread.
async fn offload_blocking<T, F>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    crate::ipc::run_blocking(move || Ok(operation()))
        .await
        .map_err(|error| format!("account blocking task failed: {error}"))?
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    GraceStarted,
    DayBefore,
    Stopped,
    Recovered,
}

impl NoticeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            NoticeKind::GraceStarted => "grace_started",
            NoticeKind::DayBefore => "day_before",
            NoticeKind::Stopped => "stopped",
            NoticeKind::Recovered => "recovered",
        }
    }

    pub fn to_billing_notice_type(&self) -> BillingNoticeType {
        match self {
            NoticeKind::GraceStarted => BillingNoticeType::GraceStarted,
            NoticeKind::DayBefore => BillingNoticeType::DayBefore,
            NoticeKind::Stopped => BillingNoticeType::Stopped,
            NoticeKind::Recovered => BillingNoticeType::Recovered,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeMarker {
    pub cycle_started_at: u64,
    pub kind: NoticeKind,
}

impl NoticeMarker {
    pub fn new(cycle_started_at: u64, kind: NoticeKind) -> Self {
        Self {
            cycle_started_at,
            kind,
        }
    }

    pub fn encode(&self) -> String {
        format!("{}:{}", self.cycle_started_at, self.kind.as_str())
    }

    pub fn parse(raw: &str) -> Option<Self> {
        let (ts_part, kind_part) = raw.split_once(':')?;
        let cycle_started_at = ts_part.parse::<u64>().ok()?;
        let kind = match kind_part {
            "grace_started" => NoticeKind::GraceStarted,
            "day_before" => NoticeKind::DayBefore,
            "stopped" => NoticeKind::Stopped,
            "recovered" => NoticeKind::Recovered,
            _ => return None,
        };
        Some(Self {
            cycle_started_at,
            kind,
        })
    }
}

pub fn render_notice(
    kind: NoticeKind,
    recipient_email: &str,
    portal_url: Option<&str>,
    grace_ends_at: Option<u64>,
    now: u64,
) -> BillingMessage {
    let portal_url_str = portal_url.unwrap_or("https://checka.cc/billing");
    let (subject, body_text, body_html) = match kind {
        NoticeKind::GraceStarted => {
            let deadline_str = grace_ends_at
                .map(|t| format!("Deadline timestamp: {t}"))
                .unwrap_or_else(|| "in 7 days".to_string());
            (
                "Action Required: Ferryx Grace Period Started".to_string(),
                format!(
                    "Your Ferryx subscription requires attention. A 7-day grace period has started ({deadline_str}). Please update your billing information at: {portal_url_str}"
                ),
                format!(
                    "<div style=\"font-family:sans-serif;max-width:560px;margin:0 auto;padding:24px;border:1px solid #e5e7eb;border-radius:8px;\"><h2>Action Required: Grace Period Started</h2><p>Your Ferryx subscription requires attention. A 7-day grace period has started.</p><p><strong>Grace Deadline:</strong> {deadline_str}</p><p><a href=\"{portal_url_str}\" style=\"display:inline-block;padding:12px 24px;background:#ef4444;color:#ffffff;text-decoration:none;border-radius:6px;font-weight:bold;\">Update Payment Method</a></p></div>"
                ),
            )
        }
        NoticeKind::DayBefore => {
            let deadline_str = grace_ends_at
                .map(|t| format!("Deadline timestamp: {t}"))
                .unwrap_or_else(|| "in 24 hours".to_string());
            (
                "Urgent: 24 Hours Remaining Before Ferryx Remote Access Suspension".to_string(),
                format!(
                    "Urgent notice: 24 hours remaining in your Ferryx grace period ({deadline_str}). Remote access will be suspended if payment is not resolved. Manage billing: {portal_url_str}"
                ),
                format!(
                    "<div style=\"font-family:sans-serif;max-width:560px;margin:0 auto;padding:24px;border:1px solid #e5e7eb;border-radius:8px;\"><h2>Urgent: 24 Hours Remaining</h2><p>Your Ferryx grace period expires in 24 hours.</p><p><strong>Suspension Deadline:</strong> {deadline_str}</p><p><a href=\"{portal_url_str}\" style=\"display:inline-block;padding:12px 24px;background:#dc2626;color:#ffffff;text-decoration:none;border-radius:6px;font-weight:bold;\">Resolve Payment Now</a></p></div>"
                ),
            )
        }
        NoticeKind::Stopped => (
            "Ferryx Remote Access Suspended".to_string(),
            format!(
                "Your Ferryx remote access has been suspended due to overdue payment or plan limits. Reactivate your subscription at: {portal_url_str}"
            ),
            format!(
                "<div style=\"font-family:sans-serif;max-width:560px;margin:0 auto;padding:24px;border:1px solid #e5e7eb;border-radius:8px;\"><h2>Remote Access Suspended</h2><p>Your Ferryx remote access has been suspended. Existing sessions and connections are paused until payment is restored.</p><p><a href=\"{portal_url_str}\" style=\"display:inline-block;padding:12px 24px;background:#111827;color:#ffffff;text-decoration:none;border-radius:6px;font-weight:bold;\">Reactivate Subscription</a></p></div>"
            ),
        ),
        NoticeKind::Recovered => (
            "Ferryx Subscription Restored".to_string(),
            format!(
                "Good news! Your Ferryx subscription has been restored and remote access is fully active. Manage your account at: {portal_url_str}"
            ),
            format!(
                "<div style=\"font-family:sans-serif;max-width:560px;margin:0 auto;padding:24px;border:1px solid #e5e7eb;border-radius:8px;\"><h2>Subscription Restored</h2><p>Your Ferryx account has returned to good standing. All remote access and machine limits are active.</p><p><a href=\"{portal_url_str}\" style=\"display:inline-block;padding:12px 24px;background:#10b981;color:#ffffff;text-decoration:none;border-radius:6px;font-weight:bold;\">View Account</a></p></div>"
            ),
        ),
    };

    let deadline = match kind {
        NoticeKind::GraceStarted | NoticeKind::DayBefore => grace_ends_at,
        NoticeKind::Stopped | NoticeKind::Recovered => None,
    };

    let _ = now;

    BillingMessage {
        to: recipient_email.to_string(),
        notice_type: kind.to_billing_notice_type(),
        subject,
        body_text,
        body_html,
        deadline,
        portal_url: portal_url.map(str::to_string),
    }
}

/// Cycle key of a recovery: the grace start of the cycle that just ended.
///
/// `persist_owner_evaluation` clears `grace_started_at` before the notice runs, so the previous
/// grace start survives only inside the last recorded marker (`"<grace_started_at>:<kind>"`).
/// Keying recovery to that cycle is what lets every grace cycle send its own recovery once.
pub fn recovered_cycle_start(last_notice: Option<&str>, now: u64) -> u64 {
    last_notice
        .and_then(NoticeMarker::parse)
        .map(|marker| marker.cycle_started_at)
        .unwrap_or(now)
}

/// Whether an already persisted `existing` marker makes `candidate` redundant or stale.
///
/// A candidate is skipped when the exact marker is recorded already or when a newer grace cycle
/// is recorded; inside one cycle a later transition still replaces an earlier one.
pub fn marker_is_superseded(existing: &NoticeMarker, candidate: &NoticeMarker) -> bool {
    existing == candidate || existing.cycle_started_at > candidate.cycle_started_at
}

pub fn decide_notice(
    prev_status: EntitlementStatus,
    next_status: EntitlementStatus,
    grace_started_at: Option<u64>,
    last_notice: Option<&str>,
    now: u64,
) -> Option<NoticeKind> {
    let parsed_marker = last_notice.and_then(NoticeMarker::parse);

    if next_status == EntitlementStatus::Ok {
        if prev_status != EntitlementStatus::Ok {
            let recovered_cycle = recovered_cycle_start(last_notice, now);
            let already_sent_recovered = matches!(
                parsed_marker.as_ref(),
                Some(marker)
                    if marker.kind == NoticeKind::Recovered
                        && marker.cycle_started_at == recovered_cycle
            );
            if !already_sent_recovered {
                return Some(NoticeKind::Recovered);
            }
        }
        return None;
    }

    let cycle_start = grace_started_at?;
    let deadline = cycle_start.saturating_add(GRACE_SECS);

    let same_cycle_kind = match parsed_marker {
        Some(marker) if marker.cycle_started_at == cycle_start => Some(marker.kind),
        _ => None,
    };

    if next_status == EntitlementStatus::Stopped {
        if same_cycle_kind != Some(NoticeKind::Stopped) {
            return Some(NoticeKind::Stopped);
        }
        return None;
    }

    if matches!(
        next_status,
        EntitlementStatus::PastDue | EntitlementStatus::OverLimit
    ) {
        let day_before_threshold = deadline.saturating_sub(DAY_SECS);
        if now >= day_before_threshold {
            if same_cycle_kind != Some(NoticeKind::DayBefore)
                && same_cycle_kind != Some(NoticeKind::Stopped)
            {
                return Some(NoticeKind::DayBefore);
            }
            return None;
        }

        if same_cycle_kind.is_none() {
            return Some(NoticeKind::GraceStarted);
        }
    }

    None
}

/// Notice the persisted billing state owes, whatever evaluation wrote that state.
///
/// The relay suspension sweeper and the admission paths persist `grace_started_at` /
/// `stopped_at` through `entitlement_for_user`, which never notifies: when such a path observes a
/// transition first, the entitlement route's edge hook later sees no transition at all. This
/// decider therefore reads the persisted state instead of an edge. A stop is announced only once
/// `stopped_at` records it - never from the deadline alone - so the sweeper cannot invent an
/// enforcement decision, and a recovery is keyed to the cycle the last marker recorded.
pub fn decide_notice_from_state(
    grace_started_at: Option<u64>,
    stopped_at: Option<u64>,
    last_notice: Option<&str>,
    now: u64,
) -> Option<NoticeKind> {
    let marker = last_notice.and_then(NoticeMarker::parse);

    let Some(cycle_start) = grace_started_at else {
        // No active grace cycle: the owner is in good standing.
        if stopped_at.is_some() {
            // A stop recorded without a cycle cannot be dated; keep the cycle the last marker
            // recorded so a repeated pass does not send it again.
            let cycle = marker
                .as_ref()
                .map(|existing| existing.cycle_started_at)
                .unwrap_or(now);
            let stopped = NoticeMarker::new(cycle, NoticeKind::Stopped);
            if let Some(existing) = marker.as_ref() {
                if marker_is_superseded(existing, &stopped) {
                    return None;
                }
            }
            return Some(NoticeKind::Stopped);
        }
        return match marker.as_ref() {
            Some(existing) if existing.kind != NoticeKind::Recovered => Some(NoticeKind::Recovered),
            _ => None,
        };
    };

    // A marker from a newer cycle means an evaluation already moved past this record.
    if let Some(existing) = marker.as_ref() {
        if existing.cycle_started_at > cycle_start {
            return None;
        }
    }

    let same_cycle_kind = marker
        .as_ref()
        .filter(|existing| existing.cycle_started_at == cycle_start)
        .map(|existing| existing.kind);

    if stopped_at.is_some() {
        return (same_cycle_kind != Some(NoticeKind::Stopped)).then_some(NoticeKind::Stopped);
    }

    let deadline = cycle_start.saturating_add(GRACE_SECS);
    if now >= deadline.saturating_sub(DAY_SECS) {
        return (same_cycle_kind != Some(NoticeKind::DayBefore)
            && same_cycle_kind != Some(NoticeKind::Stopped))
        .then_some(NoticeKind::DayBefore);
    }

    same_cycle_kind.is_none().then_some(NoticeKind::GraceStarted)
}

pub fn resolve_owner_email(store: &AccountStore, owner_key: &str) -> Option<String> {
    if let Some(user_id) = owner_key.strip_prefix("user:") {
        return store.users.get(user_id).map(|u| u.email.clone());
    }
    if let Some(org_id) = owner_key.strip_prefix("org:") {
        if let Some(org) = store.orgs.get(org_id) {
            return store.users.get(&org.owner_user_id).map(|u| u.email.clone());
        }
        return None;
    }
    store.users.get(owner_key).map(|u| u.email.clone())
}

pub fn resolve_owner_portal_url(store: &AccountStore, owner_key: &str) -> Option<String> {
    let org_id = owner_key.strip_prefix("org:");
    let user_id = owner_key.strip_prefix("user:").unwrap_or(owner_key);

    store
        .subscriptions
        .values()
        .filter(|s| match org_id {
            Some(oid) => s.org_id.as_deref() == Some(oid),
            None => s.org_id.is_none() && s.owner_user_id == user_id,
        })
        .filter_map(|s| s.manage_url.clone())
        .next()
}

/// Dispatches, for one owner, the notice the entitlement edge warrants.
///
/// Delivery contract: the transport call runs outside every store transaction, the marker is
/// committed only after the transport reports success, and a failed delivery returns `Err` with
/// the marker untouched, so the next evaluation retries it. An OS advisory exclusive file lock
/// (`billing-notices.lock`) protects normal concurrent cross-process evaluation and dispatch;
/// a crash between a successful provider call and the marker commit can still re-send one notice,
/// because the external provider call is unkeyed (crash window remains unkeyed and unchanged).
pub async fn maybe_notify(
    state: &Arc<AccountState>,
    owner_key: &str,
    prev_status: EntitlementStatus,
    next_status: EntitlementStatus,
    now: u64,
) -> Result<Option<NoticeKind>, String> {
    notify_from_store(
        state,
        owner_key,
        now,
        move |grace_started_at, _stopped_at, last_notice, now| {
            decide_notice(prev_status, next_status, grace_started_at, last_notice, now)
        },
    )
    .await
}

/// Dispatches the notice the persisted billing state owes, whatever edge produced that state.
///
/// The relay suspension sweeper and the admission paths persist `grace_started_at` /
/// `stopped_at` through `entitlement_for_user`, which never notifies. When such a path observes a
/// transition first, the entitlement route's edge hook sees no transition afterwards, so the
/// notice would be lost. This entry point derives the owed notice from the state itself; the
/// shared marker dedup keeps it safe next to the edge hook.
pub async fn maybe_notify_from_state(
    state: &Arc<AccountState>,
    owner_key: &str,
    now: u64,
) -> Result<Option<NoticeKind>, String> {
    notify_from_store(
        state,
        owner_key,
        now,
        |grace_started_at, stopped_at, last_notice, now| {
            decide_notice_from_state(grace_started_at, stopped_at, last_notice, now)
        },
    )
    .await
}

/// Cycle key a notice is recorded under.
fn notice_cycle_start(
    kind: NoticeKind,
    grace_started_at: Option<u64>,
    last_notice: Option<&str>,
    now: u64,
) -> u64 {
    match kind {
        NoticeKind::Recovered => recovered_cycle_start(last_notice, now),
        NoticeKind::GraceStarted | NoticeKind::DayBefore | NoticeKind::Stopped => {
            grace_started_at.unwrap_or(now)
        }
    }
}

/// RAII file lock guard for notice dispatches across processes and threads.
struct NoticeFileLock {
    _file: std::fs::File,
}

impl NoticeFileLock {
    fn acquire(dir: &std::path::Path) -> Result<Self, String> {
        let lock_path = dir.join("billing-notices.lock");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| format!("failed to open billing notices lock file: {error}"))?;
        file.lock()
            .map_err(|error| format!("failed to acquire billing notices file lock: {error}"))?;
        Ok(Self { _file: file })
    }
}

/// Reads the owner's billing state, decides with `decide`, then sends and records one notice.
///
/// Synchronization contract:
/// An OS advisory exclusive file lock (`billing-notices.lock`) is acquired across processes
/// before re-reading state, deciding, sending, and committing the notice marker.
/// Concurrent callers (intra-process or cross-process) block on the file lock and re-read
/// the fresh persisted state under the lock before deciding or sending, preventing duplicate
/// email dispatches.
async fn notify_from_store<D>(
    state: &Arc<AccountState>,
    owner_key: &str,
    now: u64,
    decide: D,
) -> Result<Option<NoticeKind>, String>
where
    D: FnOnce(Option<u64>, Option<u64>, Option<&str>, u64) -> Option<NoticeKind> + Send + 'static,
{
    let data_dir = state.data_dir.clone();
    let lock_guard = crate::ipc::run_blocking(move || Ok(NoticeFileLock::acquire(&data_dir)))
        .await
        .map_err(|error| format!("billing notices lock thread panicked: {error}"))??;

    let state_for_read = Arc::clone(state);
    let owner_key_for_read = owner_key.to_string();
    let planned = offload_blocking(
        move || -> Result<Option<(NoticeKind, BillingMessage, NoticeMarker, String)>, String> {
            let store = state_for_read.load()?;
            let record = store.billing_states.get(&owner_key_for_read);
            let grace_started_at = record.and_then(|record| record.grace_started_at);
            let stopped_at = record.and_then(|record| record.stopped_at);
            let last_notice = record.and_then(|record| record.last_notice.clone());

            let kind = match decide(grace_started_at, stopped_at, last_notice.as_deref(), now) {
                Some(kind) => kind,
                None => return Ok(None),
            };

            let recipient_email = resolve_owner_email(&store, &owner_key_for_read)
                .ok_or_else(|| format!("No email found for owner {owner_key_for_read}"))?;
            let portal_url = resolve_owner_portal_url(&store, &owner_key_for_read);
            let grace_ends_at = grace_started_at.map(|started| started.saturating_add(GRACE_SECS));

            let cycle_start =
                notice_cycle_start(kind, grace_started_at, last_notice.as_deref(), now);
            let marker = NoticeMarker::new(cycle_start, kind);
            let message = render_notice(
                kind,
                &recipient_email,
                portal_url.as_deref(),
                grace_ends_at,
                now,
            );

            let dispatch_key = format!("{owner_key_for_read}:{}", marker.encode());
            Ok(Some((kind, message, marker, dispatch_key)))
        },
    )
    .await?;

    let Some((notice_kind, message, marker, dispatch_key)) = planned else {
        drop(lock_guard);
        return Ok(None);
    };

    let dispatch_guard = {
        let mut in_flight = active_dispatches().lock();
        if !in_flight.insert(dispatch_key.clone()) {
            drop(lock_guard);
            return Ok(None);
        }
        DispatchGuard { key: dispatch_key }
    };

    // Move lock_guard, dispatch_guard, mailer, message, and state mutation into ONE single blocking task.
    // If the async parent task is cancelled (e.g. timeout or task abort) while the mailer is sending,
    // the OS blocking thread cannot be aborted: holding lock_guard and dispatch_guard inside this blocking
    // closure guarantees that the lock is NOT dropped before the marker is committed to the store,
    // preventing concurrent or subsequent evaluators from sending duplicate emails.
    let mailer = state.mailer.clone();
    let state_for_write = Arc::clone(state);
    let owner_key_for_write = owner_key.to_string();
    let marker_label = marker.encode();
    let marker_label_for_log = marker_label.clone();

    let recorded = offload_blocking(move || -> Result<bool, String> {
        let _held_lock = lock_guard;
        let _held_dispatch = dispatch_guard;

        mailer
            .send_billing_notice(&message)
            .map_err(|error| format!("Mail delivery failed: {error}"))?;

        state_for_write
            .mutate(move |store| {
                let existing = store
                    .billing_states
                    .get(&owner_key_for_write)
                    .and_then(|record| record.last_notice.as_deref())
                    .and_then(NoticeMarker::parse);
                if let Some(existing) = existing {
                    if marker_is_superseded(&existing, &marker) {
                        return Ok(false);
                    }
                }
                match store.billing_states.get_mut(&owner_key_for_write) {
                    Some(record) => record.last_notice = Some(marker_label.clone()),
                    None => {
                        store.billing_states.insert(
                            owner_key_for_write.clone(),
                            BillingStateRecord {
                                owner_key: owner_key_for_write.clone(),
                                grace_started_at: None,
                                stopped_at: None,
                                last_notice: Some(marker_label.clone()),
                            },
                        );
                    }
                }
                Ok(true)
            })
            .map_err(|error| format!("Failed to record notice marker: {error:?}"))
    })
    .await?;

    if !recorded {
        tracing::warn!(
            owner_key = %owner_key,
            marker = %marker_label_for_log,
            "notice marker already recorded by a concurrent evaluation; kept the newer marker"
        );
    }

    Ok(Some(notice_kind))
}

/// Sends, for every owner with billing state, the notice that state owes.
///
/// State-based on purpose: the relay suspension sweeper and the admission paths persist
/// transitions through `entitlement_for_user` without notifying, so a clock-driven pass must
/// derive the owed notice from `grace_started_at` / `stopped_at` / `last_notice`. At most one
/// notice per owner per pass is sent - the most advanced one the state owes - so a cycle that was
/// first observed late does not replay a backlog of stale notices.
pub async fn sweep_billing_notices(
    state: &Arc<AccountState>,
    now: u64,
) -> Result<Vec<(String, NoticeKind)>, String> {
    let state_for_read = Arc::clone(state);
    let owner_keys = offload_blocking(move || -> Result<Vec<String>, String> {
        let store = state_for_read.load()?;
        Ok(store.billing_states.keys().cloned().collect())
    })
    .await?;

    let mut sent = Vec::new();
    for key in owner_keys {
        match maybe_notify_from_state(state, &key, now).await {
            Ok(Some(kind)) => sent.push((key, kind)),
            Ok(None) => {}
            Err(error) => tracing::warn!(
                owner_key = %key,
                %error,
                "billing notice delivery failed; the untouched marker stays retryable"
            ),
        }
    }

    Ok(sent)
}

/// Starts the periodic billing notice sweeper on the current Tokio runtime.
///
/// The grace, suspension and recovery notices must fire while the entitlement state stays
/// unchanged, so they cannot ride on an entitlement request: this sweeper derives them from the
/// persisted state every [`NOTICE_SWEEP_INTERVAL`] with the billing clock (the debug clock offset
/// included). The first tick runs immediately, so a restart re-checks outstanding notices; a
/// failed tick is logged and never stops the loop.
pub fn spawn_billing_notice_sweeper(state: Arc<AccountState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(NOTICE_SWEEP_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let now = crate::account::billing::routes::billing_now();
            if let Err(error) = sweep_billing_notices(&state, now).await {
                tracing::warn!(%error, "billing notice sweep failed");
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    use crate::account::origin::DeploymentMode;
    use crate::account::service::AccountState;
    use crate::account::store::{BillingStateRecord, MachineRecord, UserRecord};
    use crate::account::mailer::{Mailer, MailerError};

    #[derive(Default)]
    struct TestMailer {
        sent: Mutex<Vec<BillingMessage>>,
        should_fail: AtomicBool,
        fail_count: AtomicUsize,
    }

    impl TestMailer {
        fn new() -> Self {
            Self::default()
        }

        fn messages(&self) -> Vec<BillingMessage> {
            self.sent.lock().unwrap().clone()
        }
    }

    impl Mailer for TestMailer {
        fn send_magic_link(&self, _to: &str, _url: &str) -> Result<(), MailerError> {
            Ok(())
        }

        fn send_billing_notice(&self, message: &BillingMessage) -> Result<(), MailerError> {
            if self.should_fail.load(Ordering::SeqCst) {
                self.fail_count.fetch_add(1, Ordering::SeqCst);
                return Err(MailerError::mail_failed("simulated network outage"));
            }
            self.sent.lock().unwrap().push(message.clone());
            Ok(())
        }
    }

    fn create_test_state(test_dir: &std::path::Path, mailer: Arc<dyn Mailer>) -> Arc<AccountState> {
        let state = AccountState::new(test_dir, "https://checka.cc", mailer)
            .with_deployment_mode(DeploymentMode::Commercial);
        Arc::new(state)
    }

    fn seed_user(state: &AccountState, user_id: &str, email: &str) {
        state
            .mutate(|store| {
                store.users.insert(
                    user_id.to_string(),
                    UserRecord {
                        user_id: user_id.to_string(),
                        email: email.to_string(),
                        created_at: 1000,
                    },
                );
                Ok(())
            })
            .expect("seed user");
    }

    #[test]
    fn test_notice_marker_roundtrip() {
        let marker = NoticeMarker::new(1700000000, NoticeKind::GraceStarted);
        let encoded = marker.encode();
        assert_eq!(encoded, "1700000000:grace_started");

        let parsed = NoticeMarker::parse(&encoded).expect("parse encoded marker");
        assert_eq!(parsed, marker);

        let day_before = NoticeMarker::new(1700000000, NoticeKind::DayBefore);
        assert_eq!(day_before.encode(), "1700000000:day_before");
        assert_eq!(NoticeMarker::parse("1700000000:day_before"), Some(day_before));

        let stopped = NoticeMarker::new(1700000000, NoticeKind::Stopped);
        assert_eq!(stopped.encode(), "1700000000:stopped");
        assert_eq!(NoticeMarker::parse("1700000000:stopped"), Some(stopped));

        let recovered = NoticeMarker::new(0, NoticeKind::Recovered);
        assert_eq!(recovered.encode(), "0:recovered");
        assert_eq!(NoticeMarker::parse("0:recovered"), Some(recovered));

        assert_eq!(NoticeMarker::parse("invalid"), None);
        assert_eq!(NoticeMarker::parse("notanumber:stopped"), None);
        assert_eq!(NoticeMarker::parse("1700000000:unknown_kind"), None);
    }

    #[test]
    fn test_render_all_templates() {
        let email = "user@example.com";
        let portal = "https://billing.checka.cc/p/123";
        let deadline = 1700604800;

        let grace_msg = render_notice(NoticeKind::GraceStarted, email, Some(portal), Some(deadline), 1700000000);
        assert_eq!(grace_msg.to, email);
        assert_eq!(grace_msg.notice_type, BillingNoticeType::GraceStarted);
        assert!(grace_msg.subject.contains("Grace Period Started"));
        assert!(grace_msg.body_text.contains("7-day grace period"));
        assert!(grace_msg.body_text.contains(portal));
        assert_eq!(grace_msg.deadline, Some(deadline));
        assert_eq!(grace_msg.portal_url.as_deref(), Some(portal));

        let day_before_msg = render_notice(NoticeKind::DayBefore, email, Some(portal), Some(deadline), 1700518400);
        assert_eq!(day_before_msg.to, email);
        assert_eq!(day_before_msg.notice_type, BillingNoticeType::DayBefore);
        assert!(day_before_msg.subject.contains("24 Hours Remaining"));
        assert!(day_before_msg.body_text.contains("24 hours remaining"));
        assert_eq!(day_before_msg.deadline, Some(deadline));

        let stopped_msg = render_notice(NoticeKind::Stopped, email, Some(portal), None, 1700604801);
        assert_eq!(stopped_msg.to, email);
        assert_eq!(stopped_msg.notice_type, BillingNoticeType::Stopped);
        assert!(stopped_msg.subject.contains("Suspended"));
        assert!(stopped_msg.body_text.contains("suspended"));
        assert_eq!(stopped_msg.deadline, None);

        let recovered_msg = render_notice(NoticeKind::Recovered, email, Some(portal), None, 1700700000);
        assert_eq!(recovered_msg.to, email);
        assert_eq!(recovered_msg.notice_type, BillingNoticeType::Recovered);
        assert!(recovered_msg.subject.contains("Restored"));
        assert!(recovered_msg.body_text.contains("restored"));
        assert_eq!(recovered_msg.deadline, None);
    }

    #[tokio::test]
    async fn test_virtual_clock_lifecycle_transition_and_dedup() {
        let temp_dir = std::env::temp_dir().join(format!("notices-test-cycle-{}", uuid::Uuid::new_v4()));
        let mailer = Arc::new(TestMailer::new());
        let state = create_test_state(&temp_dir, mailer.clone());

        let user_id = "user_cycle_1";
        let email = "cycle1@example.com";
        seed_user(&state, user_id, email);

        let t0 = 1_000_000u64;

        state
            .mutate(|store| {
                store.billing_states.insert(
                    format!("user:{user_id}"),
                    BillingStateRecord {
                        owner_key: format!("user:{user_id}"),
                        grace_started_at: Some(t0),
                        stopped_at: None,
                        last_notice: None,
                    },
                );
                Ok(())
            })
            .unwrap();

        let res = maybe_notify(
            &state,
            &format!("user:{user_id}"),
            EntitlementStatus::Ok,
            EntitlementStatus::PastDue,
            t0,
        )
        .await;
        assert_eq!(res, Ok(Some(NoticeKind::GraceStarted)));

        let msgs = mailer.messages();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].to, email);
        assert_eq!(msgs[0].notice_type, BillingNoticeType::GraceStarted);

        let res_dup = maybe_notify(
            &state,
            &format!("user:{user_id}"),
            EntitlementStatus::PastDue,
            EntitlementStatus::PastDue,
            t0 + 100,
        )
        .await;
        assert_eq!(res_dup, Ok(None));
        assert_eq!(mailer.messages().len(), 1);

        let t_day_before = t0 + GRACE_SECS - DAY_SECS;
        let res_day_before = maybe_notify(
            &state,
            &format!("user:{user_id}"),
            EntitlementStatus::PastDue,
            EntitlementStatus::PastDue,
            t_day_before,
        )
        .await;
        assert_eq!(res_day_before, Ok(Some(NoticeKind::DayBefore)));

        let msgs = mailer.messages();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].notice_type, BillingNoticeType::DayBefore);

        let res_day_before_dup = maybe_notify(
            &state,
            &format!("user:{user_id}"),
            EntitlementStatus::PastDue,
            EntitlementStatus::PastDue,
            t_day_before + 3600,
        )
        .await;
        assert_eq!(res_day_before_dup, Ok(None));
        assert_eq!(mailer.messages().len(), 2);

        let t_stopped = t0 + GRACE_SECS + 1;
        state
            .mutate(|store| {
                if let Some(b) = store.billing_states.get_mut(&format!("user:{user_id}")) {
                    b.stopped_at = Some(t_stopped);
                }
                Ok(())
            })
            .unwrap();

        let res_stopped = maybe_notify(
            &state,
            &format!("user:{user_id}"),
            EntitlementStatus::PastDue,
            EntitlementStatus::Stopped,
            t_stopped,
        )
        .await;
        assert_eq!(res_stopped, Ok(Some(NoticeKind::Stopped)));

        let msgs = mailer.messages();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[2].notice_type, BillingNoticeType::Stopped);

        let res_stopped_dup = maybe_notify(
            &state,
            &format!("user:{user_id}"),
            EntitlementStatus::Stopped,
            EntitlementStatus::Stopped,
            t_stopped + 3600,
        )
        .await;
        assert_eq!(res_stopped_dup, Ok(None));
        assert_eq!(mailer.messages().len(), 3);

        let t_recovered = t_stopped + 10_000;
        state
            .mutate(|store| {
                if let Some(b) = store.billing_states.get_mut(&format!("user:{user_id}")) {
                    b.grace_started_at = None;
                    b.stopped_at = None;
                }
                Ok(())
            })
            .unwrap();

        let res_recovered = maybe_notify(
            &state,
            &format!("user:{user_id}"),
            EntitlementStatus::Stopped,
            EntitlementStatus::Ok,
            t_recovered,
        )
        .await;
        assert_eq!(res_recovered, Ok(Some(NoticeKind::Recovered)));

        let msgs = mailer.messages();
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[3].notice_type, BillingNoticeType::Recovered);

        let res_recovered_dup = maybe_notify(
            &state,
            &format!("user:{user_id}"),
            EntitlementStatus::Ok,
            EntitlementStatus::Ok,
            t_recovered + 100,
        )
        .await;
        assert_eq!(res_recovered_dup, Ok(None));
        assert_eq!(mailer.messages().len(), 4);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_new_grace_cycle_after_recovery() {
        let temp_dir = std::env::temp_dir().join(format!("notices-test-cycle2-{}", uuid::Uuid::new_v4()));
        let mailer = Arc::new(TestMailer::new());
        let state = create_test_state(&temp_dir, mailer.clone());

        let user_id = "user_cycle_2";
        let email = "cycle2@example.com";
        seed_user(&state, user_id, email);

        let t0 = 2_000_000u64;

        state
            .mutate(|store| {
                store.billing_states.insert(
                    format!("user:{user_id}"),
                    BillingStateRecord {
                        owner_key: format!("user:{user_id}"),
                        grace_started_at: Some(t0),
                        stopped_at: None,
                        last_notice: None,
                    },
                );
                Ok(())
            })
            .unwrap();

        let res1 = maybe_notify(
            &state,
            &format!("user:{user_id}"),
            EntitlementStatus::Ok,
            EntitlementStatus::PastDue,
            t0,
        )
        .await;
        assert_eq!(res1, Ok(Some(NoticeKind::GraceStarted)));

        state
            .mutate(|store| {
                if let Some(b) = store.billing_states.get_mut(&format!("user:{user_id}")) {
                    b.grace_started_at = None;
                }
                Ok(())
            })
            .unwrap();

        let res_rec = maybe_notify(
            &state,
            &format!("user:{user_id}"),
            EntitlementStatus::PastDue,
            EntitlementStatus::Ok,
            t0 + 5000,
        )
        .await;
        assert_eq!(res_rec, Ok(Some(NoticeKind::Recovered)));

        let t1 = t0 + 100_000;
        state
            .mutate(|store| {
                if let Some(b) = store.billing_states.get_mut(&format!("user:{user_id}")) {
                    b.grace_started_at = Some(t1);
                }
                Ok(())
            })
            .unwrap();

        let res2 = maybe_notify(
            &state,
            &format!("user:{user_id}"),
            EntitlementStatus::Ok,
            EntitlementStatus::PastDue,
            t1,
        )
        .await;
        assert_eq!(res2, Ok(Some(NoticeKind::GraceStarted)));

        let msgs = mailer.messages();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].notice_type, BillingNoticeType::GraceStarted);
        assert_eq!(msgs[1].notice_type, BillingNoticeType::Recovered);
        assert_eq!(msgs[2].notice_type, BillingNoticeType::GraceStarted);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    /// Writes the owner's grace state, creating the billing row when it does not exist yet.
    ///
    /// No row is guaranteed before the notice path runs (a first cycle has no persisted state),
    /// so a missing row is seeded here instead of silently skipping the write. The read-back
    /// assertion keeps that guarantee loud: a helper that writes nothing must fail the test that
    /// relies on it rather than leaving the notice decision to be read as "no notice owed".
    fn set_grace_started_at(state: &AccountState, owner_key: &str, value: Option<u64>) {
        state
            .mutate(|store| {
                let record = store
                    .billing_states
                    .entry(owner_key.to_string())
                    .or_insert_with(|| BillingStateRecord {
                        owner_key: owner_key.to_string(),
                        grace_started_at: None,
                        stopped_at: None,
                        last_notice: None,
                    });
                record.grace_started_at = value;
                record.stopped_at = None;
                Ok(())
            })
            .expect("set grace state");

        let store = state.load().expect("load store");
        let record = store.billing_states.get(owner_key).unwrap_or_else(|| {
            panic!("set_grace_started_at must leave a billing_states row for {owner_key}")
        });
        assert_eq!(
            record.grace_started_at, value,
            "set_grace_started_at must persist graceStartedAt={value:?} for {owner_key}"
        );
    }

    fn marker_of(state: &AccountState, owner_key: &str) -> Option<String> {
        state
            .load()
            .expect("load store")
            .billing_states
            .get(owner_key)
            .and_then(|record| record.last_notice.clone())
    }

    #[test]
    fn test_marker_supersede_predicate_keeps_newer_cycle() {
        let candidate = NoticeMarker::new(1700000000, NoticeKind::GraceStarted);

        assert!(marker_is_superseded(
            &NoticeMarker::new(1700000000, NoticeKind::GraceStarted),
            &candidate
        ));
        assert!(
            !marker_is_superseded(&NoticeMarker::new(1700000000, NoticeKind::Stopped), &candidate),
            "a later transition inside the same cycle still wins"
        );
        assert!(
            marker_is_superseded(
                &NoticeMarker::new(1700000001, NoticeKind::GraceStarted),
                &candidate
            ),
            "a newer cycle must survive a late write from an older evaluation"
        );
        assert!(!marker_is_superseded(
            &NoticeMarker::new(1699999999, NoticeKind::Stopped),
            &candidate
        ));
    }

    #[tokio::test]
    async fn test_two_grace_cycles_each_send_exactly_one_recovered_notice() {
        let temp_dir =
            std::env::temp_dir().join(format!("notices-test-two-cycles-{}", uuid::Uuid::new_v4()));
        let mailer = Arc::new(TestMailer::new());
        let state = create_test_state(&temp_dir, mailer.clone());

        let user_id = "user_two_cycles";
        let owner_key = format!("user:{user_id}");
        seed_user(&state, user_id, "two-cycles@example.com");

        // First grace cycle: grace started -> stopped -> recovered.
        let t0 = 6_000_000u64;
        set_grace_started_at(&state, &owner_key, Some(t0));
        assert_eq!(
            maybe_notify(
                &state,
                &owner_key,
                EntitlementStatus::Ok,
                EntitlementStatus::PastDue,
                t0,
            )
            .await,
            Ok(Some(NoticeKind::GraceStarted))
        );

        let t0_stopped = t0 + GRACE_SECS + 1;
        assert_eq!(
            maybe_notify(
                &state,
                &owner_key,
                EntitlementStatus::PastDue,
                EntitlementStatus::Stopped,
                t0_stopped,
            )
            .await,
            Ok(Some(NoticeKind::Stopped))
        );

        // routes.rs clears the grace state before the notice runs on recovery.
        set_grace_started_at(&state, &owner_key, None);
        assert_eq!(
            maybe_notify(
                &state,
                &owner_key,
                EntitlementStatus::Stopped,
                EntitlementStatus::Ok,
                t0_stopped + 1,
            )
            .await,
            Ok(Some(NoticeKind::Recovered))
        );
        assert_eq!(
            marker_of(&state, &owner_key),
            Some(format!("{t0}:recovered")),
            "recovery must be keyed to the grace start of the cycle that just ended"
        );

        assert_eq!(
            maybe_notify(
                &state,
                &owner_key,
                EntitlementStatus::Stopped,
                EntitlementStatus::Ok,
                t0_stopped + 2,
            )
            .await,
            Ok(None),
            "repeating the recovery transition must not send twice"
        );

        // Second grace cycle: its recovery must not be deduplicated against the first cycle.
        let t1 = t0 + 4 * GRACE_SECS;
        set_grace_started_at(&state, &owner_key, Some(t1));
        assert_eq!(
            maybe_notify(
                &state,
                &owner_key,
                EntitlementStatus::Ok,
                EntitlementStatus::PastDue,
                t1,
            )
            .await,
            Ok(Some(NoticeKind::GraceStarted))
        );

        let t1_stopped = t1 + GRACE_SECS + 1;
        assert_eq!(
            maybe_notify(
                &state,
                &owner_key,
                EntitlementStatus::PastDue,
                EntitlementStatus::Stopped,
                t1_stopped,
            )
            .await,
            Ok(Some(NoticeKind::Stopped))
        );

        set_grace_started_at(&state, &owner_key, None);
        assert_eq!(
            maybe_notify(
                &state,
                &owner_key,
                EntitlementStatus::Stopped,
                EntitlementStatus::Ok,
                t1_stopped + 1,
            )
            .await,
            Ok(Some(NoticeKind::Recovered)),
            "the second cycle sends its own recovered notice"
        );
        assert_eq!(marker_of(&state, &owner_key), Some(format!("{t1}:recovered")));

        let messages = mailer.messages();
        let kinds: Vec<BillingNoticeType> = messages
            .iter()
            .map(|message| message.notice_type)
            .collect();
        assert_eq!(
            kinds,
            vec![
                BillingNoticeType::GraceStarted,
                BillingNoticeType::Stopped,
                BillingNoticeType::Recovered,
                BillingNoticeType::GraceStarted,
                BillingNoticeType::Stopped,
                BillingNoticeType::Recovered,
            ]
        );
        assert_eq!(
            messages
                .iter()
                .filter(|message| message.notice_type == BillingNoticeType::Recovered)
                .count(),
            2,
            "each grace cycle sends exactly one recovered notice"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_mail_failure_leaves_marker_unset_and_retries_successfully() {
        let temp_dir = std::env::temp_dir().join(format!("notices-test-retry-{}", uuid::Uuid::new_v4()));
        let mailer = Arc::new(TestMailer::new());
        let state = create_test_state(&temp_dir, mailer.clone());

        let user_id = "user_retry";
        let email = "retry@example.com";
        seed_user(&state, user_id, email);

        let t0 = 3_000_000u64;

        state
            .mutate(|store| {
                store.billing_states.insert(
                    format!("user:{user_id}"),
                    BillingStateRecord {
                        owner_key: format!("user:{user_id}"),
                        grace_started_at: Some(t0),
                        stopped_at: None,
                        last_notice: None,
                    },
                );
                Ok(())
            })
            .unwrap();

        mailer.should_fail.store(true, Ordering::SeqCst);

        let fail_res = maybe_notify(
            &state,
            &format!("user:{user_id}"),
            EntitlementStatus::Ok,
            EntitlementStatus::PastDue,
            t0,
        )
        .await;
        assert!(fail_res.is_err(), "Must report delivery error");

        let store = state.load().unwrap();
        let bstate = store.billing_states.get(&format!("user:{user_id}")).unwrap();
        assert_eq!(
            bstate.last_notice, None,
            "Marker must NOT be committed when delivery fails"
        );

        mailer.should_fail.store(false, Ordering::SeqCst);

        let retry_res = maybe_notify(
            &state,
            &format!("user:{user_id}"),
            EntitlementStatus::Ok,
            EntitlementStatus::PastDue,
            t0 + 10,
        )
        .await;
        assert_eq!(retry_res, Ok(Some(NoticeKind::GraceStarted)));

        let store_after = state.load().unwrap();
        let bstate_after = store_after.billing_states.get(&format!("user:{user_id}")).unwrap();
        assert_eq!(
            bstate_after.last_notice,
            Some(format!("{t0}:grace_started")),
            "Marker must be committed after successful delivery"
        );

        let msgs = mailer.messages();
        assert_eq!(msgs.len(), 1);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_state_sweep_sends_day_before_without_state_change() {
        let temp_dir = std::env::temp_dir().join(format!("notices-test-sweeper-{}", uuid::Uuid::new_v4()));
        let mailer = Arc::new(TestMailer::new());
        let state = create_test_state(&temp_dir, mailer.clone());

        let user_id1 = "user_sw_1";
        let user_id2 = "user_sw_2";
        seed_user(&state, user_id1, "sw1@example.com");
        seed_user(&state, user_id2, "sw2@example.com");

        let t0 = 4_000_000u64;

        state
            .mutate(|store| {
                store.billing_states.insert(
                    format!("user:{user_id1}"),
                    BillingStateRecord {
                        owner_key: format!("user:{user_id1}"),
                        grace_started_at: Some(t0),
                        stopped_at: None,
                        last_notice: Some(format!("{t0}:grace_started")),
                    },
                );
                store.billing_states.insert(
                    format!("user:{user_id2}"),
                    BillingStateRecord {
                        owner_key: format!("user:{user_id2}"),
                        grace_started_at: Some(t0 + 200_000),
                        stopped_at: None,
                        last_notice: Some(format!("{}:grace_started", t0 + 200_000)),
                    },
                );
                Ok(())
            })
            .unwrap();

        let t_sweep = t0 + GRACE_SECS - DAY_SECS + 100;
        let swept = sweep_billing_notices(&state, t_sweep).await.unwrap();

        assert_eq!(swept.len(), 1);
        assert_eq!(swept[0].0, format!("user:{user_id1}"));
        assert_eq!(swept[0].1, NoticeKind::DayBefore);

        let swept_dup = sweep_billing_notices(&state, t_sweep + 50).await.unwrap();
        assert!(swept_dup.is_empty(), "Consecutive sweep must not duplicate");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_state_sweep_sends_stopped_persisted_without_notification() {
        let temp_dir = std::env::temp_dir().join(format!(
            "notices-test-state-stopped-{}",
            uuid::Uuid::new_v4()
        ));
        let mailer = Arc::new(TestMailer::new());
        let state = create_test_state(&temp_dir, mailer.clone());

        let user_id = "user_state_stopped";
        let email = "state-stopped@example.com";
        seed_user(&state, user_id, email);

        // Two machines on a Free plan put the owner over the limit, so the evaluation stops the
        // owner once the grace deadline has passed. The state below is what the admission and
        // suspension paths persist for such an owner.
        let t0 = 7_000_000u64;
        state
            .mutate(|store| {
                for index in 0..2 {
                    let machine_id = format!("state-stop-machine-{index}");
                    store.machines.insert(
                        machine_id.clone(),
                        MachineRecord {
                            machine_record_id: format!("state-stop-record-{index}"),
                            owner_user_id: user_id.to_string(),
                            machine_id,
                            display_name: format!("machine {index}"),
                            public_key: "public-key".to_string(),
                            attach_public_key: "attach-public-key".to_string(),
                            relay_origin: "https://relay.test".to_string(),
                            platform: "linux".to_string(),
                            enrollment_epoch: 1,
                            enrolled_at: t0,
                            last_seen_at: t0,
                        },
                    );
                }
                store.billing_states.insert(
                    user_id.to_string(),
                    BillingStateRecord {
                        owner_key: user_id.to_string(),
                        grace_started_at: Some(t0),
                        stopped_at: None,
                        last_notice: Some(format!("{t0}:grace_started")),
                    },
                );
                Ok(())
            })
            .expect("seed the over-limit grace cycle");

        // The relay suspension sweeper observes the deadline first and persists the stop through
        // the entitlement evaluation, which never notifies.
        let t_stopped = t0 + GRACE_SECS + 1;
        let entitlement =
            crate::account::billing::routes::entitlement_for_user(&state, user_id, t_stopped)
                .expect("entitlement evaluation must persist the stop");
        assert_eq!(entitlement.status, EntitlementStatus::Stopped);

        let persisted = state
            .load()
            .expect("load store")
            .billing_states
            .get(user_id)
            .cloned()
            .expect("billing state must exist after the evaluation");
        assert_eq!(persisted.stopped_at, Some(t_stopped));
        assert_eq!(
            persisted.last_notice,
            Some(format!("{t0}:grace_started")),
            "the evaluation persists the stop without sending any notice"
        );
        assert!(mailer.messages().is_empty(), "no notice was sent yet");

        let swept = sweep_billing_notices(&state, t_stopped).await.unwrap();
        assert_eq!(swept.len(), 1);
        assert_eq!(swept[0].0, user_id.to_string());
        assert_eq!(swept[0].1, NoticeKind::Stopped);

        let messages = mailer.messages();
        assert_eq!(messages.len(), 1, "exactly one stopped notice");
        assert_eq!(messages[0].to, email);
        assert_eq!(messages[0].notice_type, BillingNoticeType::Stopped);
        assert_eq!(marker_of(&state, user_id), Some(format!("{t0}:stopped")));

        let swept_again = sweep_billing_notices(&state, t_stopped + 600).await.unwrap();
        assert!(
            swept_again.is_empty(),
            "a stopped notice is sent exactly once per cycle"
        );
        assert_eq!(mailer.messages().len(), 1);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_concurrent_evaluation_deduplication() {
        let temp_dir = std::env::temp_dir().join(format!("notices-test-concurrent-{}", uuid::Uuid::new_v4()));
        let mailer = Arc::new(TestMailer::new());
        let state = create_test_state(&temp_dir, mailer.clone());

        let user_id = "user_concurrent";
        let email = "concurrent@example.com";
        seed_user(&state, user_id, email);

        let t0 = 5_000_000u64;

        state
            .mutate(|store| {
                store.billing_states.insert(
                    format!("user:{user_id}"),
                    BillingStateRecord {
                        owner_key: format!("user:{user_id}"),
                        grace_started_at: Some(t0),
                        stopped_at: None,
                        last_notice: None,
                    },
                );
                Ok(())
            })
            .unwrap();

        let mut handles = Vec::new();
        for _ in 0..5 {
            let state_clone = state.clone();
            let user_id_owned = user_id.to_string();
            handles.push(tokio::spawn(async move {
                maybe_notify(
                    &state_clone,
                    &format!("user:{user_id_owned}"),
                    EntitlementStatus::Ok,
                    EntitlementStatus::PastDue,
                    t0,
                )
                .await
            }));
        }

        let mut sent_count = 0;
        for handle in handles {
            let res = handle.await.unwrap().unwrap();
            if res.is_some() {
                sent_count += 1;
            }
        }

        assert_eq!(sent_count, 1, "Only one notice should be dispatched across concurrent triggers");
        assert_eq!(mailer.messages().len(), 1);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    struct LockCheckingMailer {
        lock_dir: std::path::PathBuf,
        sent: Mutex<Vec<BillingMessage>>,
        lock_observed_held: AtomicUsize,
    }

    impl LockCheckingMailer {
        fn new(lock_dir: &std::path::Path) -> Self {
            Self {
                lock_dir: lock_dir.to_path_buf(),
                sent: Mutex::new(Vec::new()),
                lock_observed_held: AtomicUsize::new(0),
            }
        }

        fn messages(&self) -> Vec<BillingMessage> {
            self.sent.lock().unwrap().clone()
        }
    }

    impl Mailer for LockCheckingMailer {
        fn send_magic_link(&self, _to: &str, _url: &str) -> Result<(), MailerError> {
            Ok(())
        }

        fn send_billing_notice(&self, message: &BillingMessage) -> Result<(), MailerError> {
            let lock_path = self.lock_dir.join("billing-notices.lock");
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock_path)
                .map_err(|error| MailerError::mail_failed(format!("failed to open lock file: {error}")))?;

            // When production code holds NoticeFileLock across send_billing_notice,
            // an external try_lock on the same file MUST fail with WouldBlock.
            // If try_lock unexpectedly succeeds, the file lock was NOT held during send (mutation RED).
            match file.try_lock() {
                Ok(()) => {
                    return Err(MailerError::mail_failed(
                        "assertion failed: billing notices file lock was not held during mail send",
                    ));
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    self.lock_observed_held.fetch_add(1, Ordering::SeqCst);
                }
                Err(std::fs::TryLockError::Error(error)) => {
                    return Err(MailerError::mail_failed(format!(
                        "unexpected try_lock error: {error}"
                    )));
                }
            }

            self.sent.lock().unwrap().push(message.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_notice_lock_spans_send_and_reloads_shared_store() {
        let temp_dir = std::env::temp_dir().join(format!("notices-test-lock-reload-{}", uuid::Uuid::new_v4()));
        let mailer1 = Arc::new(LockCheckingMailer::new(&temp_dir));
        let mailer2 = Arc::new(LockCheckingMailer::new(&temp_dir));
        let state1 = create_test_state(&temp_dir, mailer1.clone() as Arc<dyn Mailer>);
        let state2 = create_test_state(&temp_dir, mailer2.clone() as Arc<dyn Mailer>);

        let user_id = "user_cross_process";
        let email = "cross@example.com";
        seed_user(&state1, user_id, email);

        let t0 = 8_000_000u64;

        state1
            .mutate(|store| {
                store.billing_states.insert(
                    user_id.to_string(),
                    BillingStateRecord {
                        owner_key: user_id.to_string(),
                        grace_started_at: Some(t0),
                        stopped_at: None,
                        last_notice: None,
                    },
                );
                Ok(())
            })
            .unwrap();

        // Dispatch notice via state1.
        // Inside mailer1.send_billing_notice, LockCheckingMailer verifies that file.try_lock()
        // fails with WouldBlock, proving the advisory file lock is actively held during external dispatch.
        let res1 = maybe_notify(
            &state1,
            user_id,
            EntitlementStatus::Ok,
            EntitlementStatus::PastDue,
            t0,
        )
        .await
        .unwrap();

        assert_eq!(res1, Some(NoticeKind::GraceStarted));
        assert_eq!(mailer1.lock_observed_held.load(Ordering::SeqCst), 1, "File lock must be held during send");
        assert_eq!(mailer1.messages().len(), 1, "Exactly one email sent by state1");

        // External proof that lock is released once notify_from_store returns:
        // try_lock must now succeed cleanly on the lock file.
        let lock_path = temp_dir.join("billing-notices.lock");
        let external_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_path)
            .expect("open lock file after release");
        external_file.try_lock().expect("file lock must be released after notify_from_store returns");

        // Crucial: immediately drop external_file before state2 attempts to acquire the lock to avoid deadlock!
        drop(external_file);

        // Second instance immediately attempts maybe_notify on state2.
        // Because the marker was persisted under the lock, state2 observes the persisted marker and sends ZERO emails.
        let res2 = maybe_notify(
            &state2,
            user_id,
            EntitlementStatus::Ok,
            EntitlementStatus::PastDue,
            t0,
        )
        .await
        .unwrap();

        assert_eq!(res2, None, "Second instance must not send duplicate notice");
        assert_eq!(mailer2.messages().len(), 0, "Second instance mailer sent zero emails");

        // Verify that the final persisted state in the shared DB records the marker
        let marker = marker_of(&state1, user_id);
        assert_eq!(marker, Some(format!("{t0}:grace_started")));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    struct BlockingCancelMailer {
        entered_tx: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        release_rx: std::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
        send_count: AtomicUsize,
    }

    impl BlockingCancelMailer {
        fn new(
            entered_tx: tokio::sync::oneshot::Sender<()>,
            release_rx: tokio::sync::oneshot::Receiver<()>,
        ) -> Self {
            Self {
                entered_tx: Mutex::new(Some(entered_tx)),
                release_rx: std::sync::Mutex::new(Some(release_rx)),
                send_count: AtomicUsize::new(0),
            }
        }
    }

    impl Mailer for BlockingCancelMailer {
        fn send_magic_link(&self, _to: &str, _url: &str) -> Result<(), MailerError> {
            Ok(())
        }

        fn send_billing_notice(&self, _message: &BillingMessage) -> Result<(), MailerError> {
            if let Some(tx) = self.entered_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
            if let Some(rx) = self.release_rx.lock().unwrap().take() {
                let _ = rx.blocking_recv();
            }
            self.send_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_parent_cancellation_during_send_commits_marker_and_prevents_duplicate() {
        let temp_dir = std::env::temp_dir().join(format!("notices-test-cancel-{}", uuid::Uuid::new_v4()));
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();

        let mailer = Arc::new(BlockingCancelMailer::new(entered_tx, release_rx));
        let state1 = create_test_state(&temp_dir, mailer.clone() as Arc<dyn Mailer>);
        let state2 = create_test_state(&temp_dir, mailer.clone() as Arc<dyn Mailer>);

        let user_id = "user_cancel_race";
        let email = "cancel@example.com";
        seed_user(&state1, user_id, email);

        let t0 = 9_000_000u64;

        state1
            .mutate(|store| {
                store.billing_states.insert(
                    user_id.to_string(),
                    BillingStateRecord {
                        owner_key: user_id.to_string(),
                        grace_started_at: Some(t0),
                        stopped_at: None,
                        last_notice: None,
                    },
                );
                Ok(())
            })
            .unwrap();

        // Spawn async task calling maybe_notify on state1.
        let s1 = state1.clone();
        let u1 = user_id.to_string();
        let handle = tokio::spawn(async move {
            maybe_notify(
                &s1,
                &u1,
                EntitlementStatus::Ok,
                EntitlementStatus::PastDue,
                t0,
            )
            .await
        });

        // Await signal that send_billing_notice was entered on the OS worker thread.
        tokio::time::timeout(Duration::from_secs(3), entered_rx)
            .await
            .expect("timeout awaiting mailer entry")
            .expect("mailer entry channel dropped");

        // Abort / cancel the parent async task while mailer is blocked on release_rx.
        handle.abort();
        let join_err = handle.await.expect_err("handle must be cancelled");
        assert!(join_err.is_cancelled(), "join error must be cancelled");

        // Release the blocking thread so send_billing_notice finishes and the closure proceeds to state.mutate.
        release_tx.send(()).expect("release mailer");

        // Call maybe_notify on state2 immediately with a bounded timeout.
        // Because the production file lock is held by the first blocking thread until it commits the marker,
        // state2's notify_from_store blocks on NoticeFileLock::acquire until the first thread releases it.
        // Once acquired, state2 re-reads the fresh store, observes the committed marker, and returns None.
        let res2 = tokio::time::timeout(
            Duration::from_secs(3),
            maybe_notify(
                &state2,
                user_id,
                EntitlementStatus::Ok,
                EntitlementStatus::PastDue,
                t0,
            ),
        )
        .await
        .expect("state2 notification timed out waiting for lock release")
        .expect("state2 notification evaluation failed");

        assert_eq!(res2, None, "Subsequent call must not send duplicate email");
        assert_eq!(mailer.send_count.load(Ordering::SeqCst), 1, "Exactly one email sent in total");
        assert_eq!(marker_of(&state1, user_id), Some(format!("{t0}:grace_started")), "Marker must be committed");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
