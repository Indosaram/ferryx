# REPORT — PASS 17

## Scope and outcome

The dispatch asked me to **localise the stall, not explain it**, by instrumenting the delegated `.bat` with
timestamped step markers, then to test the conhost lead and compare against the known-good window.

**Outcome: the stall is LOCALISED — and it is INTERMITTENT.** It is a line in a marker log, not a gap:
**the task's `cmd.exe` never executes its own first line.** Two of three runs stalled at exactly that point and
one completed the whole chain. The conhost lead is **refuted**. The stall remains **unexplained in cause**, and I
say so plainly rather than substituting a guess.

## Revision

**`bc078f70`** for everything in this pass — staged from the **clean candidate tree** plus my two verifier
instruments (the bat markers and the pane probe).

---

## 1. THE LOCALISATION — a measured line, and it is the first line

### Run r1 — COMPLETED (the chain is intact end to end)

```
[2026-10-05  5:38:28.53] 01_ENTRY cwd=C:\Windows\System32
[2026-10-05  5:38:28.54] 02_AFTER_CD cwd=…\source-21dea3c0 err=0
[2026-10-05  5:38:28.54] 03_PATH=C:\Windows\system32;…;C:\Program Files\nodejs\;…;C:\Users\sook\.bun\bin;…
    C:\Program Files\nodejs\node.exe
[2026-10-05  5:38:28.60] 04_AFTER_WHERE_NODE err=0
[2026-10-05  5:38:28.60] 05_BEFORE_NODE cmd="…node.exe" "…pane-liveness.mjs" …
[2026-10-05  5:38:33.62] 06_AFTER_NODE err=1
[2026-10-05  5:38:33.62] 07_EXIT
```

Every step ran: node resolved (`where node` → `C:\Program Files\nodejs\node.exe`, `err=0`), the delegated
runner started, ran **5.02 s**, and exited 1. The action timeline reached
`launch.binary → fixture-setup → owned-window → pane-inventory-before → click-pane-affordance`.

### Runs r2 and r3 — STALLED, at a point the markers pin down exactly

| observation | r2 | r3 |
|---|---|---|
| `relaunch.markers.log` created | **NO** | **NO** |
| **the bat's `01_ENTRY` marker** | **absent** | **absent** |
| `relaunch-record.json` written | yes | yes |
| task `Status` / `LastResult` | `Running` / `267009` | `Running` / `267009` |
| the task's `cmd.exe` exists | **yes — PID 26416, S1, PPID 1264** | yes |

**This is the answer to the dispatch's question 2 — the last marker before the stall:**

> **There is NO marker at all. The stall is BEFORE `01_ENTRY` — i.e. before the batch file's first line
> executes.** The scheduler created the process (it is a real `cmd.exe` with the Schedule `svchost` as its
> parent, started at exactly the `Last Run Time`), and that process **never ran its own first command**.

**The stall is therefore between "process created" and "line 1 executed"** — it is not in my markers' territory
at all, which is exactly why the markers are silent.

### What the stuck process was doing — measured

```
PID 26416  cmd.exe  S1  PPID 1264 (svchost -s Schedule)  start 05:39:39.648
  children = 1:  conhost.exe PID 41672  start 05:39:39.655   <- allocated 7 ms later
  cmd.exe:    cpu=0  handles=24  threads=1   wsMB=2.8
              thread 64996 state=Wait  waitReason=Executive
  conhost:    cpu=0.0156  handles=52  threads=1  wsMB=6.1
              thread 40616 state=Wait  waitReason=Executive
```

**Both processes are alive, single-threaded, zero CPU, waiting on `Executive`** (a kernel object). So the
console *was* allocated; the process is blocked on a kernel wait **after** console setup and **before** running
the batch.

## 2. The conhost lead — TESTED and REFUTED

The dispatch asked me to count conhost by session and parent liveness, identify which are mine, and test whether
reaping **only my own** changes the outcome. Done:

| | before reap | after reap |
|---|---|---|
| session-1 `conhost.exe` | **6** (13060, 1388, 25988, 48276, 41672, 62884) | **2** (13060, 62884) |
| identified as mine (dead parent = my killed probe PIDs 5420 / 62652 / 17752, plus my stuck cmd's own) | **4** | 0 |
| **trivial task after the reap** | — | **`Running` / `267009` / no output** |

**Reaping my own leaked conhosts did NOT clear the stall.** So the conhost population is **not** the mechanism.
**Foreign conhosts were left alone** (13060's parent is `maho-host.exe`; 62884's is 59012 — neither is mine).

I also confirmed the population is not obviously pathological: **67 total (S0 = 63, S1 = 4), 5 with a dead
parent** — and a session-1 delegated run has **completed** with that population in place (r1).

## 3. The intermittent character — the most important new fact

| run | started | delegation | marker log |
|---|---|---|---|
| r1 | 05:38:28 | **completed in 5 s** | full, 7 markers |
| r2 | 05:39:39 | **stalled** | none |
| r3 | 05:42:28 (after reaping my conhosts, nothing of mine running) | **stalled** | none |

**So the stall flipped between 05:38 and 05:39 on the same host, same staged tree, same command line — with no
change I made in between.** That is the finding: **the delegation is intermittent**, not deterministically broken.
It also means **r2/r3 are not a regression from anything I did** (r3 ran with my conhosts reaped and my processes
gone, and still stalled).

## 4. The known-good comparison — data, not a conclusion

| window | delegation |
|---|---|
| **04:23–04:26** (pass 13/14) | worked |
| **05:38:28** (r1, this pass) | worked |
| **05:39:39–05:42** (r2, r3, this pass) | stalled |

**The stall sits between two working windows, and it also occurred *within* this pass between two runs two
minutes apart.** So "whatever differed at 04:26" is not a stable contrast I can measure — the host moves in and
out of the condition. **Reporting the sequence, not a cause.**

## 5. Trap 14 RECURRED, and it cost me a measurement

The first instrumented run failed with:

```
verdict = FAIL   code = ASSERTION_FAILURE   message = probePane is not defined
```

**My pane probe's import line never landed.** At `bc078f70` the lane changed that import to
`import { bindPaneSession, createPaneInventoryReader } from '../lib/qa-scenarios/pane-binding.mjs';`, while my
injection anchored on the **older** form — so the import was skipped **while my two call-site injections still
applied**. I had "verified" the injection by checking the file had changed; **it had, for the call sites.**

**And it recurred a second time in this pass**: my `.NET` retest reported `ArgumentList` as **null** and I read
`HUNG` — because I ran that script under **Windows PowerShell 5.1**, where `ProcessStartInfo.ArgumentList` does
not exist. **The corrected pwsh 7.6.6 run had already shown all four forms completing in 23–46 ms.** So the
`.NET_cmd_now=HUNG` line in this pass's evidence is **my bug again**, not host evidence, and I am labelling it as
such rather than letting it sit in the record as a data point.

**Generalised, as the dispatch asked:**

> **Verify each step's precondition before blaming the next layer — and verify the precondition itself, not a
> proxy for it.** "The file changed" is not "the import landed". "The script ran" is not "the script ran under the
> interpreter that has the API I used". **Twice in this pass, a hang I was about to attribute to the host was my
> own harness.** The cheap check that catches both: **assert the specific thing you changed**, and **state which
> interpreter/version the test ran under**.

## 6. What I did NOT do

- **I did not explain the stall.** Cause unknown; I am not guessing.
- **I did not reap foreign processes** — only 4 conhosts and 1 `cmd.exe` whose parents I could prove were mine.
- **No reboot request, no elevation, no product repairs.**

## 7. The pass-13 measurements — NOT taken, and why

The dispatch said to continue into them **if the runner reaches `launch.binary`**. It did in **r1** — but r1's
inner run **failed on my own injection bug**, so it produced no valid `verifier-probe.jsonl`, no `settledBy`,
and no receipt-case evidence. **r2 and r3 stalled before the app ever launched.** So there is **no valid run to
measure**, and the four-case discrimination, native-vs-DOM, the inventory delta and `settledBy` remain unmeasured.

**The good news for the next attempt**: r1 proves the chain works when the stall is absent, and my corrected
injection (import verified per-patch) is staged and ready.

## Teardown

| Resource | Action | Receipt |
|---|---|---|
| my probe tasks (`ferryx-p17-probe`, `ferryx-p17-after`, and the runner's `ferryx-qa-*`) | ended + deleted by name | `ownTasksLeft=0` |
| **my 4 leaked conhosts + 1 stuck task `cmd.exe`** (41672, 26416, 1388, 25988, 48276) | killed by **exact PID**, each **identity-checked** against its expected parent PID | `IDENTITY_OK` ×5 |
| my stuck launchers/children (36180, 59444, 30380, 14612, 67152) | killed by exact PID, identity = `pane-liveness`/`run17` in the command line | `IDENTITY_OK`; 2 mismatches correctly refused |
| **FOREIGN conhosts** (13060 ← `maho-host.exe`; 62884 ← 59012) and other sessions' processes | **reported, not touched** | — |
| the Schedule service | not modified | — |
| the host profile | untouched | `sha=D07A1698…` |
| the **candidate tree** | never edited by me | `HEAD = bc078f70`, `git status --porcelain` empty |
| final | — | `REMAINING_MINE=0`, `PORT_5173=FREE`, `FREE_GB=15.27` |

## Carry-forward traps (fifteen)

1–13 as previously recorded (scheduled-task PATH; `\r\n` wire discipline; `TcpClient.Connect` timeout;
`-ArgumentList @()`; `serve-dist.mjs` needs the DIST dir; task cwd is `%SystemRoot%\System32`; stale
`.done` markers; `FRONTEND_PORT_OCCUPIED`; the app-stdio sink gap; a wedged task subsystem blocks every run;
`isAdmin=True` is a false positive; the service DACL decides a restart; the four-form launch matrix and
check-a-process's-children).

**14. Verify each step's precondition, not a proxy for it** — and **state which interpreter/version the test ran
under**. Two hangs this pass were my own harness (an injection anchor that silently missed; `ArgumentList` used
under PowerShell 5.1 where it does not exist).

**15. NEW — a task whose `cmd.exe` is created but never executes its first line is an INTERMITTENT stall, not a
broken configuration.** Localise it with a marker written by the bat's **own first line**: if the marker file is
never created while the task reports `Running`/`0x41301` and a real `cmd.exe` exists with the service as its
parent, the stall is **before line 1**, and no amount of instrumenting the body will show it. Distinguish it from
a deterministic failure by **repeating the run** — mine completed once and stalled twice with no configuration
change.
