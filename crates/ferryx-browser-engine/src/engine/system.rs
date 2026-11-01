//! Downloads, find in page, zoom, print, file chooser and the page Notification shim.
//!
//! Every decision (where a download goes, which file the page gets, whether a site may
//! notify) is Ferryx's: the engine reports with an event and waits for the answering command.
//! All handlers here run on the CEF UI thread, like the commands, so the pending callbacks live
//! in thread-locals.

use super::{browser_of, emit, key_of, Key};
use cef::*;
use ferryx_browser_engine_protocol::{
    notification_permission_script, notification_shim, DownloadAction, DownloadFailure, DownloadState, Event,
    FileDialogMode as PaneFileDialogMode, PagePermission, PAGE_MESSAGE_CONSOLE_PREFIX,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// Report progress at most this often in bytes, besides every state change.
const PROGRESS_STEP_BYTES: u64 = 512 * 1024;

struct Download {
    /// Answered by [`Command::DownloadPath`](ferryx_browser_engine_protocol::Command).
    before: Option<BeforeDownloadCallback>,
    /// The latest control handle CEF gave for the download.
    control: Option<DownloadItemCallback>,
    last_state: Option<DownloadState>,
    last_reported: u64,
    /// Ferryx answered with an empty path: the download is cancelled once CEF hands a control.
    refused: bool,
}

thread_local! {
    static DOWNLOADS: RefCell<HashMap<u32, Download>> = RefCell::new(HashMap::new());
    static FILE_DIALOGS: RefCell<HashMap<u32, FileDialogCallback>> = RefCell::new(HashMap::new());
    static NEXT_DIALOG: Cell<u32> = const { Cell::new(1) };
}

fn tab_of(browser: Option<&mut Browser>) -> String {
    match browser.and_then(key_of) {
        Some(Key::Tab(tab)) => tab,
        _ => String::new(),
    }
}

fn host_of(tab: &str) -> Option<BrowserHost> {
    browser_of(tab).and_then(|browser| browser.host())
}

fn text(value: Option<&CefString>) -> String {
    value.map(CefString::to_string).unwrap_or_default()
}

fn user_text(value: CefStringUserfree) -> String {
    CefString::from(&value).to_string()
}

fn list(value: Option<&mut CefStringList>) -> Vec<String> {
    let Some(value) = value else { return Vec::new() };
    let raw: *mut sys::_cef_string_list_t = value.into();
    // A borrowed view: dropping it does not free CEF's list.
    CefStringList::from(raw).into_iter().collect()
}

fn bytes(value: i64) -> Option<u64> {
    u64::try_from(value).ok()
}

fn failure(reason: DownloadInterruptReason) -> DownloadFailure {
    if reason == DownloadInterruptReason::FILE_NO_SPACE {
        DownloadFailure::DiskFull
    } else if reason == DownloadInterruptReason::FILE_ACCESS_DENIED || reason == DownloadInterruptReason::FILE_BLOCKED {
        DownloadFailure::AccessDenied
    } else if [
        DownloadInterruptReason::NETWORK_FAILED,
        DownloadInterruptReason::NETWORK_TIMEOUT,
        DownloadInterruptReason::NETWORK_DISCONNECTED,
        DownloadInterruptReason::NETWORK_SERVER_DOWN,
    ]
    .contains(&reason)
    {
        DownloadFailure::Network
    } else if [
        DownloadInterruptReason::SERVER_FAILED,
        DownloadInterruptReason::SERVER_BAD_CONTENT,
        DownloadInterruptReason::SERVER_UNAUTHORIZED,
        DownloadInterruptReason::SERVER_FORBIDDEN,
        DownloadInterruptReason::SERVER_UNREACHABLE,
    ]
    .contains(&reason)
    {
        DownloadFailure::Server
    } else {
        DownloadFailure::Other
    }
}

fn state_of(item: &DownloadItem) -> DownloadState {
    if item.is_complete() != 0 {
        DownloadState::Complete
    } else if item.is_canceled() != 0 {
        DownloadState::Cancelled
    } else if item.is_interrupted() != 0 {
        DownloadState::Failed(failure(item.interrupt_reason()))
    } else if item.is_paused() != 0 {
        DownloadState::Paused
    } else {
        DownloadState::InProgress
    }
}

fn finished(state: DownloadState) -> bool {
    matches!(state, DownloadState::Complete | DownloadState::Cancelled | DownloadState::Failed(_))
}

/// Answers [`Event::DownloadBegin`]: an empty path cancels the download. CEF cancels a
/// download whose before-download callback is released without being run.
pub(super) fn download_path(download: u32, path: &str) {
    let before = DOWNLOADS.with_borrow_mut(|downloads| {
        let entry = downloads.get_mut(&download)?;
        if path.is_empty() {
            entry.refused = true;
            if let Some(control) = entry.control.as_ref() {
                control.cancel();
            }
        }
        entry.before.take()
    });
    if path.is_empty() {
        // The entry stays (marked refused) so a late update is cancelled too.
        drop(before);
        return;
    }
    if let Some(before) = before {
        before.cont(Some(&CefString::from(path)), 0);
    }
}

pub(super) fn download_control(download: u32, action: DownloadAction) {
    DOWNLOADS.with_borrow(|downloads| {
        let Some(control) = downloads.get(&download).and_then(|entry| entry.control.as_ref()) else {
            return;
        };
        match action {
            DownloadAction::Cancel => control.cancel(),
            DownloadAction::Pause => control.pause(),
            DownloadAction::Resume => control.resume(),
        }
    });
}

pub(super) fn file_chosen(request: u32, paths: &[String]) {
    let Some(callback) = FILE_DIALOGS.with_borrow_mut(|dialogs| dialogs.remove(&request)) else {
        return;
    };
    if paths.is_empty() {
        callback.cancel();
        return;
    }
    let mut chosen = CefStringList::new();
    for path in paths {
        chosen.append(path);
    }
    callback.cont(Some(&mut chosen));
}

pub(super) fn find(tab: &str, query: &str, forward: bool, match_case: bool, find_next: bool) {
    if let Some(host) = host_of(tab) {
        host.find(Some(&CefString::from(query)), forward as _, match_case as _, find_next as _);
    }
}

pub(super) fn stop_find(tab: &str, clear_selection: bool) {
    if let Some(host) = host_of(tab) {
        host.stop_finding(clear_selection as _);
    }
}

/// CEF zoom levels are steps of 20%: level = log1.2(factor).
pub(super) fn zoom(tab: &str, percent: u32) {
    let factor = f64::from(percent.clamp(25, 500)) / 100.0;
    if let Some(host) = host_of(tab) {
        host.set_zoom_level(factor.ln() / 1.2_f64.ln());
    }
}

pub(super) fn print(tab: &str) {
    if let Some(host) = host_of(tab) {
        host.print();
    }
}

pub(super) fn print_pdf(tab: &str, path: &str) {
    let Some(host) = host_of(tab) else {
        emit(Event::PdfDone { tab: tab.to_string(), ok: false, path: path.to_string() });
        return;
    };
    let settings = PdfPrintSettings { print_background: 1, ..Default::default() };
    let mut callback = PdfDone::new(tab.to_string());
    host.print_to_pdf(Some(&CefString::from(path)), Some(&settings), Some(&mut callback));
}

pub(super) fn notification_permission(tab: &str, state: PagePermission) {
    if let Some(frame) = browser_of(tab).and_then(|browser| browser.main_frame()) {
        let script = notification_permission_script(state);
        frame.execute_java_script(Some(&CefString::from(script.as_str())), None, 0);
    }
}

/// The page's Notification shim reports over the console, which the engine already sees.
fn shim_source() -> String {
    let post = format!(
        "(() => {{ const log = console.log.bind(console); return (message) => log({prefix:?} + message); }})()",
        prefix = PAGE_MESSAGE_CONSOLE_PREFIX
    );
    notification_shim(&post)
}

/// Installs the Notification shim when a tab's main frame starts loading a document.
pub(super) fn on_load_start(browser: Option<&mut Browser>, frame: Option<&mut Frame>) {
    let (Some(browser), Some(frame)) = (browser, frame) else { return };
    if frame.is_main() == 0 || !matches!(key_of(browser), Some(Key::Tab(_))) {
        return;
    }
    frame.execute_java_script(Some(&CefString::from(shim_source().as_str())), None, 0);
}

/// Turns a Notification shim console line into [`Event::PageMessage`]; true when it was one.
pub(super) fn on_console_message(browser: Option<&mut Browser>, message: Option<&CefString>) -> bool {
    let message = text(message);
    let Some(body) = message.strip_prefix(PAGE_MESSAGE_CONSOLE_PREFIX) else {
        return false;
    };
    let Some(browser) = browser else { return true };
    let url = browser.main_frame().map(|frame| user_text(frame.url())).unwrap_or_default();
    if let Some(Key::Tab(tab)) = key_of(browser) {
        emit(Event::PageMessage { tab, url, message: body.to_string() });
    }
    true
}

wrap_pdf_print_callback! {
    struct PdfDone {
        tab: String,
    }

    impl PdfPrintCallback {
        fn on_pdf_print_finished(&self, path: Option<&CefString>, ok: ::std::os::raw::c_int) {
            emit(Event::PdfDone { tab: self.tab.clone(), ok: ok != 0, path: text(path) });
        }
    }
}

wrap_download_handler! {
    pub(super) struct EngineDownloadHandler;

    impl DownloadHandler {
        fn can_download(
            &self,
            _browser: Option<&mut Browser>,
            _url: Option<&CefString>,
            _request_method: Option<&CefString>,
        ) -> ::std::os::raw::c_int {
            1
        }

        fn on_before_download(
            &self,
            browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            suggested_name: Option<&CefString>,
            callback: Option<&mut BeforeDownloadCallback>,
        ) -> ::std::os::raw::c_int {
            let (Some(item), Some(callback)) = (download_item, callback) else { return 0 };
            let download = item.id();
            DOWNLOADS.with_borrow_mut(|downloads| {
                downloads.insert(
                    download,
                    Download { before: Some(callback.clone()), control: None, last_state: None, last_reported: 0, refused: false },
                )
            });
            emit(Event::DownloadBegin {
                download,
                tab: tab_of(browser),
                total: bytes(item.total_bytes()),
                url: user_text(item.url()),
                name: text(suggested_name),
            });
            1
        }

        fn on_download_updated(
            &self,
            _browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            callback: Option<&mut DownloadItemCallback>,
        ) {
            let Some(item) = download_item else { return };
            let download = item.id();
            let state = state_of(item);
            let received = bytes(item.received_bytes()).unwrap_or(0);
            let report = DOWNLOADS.with_borrow_mut(|downloads| {
                let entry = downloads.get_mut(&download)?;
                if let Some(callback) = callback {
                    entry.control = Some(callback.clone());
                }
                if entry.refused && !finished(state) {
                    if let Some(control) = entry.control.as_ref() {
                        control.cancel();
                    }
                    return None;
                }
                let changed = entry.last_state != Some(state);
                let progressed = received.saturating_sub(entry.last_reported) >= PROGRESS_STEP_BYTES;
                if !changed && !progressed {
                    return None;
                }
                entry.last_state = Some(state);
                entry.last_reported = received;
                Some(())
            });
            if finished(state) {
                DOWNLOADS.with_borrow_mut(|downloads| downloads.remove(&download));
            }
            if report.is_some() {
                emit(Event::DownloadUpdate {
                    download,
                    received,
                    total: bytes(item.total_bytes()),
                    state,
                    path: user_text(item.full_path()),
                });
            }
        }
    }
}

wrap_find_handler! {
    pub(super) struct EngineFindHandler;

    impl FindHandler {
        fn on_find_result(
            &self,
            browser: Option<&mut Browser>,
            _identifier: ::std::os::raw::c_int,
            count: ::std::os::raw::c_int,
            _selection_rect: Option<&Rect>,
            active_match_ordinal: ::std::os::raw::c_int,
            final_update: ::std::os::raw::c_int,
        ) {
            let tab = tab_of(browser);
            if tab.is_empty() {
                return;
            }
            emit(Event::FindResult {
                tab,
                count: u32::try_from(count).unwrap_or(0),
                active: u32::try_from(active_match_ordinal).unwrap_or(0),
                final_update: final_update != 0,
            });
        }
    }
}

wrap_dialog_handler! {
    pub(super) struct EngineDialogHandler;

    impl DialogHandler {
        fn on_file_dialog(
            &self,
            browser: Option<&mut Browser>,
            mode: FileDialogMode,
            _title: Option<&CefString>,
            default_file_path: Option<&CefString>,
            accept_filters: Option<&mut CefStringList>,
            _accept_extensions: Option<&mut CefStringList>,
            _accept_descriptions: Option<&mut CefStringList>,
            callback: Option<&mut FileDialogCallback>,
        ) -> ::std::os::raw::c_int {
            let Some(callback) = callback else { return 0 };
            let tab = tab_of(browser);
            // Windows Ferryx has no tab for (DevTools, a stray) keep Chromium's own dialog.
            if tab.is_empty() {
                return 0;
            }
            let mode = if mode == FileDialogMode::OPEN_MULTIPLE {
                PaneFileDialogMode::OpenMultiple
            } else if mode == FileDialogMode::OPEN_FOLDER {
                PaneFileDialogMode::OpenFolder
            } else if mode == FileDialogMode::SAVE {
                PaneFileDialogMode::Save
            } else {
                PaneFileDialogMode::Open
            };
            let request = NEXT_DIALOG.get();
            NEXT_DIALOG.set(request.wrapping_add(1));
            FILE_DIALOGS.with_borrow_mut(|dialogs| dialogs.insert(request, callback.clone()));
            emit(Event::FileDialog {
                request,
                tab,
                mode,
                accept: list(accept_filters),
                default_path: text(default_file_path),
            });
            1
        }
    }
}
