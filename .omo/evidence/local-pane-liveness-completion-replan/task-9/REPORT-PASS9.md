# Task 9 — PASS 9 (in progress): `757f8414`

Verifier: sole remote verifier. Date: 2026-10-05 (+0900).
HEAD **`757f8414`** (`757f8414c32edbef625c56cea17515ac63c69cb0`), tree clean, unchanged by this pass.
`48b4ed93..757f8414` = **3 files, all `scripts/`, 0 Rust** → the warm pass-4 binary
(`6ad63c5afdcbb8348fccd0253d0cdb0066fc054bda99b41387b5bc5fbeb5b042`) is this revision's binary; **no rebuild**.

## 1. Runner vitest on linux — PASS 64/64 (after a TMPDIR workaround)

| # | Command | Raw exit | Selected | Asserted line | Verdict |
|---|---|---|---|---|---|
| 1 | `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs` | **1** | **no tests** | `Error: Unknown system error -122: Unknown system error -122, write` / `Tests no tests` | **DISCARDED — environment** |
| 2 | same, with `TMPDIR=/home/indo/ferryx-pane-completion/vitest-tmp` | **0** | **64** | `Test Files 1 passed (1)` / `Tests 64 passed (64)` | **PASS** |

**Run 1 was an environment failure, not a code failure:** `Unknown system error -122` is **`EDQUOT` (disk quota
exceeded)**, and `/tmp` on this host is a **16 GB tmpfs at 80 % (13 GB used, 3.13 GB free)**. The big consumers
were **not mine** — `/tmp/inferx-plan-wt` 7.6 G, `/tmp/parity-resumed.*`, `/tmp/pt-media-diagnostic.*`,
`/tmp/inferx-task8-target-fix`, `/tmp/inferx-plan` — so **per the new rule I deleted nothing I could not prove
was mine** and instead redirected `TMPDIR` for the run. Run 2 then passed cleanly with the same
`TESTFILE_SHA=0a1ad75f970cc6870e63a5d02d95588975d8ad8383e2b7ebec06e633aec03bc1`, `DECLARED=64`.

**Recommendation for future passes: set `TMPDIR` from the start**, so a host-wide tmpfs squeeze cannot
masquerade as a code failure.

## 2. Windows scenarios at `757f8414` — staging

linux **13/13 `STAGED_OK`**, Windows **13/13 `STAGED_OK`**, `STAGED_BAD_COUNT=0`, both hosts at test-file
`0a1ad75f…`, `ui/dist/index.html` present, warm binary present.

## 3. Scenario results

| Scenario | Raw exit | Typed code | Pane click | Verdict |
|---|---|---|---|---|
| `split-happy` | **4** | **`PANE_BINDING_UNBOUND`** | **FOUND + CLICKED** (1 actionable) | **BLOCKED** |
| `split-attach-stall` | **4** | `PANE_AFFORDANCE_NOT_FOUND` | not found | **BLOCKED** |
| `split-cancel` | **4** | `PANE_AFFORDANCE_NOT_FOUND` | not found | **BLOCKED** |

`cleanupGate`: **`ok=true`, `directoriesRemoved=true`, `processesReaped=true`** on all three;
`NODE_HANG_OBSERVED=NO_TERMINATED`; `.exit` content `4`. **No capture exists** (`IMAGE_COUNT=0`) → **no
recognition claim**.

### 3.1 THE FIRST REAL CANDIDATE LIST (split-happy)

```json
{"action":"click-pane-affordance","selectorNames":["New Terminal"],"selectorAutomationIds":[],
 "assertedUniqueEnabled":true,"code":null,"interactive":true,"sessionId":1,
 "windowsSearchedCount":2,"candidateCount":1,"actionableCount":1,
 "candidates":[{"windowHwnd":15796732,"name":"New Terminal","controlType":"ControlType.Button",
                "automationId":"","enabled":true,"offscreen":false,"rectEmpty":false,"rect":"910,715,156,36"}],
 "chosen":{"windowHwnd":15796732,"name":"New Terminal","controlType":"ControlType.Button",
           "automationId":"","enabled":true,"offscreen":false,"rectEmpty":false,"rect":"910,715,156,36"},
 "warmElements":16,"warmAttempts":1,"warmElapsedMs":29}
```

**The affordance is findable, unique, and clickable.** Then:

```
PANE_BINDING_UNBOUND: no presentation receipt named a session other than the 1 fixture session(s) within
8000ms, so the UI pane step created no observable pane (observed sessions: [])
```

### 3.2 The asymmetry that matters (launch-relative timing)

| Scenario | fixture-setup | windows-enumerated | pane click | warm attempts | warm elapsed | outcome |
|---|---|---|---|---|---|---|
| `split-happy` | 1481 ms | 2908 ms | **3387 ms** | **1** | **29 ms** | found + clicked |
| `split-attach-stall` | 1372 ms | 2764 ms | 7303 ms | **33** | **4087 ms** | not found |
| `split-cancel` | 1463 ms | 2864 ms | 7332 ms | **33** | **4053 ms** | not found |

**All three saw `warmElements: 16` on their first warm probe.** split-happy then found the button on its very
next query 29 ms later; the other two retried for the full 4 s budget and never saw the DOM. See §4 for the
latency measurement that decides the mechanism.

---

## 4. ACTIVATION LATENCY — the measurement the dispatch asked for

Direct measurement (`latency-probe.ps1`), in the interactive session, `ui/dist` served, the app launched
fresh for each of four **first-attach delays**; at each delay: attach once (one `FindAll(Descendants,
TrueCondition)` per visible window root), then poll the tree every 250 ms until it flips from
Chromium-internal to DOM-bearing.

| first-attach delay | launch->attach | **first probe** | **DOM appears at (from launch)** | **latency after attach** | polls |
|---|---|---|---|---|---|
| 1000 ms | 1287 ms | **16 elements / 2 named** | 1640 ms | **+353 ms** | 2 |
| 3000 ms | 3041 ms | **16 elements / 2 named** | 3358 ms | **+317 ms** | 2 |
| 6000 ms | 6051 ms | **16 elements / 2 named** | 6388 ms | **+337 ms** | 2 |
| 12000 ms | 12045 ms | **16 elements / 2 named** | 12362 ms | **+317 ms** | 2 |

Poll traces (the flip is clean and total):

```
delay  1000ms: 1287ms:16/2   1640ms:81/38 DOM
delay  3000ms: 3041ms:16/2   3358ms:110/65 DOM
delay  6000ms: 6051ms:16/2   6387ms:110/65 DOM
delay 12000ms: 12045ms:16/2 12362ms:110/65 DOM
```

`session=1`, `pollMs=250`, `serverUp=true`, `appAliveAtEnd=true` on all four.

### 4.1 Which of (a)/(b)/(c) — **ANSWER: (a)**

**One attach, then a wait of ~400 ms, is sufficient and deterministic.** Evidence:

- The DOM **never** exists at the first attach — **all four delays** show exactly `16 elements / 2 named`,
  the Chromium-internal-only tree.
- The DOM appears **317–353 ms after the attach**, a spread of only **36 ms** across delays spanning 1 s to
  12 s from launch. Activation is therefore **not** a function of how long the app has been running, so
  attaching earlier buys nothing — that **rules out (b)**.
- Four out of four runs produced the DOM within ~350 ms of the attach, so it is **not nondeterministic** in
  the (c) sense. The residual 81-vs-110 element variance is the app's own toast/sidebar state, not
  activation flakiness.
- Therefore **the warm budget (4 s) is ~12x the measured requirement** and is not too small.

**Correction to my own pass-8 reading.** I reported the activation latency as 'on the order of several
seconds', inferred from the t+12 s -> t+18 s gap. **That was wrong**: the two queries were 6 s apart, so they
merely *straddled* a ~320 ms event; the gap told us nothing about its magnitude. The pass-8 conclusion (lazy
activation) stands, but its *timing* was misread and is corrected here.

### 4.2 Why two of three runs burned the full 4 s — the defect, located in source

The dispatch describes the emitter as retrying the warm attach. **Reading the implementation, it does not.**
`uiaWarmLines` (`native-driver.mjs:313-326`) is **a single pass with no wait**:

```js
function uiaWarmLines(windowVar, warmBudgetMs, warmIntervalMs) {
  return [
    `$warmBudgetMs = ${warmBudgetMs};`, `$warmIntervalMs = ${warmIntervalMs};`,
    '$warmElements = 0;',
    `foreach ($warmWindow in ${windowVar}) {`,
    '  $warmRoot = ...::FromHandle(...$warmWindow.hwnd);',
    '  if ($warmRoot -eq $null) { continue };',
    '  $warmNodes = $warmRoot.FindAll(Descendants, TrueCondition);',   // the attach — ONCE
    '  $warmElements = $warmElements + $warmNodes.Count;',
    '}',
    '$diag.warmElements = $warmElements;',                            // recorded, then the real query runs
  ];
}
```

`$warmBudgetMs` / `$warmIntervalMs` are **declared and then unused** in this helper. The retry loop that does
exist (`:959-985`) retries the **scope search under the affordance *name* condition**
(`$node.FindAll(..., $condition)` at `:973`), **not** the `TrueCondition` attach. That matches the telemetry:

- `warmElements: 16` on all three runs — the single attach pass saw the pre-activation tree.
- `warmAttempts: 33` / `warmElapsedMs: 4087` on two runs — 33 retries of the *name* search over 4 s, each
  returning 0 on an unbuilt tree, and **a name-condition `FindAll` does not re-trigger Chromium's build**.
- `warmAttempts: 1` / `warmElapsedMs: 29` on `split-happy` — its scope search matched on the first attempt,
  i.e. that run's tree was already built when the probe ran.

**The fix implied by the measurement:** re-issue the **`TrueCondition` attach** in a bounded loop until the
element count rises past the Chromium-internal baseline (or ~400–500 ms elapses), *then* run the real query.
The existing 4 s budget is ample; it needs to wrap the attach rather than the name search.

### 4.3 The `split-happy` asymmetry — what I can and cannot say

`split-happy` found `New Terminal` on `warmAttempts: 1` / 29 ms, while the other two failed after 33 attempts.
**I could not isolate which earlier step in `split-happy`'s sequence had already attached and let the tree
build**, so I will not assert one. What the evidence rules out: it is **not** 'the tree was warm from launch'
(all three saw `warmElements: 16` at the attach) and **not** a longer run (all three reached the pane step
within ~3–7 s of launch, and the measurement shows launch-relative time is irrelevant). The most likely
remaining explanation — an earlier `TrueCondition` attach in that run's own path, followed by >= ~320 ms
before the pane probe — is **a hypothesis I did not verify**; it is testable by instrumenting the action
timeline for the first `warmElements > 16` event.

---

## 5. Residual classification

| Item | Class | Evidence |
|---|---|---|
| Activation latency | **MEASURED: ~317–353 ms after the first attach, deterministic, launch-independent** | §4 |
| `PANE_AFFORDANCE_NOT_FOUND` (2 of 3) | **candidate-caused, scripts lane** — the warm attach is a single no-wait pass; the retry loop retries the name search | §4.2 |
| `split-happy` found the affordance | **PASS on the search** — first real candidate list: unique, enabled, on-window | §3.1 |
| `PANE_BINDING_UNBOUND` (`split-happy`) | **candidate-caused, scripts lane — the new frontier**; the click happened, no pane binding followed, `observed sessions: []` | §3.1 |
| vitest run 1 (`no tests`, errno -122) | **environment — `/tmp` tmpfs quota**; run 2 with `TMPDIR` = 64/64 | §1 |
| `cleanupGate` on all three | **PASS** — `ok=true`, dirs removed, procs reaped, `NO_TERMINATED`, port 5173 free after | §3, §6 |
| Image-reader lane | **no capture exists (`IMAGE_COUNT=0`, no `screencapture` action) -> no recognition claim** | §3 |
| `externally-stopped` / `adopted` | **structurally-not-run** — truthful `fixture-setup` failure remains expected | author + source |

---

## 6. Cleanup receipts

| Resource | Teardown | Receipt |
|---|---|---|
| three scenario wrappers + their trees | exited on their own exit files | `NODE_HANG_OBSERVED=NO_TERMINATED`; `TASK_OWNED_ALIVE_COUNT=0` |
| frontend listeners | closed by the runner's own cleanup pass | `PORT_5173_AFTER=FREE` on all three |
| isolation roots | removed | `ISO_EXISTS_AFTER=False` on all three |
| latency-probe app instances + its server | killed by **exact PID recorded at spawn** (`$appPid`, `$srvPid`), four launches | `PORT_5173=FREE`, no probe processes left |
| **`/tmp` consumers** | **NOT touched** — `/tmp/inferx-plan-wt` (7.6 G), `/tmp/parity-resumed.*`, `/tmp/pt-media-diagnostic.*`, `/tmp/inferx-task8-target-fix`, `/tmp/inferx-plan` are **not mine** | refused to delete, per the standing rule |
| **candidate tree** | **never edited** | clean at `757f8414` |
| foreign trees/processes | **untouched** | — |
