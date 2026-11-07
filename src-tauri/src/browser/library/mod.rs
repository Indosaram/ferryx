//! The browser library: bookmarks, history and omnibox suggestions, shared by both engines.
//! Stores live under `<data>/browser/`; see each module for the bulk-insert API importers use.

pub mod bookmarks;
pub mod history;
pub mod recorder;
pub mod suggest;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use bookmarks::{BookmarkError, BookmarkStore, LoadOutcome};
use history::{HistoryError, HistoryStore};

/// Lazily opened stores for one data dir. Every method does blocking I/O.
pub struct BrowserLibrary {
    data_dir: PathBuf,
    bookmarks: Mutex<Option<BookmarkStore>>,
    history: Mutex<Option<HistoryStore>>,
    recovered: Mutex<Option<LoadOutcome>>,
}

impl BrowserLibrary {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            bookmarks: Mutex::new(None),
            history: Mutex::new(None),
            recovered: Mutex::new(None),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Runs `f` on the bookmark store, opening it on first use.
    pub fn with_bookmarks<T>(
        &self,
        f: impl FnOnce(&mut BookmarkStore) -> Result<T, BookmarkError>,
    ) -> Result<T, BookmarkError> {
        let mut guard = self.bookmarks.lock().unwrap_or_else(PoisonError::into_inner);
        let store = match guard.as_mut() {
            Some(store) => store,
            None => {
                let (store, outcome) = BookmarkStore::open(BookmarkStore::default_path(&self.data_dir));
                if matches!(outcome, LoadOutcome::RecoveredFromBackup | LoadOutcome::ResetCorrupt) {
                    tracing::warn!(?outcome, "bookmark store was unreadable");
                    *self.recovered.lock().unwrap_or_else(PoisonError::into_inner) = Some(outcome);
                }
                guard.insert(store)
            }
        };
        f(store)
    }

    /// The recovery the bookmark store needed when it was opened, if any.
    pub fn bookmark_recovery(&self) -> Option<LoadOutcome> {
        *self.recovered.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `f` on the history store, opening it on first use.
    pub fn with_history<T>(
        &self,
        f: impl FnOnce(&mut HistoryStore) -> Result<T, HistoryError>,
    ) -> Result<T, HistoryError> {
        let mut guard = self.history.lock().unwrap_or_else(PoisonError::into_inner);
        let store = match guard.as_mut() {
            Some(store) => store,
            None => guard.insert(HistoryStore::open(&HistoryStore::default_path(&self.data_dir))?),
        };
        f(store)
    }
}
