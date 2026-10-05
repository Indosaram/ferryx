# REPORT — PASS 21 (harness defects)

Two harness defects fixed, authoring only. **Nothing was compiled, executed, or run** — no build, no
test, no LSP, no PowerShell, no scheduled task, no product, no GUI, no commit, no remote action. Every
claim below is from reading the files on disk (`/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`,
HEAD `cfb4374b`, branch `work/local-pane-liveness-completion-foundation`).

Inputs read first: `REPORT-PASS20-verifier.md` (348 lines, eighteen traps) and `win-pass20/` (11 files),
plus `notes/facts/ferryx-pane-liveness-task9-pass20-first-native-verdict-and-two-harness-defects-2026-10-05.md`.

## Scope of the change

| file | lines | what |
|---|---|---|
| `scripts/lib/qa-scenarios/native-driver.mjs` | 883, 990–1010, 1018–1060, 1071–1104, 1167–1175, 1205–1208, 1239–1242 | defect 1: window-root fallback + pooled candidates + scope evidence |
| `scripts/lib/qa-scenarios/windows-interactive.mjs` | 422–494, 509–529, 636–641, 718–737, 330–334 | defect 2: content wait + typed detail + marker text enrichment |
| `scripts/lib/qa-scenarios/common-harness.mjs` | 1481–1528 | defect 2: `waitForFile` acceptance predicate (opt-in) |
| `scripts/qa/pane-liveness.test.mjs` | 1975–2062 | defect 1 tests (2) |
| `scripts/qa/pane-liveness-delegation-retry.test.mjs` | 21–27, 665–746 | defect 2 tests (5) + header count |
| `scripts/qa/pane-liveness-vitest.config.mjs` | 10–17 | comment counts only; the gate command and argv are untouched |

`git status --porcelain` after the work: exactly those six files, all inside the allowed scope. No
`src-tauri/**`, no `ui/**`, no scenario argv, no budget value, no frozen assertion.

---

## DEFECT 1 — the split search failed on focus resolution instead of falling back to the window

### The exact change

`buildWindowsSplitRightScript` (the PowerShell the Windows split probe runs) previously did:

```
'$diag.focusedFound = [bool]($focused -ne $null);',
"if ($focused -eq $null) { Fail 'SPLIT_RIGHT_NOT_FOUND' 'no focused pane could be identified inside the owned window, so no affordance was clicked' }",
```

so with `focusedFound:false` the probe **failed before the scope walk** and the search never ran — which
is exactly the pass-20 shape (`focusedFound:false`, `scopeDepth:-1`, `scopeIsWindowRoot:false`) while the
verifier's independent UIA dump measured `ControlType.Button | Split pane right` **present in the same
108-element window**.

Replaced (native-driver.mjs:990–1005) by a **scope-origin declaration** — the search always runs:

```
"if ($focused -eq $null) { $diag.scopeOrigin = 'window-root' } else { $diag.scopeOrigin = 'focused-pane' }",
```

and the warm re-query loop now has two branches (native-driver.mjs:1018–1060):

- **focus resolved** — the existing pass-1 (walk up from the focused element) / pass-2 (each remaining
  window from its root) ancestor walk, byte-for-byte the old search except that the match now also records
  `$scopeRoots.Add($node)` and `$diag.scopeOrigin = $scopeOriginForNode` (`'focused-pane'` for pass 1,
  `'window-root'` for pass 2 — i.e. which scope actually produced it).
- **focus unresolved** — the fallback: `foreach ($window in $searched)` (the multi-window enumeration that
  already exists and that the warm block already attaches), each visible owned window's root is searched
  with the **same** `$condition`; every root that currently exposes the bound name is added to
  `$scopeRoots`, and the first one is recorded as `$scope` / `scopeDepth 0` / `scopeIsWindowRoot $true` /
  `matchedWindowHwnd`.

Candidates are then **pooled over the whole scope set** (native-driver.mjs:1071–1084):

```
'$items = New-Object System.Collections.ArrayList;',
'foreach ($scopeNode in $scopeRoots) {',
'  $nodeItems = $scopeNode.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition);',
'  for ($j = 0; $j -lt $nodeItems.Count; $j = $j + 1) {',
'    $items.Add($nodeItems.Item($j)) | Out-Null;',
```

`$items` stays the flat element list that the candidate loop, `$diag.candidateCount = $items.Count` and
`$chosen = $items.Item([int]$actionable[0].index)` all address by index — so the existing pass-4
ambiguity rule now decides **across the searched scope**, never per window, and the focused path's
behaviour is unchanged (one node ⇒ identical indices).

The typed codes and their order are untouched: zero pooled matches → `SPLIT_RIGHT_NOT_FOUND` (+ the
bounded `BuildInventory` on the walk's not-found path, still emitted before its Fail);
`actionable > 1` → `SPLIT_RIGHT_NOT_UNIQUE`; present but not actionable → `SPLIT_RIGHT_DISABLED`; exactly
one → the single `InvokePattern.Invoke()` click. `NO_OWNED_WINDOW` is unchanged.

### Why the fallback cannot invent a match

- It searches the **same** UIA condition (the exact bound name / automation-id `PropertyCondition`, never
  a substring filter) over real `AutomationElement` roots obtained from the enumerated window handles.
- It requires **exactly one actionable** match in the pooled candidate list; `0` is still
  `SPLIT_RIGHT_NOT_FOUND`, `>1` is still `SPLIT_RIGHT_NOT_UNIQUE` with the **full candidate list**
  (`$diag.candidates` = every pooled candidate with name, control type, automation id, enabled, offscreen,
  rect, rectEmpty and in-window). Ambiguity is never resolved by taking the first match, and the click
  still addresses `$items.Item(index)` — the actionable element itself, not "the first found".
- Nothing is fabricated: the clicked element comes from `FindAll` on a live root, and the click is the
  same single-statement `Invoke` in the same `try`.
- Fail-closed is preserved end to end: if the pooled list were ever empty the probe hits
  `if ($items.Count -eq 0) { Fail 'SPLIT_RIGHT_NOT_FOUND' ... }`, never a click.

### The honest possibility, recorded rather than hidden

If the app renders **one pane toolbar per pane leaf**, a single-pane window holds exactly **one**
`Split pane right` button and the fallback finds one → click. If it finds **several**, the correct answer
is `SPLIT_RIGHT_NOT_UNIQUE` and the scenario genuinely needs pane scoping — the fallback is then doing its
job by *reporting* that, not by passing. The source comment says this in place
(native-driver.mjs:995–1004), and the next pass can read it straight from the evidence.

### Candidate / scope evidence recorded

- `$diag.scopeOrigin` (`'focused-pane'` | `'window-root'`) — new field, in the emitted payload, in
  `classifyWindowsSplitRight`'s `measured` (native-driver.mjs:1172) and in the driver's evidence action:
  `scope: { focusedFound, focusSource, origin, depth, isWindowRoot }` (native-driver.mjs:1242).
- `scopeDepth` / `scopeIsWindowRoot` / `matchedWindowHwnd` — unchanged semantics; on the fallback they read
  `0` / `true` / the first matching window when something was found, and stay `-1` / `false` / `null` when
  nothing was (the next pass can distinguish "the fallback ran and found nothing" from "the search never
  ran", which the pass-20 signature could not).
- `windowSearchDepths` now carries `matchCount` per window on the fallback path (depth `0`,
  `containsFocus $false`) — how many matches each searched window root contributed.
- `candidates` / `candidateCount` / `actionableCount` / `chosen` — unchanged fields, now pooled.
- The three in-script verdict details now name the scope that actually searched:
  `"the searched $($diag.scopeOrigin) scope contains ..."` (native-driver.mjs:1101–1103), and the
  classifier's not-found detail says `the searched window scope` when `scopeOrigin === 'window-root'`
  (native-driver.mjs:1205–1208). A window-wide run can no longer be reported as a focused-pane miss.

### Tests added (2, in the frozen suite file)

- `pane-liveness.test.mjs:1975` — `split-affordance search falls back to the window roots when no focused
  pane can be resolved`: the old hard `Fail` on unresolved focus is gone; the scope-origin line, the
  fallback root search, the pooling and its ordering (pool → count → NOT_UNIQUE → DISABLED) are asserted,
  the inventory-before-Fail contract and all four typed codes survive, and the chosen element is still
  addressed by index.
- `pane-liveness.test.mjs:2022` — `the window-root fallback reports a real match or a real ambiguity, never
  an invented one`: `SPLIT_CLICKED` with `focusedFound:false` + `scopeOrigin:'window-root'` is a click;
  two pooled actionable matches stay `SPLIT_RIGHT_NOT_UNIQUE` with both candidates and no `chosen`;
  nothing found stays `SPLIT_RIGHT_NOT_FOUND` and records `scopeOrigin:'window-root'` / `scopeDepth 0` /
  `searched window scope`; a pre-field probe reports `scopeOrigin: null`.

### Not changed, and why (checked, not assumed)

`buildWindowsNewPaneScript` does **not** share this defect: it searches window-wide from the start (its
first `Fail` is `PANE_AFFORDANCE_NOT_FOUND` on `$diag.candidateCount -eq 0`, with no focus resolution at
all), which is consistent with the pane step passing in pass 20. Left untouched.

---

## DEFECT 2 — `awaitExitFile` read the exit file before it had content

### The exact change

`awaitExitFile` (windows-interactive.mjs) waited for the file to **appear** and read it **once**. Measured
cost: the outer run reported `unreadable exit code ""` while `relaunch.exit` held `34 20 0D 0A` = `"4 \r\n"`
— four parseable bytes — and **threw away a completed inner run carrying the first native verdict**.

New shape (windows-interactive.mjs:422–494), with the constants at 436–442:

```
const EXIT_FILE_CONTENT_REARM_MS = 250;                                        // 436
const EXIT_FILE_CONTENT_BUDGET_MS = BUDGETS.interactiveRelaunchSequenceMs;      // 439
const EXIT_FILE_PAYLOAD_ACCEPTED = text => parseRelaunchExitFile(text) !== null; // 442
export async function awaitExitFile(exitPath, timeoutMs = EXIT_FILE_CONTENT_BUDGET_MS) {   // 444
```

- **The wait is now for CONTENT.** `waitForFile` receives `{ accept: EXIT_FILE_PAYLOAD_ACCEPTED }`: its
  watcher stays armed across the create event and resolves on the write that satisfies the predicate. A
  `""` or half-written read is "not yet written", never a verdict.
- **Bounded, event-driven, no fixed sleep.** Each armed segment is
  `Math.min(remaining, EXIT_FILE_CONTENT_REARM_MS)`; on a segment deadline the loop re-arms while the
  caller's deadline (the attempt's remaining sequence budget) still has room. There is no unbounded loop
  and no sleep that decides anything — the only authority is the deadline.
- **Appearance is tracked independently of payload**, so the deadline still distinguishes the two typed
  outcomes: the file never appeared → `timedOut: true` (the caller's unchanged `NO_EXIT_FILE`); the file
  appeared without ever carrying a parseable payload → `timedOut: false, empty: true` (the caller's
  `UNREADABLE_EXIT`, now with the evidence the pass-20 detail lacked).
- **The typed detail is never a bare `""`** (windows-interactive.mjs:735):
  `delegated run wrote an unreadable exit code: path=<exitPath> bytesObserved=<n> content=<json> after the
  <budget>ms exit-file content budget expired (the file appeared, so the run reached its exit line; a
  0-byte observation is a create-before-write read, not a verdict about the run)` — plus
  `ledger.exitFileBytes` / `ledger.exitFileContentBudgetMs` (windows-interactive.mjs:639–641, 733–734) and
  `exitFileBytes` in the ledger summary projection (330–334). The `NO_EXIT_FILE` detail also records
  `observed bytes=<n>`.
- `awaitExitFile` is now exported so the behaviour can be tested directly against real files (it has one
  production call site, windows-interactive.mjs:718).

The shared helper (common-harness.mjs:1481–1528) gained an **opt-in** third parameter:
`waitForFile(path, stopPromise, options = {})` with `options.accept`. With no predicate the semantics are
exactly as before (resolve as soon as the file is readable), so every existing caller — barriers, the
entry marker, the ack waits — is unchanged; with a predicate the watcher keeps waiting for content
instead of resolving on an appearance that carries nothing yet.

### The read-once assumption, checked everywhere it appears in that file

| site | verdict |
|---|---|
| `awaitExitFile` (windows-interactive.mjs:444) | **the defect** — fixed as above |
| `awaitEntryMarker` (509–529) — the entry marker | **holds for the recorded TEXT, not for the verdict.** The marker's assertion is its *appearance* (the bat's first line reached its own redirect, which is what proves the process executed its first line), so the same two-step redirect can return `""` for a marker that *was* written. Turning that into "not appeared" would declare a stall, end the attempt's task and kill a cmd.exe that really did start — so `appeared` is untouched, and only the recorded text is re-read within one bounded `EXIT_FILE_CONTENT_REARM_MS` slice (never longer: a fast stall window matters more than a rich marker text). |
| `readText(plan.outPath)` / `readText(plan.errPath)` (720–721) | **not a defect, and now better ordered.** The bat runs the runner with `> out 2> err` on the line *before* `echo %ERRORLEVEL% > exit`, so by the time the exit file carries its payload both captures are complete. The fix strengthens this: the exit file is now accepted only on content, which is strictly after the runner line finished, where the old code accepted the file's mere creation. |
| `readWindowsRelaunchRecord` (791–795) | **not a defect.** `relaunch-record.json` is written by Node before the task is created, not by cmd.exe, so there is no create-vs-write race. |

### Tests added (5, in the delegation-retry suite)

- `pane-liveness-delegation-retry.test.mjs:665` — the file is created **empty** and then written with the
  measured 4-byte payload (`'4 \r\n'`); the wait must return the parsed `4` with `bytes === 4`, and
  `parseRelaunchExitFile('')` must stay `null`.
- `:685` — created but never written: `timedOut:false`, `empty:true`, `bytes:0`, and the path + deadline are
  named (the pass-20 failure carried a bare `""`).
- `:703` — never appears: still the typed no-exit-file outcome (`timedOut:true`, `bytes:0`, `text:null`).
- `:715` — a half-written payload (`'x'`) is not accepted; the completed `'0 \r\n'` is.
- `:730` — `waitForFile` keeps its creation-only semantics for callers that pass no predicate (an existing
  empty file still resolves to `''`), while a predicate that is not satisfied yet does **not** resolve.

These are event-driven and timing-safe: the watcher is armed synchronously before the test's write (the
first `await` in `awaitExitFile` evaluates `waitForFile`, whose executor arms the watch), and the 250 ms
re-arm is the safety net, so no test depends on a sleep or on winning a race.

---

## Counts, gate, and what was NOT run

- The canonical config is unchanged in command and argv:
  `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs`.
- Files and test counts: `scripts/qa/pane-liveness.test.mjs` **66** (was 64; +2 defect-1 tests) and
  `scripts/qa/pane-liveness-delegation-retry.test.mjs` **20** (was 15; +5 defect-2 tests) — **86** under
  the one frozen command (was 79). Counted from `^test(` in each file; the two header comments and the
  vitest-config comment were updated to say so.
- **Nothing was compiled or run.** No `cargo`, no `bun`, no `vitest`, no PowerShell, no scheduled task, no
  product launch, no GUI, no LSP server (the LSP probe available in this session is not installed for this
  worktree and was not used), no `git add`/`commit`. The verification performed is static: anchor counts
  (each patch anchor matched exactly once in the pre-edit file), the generated PowerShell array read back
  line by line after each edit (here-string header/terminator alone on their lines, one statement per
  line, no empty entries, `Emit;` last, braces balanced across the new `if/else` branch), and a review of
  the full `git diff`.
- One bug was caught by that read-back and fixed before finishing: `appeared` was initially only set on an
  *accepted* read, which would have reported a created-but-empty exit file as "no exit file" instead of
  the typed unreadable-exit failure. The final observation + per-segment presence check
  (windows-interactive.mjs:463–484) fixed it, and test `:685` pins it.

## What the next pass should read first

`actions.jsonl`'s `click-split-affordance` entry: `scope.origin` and `scope.depth` say whether the click
came from the focused pane (`focused-pane`, depth > 0) or from the window-root fallback
(`window-root`, depth 0), `candidates`/`actionableCount`/`chosen` say what was pooled, and
`windowSearchDepths[].matchCount` says what each window root contributed. If `origin` is `window-root` and
the verdict is `SPLIT_RIGHT_NOT_UNIQUE`, the run is telling us the app exposes the affordance per pane
leaf and the scenario needs real pane scoping — that is a product-level answer, not a harness defect.
