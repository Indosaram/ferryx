use crate::terminal::PtyError;
use crate::worktree::WorktreeError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IpcErrorCode {
    WorktreeBusy,
    WorktreeLocked,
    WorktreeRemovedBranchRetained,
    WorktreeRemovedPruneFailed,
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
    ScanCancelled,
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
    BrowserStaleRef,
    BrowserInvalidReference,
    BrowserNoTab,
    BrowserTabNotFound,
    BrowserHostUnavailable,
    BrowserDialogNotFound,
    BrowserDownloadCancelled,
    BrowserAutomationSnapshotStale,
    BrowserAutomationTargetNotFound,
    BrowserAutomationFailed,
    BrowserCliUnavailable,
    BrowserWaitTimeout,
    BrowserScreenshotFailed,
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
    InvalidArgument,
    InternalError,
    Unsupported,
    // Local pane reliable split and liveness capability error codes
    UnsupportedCapability,
    SpawnRequestConflict,
    SpawnRequestExpired,
    SpawnEpochChanged,
    SpawnAttemptTimeout,
    SpawnCancelled,
    SessionIdConflict,
    // Paired host and machine protocol error codes (P08)
    SessionExpired,
    ParentSessionMismatch,
    Timeout,
    HostUnavailable,
    OperationOutcomeUnknown,
    OperationNotFound,
    OperationResultExpired,
    SessionOwnershipChanged,
    ControlConflict,
    StaleEpoch,
    StaleGeneration,
    PairedProxyUnavailable,
    PairedHostRemoteError,
    PairedHostInvalidResponse,
    PairedHostStaleGeneration,
    PairedHostRedirectRejected,
    Unauthorized,
    MachineAccessRequired,
    MachineOwnerUnsupported,
    MachineServiceUnavailable,
    InvalidRequest,
    UnsupportedPath,
    PayloadTooLarge,
    PermissionDenied,
    RateLimited,
    CapacityExceeded,
    DirectoryNotFound,
    ProjectNotFound,
    ProjectBusy,
    NotAGitRepository,
    BaseRefUnavailable,
    InvalidBaseRef,
    InvalidWorktree,
    WorkspaceIdMismatch,
    #[serde(rename = "GITHUB_ISSUE_REPOSITORY_MISMATCH")]
    GitHubIssueRepositoryMismatch,
    OutputLimitExceeded,
    RequestConflict,
    StaleRevision,
    AgentResumeUnsupported,
    AuthExpired,
    NotFound,
    MethodNotAllowed,
    #[serde(untagged)]
    Custom(String),
}

impl std::fmt::Display for IpcErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = serde_json::to_value(self).map_err(|_| std::fmt::Error)?;
        f.write_str(value.as_str().ok_or(std::fmt::Error)?)
    }
}

impl IpcErrorCode {
    pub fn from_code_str(s: &str) -> Self {
        match s {
            "SESSION_NOT_FOUND" => Self::SessionNotFound,
            "SESSION_EXPIRED" => Self::SessionExpired,
            "PARENT_SESSION_MISMATCH" => Self::ParentSessionMismatch,
            "TIMEOUT" => Self::Timeout,
            "HOST_UNAVAILABLE" => Self::HostUnavailable,
            "OPERATION_OUTCOME_UNKNOWN" => Self::OperationOutcomeUnknown,
            "OPERATION_NOT_FOUND" => Self::OperationNotFound,
            "OPERATION_RESULT_EXPIRED" => Self::OperationResultExpired,
            "SESSION_OWNERSHIP_CHANGED" => Self::SessionOwnershipChanged,
            "CONTROL_CONFLICT" => Self::ControlConflict,
            "STALE_EPOCH" => Self::StaleEpoch,
            "STALE_GENERATION" => Self::StaleGeneration,
            "PAIRED_PROXY_UNAVAILABLE" => Self::PairedProxyUnavailable,
            "PAIRED_HOST_REMOTE_ERROR" => Self::PairedHostRemoteError,
            "PAIRED_HOST_INVALID_RESPONSE" => Self::PairedHostInvalidResponse,
            "PAIRED_HOST_STALE_GENERATION" => Self::PairedHostStaleGeneration,
            "PAIRED_HOST_REDIRECT_REJECTED" => Self::PairedHostRedirectRejected,
            "UNAUTHORIZED" => Self::Unauthorized,
            "MACHINE_ACCESS_REQUIRED" => Self::MachineAccessRequired,
            "MACHINE_OWNER_UNSUPPORTED" => Self::MachineOwnerUnsupported,
            "MACHINE_SERVICE_UNAVAILABLE" => Self::MachineServiceUnavailable,
            "INVALID_REQUEST" => Self::InvalidRequest,
            "INVALID_PATH" => Self::InvalidPath,
            "UNSUPPORTED_PATH" => Self::UnsupportedPath,
            "PAYLOAD_TOO_LARGE" => Self::PayloadTooLarge,
            "PERMISSION_DENIED" => Self::PermissionDenied,
            "RATE_LIMITED" => Self::RateLimited,
            "CAPACITY_EXCEEDED" => Self::CapacityExceeded,
            "DIRECTORY_NOT_FOUND" => Self::DirectoryNotFound,
            "PROJECT_NOT_FOUND" => Self::ProjectNotFound,
            "PROJECT_BUSY" => Self::ProjectBusy,
            "NOT_A_GIT_REPOSITORY" => Self::NotAGitRepository,
            "BASE_REF_UNAVAILABLE" => Self::BaseRefUnavailable,
            "INVALID_BASE_REF" => Self::InvalidBaseRef,
            "INVALID_WORKTREE" => Self::InvalidWorktree,
            "WORKSPACE_ID_MISMATCH" => Self::WorkspaceIdMismatch,
            "GITHUB_ISSUE_REPOSITORY_MISMATCH" => Self::GitHubIssueRepositoryMismatch,
            "WORKTREE_NOT_FOUND" => Self::WorktreeNotFound,
            "WORKTREE_BUSY" => Self::WorktreeBusy,
            "WORKTREE_EXISTS" | "WORKTREE_ALREADY_EXISTS" => Self::WorktreeAlreadyExists,
            "WORKTREE_LOCKED" => Self::WorktreeLocked,
            "DIRTY_WORKTREE" => Self::DirtyWorktree,
            "UNMERGED_BRANCH" => Self::UnmergedBranch,
            "WORKTREE_REMOVED_BRANCH_RETAINED" => Self::WorktreeRemovedBranchRetained,
            "WORKTREE_REMOVED_PRUNE_FAILED" => Self::WorktreeRemovedPruneFailed,
            "OUTPUT_LIMIT_EXCEEDED" => Self::OutputLimitExceeded,
            "REQUEST_CONFLICT" => Self::RequestConflict,
            "STALE_REVISION" => Self::StaleRevision,
            "AGENT_RESUME_UNSUPPORTED" => Self::AgentResumeUnsupported,
            "AGENT_SESSION_CONFLICT" => Self::AgentSessionConflict,
            "AGENT_RESUME_INVALID" => Self::AgentResumeInvalid,
            "AUTH_EXPIRED" => Self::AuthExpired,
            "NOT_FOUND" => Self::NotFound,
            "METHOD_NOT_ALLOWED" => Self::MethodNotAllowed,
            "INTERNAL_ERROR" => Self::InternalError,
            "UNSUPPORTED" => Self::Unsupported,
            "SESSION_ID_CONFLICT" => Self::SessionIdConflict,
            "INVALID_ARGUMENT" => Self::InvalidArgument,
            "IO_ERROR" => Self::IoError,
            "PARSE_ERROR" => Self::ParseError,
            "GIT_ERROR" => Self::GitError,
            "SCAN_CANCELLED" => Self::ScanCancelled,
            "BROWSER_STALE_REF" => Self::BrowserStaleRef,
            "BROWSER_INVALID_REFERENCE" => Self::BrowserInvalidReference,
            "BROWSER_NO_TAB" => Self::BrowserNoTab,
            "BROWSER_TAB_NOT_FOUND" => Self::BrowserTabNotFound,
            "BROWSER_HOST_UNAVAILABLE" => Self::BrowserHostUnavailable,
            "BROWSER_DIALOG_NOT_FOUND" => Self::BrowserDialogNotFound,
            "BROWSER_DOWNLOAD_CANCELLED" => Self::BrowserDownloadCancelled,
            "BROWSER_AUTOMATION_SNAPSHOT_STALE" => Self::BrowserStaleRef,
            "BROWSER_AUTOMATION_TARGET_NOT_FOUND" => Self::BrowserInvalidReference,
            "BROWSER_NOT_FOUND" => Self::BrowserNotFound,
            "WEBVIEW_NOT_FOUND" => Self::WebviewNotFound,
            "BROWSER_URL_INVALID" => Self::BrowserUrlInvalid,
            "BROWSER_URL_SCHEME_DENIED" => Self::BrowserUrlSchemeDenied,
            "BROWSER_FIND_FAILED" => Self::BrowserFindFailed,
            "BROWSER_DOWNLOAD_FAILED" => Self::BrowserDownloadFailed,
            "BROWSER_BOUNDS_INVALID" => Self::BrowserBoundsInvalid,
            "BROWSER_CREATE_FAILED" => Self::BrowserCreateFailed,
            "BROWSER_NAVIGATION_FAILED" => Self::BrowserNavigationFailed,
            "BROWSER_HISTORY_FAILED" => Self::BrowserHistoryFailed,
            "BROWSER_COOKIE_IMPORT_FAILED" => Self::BrowserCookieImportFailed,
            "BROWSER_CLOSE_FAILED" => Self::BrowserCloseFailed,
            "BROWSER_AUTOMATION_FAILED" => Self::BrowserAutomationFailed,
            "BROWSER_CLI_UNAVAILABLE" => Self::BrowserCliUnavailable,
            "BROWSER_WAIT_TIMEOUT" => Self::BrowserWaitTimeout,
            "BROWSER_SCREENSHOT_FAILED" => Self::BrowserScreenshotFailed,
            "UNSUPPORTED_CAPABILITY" => Self::UnsupportedCapability,
            "SPAWN_REQUEST_CONFLICT" => Self::SpawnRequestConflict,
            "SPAWN_REQUEST_EXPIRED" => Self::SpawnRequestExpired,
            "SPAWN_EPOCH_CHANGED" => Self::SpawnEpochChanged,
            "SPAWN_ATTEMPT_TIMEOUT" => Self::SpawnAttemptTimeout,
            "SPAWN_CANCELLED" => Self::SpawnCancelled,
            other => Self::Custom(other.to_string()),
        }
    }
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

    pub fn internal(message: impl std::fmt::Display) -> Self {
        Self::new(IpcErrorCode::InternalError, message.to_string())
    }

    pub fn native_terminal_unsupported() -> Self {
        Self::new(
            IpcErrorCode::NativeTerminalUnsupported,
            "Native terminal support is not compiled into this build (cargo feature `native-terminal` is disabled)",
        )
    }

    pub fn unsupported_capability(message: impl Into<String>) -> Self {
        Self::new(IpcErrorCode::UnsupportedCapability, message)
    }

    pub fn spawn_request_conflict(message: impl Into<String>) -> Self {
        Self::new(IpcErrorCode::SpawnRequestConflict, message)
    }

    pub fn spawn_request_expired(message: impl Into<String>) -> Self {
        Self::new(IpcErrorCode::SpawnRequestExpired, message)
    }

    pub fn spawn_epoch_changed(message: impl Into<String>) -> Self {
        Self::new(IpcErrorCode::SpawnEpochChanged, message)
    }

    pub fn spawn_attempt_timeout(message: impl Into<String>) -> Self {
        Self::new(IpcErrorCode::SpawnAttemptTimeout, message)
    }

    pub fn spawn_cancelled(message: impl Into<String>) -> Self {
        Self::new(IpcErrorCode::SpawnCancelled, message)
    }
}

impl From<WorktreeError> for IpcError {
    fn from(error: WorktreeError) -> Self {
        let message = error.to_string();
        match error {
            WorktreeError::WorktreeBusy {
                path,
                live_session_ids,
            } => Self::new(IpcErrorCode::WorktreeBusy, message).with_details(json!({
                "path": path, "liveSessionIds": live_session_ids,
            })),
            WorktreeError::WorktreeLocked { path, reason } => {
                Self::new(IpcErrorCode::WorktreeLocked, message).with_details(json!({
                    "path": path, "reason": reason,
                }))
            }
            WorktreeError::WorktreeRemovedBranchRetained {
                path,
                branch,
                source,
            } => Self::new(IpcErrorCode::WorktreeRemovedBranchRetained, message).with_details(
                json!({
                    "path": path, "branch": branch, "worktreeRemoved": true,
                    "branchDeleted": false, "cause": Self::from(*source),
                }),
            ),
            WorktreeError::WorktreeRemovedPruneFailed { path, source } => {
                Self::new(IpcErrorCode::WorktreeRemovedPruneFailed, message).with_details(json!({
                    "path": path, "worktreeRemoved": true, "pruned": false,
                    "branchDeleted": false, "cause": Self::from(*source),
                }))
            }
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
            WorktreeError::RemoteUnsupported => Self::new(IpcErrorCode::Unsupported, message),
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
                IpcErrorCode::BrowserStaleRef
            }
            crate::browser::BrowserError::AutomationTargetNotFound(_) => {
                IpcErrorCode::BrowserInvalidReference
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_split_reliability_typed_error_codes_roundtrip() {
        let cases = [
            (IpcErrorCode::UnsupportedCapability, "UNSUPPORTED_CAPABILITY"),
            (IpcErrorCode::SpawnRequestConflict, "SPAWN_REQUEST_CONFLICT"),
            (IpcErrorCode::SpawnRequestExpired, "SPAWN_REQUEST_EXPIRED"),
            (IpcErrorCode::SpawnEpochChanged, "SPAWN_EPOCH_CHANGED"),
            (IpcErrorCode::SpawnAttemptTimeout, "SPAWN_ATTEMPT_TIMEOUT"),
            (IpcErrorCode::SpawnCancelled, "SPAWN_CANCELLED"),
            (IpcErrorCode::SessionIdConflict, "SESSION_ID_CONFLICT"),
        ];
        for (code, wire) in cases {
            let encoded = serde_json::to_value(&code).expect("encode typed error");
            assert_eq!(encoded, json!(wire));
            assert_eq!(serde_json::from_value::<IpcErrorCode>(encoded).expect("decode typed error"), code);
            assert_eq!(IpcErrorCode::from_code_str(wire), code);
        }
    }

    #[test]
    fn test_ipc_error_code_custom_roundtrip() {
        let code = IpcErrorCode::Custom("MY_UNKNOWN_CODE".to_string());
        let json_str = serde_json::to_string(&code).unwrap();
        assert_eq!(json_str, "\"MY_UNKNOWN_CODE\"");
        let deserialized: IpcErrorCode = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized, code);

        let known = IpcErrorCode::SessionNotFound;
        let known_str = serde_json::to_string(&known).unwrap();
        assert_eq!(known_str, "\"SESSION_NOT_FOUND\"");
        let deserialized_known: IpcErrorCode = serde_json::from_str(&known_str).unwrap();
        assert_eq!(deserialized_known, known);

        let from_str = IpcErrorCode::from_code_str("CUSTOM_CODE_123");
        assert_eq!(
            from_str,
            IpcErrorCode::Custom("CUSTOM_CODE_123".to_string())
        );
    }

    #[test]
    fn test_browser_structured_recovery_error_codes() {
        let cases = vec![
            (IpcErrorCode::BrowserStaleRef, "\"BROWSER_STALE_REF\"", "BROWSER_STALE_REF"),
            (IpcErrorCode::BrowserInvalidReference, "\"BROWSER_INVALID_REFERENCE\"", "BROWSER_INVALID_REFERENCE"),
            (IpcErrorCode::BrowserNoTab, "\"BROWSER_NO_TAB\"", "BROWSER_NO_TAB"),
            (IpcErrorCode::BrowserTabNotFound, "\"BROWSER_TAB_NOT_FOUND\"", "BROWSER_TAB_NOT_FOUND"),
            (IpcErrorCode::BrowserHostUnavailable, "\"BROWSER_HOST_UNAVAILABLE\"", "BROWSER_HOST_UNAVAILABLE"),
            (IpcErrorCode::BrowserDialogNotFound, "\"BROWSER_DIALOG_NOT_FOUND\"", "BROWSER_DIALOG_NOT_FOUND"),
            (IpcErrorCode::BrowserDownloadCancelled, "\"BROWSER_DOWNLOAD_CANCELLED\"", "BROWSER_DOWNLOAD_CANCELLED"),
        ];

        for (code, expected_json, code_str) in cases {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(json, expected_json);
            let parsed = IpcErrorCode::from_code_str(code_str);
            assert_eq!(parsed, code);
        }

        // Aliased backwards compatibility: legacy strings map to structured recovery codes
        assert_eq!(
            IpcErrorCode::from_code_str("BROWSER_AUTOMATION_SNAPSHOT_STALE"),
            IpcErrorCode::BrowserStaleRef
        );
        assert_eq!(
            IpcErrorCode::from_code_str("BROWSER_AUTOMATION_TARGET_NOT_FOUND"),
            IpcErrorCode::BrowserInvalidReference
        );
    }

    #[test]
    fn test_browser_error_conversion_maps_to_recovery_codes() {
        use crate::browser::BrowserError;

        let stale_err = IpcError::from(BrowserError::AutomationSnapshotStale);
        assert_eq!(stale_err.code, IpcErrorCode::BrowserStaleRef);
        assert_eq!(serde_json::to_string(&stale_err.code).unwrap(), "\"BROWSER_STALE_REF\"");

        let not_found_ref = IpcError::from(BrowserError::AutomationTargetNotFound("e-1".into()));
        assert_eq!(not_found_ref.code, IpcErrorCode::BrowserInvalidReference);
        assert_eq!(serde_json::to_string(&not_found_ref.code).unwrap(), "\"BROWSER_INVALID_REFERENCE\"");

        let not_found_browser = IpcError::from(BrowserError::NotFound("b-1".into()));
        assert_eq!(not_found_browser.code, IpcErrorCode::BrowserNotFound);
    }
}
