//! The supervisor lease, as the hub's side effects see it. The daemon installs one predicate
//! (holder now, re-checked against both clocks); every chat send and every goal settle or
//! complete write asks it immediately before the side effect and leaves the item queued or
//! held when it says no. Nothing installed is the unleased hub: every call passes.
use std::sync::{Arc, RwLock};

/// Whether this daemon may act as the supervisor right now.
pub type HolderGate = Arc<dyn Fn() -> bool + Send + Sync>;

/// Refusal code of a side effect a standby was asked to perform.
pub const SUPERVISOR_STANDBY: &str = "SUPERVISOR_STANDBY";

static GATE: RwLock<Option<HolderGate>> = RwLock::new(None);

/// Set in a process that learned it runs beside a standby daemon (`ferryx-cli mcp`): every
/// supervisor read or write it would serve from the hub database is refused with this.
static STANDBY: RwLock<Option<crate::supervisor::request::RequestFailure>> = RwLock::new(None);

tokio::task_local! {
    static SCOPED: HolderGate;
    static SCOPED_STANDBY: Option<crate::supervisor::request::RequestFailure>;
}

/// Installs (or with `None` removes) the daemon's holder predicate.
pub fn install(gate: Option<HolderGate>) {
    *GATE.write().unwrap_or_else(|e| e.into_inner()) = gate;
}

/// Marks this process as serving beside a standby (or clears it with `None`).
pub fn set_standby(refusal: Option<crate::supervisor::request::RequestFailure>) {
    *STANDBY.write().unwrap_or_else(|e| e.into_inner()) = refusal;
}

/// The SUPERVISOR_STANDBY refusal this process answers supervisor state with, if any.
pub fn standby_refusal() -> Option<crate::supervisor::request::RequestFailure> {
    if let Ok(scoped) = SCOPED_STANDBY.try_with(Clone::clone) {
        return scoped;
    }
    STANDBY.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Runs `future` with `refusal` as this task's standby state only (tests).
pub async fn scope_standby<F: std::future::Future>(
    refusal: Option<crate::supervisor::request::RequestFailure>,
    future: F,
) -> F::Output {
    SCOPED_STANDBY.scope(refusal, future).await
}

/// True when this daemon holds the lease now (or runs unleased). A scoped gate
/// ([`scope`]) wins over the installed one.
pub fn is_holder_now() -> bool {
    if let Ok(holder) = SCOPED.try_with(|gate| gate()) {
        return holder;
    }
    let gate = GATE.read().unwrap_or_else(|e| e.into_inner()).clone();
    gate.is_none_or(|gate| gate())
}

/// Runs `future` with `gate` as the holder predicate of this task only.
pub async fn scope<F: std::future::Future>(gate: HolderGate, future: F) -> F::Output {
    SCOPED.scope(gate, future).await
}

/// Runs `future` once `wait` has passed on the local monotonic clock. After a takeover the
/// new holder waits out the previous holder's lease deadline (+5s) before it opens Slack, so
/// two Socket Mode sockets never overlap.
pub async fn after_takeover<F: std::future::Future>(
    wait: std::time::Duration,
    future: F,
) -> F::Output {
    if !wait.is_zero() {
        tracing::info!(wait_secs = wait.as_secs(), "waiting out the previous supervisor's lease before opening Slack");
        tokio::time::sleep(wait).await;
    }
    future.await
}

/// The refusal a standby answers a supervisor request or read with (409), naming the holder.
/// Adapters that carry details add `{holderMachineId, holderLabel}` from the same values.
pub fn standby_failure(
    holder_machine_id: Option<&str>,
    holder_label: Option<&str>,
) -> crate::supervisor::request::RequestFailure {
    let holder = holder_label.or(holder_machine_id).unwrap_or("another machine");
    let mut failure = crate::supervisor::request::RequestFailure::new(
        SUPERVISOR_STANDBY,
        format!("{holder} runs the supervisor; this machine is on standby"),
    );
    failure.holder = Some((holder_machine_id.map(str::to_owned), holder_label.map(str::to_owned)));
    failure
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_scoped_gate_wins_and_nothing_installed_passes() {
        assert!(is_holder_now(), "an unleased hub acts");
        let standby: HolderGate = Arc::new(|| false);
        assert!(!scope(standby, async { is_holder_now() }).await);
    }

    #[test]
    fn a_standby_refusal_is_409_and_names_the_holder() {
        let refused = standby_failure(Some("mach-b"), Some("Laptop B"));
        assert_eq!(refused.code, SUPERVISOR_STANDBY);
        assert_eq!(refused.status, 409);
        assert_eq!(refused.holder, Some((Some("mach-b".into()), Some("Laptop B".into()))));
        assert!(refused.message.contains("Laptop B"), "{}", refused.message);
    }

    #[tokio::test(start_paused = true)]
    async fn a_takeover_runs_nothing_before_the_old_deadline_passed() {
        let started = tokio::time::Instant::now();
        let wait = std::time::Duration::from_secs(125);
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        let task = tokio::spawn(after_takeover(wait, async move {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            tokio::time::Instant::now()
        }));
        tokio::time::sleep_until(started + wait - std::time::Duration::from_secs(1)).await;
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst), "opened before the old deadline");
        let opened_at = task.await.unwrap();
        assert!(opened_at - started >= wait, "opened after {:?}", opened_at - started);
    }
}
