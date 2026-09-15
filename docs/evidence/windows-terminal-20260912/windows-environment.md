# Windows interactive QA environment — maho-win (observed 2026-09-12)

Lane: environment diagnosis (read-only). RED Windows runtime work now belongs to `dag_0180c8f2-ce1f-4aef-b114-1483c74f995f` on an isolated checkout. This file is the only write of this lane; no app/daemon was launched, killed, or mutated remotely; no scheduled task was registered; the existing checkout was not modified. All facts below are observed via ssh read-only probes or local repo reads unless explicitly marked [HYPOTHESIS].

## Lead review: execution corrections

The commands below are an unexecuted proposal, not an approved runbook or current success evidence.
The runtime lane must pin its corrected executable commands in `windows-runtime-red.md` before use:

- Skip Phase A's direct binary/daemon launch. All desktop execution is exactly `bun tauri dev`.
- Replace `Start-Sleep` and turn-level readiness polling with event/state subscriptions and bounded monitor commands.
- Use real Node for Vitest: `node node_modules/vitest/vitest.mjs run --maxWorkers=1` with the pinned target files. Historical test counts below are not this session's Windows baseline.
- Set BOTH `FERRYX_SESSION_DIR` and `FERRYX_RUNTIME_DIR` to unique QA paths. Source inspection confirms that the session override alone does not isolate daemon endpoints. Also isolate QA application settings.
- Cleanup must use the exact inventory of owned PIDs, task names, paths and junctions. The broad command-line wildcard example below must not be executed as-is.
- Shell production code has since been repaired with RED/GREEN UI tests. This report's initial missing-menu observations describe the earlier baseline; actual Windows shell spawning is still unverified.
- The existing checkout and installed application remain read-only. Use the committed baseline selected by the runtime lane, not the stale revision or hypothetical fix branch shown below.

## 1. Sessions and SSH facts (observed)

| Fact | Value | Probe |
|---|---|---|
| ssh identity | `desktop-1lapjmp\sook`, RC=0 | `ssh maho-win whoami` |
| Session list | `services` = session **0** (Disc); `console` = session **1**, user sook, **Active** (logon 2026-09-10 02:33) | `qwinsta`, `quser` |
| SSH lands in | session 0 (services) — never proves GUI | quser/session 0 processes |
| Default ssh shell | **PowerShell 7.6.6** (`c:\program files\powershell\7\pwsh.exe`), NOT cmd | `$PSVersionTable.PSVersion` = 7.6.6 in login shell; `ver` failed with PowerShell parser error (Korean locale) |
| Windows PowerShell 5.1 | present (`powershell.exe`) | used by proven capture script |

DEVIATION: memory `ferryx-windows-verification.md` says "Default ssh shell is cmd" — stale. New remote scripts must assume pwsh semantics; cmd lessons remain valid only inside `.cmd` files run via `cmd /c` (which the proven task pattern uses).

Probe mechanics (observed, cost me two failures):
- Single exe invocations work: `ssh maho-win tasklist`, `ssh maho-win netstat -ano`, `ssh maho-win "schtasks /query /tn X /xml"`.
- Nested `powershell -Command "$..."` double-interpolates `$` (the pwsh login shell consumes the inner quotes). Send PowerShell code directly with local single quotes; never wrap `$` in inner double quotes.

## 2. Host resources (observed)

- OS: Microsoft Windows NT 10.0.26200.0 (Windows 11).
- GPU: NVIDIA GeForce RTX 4060 Ti, current mode **3840x1600**; a **Parsec Virtual Display Adapter** is also installed (which display is primary in session 1 is unmeasured — [HYPOTHESIS] the capture script's in-session `PrimaryScreen.Bounds` resolves this at runtime).
- Disk: C: free **192.98 GB** — ample for an isolated checkout + cold build.
- Session-0 `[System.Windows.Forms.SystemInformation]::VirtualScreen` reported **1024x768** — that is session 0, not the desktop; do not use it for coordinate math.

## 3. Toolchain (observed)

- rustc 1.97.0 / cargo 1.97.0 (`C:\Users\sook\.cargo\bin\cargo.exe`)
- bun 1.4.0 (`C:\Users\sook\.bun\bin\bun.exe`), node v24.19.0, git 2.55.0.windows.2
- No cargo/rustc/vite running; **TCP 5173 has no listener** (`netstat -ano` LISTENING grep empty) → dev port free. `ui/vite.config.ts` uses `strictPort: true` on 127.0.0.1:5173, so this is a hard prerequisite that currently holds.

## 4. Live processes to preserve (kill-safety contract)

Observed via `tasklist` + `Get-Process -Id ... | Select Path`:

| Process | PIDs | Path / session | Rule |
|---|---|---|---|
| User's installed Ferryx GUI | 17288, 1756, 20196 | `C:\Users\sook\AppData\Local\Ferryx\ferryx.exe`, console session 1 | **NEVER kill** |
| User's daemon runtime | (port file) | `C:\Users\sook\AppData\Local\Ferryx\runtime\daemon.port` = **53986** | Never read-modify; QA isolates via `FERRYX_SESSION_DIR` |
| User's relays/agents | 6x ferryx-remote-helper.exe, 8x bun.exe (two ~1 GB), 3x node.exe | services session 0 | **NEVER kill** |

Cleanup (§8) must filter strictly on QA paths (`C:\Users\sook\ferryx-winbuild\qa-20260912*`), never on process names.

## 5. Existing checkout (observed) and isolated-checkout plan

`C:\Users\sook\ferryx-winbuild\orca-lite` — branch `ssh-remote-hosts`, HEAD `e2a1906` "chore(release): stamp v2026.09.02.1", **dirty with foreign work (read-only for everyone)**:

- `M scripts/build-msix.ps1`, `M src-tauri/windows/msix/AppxManifest.xml` — foreign edits, preserve.
- `T src-tauri/vendor/ghostty`; `D src-tauri/target`, `D ui/dist`, `D ui/node_modules` — explained (observed via `git ls-files -s` + `git cat-file -p`): the index tracks **mode-120000 symlinks whose blobs contain macOS absolute paths** (`/Users/indo/code/project/orca-lite/...`). The worktree ghostty is a real directory with full ghostty source (`include/`, `src/`, `dist/`, `example/`, `macos/`, ...; `LinkType` empty → not a reparse point). This state is foreign work — do NOT "repair" it.
- Shell-selection backend IS present here: `git log --oneline -- src-tauri/src/terminal/shell.rs` lists `296e9a2 feat(terminal): selectable default shell (PowerShell/cmd/WSL/custom)`.
  - CORRECTION: my earlier probe printed `SHELL-SELECTION-296e9a2=MISSING` — false reading caused by my own probe bug (PowerShell `if (git ...)` tests stdout, not `$LASTEXITCODE`). The path-log above is authoritative.
- `ui/node_modules` exists on disk (`Test-Path` = True) despite `D` status.
- origin = `C:/Users/sook/ferryx-winbuild/ferryx-win.bundle`, which **no longer exists** (`Test-Path` = False) → there is no fetch source on the host; sync must be a fresh bundle + scp.
- No existing debug exe: `Test-Path src-tauri\target\debug\ferryx.exe` = False; no `*.exe` under root `target\debug` → first QA build is cold.

### Isolated QA checkout plan (execution owned by dag_0180c8f2)

1. mac: `git -C /Users/indo/code/project/orca-lite bundle create /tmp/ferryx-qa-win-20260912.bundle <qa-branch>` (branch carrying the fixes; local main is at `f4ab00e2` on top of `99e0450d`; mac tree is dirty with unrelated `site/*` foreign work — bundle a branch, not the dirty tree).
2. mac: `scp /tmp/ferryx-qa-win-20260912.bundle maho-win:C:/Users/sook/ferryx-winbuild/` (scp fetch was proven previously; push direction re-verify live).
3. win: `git clone C:\Users\sook\ferryx-winbuild\ferryx-qa-win-20260912.bundle C:\Users\sook\ferryx-winbuild\qa-20260912`
4. win: `git -C C:\Users\sook\ferryx-winbuild\qa-20260912 checkout <qa-branch>` (bundle HEAD ref quirk, per memory).
5. ghostty: junction from the existing real ghostty dir (fast, proven trick) — `cmd /c mklink /J C:\Users\sook\ferryx-winbuild\qa-20260912\src-tauri\vendor\ghostty C:\Users\sook\ferryx-winbuild\orca-lite\src-tauri\vendor\ghostty`. If the fresh gitlink path exists as an empty dir, `rmdir` it first; if submodule machinery fights back, fall back to `git submodule update --init --depth 1` (966 MB). Slow alternative only.
6. deps: `bun install` at QA root (root devDependency `@tauri-apps/cli`) **and** `bun install --cwd ui` — REQUIRED because `scripts/dev-frontend.mjs:7` imports `../ui/node_modules/vite/dist/node/index.js` and its `startFrontend` first runs `bun run --cwd ui build` (beforeDevCommand = `bun scripts/dev-frontend.mjs`, `src-tauri/tauri.conf.json:7`). [HYPOTHESIS to verify live: whether root `bun.lock` covers ui; ui/package.json is a separate package named "ui".]
7. Target: fresh `src-tauri\target` in the QA clone by default. Reusing an existing target cache is allowed ONLY if no build owns it — re-verify ownership immediately before (no cargo/rustc/build processes; `Get-CimInstance Win32_Process` path-filtered) and never while any build runs. Observed baseline: no usable debug exe at either target path (`src-tauri\target\debug\ferryx.exe` and root `target\debug\*.exe` both absent), and in the existing checkout `src-tauri/target` is a tracked-but-absent symlink entry — so expect a cold build.

## 6. GUI access technique (proven pattern, captured verbatim from the host)

`MahoQACapture` task XML (read via `schtasks /query /tn "MahoQACapture" /xml`):
- Principal: `UserId S-1-5-21-3510490687-4041447508-1362901398-1002` (sook), `LogonType InteractiveToken` — no password needed; runs in console session 1.
- Action: `cmd /c C:\Users\sook\qa-capture.cmd`; TimeTrigger `2026-09-11T23:59:00`.

`C:\Users\sook\qa-capture.cmd` exists (Test-Path True), content captured verbatim (this is the proven screenshot recipe):

```
@echo off
powershell -NoProfile -Command "Add-Type -AssemblyName System.Windows.Forms,System.Drawing; $b=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds; $bmp=New-Object System.Drawing.Bitmap($b.Width,$b.Height); $g=[System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($b.Location,[System.Drawing.Point]::Empty,$b.Size); $bmp.Save('C:\Users\sook\qa-console.png'); $g.Dispose(); $bmp.Dispose()"
echo DONE >> C:\Users\sook\qa-console.done
```

Pre-existing leftover tasks observed Ready (do NOT modify, not ours): `Ferryx-WSLg-Dev`, `FerryxCapture0906`, `MahoQACapture`, `MahoQARun3` (runs `C:\Users\sook\win-maho-run2.cmd`). Also present: `C:\Users\sook\ferryx-permqa-*.cmd`, many `*.ps1` QA helpers — evidence of the established file-based QA style.

GUI-automation tool assessment (observed): no nircmd/AutoHotkey needed; PowerShell 5.1/7 + .NET WinForms (`CopyFromScreen`, `SendKeys`) + user32 P/Invoke (`SetCursorPos`, `mouse_event`, `SetForegroundWindow`, `MoveWindow`, `GetWindowRect`) cover capture, click, typing, and resize. Everything runs from InteractiveToken scheduled tasks in session 1.

## 7. Literal QA invocation plan (proposed; RED execution belongs to dag_0180c8f2)

### Phase A — headless sanity (no GUI, optional pre-check)

In the QA clone (after §5 steps 5-6):
```
set FERRYX_SESSION_DIR=C:\Users\sook\ferryx-winbuild\qa-20260912-session
cargo build --manifest-path src-tauri/Cargo.toml --bin ferryx
src-tauri\target\debug\ferryx.exe --daemon            (background, log captured)
bun script/qa/win-daemon-e2e.mjs C:\Users\sook\ferryx-winbuild\qa-20260912
```
PASS = `handshakeOk && errorProbeOk && spawnOk && writeOk && outputMarkerOk && cwdOk` (script/qa/win-daemon-e2e.mjs header; note its default repoRoot is the OLD checkout — always pass the QA clone path). Multi-step remote logic goes in a `.cmd` file with explicit step RCs (win-e2e3.cmd pattern; `%ERRORLEVEL%` early expansion and `&` chaining produce fake signals). Cleanup: taskkill the QA-path daemon only (§8). Never point integration tests (`cargo test --all-targets`, `daemon_persistence_contract`) at the shared host — see the Rust bullet under RED/GREEN; build targets (`cargo build --bin ferryx`) are safe.

### Phase B — interactive desktop QA via scheduled task

Launch (register + start; new unique names; trigger optional for manual start):
```powershell
$action    = New-ScheduledTaskAction -Execute "cmd.exe" -Argument '/c C:\Users\sook\ferryx-winbuild\qa-20260912\qa-run.cmd'
$principal = New-ScheduledTaskPrincipal -UserId sook -LogonType Interactive
$settings  = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Hours 2)
Register-ScheduledTask -TaskName "FerryxQADev20260912" -Action $action -Principal $principal -Settings $settings -Force
Start-ScheduledTask -TaskName "FerryxQADev20260912"
```

`qa-run.cmd` (new file inside the QA clone; desktop launch is EXACTLY `bun tauri dev`, debug only):
```
@echo off
cd /d C:\Users\sook\ferryx-winbuild\qa-20260912
set FERRYX_SESSION_DIR=C:\Users\sook\ferryx-winbuild\qa-20260912-session
bun tauri dev > C:\Users\sook\ferryx-winbuild\qa-20260912\qa-tauri-dev.log 2>&1
echo TAURI-DEV-EXIT=%ERRORLEVEL% >> C:\Users\sook\ferryx-winbuild\qa-20260912\qa-tauri-dev.log
```

Ready check (poll, no fixed sleeps): `Get-Process ferryx | Where-Object { $_.Path -like 'C:\Users\sook\ferryx-winbuild\qa-20260912*' } | Select-Object Id,MainWindowHandle,MainWindowTitle` + `Get-Content ...\qa-tauri-dev.log -Tail 5`. The QA window is identified by PID→`MainWindowHandle`, never by title alone (user's window is also titled "Ferryx"; tauri.conf.json sets title "Ferryx", label "main", 1280x850).

Screenshot: register `FerryxQACapture20260912` with the proven qa-capture.cmd recipe writing to `C:\Users\sook\ferryx-winbuild\qa-20260912\qa-shot-N.png` + `.done` sentinel; `Start-ScheduledTask`, poll for `.done`, fetch:
`scp maho-win:C:/Users/sook/ferryx-winbuild/qa-20260912/qa-shot-N.png docs/evidence/windows-terminal-20260912/`
Recommendation [HYPOTHESIS]: add `[Win32]::SetProcessDPIAware()` to new capture/input scripts so PNG pixels and click coordinates are consistently physical pixels on the 3840x1600 display (the proven script did not set it).

Input automation `FerryxQAInput20260912` task running `qa-input.ps1` (new file in QA clone), full literal skeleton:
```powershell
param([int]$X, [int]$Y, [string]$Text)
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Win32 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int X, int Y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, uint dx, uint dy, uint dwData, UIntPtr dwExtraInfo);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr hWnd, int X, int Y, int nWidth, int nHeight, bool bRepaint);
}
"@
[Win32]::SetProcessDPIAware() | Out-Null
$qa = Get-Process ferryx | Where-Object { $_.Path -like 'C:\Users\sook\ferryx-winbuild\qa-20260912*' } | Select-Object -First 1
$h = $qa.MainWindowHandle
[Win32]::SetForegroundWindow($h) | Out-Null        # fallback: the click itself grants focus
Start-Sleep -Milliseconds 400
[Win32]::SetCursorPos($X, $Y) | Out-Null
[Win32]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)   # LEFTDOWN
[Win32]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)   # LEFTUP
Start-Sleep -Milliseconds 400
if ($Text) { [System.Windows.Forms.SendKeys]::SendWait($Text) }
```
Click/type coordinates are derived by analyzing the fetched screenshot PNG (physical pixels) — no blind coordinates. Resize variant: `[Win32]::MoveWindow($h, 40, 40, 2560, 1300, $true)` — moves only the QA window, non-destructive.

### Scenarios (markers per docs/evidence/windows-terminal-20260912/PLAN.md)

1. **Shell choice**: click tab-bar `+`, choose Command Prompt, enter `echo FERRYX_WIN_SHELL_OK`. PASS = real selectable shell entries + observed output in screenshot.
   RED fact (code-observed): `rg "Command Prompt"` across `ui/src` returns EXACTLY ONE hit — `ui/src/components/settings/TerminalSection.tsx:241` (Settings→Terminal selector). **No New-Tab shell picker exists in ui/src**; the tab-bar "+ → choose shell" surface does not exist yet, so its absence on the first screenshot is the RED evidence. [HYPOTHESIS] the fix adds the picker to the new-tab flow and forwards the choice through `DaemonRequest::Spawn.shell` (back-compat field, per memory ferryx-windows-verification.md).
2. **Startup**: launch exactly `bun tauri dev` in the interactive session; enter `echo FERRYX_WIN_START_OK`. PASS = visible native terminal + observed output + NO "Failed to update native terminal bounds".
   Seam (code-observed): `ui/src/components/NativeTerminalPane.tsx:1827-1828` sets that error when `cmd_native_terminal_set_bounds` fails; existing UI test asserting the message: `ui/src/components/NativeTerminalPane.test.tsx:418`.
3. **Resize**: `MoveWindow` on the QA window, then enter `echo FERRYX_WIN_RESIZE_OK`. PASS = valid viewport geometry + unobscured output.

### RED/GREEN commands (literal)

- UI: `cd ui; bunx vitest run src/components/NativeTerminalPane.test.tsx` — canonical full gate is `bun run --cwd ui test` (vitest; NEVER bare `bun test`, per memory ferryx-qa-runner-and-port-contract.md).
- Vitest runtime (VERIFIED constraint, binding): on maho-win vitest must execute with real Node at `C:\Users\sook\node\node.exe`; a Bun fallback silently breaks `vi.mock`. Pin the real Node ahead of any Bun shim before running the gate (e.g. `$env:PATH = "C:\Users\sook\node;$env:PATH"`), then confirm inside that environment with `node --version`. (Node v24.19.0 was observed on PATH this session; its PATH location was not verified.)
- Rust: `cargo test --manifest-path src-tauri/Cargo.toml --lib` ONLY. NEVER `cargo test --all-targets` and never the `daemon_persistence_contract` integration test on maho-win — verified hazard: it may shut down the LIVE shared daemon (which owns the user's PTYs). If an integration target is ever unavoidable, it must run against the isolated `FERRYX_SESSION_DIR` daemon only, with the target explicitly inspected first. Windows baseline 280 pass / 28 fail identical to pre-fix f157191 (do not chase as regressions).
- Desktop invocation stays literally `bun tauri dev` even though the root package.json `tauri` script expands to `cargo tauri` (observed in root package.json) — do not substitute `cargo tauri dev` or the bare debug exe.
- Readiness is event-based everywhere: process appearance (`Get-Process` QA-path), log sentinels (`TAURI-DEV-EXIT`), and `.done` sentinels — no fixed-sleep readiness in QA scripts or tests; tests must await the exact event/state under test, never time-luck.
- Keep build.rs comctl32-v6 manifest embedding (unconditional `cargo:rustc-link-arg=/MANIFEST:EMBED`) — any new Windows test gate dies at load without it (STATUS_ENTRYPOINT_NOT_FOUND via muda/TaskDialogIndirect).
- All gates honor `set -o pipefail` locally; over ssh, pwsh normalizes exit codes — put multi-step logic in `.cmd`/`.ps1` files with explicit RCs.

## 8. Cleanup scheme (literal)

```powershell
# QA-owned processes ONLY — strict path/commandline filter; never by process name
Get-CimInstance Win32_Process | Where-Object {
  ($_.ExecutablePath -like 'C:\Users\sook\ferryx-winbuild\qa-20260912*') -or
  ($_.CommandLine -like '*ferryx-winbuild\qa-20260912*')
} | ForEach-Object { taskkill /PID $_.ProcessId /T /F }
Unregister-ScheduledTask -TaskName "FerryxQADev20260912"     -Confirm:$false
Unregister-ScheduledTask -TaskName "FerryxQACapture20260912" -Confirm:$false
Unregister-ScheduledTask -TaskName "FerryxQAInput20260912"   -Confirm:$false
Remove-Item C:\Users\sook\ferryx-winbuild\qa-20260912-session -Recurse -Force
```
- NEVER touch: user's `%LOCALAPPDATA%\Ferryx` PIDs 17288/1756/20196, daemon.port 53986, or any Services-session bun/node/ferryx-remote-helper.
- Leave pre-existing tasks (`MahoQACapture`, `MahoQARun3`, `FerryxCapture0906`, `Ferryx-WSLg-Dev`) and `C:\Users\sook\qa-capture.cmd` untouched.
- QA clone removal only after the lead confirms; `ferryx-winbuild\orca-lite` stays untouched (foreign dirty work preserved).

## 9. Session-dir / dev-server facts (code-observed, file:line)

- `FERRYX_SESSION_DIR` controls session-state storage, not the daemon endpoint. `get_runtime_dir()` in `src-tauri/src/daemon/server.rs:140-154` reads `FERRYX_RUNTIME_DIR`; without it, Windows uses `%LOCALAPPDATA%\Ferryx\runtime-dev` for debug. `get_socket_path()` at lines 162-164 appends `daemon.port`, and `DaemonClient::new()` uses that path (`client.rs:342`). The corrected launch must set a unique `FERRYX_RUNTIME_DIR` as well as `FERRYX_SESSION_DIR`; the original single-override recipe above is incomplete.
- `src-tauri/tauri.conf.json`: beforeDevCommand `bun scripts/dev-frontend.mjs` (line 7), devUrl `http://127.0.0.1:5173` (line 8), window title "Ferryx", label "main", 1280x850 (lines ~15-19).
- `ui/vite.config.ts:20-26`: strictPort 5173 on 127.0.0.1.
- Root `package.json` scripts (observed): `"dev": "cargo tauri dev"`, `"tauri": "cargo tauri"` — but the desktop launch directive remains the literal `bun tauri dev`; do not substitute.
- `scripts/dev-frontend.mjs:6-16`: runs `bun run --cwd ui build` FIRST (a build gate inside dev), then serves vite from `ui/node_modules` — hence the mandatory ui deps step and a slower first launch.
- Tab bar anchor for local (non-GUI) UI tests: `ui/src/components/TabBar.tsx:329` `data-testid="tab-strip"`.

## 10. Unknowns requiring the live QA lane (dag_0180c8f2)

1. Real desktop `VirtualScreen`/primary display in session 1 (self-measured by the capture script; Parsec adapter may be primary).
2. scp push direction for the bundle (fetch proven previously; re-verify).
3. Whether root `bun install` alone covers ui deps or `bun install --cwd ui` is required (dev-frontend.mjs imports `ui/node_modules/vite` directly).
4. Cold-build duration and the AV heuristic that previously made `target\debug\ferryx.exe` vanish minutes after link (if it vanishes mid-QA, rebuild; the rename workaround breaks `tauri dev` exe management).
5. Windows location of the switchDebug trace (macOS: `/tmp/ferryx-switch-debug.jsonl`; unverified on Windows).
6. Exact GUI element coordinates/testids for the new-tab "+" button and picker (only `tab-strip` testid captured; picker may not exist — scenario 1 RED).
7. Whether the user's console session stays unlocked/visible during QA (Active at probe; if locked, CopyFromScreen captures the lock screen — require the user present/awake, or a screen present via Parsec).
8. `mklink /J` behavior over a fresh submodule gitlink path (rmdir-then-link if needed).
9. Whether a QA daemon child (if spawned detached rather than in-process) is caught by the §8 ExecutablePath filter (it is, by path; verify live).
10. SetForegroundWindow from a scheduled task may be denied by foreground-lock — click-first fallback is in the script; verify live.

## 11. Constraint compliance

- Probes executed (all read-only): whoami, qwinsta, quser, rustc/cargo/bun/node/git versions, Get-Command paths, tasklist, netstat, schtasks query (+2 task XMLs), git status/log/ls-files/ls-tree/cat-file/ls-tree on the remote checkout, Test-Path/Get-ChildItem/Get-Content reads, Get-CimInstance Win32_VideoController, pwsh `$PSVersionTable`, session-0 VirtualScreen probe.
- NOT done (by scope): no app/daemon launch or kill, no scheduled-task registration, no remote file writes, no checkout mutation, no bundle/scp, no commits, no release builds, no GUI automation. RED GUI execution deferred to dag_0180c8f2-ce1f-4aef-b114-1483c74f995f.
- Only local write: this report (+ PLAN.md added by the lead).
