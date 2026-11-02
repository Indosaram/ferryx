//! The engine: one CEF browser process serving every tab of one Ferryx browser profile.
//!
//! Ferryx drives it over stdin and stdout with the line protocol in
//! `ferryx-browser-engine-protocol`. Each tab is a Chrome-style browser (toolbar, extension
//! buttons, menus) in a frameless window the platform module keeps over the tab's pane. Windows
//! the browser opens on its own (a new tab, a sign-in popup) are reported to Ferryx, which makes
//! a tab for them and adopts them.

use cef::*;
use ferryx_browser_engine_protocol::{Command, Event, Rect as PaneRect};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{BufRead, Write};

mod platform;
mod permissions;
mod system;
mod import_cookies;
mod vault;

#[cfg(target_os = "linux")]
mod linux_sandbox;

#[cfg(target_os = "macos")]
pub mod mac_app;

#[cfg(target_os = "macos")]
pub type Library = library_loader::LibraryLoader;
#[cfg(not(target_os = "macos"))]
pub struct Library;

pub fn load_cef() -> Library {
    #[cfg(target_os = "macos")]
    let library = {
        let loader = library_loader::LibraryLoader::new(&std::env::current_exe().unwrap(), false);
        assert!(loader.load());
        loader
    };
    #[cfg(not(target_os = "macos"))]
    let library = Library;
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    #[cfg(target_os = "macos")]
    mac_app::setup_application();
    library
}

/// Runs a CEF subprocess, or the browser process until Ferryx closes stdin.
pub fn run(main_args: &MainArgs, cmd_line: &CommandLine, sandbox_info: *mut u8) {
    let is_browser_process = cmd_line.has_switch(Some(&CefString::from("type"))) != 1;
    let code = execute_process(Some(main_args), None, sandbox_info);
    if !is_browser_process {
        assert!(code >= 0, "cannot execute non-browser process");
        return;
    }
    assert_eq!(code, -1, "cannot execute browser process");

    let mut app = EngineApp::new();
    // Without a cache path the default profile keeps cookies in memory only, so logins would not
    // survive a restart; with none set at all (a private profile) everything stays in memory.
    let cache_dir = std::env::var("FERRYX_CEF_CACHE").ok();
    if let Some(dir) = cache_dir.as_deref() {
        // Chrome's own session (the windows open when it last stopped) is not Ferryx's tab
        // list; left in place, a crash or kill brings those windows back as extra tabs.
        let sessions = std::path::Path::new(dir).join("Default").join("Sessions");
        if let Err(error) = std::fs::remove_dir_all(&sessions)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("ferryx-engine: could not clear {}: {error}", sessions.display());
        }
    }
    let cache = cache_dir.map(|dir| CefString::from(dir.as_str())).unwrap_or_default();
    // Linux installs live in the user's data dir, where the setuid sandbox cannot exist: the
    // namespace sandbox is used, and only a kernel without user namespaces runs unsandboxed.
    #[cfg(target_os = "linux")]
    let sandboxed = cfg!(feature = "sandbox") && linux_sandbox::sandbox_usable();
    #[cfg(not(target_os = "linux"))]
    let sandboxed = cfg!(feature = "sandbox");
    let settings = Settings {
        no_sandbox: (!sandboxed) as _,
        persist_session_cookies: 1,
        root_cache_path: cache.clone(),
        cache_path: cache,
        ..Default::default()
    };
    assert_eq!(initialize(Some(main_args), Some(&settings), Some(&mut app), sandbox_info), 1);
    #[cfg(target_os = "macos")]
    let _delegate = mac_app::setup_delegate();
    run_message_loop();
    shutdown();
}

/// Which pane a browser window belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Key {
    Tab(String),
    /// Opened by the browser itself and not yet adopted by Ferryx.
    Popup(u32),
    /// DevTools: an ordinary window of its own.
    Free,
}

struct Pane {
    view: BrowserView,
    window: Option<Window>,
    rect: PaneRect,
    visible: bool,
    url: String,
    title: String,
    loading: Option<(bool, bool, bool)>,
    /// Keeps the tab's DevTools message observer registered (dropping it unregisters).
    devtools: Option<Registration>,
}

impl Pane {
    fn new(view: BrowserView, rect: PaneRect, visible: bool) -> Self {
        Self {
            view,
            window: None,
            rect,
            visible,
            url: String::new(),
            title: String::new(),
            loading: None,
            devtools: None,
        }
    }
}

thread_local! {
    static TABS: RefCell<HashMap<String, Pane>> = RefCell::new(HashMap::new());
    static POPUPS: RefCell<HashMap<u32, Pane>> = RefCell::new(HashMap::new());
    /// DevTools windows: ordinary windows Ferryx does not track.
    static DEVTOOLS: RefCell<Vec<BrowserView>> = const { RefCell::new(Vec::new()) };
    /// Browsers Chrome's UI created that have no address yet: moved once they have one.
    static STRAYS: RefCell<Vec<Browser>> = const { RefCell::new(Vec::new()) };
    static NEXT_POPUP: Cell<u32> = const { Cell::new(1) };
    static LIVE_BROWSERS: Cell<usize> = const { Cell::new(0) };
    static QUITTING: Cell<bool> = const { Cell::new(false) };
    /// DevTools calls in flight: message id to the Ferryx tab that asked.
    static DEVTOOLS_CALLS: RefCell<HashMap<u32, String>> = RefCell::new(HashMap::new());
    /// Tabs whose `focus` command waits for their window to become active (see `focus_tab`).
    static PENDING_FOCUS: RefCell<HashMap<String, bool>> = RefCell::new(HashMap::new());
}

fn emit(event: Event) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", event.to_line());
    let _ = out.flush();
}

fn same(view: &BrowserView, browser: &mut Browser) -> bool {
    if view.browser().is_some_and(|candidate| candidate.is_same(Some(browser)) != 0) {
        return true;
    }
    // In on_after_created a view's browser() can still be unset; matching on the view itself
    // keeps an engine window from being taken for a stray and rehomed again (an endless loop).
    browser_view_get_for_browser(Some(browser))
        .is_some_and(|other| View::from(view).is_same(Some(&mut View::from(&other))) != 0)
}

fn key_of(browser: &mut Browser) -> Option<Key> {
    let tab = TABS.with_borrow(|tabs| {
        tabs.iter().find(|(_, pane)| same(&pane.view, browser)).map(|(tab, _)| tab.clone())
    });
    if let Some(tab) = tab {
        return Some(Key::Tab(tab));
    }
    POPUPS.with_borrow(|popups| {
        popups.iter().find(|(_, pane)| same(&pane.view, browser)).map(|(id, _)| Key::Popup(*id))
    })
    .or_else(|| DEVTOOLS.with_borrow(|views| views.iter().any(|view| same(view, browser)).then_some(Key::Free)))
}

/// Reports a window to Ferryx as one the browser opened on its own; Ferryx gives it a tab.
fn hold_popup(view: BrowserView, url: &str) {
    let id = NEXT_POPUP.get();
    NEXT_POPUP.set(id + 1);
    let mut pane = Pane::new(view.clone(), PaneRect::default(), false);
    pane.url = url.to_string();
    POPUPS.with_borrow_mut(|popups| popups.insert(id, pane));
    let mut window_delegate = EngineWindowDelegate::new(RefCell::new(Some(view)), Key::Popup(id));
    window_create_top_level(Some(&mut window_delegate));
}

/// Chrome's own UI (a new tab after an extension install, "Open link in new tab", the menu)
/// creates browsers in Chrome windows of their own, outside every pane. The page moves into a
/// browser of the engine's, which Ferryx adopts as a tab, and the Chrome window closes.
fn rehome(browser: &mut Browser, url: &str) {
    if let Some(host) = browser.host() {
        host.close_browser(1);
    }
    let mut client = EngineClient::new();
    let mut delegate = EngineBrowserViewDelegate::new(RuntimeStyle::CHROME);
    if let Some(view) = browser_view_create(
        Some(&mut client),
        Some(&CefString::from(url)),
        Some(&BrowserSettings::default()),
        None,
        None,
        Some(&mut delegate),
    ) {
        hold_popup(view, url);
    }
}

fn with_pane<T>(key: &Key, f: impl FnOnce(&mut Pane) -> T) -> Option<T> {
    match key {
        Key::Tab(tab) => TABS.with_borrow_mut(|tabs| tabs.get_mut(tab).map(f)),
        Key::Popup(id) => POPUPS.with_borrow_mut(|popups| popups.get_mut(id).map(f)),
        Key::Free => None,
    }
}

fn place(pane: &Pane) {
    if let Some(window) = pane.window.as_ref() {
        platform::place(window, pane.rect, pane.visible);
    }
}

fn browser_of(tab: &str) -> Option<Browser> {
    TABS.with_borrow(|tabs| tabs.get(tab).and_then(|pane| pane.view.browser()))
}

fn open(tab: String, rect: PaneRect, visible: bool, url: &str) {
    if TABS.with_borrow(|tabs| tabs.contains_key(&tab)) {
        return;
    }
    let mut client = EngineClient::new();
    let mut delegate = EngineBrowserViewDelegate::new(RuntimeStyle::CHROME);
    let Some(view) = browser_view_create(
        Some(&mut client),
        Some(&CefString::from(url)),
        Some(&BrowserSettings::default()),
        None,
        None,
        Some(&mut delegate),
    ) else {
        emit(Event::Closed { tab });
        return;
    };
    TABS.with_borrow_mut(|tabs| tabs.insert(tab.clone(), Pane::new(view.clone(), rect, visible)));
    let mut window_delegate = EngineWindowDelegate::new(RefCell::new(Some(view)), Key::Tab(tab));
    window_create_top_level(Some(&mut window_delegate));
}

fn adopt(tab: String, popup: u32) {
    let Some(pane) = POPUPS.with_borrow_mut(|popups| popups.remove(&popup)) else {
        emit(Event::Closed { tab });
        return;
    };
    let (url, title, loading) = (pane.url.clone(), pane.title.clone(), pane.loading);
    TABS.with_borrow_mut(|tabs| tabs.insert(tab.clone(), pane));
    // What the page reported before Ferryx had a tab for it.
    if !url.is_empty() {
        emit(Event::Address { tab: tab.clone(), url });
    }
    if !title.is_empty() {
        emit(Event::Title { tab: tab.clone(), title });
    }
    if let Some((loading, can_go_back, can_go_forward)) = loading {
        emit(Event::Loading { tab, loading, can_go_back, can_go_forward });
    }
}

fn close(tab: &str) {
    // Ferryx closed the tab: requests it held are denied (dropped) and their prompts go.
    permissions::forget_tab(tab);
    PENDING_FOCUS.with_borrow_mut(|pending| pending.remove(tab));
    if let Some(pane) = TABS.with_borrow_mut(|tabs| tabs.remove(tab))
        && let Some(browser) = pane.view.browser()
        && let Some(host) = browser.host()
    {
        host.close_browser(1);
    }
}

/// A CDP method name: `Domain.method`, ASCII letters only around one or more dots.
fn is_cdp_method(method: &str) -> bool {
    !method.is_empty()
        && method.contains('.')
        && method.split('.').all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric()))
}

fn devtools_failure(tab: String, id: u32, message: &str) {
    let escaped: String = message.chars().filter(|c| !matches!(c, '"' | '\\') && !c.is_control()).collect();
    emit(Event::Devtools { tab, id, ok: false, result: format!("{{\"message\":\"{escaped}\"}}") });
}

/// Runs one CDP call on the tab's browser through CEF's in-process DevTools message API; the
/// answer arrives in [`EngineDevtoolsObserver`]. No DevTools port is opened.
fn devtools(tab: String, id: u32, method: &str, params: &str) {
    if !is_cdp_method(method) || !params.trim_start().starts_with('{') || i32::try_from(id).is_err() {
        devtools_failure(tab, id, "malformed devtools call");
        return;
    }
    let Some(host) = browser_of(&tab).and_then(|browser| browser.host()) else {
        devtools_failure(tab, id, "the tab has no browser");
        return;
    };
    with_pane(&Key::Tab(tab.clone()), |pane| {
        if pane.devtools.is_none() {
            let mut observer = EngineDevtoolsObserver::new();
            pane.devtools = host.add_dev_tools_message_observer(Some(&mut observer));
        }
    });
    DEVTOOLS_CALLS.with_borrow_mut(|calls| calls.insert(id, tab.clone()));
    let message = format!("{{\"id\":{id},\"method\":\"{method}\",\"params\":{params}}}");
    if host.send_dev_tools_message(Some(message.as_bytes())) == 0 {
        DEVTOOLS_CALLS.with_borrow_mut(|calls| calls.remove(&id));
        devtools_failure(tab, id, "the browser refused the devtools call");
    }
}

fn execute(command: Command) {
    match command {
        Command::Open { tab, rect, visible, url } => open(tab, rect, visible, &url),
        Command::Adopt { tab, popup } => adopt(tab, popup),
        Command::Bounds { tab, rect } => {
            with_pane(&Key::Tab(tab), |pane| {
                pane.rect = rect;
                place(pane);
            });
        }
        Command::Show { tab } => {
            with_pane(&Key::Tab(tab), |pane| {
                pane.visible = true;
                place(pane);
            });
        }
        Command::Hide { tab } => {
            with_pane(&Key::Tab(tab), |pane| {
                pane.visible = false;
                place(pane);
            });
        }
        Command::Raise => TABS.with_borrow(|tabs| {
            for pane in tabs.values().filter(|pane| pane.visible) {
                if let Some(window) = pane.window.as_ref() {
                    platform::raise(window);
                }
            }
        }),
        Command::Close { tab } => close(&tab),
        Command::Navigate { tab, url } => {
            if let Some(frame) = browser_of(&tab).and_then(|browser| browser.main_frame()) {
                frame.load_url(Some(&CefString::from(url.as_str())));
            }
        }
        Command::Back { tab } => {
            if let Some(browser) = browser_of(&tab) {
                browser.go_back();
            }
        }
        Command::Forward { tab } => {
            if let Some(browser) = browser_of(&tab) {
                browser.go_forward();
            }
        }
        Command::Reload { tab, ignore_cache } => {
            if let Some(browser) = browser_of(&tab) {
                if ignore_cache {
                    browser.reload_ignore_cache();
                } else {
                    browser.reload();
                }
            }
        }
        Command::Permission { request, allow } => permissions::answer(request, allow),
        Command::Find { tab, query, forward, match_case, find_next } => {
            system::find(&tab, &query, forward, match_case, find_next)
        }
        Command::StopFind { tab, clear_selection } => system::stop_find(&tab, clear_selection),
        Command::Zoom { tab, percent } => system::zoom(&tab, percent),
        Command::Print { tab } => system::print(&tab),
        Command::PrintPdf { tab, path } => system::print_pdf(&tab, &path),
        Command::DownloadPath { download, path } => system::download_path(download, &path),
        Command::DownloadControl { download, action } => system::download_control(download, action),
        Command::FileChosen { request, paths } => system::file_chosen(request, &paths),
        Command::NotificationPermission { tab, state } => system::notification_permission(&tab, state),
        Command::Devtools { tab, id, method, params } => devtools(tab, id, &method, &params),
        Command::ImportCookies { path } => {
            import_cookies::apply(std::path::Path::new(&path));
        }
        Command::Fill { tab, script } => {
            if let Some(browser) = browser_of(&tab) {
                vault::fill(&browser, &script);
            }
        }
        Command::Focus { tab, address } => {
            let window = TABS.with_borrow(|tabs| tabs.get(&tab).and_then(|pane| pane.window.clone()));
            let Some(window) = window else {
                eprintln!("ferryx-engine: focus {tab}: the tab has no window yet");
                return;
            };
            // Moving X focus into the window activates its widget only once Chromium handles the
            // FocusIn, and activation restores the widget's last focused view: a view focused
            // before that (Chrome's address bar) loses focus again. An inactive window gets its
            // view focused from `on_window_activation_changed` instead.
            let active = window.is_active() != 0;
            eprintln!("ferryx-engine: focus {tab} address={address}: window active {active}");
            if active {
                focus_tab(&tab, address);
            } else {
                PENDING_FOCUS.with_borrow_mut(|pending| pending.insert(tab.clone(), address));
                platform::focus(&window);
            }
        }
    }
}

/// Focuses Chrome's address bar (`address`) or the page of a tab whose window is active.
fn focus_tab(tab: &str, address: bool) {
    let Some(host) = browser_of(tab).and_then(|browser| browser.host()) else { return };
    let window = TABS.with_borrow(|tabs| tabs.get(tab).and_then(|pane| pane.window.clone()));
    let focused_view = || {
        window
            .as_ref()
            .and_then(|window| window.focused_view())
            .map(|view| CefString::from(&view.type_string()).to_string())
    };
    if !address {
        host.set_focus(1);
        eprintln!("ferryx-engine: focus {tab}: focused view now {:?}", focused_view());
        return;
    }
    let can = host.can_execute_chrome_command(sys::IDC_FOCUS_LOCATION);
    host.execute_chrome_command(sys::IDC_FOCUS_LOCATION, WindowOpenDisposition::CURRENT_TAB);
    let after_command = focused_view();
    eprintln!("ferryx-engine: focus {tab} address: IDC_FOCUS_LOCATION enabled {can}, focused view now {after_command:?}");
    // The page's BrowserView still focused means the command did not reach the omnibox: press
    // Chrome's own Ctrl+L in the (active) window instead.
    if after_command.as_deref() == Some("BrowserView")
        && let Some(window) = window.as_ref()
    {
        // VKEY_L = 0x4C; EVENTFLAG_CONTROL_DOWN = 1 << 2.
        window.send_key_press(0x4C, 1 << 2);
        eprintln!("ferryx-engine: focus {tab} address: sent Ctrl+L, focused view now {:?}", focused_view());
    }
}

/// Ferryx closed stdin (it quit or dropped this engine): close every browser, then quit.
fn quit() {
    QUITTING.set(true);
    let views: Vec<BrowserView> = TABS
        .with_borrow_mut(|tabs| tabs.drain().map(|(_, pane)| pane.view).collect::<Vec<_>>())
        .into_iter()
        .chain(POPUPS.with_borrow_mut(|popups| popups.drain().map(|(_, pane)| pane.view).collect::<Vec<_>>()))
        .collect();
    for view in views {
        if let Some(host) = view.browser().and_then(|browser| browser.host()) {
            host.close_browser(1);
        }
    }
    if LIVE_BROWSERS.get() == 0 {
        quit_message_loop();
    }
}

fn read_commands() {
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if let Some(command) = Command::parse(&line) {
            let mut task = RunCommand::new(RefCell::new(Some(command)));
            post_task(ThreadId::UI, Some(&mut task));
        }
    }
    let mut task = Quit::new();
    post_task(ThreadId::UI, Some(&mut task));
}

wrap_task! {
    struct FocusTab {
        tab: String,
        address: bool,
    }

    impl Task {
        fn execute(&self) {
            focus_tab(&self.tab, self.address);
        }
    }
}

wrap_task! {
    struct RunCommand {
        command: RefCell<Option<Command>>,
    }

    impl Task {
        fn execute(&self) {
            if let Some(command) = self.command.borrow_mut().take() {
                execute(command);
            }
        }
    }
}

wrap_task! {
    struct Quit;

    impl Task {
        fn execute(&self) {
            quit();
        }
    }
}

wrap_app! {
    struct EngineApp;

    impl App {
        fn on_before_command_line_processing(
            &self,
            process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            // Ferryx owns the tab list and restores it itself: Chrome reopening its last session
            // (after a crash or kill) would add stray windows on top of Ferryx's tabs.
            let browser_process = process_type.is_none_or(|kind| kind.to_string().is_empty());
            if let (true, Some(command_line)) = (browser_process, command_line) {
                for switch in ["no-startup-window", "disable-session-crashed-bubble", "hide-crash-restore-bubble"] {
                    command_line.append_switch(Some(&CefString::from(switch)));
                }
                // Linux: the pane windows are reparented into Ferryx's X window, so Chromium
                // must use X11 (XWayland in a Wayland session), never its Wayland backend.
                #[cfg(target_os = "linux")]
                if command_line.has_switch(Some(&CefString::from("ferryx-parent-xid"))) == 1 {
                    command_line.append_switch_with_value(
                        Some(&CefString::from("ozone-platform")),
                        Some(&CefString::from("x11")),
                    );
                }
            }
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(EngineBrowserProcessHandler::new())
        }
    }
}

wrap_browser_process_handler! {
    struct EngineBrowserProcessHandler;

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            if let Ok(dir) = std::env::var("FERRYX_CEF_CACHE") {
                import_cookies::apply_pending(std::path::Path::new(&dir));
            }
            std::thread::spawn(read_commands);
        }

        fn default_client(&self) -> Option<Client> {
            // Used for the browsers Chrome's own UI creates; `on_after_created` moves them.
            Some(EngineClient::new())
        }
    }
}

wrap_window_delegate! {
    struct EngineWindowDelegate {
        view: RefCell<Option<BrowserView>>,
        key: Key,
    }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            Size { width: 800, height: 600 }
        }
    }

    impl PanelDelegate {}

    impl WindowDelegate {
        fn on_window_created(&self, window: Option<&mut Window>) {
            let view = self.view.borrow();
            let (Some(window), Some(view)) = (window, view.as_ref()) else {
                return;
            };
            // Chrome style hands its toolbar (address bar, extension buttons, menu) to the
            // window: stack it above the page.
            let layout = window.set_to_box_layout(Some(&BoxLayoutSettings { horizontal: 0, ..Default::default() }));
            let mut page = View::from(view);
            window.add_child_view(Some(&mut page));
            if let Some(layout) = layout {
                layout.set_flex_for_view(Some(&mut page), 1);
            }
            if let Some(mut toolbar) = view.chrome_toolbar() {
                window.add_child_view_at(Some(&mut toolbar), 0);
                toolbar.set_visible(1);
            }
            window.layout();

            match &self.key {
                Key::Free => window.show(),
                key => {
                    platform::embed(window);
                    let handle = platform::handle(window);
                    let stored = with_pane(key, |pane| {
                        pane.window = Some(window.clone());
                        place(pane);
                    });
                    match key {
                        Key::Tab(tab) if stored.is_some() => emit(Event::Created { tab: tab.clone(), handle }),
                        Key::Popup(popup) => emit(Event::Popup {
                            popup: *popup,
                            handle,
                            url: view
                                .browser()
                                .and_then(|browser| browser.main_frame())
                                .map(|frame| CefString::from(&frame.url()).to_string())
                                .unwrap_or_default(),
                        }),
                        _ => {}
                    }
                }
            }
        }

        fn on_window_destroyed(&self, _window: Option<&mut Window>) {
            self.view.borrow_mut().take();
        }

        fn on_window_activation_changed(&self, _window: Option<&mut Window>, active: ::std::os::raw::c_int) {
            let Key::Tab(tab) = &self.key else { return };
            if active == 0 {
                return;
            }
            if let Some(address) = PENDING_FOCUS.with_borrow_mut(|pending| pending.remove(tab)) {
                eprintln!("ferryx-engine: focus {tab} address={address}: window activated");
                // Activation restores the widget's stored focused view after this callback
                // returns, so the view is focused from a task that runs once that is done.
                let mut task = FocusTab::new(tab.clone(), address);
                post_task(ThreadId::UI, Some(&mut task));
            }
        }

        fn is_frameless(&self, _window: Option<&mut Window>) -> ::std::os::raw::c_int {
            (self.key != Key::Free) as _
        }

        fn can_close(&self, _window: Option<&mut Window>) -> ::std::os::raw::c_int {
            match self.view.borrow().as_ref().and_then(|view| view.browser()).and_then(|browser| browser.host()) {
                Some(host) => host.try_close_browser(),
                None => 1,
            }
        }

        fn window_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::CHROME
        }
    }
}

wrap_browser_view_delegate! {
    struct EngineBrowserViewDelegate {
        runtime_style: RuntimeStyle,
    }

    impl ViewDelegate {}

    impl BrowserViewDelegate {
        fn chrome_toolbar_type(&self, _browser_view: Option<&mut BrowserView>) -> ChromeToolbarType {
            ChromeToolbarType::NORMAL
        }

        fn on_popup_browser_view_created(
            &self,
            _browser_view: Option<&mut BrowserView>,
            popup_browser_view: Option<&mut BrowserView>,
            is_devtools: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            let Some(view) = popup_browser_view.cloned() else {
                return 0;
            };
            if is_devtools != 0 {
                DEVTOOLS.with_borrow_mut(|views| views.push(view.clone()));
                let mut window_delegate = EngineWindowDelegate::new(RefCell::new(Some(view)), Key::Free);
                window_create_top_level(Some(&mut window_delegate));
            } else {
                hold_popup(view, "");
            }
            1
        }

        fn browser_runtime_style(&self) -> RuntimeStyle {
            self.runtime_style
        }
    }
}

wrap_client! {
    struct EngineClient;

    impl Client {
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(EngineDisplayHandler::new())
        }

        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(EngineLifeSpanHandler::new())
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(EngineLoadHandler::new())
        }

        fn permission_handler(&self) -> Option<PermissionHandler> {
            Some(permissions::EnginePermissionHandler::new())
        }

        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(system::EngineDownloadHandler::new())
        }

        fn find_handler(&self) -> Option<FindHandler> {
            Some(system::EngineFindHandler::new())
        }

        fn dialog_handler(&self) -> Option<DialogHandler> {
            Some(system::EngineDialogHandler::new())
        }
    }
}

wrap_display_handler! {
    struct EngineDisplayHandler;

    impl DisplayHandler {
        fn on_address_change(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            let (Some(browser), Some(frame)) = (browser, frame) else { return };
            if frame.is_main() == 0 {
                return;
            }
            let url = url.map(CefString::to_string).unwrap_or_default();
            let key = key_of(browser);
            let stray = STRAYS.with_borrow_mut(|strays| {
                let before = strays.len();
                strays.retain(|candidate| candidate.is_same(Some(browser)) == 0);
                strays.len() != before
            });
            // Only now is an engine view's browser known for certain: a browser still without a
            // pane here is one Chrome's own UI opened.
            if stray && key.is_none() {
                rehome(browser, &url);
                return;
            }
            let Some(key) = key else { return };
            with_pane(&key, |pane| pane.url = url.clone());
            if let Key::Tab(tab) = key {
                emit(Event::Address { tab, url });
            }
        }

        fn on_title_change(&self, browser: Option<&mut Browser>, title: Option<&CefString>) {
            let Some(browser) = browser else { return };
            let title = title.map(CefString::to_string).unwrap_or_default();
            let Some(key) = key_of(browser) else { return };
            with_pane(&key, |pane| pane.title = title.clone());
            if let Key::Tab(tab) = key {
                emit(Event::Title { tab, title });
            }
        }

        fn on_media_access_change(
            &self,
            browser: Option<&mut Browser>,
            has_video_access: ::std::os::raw::c_int,
            has_audio_access: ::std::os::raw::c_int,
        ) {
            let Some(browser) = browser else { return };
            if let Some(Key::Tab(tab)) = key_of(browser) {
                emit(Event::MediaAccess { tab, video: has_video_access != 0, audio: has_audio_access != 0 });
            }
        }

        fn on_console_message(
            &self,
            browser: Option<&mut Browser>,
            _level: LogSeverity,
            message: Option<&CefString>,
            _source: Option<&CefString>,
            _line: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            // The Notification shim's messages are protocol traffic, not page console output.
            system::on_console_message(browser, message) as _
        }
    }
}

wrap_load_handler! {
    struct EngineLoadHandler;

    impl LoadHandler {
        fn on_load_start(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _transition_type: TransitionType,
        ) {
            system::on_load_start(browser, frame);
        }

        fn on_loading_state_change(
            &self,
            browser: Option<&mut Browser>,
            is_loading: ::std::os::raw::c_int,
            can_go_back: ::std::os::raw::c_int,
            can_go_forward: ::std::os::raw::c_int,
        ) {
            let Some(browser) = browser else { return };
            let state = (is_loading != 0, can_go_back != 0, can_go_forward != 0);
            let Some(key) = key_of(browser) else { return };
            with_pane(&key, |pane| pane.loading = Some(state));
            if let Key::Tab(tab) = key {
                emit(Event::Loading { tab, loading: state.0, can_go_back: state.1, can_go_forward: state.2 });
            }
        }
    }
}

wrap_life_span_handler! {
    struct EngineLifeSpanHandler;

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            LIVE_BROWSERS.set(LIVE_BROWSERS.get() + 1);
            let Some(browser) = browser else { return };
            vault::attach(browser);
            if key_of(browser).is_some() {
                return;
            }
            // An engine view's own browser can arrive here before the view knows it, so the
            // decision waits for the first address (on_address_change).
            STRAYS.with_borrow_mut(|strays| strays.push(browser.clone()));
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            LIVE_BROWSERS.set(LIVE_BROWSERS.get().saturating_sub(1));
            if let Some(browser) = browser {
                vault::detach(browser);
                STRAYS.with_borrow_mut(|strays| strays.retain(|candidate| candidate.is_same(Some(browser)) == 0));
                DEVTOOLS.with_borrow_mut(|views| views.retain(|view| !same(view, browser)));
                match key_of(browser) {
                    Some(Key::Tab(tab)) => {
                        TABS.with_borrow_mut(|tabs| tabs.remove(&tab));
                        permissions::forget_tab(&tab);
                        emit(Event::Closed { tab });
                    }
                    Some(Key::Popup(id)) => {
                        POPUPS.with_borrow_mut(|popups| popups.remove(&id));
                    }
                    _ => {}
                }
            }
            if QUITTING.get() && LIVE_BROWSERS.get() == 0 {
                quit_message_loop();
            }
        }
    }
}

wrap_dev_tools_message_observer! {
    struct EngineDevtoolsObserver;

    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(
            &self,
            _browser: Option<&mut Browser>,
            message_id: ::std::os::raw::c_int,
            success: ::std::os::raw::c_int,
            result: Option<&[u8]>,
        ) {
            let Ok(id) = u32::try_from(message_id) else { return };
            // Only answers to calls Ferryx made are reported; DevTools windows use their own ids.
            let Some(tab) = DEVTOOLS_CALLS.with_borrow_mut(|calls| calls.remove(&id)) else { return };
            let result = result.map(|bytes| String::from_utf8_lossy(bytes).into_owned()).unwrap_or_else(|| "{}".into());
            emit(Event::Devtools { tab, id, ok: success != 0, result });
        }
    }
}
