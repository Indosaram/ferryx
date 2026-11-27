//! Per-window display refresh discovery and change notifications.
//!
//! The first observation seeds a window without publishing a change. Subsequent
//! distinct rates publish immediately, including distinct transitions within the
//! debounce window; repeated notifications must never hide a real mode switch.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

pub const DEFAULT_REFRESH_HZ: u32 = 60;
pub const MIN_REFRESH_HZ: u32 = 24;
pub const MAX_REFRESH_HZ: u32 = 240;
const DEBOUNCE_WINDOW: Duration = Duration::from_millis(150);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RefreshSource {
    AppKit,
    CoreGraphics,
    Win32,
    Gdk,
    Fallback,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayRefreshInfo {
    pub refresh_hz: u32,
    pub millihertz: u32,
    pub monitor_name: Option<String>,
    pub source: RefreshSource,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayRefreshChange {
    pub window_label: String,
    pub refresh_hz: u32,
    pub millihertz: u32,
}

pub fn millihertz_to_hz(millihertz: u32) -> u32 {
    if millihertz == 0 {
        DEFAULT_REFRESH_HZ
    } else {
        // Widen the addition so even an out-of-range OS value cannot overflow.
        (((u64::from(millihertz) + 500) / 1000) as u32)
            .clamp(MIN_REFRESH_HZ, MAX_REFRESH_HZ)
    }
}

pub fn clamp_refresh_hz(hz: i64) -> u32 {
    if hz <= 0 {
        DEFAULT_REFRESH_HZ
    } else {
        hz.clamp(i64::from(MIN_REFRESH_HZ), i64::from(MAX_REFRESH_HZ)) as u32
    }
}

pub struct DisplayRefreshHub {
    last_observed: Mutex<HashMap<String, u32>>,
    debounce: Mutex<HashMap<String, (u32, Instant)>>,
    sender: broadcast::Sender<DisplayRefreshChange>,
}

impl Default for DisplayRefreshHub {
    fn default() -> Self {
        let (sender, _) = broadcast::channel(32);
        Self {
            last_observed: Mutex::new(HashMap::new()),
            debounce: Mutex::new(HashMap::new()),
            sender,
        }
    }
}

impl DisplayRefreshHub {
    pub fn subscribe(&self) -> broadcast::Receiver<DisplayRefreshChange> {
        self.sender.subscribe()
    }

    /// Seed silently, then publish only changes in rounded, clamped refresh Hz.
    /// Repeated equal rates within 150 ms are debounced; different rates are
    /// never suppressed, and equal rates outside that interval are deduplicated.
    pub fn observe(
        &self,
        window_label: &str,
        info: DisplayRefreshInfo,
    ) -> Option<DisplayRefreshChange> {
        self.observe_at(window_label, info, Instant::now())
    }

    fn observe_at(
        &self,
        window_label: &str,
        info: DisplayRefreshInfo,
        now: Instant,
    ) -> Option<DisplayRefreshChange> {
        // Hold this lock through publication to preserve transition order even
        // when the command path and a native notification race.
        let mut observed = self.last_observed.lock();
        let mut debounce = self.debounce.lock();
        if let Some((hz, last_seen)) = debounce.get(window_label) {
            if *hz == info.refresh_hz && now.duration_since(*last_seen) < DEBOUNCE_WINDOW {
                return None;
            }
        }
        debounce.insert(window_label.to_owned(), (info.refresh_hz, now));
        let previous = observed.insert(window_label.to_owned(), info.refresh_hz);
        if previous.is_none() || previous == Some(info.refresh_hz) {
            return None;
        }
        let change = DisplayRefreshChange {
            window_label: window_label.to_owned(),
            refresh_hz: info.refresh_hz,
            millihertz: info.millihertz,
        };
        // No subscribers is normal before a remote-desktop session starts.
        let _ = self.sender.send(change.clone());
        Some(change)
    }

    pub fn remove_window(&self, window_label: &str) {
        let mut observed = self.last_observed.lock();
        let mut debounce = self.debounce.lock();
        observed.remove(window_label);
        debounce.remove(window_label);
    }
}

pub fn global_refresh_hub() -> &'static DisplayRefreshHub {
    static HUB: OnceLock<DisplayRefreshHub> = OnceLock::new();
    HUB.get_or_init(DisplayRefreshHub::default)
}

#[cfg(feature = "desktop")]
fn fallback_info() -> DisplayRefreshInfo {
    DisplayRefreshInfo {
        refresh_hz: DEFAULT_REFRESH_HZ,
        millihertz: DEFAULT_REFRESH_HZ * 1000,
        monitor_name: None,
        source: RefreshSource::Fallback,
    }
}

/// Native GUI APIs are touched only on their GUI thread. Dispatch failures and
/// teardown timeouts return the documented 60 Hz fallback, never block forever.
#[cfg(feature = "desktop")]
pub fn query_window_refresh_rate<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> DisplayRefreshInfo {
    #[cfg(target_os = "macos")]
    {
        if objc2::MainThreadMarker::new().is_some() {
            return platform::query(window);
        }
    }
    #[cfg(target_os = "linux")]
    {
        if gtk::is_initialized_main_thread() {
            return platform::query(window);
        }
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let query_window = window.clone();
        if let Err(error) = window.run_on_main_thread(move || {
            let _ = sender.send(platform::query(&query_window));
        }) {
            tracing::debug!(%error, "display refresh query dispatch failed");
            return fallback_info();
        }
        match receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(info) => info,
            Err(error) => {
                tracing::debug!(%error, "display refresh query did not finish");
                fallback_info()
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        platform::query(window)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = window;
        fallback_info()
    }
}

#[cfg(all(
    feature = "desktop",
    any(target_os = "macos", target_os = "windows", target_os = "linux")
))]
fn refresh_windows<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::{Emitter, Manager};

    for (label, window) in app.webview_windows() {
        let info = query_window_refresh_rate(&window);
        if let Some(change) = global_refresh_hub().observe(&label, info) {
            if let Err(error) = window.emit("rd-display-refresh-changed", change) {
                tracing::debug!(%error, window_label = %label, "display refresh event failed");
            }
        }
    }
}

/// Install once during desktop setup. The native observer also seeds existing
/// windows, so the first stationary mode switch is a genuine transition.
#[cfg(feature = "desktop")]
pub fn install_platform_refresh_observer<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        let observer_app = app.clone();
        if let Err(error) = app.run_on_main_thread(move || {
            refresh_windows(&observer_app);
            platform::install(&observer_app);
        }) {
            tracing::debug!(%error, "display refresh observer dispatch failed");
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let _ = app;
}

// Keep the pure hub usable by headless builds without linking Tauri or GTK.
#[cfg(not(feature = "desktop"))]
pub fn install_platform_refresh_observer<T>(_app: &T) {}

#[cfg(all(feature = "desktop", target_os = "macos"))]
mod platform {
    use super::*;
    use objc2::rc::Retained;
    use objc2::{define_class, msg_send, sel, AnyThread, DefinedClass};
    use objc2_app_kit::{NSApplicationDidChangeScreenParametersNotification, NSWindow};
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol};

    pub fn query<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> DisplayRefreshInfo {
        let Ok(pointer) = window.ns_window() else {
            return fallback_info();
        };
        // SAFETY: called only on the main thread; Tauri owns the live NSWindow
        // and we borrow it only for this synchronous screen query.
        let Some(native_window) = (unsafe { (pointer as *const NSWindow).as_ref() }) else {
            return fallback_info();
        };
        let Some(screen) = native_window.screen() else {
            return fallback_info();
        };
        let refresh_hz = clamp_refresh_hz(screen.maximumFramesPerSecond() as i64);
        DisplayRefreshInfo {
            refresh_hz,
            millihertz: refresh_hz * 1000,
            monitor_name: Some(screen.localizedName().to_string()),
            source: RefreshSource::AppKit,
        }
    }

    struct ObserverIvars {
        callback: Box<dyn Fn() + Send + Sync>,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements. The callback owns
        // only a thread-safe AppHandle; query_window_refresh_rate handles dispatch.
        #[unsafe(super(NSObject))]
        #[name = "FerryxDisplayRefreshObserver"]
        #[ivars = ObserverIvars]
        struct ScreenObserver;

        unsafe impl NSObjectProtocol for ScreenObserver {}

        impl ScreenObserver {
            #[unsafe(method(screenParametersChanged:))]
            fn screen_parameters_changed(&self, _notification: &NSNotification) {
                (self.ivars().callback)();
            }
        }
    );

    pub fn install<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
        static INSTALL: std::sync::Once = std::sync::Once::new();
        INSTALL.call_once(|| {
            let app = app.clone();
            let allocated = ScreenObserver::alloc().set_ivars(ObserverIvars {
                callback: Box::new(move || refresh_windows(&app)),
            });
            // SAFETY: freshly allocated NSObject subclass with initialized ivars.
            let observer: Retained<ScreenObserver> = unsafe { msg_send![super(allocated), init] };
            // Use the selector API: Foundation's block observer requires the
            // NSOperation/block2 features, which are not enabled in Cargo.toml.
            // SAFETY: the selector and notification signature match, and the
            // strong observer reference is intentionally kept for process life.
            unsafe {
                NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                    &observer,
                    sel!(screenParametersChanged:),
                    Some(NSApplicationDidChangeScreenParametersNotification),
                    None,
                );
            }
            let _ = Retained::into_raw(observer);
        });
    }
}

#[cfg(all(feature = "desktop", target_os = "windows"))]
mod platform {
    use super::*;
    use std::cell::RefCell;
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplaySettingsW, GetMonitorInfoW, MonitorFromWindow, DEVMODEW,
        ENUM_CURRENT_SETTINGS, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
    };
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, SetWindowsHookExW, CWPSTRUCT, WH_CALLWNDPROC, WM_DISPLAYCHANGE,
    };

    pub fn query<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> DisplayRefreshInfo {
        let Ok(hwnd) = window.hwnd() else {
            return fallback_info();
        };
        // SAFETY: native handles and writable structures are valid for each
        // synchronous Win32 call, with the required size fields initialized.
        unsafe {
            let monitor = MonitorFromWindow(hwnd.0 as _, MONITOR_DEFAULTTONEAREST);
            if monitor.is_null() {
                return fallback_info();
            }
            let mut monitor_info: MONITORINFOEXW = std::mem::zeroed();
            monitor_info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if GetMonitorInfoW(monitor, &mut monitor_info.monitorInfo) == 0 {
                return fallback_info();
            }
            let mut mode: DEVMODEW = std::mem::zeroed();
            mode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
            if EnumDisplaySettingsW(
                monitor_info.szDevice.as_ptr(),
                ENUM_CURRENT_SETTINGS,
                &mut mode,
            ) == 0
            {
                return fallback_info();
            }
            let refresh_hz = if mode.dmDisplayFrequency <= 1 {
                DEFAULT_REFRESH_HZ
            } else {
                clamp_refresh_hz(i64::from(mode.dmDisplayFrequency))
            };
            let length = monitor_info.szDevice.iter().position(|c| *c == 0)
                .unwrap_or(monitor_info.szDevice.len());
            DisplayRefreshInfo {
                refresh_hz,
                millihertz: refresh_hz * 1000,
                monitor_name: Some(String::from_utf16_lossy(&monitor_info.szDevice[..length])),
                source: RefreshSource::Win32,
            }
        }
    }

    thread_local! {
        // A thread-local hook avoids replacing Tauri's window procedure. Windows
        // removes the hook when its owning GUI thread exits.
        static DISPLAY_HOOK: RefCell<Option<(usize, Box<dyn Fn()>)>> = RefCell::new(None);
    }

    unsafe extern "system" fn display_change_hook(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code >= 0 && lparam != 0 {
            // SAFETY: a nonnegative WH_CALLWNDPROC hook receives a CWPSTRUCT
            // pointer valid for the duration of this callback.
            let message = unsafe { &*(lparam as *const CWPSTRUCT) };
            if message.message == WM_DISPLAYCHANGE {
                DISPLAY_HOOK.with(|hook| {
                    if let Some((_, callback)) = hook.borrow().as_ref() {
                        callback();
                    }
                });
            }
        }
        // SAFETY: forwarding every message preserves the existing hook chain;
        // the first argument is ignored by CallNextHookEx.
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    pub fn install<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
        DISPLAY_HOOK.with(|state| {
            let mut state = state.borrow_mut();
            if state.is_some() {
                return;
            }
            // SAFETY: installed on the current GUI thread with a static callback;
            // hmod may be null for an in-process, thread-specific hook.
            let hook = unsafe {
                SetWindowsHookExW(
                    WH_CALLWNDPROC,
                    Some(display_change_hook),
                    std::ptr::null_mut(),
                    GetCurrentThreadId(),
                )
            };
            if hook.is_null() {
                tracing::debug!(error = %std::io::Error::last_os_error(), "display refresh hook unavailable");
                return;
            }
            let app = app.clone();
            *state = Some((hook as usize, Box::new(move || refresh_windows(&app))));
        });
    }
}

#[cfg(all(feature = "desktop", target_os = "linux"))]
mod platform {
    use super::*;
    use gdk::prelude::*;
    use gtk::prelude::WidgetExt;

    pub fn query<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> DisplayRefreshInfo {
        let Ok(gtk_window) = window.gtk_window() else {
            return fallback_info();
        };
        let Some(native_window) = gtk_window.window() else {
            return fallback_info();
        };
        let Some(monitor) = gtk_window.display().monitor_at_window(&native_window) else {
            return fallback_info();
        };
        // gdk 0.18 MonitorExt::refresh_rate returns signed i32 millihertz.
        let rate: i32 = monitor.refresh_rate();
        let millihertz = if rate <= 0 { DEFAULT_REFRESH_HZ * 1000 } else { rate as u32 };
        DisplayRefreshInfo {
            refresh_hz: millihertz_to_hz(millihertz),
            millihertz,
            monitor_name: monitor.model().map(|name| name.to_string()),
            source: RefreshSource::Gdk,
        }
    }

    fn watch_monitor<R: tauri::Runtime>(monitor: &gdk::Monitor, app: &tauri::AppHandle<R>) {
        let invalidated_app = app.clone();
        monitor.connect_invalidate(move |_| refresh_windows(&invalidated_app));
        // Refresh-rate notify also covers mode switches without monitor removal.
        let changed_app = app.clone();
        monitor.connect_refresh_rate_notify(move |_| refresh_windows(&changed_app));
    }

    pub fn install<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
        static INSTALL: std::sync::Once = std::sync::Once::new();
        let Some(display) = gdk::Display::default() else {
            tracing::debug!("display refresh observer has no GDK display");
            return;
        };
        INSTALL.call_once(|| {
            for index in 0..display.n_monitors() {
                if let Some(monitor) = display.monitor(index) {
                    watch_monitor(&monitor, app);
                }
            }
            let added_app = app.clone();
            display.connect_monitor_added(move |_, monitor| {
                watch_monitor(monitor, &added_app);
                refresh_windows(&added_app);
            });
            let removed_app = app.clone();
            display.connect_monitor_removed(move |_, _| refresh_windows(&removed_app));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast::error::TryRecvError;

    fn info(millihertz: u32) -> DisplayRefreshInfo {
        DisplayRefreshInfo {
            refresh_hz: millihertz_to_hz(millihertz),
            millihertz,
            monitor_name: None,
            source: RefreshSource::Fallback,
        }
    }

    #[test]
    fn millihertz_rounding_and_limits() {
        for (input, expected) in [
            (59_940, 60), (74_973, 75), (75_000, 75), (119_880, 120),
            (144_000, 144), (0, 60), (10_000, 24), (999_999, 240),
            (u32::MAX, 240),
        ] {
            assert_eq!(millihertz_to_hz(input), expected, "input {input}");
        }
    }

    #[test]
    fn hz_clamping_and_unknown_values() {
        for (input, expected) in [(0, 60), (-1, 60), (12, 24), (75, 75), (500, 240), (i64::MAX, 240)] {
            assert_eq!(clamp_refresh_hz(input), expected, "input {input}");
        }
    }

    #[test]
    fn seed_is_silent_and_distinct_transitions_publish_in_order() {
        let hub = DisplayRefreshHub::default();
        let mut receiver = hub.subscribe();
        let now = Instant::now();
        assert_eq!(hub.observe_at("main", info(60_000), now), None);
        assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));
        let mut transitions = Vec::new();
        for rate in [60_000, 75_000, 75_000, 120_000] {
            if let Some(change) = hub.observe_at("main", info(rate), now) {
                transitions.push(change.refresh_hz);
            }
        }
        assert_eq!(transitions, [75, 120]);
        for (hz, millihertz) in [(75, 75_000), (120, 120_000)] {
            assert_eq!(receiver.try_recv().unwrap(), DisplayRefreshChange {
                window_label: "main".into(), refresh_hz: hz, millihertz,
            });
        }
        assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn debounce_and_long_term_dedup_preserve_window_independence() {
        let hub = DisplayRefreshHub::default();
        let mut receiver = hub.subscribe();
        let now = Instant::now();
        assert_eq!(hub.observe_at("main", info(75_000), now), None);
        assert_eq!(hub.observe_at("other", info(60_000), now), None);
        assert_eq!(hub.observe_at("main", info(74_973), now + DEBOUNCE_WINDOW / 2), None);
        assert_eq!(hub.observe_at("main", info(75_000), now + DEBOUNCE_WINDOW), None);
        assert_eq!(hub.observe_at("other", info(75_000), now).unwrap().refresh_hz, 75);
        assert_eq!(receiver.try_recv().unwrap().window_label, "other");
        assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn remove_window_clears_observation_and_debounce_before_reseeding() {
        let hub = DisplayRefreshHub::default();
        let mut receiver = hub.subscribe();
        assert_eq!(hub.observe("main", info(60_000)), None);
        assert_eq!(hub.observe("other", info(75_000)), None);
        hub.remove_window("main");
        assert!(!hub.last_observed.lock().contains_key("main"));
        assert!(!hub.debounce.lock().contains_key("main"));
        assert_eq!(hub.last_observed.lock().get("other"), Some(&75));
        assert_eq!(hub.observe("main", info(120_000)), None);
        assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));
        hub.remove_window("missing");
        assert_eq!(hub.observe("main", info(75_000)).unwrap().refresh_hz, 75);
        assert_eq!(receiver.try_recv().unwrap().refresh_hz, 75);
        assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));
    }
}
