//! Line protocol between Ferryx and its Chromium (CEF) engine process.
//!
//! One engine process serves every tab of one browser profile, so cookies, logins and
//! extensions are shared by those tabs. Ferryx writes [`Command`] lines to the engine's stdin
//! and reads [`Event`] lines from its stdout. Each line is space separated; the last field of a
//! line that carries a URL or a title takes the rest of the line. Event lines
//! start with [`EVENT_PREFIX`] so anything else Chromium prints to stdout is ignored.
//!
//! Tab ids are Ferryx browser ids (ASCII letters, digits, `:`, `.`, `_`, `-`).

/// Marks an engine stdout line as a protocol event.
pub const EVENT_PREFIX: &str = "@ferryx ";

/// What a site asks to use. On the wire a comma-separated list of the names below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PermissionKind {
    Camera,
    Microphone,
    /// Screen, window or tab capture (getDisplayMedia).
    Screen,
    Notifications,
    Geolocation,
    /// Reading the clipboard.
    Clipboard,
    /// Anything else the engine asked about in the same request.
    Other,
}

impl PermissionKind {
    pub const ALL: [Self; 7] = [
        Self::Camera,
        Self::Microphone,
        Self::Screen,
        Self::Notifications,
        Self::Geolocation,
        Self::Clipboard,
        Self::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Camera => "camera",
            Self::Microphone => "microphone",
            Self::Screen => "screen",
            Self::Notifications => "notifications",
            Self::Geolocation => "geolocation",
            Self::Clipboard => "clipboard",
            Self::Other => "other",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }
}

fn kinds_field(kinds: &[PermissionKind]) -> String {
    kinds.iter().map(|kind| kind.as_str()).collect::<Vec<_>>().join(",")
}

fn parse_kinds(field: &str) -> Option<Vec<PermissionKind>> {
    field.split(',').map(PermissionKind::parse).collect()
}

/// A pane rectangle: pixels inside the Ferryx window on Windows, screen points from the
/// top-left on macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Ferryx to engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Opens a tab's browser window at `rect`, shown or hidden.
    Open { tab: String, rect: Rect, visible: bool, url: String },
    /// Makes a window the engine opened on its own (see [`Event::Popup`]) the window of `tab`.
    Adopt { tab: String, popup: u32 },
    Bounds { tab: String, rect: Rect },
    Show { tab: String },
    Hide { tab: String },
    /// Puts every shown pane window back directly above the Ferryx window (macOS).
    Raise,
    Close { tab: String },
    Navigate { tab: String, url: String },
    Back { tab: String },
    Forward { tab: String },
    Reload { tab: String, ignore_cache: bool },
    /// Answers [`Event::PermissionRequest`] `request`: allow or deny everything it asked for.
    Permission { request: u64, allow: bool },
    // --- downloads, print, find, zoom, file chooser, page notifications ---
    /// Searches the page; the engine answers with [`Event::FindResult`].
    Find { tab: String, query: String, forward: bool, match_case: bool, find_next: bool },
    StopFind { tab: String, clear_selection: bool },
    /// Page zoom in percent of the default size.
    Zoom { tab: String, percent: u32 },
    /// Opens the engine's print dialog for the tab.
    Print { tab: String },
    /// Saves the page as a PDF file; the engine answers with [`Event::PdfDone`].
    PrintPdf { tab: String, path: String },
    /// Answers [`Event::DownloadBegin`]: where to save it, or an empty path to cancel it.
    DownloadPath { download: u32, path: String },
    DownloadControl { download: u32, action: DownloadAction },
    /// Answers [`Event::FileDialog`]; no paths cancels the dialog.
    FileChosen { request: u32, paths: Vec<String> },
    /// Tells the page's Notification shim what Ferryx decided for its origin.
    NotificationPermission { tab: String, state: PagePermission },
    /// Sends one Chrome DevTools Protocol call to the tab's browser through CEF's in-process
    /// DevTools message API (no TCP port). `params` is a JSON object on one line; the answer
    /// is [`Event::Devtools`] with the same `id`.
    Devtools { tab: String, id: u32, method: String, params: String },
}

/// Engine to Ferryx.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The tab's window exists; `handle` is its native window (HWND on Windows, 0 on macOS).
    Created { tab: String, handle: u64 },
    /// The engine opened a window on its own (a new tab, a popup); Ferryx answers with
    /// [`Command::Adopt`] once it made a tab for it.
    Popup { popup: u32, handle: u64, url: String },
    Address { tab: String, url: String },
    Title { tab: String, title: String },
    Loading { tab: String, loading: bool, can_go_back: bool, can_go_forward: bool },
    /// The tab's browser closed inside the engine (the page, a shortcut, a crash).
    Closed { tab: String },
    /// A page in `tab` asks to use `kinds`; the engine holds the request until Ferryx answers
    /// with [`Command::Permission`]. Nothing is granted without that answer.
    PermissionRequest { tab: String, request: u64, kinds: Vec<PermissionKind>, origin: String },
    /// The engine dropped an unanswered request (navigation, the tab closed): the prompt goes.
    PermissionDismissed { tab: String, request: u64 },
    /// The page in `tab` started or stopped capturing the camera (`video`) or microphone (`audio`).
    MediaAccess { tab: String, video: bool, audio: bool },
    // --- downloads, print, find, file chooser, page notifications ---
    FindResult { tab: String, count: u32, active: u32, final_update: bool },
    PdfDone { tab: String, ok: bool, path: String },
    /// The page started a download; Ferryx answers with [`Command::DownloadPath`]. `tab` is
    /// empty when the download came from a window Ferryx has no tab for.
    DownloadBegin { download: u32, tab: String, total: Option<u64>, url: String, name: String },
    DownloadUpdate { download: u32, received: u64, total: Option<u64>, state: DownloadState, path: String },
    /// The page asked for a file chooser; Ferryx answers with [`Command::FileChosen`].
    FileDialog { request: u32, tab: String, mode: FileDialogMode, accept: Vec<String>, default_path: String },
    /// A message from the page's Notification shim (JSON), with the page URL it came from.
    PageMessage { tab: String, url: String, message: String },
    /// The answer to [`Command::Devtools`]: `result` is the CDP result object on success, the
    /// CDP error object (or `{"message":...}` when the call could not be sent) otherwise.
    Devtools { tab: String, id: u32, ok: bool, result: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadAction {
    Cancel,
    Pause,
    Resume,
}

/// Why a download stopped without completing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadFailure {
    DiskFull,
    AccessDenied,
    Network,
    Server,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadState {
    InProgress,
    Paused,
    Complete,
    Cancelled,
    Failed(DownloadFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileDialogMode {
    Open,
    OpenMultiple,
    OpenFolder,
    Save,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagePermission {
    Granted,
    Denied,
    Default,
}

/// Prefix of the console line the engine's Notification shim writes; the engine turns these
/// lines into [`Event::PageMessage`] and keeps them out of the console.
pub const PAGE_MESSAGE_CONSOLE_PREFIX: &str = "@ferryx-notify ";

const NOTIFICATION_POST_PLACEHOLDER: &str = "__FERRYX_NOTIFY_POST__";
const NOTIFICATION_SHIM_TEMPLATE: &str = include_str!("notification_shim.js");

/// The page `Notification` replacement both engines install. `post` is a JS expression for a
/// function taking the JSON message string; the host answers through
/// `window.__ferryxNotificationPermission(state)`.
pub fn notification_shim(post: &str) -> String {
    NOTIFICATION_SHIM_TEMPLATE.replace(NOTIFICATION_POST_PLACEHOLDER, post)
}

/// The script that hands a permission state to the page's Notification shim.
pub fn notification_permission_script(state: PagePermission) -> String {
    format!(
        "window.__ferryxNotificationPermission && window.__ferryxNotificationPermission('{}')",
        state.token()
    )
}

impl DownloadAction {
    fn token(self) -> &'static str {
        match self {
            Self::Cancel => "cancel",
            Self::Pause => "pause",
            Self::Resume => "resume",
        }
    }

    fn parse(token: &str) -> Option<Self> {
        match token {
            "cancel" => Some(Self::Cancel),
            "pause" => Some(Self::Pause),
            "resume" => Some(Self::Resume),
            _ => None,
        }
    }
}

impl DownloadState {
    fn token(self) -> &'static str {
        match self {
            Self::InProgress => "progress",
            Self::Paused => "paused",
            Self::Complete => "complete",
            Self::Cancelled => "cancelled",
            Self::Failed(DownloadFailure::DiskFull) => "failed-nospace",
            Self::Failed(DownloadFailure::AccessDenied) => "failed-access",
            Self::Failed(DownloadFailure::Network) => "failed-network",
            Self::Failed(DownloadFailure::Server) => "failed-server",
            Self::Failed(DownloadFailure::Other) => "failed",
        }
    }

    fn parse(token: &str) -> Option<Self> {
        Some(match token {
            "progress" => Self::InProgress,
            "paused" => Self::Paused,
            "complete" => Self::Complete,
            "cancelled" => Self::Cancelled,
            "failed-nospace" => Self::Failed(DownloadFailure::DiskFull),
            "failed-access" => Self::Failed(DownloadFailure::AccessDenied),
            "failed-network" => Self::Failed(DownloadFailure::Network),
            "failed-server" => Self::Failed(DownloadFailure::Server),
            "failed" => Self::Failed(DownloadFailure::Other),
            _ => return None,
        })
    }
}

impl FileDialogMode {
    fn token(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::OpenMultiple => "multiple",
            Self::OpenFolder => "folder",
            Self::Save => "save",
        }
    }

    fn parse(token: &str) -> Option<Self> {
        match token {
            "open" => Some(Self::Open),
            "multiple" => Some(Self::OpenMultiple),
            "folder" => Some(Self::OpenFolder),
            "save" => Some(Self::Save),
            _ => None,
        }
    }
}

impl PagePermission {
    pub fn token(self) -> &'static str {
        match self {
            Self::Granted => "granted",
            Self::Denied => "denied",
            Self::Default => "default",
        }
    }

    fn parse(token: &str) -> Option<Self> {
        match token {
            "granted" => Some(Self::Granted),
            "denied" => Some(Self::Denied),
            "default" => Some(Self::Default),
            _ => None,
        }
    }
}

/// One space-free field: spaces in a URL are percent-encoded, an empty value becomes `-`.
fn token_field(value: &str) -> String {
    let value = one_line(value).replace(' ', "%20");
    if value.is_empty() {
        "-".to_string()
    } else {
        value
    }
}

fn parse_token_field(value: &str) -> String {
    if value == "-" {
        String::new()
    } else {
        value.to_string()
    }
}

fn total_field(total: Option<u64>) -> String {
    total.map_or_else(|| "-1".to_string(), |total| total.to_string())
}

fn parse_total(value: &str) -> Option<Option<u64>> {
    if value == "-1" {
        Some(None)
    } else {
        value.parse().ok().map(Some)
    }
}

/// Accept filters (`.png`, `image/*`) as one comma-separated field; a filter that holds a
/// comma or whitespace cannot be carried and is dropped.
fn accept_field(accept: &[String]) -> String {
    let kept: Vec<&str> = accept
        .iter()
        .map(String::as_str)
        .filter(|filter| !filter.is_empty() && !filter.contains([',', ' ', '\t', '\r', '\n']))
        .collect();
    if kept.is_empty() {
        "-".to_string()
    } else {
        kept.join(",")
    }
}

fn parse_accept(value: &str) -> Vec<String> {
    if value == "-" {
        Vec::new()
    } else {
        value.split(',').map(str::to_string).collect()
    }
}

/// Paths are separated by tabs; a path holding a tab or line break cannot be carried.
fn paths_field(paths: &[String]) -> String {
    paths
        .iter()
        .filter(|path| !path.is_empty() && !path.contains(['\t', '\r', '\n']))
        .cloned()
        .collect::<Vec<_>>()
        .join("\t")
}

fn parse_paths(value: &str) -> Vec<String> {
    value.split('\t').filter(|path| !path.is_empty()).map(str::to_string).collect()
}

fn one_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

fn flag(value: bool) -> &'static str {
    if value {
        "1"
    } else {
        "0"
    }
}

fn parse_flag(value: &str) -> Option<bool> {
    match value {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// Splits off `count` space-separated fields and returns them with the rest of the line.
fn fields(line: &str, count: usize) -> Option<(Vec<&str>, &str)> {
    let mut parts = Vec::with_capacity(count);
    let mut rest = line;
    for _ in 0..count {
        let (head, tail) = match rest.split_once(' ') {
            Some(split) => split,
            None => (rest, ""),
        };
        if head.is_empty() {
            return None;
        }
        parts.push(head);
        rest = tail;
    }
    Some((parts, rest))
}

fn parse_rect(parts: &[&str]) -> Option<Rect> {
    Some(Rect {
        x: parts.first()?.parse().ok()?,
        y: parts.get(1)?.parse().ok()?,
        width: parts.get(2)?.parse().ok()?,
        height: parts.get(3)?.parse().ok()?,
    })
}

fn rect_fields(rect: &Rect) -> String {
    format!("{} {} {} {}", rect.x, rect.y, rect.width, rect.height)
}

impl Command {
    pub fn to_line(&self) -> String {
        match self {
            Self::Open { tab, rect, visible, url } => {
                format!("open {tab} {} {} {}", rect_fields(rect), flag(*visible), one_line(url))
            }
            Self::Adopt { tab, popup } => format!("adopt {tab} {popup}"),
            Self::Bounds { tab, rect } => format!("bounds {tab} {}", rect_fields(rect)),
            Self::Show { tab } => format!("show {tab}"),
            Self::Hide { tab } => format!("hide {tab}"),
            Self::Raise => "raise".to_string(),
            Self::Close { tab } => format!("close {tab}"),
            Self::Navigate { tab, url } => format!("navigate {tab} {}", one_line(url)),
            Self::Back { tab } => format!("back {tab}"),
            Self::Forward { tab } => format!("forward {tab}"),
            Self::Reload { tab, ignore_cache } => format!("reload {tab} {}", flag(*ignore_cache)),
            Self::Permission { request, allow } => format!("permission {request} {}", flag(*allow)),
            Self::Find { tab, query, forward, match_case, find_next } => format!(
                "find {tab} {} {} {} {}",
                flag(*forward),
                flag(*match_case),
                flag(*find_next),
                one_line(query)
            ),
            Self::StopFind { tab, clear_selection } => format!("stopfind {tab} {}", flag(*clear_selection)),
            Self::Zoom { tab, percent } => format!("zoom {tab} {percent}"),
            Self::Print { tab } => format!("print {tab}"),
            Self::PrintPdf { tab, path } => format!("printpdf {tab} {}", one_line(path)),
            Self::DownloadPath { download, path } => format!("dlpath {download} {}", one_line(path)),
            Self::DownloadControl { download, action } => format!("dlctl {download} {}", action.token()),
            Self::FileChosen { request, paths } => format!("filechosen {request} {}", paths_field(paths)),
            Self::NotificationPermission { tab, state } => format!("notifyperm {tab} {}", state.token()),
            Self::Devtools { tab, id, method, params } => {
                format!("devtools {tab} {id} {method} {}", one_line(params))
            }
        }
    }

    pub fn parse(line: &str) -> Option<Self> {
        let (verb, rest) = line.trim_end().split_once(' ').unwrap_or((line.trim_end(), ""));
        let tab_only = |rest: &str| fields(rest, 1).map(|(parts, _)| parts[0].to_string());
        match verb {
            "open" => {
                let (parts, url) = fields(rest, 6)?;
                Some(Self::Open {
                    tab: parts[0].to_string(),
                    rect: parse_rect(&parts[1..5])?,
                    visible: parse_flag(parts[5])?,
                    url: url.to_string(),
                })
            }
            "adopt" => {
                let (parts, _) = fields(rest, 2)?;
                Some(Self::Adopt { tab: parts[0].to_string(), popup: parts[1].parse().ok()? })
            }
            "bounds" => {
                let (parts, _) = fields(rest, 5)?;
                Some(Self::Bounds { tab: parts[0].to_string(), rect: parse_rect(&parts[1..5])? })
            }
            "show" => Some(Self::Show { tab: tab_only(rest)? }),
            "hide" => Some(Self::Hide { tab: tab_only(rest)? }),
            "raise" => Some(Self::Raise),
            "close" => Some(Self::Close { tab: tab_only(rest)? }),
            "navigate" => {
                let (parts, url) = fields(rest, 1)?;
                Some(Self::Navigate { tab: parts[0].to_string(), url: url.to_string() })
            }
            "back" => Some(Self::Back { tab: tab_only(rest)? }),
            "forward" => Some(Self::Forward { tab: tab_only(rest)? }),
            "reload" => {
                let (parts, _) = fields(rest, 2)?;
                Some(Self::Reload { tab: parts[0].to_string(), ignore_cache: parse_flag(parts[1])? })
            }
            "permission" => {
                let (parts, _) = fields(rest, 2)?;
                Some(Self::Permission { request: parts[0].parse().ok()?, allow: parse_flag(parts[1])? })
            }
            "find" => {
                let (parts, query) = fields(rest, 4)?;
                Some(Self::Find {
                    tab: parts[0].to_string(),
                    forward: parse_flag(parts[1])?,
                    match_case: parse_flag(parts[2])?,
                    find_next: parse_flag(parts[3])?,
                    query: query.to_string(),
                })
            }
            "stopfind" => {
                let (parts, _) = fields(rest, 2)?;
                Some(Self::StopFind { tab: parts[0].to_string(), clear_selection: parse_flag(parts[1])? })
            }
            "zoom" => {
                let (parts, _) = fields(rest, 2)?;
                Some(Self::Zoom { tab: parts[0].to_string(), percent: parts[1].parse().ok()? })
            }
            "print" => Some(Self::Print { tab: tab_only(rest)? }),
            "printpdf" => {
                let (parts, path) = fields(rest, 1)?;
                if path.is_empty() {
                    return None;
                }
                Some(Self::PrintPdf { tab: parts[0].to_string(), path: path.to_string() })
            }
            "dlpath" => {
                let (parts, path) = fields(rest, 1)?;
                Some(Self::DownloadPath { download: parts[0].parse().ok()?, path: path.to_string() })
            }
            "dlctl" => {
                let (parts, _) = fields(rest, 2)?;
                Some(Self::DownloadControl {
                    download: parts[0].parse().ok()?,
                    action: DownloadAction::parse(parts[1])?,
                })
            }
            "filechosen" => {
                let (parts, paths) = fields(rest, 1)?;
                Some(Self::FileChosen { request: parts[0].parse().ok()?, paths: parse_paths(paths) })
            }
            "notifyperm" => {
                let (parts, _) = fields(rest, 2)?;
                Some(Self::NotificationPermission {
                    tab: parts[0].to_string(),
                    state: PagePermission::parse(parts[1])?,
                })
            }
            "devtools" => {
                let (parts, params) = fields(rest, 3)?;
                Some(Self::Devtools {
                    tab: parts[0].to_string(),
                    id: parts[1].parse().ok()?,
                    method: parts[2].to_string(),
                    params: if params.is_empty() { "{}".to_string() } else { params.to_string() },
                })
            }
            _ => None,
        }
    }
}

impl Event {
    pub fn to_line(&self) -> String {
        let body = match self {
            Self::Created { tab, handle } => format!("created {tab} {handle}"),
            Self::Popup { popup, handle, url } => format!("popup {popup} {handle} {}", one_line(url)),
            Self::Address { tab, url } => format!("address {tab} {}", one_line(url)),
            Self::Title { tab, title } => format!("title {tab} {}", one_line(title)),
            Self::Loading { tab, loading, can_go_back, can_go_forward } => format!(
                "loading {tab} {} {} {}",
                flag(*loading),
                flag(*can_go_back),
                flag(*can_go_forward)
            ),
            Self::Closed { tab } => format!("closed {tab}"),
            Self::PermissionRequest { tab, request, kinds, origin } => {
                format!("permission-request {tab} {request} {} {}", kinds_field(kinds), one_line(origin))
            }
            Self::PermissionDismissed { tab, request } => format!("permission-dismissed {tab} {request}"),
            Self::MediaAccess { tab, video, audio } => {
                format!("media-access {tab} {} {}", flag(*video), flag(*audio))
            }
            Self::FindResult { tab, count, active, final_update } => {
                format!("findresult {tab} {count} {active} {}", flag(*final_update))
            }
            Self::PdfDone { tab, ok, path } => format!("pdf {tab} {} {}", flag(*ok), one_line(path)),
            Self::DownloadBegin { download, tab, total, url, name } => format!(
                "dlbegin {download} {} {} {} {}",
                token_field(tab),
                total_field(*total),
                token_field(url),
                one_line(name)
            ),
            Self::DownloadUpdate { download, received, total, state, path } => format!(
                "dlupdate {download} {received} {} {} {}",
                total_field(*total),
                state.token(),
                one_line(path)
            ),
            Self::FileDialog { request, tab, mode, accept, default_path } => format!(
                "filedialog {request} {} {} {} {}",
                token_field(tab),
                mode.token(),
                accept_field(accept),
                one_line(default_path)
            ),
            Self::PageMessage { tab, url, message } => {
                format!("pagemsg {tab} {} {}", token_field(url), one_line(message))
            }
            Self::Devtools { tab, id, ok, result } => {
                format!("devtools {tab} {id} {} {}", flag(*ok), one_line(result))
            }
        };
        format!("{EVENT_PREFIX}{body}")
    }

    /// Parses an engine stdout line; `None` for lines that are not protocol events.
    pub fn parse(line: &str) -> Option<Self> {
        let body = line.trim_end_matches(['\r', '\n']).strip_prefix(EVENT_PREFIX)?;
        let (verb, rest) = body.split_once(' ').unwrap_or((body, ""));
        match verb {
            "created" => {
                let (parts, _) = fields(rest, 2)?;
                Some(Self::Created { tab: parts[0].to_string(), handle: parts[1].parse().ok()? })
            }
            "popup" => {
                let (parts, url) = fields(rest, 2)?;
                Some(Self::Popup {
                    popup: parts[0].parse().ok()?,
                    handle: parts[1].parse().ok()?,
                    url: url.to_string(),
                })
            }
            "address" => {
                let (parts, url) = fields(rest, 1)?;
                Some(Self::Address { tab: parts[0].to_string(), url: url.to_string() })
            }
            "title" => {
                let (parts, title) = fields(rest, 1)?;
                Some(Self::Title { tab: parts[0].to_string(), title: title.to_string() })
            }
            "loading" => {
                let (parts, _) = fields(rest, 4)?;
                Some(Self::Loading {
                    tab: parts[0].to_string(),
                    loading: parse_flag(parts[1])?,
                    can_go_back: parse_flag(parts[2])?,
                    can_go_forward: parse_flag(parts[3])?,
                })
            }
            "closed" => Some(Self::Closed { tab: fields(rest, 1)?.0[0].to_string() }),
            "permission-request" => {
                let (parts, origin) = fields(rest, 3)?;
                Some(Self::PermissionRequest {
                    tab: parts[0].to_string(),
                    request: parts[1].parse().ok()?,
                    kinds: parse_kinds(parts[2])?,
                    origin: origin.to_string(),
                })
            }
            "permission-dismissed" => {
                let (parts, _) = fields(rest, 2)?;
                Some(Self::PermissionDismissed { tab: parts[0].to_string(), request: parts[1].parse().ok()? })
            }
            "media-access" => {
                let (parts, _) = fields(rest, 3)?;
                Some(Self::MediaAccess {
                    tab: parts[0].to_string(),
                    video: parse_flag(parts[1])?,
                    audio: parse_flag(parts[2])?,
                })
            }
            "findresult" => {
                let (parts, _) = fields(rest, 4)?;
                Some(Self::FindResult {
                    tab: parts[0].to_string(),
                    count: parts[1].parse().ok()?,
                    active: parts[2].parse().ok()?,
                    final_update: parse_flag(parts[3])?,
                })
            }
            "pdf" => {
                let (parts, path) = fields(rest, 2)?;
                Some(Self::PdfDone { tab: parts[0].to_string(), ok: parse_flag(parts[1])?, path: path.to_string() })
            }
            "dlbegin" => {
                let (parts, name) = fields(rest, 4)?;
                Some(Self::DownloadBegin {
                    download: parts[0].parse().ok()?,
                    tab: parse_token_field(parts[1]),
                    total: parse_total(parts[2])?,
                    url: parse_token_field(parts[3]),
                    name: name.to_string(),
                })
            }
            "dlupdate" => {
                let (parts, path) = fields(rest, 4)?;
                Some(Self::DownloadUpdate {
                    download: parts[0].parse().ok()?,
                    received: parts[1].parse().ok()?,
                    total: parse_total(parts[2])?,
                    state: DownloadState::parse(parts[3])?,
                    path: path.to_string(),
                })
            }
            "filedialog" => {
                let (parts, default_path) = fields(rest, 4)?;
                Some(Self::FileDialog {
                    request: parts[0].parse().ok()?,
                    tab: parse_token_field(parts[1]),
                    mode: FileDialogMode::parse(parts[2])?,
                    accept: parse_accept(parts[3]),
                    default_path: default_path.to_string(),
                })
            }
            "pagemsg" => {
                let (parts, message) = fields(rest, 2)?;
                Some(Self::PageMessage {
                    tab: parts[0].to_string(),
                    url: parse_token_field(parts[1]),
                    message: message.to_string(),
                })
            }
            "devtools" => {
                let (parts, result) = fields(rest, 3)?;
                Some(Self::Devtools {
                    tab: parts[0].to_string(),
                    id: parts[1].parse().ok()?,
                    ok: parse_flag(parts[2])?,
                    result: result.to_string(),
                })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Rect {
        Rect { x: -4, y: 20, width: 800, height: 600 }
    }

    #[test]
    fn every_command_survives_a_round_trip() {
        let tab = "3ae2a12b-a92f:x.y_z".to_string();
        let commands = [
            Command::Open {
                tab: tab.clone(),
                rect: rect(),
                visible: true,
                url: "https://example.com/a b?q=1".into(),
            },
            Command::Adopt { tab: tab.clone(), popup: 7 },
            Command::Bounds { tab: tab.clone(), rect: rect() },
            Command::Show { tab: tab.clone() },
            Command::Hide { tab: tab.clone() },
            Command::Raise,
            Command::Close { tab: tab.clone() },
            Command::Navigate { tab: tab.clone(), url: "https://example.com/".into() },
            Command::Back { tab: tab.clone() },
            Command::Forward { tab: tab.clone() },
            Command::Reload { tab: tab.clone(), ignore_cache: true },
            Command::Raise,
            Command::Permission { request: u64::MAX, allow: true },
            Command::Permission { request: 0, allow: false },
            Command::Devtools {
                tab: tab.clone(),
                id: 42,
                method: "Runtime.evaluate".into(),
                params: r#"{"expression":"document.title + ' x'","returnByValue":true}"#.into(),
            },
        ];
        for command in commands {
            assert_eq!(Command::parse(&command.to_line()), Some(command));
        }
    }

    #[test]
    fn every_event_survives_a_round_trip() {
        let tab = "b1".to_string();
        let events = [
            Event::Created { tab: tab.clone(), handle: 132_456 },
            Event::Popup { popup: 3, handle: 0, url: "chrome://newtab/".into() },
            Event::Address { tab: tab.clone(), url: "https://a.test/".into() },
            Event::Title { tab: tab.clone(), title: "Inbox (3) - mail".into() },
            Event::Loading { tab: tab.clone(), loading: false, can_go_back: true, can_go_forward: false },
            Event::PermissionRequest {
                tab: tab.clone(),
                request: 42,
                kinds: vec![PermissionKind::Camera, PermissionKind::Microphone],
                origin: "https://meet.example.com:8443".into(),
            },
            Event::PermissionRequest {
                tab: tab.clone(),
                request: 43,
                kinds: PermissionKind::ALL.to_vec(),
                origin: "https://a.test".into(),
            },
            Event::PermissionDismissed { tab: tab.clone(), request: 42 },
            Event::MediaAccess { tab: tab.clone(), video: true, audio: false },
            Event::Devtools {
                tab: tab.clone(),
                id: 9,
                ok: true,
                result: r#"{"result":{"type":"string","value":"a b"}}"#.into(),
            },
            Event::Devtools { tab: tab.clone(), id: 10, ok: false, result: r#"{"message":"no browser"}"#.into() },
            Event::Closed { tab },
        ];
        for event in events {
            assert_eq!(Event::parse(&event.to_line()), Some(event));
        }
    }

    #[test]
    fn an_empty_title_and_a_line_break_in_a_title_stay_on_one_line() {
        let empty = Event::Title { tab: "b1".into(), title: String::new() };
        assert_eq!(Event::parse(&empty.to_line()), Some(empty));
        let broken = Event::Title { tab: "b1".into(), title: "a\nb".into() };
        assert!(!broken.to_line().contains('\n'));
        assert_eq!(
            Event::parse(&broken.to_line()),
            Some(Event::Title { tab: "b1".into(), title: "a b".into() })
        );
    }

    #[test]
    fn lines_without_the_prefix_or_with_bad_fields_are_ignored() {
        assert_eq!(Event::parse("launch browser process"), None);
        assert_eq!(Event::parse("@ferryx created b1 not-a-number"), None);
        assert_eq!(Event::parse("@ferryx loading b1 1 2 0"), None);
        assert_eq!(Event::parse("@ferryx unknown b1"), None);
        assert_eq!(Command::parse("open b1 0 0 800"), None);
        assert_eq!(Command::parse("show"), None);
        assert_eq!(Command::parse("dance b1"), None);
        assert_eq!(Command::parse("permission 7 yes"), None);
        assert_eq!(Command::parse("permission x 1"), None);
        assert_eq!(Event::parse("@ferryx permission-request b1 7 camera,teleport https://a.test"), None);
        assert_eq!(Event::parse("@ferryx media-access b1 1"), None);
    }

    #[test]
    fn a_crlf_line_from_a_windows_pipe_parses() {
        assert_eq!(
            Event::parse("@ferryx closed b1\r\n"),
            Some(Event::Closed { tab: "b1".into() })
        );
        assert_eq!(Command::parse("hide b1\r"), Some(Command::Hide { tab: "b1".into() }));
    }

    #[test]
    fn every_system_command_survives_a_round_trip() {
        let tab = "b1".to_string();
        let commands = [
            Command::Find {
                tab: tab.clone(),
                query: "two  words ".trim_end().into(),
                forward: false,
                match_case: true,
                find_next: true,
            },
            Command::StopFind { tab: tab.clone(), clear_selection: true },
            Command::Zoom { tab: tab.clone(), percent: 125 },
            Command::Print { tab: tab.clone() },
            Command::PrintPdf { tab: tab.clone(), path: "/Users/me/My Docs/page.pdf".into() },
            Command::DownloadPath { download: 4, path: "C:\\Users\\me\\Downloads\\a b.zip".into() },
            Command::DownloadPath { download: 4, path: String::new() },
            Command::DownloadControl { download: 4, action: DownloadAction::Pause },
            Command::DownloadControl { download: 4, action: DownloadAction::Resume },
            Command::DownloadControl { download: 4, action: DownloadAction::Cancel },
            Command::FileChosen { request: 9, paths: vec!["/tmp/a b.png".into(), "/tmp/c.txt".into()] },
            Command::FileChosen { request: 9, paths: Vec::new() },
            Command::NotificationPermission { tab, state: PagePermission::Granted },
        ];
        for command in commands {
            assert_eq!(Command::parse(&command.to_line()), Some(command));
        }
    }

    #[test]
    fn every_system_event_survives_a_round_trip() {
        let tab = "b1".to_string();
        let events = [
            Event::FindResult { tab: tab.clone(), count: 12, active: 3, final_update: true },
            Event::PdfDone { tab: tab.clone(), ok: true, path: "/tmp/My Page.pdf".into() },
            Event::PdfDone { tab: tab.clone(), ok: false, path: String::new() },
            Event::DownloadBegin {
                download: 7,
                tab: tab.clone(),
                total: Some(1 << 33),
                url: "https://a.test/f?x=1".into(),
                name: "My File (1).zip".into(),
            },
            Event::DownloadBegin { download: 8, tab: String::new(), total: None, url: String::new(), name: "x".into() },
            Event::DownloadUpdate {
                download: 7,
                received: 1024,
                total: None,
                state: DownloadState::Paused,
                path: "/tmp/My File (1).zip".into(),
            },
            Event::DownloadUpdate {
                download: 7,
                received: 0,
                total: Some(10),
                state: DownloadState::Failed(DownloadFailure::DiskFull),
                path: String::new(),
            },
            Event::FileDialog {
                request: 2,
                tab: tab.clone(),
                mode: FileDialogMode::OpenMultiple,
                accept: vec![".png".into(), "image/*".into()],
                default_path: "/tmp/a b".into(),
            },
            Event::FileDialog {
                request: 3,
                tab: tab.clone(),
                mode: FileDialogMode::Save,
                accept: Vec::new(),
                default_path: String::new(),
            },
            Event::PageMessage {
                tab,
                url: "https://mail.test/inbox".into(),
                message: r#"{"op":"show","title":"New mail","body":"Hi there"}"#.into(),
            },
        ];
        for event in events {
            assert_eq!(Event::parse(&event.to_line()), Some(event));
        }
    }

    #[test]
    fn a_url_with_spaces_stays_one_field_and_bad_system_lines_are_ignored() {
        let event = Event::DownloadBegin {
            download: 1,
            tab: "b1".into(),
            total: None,
            url: "https://a.test/a b".into(),
            name: "a b".into(),
        };
        assert_eq!(
            Event::parse(&event.to_line()),
            Some(Event::DownloadBegin {
                download: 1,
                tab: "b1".into(),
                total: None,
                url: "https://a.test/a%20b".into(),
                name: "a b".into(),
            })
        );
        assert_eq!(Event::parse("@ferryx dlupdate 1 5 -1 exploded /tmp/x"), None);
        assert_eq!(Event::parse("@ferryx filedialog 1 b1 sideways - "), None);
        assert_eq!(Command::parse("dlctl 1 rewind"), None);
        assert_eq!(Command::parse("printpdf b1"), None);
        assert_eq!(Command::parse("zoom b1 big"), None);
    }

    #[test]
    fn the_notification_shim_carries_the_post_function() {
        let shim = notification_shim("window.__post");
        assert!(!shim.contains(NOTIFICATION_POST_PLACEHOLDER));
        assert!(shim.contains("window.__post"));
        assert_eq!(
            notification_permission_script(PagePermission::Denied),
            "window.__ferryxNotificationPermission && window.__ferryxNotificationPermission('denied')"
        );
    }

    #[test]
    fn a_devtools_call_without_params_gets_an_empty_object_and_a_bad_id_is_refused() {
        assert_eq!(
            Command::parse("devtools b1 3 Page.captureScreenshot"),
            Some(Command::Devtools {
                tab: "b1".into(),
                id: 3,
                method: "Page.captureScreenshot".into(),
                params: "{}".into(),
            })
        );
        assert_eq!(Command::parse("devtools b1 x Page.enable {}"), None);
        assert_eq!(Event::parse("@ferryx devtools b1 3 2 {}"), None);
    }
}
