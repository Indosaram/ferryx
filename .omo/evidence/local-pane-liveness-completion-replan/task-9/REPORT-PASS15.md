# REPORT — PASS 15

## Scope and outcome

The dispatch set a gate: **confirm the Task Scheduler restart cleared the wedge with the trivial-task probe,
then** run the pass-13 measurements on `bc078f70`.

**Outcome: the wedge is NOT cleared and the measurement plan was NOT executed.** No receipt-case
discrimination, no native-vs-DOM reading, no in-run inventory delta, no `settledBy` cross-check.

**But this pass also CORRECTS the diagnosis I gave in pass 13/14 — and the correction changes what the fix is.**
The blocker is **not** a wedged Task Scheduler. The scheduler is healthy and spawns processes; what hangs is
**any process launched without an inherited console**. That means **the authorised service restart would
probably not have fixed it**, and the user should know that before spending effort on it.

## Revision

**`HEAD = bc078f70`** (`feat(qa): settle the pre-split pane binding from a measured daemon inventory delta`),
tree clean. **No measurement was taken this pass**, so no revision applies to a measurement. Pass-13 source
readings were on `562144b3`; the c5 capture on `757f8414`; the isolation A/B on `5c423880`.

---

## Step 1 — the gate probe: the wedge is NOT cleared

```
at = 2026-10-05T05:18:09.7617384+09:00
service.Status = Running   StartType = Automatic   ProcessId = 1264   <- same as pass 14

trivialTask.Status           = Running
trivialTask.LastResult       = 267009   (0x41301 = SCHED_S_TASK_RUNNING)
trivialTask.OutputFileExists = False

WEDGE_CLEARED = False
```

**Not cleared**, with the same values as pass 14. Per the dispatch's criterion I did not proceed.

## Step 2 — the restart: REFUSED (and, per the new evidence, probably not the right fix)

| method | result |
|---|---|
| `Restart-Service -Name Schedule -Force` | `ServiceCommandException`: *"Service 'Task Scheduler (Schedule)' cannot be stopped … Cannot open Schedule service on computer '.'."* |
| `Stop-Service -Name Schedule -Force` | same |
| `Start-Service -Name Schedule` | **OK — a no-op** against an already-running service; **not** a restart |
| `sc.exe stop Schedule` | **exit 5** — `[SC] OpenService FAILED 5: Access is denied.` |
| `net stop schedule` | **exit 2** — `System error 5 has occurred. Access is denied.` |
| WMI `Win32_Service.StopService` | failed (invalid parameter) |

```
AFTER: Status=Running  ProcessId=1264      <- unchanged
svchost 1264 start = 2026-10-03T10:33:22   <- up ~2 days; NO restart has happened
```

**No heavier action taken** — no reboot, no registry edit, no `svchost` kill.

### The DACL proves elevation is the missing capability

`sc.exe sdshow Schedule` (read succeeds, exit 0):

```
D:(A;;CCLCSWLORC;;;AU)(A;;CCLCSWRPDTLOCRRCWDWO;;;BA)(A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;SY)(A;;CCLCSWLORC;;;BU)
S:(AU;FA;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;WD)
```

| principal | granted | has `SERVICE_STOP` (`WP`)? |
|---|---|---|
| **`AU` Authenticated Users** (what our token gets) | `CC LC SW LO RC` | **NO** — read/interrogate only |
| **`BA` Administrators** | `CC LC SW RP DT LO CR RC WD WO` | **YES** |
| `SY` LocalSystem | full | yes |

So the service **is** stoppable by an **elevated** Administrators token, and our SSH session does not hold one.
Same root cause as pass 14's trap, now proven from the DACL: the token reports Administrators *membership* while
being *filtered*.

## Step 3 — THE CORRECTED ROOT CAUSE: it is not the scheduler

This is the substantive finding of the pass, and it **supersedes** the pass-13/14 framing.

### 3.1 The scheduler is healthy and DOES spawn processes

A `.bat` task produced, at the exact run time:

```
cmd.exe /c ""C:\Users\sook\ferryx-pane-completion\wedge-p15.bat""
   PPID = 1264  (svchost.exe -k netsvcs -p -s Schedule)
   start = 2026-10-05T05:18:10.0732560+09:00
```

**The Task Scheduler service created the process.** Its engine is working; the task was accepted, `Last Run
Time` advanced, and the child exists with the service as its parent.

### 3.2 What hangs is launching WITHOUT an inherited console

Four controlled launches from the same SSH session, same host, minutes apart:

| # | launch form | result |
|---|---|---|
| **A** | `& cmd.exe /c echo A_OK` — **inherits my console** | **WORKS — 23 ms** |
| **B** | `Start-Process cmd.exe -NoNewWindow -RedirectStandardOutput` | **WORKS — output `B_OK`** |
| **C** | `.NET Process`, `UseShellExecute=false`, **`CreateNoWindow=true`**, redirect stdout → `cmd.exe /c echo C_OK` | **HANGS** (>10 s, no output) |
| **D** | **same pattern, but the app binary**: `ferryx.exe --version` | **HANGS** (>10 s) |

**Case D is the decisive one: the app binary hangs under the same launch pattern.** So this is **not a
`cmd.exe` fault at all** — the failing ingredient is the **no-console launch**, and it affects any executable.

### 3.3 Why that blocks the runner

The Task Scheduler runs as a service and therefore always launches **without an inherited console**. So:

**every scheduled task on this host hangs → the runner's `schtasks` session-1 delegation hangs → the run
stalls right after `windows-interactive-relaunch` and never reaches `launch.binary`.**

That is the mechanism behind the stall I measured in pass 13, now explained rather than observed.

### 3.4 What is ruled out

| candidate | verdict |
|---|---|
| `cmd.exe` AutoRun | **not set** (`HKLM\SOFTWARE\Microsoft\Command Processor`, `WOW6432Node`); `HKCU` key absent |
| IFEO debugger on `cmd.exe` | **absent** (both `HKLM` and `WOW6432Node`) |
| `AppInit_DLLs` | **empty**, `LoadAppInit_DLLs = 0` (both views) |
| the service being stopped/stuck | **ruled out** — it is Running and it spawns (3.1) |
| a `cmd.exe`-specific fault | **ruled out** — the app binary hangs identically (case D) |

### 3.5 RETRACTION — an earlier claim of mine was wrong

I wrote in my own reasoning that `cmd.exe` had "been hanging since 2026-10-03", citing two long-lived
processes. **I checked their children and that claim is WRONG:**

```
PID 2348  cmd.exe /c ""C:\Users\sook\minio\run-minio.bat""   children = conhost.exe, minio.exe
PID 13824 cmd.exe /c "cargo test …"                            children = rustup.exe
```

**Both are legitimately long-running workloads** — a MinIO *server* and a *cargo test* — not hangs. **I retract
the 2-day-hang claim**; the only genuine hangs are my own trivial probes, which are unambiguous (a 12 s wait for
`echo HELLO > file`).

### 3.6 What is NOT established

**I did not determine why no-console launches hang *now*, when the same `schtasks` delegation demonstrably
worked earlier today** (the 04:23–04:26 runs launched the app, rendered its UI, and produced verdicts). Something
changed on this host between roughly 04:26 and 05:05, and **I am not asserting a cause for that.** Candidates I
did not test include console/desktop-heap exhaustion, a session/desktop state change, or a resource limit
reached by concurrent sessions' workloads.

## Step 4 — the wire cross-check (not blocked, so I ran it)

| wire element | my probe | `daemon-inventory.mjs` | agree? |
|---|---|---|---|
| handshake | `{"type":"handshake","version":5,"token":"<t>"}` | `{type:'handshake', version: DAEMON_PROTOCOL_VERSION, token: credential}` | **yes** |
| version | `5` | `DAEMON_PROTOCOL_VERSION` (=5) | **yes** |
| line terminator | `[char]10` | `'\n'` | **yes** |
| list request | `{"type":"listSessions"}` | `{type:'listSessions'}` | **yes** |
| response read | one line, `ReadLine()` | one buffered line, `readFrame()` | **yes** |
| success tags | `handshakeOk` / `listSessionsOk` | same | **yes** |
| payload | `.sessions` array | `listFrame.sessions` | **yes** |
| Windows endpoint | `<runtime>/daemon.port` + `daemon.token` | same | **yes** |
| POSIX endpoint | not implemented | `daemon.sock`, no token | client is a superset |
| bounding | connect `WaitOne(1200)`, read 1500 ms — **no total deadline** | connect 1.5 s, read 2 s, **total 3 s** | client is stricter |

**No disagreement in framing, tag spelling or token handling.** The two differences both favour the client and
are not disagreements. **My probe remains a valid independent cross-check** — which is the use the dispatch
proposed for it, once a run is possible.

## What remains blocked

All four items need a run, which needs the no-console launch to work:
the four-case receipt discrimination (case 4 already ruled out by source), the native-vs-DOM state, the in-run
inventory delta with the poller fix, and **which source settled the binding plus whether it matches my probe**.

## What would clear it — and why the authorised restart probably would not

**The authorised restart is the wrong lever on this evidence.** The service is Running and its engine spawns;
the failing ingredient is the *no-console launch*, and restarting the service does not change how the scheduler
launches children. **I would not expect a service restart to clear it**, and I am saying so before it is spent.

On the evidence, in increasing weight:

1. **A machine restart** — the cheapest action that resets whatever console/session state broke, and the only one
   I would now expect to work. **Heavier than what was authorised, so it needs a fresh decision.**
2. **Investigate the host's console/session state directly** (e.g. desktop-heap or handle pressure, session
   state changes around 04:26–05:05) — also machine-level, also a fresh decision.
3. A service restart, if the user prefers to try the authorised lever first — **cheap, but I predict it fails**,
   and it would still leave the no-console launch unproven.

**Verification after any of them is unchanged**: the trivial task must reach a **terminal** state with a
**non-`0x41301`** result **and** produce its output file. **And I would now add a second gate**, because 3.2
shows the real ingredient: **a `.NET Process` with `CreateNoWindow=true` must complete** — that is the
one-minute test that predicts whether a run can start, and it is the test I will use first next time.

## Teardown

| Resource | Action | Receipt |
|---|---|---|
| my probe tasks (`ferryx-p15-probe`, `ferryx-p15-b`, `ferryx-p15-sys`, `ferryx-p15-node`, `ferryx-p15-xml`) | deleted by name | `ownTasksLeft=0` |
| **my six hung `cmd.exe` probes** (37916, 68240, 5420, 62652, 7716, 17696) | killed by **exact PID**, each **identity-checked** against `ferryx-pane-completion\wedge*` in its command line | `IDENTITY_OK` ×6 |
| my stray node probe (17752) | killed by exact PID, identity = `ferryx-pane-completion\probe.js` | `IDENTITY_OK` |
| **FOREIGN processes** — `run-minio.bat` (2348, children `conhost`+`minio.exe`), `cargo test` (13824, child `rustup.exe`), `bun run --cwd ui test` (64732, child `bun.exe` 33088) | **REPORTED, NOT KILLED** — other sessions' live work | listed above |
| my probe `.bat`/`.ps1`/output files | removed from my own scratch dir | 13 files removed |
| **the Schedule service** | **not stopped, not started, not modified** — every attempt refused before any state change | `ProcessId=1264` |
| the host profile | untouched | `sha=D07A1698…` |
| the **candidate tree** | never edited by me; clean | `HEAD = bc078f70`, `git status --porcelain` empty |
| final state | — | `REMAINING_MINE=0`, `ownTasksLeft=0`, `PORT_5173=FREE`, `FREE_GB=19.85` |

**The identity discipline earned its keep again**: the kill loop required `ferryx-pane-completion\wedge*` in
the command line, which is why the three foreign `cmd.exe` were left alone despite matching a looser pattern.

## Carry-forward traps (all twelve)

1. **A Windows scheduled task does not inherit the interactive PATH** — bare `bun`/`node` die silently.
2. **`StreamWriter.WriteLine` emits `\r\n`; the daemon's control wire wants `\n`** — the handshake blocks
   forever otherwise. Use `$w.NewLine = [char]10`.
3. **`TcpClient.Connect` has no timeout** — bound every blocking call in a poll loop.
4. **`Start-Process -ArgumentList @()` throws** — omit the parameter for a no-argument child.
5. **`serve-dist.mjs` takes the DIST dir** — passing `ui/` serves Vite's dev shell and the app renders blank
   while the HTTP check still passes (200 + `id="root"`). Pass `ui/dist`.
6. **A scheduled task's default cwd is `%SystemRoot%\System32`**, not where you launched from.
7. **A stale `.done` marker fires monitors immediately** — clear it at script start.
8. **The runner's typed `FRONTEND_PORT_OCCUPIED` refusal is real and fail-closed.**
9. **The harness gives the app no stderr sink** — fixed in the candidate by `562144b3`.
10. **A wedged task subsystem blocks every native run** (the runner requires session-1 delegation) — **check a
    trivial task's output before believing a stalled run is a code defect.**
11. **`isAdmin=True` from `WindowsPrincipal.IsInRole` is a false positive over SSH** — the token lists
    Administrators while `OpenService(SERVICE_STOP)` is denied. **Prove access by the operation, not the group
    claim.**
12. **The service DACL, not the group claim, decides a restart** — `sc.exe sdshow <name>` is readable without
    elevation and turns "Access denied" into a precise statement of the missing capability. And
    **`Start-Service` succeeding after a failed stop is a no-op, not a restart** — check the host process's PID
    and start time.

**New trap 13 (this pass):** **the hang is "launched without an inherited console", not "`cmd.exe` is broken".**
Isolate a process-launch fault with the four-form matrix — `& exe` (inherits console), `Start-Process
-NoNewWindow`, `.NET Process` with `CreateNoWindow=true`, and **the same pattern on a second, unrelated
binary**. **Case D is what distinguishes "this tool is broken" from "this launch form is broken"**, and it
changed the conclusion here. Related: **before calling a long-lived process a hang, check its children** — a
`cmd.exe` whose child is a server (`minio.exe`) or a build tool (`rustup.exe`) is working, not stuck.
