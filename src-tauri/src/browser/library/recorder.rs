//! Turns browser state changes from either engine (system webview page-load/title callbacks,
//! Chromium engine Address/Title/Loading events) into history writes. One visit is recorded
//! when a tab settles on a new URL; a later title for the same URL only updates the title.
//! Private tabs, failed loads and non-history URLs are never recorded, and nothing is recorded
//! until the UI has said history is enabled (the setting lives in the UI).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use super::history::is_recordable_url;
use crate::browser::BrowserState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordAction {
    Visit { url: String, title: Option<String> },
    Title { url: String, title: String },
}

#[derive(Default)]
pub struct HistoryRecorder {
    enabled: AtomicBool,
    last: Mutex<HashMap<String, (String, Option<String>)>>,
}

impl HistoryRecorder {
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Forgets a closed tab so a reused id records its first page again.
    pub fn forget(&self, browser_id: &str) {
        self.last.lock().unwrap_or_else(PoisonError::into_inner).remove(browser_id);
    }

    pub fn observe(&self, state: &BrowserState) -> Option<RecordAction> {
        if !self.enabled()
            || state.profile_id.is_private()
            || state.loading
            || state.load_error.is_some()
            || !is_recordable_url(&state.url)
        {
            return None;
        }
        let title = state.title.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        match last.get_mut(&state.browser_id) {
            Some((url, seen_title)) if *url == state.url => match title {
                Some(title) if seen_title.as_deref() != Some(title.as_str()) => {
                    *seen_title = Some(title.clone());
                    Some(RecordAction::Title { url: url.clone(), title })
                }
                _ => None,
            },
            _ => {
                last.insert(state.browser_id.clone(), (state.url.clone(), title.clone()));
                Some(RecordAction::Visit { url: state.url.clone(), title })
            }
        }
    }
}
