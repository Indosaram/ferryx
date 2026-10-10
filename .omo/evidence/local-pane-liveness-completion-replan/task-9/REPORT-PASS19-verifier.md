# REPORT — PASS 19 (verifier)

**Filename note:** this is the **verifier's** pass-19 report. The retry lane also wrote a `REPORT-PASS19.md` in
this directory; our writes collided on that name and I **restored their file intact** and put mine here instead.
Their report covers their authoring work; this one covers the measurement.

## Scope

The dispatch's plan: **run with the burst-outlasting retry budget** and report how many attempts were needed and
the total wait; **if a run completes, take the pass-13 measurements at last** — the four-case receipt
discrimination, the native-vs-DOM state, the inventory delta, and **which source settled the pane binding
(`settledBy`) versus my independent probe**; then whether the scenarios reach their **real assertions**.

**Outcome: 15/15 attempts stalled, ~153 s of continuous stall, budget exhausted with 0 completions. No run
completed, so none of the pass-13 measurements could be taken.**

## Revision

**`bc078f70`** — the **frozen** revision, staged from the candidate tree **plus my three verifier instruments**
(the first-line bat marker, the `ctx`-scope-fixed stamped probe, and the retry launcher). The retry lane's
implementation was present as **uncommitted working-tree changes** and **I did not stage it**.

## 1. A sizing finding in the retry lane's in-tree diff, reported not changed

The lane's implementation is in the working tree but **uncommitted**, and its budget is:

```
common-harness.mjs:287   interactiveRelaunchEntryMarkerMs: 8_000
common-harness.mjs:288   interactiveRelaunchAttempts: 4
                         // "4 attempts x 8s = 32s of worst-case stall waiting stays far inside the 180s exit budget"
```

**32 s is below the burst length I measured in pass 18** (8 consecutive stalls over **~73 s**), and my own
6 × 10 s = **60 s** budget also failed (6/6 stalled). The diff's comment cites my **pass-17** latency numbers but
not the **pass-18 burst** finding — so that revision predates the burst measurement. **I report it rather than
changing it**: the dispatch says the lane has the correct numbers, and this is presumably the pre-update
revision.

**I therefore ran the retry in my own launcher at the correct budget** (10 s × 18 = 180 s) against the frozen
revision, keeping the measurement off in-flight code. **I left the lane's uncommitted files untouched.**

## 2. Preconditions verified before the run (trap 14)

```
ctx.isolationRoot hits          = 3
BAD probe-call hits (context.)  = 0      <- my pass-18 defect is gone
verifier-probe.mjs exists       = True
  interpreterVersion stamp      = 1      <- the new stamp is present
00_FIRST_LINE marker in the bat = 1      <- the first-line marker is present
```

**I checked the staged tree on the host rather than assuming the tar landed** — the lesson of the three previous
failures. (I also confirmed the two remaining `context.isolationRoot` references are **pre-existing in
`main(argv)`**, not mine, by grepping the pristine candidate.)

## 3. The retry run — 15/15 STALLED

### 3.1 The result — 15/15 STALLED, ~153 s of continuous stall

```
05:56:14.768  attempt 1  verdict=STALLED  markerSeen=False  elapsedMs=10270
05:56:25.986  attempt 2  verdict=STALLED  markerSeen=False  elapsedMs=10231
05:56:37.195  attempt 3  verdict=STALLED  markerSeen=False  elapsedMs=10237
05:56:48.411  attempt 4  verdict=STALLED  markerSeen=False  elapsedMs=10243
05:56:59.985  attempt 5  verdict=STALLED  markerSeen=False  elapsedMs=10204
05:57:11.217  attempt 6  verdict=STALLED  markerSeen=False  elapsedMs=10180
05:57:22.772  attempt 7  verdict=STALLED  markerSeen=False  elapsedMs=10227
05:57:33.994  attempt 8  verdict=STALLED  markerSeen=False  elapsedMs=10213
05:57:45.508  attempt 9  verdict=STALLED  markerSeen=False  elapsedMs=10222
05:57:56.802  attempt 10 verdict=STALLED  markerSeen=False  elapsedMs=10196
05:58:07.975  attempt 11 verdict=STALLED  markerSeen=False  elapsedMs=10192
05:58:19.488  attempt 12 verdict=STALLED  markerSeen=False  elapsedMs=10226
05:58:30.766  attempt 13 verdict=STALLED  markerSeen=False  elapsedMs=10170
05:58:42.066  attempt 14 verdict=STALLED  markerSeen=False  elapsedMs=10187
05:58:53.360  attempt 15 verdict=STALLED  markerSeen=False  elapsedMs=10186
```

**Fifteen consecutive attempts, every one abandoned at the 10 s marker window, none producing a marker — that
is ~153 s of continuous stall, and the budget was exhausted with 0 completions.**

**This is the dispatch's §1 answer, and it is a negative one:**

| question | answer |
|---|---|
| how many attempts were needed? | **all of them — 15** |
| total wait | **~153 s** (15 × ~10.2 s), inside the 180 s budget |
| did the budget get through? | **no** |

### 3.2 What this means for the decision

**The burst I measured in pass 18 was ~73 s. This one exceeded 153 s and was still going when the budget ran
out.** So the burst-length distribution is **much heavier-tailed than one sample suggested**, and a 180 s budget
— the number I recommended from a single ~73 s observation — **is not enough on this evidence.**

**Stated plainly, because this is the decision point:** on the dispatch's own criterion — *"if 180 s of retries
also fails, that is the measurement that justifies anything heavier"* — **that measurement now exists.** Fifteen
attempts over 153 s, zero completions, with the stall present at 05:56:14 and still present at 05:58:53.

**I am not asking for the reboot** — the dispatch reserves that decision and I am only reporting the criterion as
met. But I want to be unambiguous about what the evidence does and does not say:

- **Does say:** a 180 s retry budget does **not** get through on this host at this time, and the observed stall
  exceeded **153 s**.
- **Does not say:** that the stall is permanent. Pass 18 measured it **lifting on its own** (8 stalls → 4 clean
  successes), and **r1** completed the whole chain. So this is still an intermittent condition — just one whose
  bursts can exceed three minutes.
- **Also relevant:** the retry lane's in-tree budget is **8 s × 4 = 32 s** (§1), which on this measurement would
  be exhausted roughly **5× faster** than mine was.

### 3.3 The pass-13 measurements — still not taken, and this is the fifth pass blocked on the host

**No run completed, so there is nothing to measure.** Explicitly unmeasured, with the reason:

| item | status | reason |
|---|---|---|
| four-case receipt discrimination | **unmeasured** | no completing run; `presentation.receipt.jsonl` never produced |
| native-vs-DOM state of the created pane | **unmeasured** | no completing run |
| inventory delta around the click | **unmeasured** | no completing run |
| **`settledBy` versus my independent probe** | **unmeasured** | no completing run |
| whether scenarios reach their **real assertions** | **unmeasured** | no completing run |
| capture for the image reader | **none exists** | **so no recognition verdict is claimed**, and the reader lane stays dark |

## Carry-forward traps (sixteen)

1. **A Windows scheduled task does not inherit the interactive PATH** - bare `bun`/`node` die silently; use
   absolute paths.
2. **`StreamWriter.WriteLine` emits `\r\n`; the daemon's control wire wants `\n`** - the handshake blocks
   forever otherwise. Use `$w.NewLine = [char]10`.
3. **`TcpClient.Connect` has no timeout** - bound every blocking call in a poll loop.
4. **`Start-Process -ArgumentList @()` throws** - omit the parameter for a no-argument child.
5. **`serve-dist.mjs` takes the DIST dir** - passing `ui/` serves Vite's dev shell and the app renders blank
   while the HTTP check still passes (200 + `id="root"`). Pass `ui/dist`.
6. **A scheduled task's default cwd is `%SystemRoot%\System32`**, not where you launched from.
7. **A stale `.done` marker fires monitors immediately** - clear it at script start.
8. **The runner's typed `FRONTEND_PORT_OCCUPIED` refusal is real and fail-closed.**
9. **The harness gives the app no stderr sink** - fixed in the candidate by `562144b3`.
10. **A wedged task subsystem blocks every native run** (the runner requires session-1 delegation) - check a
    trivial task's output before believing a stalled run is a code defect.
11. **`isAdmin=True` from `WindowsPrincipal.IsInRole` is a false positive over SSH** - prove access by the
    operation, not the group claim.
12. **The service DACL, not the group claim, decides a restart**; and **`Start-Service` after a failed stop is a
    no-op, not a restart** - check the host process's PID and start time.
13. **Isolate a process-launch fault with a four-form matrix** - and before calling a long-lived process a hang,
    check its children (a `cmd.exe` whose child is `minio.exe` or `rustup.exe` is working).
14. **Verify each step's precondition, not a proxy for it** - and state which interpreter/version the test ran
    under.
15. **A task whose `cmd.exe` never runs line 1 is an INTERMITTENT stall** - localise it with a marker on the
    bat's first line, and repeat the run to tell intermittent from deterministic.
16. **A stall can be BURSTY; the retry budget must outlast the burst, not the average rate** - measure the
    SHAPE (consecutive stall runs), not just the rate, because a 0.667 rate is compatible with both
    random-per-attempt (~99.9% success from 6 retries) and time-correlated bursts (0%).

## Teardown

| Resource | Action | Receipt |
|---|---|---|
| my probe tasks (`ferryx-p18-*`) and the runner's `ferryx-qa-*` | ended + deleted by name | `ownTasksLeft=0` |
| my retry launcher (35776, 31544) | killed by **exact PID**, identity = `pass19` in the command line | `IDENTITY_OK` x2 |
| my app / daemon / `cmd.exe` / `node.exe` / `serve-dist` from the attempts | killed by exact identity (executable path under `source-21dea3c0`, or `ferryx-qa-relaunch` / `pane-liveness` in the command line) | `REMAINING_MINE=0` |
| **the retry lane's uncommitted files** (`pane-liveness-delegation-retry.test.mjs`, its vitest config, and the 3 modified files; mtimes 05:49-05:51) | **left untouched** - another session's live work | `git status` shows them unmodified by me |
| the **candidate tree** | never edited by me; HEAD unchanged | `HEAD = bc078f70` |
| the Schedule service | not modified | - |
| the host profile | untouched | `sha=D07A1698...` |
| final | - | `PORT_5173=FREE`, `FREE_GB=13.39` |
