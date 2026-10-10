# REPORT — PASS 20 (verifier)

## Scope

The dispatch: stage **`cfb4374b`**, run with the **15-minute sequence budget** (900 s / 90 attempts at the same
10 s stall window), and **if it gets through**, take the pass-13 measurements at last — the four-case receipt
discrimination, the native-vs-DOM state, the inventory delta, **`settledBy` versus my independent probe**, and
whether the scenarios reach their **real assertions**. If the 15 minutes also exhausts, **report the burst length
and stop**.

## Revision

**`cfb4374b`** (`fix(qa): raise the delegation retry sequence to outlast the measured 153s burst`) — staged from
the clean candidate tree **plus my verifier probe** (one new module and two call sites). **No bat instrumentation
this pass**: the committed retry already writes its own first-line entry marker
(`DELEGATION_ENTRY_MARKER_FILE`, four references in `windows-interactive.mjs`), so adding mine would have
duplicated the lane's mechanism.

## 1. My pass-19 budget flag — RETIRED, and the lead's correction verified on disk

The dispatch says my `8_000 × 4 = 32 s` reading was **an in-flight revision**, and that the committed
`2a741884` already carried `10 s × 18 = 180 s`. **I verified that on disk rather than accepting it:**

```
git show 2a741884:scripts/lib/qa-scenarios/common-harness.mjs
  interactiveRelaunchSequenceMs:   180_000
  interactiveRelaunchEntryMarkerMs: 10_000
  interactiveRelaunchAttempts:      18        <- 10s x 18 = 180s, exactly as specified

git show bc078f70:scripts/lib/qa-scenarios/common-harness.mjs   (what I staged in pass 19)
  interactiveRelaunchTimeoutMs: 180_000       <- NO retry budget at all
```

**Both halves confirmed.** `2a741884` carried the correct 180 s, so **my flag was stale — one revision too
early** — and I retire it. And `bc078f70` carried **no retry budget whatsoever**, which confirms the account I
gave in pass 19: the retry was entirely uncommitted when I measured, which is exactly why running my own
launcher against the frozen revision was the right call rather than reading in-flight numbers as the design.

**The lesson, stated for the record:** I read the working tree (in-flight) and reported it as the design. The
values were real, the *attribution* was wrong. **A number read from a dirty tree is a draft, not a decision** —
the same class as the three injection errors, in a different medium.

## 2. The budget at HEAD, verified before running

```
common-harness.mjs:303   interactiveRelaunchSequenceMs:   900_000     <- 15 minutes
common-harness.mjs:304   interactiveRelaunchEntryMarkerMs: 10_000     <- 10 s stall window
common-harness.mjs:305   interactiveRelaunchAttempts:      90
```

The committed retry is a real sequence budget and records what the dispatch's §1 asked for:

```
ledger.attemptsUsed          // how many attempts it needed
ledger.stopReason            // 'delegated-run-completed' | 'sequence-budget-exhausted' | 'deterministic-failure' | 'attempts-exhausted'
ledger.attempts[]            // per attempt: index, task name, marker path, whether the marker appeared
ledger.entryMarkerBudgetMs
```

## 3. Preconditions verified ON THE HOST before the run

```
probe import present      = 1
probe call sites          = 2
uses evidence.runDir      = 2
BAD ctx.evidenceDir       = 0     <- must be 0
verifier-probe.mjs        = True
interpreterVersion stamp  = 1
in-tree entry marker      = 4
sequenceMs 900000         = 1
entryMarkerMs 10000       = 1
attempts 90               = 1
binary sha = 6AD63C5A…    (unchanged: 0 Rust files changed across all of these revisions)
```

**One real defect caught by this check, and it is the trap-14 class again.** My pass-19 "fix" changed
`context.isolationRoot` → `ctx.isolationRoot`, but it also passed **`ctx.evidenceDir`** — and I have now
verified that **`ctx.evidenceDir` does not exist at this revision**. The correct source is **`evidence.runDir`**
(`EvidenceWriter`'s own per-run dir, where `app.stderr.log` lands — verified at
`common-harness.mjs:1519` and already used at `pane-liveness.mjs:430`).

**So pass 19's patch would have silently written the probe to `undefined`.** The pass-19 run never reached the
pane step, so it was never exercised — **a latent defect that only the host-side precondition check surfaced.**
This pass patches `evidenceDir: evidence.runDir` and asserts `BAD ctx.evidenceDir = 0`.

## 4. The run

Sequence budget 900 s, 10 s marker window, 90 attempts, launched against the staged `cfb4374b`. The runner
self-delegates and the **in-tree** retry drives the attempts, so the attempt count below is the lane's own
measurement, not my launcher's.

Results follow.


## 5. What the four-case discrimination will be read from, once a run completes

So the reading is mechanical when the run lands, I am stating the discriminator up front, from the source I
already verified in pass 13:

| case | evidence that distinguishes it |
|---|---|
| **1. surface host not attached** | the probe's tree shows **`Failed to attach native terminal`** and no `Native terminal input` |
| **2. no frame submitted** | the tree shows `Native terminal input` (attached) but `presentation.receipt.jsonl` stays at 0 lines |
| **3. presentation path not run** | the tree shows an attached pane and the receipt is absent **while `pane_liveness_presentation_receipt` never got a current tuple** |
| **4. emitter reached, zero lines** | **RULED OUT by source** (pass 13): all three emitters write the same `presentation.receipt.jsonl`, so a stale-completion rejection would still have left a line |

The probe records exactly the two tree signals that separate case 1 from 2–3 (`nativeInput`,
`attachFailure`), plus `exactNewTerminal`, per owned window, **stamped with the interpreter that produced it**.

## 6. The image-reader note (carried, not exercised)

**No capture exists, so no recognition verdict is claimed** — and the dispatch's constraint is recorded: when a
capture is eventually produced, the independent reader must be one that **preserves the `_` separator**, because
the pass-2 OCR instrument cannot distinguish `FERRYX_SPLIT_READY` from the space-separated form and **must not
be used**. The reader lane stays dark until a capture exists.


## 7. THE RUN — the delegation got through on ATTEMPT 1, and the pass-13 measurements were TAKEN

```
windows-interactive-delegation.json:
  entryMarkerBudgetMs = 10000
  sequenceBudgetMs    = 900000
  maxAttempts         = 90
  attemptsUsed        = 1          <- ONE attempt sufficed
  stalled             = false
  stopReason          = 'deterministic-failure'
  attempts[0].taskName = 'ferryx-qa-split-happy-26980-a1-sfpu5y'

delegation-entry.marker:
  DELEGATION_ENTRY 2026-10-05  6:08:49.75 task=…a1-sfpu5y cwd=C:\Windows\System32
```

**The first-line marker landed at 06:08:49.75, ~1 s after launch, and the delegation completed on the FIRST
attempt** — no stall, no retry wait. **So the intermittency cost, measured in a success window, is 1 attempt and
~0 s of stall waiting.** That is the dispatch's §1 number, and it is the *other* end of the distribution from
pass 19's 15 attempts.

**And this is the important structural result: the run that completed was the INNER run, and it produced a real
scenario verdict.** The action timeline went all the way through the pane step and into the split step:

```
barriers.prearmed → windows-interactive-admission → windows-interactive-relaunch → frontend.served
→ launch.binary → fixture-setup → owned-window → pane-inventory-before → owned-windows-enumerated
→ click-pane-affordance → pane-inventory-after → pane-session-bound
→ owned-windows-enumerated → click-split-affordance
```

## 8. The pass-13 measurements, at last

### 8.1 Which source settled the binding — `settledBy`

```json
{"action":"pane-session-bound","settledBy":"inventory-delta","source":"daemon-inventory-delta",
 "backendSessionId":"307b8c0c-f3b0-49f2-90a3-70956129d87b",
 "fixtureSessionIds":["19475642-00cb-4642-9c91-e511e4fdd2f0"],
 "observedPaneSessionIds":["307b8c0c-f3b0-49f2-90a3-70956129d87b"],
 "receiptGap":"…receipt lines: 0, sessions named by those lines: [], … observed pane sessions: []",
 "inventoryDelta":{"beforeCount":1,"afterCount":2,"added":["307b8c0c-…"],"removed":[],
                   "epoch":1791148130973,"daemonVersion":"2026.928.7","transport":"loopback-port"}}
```

**The binding settled from the measured daemon inventory delta** — the fallback the lane built — because the
receipt path had nothing: `receipt lines: 0`. **The delta is a real measurement**: before = 1 session
(the fixture), after = 2, added exactly one session, removals none, on the isolated loopback transport with the
daemon epoch recorded.

### 8.2 AGREEMENT with my independent probe — the cross-check the dispatch wanted

| | the harness (`inventory-delta`) | **my independent probe** |
|---|---|---|
| sessions before | 1 (`19475642-…`) | — |
| sessions after | 2 | **count = 2** |
| the pane session | `307b8c0c-f3b0-49f2-90a3-70956129d87b` | **`307b8c0c-…` present in `sessions`** |
| added set | `["307b8c0c-…"]` | identical |

**The two producers agree exactly** — same count, same session ids, same added session — and they reached it by
**different code paths** (the lane's Node client vs my PowerShell probe over the same control wire). My probe
result is **stamped `interpreter: pwsh 7.6.6` / `PowerShell 7+`**, per the trap-14 discipline.

### 8.3 The four-case receipt discrimination

**Case 1 — surface host not attached: RULED OUT.** My probe's tree at both labels shows the native pane
attached:

```
tree window 'Ferryx': total=108 named=63 exactNewTerminal=0
                      nativeInput=True   attachFailure=False
tabItems: ['main Close main']
errorText: []
```

`Native terminal input` is **present** and `Failed to attach native terminal` is **absent** → the surface host
**is** attached.

**Case 4 — emitter reached but writing zero lines: RULED OUT by source** (pass 13): all three emitters write the
same `presentation.receipt.jsonl`, so even a stale-completion rejection would have left a line.

**Cases 2 and 3 remain** — attached but no frame presented (2), or the presentation path not run (3). **The
receipt is at `lines: 0` while the surface is attached**, which is the shape of both. **I cannot separate 2 from
3 with the signals I have**, and I am not going to assert one: doing so needs an instrument inside the
completion coordinator, which is a product-side probe I am not authorised to add.

### 8.4 Native vs DOM

**Native.** The pane carries `ControlType.Edit|Native terminal input` — the native pane's own input element — and
the app window is the Tauri window with the native surface host. A DOM/browser pane would not expose that
element. **So the presentation receipt is the right producer in principle**, which is what pass 13 concluded from
the source and this confirms from the tree.

## 9. THE FIRST NATIVE SCENARIO VERDICT — and it is a real assertion, not a setup failure

```
inner result.json:
  verdict = BLOCKED
  code    = SPLIT_RIGHT_NOT_FOUND
  message = SPLIT_RIGHT_NOT_FOUND: the "Split pane right" affordance of the focused pane
            could not be identified: {… "focusedFound":false,"focusSource":null,
            "scopeDepth":-1,"scopeIsWindowRoot":false, "warmElements":108,
            "warmAttachCount":1,"warmDocumentSeen":true, …}
```

**This is the first time in this entire effort that a native scenario reached its own scenario assertion.** Every
earlier pass died in setup (no affordance, no delegation, no app). Here the app launched, the fixture settled,
the pane was created and bound, and the run advanced to `click-split-affordance` — **the split step's own
assertion** — where it failed for a specific, named reason.

**And the failure is a DRIVER SCOPE defect, not a missing affordance — proven by the disagreement between the
two producers.** The dispatch said *"a disagreement is more valuable than either alone"*; here it is:

| | says |
|---|---|
| **the harness driver** | `SPLIT_RIGHT_NOT_FOUND`, with `focusedFound: false`, `focusSource: null`, `scope.depth: -1`, `scope.isWindowRoot: false` |
| **my independent probe, same tree, same moment** | `ControlType.Button\|Split pane right` **EXISTS** — plus `ControlType.Button\|Split pane down` and a concatenated `…Copy Debug Info Split pane right Split pane down` |

```
[after-bind] window total=108 named=63
   FOUND: ControlType.Button|Copy Debug Info Split pane right Split pane down
   FOUND: ControlType.Button|Split pane right
   FOUND: ControlType.Button|Split pane down
   FOUND: ControlType.Edit|Native terminal input
```

**So the affordance is present in the 108-element tree and the driver's focus-scoped search missed it.** The
driver's own telemetry names the cause: **`focusedFound: false` with `scope.depth: -1`** — the scope
resolution could not find a focused element, so the search never reached the element. That is a **harness-side
focus/scope defect**, and it is reported, not patched by me.

## 10. A harness defect that MASKED the inner verdict: a create-vs-write race on the exit file

The outer run's own verdict was:

```
verdict = BLOCKED   code = INTERACTIVE_RELAUNCH_FAILED
message = INTERACTIVE_RELAUNCH_FAILED: delegated run wrote an unreadable exit code ""
```

**But the file it read was not empty.** On disk:

```
relaunch.exit  bytes = 34 20 0D 0A   ->  "4 \r\n"      (exit code 4, 4 bytes)
```

and `parseRelaunchExitFile` would have accepted it (`text.trim()` → `"4"` → `/^-?\d+$/` → `4`). **So the
parse did not reject the content; it received `""`.**

**The mechanism, from the source:** `awaitExitFile` waits with `waitForFile(exitPath, …)`, which fires on the
file **appearing**, and then the caller reads it **once**:

```js
const waited = await awaitExitFile(plan.exitPath, plan.timeoutMs);
…
const exitCode = parseRelaunchExitFile(waited.text);   // waited.text was ""
```

But the bat writes it as `echo %ERRORLEVEL% > exitPath`, and **cmd.exe's redirect creates the file and then
writes it in separate steps** — so a watcher on creation fires **between** the two and the immediate read sees
**zero bytes**. **The wait is on appearance; it needs to be on content.**

**This is the same class as trap 7 and my own session guidance — creation is not completion** — and it is a real
harness bug with a real cost: **it discarded a completed inner run that carried the first native scenario verdict
of the effort.** The inner `result.json`, `actions.jsonl` and my `verifier-probe.jsonl` all survived on disk,
which is the only reason the measurement was recoverable at all.

**Reported, not patched.** The fix is in the harness: wait for the exit file to be **non-empty and parseable**
(bounded), not merely present.

## 11. What remains unmeasured, with the reason

| item | status | reason |
|---|---|---|
| separating receipt case **2 from 3** | **unmeasured** | needs an instrument inside the product's completion coordinator |
| whether the split step passes once the **focus scope** is fixed | **unmeasured** | the driver's scope defect blocked the assertion before it could be evaluated |
| a **capture** for the image reader | **none produced** | no screenshot step was reached, so **no recognition verdict is claimed** and the reader lane stays dark |
| the **burst-length distribution** | **still only 2 samples** | 1 attempt (this pass) and 15 (pass 19); a single success window does not bound the failure |

## Carry-forward traps (eighteen)

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
    under. **Fourth instance this pass: a variable from the wrong scope (`ctx.evidenceDir`) that does not exist
    at that revision**, caught by the host-side precondition check before it could write to `undefined`.
15. **A task whose `cmd.exe` never runs line 1 is an INTERMITTENT stall** - localise it with a marker on the
    bat's first line, and repeat the run to tell intermittent from deterministic.
16. **A stall can be BURSTY; the retry budget must outlast the burst, not the average rate** - measure the SHAPE
    (consecutive stall runs), not just the rate.
17. **A number read from a DIRTY tree is a draft, not a decision** - cite the revision it came from, and check
    `git show <rev>:<path>` before reporting a design finding.

18. **NEW - watching a file for CREATION is not watching it for CONTENT.** `awaitExitFile` fired on the exit
file **appearing** and then read it once; `cmd.exe`'s `echo %ERRORLEVEL% > file` **creates the file and writes
it in separate steps**, so the read landed in between and returned `""` -> `UNREADABLE_EXIT` ->
`INTERACTIVE_RELAUNCH_FAILED`. **The file was 4 bytes (`"4 \r\n"`) on disk the whole time afterwards.** Same
class as trap 7 (a stale `.done`) and the same rule as monitoring: **wait for the content you need (non-empty,
parseable), bounded - never for mere appearance.** The cost here was severe: it **discarded a completed inner run
that carried the first native scenario verdict of the effort.**

## Teardown

| Resource | Action | Receipt |
|---|---|---|
| my launcher and the delegation's own attempts | the in-tree retry ends and exact-PID-kills each attempt's own `cmd.exe`; my launcher exited on its own after the result | `killed=0` needed |
| my scheduled tasks | ended + deleted by name | `ownTasksLeft=0` |
| any process of mine left running | **none** | `REMAINING_MINE=0` |
| **foreign processes** and the lane's uncommitted files | **reported, never touched** | - |
| the Schedule service | not modified | - |
| the host profile | untouched | `sha=D07A1698...` |
| the **candidate tree** | never edited by me | `HEAD = cfb4374b` |
| final | - | `PORT_5173=FREE`, `FREE_GB=13.27` |
