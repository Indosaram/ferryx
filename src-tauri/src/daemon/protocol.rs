use crate::session::PersistedWorkspaceSession;
use crate::terminal::TerminalSignal;
use crate::worktree::WorktreeIdentity;
use serde::{Deserialize, Serialize};

pub const DAEMON_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DaemonRequest {
    Handshake { version: u32 },
    Ping,
    Spawn {
        workspace_id: String,
        worktree: Option<WorktreeIdentity>,
        cols: u16,
        rows: u16,
    },
    Write {
        session_id: String,
        data: Vec<u8>,
    },
    Resize {
        session_id: String,
        cols: u16,
        rows: u16,
    },
    Signal {
        session_id: String,
        signal: TerminalSignal,
    },
    Close {
        session_id: String,
    },
    ListSessions,
    Attach {
        session_id: String,
    },
    SaveSession {
        session: PersistedWorkspaceSession,
    },
    LoadSession,
    ClearSession,
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DaemonResponse {
    HandshakeOk { version: u32, pid: u32 },
    Pong,
    SpawnOk { session_id: String },
    WriteOk,
    ResizeOk,
    SignalOk,
    CloseOk,
    ListSessionsOk { sessions: Vec<String> },
    AttachOk { history: Vec<u8> },
    SaveSessionOk,
    LoadSessionOk { session: Option<PersistedWorkspaceSession> },
    ClearSessionOk,
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DaemonStreamMessage {
    Output { session_id: String, data: Vec<u8> },
    Lagged { session_id: String, history: Vec<u8> },
    Exit { session_id: String, exit_code: Option<i32> },
}
