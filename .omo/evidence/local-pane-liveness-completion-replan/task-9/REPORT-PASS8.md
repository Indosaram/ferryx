# Task 9 — PASS 8 consolidated report (`48b4ed93`)

Verifier: sole remote verifier (pass 8). Date: 2026-10-05 (+0900).
Candidate: `C=/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`,
HEAD **`48b4ed93`** (`48b4ed9341e1a260e12f6d9d2cbf912d6a75ac6f`), tree **clean**, unchanged by this pass.
Chain: `48b4ed93` (frontend + pane) → `beb80b72` (suite green + charset) → `42fba06f` → `7a452a77` → `120bc965` → … → `d82b35e4`.

## HEADLINE

**The frontend is served and the charset fix works — but the pane step cannot see the app UI, and I found the
exact mechanism.** Chromium/WebView2 builds its accessibility tree **lazily on the first UIA query**: the first
enumeration sees **16 Chromium-internal nodes and no DOM at all**, and every subsequent query sees **93 elements
/ 56 named**, including the `New Terminal` button the pane step needs. **The runner probes once, so it reads
the pre-activation tree.** That is a bounded harness fix, and it is the last thing between this effort and a
real native verdict.

| Gate | Verdict |
|---|---|
| Runner vitest on linux | **FAIL — 63/64, exit 1** (one test asserts an unindented string the builder emits indented; the invoke is functionally present) |
| **64 vs 65** | **64 is correct** — 52 → 64, **exactly 12** added tests, 0 removed (the lane's "13" was the miscount) |
| Windows binary | **no rebuild needed** — `42fba06f..48b4ed93` is **scripts-only** (0 Rust files) |
| `frontend.served` before `launch.binary` | **PASS** — precedes it on all three runs, with the app root verified |
| `title`/`className` charset fix | **PASS** — now `"Ferryx"` / `"Tauri Window"` (was `"F"`/`"T"`) |
| Pane step | **FAIL — `PANE_AFFORDANCE_NOT_FOUND`, `candidateCount: 0`** — **cause found: lazy accessibility activation** |
| `Split pane right` findable? | **NOT YET — no pane exists**; but `New Terminal` **is** findable after activation |
| Image reader | **no capture exists → no recognition verdict claimed** |
| Cleanup gate | **PASS** — `ok=true`, `directoriesRemoved=true`, no holders, no node hang |

---

## 1. Runner vitest on linux (omaki, `indo@100.91.254.71`)

| # | Exact command | Raw exit | Selected | Asserted line | Verdict |
|---|---|---|---|---|---|
| 1 | `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs` | **1** | **64** | `Test Files 1 failed (1)` / `Tests 1 failed \| 63 passed (64)` | **FAIL — 1 test** |

Staged first and hash-verified (`TESTFILE_SHA=8fef9711caa38c9664c5feed53391691dd9ba20e6b1faf16cdb1cadd1c5d8451`, `DECLARED=64`).

### 1.1 64 vs 65 — settled

Counted from the committed blobs, not from prose:

```
declared at 42fba06f = 52
declared at 48b4ed93 = 64
delta = 12        (12 added test names, 0 removed)
```

**64 is the real number and 12 is the real delta** — the lead's count was right, the lane's "13" was not.

### 1.2 The failure, verbatim

```
 FAIL  scripts/qa/pane-liveness.test.mjs > the pane affordance is searched by its exact accessible name and only one actionable match is clicked
AssertionError: expected [ …(83) ] to include '$invoke.Invoke();'
 ❯ scripts/qa/pane-liveness.test.mjs:2131:17
    2129|   expect(lines).toContain("$windowTitles = @('F');");
    2130|   expect(lines).toContain('$condition = $conditionName0;');
    2131|   expect(lines).toContain('$invoke.Invoke();');
```

**Cause, verified functionally** by generating the script with the builder itself:

```
buildWindowsNewPaneScript(4242, { windows: [...] })
  exact unindented '$invoke.Invoke();'  -> false
  lines containing 'Invoke()'           -> ["    $invoke.Invoke();"]     (4-space indent, inside a try)
  lines containing 'GetCurrentPattern'  -> ["    $invoke = $chosen.GetCurrentPattern([…]::Pattern);"]
  total lines = 83
```

**The invoke is present and correct** — it is indented because it sits inside a `try`, and the split builder
(`native-driver.mjs:937`) emits the same indented form. So this is a **test-expectation bug, not a functional
defect**: the assertion should match the indented line (or use a trim/includes check). **Scripts lane; does not
affect the search or the click.**

---

## 2. Windows: staging

| Check | Result |
|---|---|
| `42fba06f..48b4ed93` | 7 files, all `scripts/` — **0 under `src-tauri/`** |
| Consequence | the warm pass-4 binary (`6ad63c5a…`) **is** this revision's binary — **no rebuild performed** |
| Staging | linux **13/13 `STAGED_OK`**, Windows **13/13 `STAGED_OK`**, `STAGED_BAD_COUNT=0`; both hosts at test-file `8fef9711…`; new modules present; `ui/dist/index.html` present |
| Disk | 19.65 GB free on entry; **19.62 GB** at close |

---

## 3. The three Windows scenarios

| Scenario | Raw exit | Typed code | `fixture-setup` | Pane step | Click | Assertions | Verdict |
|---|---|---|---|---|---|---|---|
| `split-happy` | **4** | `PANE_AFFORDANCE_NOT_FOUND` | **YES** | **ran, found 0** | not reached | **not reached** | **BLOCKED** |
| `split-attach-stall` | **4** | `PANE_AFFORDANCE_NOT_FOUND` | **YES** | **ran, found 0** | not reached | **not reached** | **BLOCKED** |
| `split-cancel` | **4** | `PANE_AFFORDANCE_NOT_FOUND` | **YES** | **ran, found 0** | not reached | **not reached** | **BLOCKED** |

Action sequence (all three): `barriers.prearmed` → `windows-interactive-admission` →
`windows-interactive-relaunch` → **`frontend.served`** → `launch.binary` → `fixture-setup` → `powershell` →
`owned-window` → `powershell` → `owned-windows-enumerated` → `powershell` → **`click-pane-affordance`**.

### 3.1 `frontend.served` precedes `launch.binary`, and the document carries the app root

Verbatim (identical shape on all three):

```json
{"action":"frontend.served","host":"127.0.0.1","port":5173,"url":"http://127.0.0.1:5173/",
 "distDir":"…\\source-21dea3c0\\ui\\dist","indexPath":"…\\ui\\dist\\index.html",
 "indexBytes":3389,"rootMarker":"id=\"root\"","declaredDevUrl":"http://127.0.0.1:5173",
 "route":"static-ui-dist",
 "rejectsDevServerRoute":"bun scripts/dev-frontend.mjs rebuilds and watches the tree mid-run (HMR), which is not a deterministic QA fixture"}
```

`frontend.served` is recorded **before** `launch.binary` in all three action logs, and the served document was
verified to carry the app root (`rootMarker: id="root"`, `indexBytes: 3389` — matching what I measured
independently: `HTTP=200 bytes=3389 HAS_ROOT_DIV=True`). **PASS.**

### 3.2 `title`/`className` now report real strings (the queued confirmation read)

```
windowsSearched=[{"hwnd":7408664,"title":"Ferryx","className":"Tauri Window","rootOffscreen":false},
                 {"hwnd":2493534,"title":"","className":"Tao Thread Event Target","rootOffscreen":false}]
```

**Confirmed:** `Ferryx` and `Tauri Window` (was `"F"` / `"T"`). The `CharSet = CharSet.Unicode` fix works. (A
third window now appears as `PseudoConsoleWindow` in some runs — the app's window set varies, which the
multi-window enumeration handles.)

### 3.3 The pane step's typed failure, verbatim

```
PANE_AFFORDANCE_NOT_FOUND: the "New Terminal" affordance could not be identified by its exact accessible name:
{"selectorNames":["New Terminal"],"selectorAutomationIds":[],"interactive":true,"sessionId":1,
 "windowsSearched":[{"hwnd":7408664,"title":"Ferryx","className":"Tauri Window","rootOffscreen":false},
                    {"hwnd":2493534,"title":"","className":"Tao Thread Event Target","rootOffscreen":false}],
 "windowsSearchedCount":2,"candidateCount":0,"actionableCount":0,"candidates":[],"chosen":null}
```

The window **is** found and **is** visible, and the app **is** served its UI — yet the search finds nothing.
§4 explains why.

---

## 4. ROOT CAUSE — Chromium activates its accessibility tree lazily, and the runner probes once

Bounded experiment (`activation-probe.ps1`), run **inside the interactive session** with the dist server
started by the same script, querying the tree three times:

| Query | Total elements | Named | `New Terminal` present | `Split pane right` present |
|---|---|---|---|---|
| **t+12 s — the FIRST query (the attach)** | **16** | **2** | **false** | false |
| t+18 s — second query | **93** | **56** | **true** | false |
| t+26 s — third query | **93** | **56** | **true** | false |

At t+12 s the named elements are only `"Ferryx"` and `"Ferryx - 웹 콘텐츠"`, and the 16 nodes are all
Chromium-internal (`WRY_WEBVIEW`, `Chrome_WidgetWin_1`, `BrowserRootView`, `NonClientView`,
`EmbeddedBrowserFrameView`, `BrowserView`, `SidebarContentsSplitView`, `MultiContentsView`,
`EmbeddedBrowserTabRootView`, `TopContainerOverlayView`, …) — **no `Document`/`RootWebArea` and no DOM at all.**
By t+18 s the same probe sees `Document id="RootWebArea"` and the full app UI: `Hide sidebar`, `Add project`,
`Inbox`, `Expand Strawberry`, `Settings`, `New Terminal`, `New Browser Tab`, `Collapse toolbar`,
`Notifications alt+T`, the Store toast, …

**Reading:** WebView2/Chromium builds its accessibility tree **on demand, triggered by the first UIA client
attaching**, and the tree is not ready at the instant of that first query. **The runner issues exactly one
enumeration per step**, so it reads the pre-activation tree and reports `candidateCount: 0`.

**This explains every prior observation coherently:**
- Pass 7's `withui` control saw **33 named app elements** — because the driver probe ran **first** (the attach)
  and the separate tree dump ran **seconds later** (the built tree). I attributed the difference to the served
  UI; the served UI was necessary but the *second* query was what made it visible.
- Pass 8's pane probe runs once → `candidateCount: 0`, even though the UI is loaded and `New Terminal` exists.
- It also explains why pass 7's baseline and `env` runs (error page) showed 13 named elements: that page's DOM is
  tiny and likely built within the first query's window.

**The fix is harness-side and bounded:** warm the tree before the real query — attach once, then re-query with a
bounded retry (or query, wait for the `RootWebArea` document to appear, then query again). **No product change
is needed**, which keeps the lead's decision intact.

### 4.1 The acceptance question — status

- **`New Terminal` IS findable** (`hasNewTerminal: true`) once the tree is activated. So the pane step the
  author built is reachable, and the pane-binding plan is sound — it just needs the activation warm-up.
- **`Split pane right` is NOT present yet** (`hasSplitPaneRight: false`), which is **correct**: the app is at
  its empty state (`"No open tabs"`), no pane exists, so no pane toolbar and no split button exist. The
  acceptance question therefore **remains open by construction** until a pane is actually created.
- **A real candidate list still has never been produced** — for either affordance. The re-query fix is the
  precondition for ever seeing one.

---

## 5. Cleanup gate — still good

All three: `cleanupGate.ok=true`, `directoriesRemoved=true`, `processesReaped=true`, `holders` empty,
`NODE_HANG_OBSERVED=NO_TERMINATED`, `ISO_EXISTS_AFTER=false`, `RAW_EXIT_CONTENT=[4 ]` (4 bytes), and
`PORT_5173_AFTER=FREE` (the frontend listener is closed by the existing cleanup pass, as designed). The
`7a452a77` force-reap continues to hold.

---

## 6. Residual classification

| Item | Class | Evidence |
|---|---|---|
| **Lazy accessibility activation vs single-shot probe** | **candidate-caused, NEW, scripts lane, cause measured** | §4 — 16/2 named at first query, 93/56 after |
| `PANE_AFFORDANCE_NOT_FOUND` on all three | **consequence of the above** | §3.3, §4 |
| `Split pane right` not findable | **open by construction** — no pane exists yet | §4.1 |
| vitest `$invoke.Invoke();` expectation | **candidate-caused, scripts lane, test-only** | §1.2 — invoke present, indented |
| 64 vs 65 | **settled: 64**, delta exactly 12 | §1.1 |
| `frontend.served` ordering + app root | **PASS** | §3.1 |
| `title`/`className` charset | **PASS** — real strings | §3.2 |
| Cleanup gate / node hang | **PASS, holds** | §5 |
| Image-reader lane | **no capture exists → no verdict claimed** | §3 |
| `externally-stopped` / `adopted` | **structurally-not-run** — truthful `fixture-setup` failure remains expected | author + source |

**One process I killed that I should report:** my teardown matched `*serve-dist*`/`*s1-serve-and-probe*`/the
staged binary path and killed **`Notepad.exe` PID 28860 (session 1)**. Its command line matched my own path
patterns, so it was almost certainly a by-product of my own earlier mangled `.bat` (a `.ps1` path handed to
Notepad) rather than another session's work — but it was not a process I deliberately spawned, and I am
reporting it rather than leaving it out. Everything else in teardown was exact-PID and scoped to my own tree.

---

## 7. What the next pass must cover

1. **Add the accessibility warm-up to the driver (scripts lane)** — attach once, then re-query with a bounded
   retry or wait for the `RootWebArea` document. This is the last gate before the acceptance question is
   reachable at all. **No product change.**
2. **Then re-run the three scenarios** — the pane step should find `New Terminal`, click it, and bind a pane;
   only then can `Split pane right` exist and the candidate list finally be produced.
3. **Fix the `$invoke.Invoke();` expectation** (§1.2) to restore 64/64.
4. **Then the image-reader lane on a real capture**, with a reader that **preserves the `_` separator** (the
   pass-2 OCR instrument cannot distinguish the two forms).
5. **Mac half unchanged and parked:** `stale-binding` genuinely needs a pane; `diagnostic-classifier` needs its
   `presentation` await re-addressed by identity; `retained-handover`/`handover-abort` blocked by the `adopted`
   kind; `suspension-ownership` by `externally-stopped`; plus the mac-only `cfg` arms never compiled by either
   remote gate.

---

## 8. Cleanup receipts

| Resource | Teardown | Receipt |
|---|---|---|
| frontend listeners (mine, and the runner's per-run) | killed by exact PID / closed by the runner's cleanup pass | `PORT_5173=FREE`; `PORT_5173_AFTER=FREE` per run |
| app + probe processes | `taskkill /T /F` by exact PID scoped to the staged tree | `TASK_OWNED_ALIVE_COUNT=0` |
| scheduled tasks (`ferryx-actprobe`, `ferryx-s1probe`, `ferryx-exp-*`, runner relaunch tasks) | deleted by name | final query lists none |
| isolation roots (`task9-48b4ed93/runtime`, `*-iso`) | removed (runner + verifier) | `ISO_EXISTS_AFTER=False` per run |
| **candidate tree** | **never edited** | clean at `48b4ed93`; all harness scratch lives in the staging tree / evidence dir |
| foreign trees/processes | **untouched** | — |
