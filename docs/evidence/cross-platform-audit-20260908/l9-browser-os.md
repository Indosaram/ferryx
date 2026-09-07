# L9 — Embedded Browser / Notifications / OS Integration Cross-Platform Audit

Scope: `src-tauri/src/browser/`, `src-tauri/src/ipc/browser.rs`, `src-tauri/src/notification/`,
`src-tauri/src/ipc/notifications.rs`, `src-tauri/src/permissions/mod.rs`,
`src-tauri/src/ipc/native_menu.rs`, `src-tauri/src/ipc/diagnostics.rs`, `src-tauri/src/ipc/debug.rs`.

---

### Cookie import calls `Webview::set_cookie` synchronously inside an async command — documented WebView2 deadlock hazard on Windows
- **ID**: L9-BROWSER-OS-1
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/browser.rs:1286` — `.set_cookie(cookie.clone())`
- **Why it breaks**: `cmd_browser_import_cookies` is an `async fn` that calls `Webview::set_cookie` directly on the Tokio task (no `run_blocking`/dedicated thread). Tauri's own docs for `Webview::set_cookie`/`cookies()` state: "On Windows, this function deadlocks when used in a synchronous command or event handlers... You should use `async` commands and separate threads." Calling it synchronously from inside an `async` command body on Windows/WebView2 hangs the invoking task, so cookie import never returns and the frontend's cookie-import UI hangs forever on Windows.
- **Fix**: Wrap the `target.set_cookie(...)` loop in `cmd_browser_import_cookies` (`src-tauri/src/ipc/browser.rs`) with `crate::ipc::run_blocking(move || { ... })` (the same helper already used by `cmd_notification_play_sound`) so the WebView2 IPC round-trip happens off the async runtime thread, matching Tauri's documented Windows workaround.
- **Status**: OPEN

---

### Embedded browser clipboard access is silently disabled on Windows and Linux
- **ID**: L9-BROWSER-OS-2
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/ipc/browser.rs:930` — `let builder = tauri::WebviewBuilder::new(label, parsed_url)`
- **Why it breaks**: This is the only `WebviewBuilder` construction in the codebase (used for every embedded browser tab). Tauri/wry's `WebviewBuilder::enable_clipboard_access()` doc states clipboard access for the rendered page is opt-in on **Linux and Windows** and only "always enabled by default" on **macOS**. The builder chain here never calls `.enable_clipboard_access()`, so copy/paste and `navigator.clipboard` inside embedded browser tabs (OAuth flows, web app forms, docs) work on macOS but silently fail (clipboard writes/reads no-op or throw) on Windows and Linux WebView2/WebKitGTK.
- **Fix**: Add `.enable_clipboard_access()` to the `tauri::WebviewBuilder::new(label, parsed_url)` chain in `cmd_browser_create` (`src-tauri/src/ipc/browser.rs`) unconditionally (it is a documented no-op on macOS, so this is safe on all three targets).
- **Status**: OPEN

---

### Browser back/forward availability never reflects real engine history on Windows and Linux
- **ID**: L9-BROWSER-OS-3
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/ipc/browser.rs:817` — `#[cfg(target_os = "macos")]`
- **Why it breaks**: `update_webview_state` only queries `native.canGoBack()`/`canGoForward()` (via `WKWebView`) inside a `#[cfg(target_os = "macos")]` block. On Windows/Linux this block is compiled out, so `ManagedBrowserSession.native_history_flags` is never set to `true` and the back/forward buttons stay driven by the shadow `history: Vec<String>` fallback. `manager.rs`'s own comment on `native_history_flags` documents that this shadow vector "cannot represent fragment navigations or `pushState` entries the engine recorded" — exactly the case for most modern SPAs (OAuth redirects, client-side routers). Users on Windows/Linux get a back/forward toolbar that silently disagrees with what the embedded page can actually navigate to.
- **Fix**: In `cmd_browser_navigate`/`cmd_browser_reload`/`history_navigation` (`src-tauri/src/ipc/browser.rs`), add a non-macOS path that evaluates a small JS snippet (`history.length`, `window.history.state`) via `on_page_load`/`eval_with_callback`, or use `tauri::Webview::navigate` + WebView2/WebKitGTK's native history query APIs once wry exposes them, and call `manager.update_navigation_state(..., Some(can_back), Some(can_forward), ...)` from that path the same way the macOS block does.
- **Status**: OPEN

---

### Non-macOS history navigation is fire-and-forget `eval("history.back()")` with no confirmation the navigation happened
- **ID**: L9-BROWSER-OS-4
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/ipc/browser.rs:1213` — `"history.forward()"`
- **Why it breaks**: On macOS, `history_navigation` calls `native.canGoBack()`/`goBack()` synchronously and immediately reconciles manager state with the real WKWebView answer (or calls `cancel_history_navigation` if it can't navigate). On Windows/Linux the same function just does `webview.eval("history.back()")` and, as long as `eval` doesn't itself error, treats the navigation as accepted — there is no callback confirming the JS actually moved history (e.g. because `history.length` was 1). The UI's `can_go_back`/`can_go_forward` state (from L9-BROWSER-OS-3) can therefore report a wrong, un-corrected state after a no-op navigation attempt.
- **Fix**: In the `#[cfg(not(target_os = "macos"))]` branch of `history_navigation` (`src-tauri/src/ipc/browser.rs`), replace the fire-and-forget `webview.eval(script)` with `eval_webview` (already defined in the same file) so the callback result can confirm whether `history.back()/forward()` changed `location.href`, and call `manager.cancel_history_navigation` when it did not.
- **Status**: OPEN

---

### `Silent` notification sound is ignored on Windows and Linux — every notification plays the OS default sound
- **ID**: L9-BROWSER-OS-5
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/notification/notify_rust_adapter.rs:35` — `builder.summary(&content.title).body(&content.body);`
- **Why it breaks**: `NotificationContent.sound` (`NotificationSound::System | Silent`) is read on macOS in `macos_submission.rs` (`native.setSound(...)` / `native.setSound(None)`), but `submit_with_click_routing` in `notify_rust_adapter.rs` never reads `content.sound` at all — it only sets `summary`/`body`/`app_id`/`appname`/`action`. The plugin-notification fallback path in `src-tauri/src/ipc/notifications.rs` (used for target-less notifications) likewise never calls a sound-suppression API. A user who picks "Silent" in notification settings still hears the platform's default toast/XDG sound on every Windows and Linux build, while macOS correctly stays silent.
- **Fix**: In `submit_with_click_routing` (`src-tauri/src/notification/notify_rust_adapter.rs`), branch on `content.sound`: on Linux call `builder.hint(notify_rust::Hint::SuppressSound(true))` when `NotificationSound::Silent`, and for the target-less path in `TauriNotificationBackend::submit` (`src-tauri/src/ipc/notifications.rs`) apply the same hint before `.show()`.
- **Status**: OPEN

---

### macOS Dock badge has no Windows taskbar-overlay or Linux launcher-badge equivalent, despite Tauri exposing one
- **ID**: L9-BROWSER-OS-6
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/ipc/notifications.rs:250` — `Ok(SetBadgeCountResult::unsupported(count))`
- **Why it breaks**: `cmd_notification_set_badge_count` (`src-tauri/src/ipc/notifications.rs:210`) implements badge sync only for `#[cfg(target_os = "macos")]` via raw AppKit (`dockTile().setBadgeLabel(...)` in `src-tauri/src/notification/badge.rs`), and unconditionally returns `SetBadgeCountResult::unsupported(count)` for every other platform. Tauri 2.11's `Window` type exposes a cross-platform `set_badge_count(Option<i64>)` (Linux: libunity launcher badge; Windows: explicitly documented as unsupported there, directing callers to `Window::set_overlay_icon` instead for the taskbar overlay). Ferryx never calls either, so unread-count badges that work today on macOS are invisible on Linux desktops with `libunity` support and on the Windows taskbar, even though first-party APIs for both exist.
- **Fix**: In `cmd_notification_set_badge_count` (`src-tauri/src/ipc/notifications.rs`), add a `#[cfg(target_os = "linux")]` branch calling `app.get_window("main")?.set_badge_count(Some(count as i64))` and a `#[cfg(target_os = "windows")]` branch calling `Window::set_overlay_icon` with a small numeric badge image (or at minimum `set_badge_count`, which Tauri's docs say Windows ignores, and prefer `set_overlay_icon` for a real overlay), reporting `SetBadgeCountResult::unsupported` only for platforms where neither succeeds.
- **Status**: OPEN

---

### Switch-debug trace log is hardcoded to `/tmp`, a path that does not exist on Windows
- **ID**: L9-BROWSER-OS-7
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/debug.rs:48` — `.open("/tmp/ferryx-switch-debug.jsonl")`
- **Why it breaks**: `cmd_switch_debug_log` opens `/tmp/ferryx-switch-debug.jsonl` unconditionally with `OpenOptions::create(true).append(true)`. `/tmp` is not a valid absolute path on Windows (no such root); `std::fs::OpenOptions::open` will fail there. The function is enabled by default in debug builds (`switch_debug_sink_enabled_here()` returns `true` whenever `cfg!(debug_assertions)`), so every `cmd_switch_debug_log` IPC call from the frontend's debug instrumentation on a Windows dev/debug build returns an `IpcError::internal` write failure instead of tracing, silently breaking a debugging tool exactly when a Windows contributor needs it.
- **Fix**: Replace the hardcoded path with `std::env::temp_dir().join("ferryx-switch-debug.jsonl")` in `cmd_switch_debug_log` (`src-tauri/src/ipc/debug.rs`), which resolves to `/tmp` on macOS/Linux and `%TEMP%` on Windows.
- **Status**: OPEN

---

### `request_accessibility()` reports success on Windows/Linux without performing any request, and the status DTO always advertises `can_request: true`
- **ID**: L9-BROWSER-OS-8
- **Severity**: LOW
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/permissions/mod.rs:152` — `pub fn request_accessibility() -> bool {`
- **Why it breaks**: The non-macOS `request_accessibility()` (`src-tauri/src/permissions/mod.rs:151-153`) unconditionally returns `true` even though it does nothing, and `get_system_permissions_status` (`src-tauri/src/permissions/mod.rs:258`, `can_request: true,`) sets `can_request: true` for the accessibility item on every platform regardless of whether `check_accessibility()` even ran a real check (it returns `PermissionStatus::Unsupported` off-macOS). The current frontend (`ui/src/components/settings/PermissionsSection.tsx`) happens to gate the whole accessibility card behind `status?.platform === "macos"`, so this is not currently reachable in the UI, but the IPC contract itself is platform-inconsistent: any future consumer of `cmd_permissions_request_accessibility`/`cmd_permissions_get_status` on Windows/Linux gets a fabricated "request succeeded, you can request" signal for a permission concept that does not exist there.
- **Fix**: In `get_system_permissions_status` (`src-tauri/src/permissions/mod.rs`), set `can_request: cfg!(target_os = "macos")` for the accessibility item instead of the unconditional `true`, and have the non-macOS `request_accessibility()` return `false` (or better, remove the non-macOS command registration for `cmd_permissions_request_accessibility` entirely) so the IPC contract does not claim a capability that only exists on macOS.
- **Status**: OPEN

---

### Windows AppUserModelID is only applied for release-mode builds — dev/debug notification click-routing may silently mis-identify the app
- **ID**: L9-BROWSER-OS-9
- **Severity**: LOW
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/notification/notify_rust_adapter.rs:35` — `builder.summary(&content.title).body(&content.body);`
- **Why it breaks**: A few lines below the cited call (see `src-tauri/src/notification/notify_rust_adapter.rs`, the `#[cfg(target_os = "windows")]` block), `builder.app_id(_app_id)` is skipped whenever `std::env::current_exe()`'s parent directory ends with `target/debug` or `target/release` — i.e. any `cargo run`/`cargo build` invocation, including a Windows contributor's release-profile local build that is not produced by `tauri bundle`. Toasts shown from such a build carry no AppUserModelID, so Windows may group/attribute them under a generic "Rust"/console host identity rather than Ferryx, which is a real (if narrow) gap for anyone debugging notification click-routing on Windows outside the installed bundle.
- **Fix**: This is an intentional dev-mode carve-out (comment: preserves the identifier for bundled builds) and is arguably correctly scoped; if precise parity with the shipped bundle is required for local Windows debugging, gate on `tauri::is_dev()`/a build-time env var instead of matching on the `target/debug`|`target/release` path suffix, since that heuristic also matches a local *release* profile build that a Windows developer runs directly.
- **Status**: OPEN

---

### `default_desktop_user_agent` has no platform branch for `not(any(macos, windows, linux))`, silently claiming to be macOS
- **ID**: L9-BROWSER-OS-10
- **Severity**: LOW
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/browser/security.rs:61` — `"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36"`
- **Why it breaks**: This is the `#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]` fallback arm of `default_desktop_user_agent`. It is unreachable for the three shipped bundle targets (dmg/app, nsis/msi, appimage/deb all match one of the three `cfg`s above it), so it is dead code on every build Ferryx actually ships — not a live defect for users, but it is a latent trap: if a fourth target (e.g. `freebsd`) is ever added to `tauri.conf.json` without updating this function, every embedded browser tab on that platform would send a macOS user-agent string, which some sites use to serve macOS-specific download links or reject unexpected UA/OS combinations.
- **Fix**: Either delete the catch-all arm and let `default_desktop_user_agent` fail to compile on an unsupported target (forcing an explicit decision when a new target is added), or make it construct a generic UA string (`Mozilla/5.0 (X11; Unknown) ...`) instead of quietly impersonating macOS.
- **Status**: OPEN

---

### Notification permission "Unknown" status on Linux is reported without distinguishing a missing notification daemon from a real desktop-managed grant
- **ID**: L9-BROWSER-OS-11
- **Severity**: MEDIUM
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/notification/permission.rs:42` — `let can_open_settings = matches!(platform, NotificationPlatform::Windows);`
- **Why it breaks**: `DesktopFallbackPermissionProvider::status()` on Linux always returns `NotificationAuthorization::Unknown`/`authoritative: false`, and `preflight()` (`src-tauri/src/notification/service.rs`) treats non-authoritative status as always submittable. `notify-rust`'s Linux backend requires a running D-Bus notification daemon (e.g. `dunst`, `mako`, or a DE's own service); on a minimal window manager or a headless/CI-like Linux session with no daemon running, `Notification::show()` in `notify_rust_adapter.rs` returns an `Err`, which is correctly turned into `NotificationDispatchReason::BackendError` per-call — but there is no one-time capability probe, so every single dispatched notification pays the cost of discovering this at submission time with no persistent "notifications unavailable" status the UI could surface once instead of failing repeatedly.
- **Fix**: Add a lightweight one-time D-Bus notification-daemon reachability check (e.g. attempt `notify_rust::get_capabilities()` or ping `org.freedesktop.Notifications` over the session bus) in the Linux `DesktopFallbackPermissionProvider::status()` (`src-tauri/src/notification/permission.rs`) and report `NotificationPermissionStatusDto::unsupported(NotificationPlatform::Linux)` when no daemon answers, so `preflight` can reject before wasting a submission attempt and the Settings UI can show a real "no notification daemon" message instead of a generic backend error per dispatch.
- **Status**: OPEN

---

### Named persistent browser profiles are macOS-unsupported by design, but the frontend has no platform gate preventing the request from being built
- **ID**: L9-BROWSER-OS-12
- **Severity**: LOW
- **Platforms affected**: macOS
- **Evidence**: `src-tauri/src/ipc/browser.rs:874` — `return Err(BrowserError::UnsupportedProfile(`
- **Why it breaks**: `cmd_browser_create` correctly rejects `BrowserProfileId::Named` on macOS (WebKit's `WKWebViewConfiguration` has no per-profile persistent data store equivalent to WebView2/WebKitGTK's `data_directory`), returning a typed `BrowserError::UnsupportedProfile`. This is the right backend behavior (it is why this item is LOW, not a defect in itself), but it means any frontend feature that lets a user create a named browser profile (e.g. "isolated login profile per workspace") will error out only at creation time on macOS with no upfront capability flag exposed over IPC, so the UI cannot proactively hide/disable that option for macOS users without hardcoding a platform check of its own.
- **Fix**: Expose a small `cmd_browser_named_profiles_supported() -> bool` (or fold a `namedProfilesSupported` flag into an existing capabilities query) returning `cfg!(not(target_os = "macos"))`, so the frontend can hide the "named profile" affordance on macOS instead of surfacing a runtime error after the user already chose it.
- **Status**: FIXED (backend already rejects the request safely with a clear `UnsupportedProfile` error; only the proactive-UI-gate half is missing) — see `src-tauri/src/ipc/browser.rs:868-879`.

---
