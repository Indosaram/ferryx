# Task 9 — PASS 6 consolidated report (`120bc965`)

Verifier: sole remote verifier (pass 6). Date: 2026-10-05 (+0900).
Candidate: `C=/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`,
branch `work/local-pane-liveness-completion-foundation`, HEAD **`120bc965`**
(`120bc965bf7232fba57e2361f4967769ba829193`), tree **clean**, unchanged by this pass.
Chain: `120bc965` → `15042b0f` → `91d447e1` → `1df40271` → `5245e6ba` → `314251e0` → `799a582d` → `c34b90fc` → … → `d82b35e4`.

## HEADLINE

**The here-string fix works: the split click now EXECUTES for the first time.** The runner vitest gate is green
at **44/44**. The scenarios reach a real launch, pass `fixture-setup`, resolve a focused pane in the owned
visible window — and then fail on a **new, precise blocker**: the accessible name `Split pane right` is
**not present anywhere inside the owned window** (`candidateCount: 0`).

**This answers the pass-4 unknown, and the answer is not "within one pane scope".** The pane scope resolves to
the **window root** and still finds **zero** name matches, so the duplication is *not* a same-scope problem —
the affordance cannot be located at all. The pass-4 `SPLIT_RIGHT_NOT_UNIQUE` (a **count** assertion) and this
`SPLIT_RIGHT_NOT_FOUND` (an **enumeration**) disagree, and that disagreement is now the finding.

| Gate | Verdict |
|---|---|
| Runner vitest on linux | **PASS — 44/44 passed, exit 0** |
| Windows binary | **no rebuild needed** — `15042b0f..120bc965` is scripts-only (0 Rust files) |
| Self-relaunch to the interactive session | **PASS — reproduced end to end on all three runs** |
| **The split click** | **EXECUTES** — `click-split-affordance` action present for the first time |
| Selector resolution | **FAIL — `SPLIT_RIGHT_NOT_FOUND`, `candidateCount: 0`** (new blocker, scripts lane) |
| Real assertions | **NOT REACHED** (no scenario gets past the click) |
| Image reader | **no capture exists → no recognition verdict claimed** |
| `cleanupGate.ok=false` holder | **CAPTURED — the app's own `--daemon` descendant** (§5) |

---

## 1. Runner vitest on linux (omaki, `indo@100.91.254.71`)

| # | Exact command | Raw exit | Selected | Asserted line | Verdict |
|---|---|---|---|---|---|
| 1 | `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs` | **0** | **44** | `Test Files 1 passed (1)` / `Tests 44 passed (44)` | **PASS** |

The log records `TESTFILE_SHA=98d09cdc939c2d04df8a99bc0009a08731fb73da812dc3314d4f9fc382255890 DECLARED=44`,
matching the committed blob — **the delta was extracted on the host before the run** (the pass-5 mistake is
not repeated).

**Verified fix claims:**
- `grep -c "].join(' ')"` in `native-driver.mjs` → **0**; the file now has **11** `join('\n')` sites.
- `windows-interactive.mjs`: **1** `join('\n')` (session probe), **1** `join('\r\n')` (the `.bat` body), and
  **1** `join(' ')` for the `cmd` command line — the intentional single-line case.
- **Count reconciliation:** the dispatch says "13 `join('\n')` sites". Measured precisely across `scripts/`:
  **12** `join('\n')` in the two fixed files (`native-driver.mjs` 11 + `windows-interactive.mjs` 1) **plus**
  **1** `join('\r\n')` for the bat body = **13 line-structured sites**. Same number, exact attribution.
- `Add-Type @"` appears at `native-driver.mjs:306`, `:388`, `:489` — each **alone on its line**.
- **The new regression test locks the pass-5 signature exactly** (`pane-liveness.test.mjs:1222`):
  `expect(script).not.toMatch(/@"[^\n]/)`, `expect(script).not.toMatch(/[^\n]"@/)`,
  `expect(lines.filter(l => l === 'Add-Type @"')).toHaveLength(1)`, plus header/footer position and
  neighbours. Three such tests cover the split-right, window-wait and capture builders.
- `sourceDigest` changed (`5c53505e…` vs pass 5's `9f70427d…`), confirming the new module and test file are
  inside the digest.

**Not a trap:** `pane-liveness.test.mjs:447` uses `].join(' ')` — inspected: it builds a **Node `-e` script**
(a process-tree fixture), not PowerShell, so it is correctly space-joined.

---

## 2. Windows: staging

| Check | Result |
|---|---|
| `15042b0f..120bc965` | **3 files, all `scripts/`** — **0 under `src-tauri/`** |
| Consequence | the warm pass-4 binary (`6ad63c5a…`, 106 915 840 B) **is** this revision's binary — **no rebuild performed**, as predicted |
| Staging | linux **10/10 `STAGED_OK`**, Windows **10/10 `STAGED_OK`**, `STAGED_BAD_COUNT=0`; both hosts' test file at `98d09cdc…` |
| Disk | 29.7 GB free on entry; **29.67 GB** at close |

---

## 3. The three Windows scenarios — the click executes

Exact argv per the brief; the launcher self-relaunched on **all three** runs.

| Scenario | Raw exit | Typed code | `fixture-setup` | `click-split-affordance` | Real assertions | Image-reader verdict | Verdict |
|---|---|---|---|---|---|---|---|
| `split-happy` | **1** | `SPLIT_RIGHT_NOT_FOUND` | **YES** | **YES — first ever** | **not reached** | **none — no capture** | **FAIL** |
| `split-attach-stall` | **1** | `SPLIT_RIGHT_NOT_FOUND` | **YES** + `attach-handshake` registered | **YES** | **not reached** | **none — no capture** | **FAIL** |
| `split-cancel` | **1** | `SPLIT_RIGHT_NOT_FOUND` | **YES** | **YES** | **not reached** | **none — no capture** | **FAIL** |

Action sequence now (all three): `barriers.prearmed` → `windows-interactive-admission` →
`windows-interactive-relaunch` → `launch.binary` → `fixture-setup` → `powershell` → `owned-window` →
(`barriers.registered` for attach-stall) → `powershell` → **`click-split-affordance`**.

### 3.1 The candidate list, VERBATIM (the lead's explicit ask)

`split-happy`, the `click-split-affordance` action:

```json
{"at":"2026-10-04T17:20:15.293Z","action":"click-split-affordance",
 "selector":{"automationId":null,"name":"Split pane right"},"pid":7716,"assertedUniqueEnabled":false,
 "code":"SPLIT_RIGHT_NOT_FOUND",
 "window":{"mainWindowHandle":19663500,"windowVisible":true,"interactive":true,"sessionId":1},
 "scope":{"focusedFound":true,"focusSource":null,"depth":10,"isWindowRoot":false},
 "candidateCount":0,"actionableCount":0,"candidates":[],"chosen":null,
 "detail":"the \"Split pane right\" affordance of the focused pane could not be identified: …"}
```

The probe's own stdout carries the decisive line:

```json
{"probe":"split-right","selector":"Split pane right","interactive":true,"sessionId":1,
 "mainWindowHandle":19663500,"windowVisible":true,"focusedFound":true,"focusSource":null,
 "scopeDepth":10,"scopeIsWindowRoot":false,"candidateCount":0,"actionableCount":0,
 "candidates":[],"chosen":null,
 "failure":"SPLIT_RIGHT_NOT_FOUND",
 "detail":"no ancestor of the focused pane contains the split affordance"}
```

**Identical on all three runs** (window handles differ: 19663500 / 4787552 / 1313834; `scopeDepth` 10;
`focusSource` `null`; `candidateCount` 0; `candidates` `[]`).

**Reading the answer, exactly as the evidence supports it.** The walk ascends from the focused element and, at
each ancestor, does `FindAll(TreeScope::Descendants, NameProperty == "Split pane right")`; it breaks when a node
yields ≥1 match. It reached **`scopeDepth = 10`** — ten ancestors climbed — and **no ancestor matched**, which
means the walk **ran to the window root and found nothing** (`isWindowRoot` is recorded `false` only because
the loop breaks on `$node -eq $root` *before* the scope is assigned, so the flag is not evidence about which
node was reached). Therefore:

- **`candidateCount: 0` is a property of the whole owned window, not of a narrow pane scope.**
- **The duplicates are NOT within one pane scope** — there is nothing named `Split pane right` to duplicate
  in the scope. The pass-4 `SPLIT_RIGHT_NOT_UNIQUE` came from a *different, count-based* assertion in the
  older probe; the enumeration now says the name is absent.

**Two candidate explanations, stated as hypotheses (I did not prove either):**
1. **`MainWindowHandle` may not be the app's real UI window.** The `owned-window` probe enumerated **four**
   top-level windows for the app pid — two visible (`className 'T'`, titles `F` and `''`), two invisible —
   and the driver used `$proc.MainWindowHandle` = the window titled `F`. The title `F` and a title-less `T`
   window are both suspicious for a real Ferryx window; the UI may live in the other one.
2. **The accessible name may differ from the literal.** The probe matches `NameProperty` exactly against
   `"Split pane right"` (the `aria-label`/`title` in `ui/src/components/ui/IconButton.tsx` via
   `TerminalSplitView.tsx`). UIA exposes a button's accessible name through its name *property*, which for a
   webview-hosted element is not guaranteed to equal the `aria-label` string.

Either way the fix belongs to the **scripts lane** (`native-driver.mjs` / `windows-interactive.mjs`), not to
the product: the product renders the button (the UI code is unchanged and its contract test is green), and the
probe's own `owned-window` action shows a visible owned window exists.

---

## 4. Raw exit codes

| Scenario | `.exit` **content** | `.exit` **bytes** | Delegated `relaunch.exit` | Outer exit |
|---|---|---|---|---|
| `split-happy` | **`1`** | 4 | **`1`** | **1** |
| `split-attach-stall` | **`1`** | 4 | **`1`** | **1** |
| `split-cancel` | **no file** — node hung on the inherited stdio pipe after printing its result, so the batch's `echo %ERRORLEVEL%` line was never reached | — | **no file** | **not measured**; `result.json` carries `code: SPLIT_RIGHT_NOT_FOUND` and `cleanupGate.ok=false`, so the runner's own logic yields **1** |

Consistent with the corrected carry-forward: content `1`, byte length `4`, typed code in `result.json`,
`cleanupGate.ok=false` overriding the typed mapping with `EXIT.scenarioFailure = 1`.

---

## 5. The isolation-root holder — CAPTURED (the lead's explicit ask)

All three runs: `"cleanupGate": {"processesReaped": true, "socketsRemoved": true, "directoriesRemoved": false, "ok": false}`.
**No sidecar of mine ran this pass**, so the instrument is again excluded.

Captured holder evidence, taken **before** any kill:

```
HOLDER PID=22456 PPID=7716 S1 ferryx.exe  :: "…\source-21dea3c0\src-tauri\target\debug\ferryx.exe" --daemon
HOLDER PID=27816 PPID=25352 S1 node.exe   :: "…\scripts\qa\pane-liveness.mjs" …
HOLDER PID=476   PPID=10448 S0 node.exe   :: node scripts\qa\pane-liveness.mjs --scenario split-happy …
ROOT_STILL_PRESENT split-happy
```

and the decisive follow-up:

```
=== can the isolation root now be removed? ===
ISO_REMOVED_AFTER_KILL=True
```

**Conclusion for the decision you wanted to make:** the root is held by the **app's own `--daemon`
descendant**, and it becomes removable **the instant that descendant is killed**. The runner's `taskkill /T`
(no `/F`) leaves that child alive, so `rmSync` fails. **A force-reap of the runner's own tree would close both
the `cleanupGate.ok=false` and the node-hang symptoms** — the two are one cause, now with the holder named and
the removal demonstrated.

**Operational note:** because the daemon child survives, a **serialized** scenario wrapper never advances (the
outer `node` stays alive on the inherited stdio pipe). For this pass I let each scenario's result land, then
killed that scenario's own daemon by exact PID, which let the sequence continue — three results were obtained
from one wrapper this way.

---

## 6. Residual classification

| Item | Class | Evidence |
|---|---|---|
| Here-string join defect (pass-5 blocker) | **CLOSED — the click executes** | §3; vitest 44/44 including the signature-locking test |
| `SPLIT_RIGHT_NOT_FOUND`, `candidateCount: 0` | **candidate-caused, NEW, scripts lane** | §3.1 — identical on three runs, `scopeDepth` 10, empty candidate set |
| The pass-4 duplicate-set unknown | **ANSWERED — not within one pane scope; the name is absent window-wide** | §3.1 |
| `cleanupGate.ok=false` / node hang | **harness, holder now named** — the app's own `--daemon`; `ISO_REMOVED_AFTER_KILL=True` | §5 |
| Exit `1` vs typed `4` | **harness** — cleanup gate override | §4 |
| Image-reader lane | **no capture exists → no verdict claimed** (`IMAGE_COUNT=0`) | §3 |
| `externally-stopped` / `adopted` | **structurally-not-run** — truthful `fixture-setup` failure remains the expected outcome | author + source |
| Dispatch's "13 `join('\n')` sites" | **reconciled** — 12 `join('\n')` + 1 `join('\r\n')` = 13 line-structured sites | §1 |

**No verifier contamination this pass.** No invalidated result: every gate was run once on the correctly
staged revision, and I verified the staged hashes before each gate rather than after.

---

## 7. What the parked mac half must still cover

1. **Fix the selector resolution (§3.1)** — it is now the single gate in front of every scenario's real
   assertions. The two hypotheses to test first: whether `MainWindowHandle` is the app's real UI window (the
   probe saw **four** top-level windows, two visible, one titled `F` and one title-less), and whether the UIA
   accessible name actually equals the literal `"Split pane right"`.
2. **Then re-run the three Windows scenarios** — they are blocked only by item 1; self-relaunch,
   `fixture-setup` (real session) and the click path are all proven.
3. **Then the image-reader lane on a real capture**, with a reader that **preserves the `_` separator** (the
   pass-2 OCR instrument cannot distinguish `FERRYX_SPLIT_READY` from the space-separated form).
4. **Consider the force-reap decision** (§5): it closes the cleanup gate and the hang together, and would let
   a serialized wrapper advance without per-scenario intervention.
5. **Re-run the compile gate on mac** — the mac-only `cfg` arms of `native_terminal/surface_host.rs` /
   `ipc/native_terminal.rs` are still not compiled by either remote gate.
6. `diagnostic-classifier` (native + the deferred headless smoke, still NOT_RUN per F1 H-23),
   `retained-handover`, `handover-abort`, `suspension-ownership`, `split-concurrent`, `stale-binding`.

---

## 8. Cleanup receipts

| Resource | Teardown | Receipt |
|---|---|---|
| own scheduled tasks (`ferryx-qa-split-happy-476-d07cn7`, `…-split-cancel-20352-el1m68`) | deleted by name | `schtasks /delete` succeeded for both; final query lists none |
| own processes (outer `node`/`cmd`, delegated session-1 `node`, app `ferryx.exe` and its daemon child) | **exact-PID `taskkill /T /F`**, filtered on `*task9-120bc965*` / `*run-scenario6*` / the staged-tree `ferryx.exe` — **never a name pattern** | `TASK_OWNED_ALIVE_COUNT=0` |
| isolation roots (`split-happy`, `split-attach-stall`, `split-cancel`) | each cleared once its own daemon died | `ISO_REMOVED_AFTER_KILL=True` for the one tested; roots retained as evidence where the runner's own cleanup could not remove them |
| linux scratch / superseded logs | removed / superseded | `LINUX_IDLE` (0 own processes) |
| **foreign trees/processes on both hosts** | **untouched** | listed, not modified |
| staged trees (`source-21dea3c0`, `task9-120bc965`) | **retained** as the evidence source (owned) | — |
| user desktop / production app / production daemon | **never addressed** — only the staged-tree test binary, via the runner's own scheduled task | — |
