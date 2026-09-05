//! Adapter over the real owner session router. Launch allocation is supplied by
//! the daemon's workspace/task runner, not fabricated from a desktop leaf ID.
use super::{service::ControlBackend, *};
use crate::remote::RemoteSessionBackend;
use futures_util::future::BoxFuture;
use std::sync::Arc;

pub trait TaskLauncher: Send + Sync {
    fn allocate(&self, params: Value) -> BoxFuture<'_, Result<Agent>>;
    fn start(&self, target: TargetRef, params: Value) -> BoxFuture<'_, Result<Value>>;
    fn stop(&self, target: TargetRef) -> BoxFuture<'_, Result<Value>>;
}
pub struct LocalBackend {
    pub sessions: Arc<dyn RemoteSessionBackend>,
    pub launcher: Arc<dyn TaskLauncher>,
}
impl ControlBackend for LocalBackend {
    fn create(&self, params: Value) -> BoxFuture<'_, Result<Agent>> { self.launcher.allocate(params) }
    fn execute(&self, target: TargetRef, op: String, params: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            match op.as_str() {
                "start" => self.launcher.start(target, params).await,
                "stop" => self.launcher.stop(target).await,
                "prompt" => {
                    let text = params.get("text").and_then(Value::as_str).ok_or_else(||error(ScopeErrorCode::InvalidRequest,"text is required"))?;
                    if text.len() > 64 * 1024 { return Err(error(ScopeErrorCode::PayloadTooLarge,"prompt exceeds 64KiB")); }
                    let details = self.sessions.describe_session(&target.backend_session_id).await.map_err(|_|error(ScopeErrorCode::TargetExpired,"session no longer available"))?;
                    if !details.running { return Err(error(ScopeErrorCode::TargetExpired,"session exited")); }
                    self.sessions.write_input(&target.backend_session_id,text.as_bytes()).await.map_err(|_|error(ScopeErrorCode::TargetExpired,"input delivery failed; do not retry with a new request ID"))?;
                    Ok(serde_json::json!({"target":target,"stage":"accepted","bytes":text.len()}))
                }
                _ => Err(error(ScopeErrorCode::Unsupported,"operation not supported by local terminal adapter")),
            }
        })
    }
    fn read(&self, target: TargetRef, limit: usize) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let attachment = self.sessions.attach_with_sequence(&target.backend_session_id,None).await.map_err(|_|error(ScopeErrorCode::TargetExpired,"session unavailable"))?;
            let text = String::from_utf8_lossy(&attachment.snapshot.history);
            let lines: Vec<_> = text.lines().rev().take(limit).collect();
            Ok(serde_json::json!({"kind":"terminal","lines":lines.into_iter().rev().collect::<Vec<_>>(),"afterSequence":attachment.snapshot.history_end_sequence,"gap":attachment.snapshot.gap}))
        })
    }
}
