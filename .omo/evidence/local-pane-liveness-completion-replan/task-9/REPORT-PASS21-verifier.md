# REPORT — PASS 21 (verifier)

## Scope

The dispatch: **stage the new HEAD, let the delegation run, and report whether the split click now resolves** —
and if it does, whether the scenarios reach their real assertions with the asserted lines. If a capture is
produced, the image-reader lane goes live with a **`_`-preserving** reader. If the run stalls, **note which end of
the burst-length distribution it lands on** (currently two samples: 1 attempt in pass 20, 15 in pass 19).

## Standing by for the defect-fix lane

At the time of writing, the lane (`st_01a108cd`) had **not landed**: `HEAD` is still **`cfb4374b`**, the tree
is clean, and neither fix is present — no window-root fallback in `native-driver.mjs`, and no
"not yet written" content wait in `windows-interactive.mjs`. **A watch is armed** on HEAD moving and on the
exit-file fix appearing in the worktree, so this pass resumes the moment either happens.

**I have not staged `cfb4374b` again**, because staging the revision *without* the fixes would reproduce the
pass-20 result rather than answer the dispatch's question. The stage is prepared and ready:
`/tmp/t9p10/stage20` = `cfb4374b` + my verifier probe (probe import verified present, both call sites using
`evidenceDir: evidence.runDir`, and the 900 s / 10 s / 90-attempt budget confirmed in the staged
`common-harness.mjs`). It will be re-staged from the new HEAD the moment the lane lands.

## What this pass will answer, and how

| question | how it is answered |
|---|---|
| **does the split click now resolve?** | the inner `result.json` verdict: `SPLIT_RIGHT_NOT_FOUND` again (focus fallback did not help) vs a later assertion vs `PASS` |
| **which scope produced it** | the lane records which scope resolved the match (window-root fallback vs focus scope); I will read that field |
| **does the scenario reach its real assertions?** | the asserted lines from the inner `result.json` / `actions.jsonl` |
| **which end of the burst distribution?** | the delegation ledger's `attemptsUsed` and `stopReason` (1 = success window, 15+ = a long burst) |
| **is a capture produced?** | the presence of a screenshot artifact; **no capture → no recognition claim** |

## Why the two defects from pass 20 matter to this run

Both were measured, not inferred:

1. **The split search is a focus-scope defect, not a missing affordance.** The driver reported
   `SPLIT_RIGHT_NOT_FOUND` with `focusedFound: false`, `scope.depth: -1`, while my probe's tree at the same
   moment showed `ControlType.Button|Split pane right` **present in the same 108-element window**. The Windows
   body confirms the mechanism at `native-driver.mjs:992` — *"no focused pane could be identified inside the
   owned window, so no affordance was clicked"* — i.e. the search never reached the element.
2. **Creation is not content (trap 18).** `awaitExitFile` waited for `relaunch.exit` to **appear** and read it
   **once**; `cmd.exe`'s `echo … > file` creates then writes in separate steps, so the real `"4 \r\n"` was read
   as `""` → `UNREADABLE_EXIT` → `INTERACTIVE_RELAUNCH_FAILED`, **discarding a completed inner run that carried
   the first native scenario verdict.**

**Both are harness-side, and I am patching neither** — they are the lane's, and my job is to measure whether
their fixes work.


---

# PASS 21b — the two F1 audit holes assigned to me (H-16, H-17)

The dispatch assigned me **H-16 and H-17** because they need *execution*, and noted they are cheap `bun`/`cargo`
runs that do **not** need the interactive-session delegation — so the bursty stall does not block them. They run
on the remote host per the project's build rule.

**Revision for both: `cfb4374b` plus the `st_01a108cd` fixes, which are present in the worktree as uncommitted
changes.** I verified this rather than assuming it: the host's `ui/` and `src-tauri/` trees **match the local
hashes exactly** for every file under test, so the code the gates exercise is the code in this revision.

| file | host sha256 (first 16) | local | match |
|---|---|---|---|
| `ui/src/state/workspaceStore.test.tsx` | `9FACC684EC2454F1` | `9facc684ec2454f1` | **yes** |
| `ui/src/state/workspaceRestore.test.tsx` | `A71DB925536FB07C` | `a71db925536fb07c` | **yes** |
| `ui/src/lib/nativeTerminalInputQueue.test.ts` | `7488C2D1311E4313` | `7488c2d1311e4313` | **yes** |
| `ui/src/lib/localSplitContract.test.ts` | `25E630DEA6146E2F` | `25e630dea6146e2f` | **yes** |
| `src-tauri/src/ipc/pane_liveness_contract.rs` | `20C85DE073086D33` | `20c85de073086d33` | **yes** |

Toolchain on the host: **node v24.19.0, bun 1.4.0, cargo 1.97.0 (c980f4866 2026-06-30)**.

## H-16 — `workspaceStore` / `workspaceRestore` / input-queue as their OWN recorded gate — **PASS**

The hole, verbatim from `F1-PLAN-TO-ARTIFACT-AUDIT.md:347`: *"No separately recorded run: they are only covered
inside `bun run --cwd ui test`, which is RED on all three hosts."*

```
cmd: bun x vitest run --maxWorkers=1 \
       src/state/workspaceStore.test.tsx \
       src/state/workspaceRestore.test.tsx \
       src/lib/nativeTerminalInputQueue.test.ts
```

```
 ✓ src/lib/nativeTerminalInputQueue.test.ts (17 tests) 18ms
 Test Files  3 passed (3)
      Tests  100 passed (100)
   Duration  2.94s
H16_EXIT=0
```

**H-16 is CLOSED: the three suites run green as their own scoped gate, 100 tests, exit 0** — which the audit
recorded as never having happened. Note this is the **scoped** gate, not the full `bun run --cwd ui test` the
audit describes as RED; the hole was specifically that these suites had no separately recorded run.

**One measurement caveat I am labelling rather than hiding:** the run emitted substantial
`[ferryx:switch]` stdout, and my log capture went through PowerShell's `Tee-Object`. The summary lines
(`Test Files 3 passed (3)`, `Tests 100 passed (100)`, `H16_EXIT=0`) are unambiguous, but **the captured log file
may be UTF-16LE rather than UTF-8** if it was written by Windows PowerShell 5.1 — a known trap on this host
(`notes/facts/ferryx-frontend-ab-baseline-and-windows-ps-log-encoding-2026-09-28.md`). **The exit code and the
counts are the evidence; the byte encoding of the log is not**, and anyone re-reading that file should convert it
before trusting a `tail`.

## H-17 — the exact `pane_liveness_contract` filter and `localSplitContract.test.ts` under their own names

The hole, verbatim from `:348`: *"The exact `pane_liveness_contract` filter was never run under its own name
(its cases ran only as a subset of `pane_liveness_`); `localSplitContract.test.ts` appears in no recorded
scoped-command log."*

Both are launched; results below.


### H-17b — `localSplitContract.test.ts` under its own name — **PASS**

```
cmd: bun x vitest run --maxWorkers=1 src/lib/localSplitContract.test.ts
 ✓ src/lib/localSplitContract.test.ts (6 tests) 3ms
 Test Files  1 passed (1)
      Tests  6 passed (6)
   Duration  988ms
H17b_EXIT=0
```

**H-17b is CLOSED: 6 tests, exit 0**, where the audit recorded *"appears in no recorded scoped-command log."*

### H-17a — the exact `pane_liveness_contract` filter — **BLOCKED, and the block is a foreign process**

The first attempt failed at the **link** stage, not in a test:

```
LINK : fatal error LNK1104: cannot open
  '…\src-tauri\target\debug\deps\ferryx_lib-edbc0e6372dc8389.exe'
error: could not compile `ferryx` (lib test) due to 1 previous error; 97 warnings emitted
H17a_EXIT=101
```

**`LNK1104` here means the test executable could not be written because another process holds it.** I enumerated the
holders rather than guessing:

```
PID=24632 PPID=13520 S0 start=2026-10-04T23:20:14
  exe=C:\Users\sook\ferryx-pane-completion\source-21dea3c0\src-tauri\target\debug\deps\ferryx_lib-edbc0e6372dc8389.exe
  cmd="…ferryx_lib-edbc0e6372dc8389.exe" --test-threads=1
```

**That is the exact exe my link targets, it is in the staged tree, and it started `2026-10-04T23:20:14` — about
seven hours before this session began. It is NOT mine.** Per the standing rule I am **reporting it, not killing
it**, even though it is almost certainly a hung `--test-threads=1` run from an earlier session (a test exe alive
for 7+ hours). **Three other foreign holders are also present** and left alone: `task3-product-gate-01a100ad`
(×2, since 10-03) and `ferryx-baseline-herdr-50c5ca99` (×5, since 10-04).

**Retry taken, and it is the right one:** the same gate with an **isolated `CARGO_TARGET_DIR`**
(`…\ferryx-pane-completion\h17a-target`), so the link never touches the file another session holds. Cost is a
cold build; **the foreign process is untouched.**

### A precise revision note for H-16 and H-17

**`14744c8a` touches only `scripts/`** — verified with `git show --name-only`, and `git diff --name-only
cfb4374b 14744c8a -- ui src-tauri` is **empty**. So **`ui/` and `src-tauri/` are byte-identical at `cfb4374b`
and `14744c8a`**, and the H-16/H-17 results are validly attributed to **both** revisions — the gates test trees
that the fix commit did not modify.


---

# PASS 21c — the burst distribution, third sample, and the foreign-lock handling

## The burst-length distribution now has THREE samples, and this one is the longest

| sample | revision | attempts before getting through / giving up | stall duration |
|---|---|---|---|
| pass 20 | `cfb4374b` | **1 attempt** — got through immediately | **~0 s** |
| pass 19 | `cfb4374b` | **15 attempts** — budget exhausted, 0 completions | **~153 s** |
| **pass 21 (this pass)** | **`14744c8a`** | **26 attempts** — I stopped the run to re-stage | **~260 s and still going** |

```
06:36:33  attempt-1   …   06:41:31  attempt-26     (26 consecutive stalls, ~10.2 s apart)
```

**So the distribution is: 1, 15, 26+ — and the 26-attempt burst was still running when I ended it**, which means the
observed maximum is a **lower bound**. This is the dispatch's "which end of the burst distribution" question
answered with a third point, and it **strengthens the pass-19 conclusion rather than weakening it**: bursts of
~260 s exist, so the 900 s / 90-attempt budget was the right call, and a budget sized from pass 18's ~73 s would
have failed again here.

**I stopped that run deliberately** to re-stage at `89a363a0` after the H-18 commit landed — not because it
failed. Its ledger is preserved (`ev21/split-happy/delegation`, 26 attempt dirs).

## A hazard I checked, and my stage came out clean

The dispatch warned that a lane was **mid-flight editing `split-scenarios.mjs` and `pane-liveness.test.mjs`** in
the working tree, and that running against it could produce a false failure. **I verified my stage rather than
assuming**, because I build stages with `cp -R` from the working tree:

```
git archive 14744c8a scripts | tar -x -C archive21
diff -rq archive21/scripts stage21/scripts
  Only in stage21/scripts/lib/qa-scenarios: verifier-probe.mjs      <- mine
  Files …/pane-liveness.mjs and …/pane-liveness.mjs differ          <- my 3 probe edits
```

**Nothing else differs** — the lane's `split-scenarios.mjs` and `pane-liveness.test.mjs` edits were **not** in my
stage. **For the `89a363a0` stage I switched to the committed blob** (`git archive 89a363a0 scripts`), which makes
the hazard structurally impossible rather than merely checked: the tar is built from git objects, so no
working-tree edit can leak in. **That is the pattern I will use from here.**

## The foreign lock: enumerated, reported, and bought around

The first H-17a attempt failed at the **link** stage with `LNK1104` on
`ferryx_lib-edbc0e6372dc8389.exe`. I enumerated the holders instead of guessing:

```
PID=24632  PPID=13520  S0  start=2026-10-04T23:20:14
  exe = …\ferryx-pane-completion\source-21dea3c0\src-tauri\target\debug\deps\ferryx_lib-edbc0e6372dc8389.exe
  cmd = "…ferryx_lib-edbc0e6372dc8389.exe" --test-threads=1
```

**Recorded for a future session: PID 24632, started `2026-10-04T23:20:14`, holding the staged tree's test exe.**
It predates this session by ~7 hours, so it is **not mine**, and I **did not kill it** — although a
`--test-threads=1` test exe alive for 7+ hours is almost certainly a hung earlier run.

**Three other foreign holders, also left alone:**
`task3-product-gate-01a100ad` (×2, since 10-03) and `ferryx-baseline-herdr-50c5ca99` (×5, since 10-04).

**Worked around by building into an isolated `CARGO_TARGET_DIR`** (`…\ferryx-pane-completion\h17a-target`), so the
link never touches the held file. The cost is a cold build; **no foreign process was touched.**


---

# PASS 21d — THE SPLIT CLICK NOW RESOLVES, and the scenario advanced past it

**Revision: `89a363a0`**, staged from the **committed blob** (`git archive 89a363a0 scripts`) plus my verifier
probe — so the mid-flight hazard the dispatch flagged is structurally excluded, not merely checked.

## The headline — the fallback did exactly what it was built to do

The inner run's own `click-split-affordance` action, verbatim:

```json
{"action":"click-split-affordance","code":null,"assertedUniqueEnabled":true,
 "window":{"mainWindowHandle":790578,"windowVisible":true,"interactive":true,"sessionId":1},
 "scope":{"focusedFound":false,"focusSource":null,"origin":"window-root","depth":0,"isWindowRoot":true},
 "candidateCount":1,"actionableCount":1,
 "candidates":[{"index":0,"name":"Split pane right","controlType":"ControlType.Button",
                "enabled":true,"offscreen":false,"rectEmpty":false,"rect":"1706,237,26,26","inWindow":true}],
 "chosen":{"index":0,"name":"Split pane right","controlType":"ControlType.Button","enabled":true,
           "rect":"1706,237,26,26","inWindow":true},
 "detail":"clicked the single actionable split affordance of the focused pane"}
```

Read against pass 20:

| field | pass 20 (before) | **pass 21 (after)** |
|---|---|---|
| `code` | **`SPLIT_RIGHT_NOT_FOUND`** | **`null`** |
| `scope.focusedFound` | `false` | `false` — still unresolvable, as expected |
| `scope.origin` | *(field did not exist)* | **`"window-root"`** |
| `scope.depth` / `isWindowRoot` | `-1` / `false` | **`0` / `true`** |
| `candidateCount` / `actionableCount` | *(never reached)* | **`1` / `1`** |
| `detail` | *(the failure)* | **"clicked the single actionable split affordance"** |

**Focus is still unresolvable and the window-root fallback resolved it to exactly one actionable element and
clicked it.** Not `NOT_FOUND`, and **not `NOT_UNIQUE`** — uniquely resolved, so this scenario did not need pane
scoping. The candidate list carries the single element with its rect and `inWindow: true`: nothing invented, no
first-match shortcut.

## `scope.origin` versus my independent probe — AGREEMENT

```
my probe (pwsh 7.6.6), both labels, same window:
  total=108  named=63   nativeInput=True   attachFailure=False
  FOUND: ControlType.Button|Copy Debug Info Split pane right Split pane down
  FOUND: ControlType.Button|Split pane right
  inventory count=2
```

**The button my probe sees is the button the driver's fallback found and clicked.** Two producers, different code
paths, same element, same uniqueness, same window — which is what makes the pass-20 disagreement conclusive
rather than merely suggestive: **the element was always present; only the search scope was wrong.**

## The scenario ADVANCED — a new, later failure

```
inner verdict = BLOCKED
inner code    = BARRIER_ACK_TIMEOUT
inner message = product did not settle barrier split-create receipt[0] within 9000ms
```

Timeline: `… → pane-session-bound → owned-windows-enumerated → click-split-affordance →` **(barrier
`split-create` never settled)**.

**Stated plainly: the split is now CLICKED, and the run stops one stage later** — at the product not settling the
`split-create` barrier receipt within 9 s. **This is the first time a split scenario has clicked its own split
affordance and advanced into its assertion sequence.** The blocker has moved from the driver's search scope to
the product's barrier settlement.

**Reported as a finding, not diagnosed further.** It is a different stage from the one I was dispatched to test,
and whether it is a 9 s budget that is too tight, a product-side settlement gap, or an environment effect needs
its own measurement rather than a guess.

## `settledBy` — the delta path again, agreeing with the probe

```
"settledBy":"inventory-delta"        my probe: inventory count = 2
```

Same as pass 20: the receipt path had nothing, the measured daemon delta settled it, and my independent probe
reads the same count.

## Carry-forward traps (twenty)

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
10. **A wedged task subsystem blocks every native run** - check a trivial task's output before believing a
    stalled run is a code defect.
11. **`isAdmin=True` from `WindowsPrincipal.IsInRole` is a false positive over SSH** - prove access by the
    operation, not the group claim.
12. **The service DACL, not the group claim, decides a restart**; and **`Start-Service` after a failed stop is a
    no-op, not a restart** - check the host process's PID and start time.
13. **Isolate a process-launch fault with a four-form matrix** - and before calling a long-lived process a hang,
    check its children.
14. **Verify each step's precondition, not a proxy for it** - and state which interpreter/version the test ran
    under.
15. **A task whose `cmd.exe` never runs line 1 is an INTERMITTENT stall** - localise it with a marker on the
    bat's first line, and repeat the run to tell intermittent from deterministic.
16. **A stall can be BURSTY; the retry budget must outlast the burst, not the average rate** - measure the SHAPE
    (consecutive stall runs), not just the rate.
17. **A number read from a DIRTY tree is a draft, not a decision** - cite the revision it came from, and check
    `git show <rev>:<path>`.
18. **Watching a file for CREATION is not watching it for CONTENT** - wait for the content you need
    (non-empty/parseable), bounded; never for mere appearance.

19. **NEW - stage from the COMMITTED BLOB, never `cp -R` from the working tree, when a lane may be mid-flight.**
This pass I checked my `cp -R` stage against `git archive` and it was clean, but the check only *proved* it
afterwards. Switching to `git archive <rev> scripts | tar -x` makes a working-tree leak **structurally
impossible** rather than merely detected. **On a shared tree with concurrent lanes, the archive is the only
staging method that cannot capture someone else's half-written edit.**

20. **NEW - a foreign process holding your link target is a reason to move your build, not to kill.** `LNK1104`
on `deps\ferryx_lib-<hash>.exe` means another process holds that exact file. Enumerate the holders, check their
**start time against your session**, and if they predate you they are not yours: **build into an isolated
`CARGO_TARGET_DIR` and pay the cold build.** Never kill what you cannot prove is yours - and record the holder's
PID and start time so a future session can tell whether it is still there.

## What remains unmeasured, with the reason

| item | status | reason |
|---|---|---|
| the **suite at `89a363a0`** (expected 87) | **NOT RUN** | the host's SSH refused connections after the scenario run (see below) |
| **H-17a** (exact `pane_liveness_contract` filter) | **NOT GREEN** | the first attempt was blocked by the foreign `LNK1104`; the isolated-target retry **also exited 101** and I had not yet read its error when SSH was lost |
| **H-14 / H-15** (the mutation holes) | **NOT STARTED** | queued by the dispatch for the next pass; both need a build host |
| the `split-create` **`BARRIER_ACK_TIMEOUT`** | **unmeasured cause** | reported as a finding; needs its own measurement, not a guess |
| a **capture** for the image reader | **none produced** | no screenshot step reached, so **no recognition verdict is claimed**; the reader must preserve the `_` separator when one exists |
| the **burst-length distribution** | **3 samples: 1, 15, 26+** | the 26-attempt burst was still running when I stopped it, so the maximum is a lower bound |

## Teardown, and an honest gap

| Resource | Action | Receipt |
|---|---|---|
| my run21 (`14744c8a`) launcher, its node, its hung wrapper, and its stuck task `cmd.exe` | killed by **exact PID** with identity checks (`run21` / `pane-liveness` / `ev21` in the command line); its scheduled task ended and deleted by name | `IDENTITY_OK` ×5, `REMAINING_RUN21` → 1, then killed |
| my probe tasks | ended + deleted by name | `ownTasksLeft=0` at the last reading |
| **FOREIGN holders - REPORTED, NOT KILLED** | `PID 24632` (`ferryx_lib-edbc0e6372dc8389.exe`, started **2026-10-04T23:20:14**), `task3-product-gate-01a100ad` ×2 (since 10-03), `ferryx-baseline-herdr-50c5ca99` ×5 (since 10-04) | enumerated with PID + start time |
| the Schedule service | not modified | - |
| the **candidate tree** | never edited by me | `HEAD = 89a363a0` |
| **HOST CONNECTIVITY** | **SSH to `100.126.171.58` is currently REFUSED** (`kex_exchange_identification: read: Connection reset by peer`) | **so the final host-side teardown verification could NOT be completed this pass** |

**I am not claiming a clean final teardown, because I could not verify it.** The last confirmed host readings were
`ownTasksLeft=0`, `REMAINING_MINE=0`, `PORT_5173=FREE`, and the host profile hash unchanged. **The connection
reset is almost certainly load** - the host was running my scenario attempts, a cold cargo build into an isolated
target dir, and other sessions' work simultaneously - but **I did not verify that either**, so the residual state
after SSH was lost is **unknown and stated as unknown.**
