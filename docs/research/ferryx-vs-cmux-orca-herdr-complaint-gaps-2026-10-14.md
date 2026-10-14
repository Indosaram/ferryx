# Ferryx vs. cmux / Orca / Herdr user complaints: gap analysis (2026-10-14)

Companion to [cmux-orca-herdr-user-complaints-2026-10-14.md](./cmux-orca-herdr-user-complaints-2026-10-14.md). For each complaint theme that users raised against cmux, Orca and Herdr, this asks: does Ferryx already avoid it, share it, or partly address it? Every verdict below comes from reading Ferryx source at HEAD `99e7ad40`; nothing was built, run or measured, so performance claims are code-level only.

Verdicts: **HANDLED** = the code addresses the complaint class. **PARTIAL** = the core is addressed but a named gap remains. **SAME WEAKNESS** = Ferryx has the same problem users complain about elsewhere.

## Summary

| # | Complaint theme (who it hurts most) | Ferryx | Ferryx gap that matches a competitor complaint |
|---|---|---|---|
| 1 | Platform lock-in (cmux macOS-only, #330 = 220 reactions) | PARTIAL | No Windows ARM desktop or remote helper; Linux ARM desktop packaging unverified |
| 2 | Crashes and session loss on restart/update (all three; Herdr "one crash kills every agent") | PARTIAL | Daemon process death still kills every PTY; crash-restart exists only on macOS (launchd) |
| 3 | Memory growth, idle CPU, battery, frame caps (cmux 70 GB, Orca Electron, Herdr 60 Hz) | HANDLED (code-level) | No total memory ceiling across panes; no measured idle-CPU or battery numbers |
| 4 | Rendering: CJK fonts, light/dark theme, scrollback (cmux CJK clipping, Herdr OSC 11 / mode 2031) | PARTIAL | Terminal is always dark: Ghostty `light:/dark:` picks dark, OS appearance changes and mode 2031 are not propagated |
| 5 | Input: CJK IME, remapping, hotkey conflicts (Orca Korean doubling, cmux IME crashes) | PARTIAL | Shortcuts cannot be remapped by users |
| 6 | Clipboard, selection, copy mode (Herdr's signature pain) | PARTIAL | No keyboard copy mode (vi/tmux style) |
| 7 | Agent status and notifications (all three; "is my agent done?") | PARTIAL | Claude/Codex rely on screen scraping with no time-based hysteresis |
| 8 | SSH / remote / multi-machine (cmux "local only", Herdr remote slowness) | PARTIAL | Remote helper death loses that host's sessions; no automatic reattach after sleep found |
| 9 | Worktree / git workflow (Orca single repo per workspace, no jj) | PARTIAL | Single repo per workspace, no jj, hardcoded `.orca-worktrees`, leaked worktrees only detected |
| 10 | Install / update / signing (Orca SmartScreen, cmux Gatekeeper/updater) | PARTIAL | Updater download has no timeout; Windows NSIS/MSI signing not configured in `tauri.conf.json` |
| 11 | Trust: hooks in agent configs, telemetry, accounts, bypass wrappers (cmux #3547, Orca hooks) | PARTIAL | Writes `ferryx-agent-state.ts` into existing agent extension dirs (local and remote) with no removal path |
| 12 | UX / missing features (Orca: LSP, multi-window; Herdr: tmux parity) | PARTIAL | No LSP/editor, single window, no keyboard pane swap/move |

**Where Ferryx is clearly ahead:** no telemetry SDK, no forced account, no permission-bypass flag injected (theme 11); update handover keeps sessions alive (theme 2); occlusion suspend and display-rate vsync instead of a hard frame cap (theme 3); IME composition ownership and the Korean tail-key swallow (theme 5); worktree delete refuses while sessions run (theme 9).

**Where Ferryx would draw the same complaints today:** the always-dark terminal (theme 4), no copy mode (theme 6), single daemon/helper as a point of failure off macOS (themes 2 and 8), the agent extension left behind after uninstall (theme 11), and no editor/LSP (theme 12).

## Evidence by theme

### 1. Platform support - PARTIAL
- `src-tauri/src/native_terminal/platform/mod.rs:3-19`, `:59-80`: separate compositor modules for macOS, Windows and Linux plus a fallback; no single-OS lock-in.
- `src-tauri/src/native_terminal/platform/linux.rs:26-27`, `:511-512`: both Xlib and Wayland (`wl_subsurface`) handles.
- `src-tauri/src/ssh/helper_assets.rs:102-108`, `scripts/build-remote-helpers.mjs:7`: a Windows remote host resolves only to `x86_64`; Windows ARM64 returns `Unsupported`.
- `scripts/lib/release-platforms.mjs:626`: Linux ships as `appimage,deb`; no `aarch64-pc-windows` desktop target was found.

### 2. Crashes and session loss - PARTIAL
- `src-tauri/src/daemon/launchd.rs:1-21`: on macOS launchd owns the daemon (`KeepAlive { SuccessfulExit = false }`), so quitting or crashing the GUI does not kill PTYs. Lines 18-21 state Windows and Linux have no service registration: the detached daemon survives a GUI exit but is never restarted after a crash.
- `src-tauri/src/daemon/handover.rs:827-889`: on update the old daemon keeps its PTY masters and retires only after its last session ends.
- `ui/src/state/workspaceRestore.ts:188-205`: a session whose PTY is gone is restored as exited standby instead of silently respawning a shell.
- `src-tauri/src/native_terminal/surface_host.rs:1852-1856`: each GPU frame runs under `catch_unwind`.

### 3. Performance - HANDLED (code-level, unmeasured)
- `src-tauri/src/native_terminal/surface_error.rs:4-18`, `surface_host.rs:6091-6100`: an occluded surface suspends rendering.
- `surface_host.rs:1863-1913`: `FrameClock` coalesces output bursts into one pass; `renderer/gpu_context.rs:185` uses `PresentMode::AutoVsync` (display-rate, not a fixed 60 Hz).
- `src-tauri/src/terminal/output_hub.rs:8-16`: 512 KiB ring per session, bounded broadcast with `ReplayGap` re-sync.
- `surface_host.rs:43-46`: agent detection polls every 50 ms attached / 250 ms background, a small per-pane idle cost.

### 4. Rendering fidelity - PARTIAL
- `src-tauri/src/native_terminal/renderer/coretext_font.rs:744-749`: Hangul resolves through the CoreText fallback chain (tested); `terminal/pty.rs:475-479` forces a UTF-8 locale. CJK fallback on Linux/Windows is unverified.
- `src-tauri/src/terminal/preferences.rs:432-456`: for `light:X,dark:Y` the dark variant always wins.
- `ui/src/lib/settingsRuntimeBridge.ts:18-23`: OS appearance changes re-theme the web UI only; no mode 2031 handling exists in `src-tauri/src`.

### 5. Input and IME - PARTIAL
- `ui/src/components/NativeTerminalPane.tsx:267-273`: keydowns during composition (including keyCode 229) belong to the IME and are not forwarded.
- `NativeTerminalPane.tsx:3694-3719`, `:3766-3788`, `:509`: committed text is sent once at `compositionend`, and a replayed tail key is swallowed within 100 ms, the Korean doubling bug reported against Orca and cmux.
- `ui/src/lib/shortcuts.ts:531-545`: shortcuts are suppressed during composition and for AltGr. `shortcuts.ts:78-394` is a fixed table with no user override.

### 6. Clipboard, selection, copy mode - PARTIAL
- `NativeTerminalPane.tsx:306-332`, `:1711`: platform-specific copy/paste; `copySelectionOrInterrupt` copies when there is a selection, otherwise interrupts.
- `src-tauri/src/native_terminal/paste.rs:13-50`: paste safety check and bracketed paste.
- `NativeTerminalPane.tsx:1797-1845`, `src-tauri/src/clipboard_image.rs`: image paste saves (or uploads to the remote) and types the quoted path.
- No `copyMode`/`viMode`/`copy_mode`/`vi_mode` anywhere in `ui/src` or `src-tauri/src` (0 matches in both).

### 7. Agent status and notifications - PARTIAL
- `surface_host.rs:2116-2118`, `:4307-4312`: a self-reporting agent or an exited process turns screen scraping off for that session.
- `src-tauri/src/agent_detect/engine.rs:68-97`: no match keeps the previous state; events fire only on change (`surface_host.rs:2142`).
- `ui/src/lib/notificationCoordinator.ts:282-367`: notify only on entering waiting/done, de-duplicate repeats, suppress a bell within 1.5 s of completion, skip when focused; permission denial is rechecked on focus (`:77-128`).

### 8. SSH / remote - PARTIAL
- `src-tauri/src/ferryx_scope/ssh/process.rs:455-461`: the remote helper `setsid`s, so remote PTYs outlive the client's SSH session.
- `src-tauri/src/ssh/bridge.rs:2048-2054`, `:1586`: `reattach` verifies host/owner/epoch; `detach_without_kill` drops the handle without killing the child.
- `src-tauri/src/ssh/direct.rs:84-86`: `ServerAliveInterval=15`, `ServerAliveCountMax=2` detect a dead link in about 30 s.
- `src-tauri/src/ferryx_scope/ssh/helper.rs:168`, `:668`: a new helper epoch rejects old targets, so a helper crash loses that host's sessions.

### 9. Worktree / git - PARTIAL
- `src-tauri/src/worktree/registry.rs:42-62`: one workspace id maps to exactly one repo root.
- `src-tauri/src/worktree/manager.rs:15`: worktree root is the constant `.orca-worktrees`; no `jj`/`jujutsu` in `src-tauri/src`.
- `src-tauri/src/remote/workspace_api/worktrees.rs:587-588`, `manager.rs:854-869`: delete returns `WORKTREE_BUSY` while sessions run and refuses unmerged branches.
- `src-tauri/src/worktree/rescan.rs:551`: background rescan reports pruned worktrees (detection, not cleanup).

### 10. Install / update / signing - PARTIAL
- `src-tauri/tauri.conf.json:107`: hardened runtime; notarization is enforced by the local runbook (`docs/releases/LOCAL_RELEASE_RUNBOOK.md:171-179`, `:250`).
- `src-tauri/src/ipc/updater.rs:44-67`: Store/MSIX installs (package identity, `WindowsApps`, `FERRYX_STORE_BUILD`, `msix.marker`) skip the self-updater.
- `ui/src/lib/updater.ts:119-153`: `downloadAndInstall` has no timeout or cancel, so a stalled download sits in "downloading" indefinitely.

### 11. Trust - PARTIAL
- No telemetry SDK: no `sentry`/`posthog` in `src-tauri/src` or `ui/src` source.
- No bypass flag injected: `--dangerously-skip-permissions` appears only in two UI tests where a user types it into agent args (`ui/src/components/SettingsDialog.test.tsx:562`, `ui/src/lib/agentsSettings.test.ts:36`).
- Account is optional: `ui/src/lib/onboarding.ts:312-313`.
- `src-tauri/src/agent_detect` / `src-tauri/src/ipc/agents.rs:425-435` only read `~/.claude/projects`; nothing is written into `~/.claude`.
- **However** `src-tauri/src/daemon/agent_extension.rs:73-92` writes `ferryx-agent-state.ts` into `~/.omo|.pi|.omp/agent/extensions/` when that directory exists, and `src-tauri/src/ssh/direct.rs:175` / `ssh/operations.rs:261-281` install the same file on remote hosts. No removal path was found, so it outlives an uninstall, the class of complaint Orca drew for its hooks.

### 12. UX / missing features - PARTIAL
- `ui/src/state/paneTree.ts:155-172`, `ui/src/components/TerminalSplitView.tsx:468-472`: pane swap and pane-to-tab by drag only; `ui/src/lib/shortcuts.ts` has no swap/move action.
- `src-tauri/tauri.conf.json:14-27`: a single `main` window.
- No Monaco/CodeMirror/LSP client in `ui/src`; files open as read-only previews.
- `ui/src/App.tsx:2378`, `:2435`: closing a tab or pane always confirms while an agent is working or waiting; the general close confirmation defaults off (`ui/src/lib/generalSettings.ts:17`).

## Highest-leverage fixes (by competitor complaint volume)
1. **Follow OS light/dark in the terminal** (theme 4): honour Ghostty `light:`/`dark:` and call the existing theme reapply on appearance change.
2. **Daemon crash-restart on Windows and Linux** (theme 2): a `systemd --user` unit and a Windows service/scheduled task, matching `launchd.rs`.
3. **Remove the agent extension on uninstall or opt-out** (theme 11): pair `install_into` with a remove path, local and remote.
4. **Keyboard copy mode** (theme 6).
5. **Updater download timeout and cancel** (theme 10).
6. **User-remappable shortcuts** (theme 5).

## Limits
- Read-only code review by four `mahoquot/claude-opus-5-5` explore workers, with citations re-opened by the lead; one worker conflict (theme 11) was resolved by reading `agent_extension.rs` directly.
- No performance, battery or memory measurement was taken; theme 3 is a code-structure verdict.
- Linux/Windows CJK font fallback, automatic SSH reattach after sleep, and Windows NSIS/MSI signing in release scripts are unverified.

