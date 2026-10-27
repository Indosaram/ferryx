use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{HubError, Result};
use crate::goals::types::SendOutcome;

/// One agent-control operation. The daemon adapter turns this into `AgentControlOp`.
///
/// Mutations carry the caller's request id through unchanged. The hub never mints a
/// replacement id after a dropped answer, so an outcome-unknown reply is not retried.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum AgentOp {
    List,
    Read {
        session_id: String,
    },
    Send {
        session_id: String,
        text: String,
        request_id: String,
    },
    Answer {
        session_id: String,
        prompt_id: String,
        screen_revision: String,
        answer: Value,
        request_id: String,
    },
    Stop {
        session_id: String,
        request_id: String,
    },
    Wait {
        session_id: String,
        until: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after_revision: Option<String>,
        #[serde(default, skip_serializing_if = "is_zero")]
        after: u64,
        timeout_ms: u64,
    },
    Events {
        #[serde(default, skip_serializing_if = "is_zero")]
        after: u64,
    },
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl AgentOp {
    pub fn is_mutation(&self) -> bool {
        matches!(
            self,
            Self::Send { .. } | Self::Answer { .. } | Self::Stop { .. }
        )
    }
}

/// Backend the hub uses for every host, including this machine (`host_id` none or `"local"`).
#[async_trait]
pub trait AgentControl: Send + Sync {
    async fn execute(&self, host_id: Option<&str>, op: AgentOp) -> Result<Value>;

    /// Sends `text` once and reports whether the turn started. Never retries.
    ///
    /// This default cannot observe the pane, so an accepted send counts as `Started`. It
    /// serves fakes; the daemon bridge overrides it with a turn-start observation.
    async fn send_confirmed(
        &self,
        host_id: Option<&str>,
        session_id: &str,
        text: &str,
        request_id: &str,
    ) -> SendOutcome {
        let op = AgentOp::Send {
            session_id: session_id.to_owned(),
            text: text.to_owned(),
            request_id: request_id.to_owned(),
        };
        match self.execute(host_id, op).await {
            Ok(_) => SendOutcome::Started,
            Err(error) => send_error_outcome(&error.to_string()),
        }
    }
}

/// Wire code of an unknown send outcome: the bytes may or may not have reached the pane.
pub const OPERATION_OUTCOME_UNKNOWN: &str = "OPERATION_OUTCOME_UNKNOWN";

/// Maps a failed send's error text (`"{CODE} {message}"` from the daemon bridge) to an
/// outcome. An unknown outcome is uncertain; every other failure typed nothing.
pub fn send_error_outcome(message: &str) -> SendOutcome {
    if message.contains(OPERATION_OUTCOME_UNKNOWN) {
        return SendOutcome::Uncertain {
            code: OPERATION_OUTCOME_UNKNOWN.to_owned(),
        };
    }
    let first = message.split_whitespace().next().unwrap_or("");
    let is_code = !first.is_empty()
        && first
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_');
    SendOutcome::Refused {
        code: if is_code {
            first.to_owned()
        } else {
            "SEND_FAILED".to_owned()
        },
    }
}

/// In-memory backend for hub tests and for a host the fleet should treat as failed or hung.
#[derive(Debug)]
pub struct RecordingControl {
    pub ops: Mutex<Vec<(Option<String>, AgentOp)>>,
    pub result: Mutex<Value>,
    pub hang: AtomicBool,
    pub fail_hosts: Mutex<HashSet<String>>,
}

impl Default for RecordingControl {
    fn default() -> Self {
        Self {
            ops: Mutex::new(Vec::new()),
            result: Mutex::new(json!({"ok": true})),
            hang: AtomicBool::new(false),
            fail_hosts: Mutex::new(HashSet::new()),
        }
    }
}

impl RecordingControl {
    pub fn ops(&self) -> Vec<(Option<String>, AgentOp)> {
        self.ops.lock().expect("ops").clone()
    }
}

#[async_trait]
impl AgentControl for RecordingControl {
    async fn execute(&self, host_id: Option<&str>, op: AgentOp) -> Result<Value> {
        if self.hang.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        if let Some(host) = host_id {
            if self.fail_hosts.lock().expect("fail").contains(host) {
                return Err(HubError::message(format!("host {host} failed")));
            }
        }
        self.ops
            .lock()
            .expect("ops")
            .push((host_id.map(str::to_owned), op));
        Ok(self.result.lock().expect("result").clone())
    }
}

/// Launches a pane and returns its daemon session id. The lane runner uses this
/// instead of the gateway's SSH/tmux runner.
#[async_trait]
pub trait TaskLauncher: Send + Sync {
    async fn launch_pane(&self, cwd: &str, command: &[String]) -> Result<String>;
}

#[derive(Debug, Default)]
pub struct FixedLauncher {
    pub session_id: Mutex<String>,
    pub launched: Mutex<Vec<(String, Vec<String>)>>,
}

impl FixedLauncher {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: Mutex::new(session_id.into()),
            launched: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl TaskLauncher for FixedLauncher {
    async fn launch_pane(&self, cwd: &str, command: &[String]) -> Result<String> {
        self.launched
            .lock()
            .expect("launched")
            .push((cwd.to_owned(), command.to_vec()));
        Ok(self.session_id.lock().expect("session").clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backend whose every operation fails with one error text.
    struct FailingControl(&'static str);

    #[async_trait]
    impl AgentControl for FailingControl {
        async fn execute(&self, _host_id: Option<&str>, _op: AgentOp) -> Result<Value> {
            Err(HubError::message(self.0))
        }
    }

    #[tokio::test]
    async fn the_default_confirmed_send_maps_an_accepted_send_to_started() {
        let control = RecordingControl::default();
        let outcome = control
            .send_confirmed(None, "pane-1", "next step", "hub-goal-lane-a-1")
            .await;
        assert_eq!(outcome, SendOutcome::Started);
        assert_eq!(
            control.ops(),
            vec![(
                None,
                AgentOp::Send {
                    session_id: "pane-1".into(),
                    text: "next step".into(),
                    request_id: "hub-goal-lane-a-1".into(),
                }
            )]
        );
    }

    #[tokio::test]
    async fn the_default_confirmed_send_maps_failures_without_retrying() {
        let unknown = FailingControl("OPERATION_OUTCOME_UNKNOWN the writer did not answer");
        assert_eq!(
            unknown
                .send_confirmed(None, "pane-1", "x", "hub-goal-a-1")
                .await,
            SendOutcome::Uncertain {
                code: OPERATION_OUTCOME_UNKNOWN.into()
            }
        );
        let typing = FailingControl("CONTROL_CONFLICT a person typed in this pane");
        assert_eq!(
            typing
                .send_confirmed(None, "pane-1", "x", "hub-goal-a-1")
                .await,
            SendOutcome::Refused {
                code: "CONTROL_CONFLICT".into()
            }
        );
        let plain = FailingControl("host remote failed");
        assert_eq!(
            plain
                .send_confirmed(None, "pane-1", "x", "hub-goal-a-1")
                .await,
            SendOutcome::Refused {
                code: "SEND_FAILED".into()
            }
        );
    }
}
