use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub path: PathBuf,
    pub head: String,
    pub branch: Option<String>,
    pub bare: bool,
    pub detached: bool,
    pub locked: Option<String>,
    pub prunable: Option<String>,
}

impl Worktree {
    /// Returns the short branch name if this worktree is checked out on a branch.
    /// For example, `refs/heads/orca/ws1/feat` -> `orca/ws1/feat`.
    pub fn branch_short_name(&self) -> Option<&str> {
        self.branch
            .as_deref()
            .map(|b| b.strip_prefix("refs/heads/").unwrap_or(b))
    }

    /// Extracts Orca workspace ID and slug if the worktree branch follows `orca/<ws-id>/<slug>`.
    pub fn orca_info(&self) -> Option<OrcaWorktreeInfo> {
        let branch = self.branch_short_name()?;
        let parts: Vec<&str> = branch.split('/').collect();
        if parts.len() >= 3 && parts[0] == "orca" {
            Some(OrcaWorktreeInfo {
                ws_id: parts[1].to_string(),
                slug: parts[2..].join("/"),
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrcaWorktreeInfo {
    pub ws_id: String,
    pub slug: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorktreeIdentity {
    pub ws_id: String,
    pub slug: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirtyFile {
    pub status_code: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirtyState {
    pub is_dirty: bool,
    pub files: Vec<DirtyFile>,
}

impl DirtyState {
    pub fn clean() -> Self {
        Self {
            is_dirty: false,
            files: Vec::new(),
        }
    }

    pub fn dirty(files: Vec<DirtyFile>) -> Self {
        Self {
            is_dirty: !files.is_empty(),
            files,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorktreeOptions {
    pub ws_id: String,
    pub slug: String,
    pub path: PathBuf,
    pub base_ref: Option<String>,
}

impl CreateWorktreeOptions {
    pub fn new(
        ws_id: impl Into<String>,
        slug: impl Into<String>,
        path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            ws_id: ws_id.into(),
            slug: slug.into(),
            path: path.into(),
            base_ref: None,
        }
    }

    pub fn with_base_ref(mut self, base_ref: impl Into<String>) -> Self {
        self.base_ref = Some(base_ref.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchDeletionPreview {
    pub branch: String,
    pub head: String,
    pub upstream: Option<String>,
    pub merged: bool,
    pub ahead: Option<u64>,
    pub behind: Option<u64>,
}

#[derive(Debug, Error)]
pub enum WorktreeError {
    #[error("Git command failed ({command}): {stderr} (exit code: {code:?})")]
    GitError {
        command: String,
        stderr: String,
        stdout: String,
        code: Option<i32>,
    },

    #[error("Worktree at '{path}' is dirty ({count} uncommitted/untracked files); deletion forbidden for 1-writer-1-worktree isolation")]
    DirtyWorktree {
        path: PathBuf,
        count: usize,
        files: Vec<String>,
    },

    #[error("Worktree '{path}' already has active writer '{owner_id}'")]
    WriterAlreadyActive { path: PathBuf, owner_id: String },

    #[error("Writer lease for '{path}' is owned by '{owner_id}', not '{requested_owner_id}'")]
    WriterLeaseOwnerMismatch {
        path: PathBuf,
        owner_id: String,
        requested_owner_id: String,
    },

    #[error(
        "Branch '{branch}' at {head} is not merged; use the explicit destructive deletion API"
    )]
    UnmergedBranch { branch: String, head: String },

    #[error("Workspace '{workspace_id}' is not registered")]
    WorkspaceNotFound { workspace_id: String },

    #[error("Workspace '{workspace_id}' is already registered")]
    WorkspaceAlreadyRegistered { workspace_id: String },

    #[error("Worktree identity '{ws_id}/{slug}' was not found in workspace '{workspace_id}'")]
    WorktreeIdentityNotFound {
        workspace_id: String,
        ws_id: String,
        slug: String,
    },

    #[error("Path '{path}' escapes registered workspace root '{root}'")]
    PathOutsideWorkspace { path: PathBuf, root: PathBuf },

    #[error("Invalid path '{path}': {reason}")]
    InvalidPath { path: PathBuf, reason: String },

    #[error("Worktree not found at '{path}'")]
    WorktreeNotFound { path: PathBuf },

    #[error("Worktree already exists at target path '{path}'")]
    WorktreeAlreadyExists { path: PathBuf },

    #[error("Branch already exists: '{branch}'")]
    BranchAlreadyExists { branch: String },

    #[error("Invalid workspace ID or slug for branch namespace: {reason}")]
    InvalidNamespace { reason: String },

    #[error("Repository root invalid or not found at '{path}'")]
    InvalidRepoRoot { path: PathBuf },

    #[error("'{path}' is not inside a git repository; worktree operations are unavailable")]
    NotAGitRepository { path: PathBuf },

    #[error("Failed to parse git output: {0}")]
    ParseError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
