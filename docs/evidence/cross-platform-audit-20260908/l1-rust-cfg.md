# L1 — Rust `cfg(target_os)` Conditional-Compilation Audit

Scope: `src-tauri/src` platform branches (`target_os = "macos"/"windows"/"linux"`), the
`Cargo.toml` per-target dependency tables, and the `native-terminal` disabled-command
stub. Findings are ranked by real user impact; test-only issues are labeled as such.

### Accessibility permission "grant" silently reports success on Windows/Linux
- **ID**: L1-RUST-CFG-1
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/permissions/mod.rs:152` — `pub fn request_accessibility() -> bool {`
- **Why it breaks**: The non-macOS sibling of `request_accessibility` unconditionally returns `true`, while `check_accessibility()` on the same platforms returns `PermissionStatus::Unsupported`/`Denied`. `cmd_permissions_request_accessibility` (`src-tauri/src/ipc/permissions.rs:21`) forwards this `true` straight to the frontend, so a Windows/Linux user who clicks "Grant Accessibility" is told the request succeeded even though nothing was requested and no OS permission model was touched.
- **Fix**: Change the `#[cfg(not(target_os = "macos"))]` body of `request_accessibility` in `src-tauri/src/permissions/mod.rs` to return `false` (or a result consistent with `PermissionStatus::Unsupported`) so the IPC layer cannot claim a grant that never happened.
- **Status**: OPEN

### Coding-agent extension install silently no-ops when `HOME` is unset on Windows
- **ID**: L1-RUST-CFG-2
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/agent_extension.rs:25` — `std::env::var_os("HOME").map(PathBuf::from)`
- **Why it breaks**: `home_dir()` (used by `extension_dirs()` at line 15) reads only the Unix `HOME` variable. Plain `cmd.exe`/PowerShell sessions do not set `HOME`, so `extension_dirs()` returns `Vec::new()` and `install_agent_state_extension()` (called unconditionally from `src-tauri/src/daemon/server.rs:1132`) installs the Ferryx agent-state extension into zero directories, with no error, log, or user-visible signal that the feature never ran.
- **Fix**: In `home_dir()`, fall back to `std::env::var_os("USERPROFILE")` (and optionally `%HOMEDRIVE%%HOMEPATH%`) when `HOME` is absent, mirroring the pattern already used in `src-tauri/src/remote/state.rs:421-422` and `src-tauri/src/ipc/ssh.rs:55-56`.
- **Status**: OPEN

### Agent session-id detection (Antigravity/opencode/pi) silently degrades without `HOME`
- **ID**: L1-RUST-CFG-3
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/agents.rs:321` — `if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {`
- **Why it breaks**: `antigravity_session_id` (same pattern repeated at `src-tauri/src/ipc/agents.rs:345` for opencode, and `src-tauri/src/ipc/agents.rs:454` for `pi_session_id_from_session_dir`) reads only `HOME`. On Windows, where `HOME` is frequently unset, every one of these branches is skipped and the function silently falls through to `opencode_session_id_from_cli`/`None` instead of reading the real per-agent session cache, degrading session/agent-state correlation with no diagnostic.
- **Fix**: Introduce a shared `platform_home_dir()` helper (checking `HOME` then `USERPROFILE`) in `src-tauri/src/ipc/agents.rs` and use it at all three call sites instead of raw `env::var_os("HOME")`.
- **Status**: OPEN

### Dock/taskbar badge counts are macOS-only with no Windows overlay-icon or Linux Unity-launcher fallback
- **ID**: L1-RUST-CFG-4
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/notification/badge.rs:48` — `#[cfg(target_os = "macos")]`
- **Why it breaks**: `macos_impl::apply_dock_badge_label` has no sibling module. `cmd_notification_set_badge_count` (`src-tauri/src/ipc/notifications.rs:216` for macOS / `src-tauri/src/ipc/notifications.rs:249-250` for the rest) compiles fine off-macOS and correctly returns `SetBadgeCountResult::unsupported(count)`, so this does not fail to compile or lie about success — but it means unread-count badges (task completions, background build results) are entirely invisible on Windows (`ITaskbarList3::SetOverlayIcon`) and Linux (libunity `.desktop` badge/`unity-webapps` counter), a real, permanent feature gap on 2 of 3 shipped targets rather than a temporary stub.
- **Fix**: Add a `#[cfg(target_os = "windows")]` implementation using `windows-sys`'s `ITaskbarList3::SetOverlayIcon` (crate already depends on `windows-sys` per `src-tauri/Cargo.toml` target table) to draw the decimal count as a small overlay icon on the taskbar button, and treat Linux as `unsupported` only if no `libunity`/`.desktop` badge API is adopted.
- **Status**: OPEN

### Ghostty theme/config discovery has no Windows candidate paths at all
- **ID**: L1-RUST-CFG-5
- **Severity**: LOW
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/terminal/preferences.rs:526` — `if cfg!(target_os = "macos") {`
- **Why it breaks**: `theme_config_candidates` (line 526) and `ghostty_config_candidates` (`src-tauri/src/terminal/preferences.rs:622`) only special-case `macos`; the rest of the function relies on `XDG_CONFIG_HOME`/`HOME`/`GHOSTTY_RESOURCES_DIR`, none of which are Windows conventions, and `env::var_os("HOME")` (line 495, line 619) is frequently absent on Windows. Since Ghostty itself does not ship a native Windows build today, this degrades silently to "no theme/config found" rather than crashing — low impact, but the function returns an empty candidate list rather than surfacing "platform unsupported."
- **Fix**: If Ghostty import is meant to be attempted on Windows at all, add a `%APPDATA%\ghostty` candidate; otherwise short-circuit `ghostty_config_candidates`/`theme_config_candidates` with a `#[cfg(target_os = "windows")] return Vec::new();` guard and have the caller report `TerminalPreferencesStatus::Absent` explicitly rather than relying on incidental empty-vec behavior.
- **Status**: OPEN

### `~` tilde-path expansion for "open file at path" only understands `HOME`, not `USERPROFILE`
- **ID**: L1-RUST-CFG-6
- **Severity**: LOW
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/browser.rs:1625` — `std::env::var_os("HOME")`
- **Why it breaks**: `cmd_open_file_path`'s tilde expansion (`src-tauri/src/ipc/browser.rs:1624-1628`) only reads `HOME`. On Windows, when `HOME` is unset, `~/`-prefixed paths from agent/tool output (e.g. `~/project/file.ts:12`) fall through to `unwrap_or_else` and are treated as a literal relative path `~/project/file.ts`, which will not exist, so "reveal in editor" silently returns `Ok(false)` instead of opening the intended file.
- **Fix**: In `src-tauri/src/ipc/browser.rs`, change the `std::env::var_os("HOME")` call at line 1625 to `std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))`.
- **Status**: OPEN

### Background daemon has no Windows Service / Linux systemd autostart counterpart to launchd
- **ID**: L1-RUST-CFG-7
- **Severity**: LOW
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/daemon/launchd.rs:16` — `#[cfg(not(target_os = "macos"))]`
- **Why it breaks**: `get_launchd_plist_path()` correctly returns `None` off-macOS so `install_launchd_agent()`/`uninstall_launchd_agent()` fail gracefully (`Cannot determine HOME directory`) rather than crashing, and there is currently no IPC command or startup path in the tree that calls them (`install_launchd_agent`/`uninstall_launchd_agent` have zero callers outside `launchd.rs` itself and its tests). The module therefore compiles and degrades safely today, but there is no Windows Service (`sc create`) or Linux `systemd --user` unit generator anywhere in the crate, so "run the daemon at login" can only ever be a macOS feature if it is ever wired up to the UI.
- **Fix**: If daemon autostart is intended to ship cross-platform, add sibling `install_service_agent()`/`uninstall_service_agent()` implementations gated `#[cfg(target_os = "windows")]` (via `sc.exe create`/`schtasks` or the `windows-service` crate) and `#[cfg(not(any(target_os = "macos", target_os = "windows")))]` (writing a `systemd --user` unit under `~/.config/systemd/user/`); otherwise remove the unused macOS-only functions and document autostart as macOS-only in `src-tauri/src/daemon/AGENTS.md`.
- **Status**: OPEN

### Full disk access / accessibility "open settings" deep link has no Linux target at all
- **ID**: L1-RUST-CFG-8
- **Severity**: LOW
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/permissions/mod.rs:268` — `#[cfg(target_os = "macos")]`
- **Why it breaks**: `open_system_settings_for_target` has explicit macOS (`x-apple.systempreferences:...`) and Windows (`ms-settings:notifications`) branches, but the trailing `#[cfg(not(any(target_os = "macos", target_os = "windows")))]` Linux branch always returns `opened: false, reason: Some("target unsupported on Linux")` for every target including `notifications`, even though `deep_links` is already correctly forced to `false` for notifications on Linux via `notification_item`'s `can_open_settings` field (`src-tauri/src/permissions/mod.rs:211` — `can_open_settings: raw.can_open_settings,`). This is consistent/non-misleading, but means Linux users get zero deep-linkable settings for any of the three permission rows, unlike Windows which gets one.
- **Fix**: This is intentionally consistent (no false affordance is shown because `can_open_settings` is `false` on Linux), so no functional fix is required; if desired, add a best-effort `xdg-open settings://notifications` or desktop-environment-specific fallback (e.g. GNOME `gsettings`) inside the trailing Linux `cfg` block of `open_system_settings_for_target`.
- **Status**: FIXED (already consistent: `can_open_settings` is `false` on Linux so the UI never renders a dead button; see `src-tauri/src/permissions/mod.rs:211`)

### Cargo.toml: notification click-routing crate is Windows+Linux only, no macOS parity concern (verified symmetric)
- **ID**: L1-RUST-CFG-9
- **Severity**: LOW
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/Cargo.toml:143` — `[target.'cfg(any(target_os = "windows", target_os = "linux"))'.dependencies]`
- **Why it breaks**: `notify-rust` is pulled in only for Windows/Linux because macOS notification click-routing is handled natively via `objc2-user-notifications` (`src-tauri/Cargo.toml:113-125`). This is intentional and symmetric — flagged only to record that the audit checked it: both code paths exist (`src-tauri/src/notification/permission.rs:55` macOS provider vs. `DesktopFallbackPermissionProvider` for the rest) and neither silently drops functionality relative to the other. No action needed.
- **Fix**: N/A — dependency table is correctly platform-partitioned; no code change required.
- **Status**: FIXED (symmetric by design; macOS uses `objc2-user-notifications`, Windows/Linux use `notify-rust`, see `src-tauri/src/notification/permission.rs:16-23`)

### `TargetPlatform::CURRENT` shell resolution correctly branches all three OSes (verified, no defect)
- **ID**: L1-RUST-CFG-10
- **Severity**: LOW
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/terminal/shell.rs:262` — `} else if cfg!(target_os = "macos") {`
- **Why it breaks**: N/A — recorded because this is one of the highest-risk shell-resolution paths in the tree (default shell / PATH lookup) and it is fully symmetric: `TargetPlatform::CURRENT` (`src-tauri/src/terminal/shell.rs:260-265`) has explicit `match` arms for `Windows` (line 293), `MacOS` (line 327), and `Linux` (line 347) in `resolve_shell_command_pure`, each covered by targeted unit tests (e.g. `src-tauri/src/terminal/shell.rs:469`, `594`, `633`). No missing branch found.
- **Fix**: N/A — no change required; included to document that this file was audited and found symmetric.
- **Status**: FIXED (all three platform arms present and tested, see `src-tauri/src/terminal/shell.rs:293,327,347`)

### PTY locale injection assumes only two locale conventions (macOS vs. everything else)
- **ID**: L1-RUST-CFG-11
- **Severity**: LOW
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/terminal/pty.rs:32` — `let locale = if cfg!(target_os = "macos") {`
- **Why it breaks**: `utf8_locale_override` injects `LANG=C.UTF-8` for every non-macOS target, including Windows. Windows PTY shells (`cmd.exe`, PowerShell, `pwsh`) do not consult the POSIX `LANG`/`LC_*` variables for encoding at all — Windows console encoding is controlled by the code page (`chcp`) / `[Console]::OutputEncoding` — so injecting `LANG=C.UTF-8` there is inert rather than actively harmful, but it is dead logic on Windows PTYs (ConPTY) and could confuse anyone debugging why `LANG` is set but ignored.
- **Fix**: Narrow the `not(macos)` branch in `src-tauri/src/terminal/pty.rs` to `#[cfg(unix)]`/`cfg!(unix)` explicitly (i.e. skip the `LANG` injection entirely under `cfg!(target_os = "windows")`), so the function's contract is "sets POSIX locale env for POSIX shells" rather than implicitly running on Windows too.
- **Status**: OPEN
