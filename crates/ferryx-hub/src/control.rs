//! Hub control socket. `stop` drops chat, cron, and HTTP tasks. The daemon and its
//! PTYs stay up. `start` is the signal to bind them again. `status` reports what is live.
//! `cutover` moves the owner's supervision from the legacy intake to the unified supervisor
//! (refused unless the mhc engine is ready with every required capability), `rollback` moves
//! it back without touching receipts or PTYs, and `intake` reports which one is open
//! ([`crate::supervisor::cutover`]).
use serde::Serialize;
use serde_json::{json, Value};

use crate::error::{HubError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlOp {
    Stop,
    Start,
    Status,
    /// `ferryx hub cron approve-rerun <id>`: the owner approves one more verified rerun of a
    /// cron problem waiting on them. Reaching this socket (0600, owner-only) is the authority;
    /// no secret travels through chat.
    ApproveRerun(i64),
    Cutover,
    Rollback,
    Intake,
    Report(Box<ReportRequest>),
    ReportStatus(i64),
    ReportMutate(Box<crate::report_mutation::Request>),
    ReportAdopt(Box<crate::report_adoption::Request>),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportRequest {
    pub source: String,
    pub topic: String,
    pub salience: crate::report_gate::Salience,
    pub body: String,
    pub state: String,
    pub quote: Option<String>,
    pub sender: Option<String>,
    pub origin: Option<String>,
    pub thread: Option<String>,
    pub blocks: Option<Vec<Value>>,
    /// A producer must supply the eligibility evidence it observed, not a guessed age.
    pub blocked_reason: Option<String>,
    /// The cron run whose failure this report is about (`FERRYX_CRON_RUN_ID` of the reporting
    /// script), when the producer runs as a hub cron job.
    #[serde(default)]
    pub incident: Option<String>,
}

impl ReportRequest {
    fn report(&self) -> crate::report_gate::Report {
        let mut report = crate::report_gate::Report::new(
            uuid::Uuid::new_v4().to_string(), &self.source, &self.topic, self.salience, &self.body,
        ).with_state(&self.state);
        report.quote.clone_from(&self.quote);
        report.sender.clone_from(&self.sender);
        report.origin_ref.clone_from(&self.origin);
        report.thread.clone_from(&self.thread);
        report.with_incident(self.incident.clone())
    }
}

pub fn parse_control(line: &str) -> Result<ControlOp> {
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|error| HubError::message(format!("hub control: {error}")))?;
    match value.get("op").and_then(Value::as_str) {
        Some("stop") => Ok(ControlOp::Stop),
        Some("start") => Ok(ControlOp::Start),
        Some("status") => Ok(ControlOp::Status),
        Some("approve_rerun") => value
            .get("problem")
            .and_then(Value::as_i64)
            .map(ControlOp::ApproveRerun)
            .ok_or_else(|| HubError::message("approve_rerun needs a numeric problem id")),
        Some("cutover") => Ok(ControlOp::Cutover),
        Some("rollback") => Ok(ControlOp::Rollback),
        Some("intake") => Ok(ControlOp::Intake),
        Some("report_status") => value.get("outbox_id").and_then(Value::as_i64)
            .filter(|id| *id > 0).map(ControlOp::ReportStatus)
            .ok_or_else(|| HubError::message("report_status needs a positive outbox_id")),
        Some("report_adopt") => {
            let mut payload = value.clone();
            if let Some(object) = payload.as_object_mut() { object.remove("op"); }
            let request: crate::report_adoption::Request = serde_json::from_value(payload)
                .map_err(|error| HubError::message(format!("invalid adoption: {error}")))?;
            request.validate()?;
            Ok(ControlOp::ReportAdopt(Box::new(request)))
        }
        Some("report_mutate") => {
            let mut payload = value.clone();
            if let Some(object) = payload.as_object_mut() { object.remove("op"); }
            let request: crate::report_mutation::Request = serde_json::from_value(payload)
                .map_err(|error| HubError::message(format!("invalid mutation: {error}")))?;
            request.validate()?;
            Ok(ControlOp::ReportMutate(Box::new(request)))
        }
        Some("report") => {
            let mut payload = value.clone();
            if let Some(object) = payload.as_object_mut() {
                object.remove("op");
            }
            let request: ReportRequest = serde_json::from_value(payload)
                .map_err(|error| HubError::message(format!("invalid report: {error}")))?;
            if [&request.source, &request.topic, &request.body, &request.state]
                .iter().any(|field| field.trim().is_empty())
                || request.blocked_reason.as_deref().is_some_and(|reason| reason.trim().is_empty())
                || request.incident.as_deref().is_some_and(|run| run.trim().is_empty())
                || request.thread.as_deref().is_some_and(|thread| thread.trim().is_empty())
                || request.blocks.as_ref().is_some_and(|blocks| blocks.is_empty() || blocks.len() > 50
                    || blocks.iter().any(|block| !block.is_object()))
            {
                return Err(HubError::message("report fields and supplied eligibility evidence must not be empty"));
            }
            Ok(ControlOp::Report(Box::new(request)))
        }
        _ => Err(HubError::message(
            "hub control expected stop, start, status, approve_rerun, cutover, rollback, or intake",
        )),
    }
}

pub fn control_socket(home: &std::path::Path) -> std::path::PathBuf {
    home.join(".ferryx").join("hub").join("control.sock")
}

/// `hub stop` writes `hub_stopped = 1` and `hub start` writes `hub_stopped = 0`.
/// A hub database with neither is a fresh install. It stays stopped too, because the
/// hub uses the Slack tokens omo-gateway used and would otherwise open a second
/// Socket Mode connection next to a running omo-gateway. Only an explicit `hub start`
/// (the cutover script sends one after omo is unloaded) or `FERRYX_HUB=on` opens it.
pub async fn mark_stopped(pool: &sqlx::SqlitePool) -> Result<()> {
    sqlx::query(
        "INSERT INTO hub_meta(key, value) VALUES('hub_stopped', '1')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clear_stopped(pool: &sqlx::SqlitePool) -> Result<()> {
    sqlx::query(
        "INSERT INTO hub_meta(key, value) VALUES('hub_stopped', '0')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// The owner pauses (`true`) or resumes (`false`) every supervisor action at once. The flag
/// is the hub's, not a lane's: recovery and answers read it ([`is_paused`]) before they act,
/// and it survives a restart. It never stops the hub or touches a PTY.
pub async fn set_paused(pool: &sqlx::SqlitePool, paused: bool) -> Result<()> {
    sqlx::query(
        "INSERT INTO hub_meta(key, value) VALUES('supervisor_paused', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(if paused { "1" } else { "0" })
    .execute(pool)
    .await?;
    Ok(())
}

/// Whether the owner paused every supervisor action. A hub that never set it is not paused.
pub async fn is_paused(pool: &sqlx::SqlitePool) -> Result<bool> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = 'supervisor_paused'")
            .fetch_optional(pool)
            .await?;
    Ok(value.as_deref() == Some("1"))
}

pub async fn is_stopped(pool: &sqlx::SqlitePool) -> Result<bool> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = 'hub_stopped'")
            .fetch_optional(pool)
            .await?;
    Ok(value.as_deref() != Some("0"))
}

pub fn control_line(op: ControlOp) -> String {
    match op {
        ControlOp::Stop => "{\"op\":\"stop\"}\n".to_owned(),
        ControlOp::Start => "{\"op\":\"start\"}\n".to_owned(),
        ControlOp::Status => "{\"op\":\"status\"}\n".to_owned(),
        ControlOp::ApproveRerun(problem) => {
            format!("{}\n", json!({"op": "approve_rerun", "problem": problem}))
        }
        ControlOp::Cutover => "{\"op\":\"cutover\"}\n".to_owned(),
        ControlOp::Rollback => "{\"op\":\"rollback\"}\n".to_owned(),
        ControlOp::Intake => "{\"op\":\"intake\"}\n".to_owned(),
        ControlOp::ReportStatus(id) => format!("{}\n", json!({"op":"report_status", "outbox_id":id})),
        ControlOp::ReportAdopt(request) => {
            let mut value = json!(request);
            value["op"] = json!("report_adopt");
            format!("{value}\n")
        }
        ControlOp::ReportMutate(request) => {
            let mut value = json!(request);
            value["op"] = json!("report_mutate");
            format!("{value}\n")
        }
        ControlOp::Report(request) => {
            let mut value = json!(request);
            value["op"] = json!("report");
            format!("{value}\n")
        }
    }
}

/// Admission is durable even when Slack is unavailable; the normal outbox worker delivers it.
pub async fn report_reply(pool: &sqlx::SqlitePool, session: &str, request: &ReportRequest) -> Value {
    match queue_report(pool, session, request).await {
        Ok((decision, outbox)) => json!({"status":"ok", "op":"report",
            "decision":decision.as_str(), "outbox_id":outbox}),
        Err(error) => json!({"status":"failed", "op":"report", "error":error.to_string()}),
    }
}

/// Reads a delivery receipt without resubmitting or posting the report. `uncertain` means a
/// post may have reached Slack without a recorded outcome (a timeout, or a sender that died
/// mid-post); the hub never retries it on its own, so a producer must not resubmit it either.
pub async fn report_status_reply(pool: &sqlx::SqlitePool, id: i64) -> Value {
    type Row = (Option<String>, Option<String>, Option<String>, Option<String>,
        Option<String>, Option<String>, Option<i64>, Option<String>, i64, Option<String>);
    let row = sqlx::query_as::<_, Row>(
        "SELECT delivered_at, cancelled_at, posted_channel, posted_ts, send_uncertain_at,
                send_uncertain_error, send_inflight, send_claim_until, send_chunks_done, send_chunks_json
           FROM chat_outbox WHERE id = ?")
            .bind(id).fetch_optional(pool).await;
    match row {
        Ok(Some((delivered, cancelled, channel, ts, uncertain_at, uncertain_error, inflight,
            claim_until, chunks_done, chunks))) => {
            // A live claim is pending, not a final unknown; see report_mutation::statuses.
            let mutations = match crate::report_mutation::statuses(pool, id).await {
                Ok(rows) => rows,
                Err(error) => return json!({"status":"failed","op":"report_status","error":error.to_string()}),
            };
            // A sender that is still inside its lease is posting now; past it, nobody recorded
            // the outcome of the chunk it was posting.
            let abandoned = inflight.is_some() && claim_until.as_deref()
                .and_then(|until| chrono::DateTime::parse_from_rfc3339(until).ok())
                .is_none_or(|until| until <= chrono::Utc::now());
            let delivery = if cancelled.is_some() { "cancelled" }
                else if delivered.is_some() { "delivered" }
                else if uncertain_at.is_some() || abandoned { "uncertain" }
                else { "pending" };
            let chunks_total = chunks.as_deref()
                .and_then(|chunks| serde_json::from_str::<Vec<Value>>(chunks).ok())
                .map(|chunks| chunks.len());
            json!({"status":"ok", "op":"report_status", "outbox_id":id,
                "delivery":delivery, "channel":channel, "ts":ts,"mutations":mutations,
                "chunks_posted":chunks_done, "chunks_total":chunks_total,
                "uncertain_since":uncertain_at, "uncertain_error":uncertain_error})
        }
        Ok(None) => json!({"status":"failed", "op":"report_status", "code":"not_found", "outbox_id":id}),
        Err(error) => json!({"status":"failed", "op":"report_status", "error":error.to_string()}),
    }
}

async fn queue_report(
    pool: &sqlx::SqlitePool,
    session: &str,
    request: &ReportRequest,
) -> Result<(crate::report_gate::Decision, Option<i64>)> {
    use crate::storage::WriteTx;
    let destination = if let Some(thread) = &request.thread {
        let mut key = crate::chat::session::SessionKey::from_storage_key(session)
            .map_err(|error| HubError::message(error.to_string()))?;
        key.thread_id = Some(thread.clone());
        key.storage_key()
    } else {
        session.to_owned()
    };
    let mut tx = pool.begin_write().await?;
    let report = request.report();
    let text = match report.quote.as_deref() {
        Some(quote) => format!("{}\n\n{}", report.body, quote),
        None => report.body.clone(),
    };
    let mut result = match &request.blocks {
        Some(blocks) => crate::report_gate::queue_card_in(
            &mut tx, &destination, &report, request.blocked_reason.is_some(), &text, blocks,
        ).await?,
        None => crate::report_gate::queue_delivery_in(
            &mut tx, &destination, &report, request.blocked_reason.is_some(), &text, None,
        ).await?,
    };
    if result.0 == crate::report_gate::Decision::Duplicate {
        result.1 = sqlx::query_scalar(
            "SELECT o.id FROM chat_outbox o JOIN report_delivery_parts p ON p.outbox_id = o.id
             WHERE o.session_key = ? AND o.cancelled_at IS NULL
               AND json_extract(p.report_json, '$.source') = ?
               AND json_extract(p.report_json, '$.topic_key') = ?
               AND json_extract(p.report_json, '$.state_hash') = ?
             ORDER BY o.id DESC LIMIT 1",
        ).bind(&destination).bind(&report.source).bind(&report.topic_key).bind(&report.state_hash)
            .fetch_optional(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(result)
}

/// The control reply to `approve_rerun`. The rerun itself runs in the background and reports
/// to the owner like any remediation outcome.
pub async fn approve_rerun_reply(problem: i64) -> String {
    let reply = match crate::cron_remediate::owner_approve(problem).await {
        Ok(()) => {
            json!({"status": "ok", "op": "approve_rerun", "problem": problem, "rerun": "started"})
        }
        Err(error) => {
            json!({"status": "failed", "op": "approve_rerun", "problem": problem, "error": error.to_string()})
        }
    };
    format!("{reply}\n")
}

/// What the daemon knows about its own hub tasks. The control socket fills it in.
#[derive(Debug, Clone, Default)]
pub struct LiveHub {
    /// A generation of chat, cron, and HTTP tasks is spawned.
    pub running: bool,
    pub http_addr: String,
    /// The HTTP listener is bound right now.
    pub http_bound: bool,
    /// The daemon was built with Slack and a Slack app token is present.
    pub slack_configured: bool,
    pub slack: Option<crate::chat::slack::SocketModeStatus>,
    pub discord_configured: bool,
    pub version: String,
    /// The supervisor lease as this daemon sees it; `None` from a caller without one.
    pub lease: Option<LiveLease>,
}

/// This daemon's supervisor lease role, as reported on the wire.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LeaseRole {
    /// Runs the hub (a leased holder, a provisional offline hub, or no lease client at all).
    #[default]
    Holder,
    /// Another machine (or an unknown one) holds the lease; this hub stays off.
    Standby,
    /// The relay cannot arbitrate; the hub stays off.
    Unleased,
}

/// Why a standby would not take the supervisor over now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromotionBlock {
    pub code: String,
    pub message: String,
}

/// An open goal of this machine's archived supervisor the owner may adopt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptableGoal {
    pub goal_id: String,
    pub title: String,
    pub supervisor_id: String,
    pub from_machine: String,
}

/// A host the holder could not push its fence to (`host: None` is this machine's daemon).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnreachableHost {
    pub host: Option<String>,
    pub reason: String,
}

/// The supervisor lease fields of `status` (`supervisor_lease`) and of the snapshot's `hub`
/// object, camelCase on the wire. Ids, labels and counts only: no token or secret.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveLease {
    pub role: LeaseRole,
    pub holder_machine_id: Option<String>,
    pub holder_label: Option<String>,
    pub holder_paired: bool,
    /// The lease fence term this machine knows (0: none known, or an unleased hub).
    pub fence: u64,
    /// Seconds until this holder's grant runs out unless renewed (0: no expiring grant held).
    pub lease_expires_in: u64,
    /// Filled by the checkpoint replication code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_id: Option<String>,
    /// Filled by the checkpoint replication code (seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_age: Option<u64>,
    /// Filled by the auto-promotion code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promotion_block_reason: Option<PromotionBlock>,
    /// Filled by the archived-goal adoption code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adoptable: Option<Vec<AdoptableGoal>>,
    pub observe_dropped: u64,
    pub unmapped_hosts: Vec<String>,
    pub unreachable_hosts: Vec<UnreachableHost>,
    /// Account machines the relay has not seen speak the lease protocol (labels, or a count of
    /// unnamed ones). Each would run a supervisor of its own beside the holder's.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub machines_without_lease: Vec<String>,
    /// A standby that has known no holder for longer than the lease TTL.
    #[serde(skip)]
    pub no_holder_past_ttl: bool,
    /// Why no relay can arbitrate this enrolled machine's lease (e.g. `RELAY_MODE_OFF`); the
    /// hub holds standby (or runs provisionally) until one can.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_unavailable: Option<PromotionBlock>,
}

impl LiveLease {
    fn standby(&self) -> bool {
        self.role == LeaseRole::Standby
    }

    /// The plain-text hint `hub status` prints on standby, naming the holder.
    fn status_hint(&self) -> Option<String> {
        if !self.standby() {
            return None;
        }
        let holder = match (&self.holder_label, &self.holder_machine_id) {
            (Some(label), Some(id)) => format!("{label} ({id})"),
            (Some(label), None) => label.clone(),
            (None, Some(id)) => id.clone(),
            (None, None) => "no known machine".to_owned(),
        };
        Some(format!(
            "standby: the supervisor runs on {holder}; this machine's hub stays off while it holds the lease"
        ))
    }
}

/// Host ids an imported checkpoint could not scope to a machine (`unmapped:<id>`), sorted.
pub async fn unmapped_hosts(pool: &sqlx::SqlitePool) -> Vec<String> {
    use crate::checkpoint::{HOST_ID_TABLES, UNMAPPED_HOST_PREFIX};
    let mut hosts = std::collections::BTreeSet::new();
    for table in HOST_ID_TABLES {
        let columns: i64 =
            sqlx::query_scalar("SELECT count(*) FROM pragma_table_info(?) WHERE name = 'host_id'")
                .bind(*table)
                .fetch_one(pool)
                .await
                .unwrap_or(0);
        if columns == 0 {
            continue;
        }
        let found: Vec<String> = sqlx::query_scalar(&format!(
            "SELECT DISTINCT host_id FROM {table} WHERE substr(host_id, 1, ?) = ?"
        ))
        .bind(UNMAPPED_HOST_PREFIX.len() as i64)
        .bind(UNMAPPED_HOST_PREFIX)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
        hosts.extend(found);
    }
    hosts.into_iter().collect()
}

/// The `status` reply. Counts and states only: no tokens, message text, or watcher stderr.
/// `healthy` is false when the hub is stopped, HTTP is not bound, a configured Slack app has
/// no open socket, or an inbox watcher's last run failed; each reason is in `problems`. A
/// standby's stopped hub is healthy, unless it has known no holder for longer than the TTL.
/// `warnings` are informational and never affect `healthy` (account machines without lease
/// support: a sleeping peer or a restarted relay can list one that is fine).
/// The supervisor engine is this process's as [`crate::supervisor::setup::EngineView::read`]
/// sees it (the hub env file is read off the runtime threads); see [`status_report_with`].
pub async fn status_report(pool: &sqlx::SqlitePool, live: &LiveHub) -> Value {
    // Tests never read the developer's hub env file.
    let engine = if cfg!(test) {
        crate::supervisor::setup::EngineView::default()
    } else {
        crate::supervisor::setup::EngineView::read().await
    };
    status_report_with(pool, live, &engine).await
}

/// [`status_report`] with the supervisor engine given. Supervisor setup is a problem only once
/// the engine flag is on: the engine needs fixing (a fault, or a required capability missing),
/// no owner identity exists, or the automatic cutover is set while the unified intake is still
/// closed. An engine that is merely on its way (absent right after enabling, starting, or
/// waiting without a fault) is a normal start and leaves `healthy` alone. With the flag off
/// (fresh and legacy installs) it never affects `healthy`, nor on a standby, whose engine
/// runs on the lease holder.
pub async fn status_report_with(
    pool: &sqlx::SqlitePool,
    live: &LiveHub,
    engine: &crate::supervisor::setup::EngineView,
) -> Value {
    let mut problems = Vec::new();
    let mut warnings = Vec::new();
    let stopped_flag = is_stopped(pool).await.unwrap_or(true);
    let standby = live.lease.as_ref().is_some_and(LiveLease::standby);
    if !live.running && !standby {
        problems.push("hub tasks are not running".to_owned());
    }
    if standby && live.lease.as_ref().is_some_and(|lease| lease.no_holder_past_ttl) {
        problems.push("no supervisor holds the lease".to_owned());
    }
    if let Some(lease) = live.lease.as_ref().filter(|lease| !lease.machines_without_lease.is_empty()) {
        warnings.push(format!("machines without lease support: {}", lease.machines_without_lease.join(", ")));
    }
    if let Some(reason) = live.lease.as_ref().and_then(|lease| lease.lease_unavailable.as_ref()) {
        problems.push(format!("lease unavailable: {}", reason.message));
    }
    if live.running && !live.http_bound {
        problems.push(format!("http {} is not bound", live.http_addr));
    }
    let slack_connected = live.slack.as_ref().is_some_and(|slack| slack.connected);
    if live.running && live.slack_configured && !slack_connected {
        problems.push("slack socket mode is not connected".to_owned());
    }
    let (jobs, enabled): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(CASE WHEN enabled = 1 THEN 1 ELSE 0 END), 0) FROM scheduled_jobs",
    )
    .fetch_one(pool)
    .await
    .unwrap_or((0, 0));
    let watchers: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT source, status, updated_at FROM inbox_watcher_health ORDER BY source",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for (source, status, _) in &watchers {
        if live.running && matches!(status.as_str(), "failed" | "auth_failed") {
            problems.push(format!("inbox watcher {source} {status}"));
        }
    }
    let notify = crate::sessions::watch::effective_notify_mode(pool)
        .await
        .map(|mode| mode.as_str())
        .unwrap_or("actionable");
    let status_hint = live.lease.as_ref().and_then(LiveLease::status_hint);
    let setup = crate::supervisor::setup::setup_status(pool, engine.clone()).await.ok();
    if let Some(setup) = setup.as_ref().filter(|setup| setup.engine_enabled && !standby) {
        if setup.next_action == crate::supervisor::setup::NextAction::FixEngine {
            let why = setup.engine_fault.as_ref().map_or_else(
                || setup.engine_state.clone(),
                |fault| format!("{} ({})", setup.engine_state, fault.code),
            );
            problems.push(format!("supervisor engine is enabled but not ready: {why}"));
        }
        if setup.owner.is_none() {
            problems.push("supervisor engine is enabled but no owner identity exists".to_owned());
        }
        if setup.auto_cutover && !setup.intake.unified_open {
            problems.push("supervisor automatic cutover is set but the unified intake is still closed".to_owned());
        }
    }
    json!({
        "status": "ok",
        "op": "status",
        "version": live.version,
        "running": live.running,
        "stopped_flag": stopped_flag,
        "healthy": problems.is_empty(),
        "problems": problems,
        "warnings": warnings,
        "http": {"addr": live.http_addr, "bound": live.http_bound},
        "slack": {
            "configured": live.slack_configured,
            "running": live.slack.as_ref().is_some_and(|slack| slack.running),
            "connected": slack_connected,
            "connects": live.slack.as_ref().map_or(0, |slack| slack.connects),
            "last_error": live.slack.as_ref().and_then(|slack| slack.last_error.clone()),
        },
        "discord": {"configured": live.discord_configured},
        "cron": {"jobs": jobs, "enabled": enabled},
        "notify_mode": notify,
        "supervisor_intake": crate::supervisor::cutover::installation_intake(pool)
            .await
            .ok()
            .and_then(|intake| serde_json::to_value(intake).ok()),
        "supervisor_setup": setup.as_ref().map(|setup| json!({
            "engine_enabled": setup.engine_enabled,
            "engine_state": setup.engine_state,
            "engine_fault": setup.engine_fault.as_ref().map(|fault| fault.code.clone()),
            "identity": setup.owner.is_some(),
            "intake": if setup.intake.unified_open { "unified" } else { "legacy" },
            "auto_cutover": setup.auto_cutover,
        })),
        "inbox_watchers": watchers
            .iter()
            .map(|(source, status, updated_at)| json!({"source": source, "status": status, "updated_at": updated_at}))
            .collect::<Vec<_>>(),
        "supervisor_lease": live.lease,
        "status_hint": status_hint,
    })
}

/// Whether a control reply is `"status":"ok"`. A failed stop or start must not look like one.
pub fn reply_ok(reply: &str) -> bool {
    serde_json::from_str::<Value>(reply.trim())
        .ok()
        .and_then(|value| {
            value
                .get("status")
                .and_then(Value::as_str)
                .map(|s| s == "ok")
        })
        .unwrap_or(false)
}

/// Whether a `status` reply says the hub is healthy.
pub fn reply_healthy(reply: &str) -> bool {
    reply_ok(reply)
        && serde_json::from_str::<Value>(reply.trim())
            .ok()
            .and_then(|value| value.get("healthy").and_then(Value::as_bool))
            .unwrap_or(false)
}

/// The `status_hint` of a `status` reply, when it has one (a standby naming the holder).
pub fn reply_status_hint(reply: &str) -> Option<String> {
    serde_json::from_str::<Value>(reply.trim())
        .ok()?
        .get("status_hint")?
        .as_str()
        .map(str::to_owned)
}

/// The informational `warnings` lines of a `status` reply; empty for any other reply.
pub fn reply_warnings(reply: &str) -> Vec<String> {
    serde_json::from_str::<Value>(reply.trim())
        .ok()
        .and_then(|value| {
            value.get("warnings").and_then(Value::as_array).map(|lines| {
                lines.iter().filter_map(Value::as_str).map(str::to_owned).collect()
            })
        })
        .unwrap_or_default()
}

/// The reply to `cutover`, `rollback` or `intake`: the intake after the op and the steps it
/// committed, or the typed refusal (`MHC_CAPABILITY_MISSING`, `MHC_NOT_RUNNING`, ...).
pub fn intake_reply(
    op: &str,
    result: std::result::Result<crate::supervisor::cutover::Transition, crate::supervisor::SupervisorError>,
) -> Value {
    match result {
        Ok(transition) => json!({
            "status": "ok",
            "op": op,
            "intake": transition.intake,
            "steps": transition.steps,
        }),
        Err(error) => json!({
            "status": "failed",
            "op": op,
            "code": error.code(),
            "error": error.to_string(),
        }),
    }
}

/// Writes one control line and reads the reply. The daemon process is not signaled.
#[cfg(unix)]
pub async fn send_control(path: &std::path::Path, op: ControlOp) -> Result<String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let mut stream = tokio::net::UnixStream::connect(path)
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    stream
        .write_all(control_line(op).as_bytes())
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    let mut reader = tokio::io::BufReader::new(stream);
    let mut response = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        reader.read_line(&mut response),
    )
    .await
    .map_err(|_| HubError::message("hub control timed out"))?
    .map_err(|error| HubError::message(error.to_string()))?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_start_and_status_are_the_only_ops() {
        assert_eq!(
            parse_control("{\"op\":\"stop\"}\n").unwrap(),
            ControlOp::Stop
        );
        assert_eq!(
            parse_control("{\"op\":\"start\"}").unwrap(),
            ControlOp::Start
        );
        assert_eq!(
            parse_control("{\"op\":\"status\"}").unwrap(),
            ControlOp::Status
        );
        assert_eq!(
            parse_control(&control_line(ControlOp::Status)).unwrap(),
            ControlOp::Status
        );
        assert_eq!(
            parse_control(&control_line(ControlOp::ApproveRerun(134))).unwrap(),
            ControlOp::ApproveRerun(134)
        );
        assert!(parse_control("{\"op\":\"approve_rerun\"}").is_err());
        for op in [ControlOp::Cutover, ControlOp::Rollback, ControlOp::Intake] {
            assert_eq!(parse_control(&control_line(op.clone())).unwrap(), op);
        }
        assert!(parse_control("{\"op\":\"restart-daemon\"}").is_err());
    }

    #[tokio::test]
    async fn approve_rerun_without_a_remediator_fails_and_says_why() {
        let reply = approve_rerun_reply(7).await;
        assert!(!reply_ok(&reply), "{reply}");
        assert!(reply.contains("cron remediator not installed"), "{reply}");
    }

    #[tokio::test]
    async fn external_reports_are_queued_once_and_stale_cards_are_not_queued() {
        let pool = crate::storage::memory().await.unwrap();
        let input = json!({"op":"report", "source":"autopilot", "topic":"card:42",
            "salience":"action", "body":"새 공고를 확인해 주세요.", "state":"eligible"});
        let ControlOp::Report(mut request) = parse_control(&input.to_string()).unwrap() else {
            panic!("report request expected");
        };
        assert_eq!(parse_control(&control_line(ControlOp::Report(request.clone()))).unwrap(),
            ControlOp::Report(request.clone()));
        let first = report_reply(&pool, "owner", &request).await;
        assert_eq!(first["decision"], "sent");
        let retry = report_reply(&pool, "owner", &request).await;
        assert_eq!(retry["decision"], "duplicate");
        assert!(first["outbox_id"].as_i64().is_some());
        assert_eq!(retry["outbox_id"], first["outbox_id"]);
        assert!(report_reply(&pool, "other-owner", &request).await["outbox_id"].is_null());
        sqlx::query("UPDATE chat_outbox SET cancelled_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(first["outbox_id"].as_i64().unwrap()).execute(&pool).await.unwrap();
        let cancelled = report_reply(&pool, "owner", &request).await;
        assert_eq!(cancelled["decision"], "duplicate");
        assert!(cancelled["outbox_id"].is_null());
        request.topic = "card:43".into();
        request.blocked_reason = Some("provider returned preferred_only=true".into());
        assert_eq!(report_reply(&pool, "owner", &request).await["decision"], "stale");
        let queued: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_outbox")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(queued, 1);
    }

    #[test]
    fn external_report_boundary_rejects_missing_identity_and_unknown_salience() {
        let input = json!({"op":"report", "source":"autopilot", "topic":"card:42",
            "salience":"action", "body":"확인해 주세요.", "state":"eligible"});
        for field in ["source", "topic", "body", "state"] {
            let mut invalid = input.clone();
            invalid[field] = json!(" ");
            assert!(parse_control(&invalid.to_string()).is_err());
        }
        for blocks in [json!([]), json!(["not a block"]), json!({"type":"actions"})] {
            let mut invalid = input.clone();
            invalid["blocks"] = blocks;
            assert!(parse_control(&invalid.to_string()).is_err());
        }
        let mut invalid = input;
        invalid["salience"] = json!("urgent");
        assert!(parse_control(&invalid.to_string()).is_err());
    }

    #[tokio::test]
    async fn card_persistence_failure_rolls_back_admission() {
        // Given a valid external card and a storage failure saving its blocks.
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("CREATE TRIGGER reject_blocks BEFORE UPDATE OF blocks_json ON chat_outbox BEGIN SELECT RAISE(ABORT, 'test failure'); END")
            .execute(&pool).await.unwrap();
        let input = json!({"op":"report", "source":"autopilot", "topic":"card:42",
            "salience":"decision", "body":"지원 여부를 선택해 주세요.", "state":"eligible",
            "blocks":[{"type":"actions","elements":[]}]});
        let ControlOp::Report(request) = parse_control(&input.to_string()).unwrap() else {
            panic!("report request expected");
        };

        // When the same transaction cannot persist the interactive content.
        assert_eq!(report_reply(&pool, "owner", &request).await["status"], "failed");

        // Then no reservation or orphaned text-only message survives.
        for table in ["chat_outbox", "report_topics", "report_suppressions", "report_delivery_parts", "delivered_alerts"] {
            let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&pool).await.unwrap();
            assert_eq!(count, 0, "{table}");
        }
    }

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn external_card_retry_preserves_buttons_and_thread() {
        // Given a durably admitted card, without an initial transport attempt.
        let pool = crate::storage::memory().await.unwrap();
        let owner = crate::chat::session::SessionKey::new("slack", None::<String>, "DOWNER", None::<String>, "UOWNER");
        let blocks = json!([{"type":"actions","elements":[{"type":"button",
            "text":{"type":"plain_text","text":"Approve"},
            "action_id":"omon:btn:autopilot:apply:42", "value":"omon:btn:autopilot:apply:42"}]}]);
        let input = json!({"op":"report", "source":"autopilot", "topic":"card:42",
            "salience":"decision", "body":"지원 여부를 선택해 주세요.", "state":"eligible",
            "quote":"Project #42: Rust integration", "thread":"1700000000.1", "blocks":blocks});
        let ControlOp::Report(request) = parse_control(&input.to_string()).unwrap() else {
            panic!("report request expected");
        };
        let admitted = report_reply(&pool, &owner.storage_key(), &request).await;
        assert_eq!(admitted["decision"], "sent");
        let id = admitted["outbox_id"].as_i64().unwrap();
        let (base, calls) = crate::chat::slack_web::mock::serve("B1").await;

        let scope = crate::report_rules::scope_from_alert("autopilot", None, None, Some("card:42"));
        crate::report_rules::record(&pool, &scope, crate::report_rules::Effect::Digest, "digest", None).await.unwrap();
        crate::chat::slack::with_test_api(base, "test-token".into(), async {
            assert_eq!(crate::chat::reply::flush_pending(&pool).await.unwrap(), 0);
            assert!(calls.lock().unwrap().iter().all(|(method, _)| method != "chat.postMessage"));
            let due = (chrono::Utc::now() - chrono::Duration::minutes(3)).to_rfc3339();
            let old = (chrono::Utc::now() - chrono::Duration::days(2)).to_rfc3339();
            sqlx::query("UPDATE chat_outbox SET report_not_before=?,created_at=? WHERE id=?")
                .bind(due).bind(old).bind(id).execute(&pool).await.unwrap();
            assert_eq!(crate::chat::reply::flush_stale(&pool, crate::chat::reply::OUTBOX_SETTLE, chrono::Duration::hours(24)).await.unwrap(), 1);
        }).await;

        // Then the actual HTTP request retains the original action IDs and thread.
        let receipt = report_status_reply(&pool, id).await;
        assert_eq!(receipt["delivery"], "delivered");
        assert_eq!(receipt["channel"], "DOWNER");
        assert!(receipt["ts"].as_str().is_some_and(|ts| !ts.is_empty()));
        let calls = calls.lock().unwrap();
        let posts: Vec<_> = calls.iter().filter(|(method, _)| method == "chat.postMessage").collect();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].1["blocks"], blocks);
        assert_eq!(posts[0].1["thread_ts"], "1700000000.1");
        assert_eq!(posts[0].1["channel"], "DOWNER");
        assert_eq!(posts[0].1["text"], format!("{}\n\n{}", request.body, request.quote.as_deref().unwrap()));
    }

    #[test]
    fn report_status_requires_a_positive_outbox_identity() {
        let op = ControlOp::ReportStatus(42);
        assert_eq!(parse_control(&control_line(op.clone())).unwrap(), op);
        for id in [json!(0), json!(-1), json!(null), json!("42")] {
            assert!(parse_control(&json!({"op":"report_status", "outbox_id":id}).to_string()).is_err());
        }
    }

    #[test]
    fn report_adoption_roundtrips_without_producer_content() {
        let input = json!({"op":"report_adopt","channel":"DOWNER","ts":"1700.1",
            "source":"autopilot","topic":"autopilot:item:42"});
        let op = parse_control(&input.to_string()).unwrap();
        assert_eq!(parse_control(&control_line(op.clone())).unwrap(), op);
        let mut invalid = input;
        invalid["blocks"] = json!([]);
        assert!(parse_control(&invalid.to_string()).is_err());
    }

    #[test]
    fn report_mutation_roundtrips_and_requires_parent_identity() {
        for change in [json!({"kind":"restore_actions"}),
            json!({"kind":"action_row","row":{"type":"context","elements":[{"type":"plain_text","text":"processing"}]}})] {
            let input = json!({"op":"report_mutate","outbox_id":42,
                "mutation_id":"processing:1","change":change});
            let op = parse_control(&input.to_string()).unwrap();
            assert_eq!(parse_control(&control_line(op.clone())).unwrap(), op);
            for id in [json!(0), json!(-1), json!(null)] {
                let mut invalid = input.clone();
                invalid["outbox_id"] = id;
                assert!(parse_control(&invalid.to_string()).is_err());
            }
            let mut invalid = input;
            invalid["mutation_id"] = json!("");
            assert!(parse_control(&invalid.to_string()).is_err());
            invalid["mutation_id"] = json!("processing:1");
            invalid["change"] = json!({"kind":"action_row","row":{"type":"actions",
                "elements":[{"type":"button","action_id":"unapproved"}]}});
            assert!(parse_control(&invalid.to_string()).is_err());
        }
    }

    #[tokio::test]
    async fn queued_and_cancelled_reports_never_claim_a_delivery_receipt() {
        let pool = crate::storage::memory().await.unwrap();
        let input = json!({"op":"report", "source":"autopilot", "topic":"card:42",
            "salience":"decision", "body":"지원 여부를 선택해 주세요.", "state":"eligible"});
        let ControlOp::Report(request) = parse_control(&input.to_string()).unwrap() else {
            panic!("report request expected");
        };
        let admitted = report_reply(&pool, "owner", &request).await;
        let id = admitted["outbox_id"].as_i64().unwrap();
        let pending = report_status_reply(&pool, id).await;
        assert_eq!(pending["delivery"], "pending");
        assert!(pending["channel"].is_null());
        assert!(pending["ts"].is_null());
        sqlx::query("UPDATE chat_outbox SET cancelled_at = ? WHERE id = ?")
            .bind(crate::storage::now_rfc3339()).bind(id).execute(&pool).await.unwrap();
        let cancelled = report_status_reply(&pool, id).await;
        assert_eq!(cancelled["delivery"], "cancelled");
        assert!(cancelled["ts"].is_null());
        assert_eq!(report_status_reply(&pool, id + 1).await["code"], "not_found");
    }

    #[test]
    fn a_failed_reply_is_not_ok() {
        assert!(reply_ok("{\"status\":\"ok\",\"op\":\"start\"}\n"));
        assert!(!reply_ok(
            "{\"status\":\"failed\",\"error\":\"hub start was not persisted\"}"
        ));
        assert!(!reply_ok(""));
        assert!(!reply_ok("garbage"));
        assert!(!reply_healthy("{\"status\":\"ok\",\"healthy\":false}"));
        assert!(reply_healthy("{\"status\":\"ok\",\"healthy\":true}"));
    }

    fn live_slack(connected: bool) -> crate::chat::slack::SocketModeStatus {
        crate::chat::slack::SocketModeStatus {
            running: true,
            connected,
            connects: u64::from(connected),
            last_error: None,
        }
    }

    #[tokio::test]
    async fn status_names_each_reason_the_hub_is_not_healthy() {
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled) VALUES('a','every 1m','{}',1),('b','every 1m','{}',0)")
            .execute(&pool)
            .await
            .unwrap();
        let stopped = status_report(
            &pool,
            &LiveHub {
                http_addr: "127.0.0.1:9119".into(),
                ..LiveHub::default()
            },
        )
        .await;
        assert_eq!(stopped["healthy"], false);
        assert_eq!(stopped["running"], false);
        assert_eq!(stopped["stopped_flag"], true);
        assert_eq!(stopped["cron"]["jobs"], 2);
        assert_eq!(stopped["cron"]["enabled"], 1);
        assert_eq!(stopped["problems"][0], "hub tasks are not running");

        clear_stopped(&pool).await.unwrap();
        let mut live = LiveHub {
            running: true,
            http_addr: "127.0.0.1:9119".into(),
            http_bound: true,
            slack_configured: true,
            slack: Some(live_slack(true)),
            discord_configured: false,
            version: "test".into(),
            lease: None,
        };
        let up = status_report(&pool, &live).await;
        assert_eq!(up["healthy"], true, "{up}");
        assert_eq!(up["stopped_flag"], false);
        assert_eq!(up["slack"]["connected"], true);
        assert!(reply_healthy(&up.to_string()));

        live.slack = Some(live_slack(false));
        live.http_bound = false;
        let down = status_report(&pool, &live).await;
        assert_eq!(down["healthy"], false);
        let problems = down["problems"].to_string();
        assert!(
            problems.contains("http 127.0.0.1:9119 is not bound"),
            "{problems}"
        );
        assert!(
            problems.contains("slack socket mode is not connected"),
            "{problems}"
        );

        live.slack = Some(live_slack(true));
        live.http_bound = true;
        sqlx::query("INSERT INTO inbox_watcher_health(source, status, detail, updated_at) VALUES('imsg','failed','secret stderr','2026-10-23T00:00:00Z')")
            .execute(&pool)
            .await
            .unwrap();
        let watcher = status_report(&pool, &live).await;
        assert_eq!(watcher["healthy"], false);
        assert_eq!(watcher["problems"][0], "inbox watcher imsg failed");
        assert!(
            !watcher.to_string().contains("secret stderr"),
            "watcher stderr is not part of status"
        );
    }

    #[tokio::test]
    async fn supervisor_setup_is_a_problem_only_once_the_engine_flag_is_on() {
        use crate::meta::mhc_supervisor::{HealthState, SupervisorHealth};
        use crate::supervisor::setup::EngineView;
        let pool = crate::storage::memory().await.unwrap();
        clear_stopped(&pool).await.unwrap();
        let live = LiveHub {
            running: true,
            http_addr: "127.0.0.1:9119".into(),
            http_bound: true,
            version: "test".into(),
            ..LiveHub::default()
        };
        // A fresh or legacy hub without the engine flag stays healthy.
        let off = status_report_with(&pool, &live, &EngineView::default()).await;
        assert_eq!(off["healthy"], true, "{off}");
        assert_eq!(off["supervisor_setup"]["engine_enabled"], false);
        assert_eq!(off["supervisor_setup"]["identity"], false);

        // The flag is on but the running generation has no engine yet and no owner exists:
        // the engine is on its way, the missing owner is the problem.
        let enabled = EngineView { enabled: true, installed: false, health: None };
        let pending = status_report_with(&pool, &live, &enabled).await;
        assert_eq!(pending["healthy"], false);
        let problems = pending["problems"].to_string();
        assert!(!problems.contains("not ready"), "{problems}");
        assert!(problems.contains("no owner identity"), "{problems}");

        let health = |state: HealthState, generation: u64, drop: Option<&str>| SupervisorHealth {
            state,
            generation,
            pid: None,
            capabilities: crate::meta::mhc_profile::REQUIRED_CAPABILITIES
                .iter()
                .filter(|capability| Some(**capability) != drop)
                .map(|capability| (*capability).to_owned())
                .collect(),
            server_version: None,
            fault: None,
            detail: None,
            queued_decisions: 0,
        };
        // An engine that has not started yet, without a fault, is a normal start.
        let starting = EngineView {
            enabled: true,
            installed: true,
            health: Some(health(HealthState::Stopped, 0, None)),
        };
        let problems = status_report_with(&pool, &live, &starting).await["problems"].to_string();
        assert!(!problems.contains("not ready"), "{problems}");
        // A ready engine short of a required capability needs fixing.
        let short = EngineView {
            enabled: true,
            installed: true,
            health: Some(health(HealthState::Ready, 1, Some("memory_forget"))),
        };
        let problems = status_report_with(&pool, &live, &short).await["problems"].to_string();
        assert!(problems.contains("not ready"), "{problems}");
        assert!(problems.contains("MHC_CAPABILITY_MISSING"), "{problems}");

        // A ready engine with every capability, an owner, and auto cutover on a closed intake.
        crate::supervisor::provision_identity(&pool, "local:owner").await.unwrap();
        crate::supervisor::cutover::note_import(&pool).await.unwrap();
        crate::supervisor::setup::set_setup_flags(&pool, Some(true), None).await.unwrap();
        let ready = EngineView {
            enabled: true,
            installed: true,
            health: Some(health(HealthState::Ready, 1, None)),
        };
        let closed = status_report_with(&pool, &live, &ready).await;
        assert_eq!(closed["problems"].as_array().unwrap().len(), 1, "{closed}");
        assert!(closed["problems"][0].as_str().unwrap().contains("automatic cutover"));
        assert_eq!(closed["supervisor_setup"]["intake"], "legacy");

        crate::supervisor::cutover::cutover(&pool, ready.health.as_ref()).await.unwrap();
        let done = status_report_with(&pool, &live, &ready).await;
        assert_eq!(done["healthy"], true, "{done}");
        assert_eq!(done["supervisor_setup"]["intake"], "unified");
        assert_eq!(done["supervisor_setup"]["engine_state"], "ready");
    }

    #[tokio::test]
    async fn a_stop_stays_stopped_until_start_clears_it() {
        let pool = crate::storage::memory().await.unwrap();
        assert!(
            is_stopped(&pool).await.unwrap(),
            "a fresh hub stays down until an explicit start"
        );
        clear_stopped(&pool).await.unwrap();
        assert!(!is_stopped(&pool).await.unwrap());
        mark_stopped(&pool).await.unwrap();
        assert!(is_stopped(&pool).await.unwrap());
        clear_stopped(&pool).await.unwrap();
        assert!(!is_stopped(&pool).await.unwrap());
    }

    fn standby_lease() -> LiveLease {
        LiveLease {
            role: LeaseRole::Standby,
            holder_machine_id: Some("mach-b".into()),
            holder_label: Some("Laptop B".into()),
            holder_paired: true,
            observe_dropped: 3,
            ..LiveLease::default()
        }
    }

    #[tokio::test]
    async fn a_standby_with_a_known_holder_is_healthy_and_names_it() {
        let pool = crate::storage::memory().await.unwrap();
        let live = LiveHub {
            http_addr: "127.0.0.1:9119".into(),
            lease: Some(standby_lease()),
            ..LiveHub::default()
        };

        let report = status_report(&pool, &live).await;

        assert_eq!(report["healthy"], true, "{report}");
        assert_eq!(report["running"], false);
        let lease = &report["supervisor_lease"];
        assert_eq!(lease["role"], "standby");
        assert_eq!(lease["holderMachineId"], "mach-b");
        assert_eq!(lease["holderLabel"], "Laptop B");
        assert_eq!(lease["holderPaired"], true);
        assert_eq!(lease["observeDropped"], 3);
        assert_eq!(lease["unmappedHosts"], json!([]));
        assert_eq!(lease["unreachableHosts"], json!([]));
        assert_eq!(lease["fence"], 0);
        assert_eq!(lease["leaseExpiresIn"], 0);
        for absent in ["checkpointId", "checkpointAge", "promotionBlockReason", "adoptable"] {
            assert!(lease.get(absent).is_none(), "{absent} in {lease}");
        }
        let hint = reply_status_hint(&report.to_string()).expect("a standby hint");
        assert!(hint.contains("Laptop B (mach-b)"), "{hint}");
        assert!(reply_healthy(&report.to_string()));
    }

    #[tokio::test]
    async fn a_standby_without_a_holder_past_the_ttl_is_unhealthy() {
        let pool = crate::storage::memory().await.unwrap();
        let live = LiveHub {
            lease: Some(LiveLease {
                holder_machine_id: None,
                holder_label: None,
                no_holder_past_ttl: true,
                ..standby_lease()
            }),
            ..LiveHub::default()
        };

        let report = status_report(&pool, &live).await;

        assert_eq!(report["healthy"], false);
        assert_eq!(report["problems"], json!(["no supervisor holds the lease"]));
        assert!(report["status_hint"].as_str().unwrap().contains("no known machine"));
    }

    /// Account machines without lease support are a warning, never a problem: the line is in
    /// `warnings` and the lease object while the status stays healthy (so `--check` passes);
    /// none leaves the field off the wire.
    #[tokio::test]
    async fn machines_without_lease_support_are_a_warning_that_keeps_status_healthy() {
        let pool = crate::storage::memory().await.unwrap();
        let live = LiveHub {
            lease: Some(LiveLease {
                machines_without_lease: vec!["Desk".into(), "1 unpaired machine".into()],
                ..standby_lease()
            }),
            ..LiveHub::default()
        };

        let report = status_report(&pool, &live).await;

        assert_eq!(report["healthy"], true, "{report}");
        assert_eq!(report["problems"], json!([]));
        assert_eq!(report["warnings"], json!(["machines without lease support: Desk, 1 unpaired machine"]));
        assert_eq!(reply_warnings(&report.to_string()), ["machines without lease support: Desk, 1 unpaired machine"]);
        assert!(reply_healthy(&report.to_string()), "a warning never fails --check");
        assert_eq!(report["supervisor_lease"]["machinesWithoutLease"], json!(["Desk", "1 unpaired machine"]));

        let clean = status_report(&pool, &LiveHub { lease: Some(standby_lease()), ..LiveHub::default() }).await;
        assert_eq!(clean["healthy"], true, "{clean}");
        assert_eq!(clean["warnings"], json!([]));
        assert!(clean["supervisor_lease"].get("machinesWithoutLease").is_none());
    }

    #[tokio::test]
    async fn an_enrolled_machine_without_relay_mode_reports_the_lease_unavailable() {
        let pool = crate::storage::memory().await.unwrap();
        let live = LiveHub {
            lease: Some(LiveLease {
                holder_machine_id: None,
                holder_label: None,
                lease_unavailable: Some(PromotionBlock { code: "RELAY_MODE_OFF".into(), message: "relay mode off".into() }),
                ..standby_lease()
            }),
            ..LiveHub::default()
        };

        let report = status_report(&pool, &live).await;

        assert_eq!(report["healthy"], false);
        assert_eq!(report["problems"], json!(["lease unavailable: relay mode off"]));
        assert_eq!(report["supervisor_lease"]["leaseUnavailable"]["code"], "RELAY_MODE_OFF");
    }

    #[tokio::test]
    async fn a_holder_reports_its_lease_and_no_hint() {
        let pool = crate::storage::memory().await.unwrap();
        let live = LiveHub {
            lease: Some(LiveLease {
                role: LeaseRole::Holder,
                fence: 7,
                lease_expires_in: 90,
                unreachable_hosts: vec![UnreachableHost { host: Some("machine:mach-c".into()), reason: "timed out".into() }],
                ..LiveLease::default()
            }),
            ..LiveHub::default()
        };

        let report = status_report(&pool, &live).await;

        assert_eq!(report["problems"][0], "hub tasks are not running", "a stopped holder stays unhealthy");
        let lease = &report["supervisor_lease"];
        assert_eq!(lease["role"], "holder");
        assert_eq!(lease["fence"], 7);
        assert_eq!(lease["leaseExpiresIn"], 90);
        assert_eq!(lease["unreachableHosts"], json!([{"host": "machine:mach-c", "reason": "timed out"}]));
        assert!(report["status_hint"].is_null());
        assert!(reply_status_hint(&report.to_string()).is_none());
    }

    #[tokio::test]
    async fn unmapped_hosts_lists_each_unscoped_host_once() {
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO fleet_inventory_cache(host_id, sessions, updated_at) VALUES('unmapped:stranger','[]','t'),('machine:mach-a','[]','t')")
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(unmapped_hosts(&pool).await, ["unmapped:stranger"]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stop_reaches_the_socket_and_the_reply_names_the_op() {
        let dir = std::env::temp_dir().join(format!(
            "ferryx-control-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control.sock");
        let listener = crate::schedule::bind_native_socket(&path).await.unwrap();
        let path_for_client = path.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = tokio::io::BufReader::new(stream);
            let mut line = String::new();
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
            reader.read_line(&mut line).await.unwrap();
            let op = parse_control(&line).unwrap();
            let mut stream = reader.into_inner();
            stream
                .write_all(format!("{{\"status\":\"ok\",\"op\":\"{:?}\"}}\n", op).as_bytes())
                .await
                .unwrap();
        });
        let reply = send_control(&path_for_client, ControlOp::Stop)
            .await
            .unwrap();
        server.await.unwrap();
        assert!(reply.contains("Stop"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_pause_flag_defaults_off_and_persists_each_change() {
        let pool = crate::storage::memory().await.expect("hub db");
        assert!(!is_paused(&pool).await.unwrap(), "a fresh hub is not paused");
        set_paused(&pool, true).await.unwrap();
        assert!(is_paused(&pool).await.unwrap());
        set_paused(&pool, false).await.unwrap();
        assert!(!is_paused(&pool).await.unwrap());
    }
}
