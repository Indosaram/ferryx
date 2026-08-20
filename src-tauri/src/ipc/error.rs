use crate::terminal::PtyError;
use crate::worktree::WorktreeError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IpcErrorCode {
    DirtyWorktree,
    WriterAlreadyActive,
    WriterLeaseOwnerMismatch,
    UnmergedBranch,
    WorkspaceNotFound,
    WorkspaceAlreadyRegistered,
    WorktreeNotFound,
    WorktreeAlreadyExists,
    PathOutsideWorkspace,
    InvalidPath,
    InvalidNamespace,
    InvalidRepoRoot,
    GitError,
    IoError,
    ParseError,
    SessionNotFound,
    PtyCreationError,
    PtySpawnError,
    PtyIoError,
    PtyResizeError,
    PtyKillError,
    ChannelError,
    InternalError,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcError {
    pub code: IpcErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl IpcError {
    pub fn new(code: IpcErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(IpcErrorCode::InternalError, message)
    }
}

impl From<WorktreeError> for IpcError {
    fn from(error: WorktreeError) -> Self {
        let message = error.to_string();
        match error {
            WorktreeError::DirtyWorktree {
                path,
                count,
                files,
            } => Self::new(IpcErrorCode::DirtyWorktree, message).with_details(json!({
                "path": path,
                "count": count,
                "files": files,
            })),
            WorktreeError::WriterAlreadyActive { path, owner_id } => {
                Self::new(IpcErrorCode::WriterAlreadyActive, message).with_details(json!({
                    "path": path,
                    "ownerId": owner_id,
                }))
            }
            WorktreeError::WriterLeaseOwnerMismatch {
                path,
                owner_id,
                requested_owner_id,
            } => Self::new(IpcErrorCode::WriterLeaseOwnerMismatch, message).with_details(json!({
                "path": path,
                "ownerId": owner_id,
                "requestedOwnerId": requested_owner_id,
            })),
            WorktreeError::UnmergedBranch { branch, head } => {
                Self::new(IpcErrorCode::UnmergedBranch, message).with_details(json!({
                    "branch": branch,
                    "head": head,
                }))
            }
            WorktreeError::WorkspaceNotFound { workspace_id } => {
                Self::new(IpcErrorCode::WorkspaceNotFound, message).with_details(json!({
                    "workspaceId": workspace_id,
                }))
            }
            WorktreeError::WorkspaceAlreadyRegistered { workspace_id } => {
                Self::new(IpcErrorCode::WorkspaceAlreadyRegistered, message).with_details(json!({
                    "workspaceId": workspace_id,
                }))
            }
            WorktreeError::WorktreeIdentityNotFound {
                workspace_id,
                ws_id,
                slug,
            } => Self::new(IpcErrorCode::WorktreeNotFound, message).with_details(json!({
                "workspaceId": workspace_id,
                "worktree": { "wsId": ws_id, "slug": slug },
            })),
            WorktreeError::PathOutsideWorkspace { path, root } => {
                Self::new(IpcErrorCode::PathOutsideWorkspace, message).with_details(json!({
                    "path": path,
                    "root": root,
                }))
            }
            WorktreeError::InvalidPath { path, reason } => {
                Self::new(IpcErrorCode::InvalidPath, message).with_details(json!({
                    "path": path,
                    "reason": reason,
                }))
            }
            WorktreeError::WorktreeNotFound { path } => {
                Self::new(IpcErrorCode::WorktreeNotFound, message)
                    .with_details(json!({ "path": path }))
            }
            WorktreeError::WorktreeAlreadyExists { path } => {
                Self::new(IpcErrorCode::WorktreeAlreadyExists, message)
                    .with_details(json!({ "path": path }))
            }
            WorktreeError::BranchAlreadyExists { branch } => {
                Self::new(IpcErrorCode::WorktreeAlreadyExists, message)
                    .with_details(json!({ "branch": branch }))
            }
            WorktreeError::InvalidNamespace { reason } => {
                Self::new(IpcErrorCode::InvalidNamespace, message)
                    .with_details(json!({ "reason": reason }))
            }
            WorktreeError::InvalidRepoRoot { path } => {
                Self::new(IpcErrorCode::InvalidRepoRoot, message)
                    .with_details(json!({ "path": path }))
            }
            WorktreeError::GitError {
                command,
                stderr,
                stdout: _,
                code,
            } => Self::new(IpcErrorCode::GitError, message).with_details(json!({
                "command": command,
                "stderr": stderr,
                "exitCode": code,
            })),
            WorktreeError::ParseError(_) => Self::new(IpcErrorCode::ParseError, message),
            WorktreeError::Io(_) => Self::new(IpcErrorCode::IoError, message),
        }
    }
}

impl From<PtyError> for IpcError {
    fn from(error: PtyError) -> Self {
        let code = match error {
            PtyError::SessionNotFound(_) => IpcErrorCode::SessionNotFound,
            PtyError::PtyCreationError(_) => IpcErrorCode::PtyCreationError,
            PtyError::SpawnError(_) => IpcErrorCode::PtySpawnError,
            PtyError::IoError(_) => IpcErrorCode::PtyIoError,
            PtyError::ResizeError(_) => IpcErrorCode::PtyResizeError,
            PtyError::KillError(_) => IpcErrorCode::PtyKillError,
            PtyError::ChannelError(_) => IpcErrorCode::ChannelError,
            PtyError::Other(_) => IpcErrorCode::InternalError,
        };
        Self::new(code, error.to_string())
    }
}
