# REPORT — PASS 14

## Scope and outcome

The dispatch authorised a **Task Scheduler restart on maho-win** with a strict order: record the before-state,
restart, verify the wedge cleared with the same probe, and **only if it clears** continue the pass-13
measurement plan at `562144b3`.

**Outcome: the restart was REFUSED and the wedge PERSISTS. I stopped there, as instructed, and did not
escalate.** Steps 4 and 5 of the dispatch were therefore **not executed** — no measurement run was taken, and
nothing is claimed about the receipt cases or the inventory delta.

## Revision

No measurement was taken this pass, so no revision applies to a measurement. The staged tree remains
**`562144b3`** (the app-stdio sink) with my pass-13 probe injected; `HEAD` is `562144b3` and the candidate
tree is clean. **This pass produced host-state evidence only.**

---

## Step 1 — before-state (recorded, as asked)

```
at=2026-10-05T05:14:54.6565384+09:00

service.Name      = Schedule
service.Status    = Running
service.StartType = Automatic
service.State     = Running
service.StartMode = Auto
service.ProcessId = 1264
service.PathName  = C:\Windows\system32\svchost.exe -k netsvcs -p

trivialTask.Status           = Running
trivialTask.LastResult       = 267009
trivialTask.LastResultHex    = 0x41301
trivialTask.OutputFileExists = False

ownTasksLeft   = 0        <- clean, as expected
totalTaskCount = 417
```

**`0x41301` is `SCHED_S_TASK_RUNNING`** — the service reports the task as *currently running* while it never
executes and never produces its output file. That is the wedge, reproduced for the third time with the same
values.

## Step 2 — the restart attempt: REFUSED

```
elevation: user=DESKTOP-1LAPJMP\sook   isAdmin=True   integrity=(empty)

Restart-Service -Name Schedule -Force
  -> FAILED: ServiceCommandException ::
     Service 'Task Scheduler (Schedule)' cannot be stopped due to the following error:
     Cannot open Schedule service on computer '.'.

Stop-Service -Name Schedule -Force          (fallback)
  -> FAILED: same message
Start-Service -Name Schedule                (fallback)
  -> OK        <- no-op: the service was already running

AFTER: Status=Running  StartType=Automatic  State=Running  ProcessId=1264
```

**The service was not restarted: `ProcessId` is still `1264`, identical to the before-state.** `Start-Service`
returning OK is not evidence of a restart — it is a no-op against an already-running service, and I am not
counting it as progress.

### Why it was refused — and the trap in the elevation check

**`isAdmin=True` is a FALSE POSITIVE and must not be read as "we had the rights".** The identity reports
membership in the Administrators group, but the SSH session's token is **not elevated** — the practical
consequence is exactly what the error says: **`OpenService` with `SERVICE_STOP` is denied.** `sc.exe query
Schedule` **succeeds** (exit 0, `STATE : 4 RUNNING`, `(STOPPABLE, NOT_PAUSABLE, ACCEPTS_SHUTDOWN)`), which
confirms the service is readable over SCM while the *stop* access is refused — i.e. the block is **access**, not
the service being un-stoppable in principle.

**So this requires elevation I cannot obtain non-interactively over SSH**, which is precisely the case the
dispatch said to stop on. **I took no heavier action** — no reboot, no registry edit, no `svchost` kill.

## Step 3 — the wedge is NOT cleared

Same probe, same task shape, after the refused restart:

```
trivialTask.Status           = Running
trivialTask.LastResult       = 267009   (0x41301)
trivialTask.OutputFileExists = False
service.ProcessId            = 1264     (unchanged from before)
```

**Stated plainly: the wedge is NOT cleared.** A trivial task still reports `Running` with `0x41301` and still
produces no output file. Per the dispatch's own criterion, a service that reports Running while a trivial task
still hangs **is not cleared** — so I did not proceed.

## Steps 4 and 5 — NOT executed

| step | status |
|---|---|
| 4. stage `562144b3`, serve the frontend, run the pane step, take the staged probe measurements | **NOT RUN** — gated on step 3, which failed. No receipt-case discrimination, no native-vs-DOM reading, no inventory delta. |
| 4. confirm the app-stdio sink reaches artifacts without a staged patch | **NOT CONFIRMED** — that confirmation requires a run, and the runner needs session-1 delegation, which the wedge blocks. I make no claim about it. |
| 5. note the parallel lane `st_01a10887` (harness-side Node daemon client, `paneBinding.settledBy`, ambiguity guard) | **Noted in the record.** My measurements would still discriminate the four receipt cases, which the delta cannot — that work remains queued behind this host blocker, not superseded by it. |

**Nothing from pass 13 is invalidated by this pass**, and nothing new is claimed. The pass-13 source findings
(case 4 ruled out; the tuple's frontend-owned fields; the split assertions already scoped to the split pane)
stand as measured on `562144b3`.

## What would clear it (for whoever can elevate)

One of, in increasing weight — **the first is the one that was attempted and refused**:

1. **Elevate the SSH session and restart the service**: `Restart-Service Schedule -Force` from an
   **elevated** context (`sc.exe stop Schedule && sc.exe start Schedule` equivalently). The refusal was
   `OpenService` access, so elevation is the whole fix.
2. Restart from an **interactive elevated** session on the console (the user's own hand).
3. Only if those fail: a machine restart — **not attempted, and not mine to take**.

**After any of these, the verification is the same trivial-task probe in Step 3**, and the run only proceeds
when it reaches a **terminal** state with a **non-`0x41301`** result **and** produces its output file.

## Teardown

| Resource | Action | Receipt |
|---|---|---|
| my two probe tasks (`ferryx-p14-before`, `ferryx-p14-after`) | deleted by name after each probe | `ownTasksLeft=0` |
| my probe `.bat` files and output paths | left as evidence (small, in my own scratch dir) | — |
| any process of mine | **none started** this pass — no app, no runner, no server was launched | `REMAINING_MINE=0` |
| port 5173 | free | `PORT_5173=FREE` |
| the host profile | **untouched** — still the restored bytes | `sha=D07A1698…`, mtime `2026-10-05T04:36:27.6955984+09:00` |
| the **candidate tree** | **never edited BY ME**, but it is **NOT clean** — 5 foreign entries, see the Correction below | `HEAD = 562144b3` |
| **exact-PID discipline** | **no kill was needed this pass** — I started no process. The rule stands unchanged. | — |

## Carry-forward traps (all ten kept)

1. **A Windows scheduled task does not inherit the interactive PATH** — bare `bun`/`node` die silently; use
   absolute paths.
2. **`StreamWriter.WriteLine` emits `\r\n`; the daemon's control wire wants `\n`** — the handshake blocks
   forever otherwise. Use `$w.NewLine = [char]10`.
3. **`TcpClient.Connect` has no timeout** — bound every blocking call in a poll loop or it hangs forever.
4. **`Start-Process -ArgumentList @()` throws** — omit the parameter for a no-argument child.
5. **`serve-dist.mjs` takes the DIST dir** — passing `ui/` serves Vite's dev shell, the app renders blank, and
   the HTTP check still passes (200 + `id="root"`). Pass `ui/dist`.
6. **A scheduled task's default cwd is `%SystemRoot%\System32`**, not where you launched from.
7. **A stale `.done` marker fires monitors immediately** — clear it at script start.
8. **The runner's typed `FRONTEND_PORT_OCCUPIED` refusal is real and fail-closed** — my leftover listener held
   5173 and the runner refused rather than reusing it; I killed it by the exact PID from the listener listing.
9. **The harness gives the app no stderr sink** — fixed in the candidate by `562144b3`.
10. **`schtasks` can wedge host-wide while the `Schedule` service still reports Running** — tasks are accepted,
    report `Running`/`0x41301`, and never execute. Since the runner **requires** session-1 delegation, a
    wedged task subsystem blocks every native run on that host. **Check a trivial task's output before believing
    a stalled run is a code defect.**

**New trap 11 (this pass):** **`isAdmin=True` from `WindowsPrincipal.IsInRole` does NOT mean the session can
open a privileged service.** An SSH session's token reports Administrators membership while `OpenService` for
`SERVICE_STOP` is still denied — so a "we are admin, try harder" reading of that check is wrong. **Prove
access by the operation, not by the group claim**: `sc.exe query <name>` succeeding while
`Stop-Service` fails is the signature of exactly this.


## Correction: the candidate tree is dirty with ANOTHER SESSION'S live work

My first draft of the teardown table said `git status --porcelain` was empty. **That was wrong when I checked
it at the end of this pass, and I am correcting it rather than leaving an inaccurate claim on the record.**

```
HEAD = 562144b3
 M scripts/lib/qa-scenarios/common-harness.mjs     mtime 2026-10-05 05:12:25
 M scripts/lib/qa-scenarios/pane-binding.mjs       mtime 2026-10-05 05:14:34
 M scripts/qa/pane-liveness.mjs                    mtime 2026-10-05 05:13:13
 M scripts/qa/pane-liveness.test.mjs               mtime 2026-10-05 05:15:26
?? scripts/lib/qa-scenarios/daemon-inventory.mjs   mtime 2026-10-05 05:16:17
```

**These are not mine.** Their mtimes (05:12–05:16) fall inside this pass but I wrote **only** to
`task-9/` evidence files — zero of the five is under `task-9/`, and I ran no edit against the candidate tree
this pass. They are the **`st_01a10887` lane's live work** described in the dispatch: a new
`daemon-inventory.mjs` (the harness-side Node daemon client) plus edits to `pane-binding.mjs` (the
`settledBy` / fallback change) and its callers and tests.

**So, per the shared-tree rules: these are read-only to me and I did not touch them.** Two consequences worth
flagging:

1. **Any future run I take must state whether it ran against `562144b3` or against this in-flight lane's
   working tree.** If the lane's edits are present when I stage, my run measures *their* uncommitted code, not
   the frozen revision — and the receipt-case discrimination would be attributed to the wrong revision.
2. **The lane's `pane-binding.mjs` change is the one my pass-13b audit described** (prefer the product's tuple,
   fall back to the measured delta, record `settledBy`, keep a one-session ambiguity guard). My audit's two
   reported points — the delta path needing an equivalent ambiguity guard, and `backendSessionId` being the only
   field with an assertion-side contract — apply directly to that file.

### Resolution (observed minutes later)

The lane **committed that work as `bc078f70`** (`feat(qa): settle the pre-split pane binding from a measured
**daemon inventory delta**`, 05:17:31), and `git status --porcelain` is **empty again**. Its message states the
implemented decision as: *the pre-split pane is a SETUP artifact, not the thing under test*, which is the
recorded decision.

**This matters for my next run: `HEAD` is now `bc078f70`, not `562144b3`.** Any measurement I take must name
which revision it ran on — and the pass-13 plan as written says `562144b3`, so a future run at `bc078f70` would
be measuring the **delta-based binding**, not the frozen revision I staged my probe against. **I will re-stage
and re-state the revision before taking that run.**
