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
    AgentResumeInvalid,
    AgentSessionConflict,
    DaemonProtocolMismatch,
    BrowserNotFound,
    WebviewNotFound,
    BrowserUrlInvalid,
    BrowserUrlSchemeDenied,
    BrowserFindFailed,
    BrowserDownloadFailed,
    BrowserBoundsInvalid,
    BrowserCreateFailed,
    BrowserNavigationFailed,
    BrowserHistoryFailed,
    BrowserCookieImportFailed,
    BrowserCloseFailed,
    BrowserAutomationSnapshotStale,
    BrowserAutomationTargetNotFound,
    BrowserAutomationFailed,
    BrowserCliUnavailable,
    PtyCreationError,
    PtySpawnError,
    PtyIoError,
    PtyResizeError,
    PtyKillError,
    ChannelError,
    NativeTerminalUnsupported,
    CliFileCollision,
    CliDirectoryCollision,
    CliParentSymlinkDenied,
    CliPlatformUnsupported,
    CliExecutableNotFound,
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

    pub fn native_terminal_unsupported() -> Self {
        Self::new(
            IpcErrorCode::NativeTerminalUnsupported,
            "Native terminal support is not compiled into this build (cargo feature `native-terminal` is disabled)",
        )
    }
}

impl From<WorktreeError> for IpcError {
    fn from(error: WorktreeError) -> Self {
        let message = error.to_string();
        match error {
            WorktreeError::DirtyWorktree { path, count, files } => {
                Self::new(IpcErrorCode::DirtyWorktree, message).with_details(json!({
                    "path": path,
                    "count": count,
                    "files": files,
                }))
            }
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
            WorktreeError::NotAGitRepository { path } => {
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

#[cfg(feature = "native-terminal")]
impl From<crate::native_terminal::NativeTerminalError> for IpcError {
    fn from(error: crate::native_terminal::NativeTerminalError) -> Self {
        let message = error.to_string();
        let code = match error {
            // A pane whose surface was released races its own in-flight geometry updates during a
            // tab switch. The frontend recognises this code and drops the update silently.
            crate::native_terminal::NativeTerminalError::SessionDetached(_) => {
                IpcErrorCode::SessionNotFound
            }
            _ => IpcErrorCode::InternalError,
        };
        Self::new(code, message)
    }
}

impl From<crate::browser::BrowserError> for IpcError {
    fn from(error: crate::browser::BrowserError) -> Self {
        let message = error.to_string();
        let code = match error {
            crate::browser::BrowserError::NotFound(_) => IpcErrorCode::BrowserNotFound,
            crate::browser::BrowserError::WebviewNotFound(_) => IpcErrorCode::WebviewNotFound,
            crate::browser::BrowserError::InvalidUrl(_) => IpcErrorCode::BrowserUrlInvalid,
            crate::browser::BrowserError::SchemeDenied(_) => IpcErrorCode::BrowserUrlSchemeDenied,
            crate::browser::BrowserError::InvalidBounds => IpcErrorCode::BrowserBoundsInvalid,
            crate::browser::BrowserError::UnsupportedProfile(_) => IpcErrorCode::InternalError,
            crate::browser::BrowserError::CreateFailed(_) => IpcErrorCode::BrowserCreateFailed,
            crate::browser::BrowserError::NavigationFailed(_) => {
                IpcErrorCode::BrowserNavigationFailed
            }
            crate::browser::BrowserError::HistoryFailed(_) => IpcErrorCode::BrowserHistoryFailed,
            crate::browser::BrowserError::FindFailed(_) => IpcErrorCode::BrowserFindFailed,
            crate::browser::BrowserError::DownloadFailed(_) => IpcErrorCode::BrowserDownloadFailed,
            crate::browser::BrowserError::CookieImport(_) => {
                IpcErrorCode::BrowserCookieImportFailed
            }
            crate::browser::BrowserError::CloseFailed(_) => IpcErrorCode::BrowserCloseFailed,
            crate::browser::BrowserError::AutomationSnapshotStale => {
                IpcErrorCode::BrowserAutomationSnapshotStale
            }
            crate::browser::BrowserError::AutomationTargetNotFound(_) => {
                IpcErrorCode::BrowserAutomationTargetNotFound
            }
            crate::browser::BrowserError::AutomationFailed(_) => {
                IpcErrorCode::BrowserAutomationFailed
            }
            crate::browser::BrowserError::CliUnavailable(_) => IpcErrorCode::BrowserCliUnavailable,
            crate::browser::BrowserError::PlatformUnsupported(_) => IpcErrorCode::InternalError,
            crate::browser::BrowserError::Internal(_) => IpcErrorCode::InternalError,
        };
        Self::new(code, message)
    }
}

impl From<crate::ipc::cli_install::CliInstallError> for IpcError {
    fn from(error: crate::ipc::cli_install::CliInstallError) -> Self {
        let message = error.to_string();
        match error {
            crate::ipc::cli_install::CliInstallError::FileCollision { path } => {
                Self::new(IpcErrorCode::CliFileCollision, message)
                    .with_details(json!({ "path": path.to_string_lossy() }))
            }
            crate::ipc::cli_install::CliInstallError::DirectoryCollision { path } => {
                Self::new(IpcErrorCode::CliDirectoryCollision, message)
                    .with_details(json!({ "path": path.to_string_lossy() }))
            }
            crate::ipc::cli_install::CliInstallError::ParentSymlinkDenied { path } => {
                Self::new(IpcErrorCode::CliParentSymlinkDenied, message)
                    .with_details(json!({ "path": path.to_string_lossy() }))
            }
            crate::ipc::cli_install::CliInstallError::ParentNotDirectory { path } => {
                Self::new(IpcErrorCode::InvalidPath, message)
                    .with_details(json!({ "path": path.to_string_lossy() }))
            }
            crate::ipc::cli_install::CliInstallError::PlatformUnsupported { platform } => {
                Self::new(IpcErrorCode::CliPlatformUnsupported, message)
                    .with_details(json!({ "platform": platform }))
            }
            crate::ipc::cli_install::CliInstallError::ExecutableNotFound(reason) => {
                Self::new(IpcErrorCode::CliExecutableNotFound, message)
                    .with_details(json!({ "reason": reason }))
            }
            crate::ipc::cli_install::CliInstallError::HomeDirNotFound => {
                Self::new(IpcErrorCode::InvalidPath, message)
            }
            crate::ipc::cli_install::CliInstallError::Io(err) => {
                Self::new(IpcErrorCode::IoError, message)
                    .with_details(json!({ "kind": format!("{:?}", err.kind()) }))
            }
        }
    }
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
