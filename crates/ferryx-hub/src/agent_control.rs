use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{HubError, Result};

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
