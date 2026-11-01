//! Import from other browsers (Chrome, Edge, Brave, Vivaldi, Opera, Arc, Firefox, Zen,
//! Safari) and from password-manager exports.
//!
//! Parsing and decryption live in the `ferryx-browser-import` crate (ported from Maho).
//! This module picks a source, runs one import type at a time and hands the records to an
//! [`ImportSink`]. Cookies go straight into the target profile (see [`cookie_sink`]).
//! Bookmarks, history and passwords are written as JSON-lines staging files under
//! `<data>/browser-import/<profile>/` (see [`staging`]) for the bookmark, history and vault
//! stores to ingest; the orchestrator connects those stores to the sink.

pub mod cookie_sink;
pub mod run;
pub mod staging;

use serde::{Deserialize, Serialize};

pub use ferryx_browser_import::{CookieEntry, DetectedBrowser, ImportError};

/// The data types the import dialog offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportDataType {
    Bookmarks,
    History,
    Cookies,
    Passwords,
}

impl ImportDataType {
    pub const ALL: [Self; 4] = [Self::Bookmarks, Self::History, Self::Cookies, Self::Passwords];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bookmarks => "bookmarks",
            Self::History => "history",
            Self::Cookies => "cookies",
            Self::Passwords => "passwords",
        }
    }
}

/// One bookmark: its folder path from the source's root (e.g. `["Bookmarks bar", "Work"]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedBookmark {
    pub title: String,
    pub url: String,
    pub folder_path: Vec<String>,
}

/// One history entry; `visited_at_unix` is seconds since the Unix epoch (fractional).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedHistoryEntry {
    pub url: String,
    pub title: String,
    pub visited_at_unix: f64,
    pub visit_count: u32,
}

/// One saved login. The password is a secret: never log it or put it in an error.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedPassword {
    pub origin_url: String,
    pub action_url: String,
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for ImportedPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportedPassword")
            .field("origin_url", &self.origin_url)
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

/// Why a sink refused a record or a batch.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SinkError {
    /// The destination store does not exist (yet) for this data type.
    #[error("no destination for {0}")]
    NoDestination(&'static str),
    #[error("{0}")]
    Failed(String),
}

/// What a sink kept of one data type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SinkOutcome {
    pub kept: usize,
    /// Records the sink chose not to keep (e.g. Google cookies).
    pub skipped: usize,
    pub notes: Vec<String>,
}

/// Where imported records go. Each `add_*` is called once per record during an import;
/// [`ImportSink::finish`] runs once when a data type completed and [`ImportSink::discard`]
/// instead when it failed or was cancelled, so a partial type is never published.
pub trait ImportSink: Send + Sync {
    fn add_bookmark(&self, bookmark: ImportedBookmark) -> Result<(), SinkError>;
    fn add_history(&self, entry: ImportedHistoryEntry) -> Result<(), SinkError>;
    /// `Ok(false)`: the cookie is not importable and was skipped.
    fn add_cookie(&self, cookie: &CookieEntry) -> Result<bool, SinkError>;
    fn add_password(&self, password: ImportedPassword) -> Result<(), SinkError>;
    fn finish(&self, data_type: ImportDataType) -> Result<SinkOutcome, SinkError>;
    fn discard(&self, data_type: ImportDataType);
}

/// Notes, counts and an optional error for one data type of one import.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportTypeResult {
    #[serde(rename = "type")]
    pub data_type: ImportDataType,
    pub imported: usize,
    pub skipped: usize,
    pub notes: Vec<String>,
    pub error: Option<ImportTypeError>,
}

/// A per-type failure: `code` is machine readable (see [`error_code`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportTypeError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub results: Vec<ImportTypeResult>,
}

impl ImportTypeError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.to_string(), message: message.into() }
    }

    pub fn from_import(error: &ImportError) -> Self {
        Self::new(error.code(), error.to_string())
    }

    pub fn from_sink(error: &SinkError) -> Self {
        let code = match error {
            SinkError::NoDestination(_) => "no-destination",
            SinkError::Failed(_) => "sink-failed",
        };
        Self::new(code, error.to_string())
    }
}
