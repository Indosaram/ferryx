use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTerminalSession {
    pub session_id: String,
    pub title: Option<String>,
    pub project_id: Option<String>,
    pub worktree_label: Option<String>,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClientControlMessage {
    Resize { cols: u16, rows: u16 },
    Signal { signal: String },
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ServerControlMessage {
    Pong,
    Error { message: String },
}
