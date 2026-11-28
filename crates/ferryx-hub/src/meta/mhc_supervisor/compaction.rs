//! Admission recovery on the same engine generation and subscribed ingest session.
use super::*;

const WAIT: Duration = Duration::from_secs(10 * 60);

pub(super) fn refused(error: &RequestError) -> bool {
    matches!(error, RequestError::Refused { error, .. }
        if error.contains("while compaction is in progress"))
}

/// The receiver was installed before prompt submission. Its routing handle belongs to this
/// Engine, never a successor generation; transport closure ends the wait rather than relaunching.
pub(super) async fn wait(
    engine: &Engine,
    handle: &str,
    events: &mut mpsc::UnboundedReceiver<Value>,
    request_timeout: Duration,
) -> Result<(), SupervisorFault> {
    let observe = |event: &Value| {
        if event.get("sessionId").and_then(Value::as_str) != Some(handle) {
            return None;
        }
        match event.get("type").and_then(Value::as_str) {
        Some("compaction_end") => Some(Ok(())),
        Some("session_closed") => Some(Err(SupervisorFault::session_closed(
            event.get("reason").and_then(Value::as_str).unwrap_or("unspecified"),
        ))),
        _ => None,
        }
    };
    while let Ok(event) = events.try_recv() {
        if let Some(result) = observe(&event) {
            return result;
        }
    }
    let waiting = async {
        let state = engine.client.request(
            json!({"type": "get_state", "sessionId": handle}), request_timeout,
        ).await.map_err(|error| request_fault("get_state", error))?;
        if state.get("isCompacting").and_then(Value::as_bool) == Some(false) {
            return Ok(());
        }
        while let Some(event) = events.recv().await {
            if let Some(result) = observe(&event) {
                return result;
            }
        }
        Err(SupervisorFault::engine_exited("engine exited during memory compaction wait"))
    };
    tokio::time::timeout(WAIT, waiting).await
        .unwrap_or_else(|_| Err(SupervisorFault::timeout("owner memory compaction")))
}

impl Inner {
    /// Import callers must see usage in SQLite before they admit the next item. A failed
    /// meter write stops admission instead of silently opening the spend breaker.
    pub(super) async fn meter_import(&self, call: crate::usage::ModelCall) -> Result<(), SupervisorFault> {
        let pool = self.usage.read().unwrap_or_else(PoisonError::into_inner).clone();
        match pool {
            Some(pool) => crate::usage::record_call(&pool, &call).await.map_err(|error| {
                tracing::warn!(%error, "memory import usage was not persisted; admission stops");
                SupervisorFault::budget("memory import usage was not persisted")
            }),
            None => {
                self.meter(call);
                Ok(())
            }
        }
    }
}
