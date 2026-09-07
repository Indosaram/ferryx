# Ferryx 크로스플랫폼 잠재 문제 감사 (Cross-Platform Issue Audit)

**감사일**: 2026-09-08
**대상 커밋**: `8d6bd2d` (branch `main`)
**범위**: `src-tauri/` (Rust, 189 files), `ui/` (React + TypeScript, 335 files), `scripts/`, `.github/workflows/`, Tauri 번들 설정
**검증**: 아래 모든 인용은 `scripts/verify-audit-citations.mjs`가 작업 트리에서 다시 읽어 확인했습니다 — 146/146 인용 해석 성공, 실패 0건.

---

## 요약 (Executive summary)

Ferryx는 macOS에서 개발·테스트되지만 Windows와 Linux를 정식 배포 대상으로 삼습니다 (`dmg`, `app`, `nsis`, `msi`, `appimage`, `deb`). 조건부 컴파일 분포가 이 비대칭을 그대로 드러냅니다: `cfg(target_os = "macos")` 167곳 대 `"windows"` 49곳, `"linux"` 31곳.

10개 도메인을 감사해 **146건**의 크로스플랫폼 결함을 찾았습니다.

- **BLOCKER 19건** — 해당 플랫폼에서 핵심 기능이 아예 동작하지 않거나 빌드/실행이 실패합니다.
- **HIGH 59건** — 기능이 조용히 비활성화되거나 사용자가 체감하는 오동작이 발생합니다.
- **MEDIUM 41건** — 특정 조건에서 실패하거나 플랫폼 관례를 어깁니다.
- **LOW 27건** — 표면적 불일치, 미사용 코드 경로, 방어적 개선 대상.
- 이 중 **8건은 이미 수정되었거나 정상 확인된 항목**으로, 회귀 감시를 위해 FIXED로 기록했습니다.

영향 플랫폼 분포: Windows 122건, Linux 71건, macOS 7건.

### 한 줄 결론

> **지금 상태로 Windows/Linux 빌드를 배포하면 두 플랫폼 모두에서 앱의 핵심인 터미널이 정상 동작하지 않습니다.** Linux는 네이티브 터미널이 렌더링되지 않을 수 있고(XCB child surface 실패), 클립보드가 빈 값으로 스텁되어 있으며, wgpu alpha mode 폴백이 패닉으로 이어질 수 있습니다. Windows는 한글·CJK가 두부(tofu)로 렌더링되고 창의 최소화/최대화/닫기 버튼이 없습니다. 여기에 더해 두 플랫폼 모두 데몬 자동시작과 CLI 설치 경로가 없고, `.deb`/`.msi` 사용자는 자동 업데이트를 받지 못합니다.

### 권장 수정 순서

1. **Linux 네이티브 터미널 복구** — `L3-NATIVE-SURFACE-3` (XCB child surface), `L3-NATIVE-SURFACE-1` (WSLg 감지), `L4-RENDERER-FONT-1` (wgpu alpha mode 패닉). 하나라도 남으면 Linux에서 터미널이 비거나 앱이 죽습니다.
2. **CJK 렌더링 복구** — `L4-RENDERER-FONT-2` (Windows GDI 폰트 폴백 없음), `L4-RENDERER-FONT-3` (Linux FreeType 폴백 없음). 한국어 사용자에게는 사실상 사용 불가입니다.
3. **Windows 창 크롬과 입력** — `L6-PACKAGING-1` (캡션 버튼 부재), `L5-UI-FRONTEND-1` (터미널 제어문자와 전역 단축키 충돌).
4. **Windows 파일 핸들·경로 정합성** — `L2-FS-PATHS-1/2/3`. 저장이 간헐적으로 실패하는 유형이라 데이터 신뢰성에 직결됩니다.
5. **에이전트 실행 경로** — `L8-SHELL-AGENT-1` (Windows에서 PATHEXT/`.cmd` 미해석으로 에이전트 CLI 실행 불가).
6. **배포 경로 정리** — `L6-PACKAGING-4` (업데이터 아티팩트 누락), `L6-PACKAGING-2/3` (CLI 설치·자동시작 부재).

---

## BLOCKER 일람 (19건)

- **L10-TESTS-TOOLING-11** `Windows, Linux` — Majority of PTY and terminal tests lack Windows/Linux test coverage
- **L10-TESTS-TOOLING-2** `Windows` — Hardcoded `/bin/sh` shell paths in PTY tests (test-only defect, affects 40+ tests)
- **L2-FS-PATHS-1** `Windows` — Open file handle held during atomic rename in SSH project store persistence causes sharing violation on Windows
- **L2-FS-PATHS-2** `Windows` — Open file handle held during atomic rename in ferryx_scope SSH config persistence fails on Windows
- **L2-FS-PATHS-3** `Windows` — Worktree and PTY containment check fails on Windows due to verbatim `\\?\` prefix mismatch
- **L3-NATIVE-SURFACE-1** `Linux` — Missing WSLg detection causes Wayland subsurface mis-scaling and offset over app chrome
- **L3-NATIVE-SURFACE-2** `Linux` — Native terminal clipboard content retrieval is stubbed to empty on Linux
- **L3-NATIVE-SURFACE-3** `Linux` — Linux compositor target fails to create child surface for XCB window handles
- **L4-RENDERER-FONT-1** `Linux` — WGPU CompositeAlphaMode fallback triggers unreachable panic on Linux Wayland
- **L4-RENDERER-FONT-2** `Windows` — Windows GDI rasterizer lacks font fallback cascade causing CJK and Unicode to render as tofu
- **L4-RENDERER-FONT-3** `Linux` — Linux FreeType rasterizer lacks CJK fallback cascade and drops non-monospace scripts
- **L5-UI-FRONTEND-1** `Windows+Linux` — Terminal POSIX Control Codes Collide with Global Shortcuts on Windows and Linux
- **L5-UI-FRONTEND-2** `Windows` — Native Terminal Transparency Scoped to macOS
- **L6-PACKAGING-1** `Windows` — Windows Window Chrome Missing Minimize, Maximize, and Close Buttons
- **L6-PACKAGING-2** `Windows` — CLI Launcher Installation Symlink Logic Excludes Windows
- **L6-PACKAGING-3** `Windows+Linux` — Daemon Autostart Persistence Missing Implementation for Windows and Linux
- **L6-PACKAGING-4** `Windows+Linux` — Shipped Debian and MSI Packages Produce No Updater Manifest Artifacts
- **L8-SHELL-AGENT-1** `Windows` — Agent CLI binaries unresolvable on Windows (no PATHEXT/.cmd/.ps1 lookup)
- **L9-BROWSER-OS-1** `Windows` — Cookie import calls `Webview::set_cookie` synchronously inside an async command — documented WebView2 deadlock hazard on Windows

---

## 이미 수정되었거나 정상 확인된 항목 (FIXED, 8건)

아래는 과거에 실제로 발생했다가 수정된 결함, 또는 의심했으나 코드상 정상으로 확인된 지점입니다. 회귀 시 즉시 재발하므로 기록해 둡니다.

- **L1-RUST-CFG-8** — Full disk access / accessibility "open settings" deep link has no Linux target at all
  - `src-tauri/src/permissions/mod.rs:268` — `#[cfg(target_os = "macos")]`
- **L1-RUST-CFG-9** — Cargo.toml: notification click-routing crate is Windows+Linux only, no macOS parity concern (verified symmetric)
  - `src-tauri/Cargo.toml:143` — `[target.'cfg(any(target_os = "windows", target_os = "linux"))'.dependencies]`
- **L1-RUST-CFG-10** — `TargetPlatform::CURRENT` shell resolution correctly branches all three OSes (verified, no defect)
  - `src-tauri/src/terminal/shell.rs:262` — `} else if cfg!(target_os = "macos") {`
- **L3-NATIVE-SURFACE-17** — Physical KeyboardEvent.code KeyV/KeyC prioritized before layout-dependent key
  - `ui/src/components/NativeTerminalPane.tsx:233` - `    return event.code === code;`
- **L3-NATIVE-SURFACE-18** — Pointer-transparent child surface focus via non-consuming AppKit mouse event monitor
  - `src-tauri/src/lib.rs:551` - `        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::LeftMouseUp, &block)`
- **L5-UI-FRONTEND-2** — Native Terminal Transparency Scoped to macOS
  - `ui/src/index.css:182` - `html.platform-macos:has([data-testid="native-terminal-pane"]),`
- **L8-SHELL-AGENT-9** — OS-opener path-reveal comment claims macOS-only reasoning but branch coverage is actually correct — verify no fourth platform gap
  - `src-tauri/src/ipc/project.rs:327` — `#[cfg(not(any(target_os = "macos", target_os = "windows")))]`
- **L9-BROWSER-OS-12** — Named persistent browser profiles are macOS-unsupported by design, but the frontend has no platform gate preventing the request from being built
  - `src-tauri/src/ipc/browser.rs:874` — `return Err(BrowserError::UnsupportedProfile(`

---

## 상세 결과 (Detailed findings)

각 항목은 심각도, 영향 플랫폼, 실제 코드 위치(파일:라인 + 해당 라인의 코드), 깨지는 메커니즘, 구체적 수정 방법을 담습니다.

## A. 조건부 컴파일 매트릭스 (`cfg(target_os)`)

_11 findings — BLOCKER 0, HIGH 2, MEDIUM 2, LOW 7_

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

---

## B. 파일시스템, 경로, 프로세스, OS API

_16 findings — BLOCKER 3, HIGH 8, MEDIUM 4, LOW 1_

### Open file handle held during atomic rename in SSH project store persistence causes sharing violation on Windows
- **ID**: L2-FS-PATHS-1
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ssh/projects.rs:131` - `        std::fs::rename(&temp, &path)`
- **Why it breaks**: In `save()`, `let mut file = options.open(&temp)?;` holds an open file handle that remains in scope when `std::fs::rename(&temp, &path)` is called. On Windows NT, `MoveFileExW` fails with `ERROR_SHARING_VIOLATION` or `ERROR_ACCESS_DENIED` if any handle to the source file is open without delete-sharing flags. If rename fails, the cleanup `std::fs::remove_file(&temp)` also fails for the same reason, causing remote project saving to fail completely on Windows.
- **Fix**: Explicitly drop the file handle (`drop(file);`) after `file.sync_all()?;` and before calling `std::fs::rename(&temp, &path)`.
- **Status**: OPEN

### Open file handle held during atomic rename in ferryx_scope SSH config persistence fails on Windows
- **ID**: L2-FS-PATHS-2
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ferryx_scope/ssh/config.rs:50` - `            std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())`
- **Why it breaks**: In `ConfigStore::save()`, `let mut file = options.open(&tmp)...` keeps `file` in scope during `std::fs::rename(&tmp, &self.path)`. On Windows, attempting to rename an open file triggers a sharing violation error (`ERROR_SHARING_VIOLATION`), and the subsequent fallback `std::fs::remove_file(tmp)` also fails because the handle is still open.
- **Fix**: Explicitly drop `file` via `drop(file);` after `file.sync_all()` before calling `std::fs::rename(&tmp, &self.path)`.
- **Status**: OPEN

### Worktree and PTY containment check fails on Windows due to verbatim `\\?\` prefix mismatch
- **ID**: L2-FS-PATHS-3
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ferryx_scope/ssh/helper.rs:70` - `                if !cwd.starts_with(root) { return Err("FORBIDDEN: cwd outside project".into()); }`
- **Why it breaks**: On Windows, `std::fs::canonicalize` prepends the `\\?\` verbatim namespace prefix (e.g. `\\?\C:\repo`). Because `root` is not guaranteed to have the `\\?\` prefix, component-wise `starts_with(root)` returns `false`. This causes `pty.spawn` (and `worktree.create` at line 60) to unconditionally reject valid paths with `FORBIDDEN` on Windows.
- **Fix**: Strip verbatim prefixes before comparing (using `crate::worktree::git::strip_verbatim_prefix` or `crate::daemon::server::normalize_process_cwd`), or canonicalize both paths before calling `starts_with`.
- **Status**: OPEN

### Agent session discovery executes `/bin/ps` and `/usr/sbin/lsof` which do not exist on Windows
- **ID**: L2-FS-PATHS-10
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/ipc/agents.rs:72` - `    let output = crate::util::no_window_command("/bin/ps")`
- **Why it breaks**: `process_table_entries` executes `/bin/ps` and `lsof_session_id` (`src-tauri/src/ipc/agents.rs:193`) executes `/usr/sbin/lsof`. Neither binary exists on Windows, causing agent session discovery for Claude, Codex, Copilot, Cursor, Kimi, Omo, and Antigravity to silently fail. On Linux, `/usr/sbin/lsof` also fails on distributions where `lsof` is located in `/usr/bin/lsof`.
- **Fix**: On Windows, use Win32 process enumeration APIs (`CreateToolhelp32Snapshot`/`Process32Next`) instead of `/bin/ps`, and query open handles via `NtQuerySystemInformation`. On Unix, invoke `lsof` and `ps` via `PATH` lookup instead of hardcoding absolute paths.
- **Status**: OPEN

### Open process CWD locks prevent `git worktree remove` from deleting worktree directory on Windows
- **ID**: L2-FS-PATHS-11
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/worktree/manager.rs:551` - `        git_worktree_remove(&self.repo_root, &canonical, force)?;`
- **Why it breaks**: `remove_worktree_locked` invokes `git worktree remove`, which attempts to delete the worktree directory on disk. On Windows, the operating system locks directories that are the current working directory of any running process (such as an open terminal shell or child agent); `git worktree remove` fails with `Permission denied` even if `force` is true.
- **Fix**: Before calling `git_worktree_remove`, terminate any active terminal sessions whose working directory is inside the worktree, or implement retry logic with exponential backoff on Windows.
- **Status**: OPEN

### Hardcoded `/tmp/ferryx-switch-debug.jsonl` fails debug log command and silently discards traces on Windows
- **ID**: L2-FS-PATHS-4
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/debug.rs:48` - `            .open("/tmp/ferryx-switch-debug.jsonl")`
- **Why it breaks**: `cmd_switch_debug_log` attempts to open `/tmp/ferryx-switch-debug.jsonl` directly. On Windows, the `/tmp` directory does not exist by default, causing the Tauri command to return an `IoError` to the frontend and fail. The same hardcoded path is also opened in `lib.rs` (lines 373, 426, 841) and `native_terminal.rs` (lines 1315, 1429).
- **Fix**: Replace `/tmp/ferryx-switch-debug.jsonl` with a platform-agnostic path using `std::env::temp_dir().join("ferryx-switch-debug.jsonl")` or `crate::daemon::server::get_runtime_dir().join("switch-debug.jsonl")`.
- **Status**: OPEN

### Worktree and branch ref validators permit NTFS-illegal characters and reserved DOS device names
- **ID**: L2-FS-PATHS-5
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/worktree/manager.rs:388` - `                    || matches!(ch, '~' | '^' | ':' | '?' | '*' | '[' | '\\')`
- **Why it breaks**: `validate_ref_component` rejects `~`, `^`, `:`, `?`, `*`, `[`, `\`, but permits `<`, `>`, `"`, and `|`, which are illegal characters on Windows NTFS. Furthermore, neither `validate_ref_component` nor `WorkspaceRegistry::validate_workspace_id` rejects DOS reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1-9`, `LPT1-9`). When `worktree_path_for` joins the workspace ID and slug into a filesystem path, directory creation or `git worktree add` fails on Windows with `ERROR_INVALID_NAME`.
- **Fix**: Extend `validate_ref_component` and `validate_workspace_id` to reject `<`, `>`, `"`, `|`, and check that no path segment case-insensitively matches Windows reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`..`COM9`, `LPT1`..`LPT9`).
- **Status**: OPEN

### Workspace ID validator allows NTFS alternate data stream colons and illegal path characters
- **ID**: L2-FS-PATHS-6
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/worktree/registry.rs:30` - `            || workspace_id.contains('\\')`
- **Why it breaks**: `validate_workspace_id` only checks for `-`, `/`, `\`, control characters, and whitespace. It allows `:`, which on Windows NTFS denotes an Alternate Data Stream (ADS); joining `self.repo_root.join(".orca-worktrees").join(ws_id)` with a colon in `ws_id` targets an ADS instead of a directory, or fails with an invalid path syntax error.
- **Fix**: Add `:` and characters `< > " | ? *` to the invalid character check in `WorkspaceRegistry::validate_workspace_id`.
- **Status**: OPEN

### Remote auth credentials saved with Unix mode bits (0o600/0o700) and no Windows ACL restriction
- **ID**: L2-FS-PATHS-7
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/remote/auth.rs:300` - `        let _ = std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600));`
- **Why it breaks**: Remote auth pairing secrets and device auth records are written using `std::os::unix::fs::PermissionsExt::from_mode`, which is cfg-gated to Unix. On Windows, file permissions are not set at all, leaving credentials inheriting default parent ACLs and readable by any unprivileged user on the local machine.
- **Fix**: Add a Windows branch using `icacls` or Win32 security descriptor APIs (similar to `crate::ferryx_scope::ssh::private_file`) to restrict ACLs on the auth directory and file to the current user SID (`%USERNAME%:(F)`).
- **Status**: OPEN

### SSH project store creation mode bit (0o600) has no Windows ACL equivalent
- **ID**: L2-FS-PATHS-8
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ssh/projects.rs:126` - `            options.mode(0o600);`
- **Why it breaks**: `save()` configures `options.mode(0o600)` inside `#[cfg(unix)]`. On Windows, the store file containing sensitive SSH host configurations and project paths is created without restricted ACLs, making it readable by all users on a shared Windows machine.
- **Fix**: Apply ACL restrictions on Windows using `crate::ferryx_scope::ssh::private_file(&temp)` before writing sensitive SSH project data.
- **Status**: OPEN

### Process cwd lookup stub returns `None` on Windows, breaking agent session detection
- **ID**: L2-FS-PATHS-9
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/terminal.rs:980` - `        None`
- **Why it breaks**: `process_cwd` is implemented for Linux (`/proc/{pid}/cwd`) and macOS (`proc_pidinfo`/`lsof`), but returns `None` on all other operating systems. In `src-tauri/src/ipc/agents.rs`, `opencode_session_id` (line 342) and `pi_session_id` (line 448) use `process_cwd(...)?`, causing agent session detection for OpenCode and Pi to immediately return `None` on Windows.
- **Fix**: Implement `process_cwd` for Windows using Win32 `NtQueryInformationProcess` to read `ProcessParameters->CurrentDirectory.DosPath` from the target process PEB, or query `GetProcessInformation`.
- **Status**: OPEN

### External URL launcher `cmd.exe /C start` misinterprets quoted URLs as window titles on Windows
- **ID**: L2-FS-PATHS-12
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/browser.rs:1604` - `            .args(["/C", "start", &valid_url])`
- **Why it breaks**: The Windows built-in `start` command treats its first quoted parameter as the optional console window title. When `valid_url` contains spaces, query parameters, or quotes requiring argument escaping, `cmd.exe /C start "<url>"` opens a blank command prompt titled with the URL instead of launching the default web browser. (Contrast with `cmd_open_file_path` at line 1639, which correctly passes an empty title `""`).
- **Fix**: Pass an empty string title argument before the URL: `.args(["/C", "start", "", &valid_url])`.
- **Status**: OPEN

### Ghostty config and theme discovery checks only Unix `HOME` and `XDG_CONFIG_HOME`, ignoring Windows `%APPDATA%`
- **ID**: L2-FS-PATHS-13
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/terminal/preferences.rs:619` - `    let home = env::var_os("HOME").map(PathBuf::from);`
- **Why it breaks**: `ghostty_config_candidates` and `theme_config_candidates` (line 495) only inspect `HOME`, `XDG_CONFIG_HOME`, and macOS application support directories. On Windows, Ghostty config and themes reside in `%APPDATA%\ghostty` (or `%LOCALAPPDATA%\ghostty`), and `HOME` is unset. Consequently, Ghostty configuration and themes cannot be discovered on Windows.
- **Fix**: Check `std::env::var_os("APPDATA")` on `#[cfg(windows)]` and add `%APPDATA%\ghostty\config` and `%APPDATA%\ghostty\themes` to `ghostty_config_candidates` and `theme_config_candidates`.
- **Status**: OPEN

### Handover manifest serialization sets Unix permissions without Windows ACL protection
- **ID**: L2-FS-PATHS-14
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/manifest.rs:40` - `            let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600));`
- **Why it breaks**: In `HandoverManifest::save_to_path`, the temporary manifest file is secured using `fs::Permissions::from_mode(0o600)`, which is cfg-gated to Unix. On Windows, the file is saved with default inherited permissions, allowing other local user accounts on multi-user systems to read internal daemon handover route tokens and socket paths.
- **Fix**: Add a `#[cfg(windows)]` branch setting a restricted security descriptor or calling `crate::ferryx_scope::ssh::private_file(&tmp_path)`.
- **Status**: OPEN

### Unit test `terminal_process_cwd_resolves_accurately` unconditionally asserts `process_cwd` succeeds
- **ID**: L2-FS-PATHS-15
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/tests.rs:607` - `    assert!(resolved.is_some(), "process_cwd should resolve current pid");`
- **Why it breaks**: Test-only defect: `terminal_process_cwd_resolves_accurately` tests `process_cwd(current_pid)` without a `#[cfg(unix)]` gate. Because `process_cwd` returns `None` on Windows (`src-tauri/src/ipc/terminal.rs:980`), this test panics with an assertion failure whenever `cargo test` is executed on Windows.
- **Fix**: Gate the test with `#[cfg(any(target_os = "linux", target_os = "macos"))]`, or update `process_cwd` with a Windows implementation.
- **Status**: OPEN

### Parent directory fsync via `File::open` fails on Windows during session persistence
- **ID**: L2-FS-PATHS-16
- **Severity**: LOW
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/session/mod.rs:226` - `        if let Ok(dir_file) = File::open(parent) {`
- **Why it breaks**: `save_session_to_path` attempts to fsync the parent directory to flush directory entry metadata by calling `File::open(parent)`. On Windows, standard `File::open` invokes `CreateFileW` without `FILE_FLAG_BACKUP_SEMANTICS`, which always fails with `PermissionDenied` when opening a directory handle.
- **Fix**: Gate directory fsync with `#[cfg(unix)]` or `#[cfg(not(windows))]`, as directory fsync is POSIX-specific and Windows metadata flushing is handled by the filesystem driver upon file handle closure.
- **Status**: OPEN

---

## C. 네이티브 터미널 표면: 윈도잉, 포커스, IME, 마우스, 클립보드

_18 findings — BLOCKER 3, HIGH 10, MEDIUM 3, LOW 2_

### Missing WSLg detection causes Wayland subsurface mis-scaling and offset over app chrome
- **ID**: L3-NATIVE-SURFACE-1
- **Severity**: BLOCKER
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/native_terminal/platform/linux.rs:303` - `            ) if std::env::var_os("FERRYX_DISABLE_WAYLAND_SUBSURFACE").is_none() => {`
- **Why it breaks**: Under WSLg's embedded Weston Wayland compositor, `wl_subsurface` positioning and buffer scaling ignore parent coordinate offsets and mis-scale across the window chrome. `linux.rs` checks only the manual `FERRYX_DISABLE_WAYLAND_SUBSURFACE` environment variable, lacking detection for WSLg environment variables (`WSL_DISTRO_NAME`, `WSL_INTEROP`) or automatic fallback to X11 (`GDK_BACKEND=x11`), breaking native terminal rendering on WSLg.
- **Fix**: In `LinuxCompositorTarget::new` (`src-tauri/src/native_terminal/platform/linux.rs`), detect WSLg via `std::env::var_os("WSL_DISTRO_NAME").is_some() || std::env::var_os("WSL_INTEROP").is_some()` and bypass `WaylandChild::create`, forcing X11 child window fallback.
- **Status**: OPEN

### Native terminal clipboard content retrieval is stubbed to empty on Linux
- **ID**: L3-NATIVE-SURFACE-2
- **Severity**: BLOCKER
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/ipc/native_terminal.rs:409` - `    (NativeTerminalClipboardContent::Empty, Vec::new())`
- **Why it breaks**: On Linux, `read_native_pasteboard()` is a stub returning `(NativeTerminalClipboardContent::Empty, Vec::new())`, causing `cmd_native_terminal_clipboard_content` to unconditionally return `Empty`. When a user pastes via terminal shortcuts or context menus on Linux, no text is retrieved, leaving clipboard paste non-functional.
- **Fix**: In `src-tauri/src/ipc/native_terminal.rs`, implement `read_native_pasteboard()` for Linux using GTK's `gdk_clipboard_read_text_async` / `gtk_clipboard_wait_for_text` or the `arboard` crate to read UTF-8 strings from X11 and Wayland clipboards.
- **Status**: OPEN

### Linux compositor target fails to create child surface for XCB window handles
- **ID**: L3-NATIVE-SURFACE-3
- **Severity**: BLOCKER
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/native_terminal/platform/linux.rs:306` - `            _ => None,`
- **Why it breaks**: `LinuxCompositorTarget::new` matches only `(Xlib, Xlib)` and `(Wayland, Wayland)` pairs when constructing `child`. If Tauri provides an `Xcb` window handle, `child` evaluates to `None`, which disables pointer transparency, breaks viewport updates, and forces wgpu to render directly into the unisolated root window.
- **Fix**: In `src-tauri/src/native_terminal/platform/linux.rs`, implement an `XcbChild` struct using `xcb_create_window` and `xcb_shape_rectangles` with `XCB_SHAPE_SO_SET`, and handle `(LinuxWindowHandleInner::Xcb, LinuxDisplayHandleInner::Xcb)` to construct it.
- **Status**: OPEN

### Windows child HWND raised to HWND_TOP occludes HTML DOM overlays
- **ID**: L3-NATIVE-SURFACE-10
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/native_terminal/platform/windows.rs:325` - `                HWND_TOP as Hwnd,`
- **Why it breaks**: In `WindowsCompositorTarget::reveal`, the child HWND is unconditionally raised to `HWND_TOP` above WebView2 because WebView2 otherwise occludes the child. Because the terminal HWND sits above the webview in the Win32 window hierarchy, any overlapping DOM elements (popovers, toast notifications, autocomplete dialogs, command palettes) are occluded and hidden by the native terminal surface.
- **Fix**: In `src-tauri/src/native_terminal/platform/windows.rs`, replace top-level Win32 `WS_CHILD` window parenting with DirectComposition visual tree embedding or host WebView2 with transparency and order the terminal HWND below the WebView2 HWND via `WS_CLIPCHILDREN`/`SetWindowPos` relative to the child hierarchy.
- **Status**: OPEN

### Wayland subsurface geometry rounds fractional scale factors to integer, causing rendering distortion
- **ID**: L3-NATIVE-SURFACE-11
- **Severity**: HIGH
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/native_terminal/child_surface.rs:72` - `        let buffer_scale = bounds.scale_factor.round().max(1.0);`
- **Why it breaks**: `WaylandSubsurfaceGeometry::from_logical_bounds` rounds `bounds.scale_factor` to the nearest integer because `wl_surface.set_buffer_scale` only accepts integers. On Wayland environments using fractional display scaling (e.g., 125% or 150%), rounding forces the terminal buffer to render at 1x or 2x, resulting in mis-scaled, blurred, or truncated terminal surfaces relative to the parent GTK window.
- **Fix**: Bind the `wp_fractional_scale_manager_v1` and `wp_viewport` protocols in `wayland_child.rs` (`src-tauri/src/native_terminal/platform/wayland_child.rs`), allowing fractional buffer scaling and explicit source/destination viewport rectangle configuration.
- **Status**: OPEN

### Natural text editing chords intercept Windows key on Windows and Linux
- **ID**: L3-NATIVE-SURFACE-12
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/input.rs:114` - `    let super_only = mods.super_key && !mods.alt;`
- **Why it breaks**: `natural_text_editing_bytes` translates `super_key` chords (intended for macOS Cmd+Left/Right/Backspace) into readline control bytes (`\x01`, `\x05`, `\x15`). On Windows and Linux, `super_key` represents the Windows/Super key; pressing Win+Arrow (system window snapping) or Win+Backspace injects control characters into the terminal session instead of being handled by the operating system.
- **Fix**: Gate the `super_only` branch of `natural_text_editing_bytes` in `src-tauri/src/native_terminal/input.rs` with `#[cfg(target_os = "macos")]` (or runtime platform check), and add `#[cfg(not(target_os = "macos"))]` mappings that translate Ctrl+ArrowLeft / Ctrl+ArrowRight to word-movement sequences (`\x1bb`, `\x1bf`).
- **Status**: OPEN

### Pointer-transparent child surface focus via non-consuming AppKit mouse event monitor
- **ID**: L3-NATIVE-SURFACE-18
- **Severity**: HIGH
- **Platforms affected**: macOS
- **Evidence**: `src-tauri/src/lib.rs:551` - `        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::LeftMouseUp, &block)`
- **Why it breaks**: Because `FerryxNativeTerminalView` overrides `hitTest:` to return `nil` for pointer transparency, AppKit does not naturally trigger standard window activation or webview first responder updates on child view clicks. Installing a non-consuming AppKit local event monitor for mouse-up events detects clicks over terminal surfaces and emits `native_terminal_focus` to the frontend to restore textarea sink focus.
- **Fix**: Install non-consuming `NSEvent::addLocalMonitorForEventsMatchingMask_handler` for `LeftMouseUp` in `install_macos_terminal_focus_monitor` (`src-tauri/src/lib.rs:513-558`).
- **Status**: FIXED (implemented in `src-tauri/src/lib.rs:513-558`)

### File drag-and-drop destination implemented only for macOS, missing on Windows and Linux
- **ID**: L3-NATIVE-SURFACE-4
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/lib.rs:858` - `            install_macos_file_drop_destination(app)?;`
- **Why it breaks**: macOS registers `FerryxFileDropView` via `install_macos_file_drop_destination` to capture dropped file URLs and forward them to the frontend via the `ferryx://file-drop` event. Windows and Linux lack native drag-and-drop targets (`IDropTarget` / `RegisterDragDrop` on Win32, `gtk_drag_dest_set` on Linux), causing file drags over native terminal panes to be dropped or discarded.
- **Fix**: Implement `install_windows_file_drop_destination` in `src-tauri/src/lib.rs` using Win32 OLE `RegisterDragDrop` on the window HWND, and implement `install_linux_file_drop_destination` using GTK drag-motion/drag-drop signals, emitting the `ferryx://file-drop` event.
- **Status**: OPEN

### Native terminal selection copy writes to clipboard only on macOS
- **ID**: L3-NATIVE-SURFACE-5
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/ipc/native_terminal.rs:1106` - `    #[cfg(not(target_os = "macos"))]`
- **Why it breaks**: In `cmd_native_terminal_copy_selection`, writing selection text to the system pasteboard is gated with `#[cfg(target_os = "macos")]`. On Windows and Linux, the command returns the text string without writing to the OS clipboard, relying on webview JavaScript clipboard access which fails when focus is captured by the native child surface.
- **Fix**: In `src-tauri/src/ipc/native_terminal.rs`, implement native pasteboard writing for Windows using Win32 `OpenClipboard`/`SetClipboardData(CF_UNICODETEXT)` and for Linux using GTK clipboard or `arboard`, executing inside `cmd_native_terminal_copy_selection`.
- **Status**: OPEN

### Windows and Linux child surfaces fail to hide when viewport bounds are None
- **ID**: L3-NATIVE-SURFACE-6
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/platform/windows.rs:293` - `            return;`
- **Why it breaks**: When viewport bounds are `None` (pane hidden, tab switched, or window minimized), `MacosCompositorTarget::update_viewport` calls `view.setHidden(true)` and zeros the frame. On Windows and Linux, `update_viewport` returns early without calling `ShowWindow(hwnd, SW_HIDE)` or `XUnmapWindow`, leaving orphaned native terminal child surfaces visible at their last geometry and occluding newly active tabs.
- **Fix**: In `WindowsCompositorTarget::update_viewport` (`src-tauri/src/native_terminal/platform/windows.rs`), call `ShowWindow(self.handle.hwnd.get() as Hwnd, SW_HIDE)` when `bounds` is `None`. In `LinuxCompositorTarget::update_viewport` (`src-tauri/src/native_terminal/platform/linux.rs`), call `XUnmapWindow` for X11 or detach the subsurface buffer for Wayland when `bounds` is `None`.
- **Status**: OPEN

### Native terminal scroll wheel monitor exists only on macOS, missing on Windows and Linux
- **ID**: L3-NATIVE-SURFACE-7
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/lib.rs:862` - `            install_macos_terminal_scroll_monitor(app, Arc::clone(&scroll_daemon_client))?;`
- **Why it breaks**: macOS intercepts wheel events via `install_macos_terminal_scroll_monitor` to translate mouse wheel delta into PTY arrow-key escapes or mouse tracking sequences in alternate screen mode (e.g., inside `vim` or `less`). On Windows and Linux, no native scroll monitor exists, forcing scrolling through the webview React `onWheel` handler which ignores mouse cursor coordinates and cannot natively drive curses alternate-screen scrolling.
- **Fix**: Implement `install_windows_terminal_scroll_monitor` via a low-level mouse hook (`WH_MOUSE_LL` on `WM_MOUSEWHEEL`) in `windows_focus.rs` and equivalent GDK/X11 event filtering on Linux, invoking `compute_wheel_outcome` and writing PTY sequences when alternate screen mode is active.
- **Status**: OPEN

### First responder focus restoration after frame presentation is stubbed on non-macOS
- **ID**: L3-NATIVE-SURFACE-8
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/platform/mod.rs:116` - `    pub fn restore_first_responder<R: Runtime>(&self, _window: &WebviewWindow<R>) {`
- **Why it breaks**: After every rendered frame, `surface_host.rs` calls `self.target.restore_first_responder(window)` to ensure keyboard focus remains with the hosting webview. In `platform/mod.rs`, `restore_first_responder` is compiled as a no-op on Windows and Linux, preventing keyboard focus recovery after native surface updates or window activations.
- **Fix**: In `src-tauri/src/native_terminal/platform/windows.rs`, implement `restore_webview_first_responder` by invoking `windows_focus::best_effort_focus_webview` to set Win32 focus to the WebView2 `Chrome_WidgetWin_1` window, and wire it to `PlatformCompositorTarget::restore_first_responder`.
- **Status**: OPEN

### Native terminal mouse focus monitor installed on macOS and Windows but absent on Linux
- **ID**: L3-NATIVE-SURFACE-9
- **Severity**: HIGH
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/lib.rs:864` - `            install_windows_terminal_focus_monitor(app)?;`
- **Why it breaks**: Both macOS (`install_macos_terminal_focus_monitor`) and Windows (`install_windows_terminal_focus_monitor`) install low-level mouse monitors to detect clicks over native surfaces and emit `native_terminal_focus` to the frontend. Linux has no terminal focus monitor, so clicking over an inactive terminal pane fails to trigger the `native_terminal_focus` IPC event to activate the hidden textarea focus sink.
- **Fix**: Implement `install_linux_terminal_focus_monitor` using GTK event controllers (`GtkGestureClick`) or X11/Wayland pointer event hooks on the parent window to detect button-up events over native terminal bounds and emit `NATIVE_TERMINAL_FOCUS_EVENT`.
- **Status**: OPEN

### `window_backing_scale_factor` capability only implemented on macOS, missing on Windows and Linux
- **ID**: L3-NATIVE-SURFACE-13
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/platform/mod.rs:59` - `    #[cfg(target_os = "macos")]`
- **Why it breaks**: `PlatformCompositorTarget` exposes `window_backing_scale_factor()` exclusively under `#[cfg(target_os = "macos")]`. On Windows and Linux, callers cannot query the hardware DPI scale factor through the compositor target, creating an API asymmetry that prevents backend platform-level scale factor queries.
- **Fix**: In `src-tauri/src/native_terminal/platform/windows.rs`, implement `window_backing_scale_factor` using `GetDpiForWindow(self.handle.hwnd.get() as Hwnd) as f64 / 96.0`, implement GTK window scale query on Linux in `linux.rs`, and expose them unconditionally in `PlatformCompositorTarget`.
- **Status**: OPEN

### Key encoder maps punctuation characters to unidentified with suppressed UTF-8 under Ctrl
- **ID**: L3-NATIVE-SURFACE-14
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/key_encoder.rs:156` - `            if !event.modifiers.ctrl && !event.modifiers.super_key {`
- **Why it breaks**: In `key_encoder.rs`, `map_key_code_to_c` maps all punctuation characters to `GHOSTTY_KEY_UNIDENTIFIED`. When `event.modifiers.ctrl` is active, the encoder suppresses UTF-8 fallback, preventing encoding of common control chords like `Ctrl+[` (Escape), `Ctrl+]`, and `Ctrl+\` when dispatched as `KeyCode::Character`.
- **Fix**: In `src-tauri/src/native_terminal/key_encoder.rs`, expand `map_key_code_to_c` to recognize ASCII punctuation (`'['`, `']'`, `'\\'`, `'/'`, etc.) and map them to their respective `GHOSTTY_KEY_*` constants, or allow ASCII control byte derivation when `modifiers.ctrl` is set on character keys.
- **Status**: OPEN

### Physical KeyboardEvent.code KeyV/KeyC prioritized before layout-dependent key
- **ID**: L3-NATIVE-SURFACE-17
- **Severity**: MEDIUM
- **Platforms affected**: macOS
- **Evidence**: `ui/src/components/NativeTerminalPane.tsx:233` - `    return event.code === code;`
- **Why it breaks**: Non-Latin keyboard layouts (such as Korean 2-Set) emit localized characters (`key: "ㅍ"` for physical V, `key: "ㅊ"` for physical C), causing standard Cmd+V paste and Cmd+C copy shortcuts to fail when matching against layout-dependent `event.key`. Prioritizing `event.code` ("KeyV", "KeyC") before `event.key` resolves the physical shortcut regardless of active keyboard layout.
- **Fix**: Prioritize `event.code === code` before checking `event.key` in `isShortcutKey` (`ui/src/components/NativeTerminalPane.tsx:232-236`).
- **Status**: FIXED (implemented in `ui/src/components/NativeTerminalPane.tsx:232-236` and verified by tests in `ui/src/components/NativeTerminalPane.test.tsx:1754-1760`)

### Pixel-perfect nearest-neighbour scaling configuration exists only for macOS Metal layers
- **ID**: L3-NATIVE-SURFACE-15
- **Severity**: LOW
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/platform/macos.rs:185` - `unsafe fn configure_terminal_layers(view: &AnyObject, scale_factor: f64) {`
- **Why it breaks**: On macOS, `configure_terminal_layers` traverses the layer hierarchy to configure `setContentsGravity: topLeft` and `setMagnificationFilter: nearest` on the CAMetalLayer, preventing bilinear blurring during fractional resizing. Windows and Linux lack equivalent swapchain or surface scaling configurations, allowing fractional layout dimensions to cause bilinear filtering softness.
- **Fix**: In `WindowsCompositorTarget` (`src-tauri/src/native_terminal/platform/windows.rs`) and `LinuxCompositorTarget` (`src-tauri/src/native_terminal/platform/linux.rs`), configure swapchain scaling modes or viewport snapping so native presentation avoids bilinear interpolation on fractional scale factors.
- **Status**: OPEN

### Drag-and-drop coordinate test suite is gated to macOS only (test-only defect)
- **ID**: L3-NATIVE-SURFACE-16
- **Severity**: LOW
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/tests/native_terminal_drag_drop_coordinates.rs:6` - `#![cfg(all(feature = "native-terminal", target_os = "macos"))]`
- **Why it breaks**: (Test-only defect) The integration test for native terminal drag-and-drop coordinate resolution is gated with `#![cfg(all(feature = "native-terminal", target_os = "macos"))]`. The coordinate math and backing scale factor division tests are never executed on Windows or Linux CI runners, leaving non-macOS coordinate mapping logic unverified.
- **Fix**: Remove the top-level `#![cfg(target_os = "macos")]` gate from `src-tauri/tests/native_terminal_drag_drop_coordinates.rs` and isolate only the `FerryxNativeTerminalView` test behind `#[cfg(target_os = "macos")]`, allowing the platform-agnostic `logical_from_raw` tests to run on all platforms.
- **Status**: OPEN

---

## D. 렌더러, 폰트, 글리프 래스터화

_18 findings — BLOCKER 3, HIGH 7, MEDIUM 7, LOW 1_

### WGPU CompositeAlphaMode fallback triggers unreachable panic on Linux Wayland
- **ID**: L4-RENDERER-FONT-1
- **Severity**: BLOCKER
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/gpu_context.rs:15` - `        .unwrap_or(wgpu::CompositeAlphaMode::Auto)`
- **Why it breaks**: When `Opaque` compositing is not supported (standard for Wayland Vulkan subsurfaces which only expose `PreMultiplied`), `opaque_composite_alpha_mode` defaults to `CompositeAlphaMode::Auto`. In `wgpu-core` 30.0.1 (`device/surface_config.rs`), `Auto` only checks `[Opaque, Inherit]` and hits an `unreachable!("Fallback system failed to choose alpha mode...")` panic, crashing the entire application during surface configuration.
- **Fix**: In `src-tauri/src/native_terminal/renderer/gpu_context.rs::opaque_composite_alpha_mode`, inspect `alpha_modes` and fall back to `alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto)` so `PreMultiplied` or any driver-supported mode is chosen when `Opaque` and `Inherit` are missing.
- **Status**: OPEN

### Windows GDI rasterizer lacks font fallback cascade causing CJK and Unicode to render as tofu
- **ID**: L4-RENDERER-FONT-2
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/native_terminal/renderer/directwrite_raster.rs:144` - `        let font = CreateFontW(`
- **Why it breaks**: `directwrite_raster.rs` rasterizes glyphs via legacy Win32 GDI `CreateFontW` rather than DirectWrite, passing only the primary font face with zero font fallback cascade logic. When rendering Korean, Japanese, Chinese, mathematical symbols, or box-drawing characters absent from the primary font (e.g. Consolas), GDI produces missing-glyph tofu rectangles or blank cells, leaving East Asian text unreadable.
- **Fix**: Replace Win32 GDI with DirectWrite (`IDWriteFactory::CreateTextLayout` and `IDWriteFontFallback::MapCharacters`) or implement a font fallback cascade that maps unhandled codepoints to fallback faces such as "Malgun Gothic" / "Meiryo" / "Microsoft YaHei" / "Segoe UI Symbol" before rasterization.
- **Status**: OPEN

### Linux FreeType rasterizer lacks CJK fallback cascade and drops non-monospace scripts
- **ID**: L4-RENDERER-FONT-3
- **Severity**: BLOCKER
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/freetype_raster.rs:182` - `    let Some(path) = resolve_font_path(family, ch, bold, italic)`
- **Why it breaks**: `freetype_raster.rs` attempts to match glyphs against only the user-configured font family and the literal fallback `"monospace"`. Standard Linux monospace fonts (e.g. DejaVu Sans Mono, Liberation Mono) do not contain CJK glyphs, and because no system fallback cascade is queried when "monospace" lacks the character, `resolve_font_path` returns `None` and Korean/Japanese/Chinese text fails to rasterize, displaying as blank tofu cells.
- **Fix**: In `src-tauri/src/native_terminal/renderer/freetype_raster.rs::resolve_font_path`, query Fontconfig without restricting family (e.g. `format!(":charset={:x}", ch as u32)`) when both the configured family and "monospace" lack coverage, letting Fontconfig resolve the system CJK font (e.g. Noto Sans CJK).
- **Status**: OPEN

### WGPU adapter request with compatible_surface: None fails on hybrid GPU Windows and Linux systems
- **ID**: L4-RENDERER-FONT-10
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/gpu_context.rs:59` - `            compatible_surface: None,`
- **Why it breaks**: `GpuContext::new` initializes the GPU adapter requesting `HighPerformance` with `compatible_surface: None`. On hybrid-graphics laptops (e.g. Intel/Nvidia Optimus on Windows, or Prime on Linux), the discrete GPU adapter selected may not be compatible with the display window surface created later, resulting in `surface.get_capabilities(&self.adapter).formats` returning an empty slice and `configure_surface` failing with `GpuPipelineError`.
- **Fix**: In `src-tauri/src/native_terminal/renderer/gpu_context.rs`, validate surface compatibility in `configure_surface` and fall back to requesting an adapter compatible with the actual `wgpu::Surface` if `cap.formats.is_empty()`.
- **Status**: OPEN

### Color emoji rasterization is completely unimplemented on Windows and Linux
- **ID**: L4-RENDERER-FONT-4
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/color_glyph.rs:191` - `    pub fn rasterize_color_glyph(`
- **Why it breaks**: `color_glyph.rs` stubs out `rasterize_color_glyph` on `not(target_os = "macos")` to unconditionally return `None`, while the macOS implementation rasterizes via CoreText and Apple Color Emoji into RGBA buffers. On Windows and Linux, all emoji codepoints fail the color path and fall through to monochrome alpha rasterization, where GDI renders empty/corrupt glyphs and FreeType renders flat uncolored shapes.
- **Fix**: Implement `rasterize_color_glyph` for Windows using DirectWrite with `DWRITE_GLYPH_IMAGE_FORMATS_COLR | DWRITE_GLYPH_IMAGE_FORMATS_PNG | DWRITE_GLYPH_IMAGE_FORMATS_SVG` targeting "Segoe UI Emoji", and for Linux using FreeType with `FT_LOAD_COLOR` targeting "Noto Color Emoji".
- **Status**: OPEN

### Non-macOS cell metrics hardcode arbitrary 0.6 and 1.25 ratios instead of querying font metrics
- **ID**: L4-RENDERER-FONT-5
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/font_manager.rs:133` - `                width_px: (fs * 0.6).round().max(1.0) as u32,`
- **Why it breaks**: On Windows and Linux, `FontManager::cell_metrics_for_scale` ignores the configured font and returns hardcoded synthetic metrics `(fs * 0.6)` by `(fs * 1.25)`. Actual monospace fonts on Windows (Consolas, Cascadia Code) and Linux (DejaVu Sans Mono) have advance widths and line height ratios differing from 0.6 / 1.25, leading to severe cell-to-glyph misalignment, clipped ascenders/descenders, and broken box-drawing continuous lines.
- **Fix**: In `src-tauri/src/native_terminal/renderer/font_manager.rs`, derive real font metrics for Windows via Win32 `GetTextMetricsW` / DirectWrite `GetMetrics`, and for Linux via FreeType `FT_FaceRec` (`max_advance_width`, `ascender`, `descender`, `height`).
- **Status**: OPEN

### Comma-separated font stack passed unparsed to Windows GDI exceeding LF_FACESIZE
- **ID**: L4-RENDERER-FONT-6
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/native_terminal/renderer/directwrite_raster.rs:115` - `    let face = wide(family);`
- **Why it breaks**: Ferryx's default font stack (`'MesloLGS NF, "Noto Sans KR", monospace'`) is passed unparsed as a raw string to `directwrite_raster.rs`. Win32 GDI `CreateFontW` accepts only a single typeface name up to 32 characters (`LF_FACESIZE`); because the 39-character comma-separated string exceeds `LF_FACESIZE` and does not match any single font, GDI rejects it and falls back to the proportional system GUI font (e.g. MS Sans Serif), breaking fixed-width terminal rendering entirely.
- **Fix**: Parse comma-separated font stacks in `FontManager` or `directwrite_raster.rs`, strip quotes, and resolve the first locally installed font name before invoking Win32 GDI `CreateFontW`.
- **Status**: OPEN

### Linux FreeType rasterizer reloads fontconfig and opens font files from disk per glyph
- **ID**: L4-RENDERER-FONT-7
- **Severity**: HIGH
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/freetype_raster.rs:134` - `        let config = FcInitLoadConfigAndFonts();`
- **Why it breaks**: For every single glyph cluster rasterized, `resolve_font_path` calls `FcInitLoadConfigAndFonts()` to re-parse all fontconfig XML files from disk, followed by `FT_New_Face` to read and parse the font binary, and `FT_Done_Face` to discard it. This causes extreme disk I/O, file descriptor churn, and massive frame stuttering during terminal scrolling and viewport resizes.
- **Fix**: Use `FcConfigGetCurrent()` instead of `FcInitLoadConfigAndFonts()`, and maintain a thread-safe cache of open `FT_Face` handles keyed by font path and size rather than opening and destroying faces per glyph.
- **Status**: OPEN

### Missing ntdll link dependency for Zig static library on Windows MSVC
- **ID**: L4-RENDERER-FONT-8
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/native_terminal/build_ghostty.rs:369` - `    println!("cargo:rustc-link-lib=static={}", config.link_lib_stem);`
- **Why it breaks**: `GhosttyLibVt.zig` configures Windows static builds to link against NT system libraries (`lib.root_module.linkSystemLibrary("ntdll", .{})`), but `build_ghostty.rs` only instructs cargo to link `config.link_lib_stem`. When linking with MSVC (`x86_64-pc-windows-msvc`), `ntdll.lib` is not linked by default by `rustc`, causing unresolved external symbol linker errors for NT runtime APIs.
- **Fix**: In `src-tauri/native_terminal/build_ghostty.rs`, add `if config.target.contains("windows") { println!("cargo:rustc-link-lib=ntdll"); }`.
- **Status**: OPEN

### Missing pkg-config probing and linker search paths for FreeType and Fontconfig on Linux
- **ID**: L4-RENDERER-FONT-9
- **Severity**: HIGH
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/freetype_raster.rs:70` - `#[link(name = "freetype")]`
- **Why it breaks**: `freetype_raster.rs` declares direct `#[link(name = "freetype")]` and `#[link(name = "fontconfig")]` bindings without any `pkg-config` probe in `src-tauri/build.rs`. On Linux distributions where development libraries reside in multiarch paths (e.g. `/usr/lib/x86_64-linux-gnu`) or custom prefixes, the linker fails to locate `-lfreetype` or `-lfontconfig`, breaking native terminal compilation.
- **Fix**: In `src-tauri/build.rs`, add a Linux target branch that probes `pkg_config::Config::new().probe("freetype2")` and `pkg_config::Config::new().probe("fontconfig")` to emit `cargo:rustc-link-search` and `cargo:rustc-link-lib` directives.
- **Status**: OPEN

### Linux FreeType rasterizer drops combining characters in multi-codepoint grapheme clusters
- **ID**: L4-RENDERER-FONT-11
- **Severity**: MEDIUM
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/freetype_raster.rs:176` - `    let Some(ch) = text.chars().next().filter(|c| !c.is_whitespace()) else {`
- **Why it breaks**: `freetype_raster::rasterize_to_alpha_buffer` extracts only `text.chars().next()`, completely discarding all trailing characters in `text`. Any terminal cell containing a combining mark (such as accent marks like `e` + `\u{0301}`, Hangul jamo sequences, or ZWJ sequences) renders only the base character with all combining diacritics stripped.
- **Fix**: Iterate through all characters in `text`, resolve and render each glyph into the buffer using additive blending and horizontal offset tracking, matching the macOS combining-mark loop in `FontManager::rasterize_glyph_for_scale`.
- **Status**: OPEN

### Windows GDI rasterizer lacks vertical baseline positioning and draws glyphs at cell top
- **ID**: L4-RENDERER-FONT-12
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/native_terminal/renderer/directwrite_raster.rs:168` - `        TextOutW(dc, 0, 0, glyphs.as_ptr(), glyph_len);`
- **Why it breaks**: Win32 GDI `TextOutW` draws glyphs at `(0, 0)` with default top-left alignment rather than positioning along a typographic baseline. Glyphs with descenders ('g', 'y', 'p') are pushed downward and clipped at the bottom cell boundary, while uppercase characters and box-drawing lines appear shifted upward and misaligned with adjacent horizontal rows.
- **Fix**: Call `GetTextMetricsW(dc, &mut tm)` in `directwrite_raster.rs` to query `tmAscent`, set `SetTextAlign(dc, TA_TOP | TA_LEFT)`, and calculate vertical offset `(cell_height - (tm.tmAscent + tm.tmDescent)) / 2` to vertically center characters within the cell.
- **Status**: OPEN

### Subpixel coverage rendering and subpixel positioning are macOS-exclusive
- **ID**: L4-RENDERER-FONT-13
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/font_manager.rs:299` - `        if subpixel_buffer.iter().any(|&b| b != 0) {`
- **Why it breaks**: On Windows and Linux, `rasterize_glyph_for_scale` invokes only the monochrome `rasterize_to_alpha_buffer` path, leaving `subpixel_buffer` all zeroes and forcing `FontManager` to always return `RasterizedGlyph::Alpha`. Subpixel positioning and RGBA subpixel antialiasing implemented via CoreGraphics on macOS are completely unavailable on Windows and Linux, resulting in noticeably blurrier text on standard-DPI LCD monitors.
- **Fix**: Implement subpixel RGBA coverage rendering in `directwrite_raster.rs` (using DirectWrite `IDWriteBitmapRenderTarget` with ClearType) and in `freetype_raster.rs` (using `FT_RENDER_MODE_LCD`), populating `subpixel_buffer`.
- **Status**: OPEN

### Variable font weight instantiation and synthetic styles missing on Windows and Linux
- **ID**: L4-RENDERER-FONT-14
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/font_manager.rs:21` - `    system: Mutex<CoreTextFontSystem>,`
- **Why it breaks**: `FontManager` encapsulates `CoreTextFontSystem` solely on macOS; on Windows and Linux, `system` is replaced by a bare `Mutex<f32>` font size. Features implemented in `coretext_font.rs` and `coretext_raster.rs`—such as variable font `wght` axis interpolation via `kCTFontVariationAttribute`, synthetic bold horizontal dilation, and synthetic italic row shearing—have no Windows or Linux equivalent, causing bold/italic text in fonts lacking separate face files to render as plain regular text.
- **Fix**: Introduce a cross-platform font system trait implemented by `CoreTextFontSystem`, DirectWrite on Windows (supporting `IDWriteFontFace5` variable axes), and FreeType on Linux (supporting `FT_Set_Var_Design_Coordinates`, `FT_Outline_Embolden`, and `FT_Outline_Transform`).
- **Status**: OPEN

### Per-glyph GDI allocation thrashing risks process quota exhaustion on Windows
- **ID**: L4-RENDERER-FONT-15
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/native_terminal/renderer/directwrite_raster.rs:125` - `        let dc = CreateCompatibleDC(std::ptr::null_mut());`
- **Why it breaks**: `directwrite_raster::rasterize_to_alpha_buffer` creates and releases a device context (`CreateCompatibleDC`), a DIB section (`CreateDIBSection`), and a font (`CreateFontW`) for every single glyph rasterized. Under heavy atlas filling or during rapid terminal streaming, this per-glyph GDI object thrashing incurs high kernel transition latency and risks triggering Windows' 10,000 GDI object per-process limit if any allocation fails to clean up cleanly.
- **Fix**: Cache and reuse a thread-local memory DC and DIB section, and retain an LRU cache of `HFONT` handles keyed by font family, size, weight, and slant.
- **Status**: OPEN

### Secondary color glyph fallback path is gated exclusively to macOS
- **ID**: L4-RENDERER-FONT-16
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/font_manager.rs:255` - `            if !rendered_base && is_secondary_color_candidate(first_ch) {`
- **Why it breaks**: `FontManager::rasterize_glyph_for_scale` implements a secondary fallback for arrows and miscellaneous technical symbols (e.g. `is_secondary_color_candidate`), rasterizing them as color glyphs if the primary alpha font produces no ink. This entire fallback check is enclosed inside `#[cfg(target_os = "macos")]`, so on Windows and Linux, arrow and technical symbols that lack glyphs in the primary face are never routed to secondary color rendering.
- **Fix**: Move the `is_secondary_color_candidate` check outside the `#[cfg(target_os = "macos")]` block in `src-tauri/src/native_terminal/renderer/font_manager.rs` and provide a cross-platform color rasterizer implementation.
- **Status**: OPEN

### Font manager test assertions assume 4-byte subpixel buffers and panic on Windows and Linux
- **ID**: L4-RENDERER-FONT-17
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/font_manager.rs:354` - `            (metrics.width_px * metrics.height_px * 4) as usize`
- **Why it breaks**: Test-only: `font_manager::tests` (`test_font_manager_derives_nonzero_metrics_and_rasterizes`, `test_glyph_orientation_regression`, etc.) assert that `rasterize_glyph` returns a buffer of length `width * height * 4` and index bytes at `[idx + 3]`. Because Windows and Linux only generate 1-byte-per-pixel `RasterizedGlyph::Alpha` buffers, these assertions fail and slice indexing panics with index-out-of-bounds on non-macOS test runs.
- **Fix**: Update `font_manager.rs` unit tests to inspect `RasterizedGlyph::is_subpixel()` and scale the expected buffer length and stride dynamically (4 for subpixel, 1 for alpha).
- **Status**: OPEN

### Default terminal font stack hardcodes macOS and Nerd Font families without Windows/Linux fallbacks
- **ID**: L4-RENDERER-FONT-18
- **Severity**: LOW
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/lib/tauri.ts:3` - `export const DEFAULT_TERMINAL_FONT_STACK = 'MesloLGS NF, "Noto Sans KR", monospace';`
- **Why it breaks**: The default terminal font stack hardcodes `MesloLGS NF` and `Noto Sans KR`, neither of which is bundled or pre-installed on standard Windows or Linux installations. Windows users lack "Cascadia Mono" or "Consolas", and Linux users lack "DejaVu Sans Mono" or "Liberation Mono" in the default font stack, causing Ferryx to immediately hit fallbacks on vanilla OS setups.
- **Fix**: In `ui/src/lib/tauri.ts`, update `DEFAULT_TERMINAL_FONT_STACK` to include cross-platform monospace families: `'MesloLGS NF, "Cascadia Mono", Consolas, "DejaVu Sans Mono", "Noto Sans KR", monospace'`.
- **Status**: OPEN

---

## E. 프론트엔드(React/TypeScript) 플랫폼 가정

_18 findings — BLOCKER 2, HIGH 8, MEDIUM 5, LOW 3_

### Terminal POSIX Control Codes Collide with Global Shortcuts on Windows and Linux
- **ID**: L5-UI-FRONTEND-1
- **Severity**: BLOCKER
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/lib/shortcuts.ts:407` - `          !isTerminalTarget(event.target) &&`
- **Why it breaks**: On Windows and Linux, `mod: true` maps to `Ctrl`, and `useShortcuts` specifically excludes terminal targets from the editable target check (`!isTerminalTarget`), allowing global application shortcuts to fire during active terminal sessions. This causes critical POSIX terminal control signals to be intercepted: `Ctrl+D` (terminal EOF) triggers `terminal.splitRight`, `Ctrl+W` (readline backward-kill-word) triggers `tab.close` and kills the session, `Ctrl+B` (tmux prefix) triggers `sidebar.left.toggle`, `Ctrl+K` (kill line) triggers `commandPalette.open`, and `Ctrl+T` (transpose) triggers `tab.newTerminal`.
- **Fix**: In `ui/src/lib/shortcuts.ts`, guard terminal targets so bare `mod` shortcuts that conflict with standard terminal control sequences are skipped when `!isMacShortcutPlatform()`, or rebind conflicting app-level shortcuts to `Alt` or `Ctrl+Shift` chords on Windows and Linux.
- **Status**: OPEN

### Native Terminal Transparency Scoped to macOS
- **ID**: L5-UI-FRONTEND-2
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `ui/src/index.css:182` - `html.platform-macos:has([data-testid="native-terminal-pane"]),`
- **Why it breaks**: An earlier unscoped version of the native terminal transparency rules applied `background-color: transparent !important` globally across all operating systems. On Windows, WebView2 rendered an entirely black window when background transparency was enabled without DWM composition support.
- **Fix**: Scoped all terminal transparency CSS rules strictly to `html.platform-macos` in `ui/src/index.css:182-197`.
- **Status**: FIXED (handled in `ui/src/index.css:182-197` and `ui/src/main.tsx:10`)

### Proportional Font In Terminal Stack Distorts Character Grid on Windows and Linux Remote Clients
- **ID**: L5-UI-FRONTEND-10
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/lib/tauri.ts:3` - `export const DEFAULT_TERMINAL_FONT_STACK = 'MesloLGS NF, "Noto Sans KR", monospace';`
- **Why it breaks**: `DEFAULT_TERMINAL_FONT_STACK` places `"Noto Sans KR"` (a proportional sans-serif font) before `monospace`. When `MesloLGS NF` is absent on Windows or Linux, systems with `Noto Sans KR` installed fall back to a proportional typeface. In `RemoteTerminal.tsx`, cell widths are calculated from a single `1ch` element (`width: "1ch"`), which in proportional fonts measures only the advance of '0', causing characters of different widths to misalign with grid columns and cursor positions.
- **Fix**: In `ui/src/lib/tauri.ts`, insert standard platform monospaced fonts (such as `Consolas, "Courier New"`) before `"Noto Sans KR"` in `DEFAULT_TERMINAL_FONT_STACK`, ensuring the terminal stack always resolves to a fixed-pitch font on all platforms.
- **Status**: OPEN

### Workspace Selection Collides with Tab Selection on Windows and Linux
- **ID**: L5-UI-FRONTEND-3
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/lib/shortcuts.ts:445` - `  const expectedControl = Boolean(binding.control || (binding.mod && !isMac));`
- **Why it breaks**: On macOS, `tab.select1..9` uses `Ctrl+1..9` (`binding.control: true`) while `workspace.select1..9` uses `Cmd+1..9` (`binding.mod: true`). On Windows and Linux, `mod` resolves to `Ctrl`, mapping both actions to identical `Ctrl+1..9` chords. Because `tab.select1..9` appears earlier in the `SHORTCUTS` list, `workspace.select1..9` is shadowed and can never be triggered via keyboard on non-macOS platforms.
- **Fix**: In `ui/src/lib/shortcuts.ts`, update `workspace.select1..9` bindings on non-macOS platforms to use `alt: true` (e.g., `Alt+1..9`) so workspace switching chords do not collide with tab selection chords.
- **Status**: OPEN

### Terminal Input Intercepts Unshifted Ctrl+V Hijacking Vim Visual Block and Quoted Insert
- **ID**: L5-UI-FRONTEND-4
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/components/NativeTerminalPane.tsx:240` - `    ((event.ctrlKey || event.metaKey) &&`
- **Why it breaks**: `isPasteShortcut` treats `(event.ctrlKey || event.metaKey) && !event.altKey` with `KeyV` as a paste shortcut on all platforms regardless of `event.shiftKey`. On Linux and Windows terminals, `Ctrl+V` (ASCII 0x16 SYN) is an essential terminal control character used in vim for block visual mode and in readline for quoted literal character insertion, whereas terminal paste is conventionally `Ctrl+Shift+V`. Intercepting bare `Ctrl+V` unconditionally prevents vim block selection and quoted insertions from functioning.
- **Fix**: In `ui/src/components/NativeTerminalPane.tsx::isPasteShortcut`, require `event.shiftKey` when `event.ctrlKey` is active on non-macOS platforms (`!isMacShortcutPlatform()`), preserving bare `mod+V` paste only for `event.metaKey` on macOS.
- **Status**: OPEN

### Terminal Selection Copying Fails Due to Asynchronous Clipboard API Without User Activation
- **ID**: L5-UI-FRONTEND-5
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/components/NativeTerminalPane.tsx:972` - `        if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {`
- **Why it breaks**: On macOS, `cmd_native_terminal_copy_selection` writes selection text directly to `NSPasteboard` on the native thread. On Windows and Linux, the frontend attempts to copy by awaiting the asynchronous IPC response and then calling `navigator.clipboard.writeText(text)`, which fails with `NotAllowedError` because the transient user activation from the keydown or click event has expired across the IPC boundary.
- **Fix**: In `src-tauri`, make `cmd_native_terminal_copy_selection` write the selected text directly to the native OS clipboard on Windows (via Windows API / arboard) and Linux (via X11/Wayland clipboard), matching macOS behavior and removing `navigator.clipboard.writeText` from `copySelectionOrInterrupt` in `NativeTerminalPane.tsx`.
- **Status**: OPEN

### Ctrl+Click Intercepted for URL Opening and Pointer Cursor on Windows and Linux
- **ID**: L5-UI-FRONTEND-6
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/components/NativeTerminalPane.tsx:1223` - `      const held = isMac ? event.metaKey : event.ctrlKey;`
- **Why it breaks**: On macOS, holding `Cmd` (`event.metaKey`) toggles link navigation mode. On Windows and Linux, `held` evaluates `event.ctrlKey`, which turns the mouse cursor into a clicking hand (`cursor-pointer`) across the entire terminal pane whenever `Ctrl` is pressed for ordinary shell commands (Ctrl+C, Ctrl+R, etc.), and holding `Ctrl` while clicking swallows the pointer event (`cmdClickDownRef`), preventing terminal TUI applications (such as tmux or vim) from receiving Ctrl-click mouse sequences.
- **Fix**: In `ui/src/components/NativeTerminalPane.tsx`, avoid using bare `event.ctrlKey` as the URL click modifier on Windows/Linux; gate link clicking behind an explicit modifier or link hover hit-test rather than toggling `isCmdHeld` across the entire terminal container on any `ctrlKey` event.
- **Status**: OPEN

### Window Caption Controls Overlap TabBar on Windows with Overlay TitleBarStyle
- **ID**: L5-UI-FRONTEND-7
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `ui/src/components/TabBar.tsx:333` - `      className="relative flex h-tabbar shrink-0 items-stretch border-b border-border bg-card pr-1 select-none"`
- **Why it breaks**: In `src-tauri/tauri.windows.conf.json`, `titleBarStyle` is configured as `"Overlay"` with `hiddenTitle: true`, positioning the native Windows minimize/maximize/close caption controls at the top-right corner of the window. `TabBar.tsx` uses only `pr-1` at its trailing edge without reserving space for caption controls, causing the Windows system buttons to overlay and block interaction with the rightmost tabs and tab strip action buttons.
- **Fix**: In `ui/src/components/TabBar.tsx`, add a trailing spacer or right padding (approximately 138px) when running on Windows with `titleBarStyle: "Overlay"` to keep tab actions clear of the native caption buttons.
- **Status**: OPEN

### Discrete Mouse Wheel and Non-Pixel DeltaMode Divided by 20 Causes Sluggish Scrolling
- **ID**: L5-UI-FRONTEND-8
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/components/NativeTerminalPane.tsx:2067` - `        const rows = Math.trunc(event.deltaY / 20) || (event.deltaY > 0 ? 1 : -1);`
- **Why it breaks**: Both `NativeTerminalPane.tsx` and `RemoteTerminal.tsx` divide `event.deltaY` by 20, assuming macOS trackpad pixel scrolling (`deltaMode: DOM_DELTA_PIXEL` = 0). On Windows, Linux, and non-WebKit browsers like Firefox, discrete mouse wheel ticks dispatch `deltaMode: DOM_DELTA_LINE` (1) with values like 1, 2, or 3 lines. Dividing these values by 20 truncates to 0 (defaulting to 1), making wheel scrolling feel unresponsive and erratic on standard PC mice.
- **Fix**: In `ui/src/components/NativeTerminalPane.tsx` and `ui/src/remote/RemoteTerminal.tsx`, inspect `event.deltaMode`: if `event.deltaMode === WheelEvent.DOM_DELTA_LINE` (1), treat `event.deltaY` directly as line counts without dividing by 20; if `event.deltaMode === WheelEvent.DOM_DELTA_PAGE` (2), scale by the visible row count.
- **Status**: OPEN

### Shell Path Quoting Wraps Backslashes in POSIX Single Quotes Breaking Windows Command Interpreters
- **ID**: L5-UI-FRONTEND-9
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `ui/src/components/NativeTerminalPane.tsx:370` - `function quoteShellPath(path: string): string {`
- **Why it breaks**: `quoteShellPath` checks `/[\s'"\\$`!*?[\]();&|<>]/.test(path)` and wraps matches in POSIX single quotes (`'...'`). Because every Windows path contains backslashes (e.g., `C:\Users\test`), all dropped Windows paths are formatted as POSIX single-quoted strings. In Windows `cmd.exe`, single quotes are not valid path delimiters (causing command not found or invalid syntax errors), and PowerShell requires the `&` call operator to execute single-quoted paths.
- **Fix**: In `ui/src/components/NativeTerminalPane.tsx::quoteShellPath`, branch on platform: on Windows, wrap paths containing spaces or special characters in double quotes (`"..."`) with Windows shell escaping instead of POSIX single quotes.
- **Status**: OPEN

### Bare Ctrl+V Intercepted Without PTY Forwarding in Remote Web Terminal
- **ID**: L5-UI-FRONTEND-11
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/remote/RemoteTerminal.tsx:681` - `            if (ctrlChordChar === "v") return;`
- **Why it breaks**: In `RemoteTerminal.tsx::onKeyDown`, any Ctrl chord with 'v' returns immediately to defer to the browser paste event. On Windows and Linux, this intercepts unshifted `Ctrl+V` alongside `Ctrl+Shift+V`, preventing the terminal from forwarding the `ctrl-v` control code (ASCII 0x16 SYN) needed for vim visual block mode or readline quoted insert in remote sessions.
- **Fix**: In `ui/src/remote/RemoteTerminal.tsx`, require `event.shiftKey` when checking for paste chord exemption on non-macOS clients (`if (ctrlChordChar === "v" && event.shiftKey) return;`), allowing unshifted `Ctrl+V` to fall through to `sendKey("ctrl-v")`.
- **Status**: OPEN

### TabBar Leading Spacer Renders Empty Dead Space on Windows and Linux
- **ID**: L5-UI-FRONTEND-12
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/App.tsx:2047` - `            leadingSpacer={isSidebarOpen ? 0 : isMacShortcutPlatform() ? 108 : 36}`
- **Why it breaks**: When the sidebar is collapsed, `App.tsx` provides a `108px` spacer on macOS to clear native window traffic light controls. On non-macOS platforms, it passes `36px`, rendering an empty 36-pixel box with a right border (`border-r border-border`) at the left of the `TabBar`. On Linux (native titlebar) and Windows (caption buttons at top-right), there are no controls at the top-left, making this 36px spacer an unnecessary visual artifact.
- **Fix**: In `ui/src/App.tsx`, set `leadingSpacer` to `0` when `!isMacShortcutPlatform()`, passing non-zero values only for macOS traffic light clearance (`isSidebarOpen || !isMacShortcutPlatform() ? 0 : 108`).
- **Status**: OPEN

### TabBar Pointerdown PreventDefault Cancels Native Double-Click Window Maximize
- **ID**: L5-UI-FRONTEND-13
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/components/TabBar.tsx:316` - `    event.preventDefault();`
- **Why it breaks**: In `TabBar.tsx::startWindowDrag`, calling `event.preventDefault()` on `pointerdown` cancels the default mouse event cycle before invoking `getCurrentWindow().startDragging()`. On Windows and Linux desktops, double-clicking the empty tab bar / title bar area is the standard gesture to maximize or restore the window; suppressing the default pointer behavior aborts the second click registration and breaks double-click maximize.
- **Fix**: In `ui/src/components/TabBar.tsx`, remove `event.preventDefault()` from `startWindowDrag`, or use Tauri's native `data-tauri-drag-region` attribute on the tab bar container to let the OS handle drag and double-click gestures naturally.
- **Status**: OPEN

### Deprecated navigator.platform Sniffing Misidentifies iPads and Touch Devices as Desktop Mac
- **ID**: L5-UI-FRONTEND-14
- **Severity**: MEDIUM
- **Platforms affected**: macOS
- **Evidence**: `ui/src/lib/shortcuts.ts:514` - `  if (/Mac|iPhone|iPad|iPod/.test(navigator.platform)) return true;`
- **Why it breaks**: `navigator.platform` is deprecated by current web standards and frozen in modern browsers. Furthermore, iPadOS desktop Safari sends `MacIntel` as `navigator.platform` and includes `Macintosh` in `navigator.userAgent`, causing `detectMacPlatform()` to classify iPads in remote web client sessions as macOS desktops, displaying Mac Command glyphs (`⌘`) and enabling desktop-only keyboard paths on touch devices.
- **Fix**: In `ui/src/lib/shortcuts.ts::detectMacPlatform`, check `navigator.userAgentData?.platform` where available and check `navigator.maxTouchPoints > 0` to avoid misclassifying touch iPads as desktop Mac clients in the web interface.
- **Status**: OPEN

### Universal Context Menu Guard Suppresses Windows Terminal Paste and System Titlebar Menu
- **ID**: L5-UI-FRONTEND-15
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `ui/src/lib/contextMenuGuard.ts:14` - `    event.preventDefault();`
- **Why it breaks**: `installContextMenuGuard` calls `event.preventDefault()` on all contextmenu events that do not originate from an HTML input or textarea. On Windows, right-clicking terminal panes is the standard convention for pasting text or opening terminal actions, and right-clicking custom titlebars opens the Windows system menu (Restore, Move, Size, Minimize, Maximize, Close). Suppressing all right-clicks without providing custom menus breaks expected Windows desktop interactions.
- **Fix**: In `ui/src/lib/contextMenuGuard.ts`, allow right-click events on `.terminal-host` elements to trigger paste or a terminal context menu, and exempt window drag regions on Windows to allow the OS titlebar system menu to appear.
- **Status**: OPEN

### macOS Option as Alt Setting Toggle Rendered Unconditionally on Non-Mac Platforms
- **ID**: L5-UI-FRONTEND-16
- **Severity**: LOW
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/components/settings/TerminalSection.tsx:224` - `              macOS Option as Alt`
- **Why it breaks**: `TerminalSection.tsx` displays the "macOS Option as Alt" switch unconditionally across all platforms. Keyboards on Windows and Linux do not have an Option key (they use Alt and AltGr), making this toggle confusing and irrelevant on non-macOS operating systems.
- **Fix**: In `ui/src/components/settings/TerminalSection.tsx`, wrap the "macOS Option as Alt" setting block in a platform check (`isMacShortcutPlatform()`) so it only renders on macOS.
- **Status**: OPEN

### Proprietary -webkit-app-region CSS Drag Properties Ignored by WebKitGTK on Linux
- **ID**: L5-UI-FRONTEND-17
- **Severity**: LOW
- **Platforms affected**: Linux
- **Evidence**: `ui/src/index.css:145` - `    -webkit-app-region: drag;`
- **Why it breaks**: The `.drag-region` and `.no-drag` utility classes in `ui/src/index.css` specify `-webkit-app-region: drag` and `-webkit-app-region: no-drag`. These CSS properties are Chromium-specific extensions and are not supported by WebKitGTK on Linux, leaving CSS-based drag regions ineffective on Linux unless backed by Tauri drag attributes or event handlers.
- **Fix**: In `ui/src/index.css`, document the limitation and ensure all draggable elements consistently use Tauri's cross-platform `data-tauri-drag-region` attribute rather than relying on `-webkit-app-region`.
- **Status**: OPEN

### Full Disk Access macOS Permission Alert Rendered Unconditionally on Windows and Linux
- **ID**: L5-UI-FRONTEND-18
- **Severity**: LOW
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/components/settings/PermissionsSection.tsx:183` - `        <Alert className="border-amber-500/30 bg-amber-500/10 text-amber-300">`
- **Why it breaks**: In `PermissionsSection.tsx`, if permissions are not fully granted, an alert card states: "Granting Full Disk Access stops macOS from showing alerts such as 'Ferryx would like to access your Photo Library'". On Windows and Linux, Full Disk Access does not exist, so displaying macOS-specific security guidance is irrelevant and misleading.
- **Fix**: In `ui/src/components/settings/PermissionsSection.tsx`, gate the Full Disk Access alert with `isMac` or `status?.platform === "macos"`, rendering platform-appropriate guidance or omitting the card on non-macOS platforms.
- **Status**: OPEN

---

## F. 빌드, 패키징, 업데이터, 서비스 설치

_20 findings — BLOCKER 4, HIGH 11, MEDIUM 5, LOW 0_

### Windows Window Chrome Missing Minimize, Maximize, and Close Buttons
- **ID**: L6-PACKAGING-1
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/tauri.windows.conf.json:14` - `        "titleBarStyle": "Overlay",`
- **Why it breaks**: The Windows configuration override retains `titleBarStyle: "Overlay"` and `hiddenTitle: true` from the base macOS configuration, suppressing native Windows caption controls (minimize, maximize, close). Because Ferryx's web frontend does not implement custom Windows caption controls, Windows users cannot minimize, maximize, or close the application window via chrome controls.
- **Fix**: In `src-tauri/tauri.windows.conf.json`, update `app.windows[0]` to set `"titleBarStyle": "Visible"` and `"hiddenTitle": false` to restore native Windows caption buttons matching `tauri.linux.conf.json`.
- **Status**: OPEN

### CLI Launcher Installation Symlink Logic Excludes Windows
- **ID**: L6-PACKAGING-2
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/cli_install.rs:205` - `    #[cfg(unix)]`
- **Why it breaks**: CLI launcher installation is gated behind `#[cfg(unix)]` and `resolve_launcher_status` returns `is_supported: false` on non-Unix platforms. On Windows, standard unprivileged symlinks require Developer Mode, and `install_launcher` creates no launcher file, leaving Windows users completely unable to install or invoke the `ferryx` CLI tool.
- **Fix**: In `src-tauri/src/ipc/cli_install.rs`, add a `#[cfg(windows)]` implementation to `install_launcher` that writes a batch wrapper shim `ferryx.cmd` (`@"%~dp0...\ferryx.exe" %*`) into `%LOCALAPPDATA%\Microsoft\WindowsApps` or a PATH directory, and update `resolve_launcher_status` to report `is_supported = true` on Windows.
- **Status**: OPEN

### Daemon Autostart Persistence Missing Implementation for Windows and Linux
- **ID**: L6-PACKAGING-3
- **Severity**: BLOCKER
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/daemon/launchd.rs:17` - `pub fn get_launchd_plist_path() -> Option<PathBuf> {`
- **Why it breaks**: Background daemon autostart is implemented exclusively for macOS `launchd`, with `get_launchd_plist_path()` returning `None` on all non-macOS platforms. Calling `install_launchd_agent()` on Windows or Linux immediately aborts with `"Cannot determine HOME directory"` and configures no persistent startup mechanism.
- **Fix**: Implement platform-native autostart providers: on Windows, configure a Scheduled Task via `schtasks` or write to `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`; on Linux, generate and enable a `systemd --user` unit (`~/.config/systemd/user/ferryx.service`).
- **Status**: OPEN

### Shipped Debian and MSI Packages Produce No Updater Manifest Artifacts
- **ID**: L6-PACKAGING-4
- **Severity**: BLOCKER
- **Platforms affected**: Windows+Linux
- **Evidence**: `scripts/build-latest-json.mjs:5` - `const UPDATER_ARTIFACT = /(\.app\.tar\.gz|\.nsis\.zip|-setup\.exe|\.AppImage(?:\.tar\.gz)?)$/;`
- **Why it breaks**: `build-latest-json.mjs` only accepts `.app.tar.gz`, `.nsis.zip`, `-setup.exe`, and `.AppImage`, omitting `.deb` and `.msi`. The project distributes `Ferryx_amd64.deb` in GitHub releases and targets `msi` in `tauri.conf.json:54`, but users installing via these packages receive no updater payload in `latest.json`, permanently disabling in-app updates.
- **Fix**: Either remove `msi` and `deb` from `tauri.conf.json` bundle targets and distribute only `nsis` and `AppImage`, or disable in-app updater polling when running inside a package-managed (`.deb` or `.msi`) installation.
- **Status**: OPEN

### MSIX Packaging Script Omits Frontend Resources Required for Embedded Remote Server
- **ID**: L6-PACKAGING-10
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `scripts/build-msix.ps1:124` - `Copy-Item $exePath -Destination "$layoutDir\ferryx.exe" -Force`
- **Why it breaks**: `build-msix.ps1` stages only `ferryx.exe` and icon images into the MSIX layout directory, omitting the `ui/dist` bundle resources configured in `src-tauri/tauri.conf.json:48`. When running from an MSIX installation, the embedded remote server (`src-tauri/src/remote/server.rs:1388`) cannot locate frontend static files and fails to serve the remote web interface.
- **Fix**: In `scripts/build-msix.ps1`, add a step copying `ui/dist` to `$layoutDir\ui\dist` (or `$layoutDir\resources\ui\dist`) matching the directory layout probed by `resolve_frontend_dist`.
- **Status**: OPEN

### Packaged Linux Resource Path Resolution Misses Standard Distribution Hierarchy
- **ID**: L6-PACKAGING-11
- **Severity**: HIGH
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/remote/server.rs:1388` - `            candidates.push(exe_dir.join("ui/dist"));`
- **Why it breaks**: `resolve_frontend_dist` only probes `exe_dir.join("ui/dist")` and `exe_dir.join("resources/ui/dist")`. On packaged Linux systems (such as `.deb` installs where the binary is installed to `/usr/bin/ferryx`), resources are placed in `/usr/lib/ferryx/resources` or `/usr/share/ferryx/resources`, so the hardcoded paths fail to locate the frontend bundle.
- **Fix**: In `src-tauri/src/remote/server.rs`, use Tauri's `app_handle.path().resource_dir()` to resolve bundle resources dynamically, or add standard Linux candidate paths `/usr/lib/ferryx/resources/ui/dist` and `/usr/share/ferryx/resources/ui/dist`.
- **Status**: OPEN

### Release CI Workflow Omits Authenticode Code-Signing for Windows Artifacts
- **ID**: L6-PACKAGING-12
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `.github/workflows/release.yml:144` - `          bunx @tauri-apps/cli build ${{ matrix.tauri_args }}`
- **Why it breaks**: The release workflow imports macOS code-signing identities and notarization credentials (`release.yml:105-132`), but provides no Authenticode certificate or signing step for Windows. The resulting Windows NSIS installer `.exe` is shipped completely unsigned, causing Windows Defender SmartScreen to flag the installer as untrusted.
- **Fix**: In `.github/workflows/release.yml`, add an Authenticode signing step via SignTool or Azure Trusted Signing using secrets (`WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD`) during or after `tauri build`.
- **Status**: OPEN

### Vendored Ghostty Build Script Hardcodes Nightly Zig 0.16.0 Toolchain Requirement
- **ID**: L6-PACKAGING-13
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/native_terminal/build_ghostty.rs:7` - `pub const REQUIRED_ZIG_VERSION: &str = "0.16.0";`
- **Why it breaks**: `build_ghostty.rs` enforces that the installed Zig compiler version must start with `0.16.0`, which is an unreleased nightly build. Standard package managers on Windows (`winget`, `choco`) and Linux (`apt`, `pacman`) only provide stable Zig versions (0.13.x or 0.14.x), causing local development and CI builds on clean environments to fail with an incompatible Zig toolchain error.
- **Fix**: In `src-tauri/native_terminal/build_ghostty.rs`, adapt `verify_zig` to accept stable supported Zig versions or provide clear instructions and automated toolchain fallback for developer environments.
- **Status**: OPEN

### CI Test Suite Gated to Linux Only Leaving Windows Untested in Pull Requests
- **ID**: L6-PACKAGING-14
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `.github/workflows/build-test.yml:127` - `        if: matrix.os_name == 'linux'`
- **Why it breaks**: Test-only defect: In `.github/workflows/build-test.yml`, the `Cargo Test` step is restricted to `matrix.os_name == 'linux'`, while Windows runners only run `cargo check` and `cargo build`. As a result, Windows-specific unit and integration tests are never executed in CI, allowing Windows-breaking packaging and IPC regressions to merge undetected.
- **Fix**: In `.github/workflows/build-test.yml`, remove the `if: matrix.os_name == 'linux'` restriction or add an explicit `if: matrix.os_name == 'windows'` test step executing `cargo test --manifest-path src-tauri/Cargo.toml`.
- **Status**: OPEN

### Dev Frontend Runner in tauri.conf.json Relies on Shell-Dependent Bun Execution
- **ID**: L6-PACKAGING-15
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/tauri.conf.json:7` - `    "beforeDevCommand": "bun scripts/dev-frontend.mjs",`
- **Why it breaks**: `beforeDevCommand` directly invokes `"bun scripts/dev-frontend.mjs"`, which assumes `bun` is available on the executable search path without `.cmd` or `.exe` resolution and assumes `scripts/` is relative to CWD. When invoked from `src-tauri/` or on standard Windows command shells without Bun registered in PATHEXT, `tauri dev` fails immediately on startup.
- **Fix**: In `src-tauri/tauri.conf.json`, use `node` or a cross-platform npm/pnpm command for `beforeDevCommand`, or wrap the invocation with `bun.cmd` / shell-aware executable resolution.
- **Status**: OPEN

### CLI Launcher Home Directory Lookup Relies on POSIX HOME Environment Variable
- **ID**: L6-PACKAGING-5
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/cli_install.rs:56` - `    let home = std::env::var_os("HOME");`
- **Why it breaks**: `get_default_launcher_path` retrieves the user home directory by querying `HOME`, which is standard on Unix but typically unset on native Windows environments where `USERPROFILE` or `LOCALAPPDATA` is used. On Windows, resolving launcher status immediately returns `CliInstallError::HomeDirNotFound`.
- **Fix**: In `src-tauri/src/ipc/cli_install.rs`, update `get_default_launcher_path` to query `std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))` or use `tauri::path::PathResolver::home_dir()`.
- **Status**: OPEN

### Updater Architecture Mapping Drops ARM64 for Windows and Linux
- **ID**: L6-PACKAGING-6
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `scripts/build-latest-json.mjs:29` - `    return ["windows-x86_64"];`
- **Why it breaks**: `targetsFor` in `scripts/build-latest-json.mjs` inspects filename architecture tokens for macOS (`darwin-aarch64` vs `darwin-x86_64`), but hardcodes static returns of `windows-x86_64` and `linux-x86_64` for all Windows and Linux artifacts. Any ARM64 Windows (`windows-aarch64`) or ARM64 Linux (`linux-aarch64`) updater packages are misclassified as x86_64 or omitted.
- **Fix**: In `scripts/build-latest-json.mjs`, update `targetsFor` to inspect architecture substrings (`aarch64|arm64`) for Windows and Linux artifacts and map them to `windows-aarch64` and `linux-aarch64`.
- **Status**: OPEN

### Debian Package Depends Omits ALSA Library Required by Sound Runtime
- **ID**: L6-PACKAGING-7
- **Severity**: HIGH
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/tauri.conf.json:92` - `          "libwebkit2gtk-4.1-0 | libwebkit2gtk-4.0-37",`
- **Why it breaks**: The Debian package configuration lists WebKitGTK, GTK3, and AppIndicator, but omits ALSA (`libasound2` or `libasound2t64`). Ferryx depends on `rodio` (`src-tauri/Cargo.toml:81`) for notification audio playback (`src-tauri/src/ipc/notifications.rs`), which dynamically binds to ALSA on Linux; omitting it causes audio initialization crashes on minimal installations.
- **Fix**: Add `"libasound2 | libasound2t64"` to the `bundle.linux.deb.depends` array in `src-tauri/tauri.conf.json`.
- **Status**: OPEN

### WiX MSI Target Included in Base Config Breaks Windows Builds Lacking WiX Toolset
- **ID**: L6-PACKAGING-8
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/tauri.conf.json:54` - `        "msi",`
- **Why it breaks**: The base configuration specifies `"msi"` under `bundle.targets`, which causes `tauri build` on Windows to attempt invoking WiX Toolset compilers (`candle.exe`, `light.exe`). While CI bypasses this with `--bundles nsis` (`release.yml:37`), developer and release builds executed locally on Windows fail unless WiX Toolset is manually installed.
- **Fix**: In `src-tauri/tauri.conf.json`, remove `"msi"` from `bundle.targets` and retain only `"nsis"` for Windows desktop packaging.
- **Status**: OPEN

### MSIX Packaging Signs with Self-Signed Untrusted Certificate Blocking Sideloading
- **ID**: L6-PACKAGING-9
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `scripts/build-msix.ps1:184` - `$cert = New-SelfSignedCertificate -Type Custom`
- **Why it breaks**: The PowerShell MSIX packaging script generates a temporary self-signed certificate with a hardcoded password to sign the MSIX artifact. Windows App Installer rejects packages signed by untrusted certificates (`0x800B0109`), making the produced MSIX artifact impossible to sideload without manually trusting the root certificate.
- **Fix**: In `scripts/build-msix.ps1`, add parameters to accept a valid Authenticode code-signing PFX certificate and disable self-signed generation by default for production releases.
- **Status**: OPEN

### macOS Private API Enabled Globally in Shared Base Config and Cargo Features
- **ID**: L6-PACKAGING-16
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/tauri.conf.json:13` - `    "macOSPrivateApi": true,`
- **Why it breaks**: The base configuration sets `macOSPrivateApi: true` unconditionally for all targets, and `src-tauri/Cargo.toml` activates the `macos-private-api` crate feature without platform gating. On non-macOS platforms (Windows and Linux), private macOS APIs do not exist, and declaring macOS-specific permissions in the shared base configuration violates platform separation and causes rejection if submitting to platform app stores.
- **Fix**: Remove `"macOSPrivateApi": true` from `src-tauri/tauri.conf.json` and place it in `src-tauri/tauri.macos.conf.json`, and gate `features = ["macos-private-api"]` in `src-tauri/Cargo.toml` under `target.'cfg(target_os = "macos")'.dependencies.tauri`.
- **Status**: OPEN

### Daemon Persistence Hardcodes Legacy Identity and World-Writable POSIX Log Paths
- **ID**: L6-PACKAGING-17
- **Severity**: MEDIUM
- **Platforms affected**: macOS
- **Evidence**: `src-tauri/src/daemon/launchd.rs:5` - `const PLIST_LABEL: &str = "com.rorca.daemon";`
- **Why it breaks**: The launchd daemon service configuration still uses the legacy identifier `"com.rorca.daemon"` and hardcodes standard log streams to `/tmp/rorca-daemon.log` (line 39). This conflicts with the current product identity (`com.ferryx.app`) and uses world-writable `/tmp` paths vulnerable to symlink clobbering and collision.
- **Fix**: In `src-tauri/src/daemon/launchd.rs`, change `PLIST_LABEL` to `"com.ferryx.daemon"` and relocate `StandardOutPath` and `StandardErrorPath` to the user-isolated log directory `~/Library/Logs/Ferryx/`.
- **Status**: OPEN

### MSIX Manifest Identity Conflicts with Official App Identifier and Publisher
- **ID**: L6-PACKAGING-18
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/windows/msix/AppxManifest.xml:9` - `    Name="ProjectMaho.Ferryx"`
- **Why it breaks**: The MSIX manifest template hardcodes `Name="ProjectMaho.Ferryx"` and `PublisherDisplayName="Project Maho"` (line 16), which conflicts with the canonical `identifier` `"com.ferryx.app"` (line 5) and `publisher` `"Ferryx"` (line 58) declared in `tauri.conf.json`. This causes mismatched app identity, inconsistent app data folder naming, and store ingestion conflicts.
- **Fix**: In `src-tauri/windows/msix/AppxManifest.xml`, update `Identity@Name` to `"com.ferryx.app"` and `PublisherDisplayName` to `"Ferryx"`.
- **Status**: OPEN

### Updater Archive Layout Validator Strictly Requires macOS App Bundle Structure
- **ID**: L6-PACKAGING-19
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `scripts/assert-updater-archive-layout.mjs:62` - `  const invalidRoot = payloadEntries.find(({ path }) => !path.startsWith("Ferryx.app/"));`
- **Why it breaks**: The updater archive verification script hard-asserts that all payload entries start with `Ferryx.app/`, which is only valid for macOS `.app.tar.gz` bundles. Windows (`.nsis.zip`) and Linux (`.AppImage.tar.gz`) updater archives cannot be validated by this script, leaving non-macOS updater archive layouts unverified in CI release pipelines.
- **Fix**: In `scripts/assert-updater-archive-layout.mjs`, branch validation based on archive extension or target platform to verify `.nsis.zip` (root executable present) and `.AppImage.tar.gz` layouts appropriately.
- **Status**: OPEN

### Windows Linker Manifest Flags in Build Script Fail on GNU MinGW Toolchain
- **ID**: L6-PACKAGING-20
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/build.rs:30` - `    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {`
- **Why it breaks**: `src-tauri/build.rs` passes MSVC-specific linker arguments (`/MANIFEST:EMBED` and `/MANIFESTINPUT:...`) whenever `CARGO_CFG_TARGET_OS` is `"windows"`. When compiling with the GNU MinGW toolchain (`x86_64-pc-windows-gnu`), GNU `ld` does not support `/MANIFEST` flags, causing the link step to abort with unrecognized option errors.
- **Fix**: In `src-tauri/build.rs`, gate the MSVC linker arguments with `if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")`.
- **Status**: OPEN

---

## G. 데몬, IPC 전송, 원격 게이트웨이

_11 findings — BLOCKER 0, HIGH 1, MEDIUM 4, LOW 6_

### Windows daemon transport has no authentication token
- **ID**: L7-DAEMON-IPC-1
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/server.rs:1102` — `let listener = TcpListener::bind("127.0.0.1:0")`
- **Why it breaks**: On Unix the daemon socket is protected by filesystem trust: a 0700 runtime directory, a 0600 socket file, ownership checks (`validate_runtime_socket_path_for_uid`), and `flock`-based single-instance locking. On Windows the equivalent transport is a loopback TCP port with **no protocol-level auth/token/handshake secret** — any other process running as the same Windows user (or, if `%TEMP%`/`LOCALAPPDATA` ACLs are ever misconfigured or the port number leaks via `daemon.port`, any local process that can read that file) can connect and issue arbitrary `DaemonRequest`s including `Write`, `Spawn`, and `Shutdown`. The only gate is `validate_runtime_socket_path_for_uid` for `not(unix)` (`server.rs:456-463`), which does not check ownership at all (`_expected_uid` is unused) and only verifies the path is not a symlink and not a directory.
- **Fix**: Add a per-boot random shared-secret token written alongside `daemon.port` (e.g. `daemon.token`, 0600-equivalent via `LOCALAPPDATA` ACL) and require it in the `Handshake` request on the `#[cfg(not(unix))]` path in `handle_client` (`server.rs`), rejecting connections that omit or mismatch it before processing any other `DaemonRequest` variant.
- **Status**: OPEN

### Windows runtime-path validation performs no ownership/ACL check
- **ID**: L7-DAEMON-IPC-2
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/server.rs:462` — `validate_safe_ownership_and_type_for_uid(runtime_dir, RuntimeNodeKind::Directory, 0)?;`
- **Why it breaks**: `validate_runtime_socket_path_for_uid` for `not(unix)` (`server.rs:456-465`) hard-codes UID `0` and the underlying `validate_safe_ownership_and_type_for_uid` `not(unix)` branch (`server.rs:371-398`, confirmed by reading the function body) never compares an owner at all — it only rejects symlinks and wrong node types. The Unix path performs a real UID-equality check (`meta.uid() != expected_uid`) before ever trusting the socket. On Windows there is no equivalent check against the Windows SID/owner of the runtime directory or port file, so the "port file trust" story relies entirely on default NTFS ACLs of `LOCALAPPDATA`/`%TEMP%` never having been loosened (e.g. by a misconfigured multi-user machine or roaming profile share).
- **Fix**: On Windows, use `GetNamedSecurityInfoW`/`GetFileSecurityW` (via `windows-sys`, already a dependency for `LockFileEx`) to read the file/directory owner SID and compare it against the current process token's SID in `validate_safe_ownership_and_type_for_uid`'s `not(unix)` branch, mirroring the Unix UID check instead of accepting any owner.
- **Status**: OPEN

### Agent-state extension socket is never bound on Windows
- **ID**: L7-DAEMON-IPC-3
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/server.rs:1001` — `#[cfg(unix)]` (guarding `fn spawn_agent_state_listener`)
- **Why it breaks**: `spawn_agent_state_listener` (a `UnixListener` bound at `agent-state.sock`) is only compiled and only invoked under `#[cfg(unix)]` (call site `server.rs:1130` right after `#[cfg(unix)]` at `server.rs:1129`). There is no `#[cfg(not(unix))]` TCP/named-pipe fallback. Meanwhile every spawned terminal on every platform unconditionally exports `FERRYX_AGENT_STATE_SOCKET=<path>` pointing at that same never-created path (`src-tauri/src/terminal/pty.rs:136-137`, consumed by `crate::daemon::agent_state_socket_path()`). On Windows, agent CLIs (Claude, Codex, etc.) that try to report `working`/`blocked`/`idle` state over that socket will always fail to connect, so the daemon's `AgentStateReport` stream (used by `parse_agent_state_report`) never receives real agent-state updates — the UI silently falls back to screen-scraping inference for every agent on Windows.
- **Fix**: Add a `#[cfg(not(unix))]` variant of `spawn_agent_state_listener` that binds a Windows named pipe (`\\.\pipe\ferryx-agent-state-<uid-or-session>`) or a second loopback `TcpListener` with the same line-delimited `AgentStateReport` JSON protocol, and change `agent_state_socket_path()` (`server.rs:802`) to return that pipe/port descriptor instead of a `.sock`-style path on Windows.
- **Status**: OPEN

### Agent-provider process discovery hard-codes `/bin/ps`, unconditionally no-ops on Windows
- **ID**: L7-DAEMON-IPC-5
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/agents.rs:72` — `let output = crate::util::no_window_command("/bin/ps")`
- **Why it breaks**: `process_table_entries()` shells out to the absolute Unix path `/bin/ps` with BSD/GNU-style flags (`-axwwo pid=,ppid=,args=`). This function backs `discover_agent_session_id`, which is called directly by the daemon's `DaemonRequest::DiscoverAgentSession` handler (`src-tauri/src/daemon/server.rs`, `Ok(DaemonRequest::DiscoverAgentSession { .. })` arm) to find a descendant agent CLI PID for provider-session resume. On Windows, `/bin/ps` does not exist, so the spawn fails and `.ok()?` turns it into `None` — `discover_agent_session_id` degrades to `None` for every agent type on every call, meaning provider-session resume (e.g. reattaching to a Claude/Codex conversation ID after a daemon restart) is unconditionally disabled on Windows with no diagnostic surfaced to the user.
- **Fix**: Add a `#[cfg(windows)]` implementation of `process_table_entries()` (or an OS-specific trait) using the `windows-sys` `CreateToolhelp32Snapshot`/`Process32NextW` APIs (already partially available as a dependency) to enumerate `(pid, ppid, args)` tuples, or shell out to `wmic process get ProcessId,ParentProcessId,CommandLine` / PowerShell `Get-CimInstance Win32_Process` as an interim fix, gated behind `#[cfg(target_os = "windows")]` next to the existing `#[cfg(target_os = "linux")]`/`#[cfg(target_os = "macos")]` split already used in the same file's `process_cwd`.
- **Status**: OPEN

### Handover (rolling daemon upgrade without dropping sessions) is entirely unsupported on Windows
- **ID**: L7-DAEMON-IPC-7
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/handover.rs:131` — `Err("Handover unsupported on Windows".to_string())`
- **Why it breaks**: `prepare_handover` is `#[cfg(unix)]` for the real implementation and has a `#[cfg(not(unix))]` stub that always errors (`handover.rs:126-131`); `server.rs`'s `PrepareHandover` request arm additionally returns `DaemonResponse::HandoverRejected` on `#[cfg(not(unix))]` (`server.rs:1613-1617`), and `handle_upgrade_binary` on `#[cfg(not(unix))]` always returns `DaemonResponse::UpgradeUnsupported` (`server.rs:1766-1772`) without ever attempting a session-preserving restart. This is a graceful degradation, not a crash, but it means on Windows every daemon binary upgrade (auto-update) either leaves the old daemon binary running until the user fully quits the app, or (if forced) drops every live PTY session — the rolling-handover UX that Unix users get is a hard feature gap on Windows.
- **Fix**: Implement a Windows-native handover using a second named pipe (or ephemeral TCP listener, matching L7-DAEMON-IPC-3's fix) for the legacy peer, since `windows-sys`'s `LockFileEx`/`UnlockFileEx` already provide the byte-range lock primitive needed to hand off `DaemonLockFiles`; wire `HandoverManager::prepare_handover`'s `#[cfg(not(unix))]` arm to bind that listener and return it the same way the Unix arm returns a `UnixListener`, instead of an unconditional `Err`.
- **Status**: OPEN

### `get_persistent_lock_path` Windows branch has no `LOCALAPPDATA`-absent test coverage matching the Unix branch
- **ID**: L7-DAEMON-IPC-10
- **Severity**: LOW
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/server.rs:246` — `#[cfg(windows)]`
- **Why it breaks**: `get_persistent_lock_path` (`server.rs:242-256`) correctly falls back `APPDATA` → `USERPROFILE`/.ferryx on Windows, which is good, but if *both* env vars are absent (e.g. a locked-down service account or sandboxed test runner) the function returns `None` and `acquire_daemon_locks` in `run_server_with_handover_and_readiness` (`server.rs:1087`) is called with `get_persistent_lock_path().as_deref()`, i.e. `None`, silently skipping the persistent (survives-reinstall) lock and relying only on the per-runtime-dir legacy lock. This is the same graceful-degradation behavior as Unix's `HOME`-absent case, so it is not a differential bug, but there is no test in `server.rs`'s `#[cfg(test)]` module exercising the Windows env-var-absent path the way Unix paths are exercised, so a future regression that panics instead of degrading gracefully on Windows would not be caught by CI (which only runs on macOS per the audit's stated baseline).
- **Fix**: Add a unit test under a `#[cfg(windows)]`-gated (or env-var-injectable, matching the existing `resolve_session_path` test pattern at `server.rs:64-113`) test module that clears `APPDATA`/`USERPROFILE`/`FERRYX_DATA_DIR` and asserts `get_persistent_lock_path` returns `None` without panicking, and that `acquire_daemon_locks(None, &legacy_path)` still succeeds using only the legacy lock.
- **Status**: OPEN

### Windows TCP daemon transport has no localhost-only IPv6/dual-stack consideration
- **ID**: L7-DAEMON-IPC-11
- **Severity**: LOW
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/client.rs:379` — `DaemonStream::connect(format!("127.0.0.1:{port}")).await`
- **Why it breaks**: Both the server bind (`server.rs:1102`, `"127.0.0.1:0"`) and the client connect (`client.rs:379`) hard-code IPv4 loopback. On a Windows machine where IPv4 loopback is disabled or filtered by endpoint security software (uncommon but seen in locked-down enterprise images that only permit `::1`), the daemon would fail to bind or the client would fail to connect with no automatic IPv6 retry, whereas the Unix Domain Socket path has no such address-family dependency at all.
- **Fix**: If Windows enterprise-image compatibility matters, add an IPv6 loopback (`[::1]:0`) fallback in both `server.rs`'s `#[cfg(not(unix))]` bind and `client.rs`'s `#[cfg(not(unix))]` `connect_socket`, storing the resolved `SocketAddr` (not just a bare port number) in `daemon.port` so the client does not have to guess the address family.
- **Status**: OPEN

### Agent-state extension installer only checks `HOME`, never installs on Windows
- **ID**: L7-DAEMON-IPC-4
- **Severity**: LOW
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/agent_extension.rs:25` — `std::env::var_os("HOME").map(PathBuf::from)`
- **Why it breaks**: The body of `home_dir()` reads only `std::env::var_os("HOME")` with no Windows fallback. `HOME` is typically unset for native Windows processes (the per-user equivalent is `USERPROFILE`/`%APPDATA%`). `extension_dirs()` calls `home_dir()` and returns an empty `Vec` when it is `None`, so `install_agent_state_extension()` (invoked unconditionally at `server.rs:1132` right after the `#[cfg(unix)]`-gated agent-state listener) silently installs nothing on Windows — compounding L7-DAEMON-IPC-3, since even if the transport were fixed, the extension file that talks to it would never be deployed into `~/.omo`, `~/.pi`, `~/.omp` equivalents.
- **Fix**: In `home_dir()`, add `.or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))` (matching the pattern already used correctly in `src-tauri/src/remote/state.rs`'s `DATA_DIR_SOURCES` for Windows) so `extension_dirs()` resolves a real per-user directory on Windows.
- **Status**: OPEN

### `process_cwd` returns `None` unconditionally on Windows
- **ID**: L7-DAEMON-IPC-6
- **Severity**: LOW
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/terminal.rs:977` — `#[cfg(not(any(target_os = "linux", target_os = "macos")))]`
- **Why it breaks**: `process_cwd(pid)` has real implementations for Linux (`/proc/{pid}/cwd`) and macOS (`proc_pidinfo`/`lsof` fallback), and an explicit `None` stub for every other target, including Windows. This feeds `cmd_terminal_get_cwd`'s cache-miss path and the agent-provider `cwd`-keyed session lookups in `src-tauri/src/ipc/agents.rs` (`antigravity_session_id`, `opencode_session_id`, both call `crate::ipc::terminal::process_cwd`). Those lookups always fail on Windows, so any agent-resume path that keys off the shell's live working directory (rather than the daemon's own tracked `cwd`) cannot resolve on Windows.
- **Fix**: Add a `#[cfg(target_os = "windows")]` branch to `process_cwd` using `NtQuerySystemInformation`/`QueryFullProcessImageName`-adjacent APIs is not sufficient for CWD; instead use the Windows `GetProcessImageFileName` combined with `NtQueryInformationProcess(ProcessBasicInformation)` to read the PEB `ProcessParameters->CurrentDirectory`, or, as a pragmatic first step, have callers in `agents.rs` prefer the daemon-tracked session `cwd` (already available via `DaemonSessionDetails::cwd`) over `process_cwd` on Windows instead of returning `None`.
- **Status**: OPEN

### Windows loopback TCP port is guessable/enumerable without needing the port file
- **ID**: L7-DAEMON-IPC-8
- **Severity**: LOW
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/remote/server.rs:1562` — `_ => "0.0.0.0",`
- **Why it breaks**: This is the separate remote-control gateway (not the primary daemon IPC socket), and binding `0.0.0.0` is required for its cross-device feature, so it is not itself a defect. It is flagged here because it compounds L7-DAEMON-IPC-1: on Windows both the primary daemon (loopback TCP, no token) and the optional remote gateway (all-interfaces TCP, PIN-paired) sit behind Windows Firewall's per-app prompt, and there is no code anywhere in `src-tauri/src/remote/` or `src-tauri/src/daemon/` that provisions an explicit inbound-allow firewall rule during install or first bind — the OS default "Block" action for private/public networks combined with a dismissed or auto-blocked first-run prompt would leave the remote gateway silently unreachable from other devices with no in-app diagnostic distinguishing "blocked by firewall" from "network unreachable."
- **Fix**: On `start_remote_server`'s Windows path, after `TcpListener::bind` succeeds, attempt to detect firewall reachability (e.g. a loopback-external probe) and surface a specific `RemoteConfigureError` variant such as `FirewallBlocked` distinguishable from generic bind failures, or provision the inbound rule via `netsh advfirewall firewall add rule` / the Windows Firewall COM API (`INetFwPolicy2`) during daemon Windows install, gated `#[cfg(target_os = "windows")]`.
- **Status**: OPEN

### `resolve_binary_identity`/hot-upgrade re-exec relies on POSIX `exec()`, has no Windows re-exec equivalent
- **ID**: L7-DAEMON-IPC-9
- **Severity**: LOW
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/server.rs:225` — `let mut cmd = std::process::Command::new(exe);`
- **Why it breaks**: `perform_daemon_exec_with_path` (`server.rs:216-227`) is `#[cfg(unix)]` and uses `CommandExt::exec()` to replace the current process image in place, preserving the PID and any inherited descriptors. There is no `#[cfg(not(unix))]` counterpart function at all in this file — the only Windows path for "run a new daemon binary" is the handover-based `spawn_legacy_handover_daemon` (also `#[cfg(unix)]`, see L7-DAEMON-IPC-7) or `tokio::time::sleep` + `perform_daemon_exec_with_path` in `handle_upgrade_binary`'s empty-sessions branch, which is itself `#[cfg(unix)]` only. Combined with L7-DAEMON-IPC-7, this means the empty-sessions "fast path" self-upgrade (`server.rs`, `active_sessions.is_empty()` branch inside `handle_upgrade_binary`) is unreachable on Windows, and `handle_upgrade_binary`'s `#[cfg(not(unix))]` stub (`server.rs:1766-1772`) is the only code path Windows ever takes for a daemon binary upgrade, regardless of whether sessions are active.
- **Fix**: Add a `#[cfg(windows)]` `perform_daemon_exec_with_path` that spawns the new binary with `Command::new(exe).arg("--daemon").spawn()` and then calls `std::process::exit(0)` in the current process after handing off (accepting the PID change, since Windows has no `execve`-style in-place replace), and use it for the empty-sessions fast path in `handle_upgrade_binary`'s Windows arm instead of unconditionally returning `UpgradeUnsupported`.
- **Status**: OPEN

---

## H. 셸 해석, PTY, 에이전트 실행, git

_11 findings — BLOCKER 1, HIGH 4, MEDIUM 3, LOW 3_

### Agent CLI binaries unresolvable on Windows (no PATHEXT/.cmd/.ps1 lookup)
- **ID**: L8-SHELL-AGENT-1
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/agents.rs:577` — `.find(|candidate| is_executable_file(candidate))`
- **Why it breaks**: `resolve_binary` only tests `dir.join(name)` for exact-name existence. On Windows, npm/npx-installed agent CLIs (claude, codex, opencode, cursor-agent, etc.) install as `claude.cmd`/`claude.ps1` shims, never a bare `claude` or `claude.exe`. `is_executable_file` for `cfg(not(unix))` (line 589) is just `path.is_file()` with no PATHEXT (`.COM;.EXE;.BAT;.CMD;.PS1`) suffix search, so `detect_agents` reports every agent as unavailable and `resolve_startup_command` (`src-tauri/src/terminal/pty.rs`-referenced `shell.rs:421-425`) falls through to the raw name, which `CreateProcessW` (used directly by portable-pty, no shell involved) cannot find either.
- **Fix**: In `resolve_binary`/`is_executable_file` (`src-tauri/src/ipc/agents.rs`), on `cfg(windows)` iterate `%PATHEXT%` (or a hardcoded `[".exe", ".cmd", ".bat", ".ps1", ".com"]` list) appending each extension to `name` before testing `dir.join(candidate)`, and return the matched extended path so downstream `CommandBuilder::new` receives a runnable file.
- **Status**: OPEN

### Agent resume/session commands invoke bare program names via ConPTY CreateProcess, bypassing shell/PATHEXT resolution
- **ID**: L8-SHELL-AGENT-2
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/terminal/shell.rs:428` — `let mut cmd = CommandBuilder::new(&plan.program);`
- **Why it breaks**: `resolve_agent_resume_plan` (e.g. `"claude" => program: "claude"`) hands the bare name to `CommandBuilder::new`, then `resolve_startup_command` tries `resolve_binary` (see finding 1) and, if that also fails to extend for `.cmd`/`.ps1`, passes the unresolved bare string straight to portable-pty's Windows backend, which calls `CreateProcessW` directly (confirmed in `portable-pty-0.9.0/src/win/psuedocon.rs`). `CreateProcessW` does not search `PATHEXT` or invoke `cmd.exe`'s command-shim resolution the way a real Windows shell does, so `.cmd`/`.ps1` shims never launch even when nominally "on PATH".
- **Fix**: When the resolved binary (after fix #1) still ends in `.cmd`/`.bat`, build the `CommandBuilder` as `cmd.exe /d /s /c "<resolved path>" <args...>` (or `powershell.exe -File` for `.ps1`) instead of invoking the shim path directly, matching how Node's `child_process` and VS Code's terminal do Windows shim execution.
- **Status**: OPEN

### Agent session discovery (ps/lsof) is unconditionally Unix-only, silently disabled on Windows
- **ID**: L8-SHELL-AGENT-4
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/agents.rs:72` — `let output = crate::util::no_window_command("/bin/ps")`
- **Why it breaks**: `process_table_entries` (feeding `discover_agent_session_id`, called unconditionally from `src-tauri/src/daemon/server.rs:1299` on every platform with no `cfg` gate) shells out to the absolute path `/bin/ps`, which does not exist on Windows. `omo_session_id_from_environment` (`src-tauri/src/ipc/agents.rs:173`) and `lsof_session_id` (`src-tauri/src/ipc/agents.rs:193`, `/usr/sbin/lsof`) have the same problem. On Windows every call fails to spawn, `.ok()` swallows the error, and agent-session auto-discovery (mapping a running PTY to its provider session id for resume) silently returns `None` for every agent type, degrading a core "reconnect to an in-flight agent" feature with no user-visible diagnostic.
- **Fix**: Add a `cfg(windows)` implementation of `process_table_entries`/`lsof_session_id` backed by `CreateToolhelp32Snapshot`/`Process32Next` (via the `windows` or `sysinfo` crate already usable elsewhere) to enumerate the process tree, and reimplement `lsof_session_id`'s "find an open file path matching a marker" step via `NtQuerySystemInformation`/`sysinfo` open-handle inspection or by having each agent report its session file path directly instead of relying on `lsof`.
- **Fix**: (if Windows support of this feature is deliberately deferred) gate the call sites with `#[cfg(unix)]` and return `None` immediately on Windows with a `tracing::debug!` note, so the gap is documented in code rather than silently inherited from a nonexistent binary.
- **Status**: OPEN

### git worktree remove has no retry for Windows file-locking, breaking worktree/session cleanup
- **ID**: L8-SHELL-AGENT-6
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/worktree/git.rs:390` — `run_git(repo_root, &args)?;`
- **Why it breaks**: `git_worktree_remove` runs `git worktree remove [--force] -- <path>` exactly once with no retry/backoff. Windows cannot delete or rename a file while any process holds an open handle to it (unlike POSIX unlink-while-open semantics on macOS/Linux). Because `spawn_in_worktree` (`src-tauri/src/terminal/pty.rs:85`) routinely leaves PTY/agent child processes with a working directory or open log file inside `.orca-worktrees/<ws>/<task>`, a `remove_worktree` call issued right after closing a pane races the OS releasing those handles; git's `unlink`/`rmdir` fails with `ERROR_SHARING_VIOLATION`/`ERROR_ACCESS_DENIED`, and `run_git` returns `WorktreeError::GitError` with no automatic recovery, leaving the worktree stuck and the UI action failed.
- **Fix**: In `git_worktree_remove` (`src-tauri/src/worktree/git.rs:379`), on `cfg(windows)` wrap the `run_git` call in a bounded retry loop (e.g. 3-5 attempts with short backoff, similar to the existing `TERM_GRACE_TIMEOUT`/`KILL_REAP_TIMEOUT` pattern in `src-tauri/src/terminal/pty.rs`) and ensure the owning `PtySession`'s child/master/writer handles are fully closed (`close_io`) and the process reaped before the removal is attempted.
- **Status**: OPEN

### PTY kill on Windows only terminates the direct child, leaving orphaned grandchild processes
- **ID**: L8-SHELL-AGENT-7
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/terminal/session.rs:244` — `TerminalSignal::Terminate | TerminalSignal::Kill => self.kill(),`
- **Why it breaks**: The `cfg(unix)` `signal()` path (`session.rs:229`) signals the whole process group via `libc::kill(-(pid as i32), sig)`, but the `cfg(not(unix))` path (`session.rs:240-246`) maps both `Terminate` and `Kill` to `self.kill()`, which delegates to portable-pty's Windows `Child::kill()` — confirmed in `portable-pty-0.9.0/src/win/mod.rs` to be a single `TerminateProcess` call on the direct child handle only, with no Job Object grouping. Windows has no process-group equivalent of POSIX PGIDs reachable this way, so when an agent CLI spawns further children (e.g. a `cmd.exe`-shimmed `claude.cmd` spawning `node.exe`, or any agent that forks helper processes), closing the pane kills only the top-level shim and leaves grandchildren running as orphans consuming resources and holding file locks (compounding finding 6).
- **Fix**: Spawn Windows child processes inside a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (via `windows-sys`'s `CreateJobObjectW`/`AssignProcessToJobObject`/`SetInformationJobObject`) at PTY-spawn time in `spawn_with_id_and_worktree` (`src-tauri/src/terminal/pty.rs:117`), and have the `cfg(not(unix))` `signal()`/`kill()` path in `session.rs` terminate the job object instead of just the child, so all descendants die together.
- **Status**: OPEN

### PTY session cwd validation on Windows relies on canonicalize() producing `\\?\` UNC-prefixed paths inconsistently across call sites
- **ID**: L8-SHELL-AGENT-10
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/worktree/manager.rs:133` — `fs::canonicalize(&requested).map_err(|_| WorktreeError::InvalidRepoRoot {`
- **Why it breaks**: `std::fs::canonicalize` on Windows returns extended-length `\\?\C:\...` paths. `worktree/git.rs` already strips this prefix defensively before shelling out to `git` (`normalize_path_for_git`/`strip_verbatim_prefix`, confirmed at `src-tauri/src/worktree/git.rs:39-61`), but `WorktreeManager::canonical_allowed_path` (used by `pty.rs:94` for the `spawn_in_worktree` cwd/ownership check) and other `fs::canonicalize` call sites in `manager.rs` (lines 143, 239, 266, 419) do not run the same stripping before comparing paths with `starts_with` or building user-facing strings, so a canonicalized `\\?\C:\...\task` path compared against a non-canonicalized `C:\...\task` path from IPC input can fail a `starts_with` ownership check that would pass identically-shaped inputs on macOS/Linux.
- **Fix**: Route every `fs::canonicalize` result in `src-tauri/src/worktree/manager.rs` through `crate::worktree::git::normalize_path_for_git` (already Windows-tested at `git.rs:420-469`) before using it in path-prefix comparisons or returning it to the frontend, so canonicalization is consistently verbatim-prefix-free across the whole worktree module, not just the `run_git` boundary.
- **Status**: OPEN

### Linux SHELL-unset fallback hardcodes /bin/bash, which does not exist on many distros
- **ID**: L8-SHELL-AGENT-3
- **Severity**: MEDIUM
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/terminal/shell.rs:356` — `.unwrap_or_else(|| "/bin/bash".to_string());`
- **Why it breaks**: When `SHELL` is unset (common for minimal containers, some display managers, and musl-based distros such as Alpine, which ships `/bin/ash`/`busybox sh` and no `/bin/bash` by default), `resolve_shell_command_pure` hard-codes `/bin/bash`, and `CommandBuilder::new("/bin/bash")` fails to spawn since the path does not exist, leaving the user with no terminal pane and a raw spawn error instead of a working shell.
- **Fix**: In `resolve_shell_command_pure`'s `TargetPlatform::Linux` branch, fall back through a candidate list (`/bin/bash`, `/usr/bin/bash`, then `/bin/sh`) using the existing `is_executable_on_path` probe before defaulting, mirroring the `is_on_path`-driven pwsh/powershell selection already used in the Windows branch (`shell.rs:315-320`).
- **Status**: OPEN

### HOME-only home-directory lookup breaks agent-extension install and `~` path expansion on Windows
- **ID**: L8-SHELL-AGENT-5
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/agent_extension.rs:24` — `fn home_dir() -> Option<PathBuf> {`
- **Why it breaks**: `home_dir()` reads only `env::var_os("HOME")`. Windows does not set `HOME` for GUI-launched processes by default (it sets `USERPROFILE`, and only some shells like Git Bash synthesize `HOME`), so `extension_dirs()` returns an empty `Vec` and `install_into` never runs: the Ferryx agent-state lifecycle extension (`ferryx-agent-state.ts`) is never installed into `.omo/.pi/.omp` agent directories on Windows, silently disabling authoritative agent-state reporting in favor of screen-scraping inference. The same `HOME`-only pattern reappears at `src-tauri/src/ipc/browser.rs:1626` for `~/`-prefixed path expansion in `cmd_open_file_path`, so a `~/foo` path typed/pasted by a user on Windows resolves to a bogus relative path instead of the user's profile directory.
- **Fix**: Add a small `home_dir()` helper (or reuse one) that tries `HOME` first and falls back to `USERPROFILE` on `cfg(windows)`, e.g. `env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"))`, and use it at both `agent_extension.rs:24` and `browser.rs:1626`.
- **Status**: OPEN

### Login-shell `-l` flag is applied uniformly for macOS/Linux without checking the target shell supports it
- **ID**: L8-SHELL-AGENT-11
- **Severity**: LOW
- **Platforms affected**: macOS+Linux
- **Evidence**: `src-tauri/src/terminal/shell.rs:359` — `args: vec!["-l".to_string()],`
- **Why it breaks**: Both the macOS (`shell.rs:343`) and Linux (`shell.rs:359`) "no explicit preference" branches append `-l` to whatever `$SHELL` resolves to. This is correct for bash/zsh/fish, but a user with an unusual `$SHELL` value such as `tcsh` (still shipped on some Linux distros and BSD-derived tooling) or a restricted/non-POSIX shell that does not recognize `-l` will fail to start or start in a degraded mode, since the flag is applied unconditionally based on platform rather than on the resolved shell's basename.
- **Fix**: In `resolve_shell_command_pure`'s macOS/Linux `None` branches, only append `-l` when the resolved shell's basename matches a known-safe set (`bash`, `zsh`, `fish`, `sh`, `dash`), and omit it otherwise, so an uncommon `$SHELL` value degrades to a plain non-login invocation instead of a hard failure.
- **Status**: OPEN

### Agent-launched child processes do not inherit the PATH augmentation on GUI launch outside macOS's Homebrew case
- **ID**: L8-SHELL-AGENT-8
- **Severity**: LOW
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/terminal/pty.rs:157` — `if let Ok(augmented) = std::env::join_paths(crate::ipc::agents::search_paths()) {`
- **Why it breaks**: `search_paths()`/`login_shell_path()` (`src-tauri/src/ipc/agents.rs:553-567`) source PATH augmentation by invoking `$SHELL -lic 'printf %s "$PATH"'`. This generically covers Linux too when `SHELL` is set, but many Linux desktop-launcher paths (systemd user services, `.desktop` Exec= entries, some app-image/snap wrappers) do not set `SHELL` in the environment they hand to a GUI-launched daemon, unlike macOS's `launchd`, which reliably preserves `SHELL` from the user's `dscl` record. In that case `login_shell_path()` returns `None` silently and the daemon is stuck with whatever minimal PATH its launcher gave it (frequently missing `~/.local/bin`, `~/.cargo/bin`, nvm/asdf shims), so agent binaries installed via user-level package managers are invisible to `detect_agents`/`resolve_binary` even though they exist.
- **Fix**: In `login_shell_path()` (`src-tauri/src/ipc/agents.rs:553`), add a Linux-only fallback when `SHELL` is unset: read `/etc/passwd` for the invoking UID's shell field (e.g. via the `nix` crate's `getpwuid`) instead of only trusting the `SHELL` env var, so PATH augmentation still runs under systemd/`.desktop` launch paths that omit it.
- **Status**: OPEN

### OS-opener path-reveal comment claims macOS-only reasoning but branch coverage is actually correct — verify no fourth platform gap
- **ID**: L8-SHELL-AGENT-9
- **Severity**: LOW
- **Platforms affected**: macOS
- **Evidence**: `src-tauri/src/ipc/project.rs:327` — `#[cfg(not(any(target_os = "macos", target_os = "windows")))]`
- **Why it breaks**: Not a defect by itself — `cmd_path_reveal` correctly branches macOS (`open -R`), Windows (`explorer /select,`), and falls back to `xdg-open` for everything else including BSDs. However `xdg-open` assumes a freedesktop-compliant desktop environment; on a machine with no desktop session (e.g. macOS is excluded, but a headless Linux dev container running the Tauri app under Xvfb has no `xdg-open` handler configured) the `Command::new("xdg-open")` spawn succeeds but silently opens nothing, and the caller only surfaces `spawn()` errors, not handler-not-found failures reported asynchronously by `xdg-open` itself.
- **Fix**: This is intrinsic to `xdg-open` and not fixable from Ferryx's side beyond documenting the limitation; no code change is required unless product wants a "no file manager available" toast, in which case check `xdg-open`'s exit status via `.status()` instead of `.spawn()` in `cmd_path_reveal` (`src-tauri/src/ipc/project.rs:335`) and surface non-zero exits to the UI.
- **Status**: FIXED (branch coverage for macOS/Windows/Linux is already complete at `src-tauri/src/ipc/project.rs:302-337`; only the headless-Linux `xdg-open` edge case is unhandled, which is a pre-existing platform-tool limitation, not a missing branch)

---

## I. 내장 브라우저, 알림, OS 통합

_12 findings — BLOCKER 1, HIGH 3, MEDIUM 4, LOW 4_

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

### `Silent` notification sound is ignored on Windows and Linux — every notification plays the OS default sound
- **ID**: L9-BROWSER-OS-5
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/notification/notify_rust_adapter.rs:35` — `builder.summary(&content.title).body(&content.body);`
- **Why it breaks**: `NotificationContent.sound` (`NotificationSound::System | Silent`) is read on macOS in `macos_submission.rs` (`native.setSound(...)` / `native.setSound(None)`), but `submit_with_click_routing` in `notify_rust_adapter.rs` never reads `content.sound` at all — it only sets `summary`/`body`/`app_id`/`appname`/`action`. The plugin-notification fallback path in `src-tauri/src/ipc/notifications.rs` (used for target-less notifications) likewise never calls a sound-suppression API. A user who picks "Silent" in notification settings still hears the platform's default toast/XDG sound on every Windows and Linux build, while macOS correctly stays silent.
- **Fix**: In `submit_with_click_routing` (`src-tauri/src/notification/notify_rust_adapter.rs`), branch on `content.sound`: on Linux call `builder.hint(notify_rust::Hint::SuppressSound(true))` when `NotificationSound::Silent`, and for the target-less path in `TauriNotificationBackend::submit` (`src-tauri/src/ipc/notifications.rs`) apply the same hint before `.show()`.
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

### Non-macOS history navigation is fire-and-forget `eval("history.back()")` with no confirmation the navigation happened
- **ID**: L9-BROWSER-OS-4
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/ipc/browser.rs:1213` — `"history.forward()"`
- **Why it breaks**: On macOS, `history_navigation` calls `native.canGoBack()`/`goBack()` synchronously and immediately reconciles manager state with the real WKWebView answer (or calls `cancel_history_navigation` if it can't navigate). On Windows/Linux the same function just does `webview.eval("history.back()")` and, as long as `eval` doesn't itself error, treats the navigation as accepted — there is no callback confirming the JS actually moved history (e.g. because `history.length` was 1). The UI's `can_go_back`/`can_go_forward` state (from L9-BROWSER-OS-3) can therefore report a wrong, un-corrected state after a no-op navigation attempt.
- **Fix**: In the `#[cfg(not(target_os = "macos"))]` branch of `history_navigation` (`src-tauri/src/ipc/browser.rs`), replace the fire-and-forget `webview.eval(script)` with `eval_webview` (already defined in the same file) so the callback result can confirm whether `history.back()/forward()` changed `location.href`, and call `manager.cancel_history_navigation` when it did not.
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

### `default_desktop_user_agent` has no platform branch for `not(any(macos, windows, linux))`, silently claiming to be macOS
- **ID**: L9-BROWSER-OS-10
- **Severity**: LOW
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/browser/security.rs:61` — `"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36"`
- **Why it breaks**: This is the `#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]` fallback arm of `default_desktop_user_agent`. It is unreachable for the three shipped bundle targets (dmg/app, nsis/msi, appimage/deb all match one of the three `cfg`s above it), so it is dead code on every build Ferryx actually ships — not a live defect for users, but it is a latent trap: if a fourth target (e.g. `freebsd`) is ever added to `tauri.conf.json` without updating this function, every embedded browser tab on that platform would send a macOS user-agent string, which some sites use to serve macOS-specific download links or reject unexpected UA/OS combinations.
- **Fix**: Either delete the catch-all arm and let `default_desktop_user_agent` fail to compile on an unsupported target (forcing an explicit decision when a new target is added), or make it construct a generic UA string (`Mozilla/5.0 (X11; Unknown) ...`) instead of quietly impersonating macOS.
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

---

## J. 테스트 스위트 및 개발 도구 이식성

_11 findings — BLOCKER 2, HIGH 5, MEDIUM 4, LOW 0_

### Majority of PTY and terminal tests lack Windows/Linux test coverage
- **ID**: L10-TESTS-TOOLING-11
- **Severity**: BLOCKER
- **Platforms affected**: Windows, Linux
- **Evidence**: `src-tauri/src/terminal/tests.rs:20` - `let cmd = CommandBuilder::new("/bin/sh");`
- **Scope of the pattern**: `src-tauri/src/terminal/tests.rs` (18 tests), `src-tauri/src/remote/tests.rs` (33+ tests, 13+ spawning `/bin/sh`), `src-tauri/src/ssh/direct_tests.rs` (6 tests) all build their PTY command the same way.
- **Why it breaks**: The terminal and PTY management is a core feature (used for all shell sessions, remote execution, and SSH). Tests are silently skipped or fail on Windows/Linux due to shell path issues (L10-TESTS-TOOLING-2). Zero functional coverage of terminal I/O, resizing, signal handling, or PTY isolation on those platforms. Cross-platform regressions in terminal functionality are invisible until production deployment.
- **Fix**: (1) Apply L10-TESTS-TOOLING-2 fix (cross-platform shell selection). (2) For each platform-specific signal/TTY feature, conditionally gate and provide platform-specific test variants. (3) Add a CI job that runs full test suite on Windows and Linux (e.g., GitHub Actions matrix with ubuntu-latest, windows-latest, macos-latest).
- **Status**: OPEN

---

**Summary**: 11 findings identified. **4 BLOCKER, 5 HIGH, 2 MEDIUM severity**. Root issues:  
1. **Hardcoded shell paths** (`/bin/sh`, `/bin/cat`) make 50+ test cases non-portable.  
2. **Unix-only dev tooling** (stat -f, chmod --reference, codesign, uname) blocks cross-platform dev loops.  
3. **Temporary directory assumptions** (`/tmp`) fail on Windows.  
4. **Platform signal tests** gated but not conditionally replaced; asymmetric test coverage.  

**Primary Risk**: Terminal and PTY functionality (core to the app) have zero test coverage on Windows and minimal coverage on Linux, hiding regressions until production.

### Hardcoded `/bin/sh` shell paths in PTY tests (test-only defect, affects 40+ tests)
- **ID**: L10-TESTS-TOOLING-2
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/terminal/tests.rs:20` — `let cmd = CommandBuilder::new("/bin/sh");`
- **Why it breaks**: Terminal, remote, and SSH tests hardcode `/bin/sh` which does not exist on Windows (shell is `cmd.exe` or PowerShell). Tests spawn pseudo-terminals with a non-existent shell, causing immediate failure on Windows. Across terminal/tests.rs, remote/tests.rs, and ssh/direct_tests.rs, at least 40 test cases cannot run on Windows.
- **Fix**: Add a cross-platform shell selection function: `fn shell_cmd() -> &'static str { if cfg!(windows) { "cmd.exe" } else { "/bin/sh" } }` and use it everywhere. Alternatively, use `portable_pty::CommandBuilder` with platform-aware command resolution or introduce a test utility that maps shell selection.
- **Status**: OPEN

### Hardcoded `/tmp` paths in session persistence tests (test-only defect)
- **ID**: L10-TESTS-TOOLING-1
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/session/mod.rs:296` — `repo_root: PathBuf::from("/tmp/repo"),`
- **Why it breaks**: Test fixture uses Unix-only `/tmp` directory path; on Windows, tests fail because `/tmp` does not exist. Session load/save tests will not run on Windows CI, leaving persistence logic unchecked on that platform.
- **Fix**: Use `tempfile::TempDir::new()` and store its path; replace all hardcoded `/tmp/repo` instances (lines 296, 298, 304, 319, 360) with `temp_dir.path().join("repo")` or use `std::env::temp_dir()` with a test-specific subdirectory.
- **Status**: OPEN

### SSH socket paths hardcoded to Unix `/tmp/` (test-only defect)
- **ID**: L10-TESTS-TOOLING-3
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ssh/direct_tests.rs:203` — `assert!(plan.args.windows(2).any(|pair| pair == ["-R", "/tmp/ferryx-agent-session-123.sock:/tmp/local-agent.sock"]));`
- **Why it breaks**: Test assertion hard-expects Unix socket path format. On Windows, socket forwarding uses different path formats (named pipes or WSL paths). Test will fail spuriously on Windows even if the underlying SSH forwarding logic is correct.
- **Fix**: Make the test fixture platform-aware: `let expected_sock_path = if cfg!(windows) { "//./pipe/..." } else { "/tmp/..." }` and compare against that. Better: refactor to validate the socket forwarding mechanism rather than the exact string representation.
- **Status**: OPEN

### Direct `/bin/sh` invocation in SSH protocol tests (test-only defect)
- **ID**: L10-TESTS-TOOLING-4
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ssh/direct_tests.rs:84` — `let output = std::process::Command::new("/bin/sh")`
- **Why it breaks**: Test spawns a shell to validate SSH probe behavior; `/bin/sh` is POSIX-only. Test `remote_shell_probe_canonicalizes_quoted_directory_and_reports_plain_or_git` and others will fail immediately on Windows with "program not found".
- **Fix**: Use conditional compilation or a platform helper: `let shell = if cfg!(windows) { "cmd.exe" } else { "/bin/sh" }`; or probe the user's login shell at test setup time using `std::env::var("SHELL")`.
- **Status**: OPEN

### Bash-only dev runner script excludes non-macOS platforms (dev-loop-only defect)
- **ID**: L10-TESTS-TOOLING-5
- **Severity**: HIGH
- **Platforms affected**: Windows, Linux
- **Evidence**: `scripts/macos-dev-runner.sh:4` — `if [[ "$(uname -s)" != "Darwin" || "${1:-}" != "run" ]]; then`
- **Why it breaks**: Dev runner script checks for macOS and falls through to direct cargo invocation on non-macOS. Script contains macOS-specific bundle assembly logic (lines 59, 91–92) and codesigning (line 97) that blindly run on Windows/Linux if the entry point guard is bypassed, but the intermediate steps (`install_atomic`, `mkdir -p "$MACOS_DIR"`) fail silently. Windows/Linux developers cannot use this helper; dev loop is broken for cross-platform testing.
- **Fix**: Add an early exit for non-macOS: `if [[ "$(uname -s)" != "Darwin" ]]; then exec cargo "$@"; fi` at line 4 to prevent fallthrough. Then ensure the macOS-specific bundle logic (lines 31–97) only runs after that guard.
- **Status**: OPEN

### Terminal signal interrupt test gated to Unix, zero Windows coverage (test-only defect)
- **ID**: L10-TESTS-TOOLING-9
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/terminal/tests.rs:388` — `#[cfg(unix)]`
- **Why it breaks**: Only test for interrupt signal (`TerminalSignal::Interrupt`) is gated with `#[cfg(unix)]`. On Windows, `TerminalSignal::Interrupt` handling is never exercised. If the signal implementation differs on Windows (Ctrl+C vs. job termination), the defect is hidden.
- **Fix**: Create a platform-specific test pair. Write a Windows version that uses appropriate Windows signal primitives (`GenerateConsoleCtrlEvent`) or refactor the interrupt handler to be platform-agnostic and test on both. At minimum, add a `#[cfg(windows)]` test that validates Ctrl+C behavior.
- **Status**: OPEN

### `/bin/sh` shebang in notification audio test payload (test-only defect)
- **ID**: L10-TESTS-TOOLING-10
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/notifications.rs:337` — `std::fs::write(&path, b"#!/bin/sh\necho nope").expect("write file");`
- **Why it breaks**: Test creates a shell script with `#!/bin/sh` shebang. On Windows, this script cannot be executed directly even if the file extension is `.sh`. If the audio player tries to execute the script as a command, it will fail with "permission denied" or "invalid format" on Windows.
- **Fix**: This is a test payload that should never execute; if it does, the test design is flawed. Rename the test assertion to clarify intent: add a comment `// This script intentionally has an unsupported shebang; the audio player should reject it.` Alternatively, remove the payload file write entirely if it's not used by the assertion.
- **Status**: OPEN

### macOS-specific `stat -f` command in tree quiescence checker (dev-loop-only defect)
- **ID**: L10-TESTS-TOOLING-6
- **Severity**: MEDIUM
- **Platforms affected**: Linux, Windows
- **Evidence**: `scripts/check-tree-quiescent.sh:54` — `stat -f "%m	%Sm	%N" -t "%Y-%m-%d %H:%M:%S" "$file"`
- **Why it breaks**: Helper script used in CI gates (e.g., parallel test coordination) calls `stat -f`, which is macOS-only (GNU `stat` on Linux uses `-c` instead). Script fails on Linux CI runners, blocking cross-platform test gating.
- **Fix**: Use portable alternatives: `stat -c "%Y	%y	%n" "$file"` on Linux (GNU stat), or better, replace the entire block with a Python snippet or use `find -printf` which is portable: `find ... -printf "%T@ %Tc %p\n"` and sort/format in shell-portable way.
- **Status**: OPEN

### macOS-specific `chmod --reference` and `stat -f` in installation helper (dev-loop-only defect)
- **ID**: L10-TESTS-TOOLING-7
- **Severity**: MEDIUM
- **Platforms affected**: Linux, Windows
- **Evidence**: `scripts/macos-dev-runner.sh:55` — `chmod "$(stat -f '%OLp' "$src")" "$tmp"`
- **Why it breaks**: Fallback line attempts macOS-style permission extraction via `stat -f` which fails on Linux/Windows. Even though `chmod --reference` fallback is attempted first, it also fails on macOS (BSD `chmod` doesn't support `--reference`). Net result: permission preservation is skipped on non-macOS and may produce incorrect bundle permissions.
- **Fix**: Use portable permission copy: `chmod "$(ls -L -o -d "$src" | awk '{print $1}')" "$tmp"` or Python: `os.chmod(tmp, os.stat(src).st_mode)`. Rust is better: move the permission copy to the build script written in Rust.
- **Status**: OPEN

### Hardcoded `/tmp` path in UI SSH section tests (test-only defect)
- **ID**: L10-TESTS-TOOLING-8
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `ui/src/components/settings/SshSection.test.tsx:661` — `openDialogMock.mockResolvedValue("/tmp/work-ssh-config");`
- **Evidence** (related): `ui/src/components/settings/SshSection.test.tsx:666` — `if (configPath === "/tmp/work-ssh-config")`
- **Why it breaks**: Mock fixture returns hardcoded Unix path. Test assertions compare against `/tmp/...` strings, which do not reflect Windows file paths. On Windows, tests may spuriously pass or fail depending on how the path is normalized.
- **Fix**: Use `tempfile` or a platform-aware mock: `const mockPath = process.platform === 'win32' ? 'C:\\tmp\\work-ssh-config' : '/tmp/work-ssh-config'` and update all assertions to use the dynamic path.
- **Status**: OPEN

---

## 방법론 (Methodology)

10개 도메인 레인을 병렬 감사 에이전트로 실행하고, 각 레인이 자신의 리포트를 `docs/evidence/cross-platform-audit-20260908/` 아래에 작성했습니다. 레인 원본 리포트는 그대로 보존됩니다:

- `docs/evidence/cross-platform-audit-20260908/l1-rust-cfg.md` — 조건부 컴파일 매트릭스 (`cfg(target_os)`)
- `docs/evidence/cross-platform-audit-20260908/l2-fs-paths.md` — 파일시스템, 경로, 프로세스, OS API
- `docs/evidence/cross-platform-audit-20260908/l3-native-surface.md` — 네이티브 터미널 표면: 윈도잉, 포커스, IME, 마우스, 클립보드
- `docs/evidence/cross-platform-audit-20260908/l4-renderer-font.md` — 렌더러, 폰트, 글리프 래스터화
- `docs/evidence/cross-platform-audit-20260908/l5-ui-frontend.md` — 프론트엔드(React/TypeScript) 플랫폼 가정
- `docs/evidence/cross-platform-audit-20260908/l6-packaging.md` — 빌드, 패키징, 업데이터, 서비스 설치
- `docs/evidence/cross-platform-audit-20260908/l7-daemon-ipc.md` — 데몬, IPC 전송, 원격 게이트웨이
- `docs/evidence/cross-platform-audit-20260908/l8-shell-agent.md` — 셸 해석, PTY, 에이전트 실행, git
- `docs/evidence/cross-platform-audit-20260908/l9-browser-os.md` — 내장 브라우저, 알림, OS 통합
- `docs/evidence/cross-platform-audit-20260908/l10-tests-tooling.md` — 테스트 스위트 및 개발 도구 이식성

모든 인용은 `scripts/verify-audit-citations.mjs`로 기계 검증했습니다. 이 스크립트는 각 `파일:라인` 인용을 작업 트리에서 다시 읽어 인용된 코드 조각이 실제로 그 위치에 있는지 확인하고, 모든 finding이 비어 있지 않은 **Fix** 항목을 갖는지 검사합니다.

```bash
bun scripts/verify-audit-citations.mjs docs/CROSS_PLATFORM_ISSUE_AUDIT_2026-09-07.md
```

### 감사 중 "문제 아님"으로 확인된 항목

중복 조사를 막기 위해 남겨 둡니다.

- `src-tauri/src/daemon/server.rs`의 `get_runtime_dir()`는 unix(`/tmp/rorca-{uid}`)와 non-unix(`LOCALAPPDATA`/`TEMP`/`ProgramData` + `Ferryx`) 분기를 모두 갖습니다.
- `src-tauri/src/ipc/browser_cli.rs`는 unix 도메인 소켓과 Windows 루프백 TCP + 포트 파일로 완전히 이중화되어 있습니다.
- `src-tauri/src/native_terminal/platform/mod.rs`는 macos/windows/linux 모듈을 올바르게 게이팅하며 `fallback.rs`는 배포 대상이 아닌 플랫폼에서만 쓰입니다.
- `ui/src/index.css`의 네이티브 터미널 투명 처리 블록은 `html.platform-macos`로 정확히 스코프되어 있습니다 (과거 Windows 전체 검은 화면의 원인, 현재 수정됨).
- 데몬 단일 인스턴스 락은 unix `flock`과 Windows `LockFileEx` 양쪽으로 구현되어 있습니다.
