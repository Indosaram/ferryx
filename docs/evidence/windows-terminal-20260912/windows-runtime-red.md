# Windows runtime evidence — st_01a0958a (QA root `C:\Users\sook\ferryx-qa-rt-st01a0958a`)

Status: IN PROGRESS (updated incrementally; final outcome at the bottom).
Worker: senpi task st_01a095a2 (child of 01a0955a). All times KST (UTC+9) as reported by the Windows box.

## 1. State at takeover (21:52 KST)

### Scheduled task
- `FerryxQA_st01a0958a`: State **Running**, LastRunTime 21:19:06, LastTaskResult 267009 (still running).
- Action: `cmd.exe /c C:\Users\sook\ferryx-qa-rt-st01a0958a\qa\launch.cmd` (v1, no WebView/profile isolation).

### QA process tree (owned by this QA task; full chain with exact PIDs)
| PID | PPID | Name | Created | Path / command |
|---|---|---|---|---|
| 872 | 2788 (svchost Schedule) | cmd.exe | 21:19:06 | `/c ...ferryx-qa-rt-st01a0958a\qa\launch.cmd` |
| 17280 | 872 | bun.exe | 21:19:06 | `bun tauri dev` |
| 11108 | 17280 | cmd.exe | 21:19:07 | `/S /C "bun scripts/dev-frontend.mjs"` |
| 8568 | 11108 | bun.exe | 21:19:07 | `bun scripts/dev-frontend.mjs` (vite dev) |
| 11136 | 8568 | esbuild.exe | 21:19:33 | `...ui\node_modules\@esbuild\win32-x64\esbuild.exe --service=0.25.12 --ping` |
| 12740 | 17280 | rustup.exe | 21:19:34 | `"cargo" run --no-default-features --features native-terminal --color always --` |
| 16992 | 12740 | cargo.exe | 21:19:34 | `cargo.exe run --no-default-features --features native-terminal --color always --` |
| 388 | 16992 | ferryx.exe | 21:25:45 | `"target\debug\ferryx.exe"` (QA debug app, **no window**) |
| 21216 | 388 | ferryx.exe | 21:25:46 | `...target\debug\ferryx.exe --daemon` (QA daemon, holds `runtime\daemon.lock`) |

All nine PIDs run in Session 1. Build cache: cargo `Finished 'dev' profile ... in 6m 10s` at 21:25:45.

### First-launch outcome (preserved verbatim)
`Running target\debug\ferryx.exe` →
```
2026-09-12T12:25:46.503715Z ERROR tauri_runtime_wry: failed to create webview: WebView2 error:
WindowsError(Error { code: HRESULT(0x8007139F), message: "그룹 또는 리소스가 요청된 작업을 실행할 올바른
상태에 있지 않습니다." })
```
Not a terminal-bounds failure; the app process stayed alive windowless. Preserved copies (made before any relaunch, log was quiescent since 21:25:46):
- Local: `docs/evidence/windows-terminal-20260912/runtime/artifacts/dev-stdout-first-attempt.log` (102837 B)
- Remote: `C:\Users\sook\ferryx-qa-rt-st01a0958a\logs\dev-stdout-first-attempt.log`

### QA runtime handles held by the failed instance
- `runtime\browser.port` = 57084, `runtime\daemon.port` = 57085, `runtime\daemon.lock` (locked by PID 21216), `runtime\handover_routes.json` = `{"routes": []}`.
- Port 9223 (CDP): **no listener** — webview never existed, no CDP endpoint to record from.

### User resources observed, NOT touched
- Installed app: ferryx.exe 17288 → 1756, 20196 (daemon, owns port 53986), msedgewebview2 19396 tree — `C:\Users\sook\AppData\Local\Ferryx\`.
- Remote helpers 2736/17196/11932/12204/21292/19412; user bun/node trees 15928→2096→19244, 15800→1056→20700, 20008→7568→20860, 10292→2360 (`bun tools/launch.js --concept return_to_sender`).
- `evidence\appdata-com.ferryx.app-snapshot-preqa` snapshot left in place; not restored, not used.

### Scoped script fixes applied (QA scripts only, no production files)
1. `launch-isolated.cmd`: leading `#` comment lines → `rem` (cmd-invalid `#` removed). Otherwise v2 as authored.
2. `monitor-build.ps1`: match decisions on ANSI-stripped text (raw `Finished` never matched through `\x1b[..m`); launcher liveness match widened to `qa\launch*.cmd`; added `failed to create webview` → `WEBVIEW_ERROR` decision.
3. `cdp-console-capture.mjs`: added bounded CDP connect retry (500 ms steps, 120 s cap, writes `.error` on failure) on top of the lead's `fs.watch` fix; remote copy was the stale pre-fix file — current file shipped.
4. New scoped helpers: `send-echo.ps1` (foreground-verified SendKeys into the QA window only, PostMessage WM_CHAR fallback to the QA thread's focused child; never sends while another app is foreground), `register-session-tasks.ps1` (FerryxShot/FerryxKey helpers), `register-task-isolated.ps1` (re-points the SAME task at `launch-isolated.cmd`).
5. `stop-qa.ps1` NOT executed (unsafe broad wildcard kill, per instruction).

## 2. Isolated relaunch (one attempt, via the existing InteractiveToken task)

_(appended as it happens)_

## 3. Outcome

_(to be filled at completion)_
