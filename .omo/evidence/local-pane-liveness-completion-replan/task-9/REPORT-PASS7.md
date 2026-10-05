# Task 9 — PASS 7 consolidated report (`42fba06f`)

Verifier: sole remote verifier (pass 7). Date: 2026-10-05 (+0900).
Candidate: `C=/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`,
branch `work/local-pane-liveness-completion-foundation`, HEAD **`42fba06f`**
(`42fba06f1874ac9c3bbff260378533a90aa02288`), tree **clean**, unchanged by this pass.
Chain: `42fba06f` (selector) → `7a452a77` (force-reap) → `120bc965` (here-string) → `15042b0f` → `91d447e1` → … → `d82b35e4`.

## HEADLINE

**The force-reap fix works completely** — both symptoms closed: the cleanup gate passes, the node hang is gone,
and the exit code is now the **typed `4`** instead of the override. **The selector fix works as designed but the
answer is a PRODUCT finding:** the multi-window search ran over both visible owned windows and the bounded
inventory is **empty in every one of them**, so the split affordance has **no UIA representation at all**.

| Gate | Verdict |
|---|---|
| Runner vitest on linux | **FAIL — 51/52, exit 1** (one test, scripts-lane contract mismatch) |
| Windows binary | **no rebuild needed** — `120bc965..42fba06f` is scripts-only (0 Rust files) |
| Force-reap / cleanup gate | **PASS — `ok=true`, `directoriesRemoved=true`, node hang gone, exit content `4`** |
| Multi-window selector search | **RAN — 2 visible windows searched, `matchedWindowHwnd: null`** |
| **`splitInventory`** | **EMPTY IN EVERY VISIBLE WINDOW → product must expose the affordance** |
| Click succeeded | **NO** — `SPLIT_RIGHT_NOT_FOUND`; no scenario reaches its real assertions |
| Image reader | **no capture exists → no recognition verdict claimed** |

---

## 1. Runner vitest on linux (omaki, `indo@100.91.254.71`)

| # | Exact command | Raw exit | Selected | Asserted line | Verdict |
|---|---|---|---|---|---|
| 1 | `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs` | **1** | **52** | `Test Files 1 failed (1)` / `Tests 1 failed \| 51 passed (52)` | **FAIL — 1 test** |

The delta was extracted on the host before the run (`TESTFILE_SHA=79cf8f6bb8c7a34eb7e00fbff2005f28e4cb56bb0bdb42235b11ec0a86329f57`,
`DECLARED=52`), matching the committed blob.

### 1.1 The failure, verbatim

```
 FAIL  scripts/qa/pane-liveness.test.mjs > split-affordance match is multi-property and exact, never a substring or first-match heuristic
AssertionError: expected [] to deeply equal [ 'split-pane-right' ]

- Expected
+ Received

- [
-   "split-pane-right",
- ]
+ []

 ❯ scripts/qa/pane-liveness.test.mjs:1664:38
    1662|   expect(byId.ok).toBe(true);
    1663|   expect(byId.matchedWindowHwnd).toBe(1);
    1664|   expect(byId.selectorAutomationIds).toEqual(['split-pane-right']);
       |                                      ^
```

**Cause (source-derived).** The test builds a probe whose automation id appears **only** in
`chosen.automationId` (`pane-liveness.test.mjs:1652-1661`) and never sets `selectorAutomationIds`. The
classifier reads it with no fallback:

```js
// native-driver.mjs
942:  selectorNames: selectorNames.length > 0 ? selectorNames : [...SPLIT_AFFORDANCE_NAMES_WIN32],   // has a fallback
943:  selectorAutomationIds: asArray(probe?.selectorAutomationIds),                                  // no fallback -> []
```

so the expectation at `:1664` cannot hold for that probe. Note the asymmetry: `selectorNames` falls back to
the bound constant, `selectorAutomationIds` does not — and the sibling assertion at `:1667`
(`classifyWindowsSplitRight({…, failure: 'SPLIT_RIGHT_NOT_FOUND'}).selectorNames` → `['Split pane right']`)
passes precisely because of that fallback.

**Blast radius: the search is NOT affected.** The PowerShell script's `$selectorAutomationIds` comes from
`buildWindowsSplitRightScript`'s options (`native-driver.mjs:740`), and the production call passes only
`{ windows }`, so it uses the module default `SPLIT_AFFORDANCE_AUTOMATION_IDS_WIN32 = []` (verified in the
action's own `"selectorAutomationIds":[]`). This is a **test/implementation contract mismatch in the reporting
path**, not a search defect. **Owning lane: scripts.**

---

## 2. Windows: staging

| Check | Result |
|---|---|
| `120bc965..42fba06f` | 4 files, all `scripts/` — **0 under `src-tauri/`** |
| Consequence | the warm pass-4 binary (`6ad63c5a…`) **is** this revision's binary — **no rebuild performed** |
| Staging | linux **10/10 `STAGED_OK`**, Windows **10/10 `STAGED_OK`**, `STAGED_BAD_COUNT=0`; both hosts at test-file `79cf8f6b…` |
| Disk | 20.79 GB free on entry; **20.8 GB** at close |

---

## 3. The three Windows scenarios

Action sequence now includes the new enumeration step:
`barriers.prearmed` → `windows-interactive-admission` → `windows-interactive-relaunch` → `launch.binary` →
`fixture-setup` → `powershell` → `owned-window` → (`barriers.registered` for attach-stall) → `powershell` →
**`owned-windows-enumerated`** → `powershell` → **`click-split-affordance`**.

| Scenario | Raw exit | Typed code | `fixture-setup` | Click | Real assertions | Image reader | Verdict |
|---|---|---|---|---|---|---|---|
| `split-happy` | **4** | `SPLIT_RIGHT_NOT_FOUND` | **YES** | **no** | **not reached** | **none — no capture** | **BLOCKED** |
| `split-attach-stall` | **4** | `SPLIT_RIGHT_NOT_FOUND` | **YES** + `attach-handshake` | **no** | **not reached** | **none — no capture** | **BLOCKED** |
| `split-cancel` | **4** | `SPLIT_RIGHT_NOT_FOUND` | **YES** | **no** | **not reached** | **none — no capture** | **BLOCKED** |

### 3.1 THE KEY EVIDENCE — `splitInventory`, verbatim

Identical on all three runs. `split-happy`:

```json
{"action":"click-split-affordance","selector":{"automationId":null,"name":"Split pane right"},
 "selectorNames":["Split pane right"],"selectorAutomationIds":[],
 "code":"SPLIT_RIGHT_NOT_FOUND",
 "window":{"mainWindowHandle":38079254,"windowVisible":true,"interactive":true,"sessionId":1},
 "windowsSearched":[{"hwnd":38079254,"title":"F","className":"T"},{"hwnd":17369732,"title":"","className":"T"}],
 "windowsSearchedCount":2,"matchedWindowHwnd":null,
 "scope":{"focusedFound":true,"focusSource":null,"depth":10,"isWindowRoot":false},
 "candidateCount":0,"actionableCount":0,"candidates":[],"chosen":null,
 "splitInventory":{
   "windows":[
     {"hwnd":38079254,"title":"F","className":"T","inspectedCount":29,"matchCount":0,"truncated":false,"matches":[]},
     {"hwnd":17369732,"title":"","className":"T","inspectedCount":0,"matchCount":0,"truncated":false,"matches":[]}
   ],
   "inspectedCount":29,"matchCount":0,"truncated":false,
   "inspectCap":4000,"matchCap":40,
   "filter":"name or automationId contains \"split\" (case-insensitive)"}}
```

`split-attach-stall`: windows `17762968` (inspected 29, matches 0) and `6622232` (inspected **0**, matches 0).
`split-cancel`: windows `22743030` (inspected 29, matches 0) and `25495876` (inspected **0**, matches 0).

**The inventory is trustworthy, and I checked why before drawing the conclusion:**
- The walk is **maximally permissive**: `$inventoryRoot.FindAll(TreeScope::Descendants, Condition::TrueCondition)`
  (`native-driver.mjs:794`) — every descendant of each visible owned window, no view filter, no name filter.
- `truncated: false` and `inspectCap: 4000` / `matchCap: 40` — the caps were never approached, so the result
  is **complete, not sampled**.
- The match test reads UIA managed properties (`$element.Current.Name`, `.AutomationId`, `:800-802`), so it is
  not subject to any P/Invoke buffer truncation.

**Decision rule applied — this is the PRODUCT branch:**
> *"If the inventory is **empty in every visible window** → the affordance has **no UIA representation at all**
> and the product must expose one (AutomationId or accessible name) — a **product** finding."*

The inventory is **empty in both visible windows** (`matchCount: 0` in each, not truncated). Therefore:
**the split affordance has no UIA representation in the app's window, and the product must expose one.**

**Supporting measurement that makes this more than an absence:** the app's windows barely expose any UIA tree
at all — the UIA-bearing window yields only **29 descendants**, and the other visible window yields **zero**.
**Leading mechanism (hypothesis, NOT proven):** WebView2 web content is not surfaced to UIA unless
accessibility is activated, which would leave the whole webview subtree invisible to `TrueCondition` — and
would explain a 29-node window. Confirming it needs product-side instrumentation, which this pass does not do.

**Also worth recording:** the pass-4 `SPLIT_RIGHT_NOT_UNIQUE` and this `SPLIT_RIGHT_NOT_FOUND` disagree, and the
pass-4 figure came from the older probe. Nothing in this pass reproduces a >1 count.

### 3.2 Multi-window search behaviour (asked explicitly)

| Field | Value | Reading |
|---|---|---|
| `visibleWindowCount` | **2** | two visible owned windows, so the pre-scenario gate passes on `>= 1` (not on a non-zero `MainWindowHandle`) |
| `windowsSearchedCount` | **2** | both visible windows were searched |
| `windowsSearched` order | main handle first, then the other by hwnd | matches the documented deterministic order |
| `matchedWindowHwnd` | **null** | **no** window produced a match |
| invisible windows | excluded | the pass-6 enumeration had 2 invisible windows (classes `M`, `I`); neither was searched |

**So the multi-window search changed nothing for the outcome** — it is the right generalisation (it removes the
`MainWindowHandle`-is-the-UI-window assumption, hypothesis 1), but the failure is not a wrong-window choice:
neither window contains the affordance.

---

## 4. The force-reap fix — both symptoms closed

| Run | `cleanupGate.ok` | `directoriesRemoved` | holders | node hang | `.exit` content | `ISO_EXISTS_AFTER` |
|---|---|---|---|---|---|---|
| `split-happy` | **true** | **true** | **empty** | **`NO_TERMINATED`** (waited on the exit file, reason `EXIT_FILE`) | **`4`** | **false** |
| `split-attach-stall` | **true** | **true** | **empty** | **`NO_TERMINATED`** | **`4`** | **false** |
| `split-cancel` | **true** | **true** | **empty** | **`NO_TERMINATED`** | **`4`** | **false** |

**Both symptoms of the one cause are closed**, exactly as predicted:
- The **node hang is gone** — node exited by itself and wrote its exit file (passes 2–6 required a manual
  exact-PID kill of the app's `--daemon` child).
- The **cleanup gate now passes** and the isolation roots are **gone** after the run, where pass 6 left them
  with `directoriesRemoved: false` and a recorded `ferryx.exe --daemon` holder.
- The **exit code is now the typed `4`** (`EXIT.nativeAutomationUnsupported`) instead of pass 6's override to
  `1`, because the gate no longer fails — the `.exit` files still contain the string `"4 "` at 4 bytes.

This also closes the pass-6 operational note: a serialized scenario wrapper now advances **without** per-scenario
intervention (all three scenarios ran back-to-back in one wrapper and all three completed).

---

## 5. Residual classification

| Item | Class | Evidence |
|---|---|---|
| Force-reap / cleanup gate / node hang (pass-6 harness item) | **FIXED — both symptoms closed** | §4 |
| Multi-window selector search (hypothesis 1) | **SHIPPED and exercised** — no longer assumes `MainWindowHandle` | §3.2 |
| Exact multi-property matching (hypothesis 2) | **SHIPPED** — exact `PropertyCondition`s + `OrCondition`, no substring/first-match | vitest + script assertions |
| **`splitInventory` empty in every visible window** | **PRODUCT finding — the affordance has no UIA representation; the product must expose one (AutomationId or accessible name)** | §3.1 |
| WebView2 accessibility as the mechanism | **hypothesis, unproven** | §3.1 |
| `split-affordance match is multi-property and exact…` test | **candidate-caused, scripts lane, reporting path only** | §1.1 |
| Window `title`/`className` are 1 character | **candidate-caused, scripts lane, diagnostic only** | §6 |
| Image-reader lane | **no capture exists → no verdict claimed** (`IMAGE_COUNT=0`) | §3 |
| `externally-stopped` / `adopted` | **structurally-not-run** — truthful `fixture-setup` failure remains expected | author + source |

**No verifier contamination this pass.** Every gate ran once on the hash-verified staged revision; no result was
invalidated or discarded.

---

## 6. A second scripts-lane defect I found while checking the inventory's trustworthiness

The evidence fields `title` and `className` are **1 character** in every run:

```
pass 7: {"hwnd":38079254,"title":"F","className":"T"}   {"hwnd":17369732,"title":"","className":"T"}
pass 6: {"className":"T","title":"F"}  {"className":"T","title":""}  {"className":"M","title":"M"}  {"className":"I","title":"D"}
```

The last pair is the tell: an **IME** window (class `IME`, title `Default IME`) reads as `'I'` / `'D'` — the
**first character of each**. **Cause:** the `DllImport` declarations for `GetWindowTextW` and `GetClassNameW`
(`native-driver.mjs:435-436` and `:548-549`) omit `CharSet = CharSet.Unicode`, so the default **ANSI**
marshalling stops at the first UTF-16 NUL byte — i.e. after one character of a UTF-16 string. (The buffers
themselves are correctly sized at 256.)

**Blast radius — diagnostic only, and I verified it does not touch the key finding:** window ordering uses
`hwnd`, not titles; the search matches UIA properties; the inventory reads `$element.Current.Name` (a managed
UIA string, no buffer). So `title`/`className` being 1 character degrades the **evidence fields** and nothing
else. **Owning lane: scripts** (one attribute on four `DllImport` declarations).

---

## 7. What the parked mac half must still cover

1. **The product-side exposure (item §3.1)** — the split affordance has no UIA representation. The product must
   expose an `AutomationId` or an accessible name that UIA can see. If the mechanism is WebView2 accessibility,
   that is a product-side activation. **This is now the single gate in front of every scenario's real
   assertions**, and it is the first finding in this series that is *not* the scripts lane.
2. **Re-run the three Windows scenarios** once the affordance is exposed — self-relaunch, `fixture-setup` (real
   session), the multi-window search, the exact matcher and the force-reap are all proven.
3. **Then the image-reader lane on a real capture**, with a reader that **preserves the `_` separator** (the
   pass-2 OCR instrument cannot distinguish `FERRYX_SPLIT_READY` from the space-separated form).
4. **Fix the vitest failure** (§1.1) and the `CharSet` defect (§6) — both scripts lane, neither blocking.
5. **Re-run the compile gate on mac** — the mac-only `cfg` arms of `native_terminal/surface_host.rs` /
   `ipc/native_terminal.rs` are still not compiled by either remote gate.
6. `diagnostic-classifier` (native + the deferred headless smoke, still NOT_RUN per F1 H-23),
   `retained-handover`, `handover-abort`, `suspension-ownership`, `split-concurrent`, `stale-binding`.

---

## 8. Cleanup receipts

| Resource | Teardown | Receipt |
|---|---|---|
| own scheduled tasks | none left | `OWN_TASK_COUNT=0` |
| own processes | none left | `TASK_OWNED_ALIVE_COUNT=0` |
| isolation roots (`split-happy`, `split-attach-stall`, `split-cancel`) | **removed by the runner's own force-reap** | `runtime/` empty at close; `ISO_EXISTS_AFTER=false` per run |
| linux scratch / logs | superseded | `LINUX_IDLE` (0 own processes) |
| **foreign trees/processes on both hosts** | **untouched** | listed, not modified |
| staged trees (`source-21dea3c0`, `task9-42fba06f`) | **retained** as the evidence source (owned) | — |
| user desktop / production app / production daemon | **never addressed** — only the staged-tree test binary, via the runner's own scheduled task | — |
