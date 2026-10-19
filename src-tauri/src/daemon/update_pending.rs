//! Pending daemon update (todo 9): a GUI never ends a live daemon owner of another data protocol.
//!
//! The data protocol is admitted only at an equal DAEMON_PROTOCOL_VERSION. When the running owner
//! speaks another version, the GUI talks to it only over the frozen control channel (control-v1):
//! it reads Inventory, asks it once to RetireWhenIdle, and reports a typed DAEMON_UPDATE_PENDING
//! error carrying { runningVersion, liveSessions }. The old owner then exits by itself (retirement
//! marker + exit 0, so its watchdog stops) when its last terminal closes, and the GUI's ordinary
//! connect loop admits the newest generation once the owner is Absent. Ending the terminals early
//! is the user's explicit choice: Shutdown { confirm_live_sessions: n } with the n they confirmed,
//! which the daemon executes only if its exact live count is still n. An unknown count is never
//! consent, so without a control endpoint or Inventory nothing can be shut down.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde_json::json;

use crate::daemon::control::{
    self, ControlConnection, ControlReply, ControlRequest, CODE_COUNT_CHANGED,
};
use crate::ipc::{IpcError, IpcErrorCode};

/// What the GUI knows about a running owner it may not attach to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePending {
    /// The running daemon's version, from the data handshake or the control Hello.
    pub running_version: Option<String>,
    /// Live sessions from control Inventory; None when the control channel is missing or
    /// Inventory failed. None is never treated as zero.
    pub live_sessions: Option<u32>,
    /// Whether RetireWhenIdle has been accepted by the old owner during this GUI run.
    pub retire_requested: bool,
}

impl UpdatePending {
    pub fn into_error(self) -> IpcError {
        IpcError::new(
            IpcErrorCode::DaemonUpdatePending,
            "Update installed; terminals are still running on the previous version",
        )
        .with_details(json!({
            "runningVersion": self.running_version,
            "liveSessions": self.live_sessions,
            "retireRequested": self.retire_requested,
        }))
    }
}

/// Runtime dirs whose old owner already accepted RetireWhenIdle during this GUI run.
fn retire_sent() -> &'static Mutex<HashSet<PathBuf>> {
    static SENT: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    SENT.get_or_init(|| Mutex::new(HashSet::new()))
}

fn retire_already_sent(runtime_dir: &Path) -> bool {
    retire_sent()
        .lock()
        .map(|sent| sent.contains(runtime_dir))
        .unwrap_or(false)
}

fn mark_retire_sent(runtime_dir: &Path) {
    if let Ok(mut sent) = retire_sent().lock() {
        sent.insert(runtime_dir.to_path_buf());
    }
}

/// Enters the pending state for the owner whose control endpoint lives in runtime_dir: reads
/// its version (Hello, only when the data handshake did not carry one) and live count
/// (Inventory), and sends RetireWhenIdle once per runtime dir per GUI run. Sends nothing else.
pub async fn enter(runtime_dir: &Path, running_version: Option<String>) -> UpdatePending {
    let mut pending = UpdatePending {
        running_version,
        live_sessions: None,
        retire_requested: retire_already_sent(runtime_dir),
    };
    let mut client = match control::connect(runtime_dir).await {
        Ok(ControlConnection::Connected(client)) => client,
        Ok(ControlConnection::Missing) => {
            tracing::warn!("Running daemon has no control endpoint; it keeps running until its terminals close");
            return pending;
        }
        Err(error) => {
            tracing::warn!(%error, "Control channel unreachable; the running daemon is left alone");
            return pending;
        }
    };
    if pending.running_version.is_none() {
        if let Ok(ControlReply::Hello(hello)) = client.call(&ControlRequest::Hello).await {
            pending.running_version = Some(hello.daemon_version);
        }
    }
    pending.live_sessions = match client.call(&ControlRequest::Inventory).await {
        Ok(ControlReply::Inventory(inventory)) => Some(inventory.total()),
        Ok(other) => {
            tracing::warn!(?other, "Control Inventory refused; live sessions unknown");
            None
        }
        Err(error) => {
            tracing::warn!(%error, "Control Inventory failed; live sessions unknown");
            None
        }
    };
    if !pending.retire_requested {
        match client.call(&ControlRequest::RetireWhenIdle).await {
            Ok(ControlReply::Live { .. }) => {
                mark_retire_sent(runtime_dir);
                pending.retire_requested = true;
            }
            Ok(other) => tracing::warn!(?other, "RetireWhenIdle refused by the running daemon"),
            Err(error) => tracing::warn!(%error, "RetireWhenIdle could not be sent"),
        }
    }
    pending
}

/// Result of the user's "Restart terminals now" after confirming confirm_live_sessions.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum RestartOutcome {
    /// The daemon accepted Shutdown for exactly this many sessions and exits by itself.
    Accepted { live: u32 },
    /// The live count moved since the user confirmed; nothing exited. The UI re-reads.
    CountChanged { live: Option<u32> },
    /// The daemon refused with this control code; nothing exited.
    Refused { code: String },
    /// No control endpoint: the previous version cannot be managed from this version.
    Unavailable,
}

/// Sends Shutdown { confirm_live_sessions } once. The daemon decides under its own lock whether
/// the count still matches; this side never retries and never escalates.
pub async fn restart_terminals(runtime_dir: &Path, confirm_live_sessions: u32) -> Result<RestartOutcome, String> {
    let mut client = match control::connect(runtime_dir).await {
        Ok(ControlConnection::Connected(client)) => client,
        Ok(ControlConnection::Missing) => return Ok(RestartOutcome::Unavailable),
        Err(error) => return Err(format!("control connect failed: {error}")),
    };
    match client
        .call(&ControlRequest::Shutdown {
            confirm_live_sessions,
        })
        .await
    {
        Ok(ControlReply::Live { live }) => Ok(RestartOutcome::Accepted { live }),
        Ok(ControlReply::Failed(failure)) if failure.code == CODE_COUNT_CHANGED => {
            Ok(RestartOutcome::CountChanged { live: failure.live })
        }
        Ok(ControlReply::Failed(failure)) => Ok(RestartOutcome::Refused { code: failure.code }),
        Ok(other) => Err(format!("unexpected Shutdown reply: {other:?}")),
        Err(error) => Err(format!("Shutdown failed: {error}")),
    }
}

#[cfg(test)]
#[path = "update_pending_tests.rs"]
mod tests;

