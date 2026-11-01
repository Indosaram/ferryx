//! Engine commands for downloads, print/PDF, find, zoom, file chooser and page notifications.
//!
//! Thin senders over the line protocol; the decisions (paths, permissions) are made in
//! `crate::ipc::browser_system`.

use super::{send, send_to_tab, TABS};
use crate::browser::BrowserError;
use ferryx_browser_engine_protocol::{Command as EngineCommand, DownloadAction, PagePermission};

/// The profile whose engine serves the tab.
pub fn profile_of(browser_id: &str) -> Option<String> {
    TABS.lock().get(browser_id).map(|tab| tab.profile.clone())
}

pub fn find(browser_id: &str, query: &str, forward: bool, find_next: bool) -> Result<(), BrowserError> {
    send_to_tab(
        browser_id,
        EngineCommand::Find {
            tab: browser_id.to_string(),
            query: query.to_string(),
            forward,
            match_case: false,
            find_next,
        },
    )
}

pub fn stop_find(browser_id: &str) -> Result<(), BrowserError> {
    send_to_tab(browser_id, EngineCommand::StopFind { tab: browser_id.to_string(), clear_selection: true })
}

/// `zoom` is a factor (1.0 = 100%).
pub fn set_zoom(browser_id: &str, zoom: f64) -> Result<(), BrowserError> {
    let percent = (zoom * 100.0).round().clamp(25.0, 500.0) as u32;
    send_to_tab(browser_id, EngineCommand::Zoom { tab: browser_id.to_string(), percent })
}

pub fn print(browser_id: &str) -> Result<(), BrowserError> {
    send_to_tab(browser_id, EngineCommand::Print { tab: browser_id.to_string() })
}

pub fn print_pdf(browser_id: &str, path: &str) -> Result<(), BrowserError> {
    send_to_tab(browser_id, EngineCommand::PrintPdf { tab: browser_id.to_string(), path: path.to_string() })
}

pub fn notification_permission(browser_id: &str, state: PagePermission) -> Result<(), BrowserError> {
    send_to_tab(browser_id, EngineCommand::NotificationPermission { tab: browser_id.to_string(), state })
}

/// Answers a download the engine began; an empty path cancels it.
pub fn download_path(profile: &str, download: u32, path: &str) -> bool {
    send(profile, &EngineCommand::DownloadPath { download, path: path.to_string() })
}

pub fn download_control(profile: &str, download: u32, action: DownloadAction) -> bool {
    send(profile, &EngineCommand::DownloadControl { download, action })
}

/// Answers a page's file chooser; no paths cancels it.
pub fn file_chosen(profile: &str, request: u32, paths: Vec<String>) -> bool {
    send(profile, &EngineCommand::FileChosen { request, paths })
}

/// Cancel, pause and resume of one engine download, for the download registry.
pub struct EngineDownloadControl {
    pub profile: String,
    pub download: u32,
}

impl crate::browser::download::DownloadControl for EngineDownloadControl {
    fn cancel(&self) {
        download_control(&self.profile, self.download, DownloadAction::Cancel);
    }

    fn pause(&self) {
        download_control(&self.profile, self.download, DownloadAction::Pause);
    }

    fn resume(&self) {
        download_control(&self.profile, self.download, DownloadAction::Resume);
    }
}
