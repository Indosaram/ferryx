//! Browser data import for Ferryx, ported from Maho's `maho-import`.
//!
//! Phase 1: parsers (Arc, Chromium, Firefox, Safari, Zen) ported from C++ to Rust.
//! Phase 2: orchestrator + workers — coordinates the import flow in pure Rust.
//! Phase 3: password decrypt paths.
//!
//! Every SQLite source is read through a private copy (`sqlite_snapshot`), so a browser that
//! is running and holds its databases open can still be imported from.

pub mod decrypt;
pub mod detect;
pub mod orchestrator;
pub mod parsers;
pub mod password_import_job;
pub mod sqlite_snapshot;
pub mod workers;

/// Secret buffers for callers (the credentials of a password import are `Zeroizing`).
pub use zeroize::Zeroizing;

pub use parsers::password_import_sources::{
    parse_password_import_source_file, parse_password_import_source_preview,
    PasswordImportCredential, PasswordImportFileFormat, PasswordImportParsedFile,
    PasswordImportPreview, PasswordImportSource, PasswordImportSourceFormat,
};
pub use password_import_job::{
    PasswordImportCancelSummary, PasswordImportCommitDestination, PasswordImportCommitError,
    PasswordImportCommitSummary, PasswordImportInput, PasswordImportJob, PasswordImportJobConfig,
    PasswordImportJobError, PasswordImportPreviewReceipt,
};

#[cfg(test)]
mod password_import_job_tests;
#[cfg(test)]
mod password_import_sources_contract_tests;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ParsedTab {
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default)]
    pub is_essential: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ParsedFolder {
    pub id: String,
    pub name: String,
    pub parent_id: String,
    pub workspace_id: String,
    pub tabs: Vec<ParsedTab>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ParsedWorkspace {
    pub uuid: String,
    pub name: String,
    pub icon: String,
    pub theme_color_hex: String,
    pub container_id: i32,
    pub pinned_tabs: Vec<ParsedTab>,
    pub favorites: Vec<ParsedTab>,
    pub regular_tabs: Vec<ParsedTab>,
    pub folders: Vec<ParsedFolder>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BookmarkEntry {
    pub title: String,
    pub url: String,
    pub is_folder: bool,
    pub children: Vec<BookmarkEntry>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
    pub visit_time: f64,
    pub visit_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportServices(pub u32);

impl ImportServices {
    pub const HISTORY: u32 = 1 << 0;
    pub const BOOKMARKS: u32 = 1 << 1;
    pub const PASSWORDS: u32 = 1 << 2;
    pub const AUTOFILL: u32 = 1 << 3;
    pub const COOKIES: u32 = 1 << 4;
    pub const WORKSPACES: u32 = 1 << 5;
    pub const FAVICONS: u32 = 1 << 6;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrowserType {
    Chrome,
    Arc,
    Brave,
    Edge,
    Vivaldi,
    Opera,
    Firefox,
    Zen,
    Safari,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetectedBrowser {
    pub browser_type: BrowserType,
    pub display_name: String,
    pub profile_path: PathBuf,
    pub services_supported: u32,
    pub requires_full_disk_access: bool,
}

impl DetectedBrowser {
    /// The directory holding the profile's data files. `profile_path` is that directory
    /// (what [`detect::detect_installed_browsers`] returns) or a file inside it (Safari's
    /// `Bookmarks.plist`, or a `Preferences` / `prefs.js` a caller names).
    pub fn profile_dir(&self) -> &std::path::Path {
        if self.profile_path.is_dir() {
            &self.profile_path
        } else {
            self.profile_path.parent().unwrap_or(&self.profile_path)
        }
    }
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum ImportError {
    #[error("file not found: {0}")]
    FileNotFound(String),
    #[error("io: {0}")]
    Io(String),
    #[error("parse: {0}")]
    Parse(String),
    #[error("unsupported scheme: {0}")]
    UnsupportedScheme(String),
    #[error("permission denied (Full Disk Access required): {0}")]
    PermissionDenied(String),
    #[error("in use by another program: {0}")]
    Locked(String),
    #[error("encryption key unavailable: {0}")]
    KeyUnavailable(String),
    #[error("decompression failed: {0}")]
    DecompressFailed(String),
}

pub type ImportResult<T> = std::result::Result<T, ImportError>;

impl ImportError {
    /// A stable machine-readable code for the UI (`details.code` of a per-type failure).
    pub fn code(&self) -> &'static str {
        match self {
            Self::FileNotFound(_) => "not-found",
            Self::PermissionDenied(_) => "permission-denied",
            Self::Locked(_) => "locked",
            Self::KeyUnavailable(_) => "key-unavailable",
            Self::Io(_) => "io",
            Self::Parse(_) | Self::DecompressFailed(_) => "parse",
            Self::UnsupportedScheme(_) => "unsupported",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CookieEntry {
    pub host: String,
    pub name: String,
    pub value: String,
    pub path: String,
    pub expires: i64,
    pub is_secure: bool,
    pub is_httponly: bool,
    pub same_site: i32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AutofillEntry {
    pub field_name: String,
    pub value: String,
    pub times_used: i32,
    pub first_used: i64,
    pub last_used: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PasswordEntry {
    pub origin_url: String,
    pub action_url: String,
    pub username: String,
    #[serde(skip_serializing)]
    pub password: String,
}
