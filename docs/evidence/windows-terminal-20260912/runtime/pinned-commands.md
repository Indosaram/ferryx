# Pinned Windows runtime QA commands — task st_01a0958a

Pinned BEFORE execution. Any deviation is recorded in windows-runtime-red.md.

## Fixed identity

- Local repo: /Users/indo/code/project/orca-lite, branch main
- Source revision under test (committed only, foreign uncommitted edits EXCLUDED):
  fa429aacd7218146c8aa5445892ec616526de2f1 (verified via `git log -1 --format=%H` before bundling)
- Remote host: maho-win (ssh, default shell PowerShell 7.6.6, user desktop-1lapjmp\sook)
- Remote QA root (NEW, uniquely named): C:\Users\sook\ferryx-qa-rt-st01a0958a\
- Scheduled task: FerryxQA_st01a0958a (principal desktop-1lapjmp\sook, LogonType Interactive)
- CDP port for WebView console: 9223 (verified free before launch)

## Phase A — local bundle (committed main only)

```
git -C /Users/indo/code/project/orca-lite bundle create /tmp/ferryx-qa-st01a0958a.bundle main
git -C /Users/indo/code/project/orca-lite bundle verify /tmp/ferryx-qa-st01a0958a.bundle
scp /tmp/ferryx-qa-st01a0958a.bundle maho-win:C:/Users/sook/ferryx-qa-rt-st01a0958a/
```

Note: `git bundle` serializes committed refs only; the working-tree edits of other workers
(TabBar.tsx, TerminalSplitView.tsx, daemon/client.rs, tests) are excluded by construction.

## Phase B — remote isolated checkout

```
New-Item -ItemType Directory -Force C:\Users\sook\ferryx-qa-rt-st01a0958a\logs, ...\qa, ...\runtime, ...\session, ...\evidence
git clone C:\Users\sook\ferryx-qa-rt-st01a0958a\ferryx-qa-st01a0958a.bundle orca-lite
git -C orca-lite checkout main            # bundle HEAD ref quirk (memory: required)
git -C orca-lite rev-parse HEAD           # must print fa429aacd7218146c8aa5445892ec616526de2f1
git -C orca-lite config core.autocrlf false
git -C orca-lite config core.longpaths true
New-Item -ItemType Junction -Path orca-lite\src-tauri\vendor\ghostty `
  -Value C:\Users\sook\ferryx-winbuild\orca-lite\src-tauri\vendor\ghostty
```

Junction precondition (verified 2026-09-12 before pinning): existing ghostty checkout
`git rev-parse HEAD` == 6a508fd5e34c7e222c052a6d00bb3891ff3feace == EXPECTED_GHOSTTY_SHA in
src-tauri/native_terminal/build_ghostty.rs. Junction is read-only reuse; build writes only to
orca-lite\target (zig caches live in OUT_DIR per build_ghostty.rs:214-215).

```
bun install          (orca-lite root, provides @tauri-apps/cli for `bun tauri`)
bun install          (orca-lite\ui, provides vite/tsc/react tree)
```

## Phase C — interactive launch (exactly `bun tauri dev`, debug)

Scheduled task action: `C:\Windows\System32\cmd.exe /c C:\Users\sook\ferryx-qa-rt-st01a0958a\qa\launch.cmd`

qa\launch.cmd (pinned, full file in runtime/qa/launch.cmd):
```
set FERRYX_RUNTIME_DIR=C:\Users\sook\ferryx-qa-rt-st01a0958a\runtime
set FERRYX_SESSION_DIR=C:\Users\sook\ferryx-qa-rt-st01a0958a\session
set WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223
cd /d C:\Users\sook\ferryx-qa-rt-st01a0958a\orca-lite
bun tauri dev >> C:\Users\sook\ferryx-qa-rt-st01a0958a\logs\dev-stdout.log 2>&1
```

- Root command is exactly `bun tauri dev` (debug build via tauri dev). No direct exe launch,
  no cargo run, no alternate command.
- Isolation: FERRYX_RUNTIME_DIR/FERRYX_SESSION_DIR redirects daemon + session state to the QA
  tree (server.rs get_runtime_dir/session_dir_override honor both). The user's installed daemon
  (%LOCALAPPDATA%\Ferryx\runtime, port 53986) is never read or written.
- WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS opens the WebView2 CDP endpoint on 127.0.0.1:9223 for
  console capture (switch-debug JSONL sink is broken on Windows — hardcoded /tmp — per
  bounds-diagnosis.md section 4, so devtools console + stdout are the surviving channels).

## Phase D — evidence capture (all event-driven, bounded)

1. monitor-build.ps1 — FileSystemWatcher on logs\dev-stdout.log, exits on
   FERRYX_FRONTEND_READY / cargo Finished / error markers / 60 min cap. No sleep-polling.
2. cdp-console-capture.mjs (node v24, built-in WebSocket) — attaches to 127.0.0.1:9223 page
   target, Runtime.enable + Log.enable, appends every consoleAPICalled/exception/Log entry to
   evidence\cdp-console.jsonl. Exits on stop-file or 30 min cap.
3. capture-screen.ps1 — full virtual screen PNG via scheduled task (interactive session 1),
   saved to evidence\screen-<n>.png.
4. dump-hwnd-geometry.ps1 — enumerates parent/child HWNDs of the QA ferryx.exe (PID-filtered,
   installed app excluded), GetWindowRect + class + text, saves evidence\hwnd-geometry.txt.
5. send-echo.ps1 — ONLY if a terminal pane is actually visible: SetForegroundWindow on the QA
   app window, SendKeys `echo FERRYX_WIN_START_OK{ENTER}`, screenshot before/after.
6. stop-qa.ps1 — kills only processes whose command line contains ferryx-qa-rt-st01a0958a
   (taskkill /T on the task root, node CDP recorder, debug ferryx.exe), then unregisters
   FerryxQA_st01a0958a. Installed app PIDs 1756/17288/20196 and foreign tasks
   (Ferryx-WSLg-Dev, FerryxCapture0906) are excluded by construction.

## Phase E — fingerprint grep (from bounds-diagnosis.md section 4)

Case-sensitive strings over logs\dev-stdout.log and evidence\cdp-console.jsonl:
`initial bounds layout failed during attach` | `Invalid terminal dimensions: cols=` |
`Invalid value or parameter:` | `GPU adapter request failed:` | `GPU device request failed:` |
`Surface create error:` | `Failed to lazily create native terminal surface host during scheduled render` |
`Failed to render native terminal snapshot` | `Could not dispatch native terminal render` |
`Main thread stopped before native terminal render completed` | `Main Ferryx window is unavailable` |
`Requested value does not exist (NoValue)` | `panicked at` |
plus webview events `terminal.surface.bounds.error` / `.attach.error` with full structured suffix.
