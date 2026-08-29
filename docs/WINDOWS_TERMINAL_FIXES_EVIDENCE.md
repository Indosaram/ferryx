# Windows Terminal Fixes — Evidence (2026-08-29)

Three Windows defects fixed on `main` with failing-first proofs, macOS regression gates, and
real-Windows-host verification (`maho-win`, Windows 10.0.26200, x86_64-pc-windows-msvc).

| # | Defect | Root cause (file:line) | Fix |
|---|--------|------------------------|-----|
| 1 | Terminal typing dead on Windows | No Windows equivalent of the macOS focus monitor: `src-tauri/src/lib.rs:513-514` gated `install_macos_terminal_focus_monitor` to macOS; the native child HWND (`WS_EX_NOACTIVATE|WS_EX_TRANSPARENT`, `WM_NCHITTEST→HTTRANSPARENT`, `platform/windows.rs:114-123,234-268`) swallows clicks, so no DOM events fire, nothing focuses the input sink, and WebView2 may hold no Win32 keyboard focus | `native_terminal/platform/windows_focus.rs`: non-consuming `WH_MOUSE_LL` hook on `WM_LBUTTONUP`; `ScreenToClient`+DPI→`session_at_logical_point`; best-effort `SetFocus` on the WebView2 `Chrome_WidgetWin_1` child; emits the existing `native_terminal_focus` event. Wired in `lib.rs:21,515` (Windows-only cfg). Frontend handler unchanged (already focuses sink immediately + next frame) |
| 2 | Shell not selectable | All PTY spawns used `CommandBuilder::new_default_prog()` (`daemon/server.rs:1046`, `terminal/pty.rs:55`); no preference surface existed | New `terminal/shell.rs`: pure `resolve_shell_command_pure` (Windows default pwsh.exe→powershell.exe; explicit pwsh/powershell/cmd/wsl/custom; non-Windows `$SHELL`→zsh/bash). Plumbed end-to-end: `TerminalPreferences.default_shell` + overrides (`preferences.rs`), `TerminalOverridesRequest.shell` (`ipc/preferences.rs`), `DaemonRequest::Spawn.shell` optional + back-compat test (`protocol.rs:83,558`), `spawn_terminal`/`handle_spawn` adoption (`client.rs:524`, `server.rs:1055`), `cmd_terminal_spawn` preference fallback (`ipc/terminal.rs:649`), Settings → Terminal "Default shell" selector + Custom path (`TerminalSection.tsx`, `SettingsDialog.tsx`, `terminalSettings.ts`, `tauri.ts`) |
| 3 | Background console windows flash | Zero `CREATE_NO_WINDOW` usage anywhere; `main.rs:2` `windows_subsystem="windows"` covers release GUI only, not spawned children | New `util/mod.rs` helper (`CREATE_NO_WINDOW=0x0800_0000`, std+tokio variants, no-op off-Windows) adopted at all 6 Windows-reachable spawn sites: `worktree/git.rs:83`, `daemon/client.rs:340` (daemon autolaunch), `ipc/browser.rs:1510` (URL open), `notification/mod.rs:62`, `terminal/preferences.rs:522` (ghostty CLI), `ipc/agents.rs:67` (agent shell probe). macOS/Linux-only spawns (launchctl, `open`, `xdg-open`, `lsof`) untouched |

## Proof

### C1 — Shell selection (macOS-runnable)
- RED (captured by lead, mutation proof): stubbing `resolve_shell_command_pure` → `cargo test --lib terminal::shell` → **8/8 FAILED**
  (`test_windows_default_pwsh_present`, `test_windows_default_pwsh_absent`, `test_windows_explicit_choices`,
  `test_custom_path_passthrough`, `test_macos_default_with_shell_env`, `test_linux_default`,
  `test_empty_and_whitespace_preference_treated_as_default`, +1)
- GREEN: restore → **8/8 ok**. Full `cargo test --lib`: **404 passed / 0 failed**.
- UI: `bun run build` (tsc + vite) green; targeted vitest (TerminalSection, terminalSettings, tauri.test) **43/43 green**.
- Protocol back-compat: `daemon/protocol.rs:558` — Spawn JSON without `shell` deserializes (None); with `"pwsh"` roundtrips.

### C2 — Console-window suppression (real Windows host)
- `src-tauri/src/util/mod.rs` `#[cfg(windows)]` test asserts `CREATE_NO_WINDOW == 0x0800_0000` and that a
  helper-configured `cmd /C exit 0` spawn succeeds (std exposes no creation-flags getter); a
  `#[cfg(not(windows))]` passthrough test keeps macOS honest.
- Executed ON maho-win: `cargo test --manifest-path src-tauri/Cargo.toml --lib` → see §Windows results below.
- Windows test-launch note: std exposes no creation-flags getter, so the `#[cfg(windows)]` helper test asserts the constant and that a helper-configured `cmd /C exit 0` spawn succeeds (flag application is the single reviewed `cfg(windows)` call site). Running the suite on Windows also required the comctl32 v6 manifest embedding (commit `022ecd0`) — without it every test binary dies at load (STATUS_ENTRYPOINT_NOT_FOUND).

### C3 — Typing
- Root cause documented above (file:line). Fix is Windows GUI-level; automated channels:
  - macOS `cargo check` proves cfg-gating (non-Windows behavior untouched); full lib suite green (no regression).
  - Real Windows host: headless daemon E2E `script/qa/win-daemon-e2e.mjs` proves the daemon→ConPTY input path
    (TCP handshake → spawn → `write "echo ferryx-e2e"` → `describeSession.endSequence` advances). See §Windows results.
  - GUI click-to-type behavior itself cannot be automated off-Windows-GUI: **user manual QA checklist below.**

### C4 — macOS regression
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` → 404 passed / 0 failed (log: /tmp/ferryx-win-gates.log)
- `cargo check` clean; `cd ui && bun run build` green; targeted vitest 43/43.
- macOS-only focus/IME code paths (`install_macos_terminal_focus_monitor` etc.) untouched — `git diff` shows no edits inside them.

## Windows results (maho-win, Windows 10.0.26200, x86_64-pc-windows-msvc, rustc 1.97.0)
- Windows ui build: green (`bun install` 359 pkgs; `bun run build` built in 14.88s).
- Windows `cargo test --lib` (after the manifest fix below): **280 passed / 28 failed** — reproduced twice with identical results. The 28 failures are
  byte-identical to the pre-fix baseline at f157191 (**269 passed / 28 failed**, identical failure set via diff),
  i.e. **zero regressions from these fixes**. They are first-ever-on-Windows tests assuming POSIX
  (PTY tests spawning `/bin/sh`, git worktree ops, DirectWrite font metrics, agent `$SHELL` probe).
- `#[cfg(windows)]` note: std exposes no getter for creation flags, so the helper test asserts the
  constant value and that a helper-configured `cmd /C exit 0` spawn succeeds; the flag application is a
  single `cfg(windows)` call site in `configure_no_window`, verified by review. Windows cargo-test
  launches additionally required embedding a Common-Controls v6 manifest for test executables
  (commit `022ecd0`): without it the loader binds comctl32 v5 and every test binary dies at load with
  STATUS_ENTRYPOINT_NOT_FOUND (TaskDialogIndirect via muda) — tauri-build's resource manifest only
  reaches bin targets.
- Daemon E2E (script/qa/win-daemon-e2e.mjs, headless, no GUI): **PASS** —
  `handshakeOk` → `registerWorkspaceOk` → `spawnOk` (session running) → `writeOk` for `echo ferryx-e2e\r`
  → `describeSession.endSequence` **0 → 9** (typed bytes reached ConPTY and the echo advanced the ring).
  Transcript captured in-session on maho-win (daemon port 50100, pid 12432; daemon killed afterwards).

## Environment notes (maho-win setup used for verification)
- ghostty vendored submodule must be populated (`git submodule update --init --depth 1 src-tauri/vendor/ghostty`).
- macOS bsdtar archives carry AppleDouble `._*` members that Windows bsdtar extracts as real files;
  `capabilities\._default.json` breaks tauri capability parsing (non-UTF-8). Cleaned with `del /s /q ._...`.
- `target\debug\ferryx.exe` was intermittently removed minutes after linking on this host (likely AV
  heuristics; no MpThreat record). Running the daemon from a renamed copy (`ferryx-run.exe`) was reliable.

## User manual QA checklist (Windows app)
1. `bun tauri dev` (또는 평소 Windows 실행 방식)으로 앱 실행 — **백그라운드에 cmd/powershell 콘솔 창이 안 뜨는지** 확인 (앱 시작 직후, git 작업 발생 시, 외부 링크 클릭 시, 알림 설정 열기 시).
2. 터미널 탭 열기 → 터미널 영역 **한 번 클릭 후 영문 타이핑** → 글자가 즉시 입력되는지 확인.
3. 같은 창에서 **한글 타이핑** → IME 조합(예: "한글")이 정상 입력되는지 확인.
4. 분할(split) 후 다른 창 클릭 → 클릭한 창에 바로 타이핑되는지 확인.
5. 설정 → 터미널 → **Default shell** 드롭다운: Windows PowerShell / PowerShell (pwsh) / 명령 프롬프트 / WSL / 사용자 지정 각각 선택 후 새 탭 열어 해당 셸 프롬프트가 뜨는지 확인 (pwsh 미설치 시 기본값은 Windows PowerShell).
6. 기존 macOS 동작 회귀 없음은 자동 게이트로 검증 완료 — macOS에서 타이핑/포커스/IME가 이전과 동일한지만 빠르게 확인 요망.
