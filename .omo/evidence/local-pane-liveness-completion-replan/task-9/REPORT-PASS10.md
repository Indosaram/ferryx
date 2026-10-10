# Task 9 — PASS 10 (in progress): the pane frontier

Verifier: sole remote verifier. Date: 2026-10-05 (+0900).
HEAD **`757f8414`** (tree clean, unchanged by this pass). `TMPDIR` redirected from the start this pass
(pass 9's run-1 `EDQUOT` was the `/tmp` tmpfs at 80%).

## Scope

Measure the pane frontier behind pass 9's `PANE_BINDING_UNBOUND` on `split-happy`: the click on
`New Terminal` succeeded (`assertedUniqueEnabled: true`, one candidate with a real rect) and then
`no presentation receipt named a session other than the 1 fixture session(s) within 8000ms ... observed
sessions: []`. Measure, in the interactive session with the frontend served: the daemon's own session
inventory **before** the click, the real UIA click, then **after** the click the daemon inventory, the app's UI
tree, the `presentation.receipt.jsonl` lines and any error text — then classify (i)-(iv).

Probe (verifier scratch, not part of the candidate): `pf-run.ps1` (session-1 orchestrator),
`pf-uia.mjs` (reuses the driver's own `awaitOwnedWindowsWindows` + `buildWindowsNewPaneScript`, plus an
inline `TrueCondition` warm per the pass-9 measurement), `pf-daemon.mjs` (speaks the daemon's own control
wire: newline-delimited JSON over TCP at `runtime/daemon.port`, authenticated with `runtime/daemon.token`).

## Measurement

_(filled in below)_

## Measurement — the pane frontier on `split-happy`

Run: the **real runner** (`scripts/qa/pane-liveness.mjs --scenario split-happy --binary … --evidence-dir … --isolation-root …`), launched **inside the interactive session** (`session=1`, `interactive=True`), with an **external poller** in the same script sampling the isolated profile's daemon session inventory every ~700 ms via the daemon's own control wire (`TcpClient` → `runtime/daemon.port` + `runtime/daemon.token` → `handshake` → `listSessions`).

### 1. Before the click — the daemon inventory and the workspace question

| sample | at (from launch) | session count | sessions |
|---|---|---|---|
| 1 | **2215 ms** | **1** | `cb71f12e-c415-440b-929a-63135dd74ca7` |
| 2 | **2929 ms** | **2** | `cb71f12e…`, `a6f9a001-66df-4812-846a-e33db77d1681` |
| 3–11 | 3642 … 9333 ms | 2 | unchanged |

**The app's active workspace is NOT registered with the isolated profile's daemon.** The pane-binding lane's
unverified assumption is therefore **confirmed as false in this setup**: the daemon registers only its own
startup workspace (`workspace_id=sook`, `repo_root=\\?\C:\Users\sook`) at boot, and the isolated profile's
`machine-workspaces.v1.json` is **`ABSENT`** — the app never registered `qa-fixture` (or anything) with this
daemon, because `create_gui_fixture_sessions` registers its workspace **only through the QA fixture path**, and
the product's own UI spawn path does not.

**The inventory grows 1 → 2 at ~2929 ms, i.e. BEFORE the pane click (7732 ms).** So the second session is not
the pane's session; it is created during boot/fixture setup.

### 2. The click — the driver's own probe ran and found nothing

```
PANE_AFFORDANCE_NOT_FOUND: the "New Terminal" affordance could not be identified by its exact accessible name:
  {"selectorNames":["New Terminal"],"selectorAutomationIds":[],"interactive":true,"sessionId":1,
   "windowsSearched":[{"hwnd":6229050,"title":"Ferryx","className":"Tauri Window","rootOffscreen":false},
                      {"hwnd":33031808,"title":"","className":"Tao Thread Event Target","rootOffscreen":false}],
   "windowsSearchedCount":2,"candidateCount":0,"actionableCount":0,"candidates":[],"chosen":null,
   "warmElements":110,"warmAttempts":33,"warmElapsedMs":4036}
```

**`warmElements: 110`** — this is the decisive number. Compare with pass 9, where the failures reported
`warmElements: 16` (the Chromium-internal pre-activation tree) and the one success reported `16` on a single
attempt. **Here the warm attach already saw 110 elements, i.e. the DOM WAS built**, and 33 further attempts over
4 s still produced `candidateCount: 0`.

### 3. After the click — the delta

| observation | before | after |
|---|---|---|
| daemon session count | 1 → 2 (at 2929 ms) | **2, unchanged** (samples 3–11, up to 9333 ms) |
| new session id | — | **none** |
| `presentation.receipt.jsonl` | absent | **absent** (`receipts: []`) |
| any receipt at all | `fixture-setup.receipt.jsonl` in the scenario's own barrier dir | unchanged |
| `machine-workspaces.v1.json` | — | **`ABSENT`** |

### 4. Classification — **(i) the click produced no spawn at all**

**Stated plainly: this is (i), not (ii) and not (iii).** The reasoning, clause by clause:

- **Not (ii)** ("a spawn happened but the app never presented it"): a spawn would appear in the daemon
  inventory as a **new session id**. The inventory is **flat at 2** across 9 samples spanning the click
  (7732 ms) and 1.6 s after it. **No spawn happened.**
- **Not (iii)** ("a pane was created and presented but the receipt the harness reads does not carry it"):
  there is **no `presentation.receipt.jsonl` at all**, so nothing was presented; and no new session exists for
  a receipt to name.
- **And the click never even fired.** The driver's own probe returned `PANE_AFFORDANCE_NOT_FOUND` with
  `candidateCount: 0`, so `InvokePattern.Invoke()` was never reached — no `New Terminal` activation occurred.
  That is why there is no spawn to observe.

**So the pass-9 `PANE_BINDING_UNBOUND` and this pass's `PANE_AFFORDANCE_NOT_FOUND` are the same underlying
blocker seen from two sides:** the pane step cannot find its affordance, therefore no click, therefore no
spawn, therefore no presentation receipt.

### 5. What is newly established about WHY the affordance is not found

`warmElements: 110` vs pass 9's `16` **localises the failure past the activation problem**: the tree is built,
and `New Terminal` is still not matched. Two facts bear on this:

1. **The app was showing its empty state, not an error page.** The empty state is where `New Terminal`
   *should* be (`ui/src/components/EmptyWorkspaceView.tsx`), and pass 9 measured that button as present in the
   tree with `warmElements` in the 93–110 range. So a built tree at 110 elements **ought** to contain it.
2. **The search order matters.** `windowsSearched` lists two windows, and the enumeration orders the one
   containing focus first. `New Terminal` lives inside the **webview** window; if the focused window is the
   `Tao Thread Event Target` window (or the focus is not inside the webview), the first search is in the wrong
   window — though the probe does then search the remaining windows, so this alone does not explain a miss.

**I did not isolate the exact cause of the name mismatch this pass, and I am not going to assert one.** What I
can state with evidence is the sharpened boundary: *the tree is built (110 elements) and the name still does
not match*, so the remaining question is about **the accessible name or the window/focus scope**, not about
lazy activation. The next measurement that would settle it is a dump of the **named elements of the
webview window at the moment of the pane click** — which the driver already has the pieces for (the same
`TrueCondition` attach that reported 110).

### 6. Also carried over

- `TMPDIR` redirected from the start (no `EDQUOT` this pass).
- Cleanup receipts and the no-capture/no-recognition status from the dispatch are recorded in pass 9's report
  and were not re-measured here.

---

## 7. Teardown — exact-PID-only, per the standing rule

| Resource | Action | Receipt |
|---|---|---|
| app + its `--daemon` child launched by my probe | killed by **exact PID recorded from my own listing** (28932, 17084), each **identity-checked** against the staged-tree executable path before the kill | `IDENTITY_OK` printed for each |
| my `manual-delta.ps1` shells (26528, 27980) | killed by exact PID, identity-checked against `*manual-delta*` in the command line, parent chain traced to my SSH session (8344 `sshd`) | `IDENTITY_OK` for both |
| the leftover `serve-dist.mjs` listener holding 5173 (26792) | killed by exact PID taken from the `Get-NetTCPConnection` listener listing | `PORT_5173=FREE` |
| own scheduled tasks (`ferryx-pd`, `ferryx-pf`) | deleted by name | `OWN_TASKS_LEFT=0` |
| **9 remaining PowerShell/pwsh shells** whose command lines match my probe script names | **REPORTED, NOT KILLED** — they match by **pattern only**; I did not record their PIDs at spawn, so per the standing rule I enumerate them and leave them alive | see §7.1 |
| **candidate tree** | **never edited** | clean at `757f8414` |
| foreign trees/processes | **untouched** | — |

### 7.1 Reported, not killed (leftovers I cannot prove are mine)

```
STILL PID=23620 pwsh.exe        STILL PID=22676 powershell.exe
STILL PID=23492 pwsh.exe        STILL PID=25748 powershell.exe
STILL PID=21664 pwsh.exe        STILL PID=21800 powershell.exe
STILL PID=18636 pwsh.exe        STILL PID=29008 powershell.exe
STILL PID=26412 powershell.exe
```

These are almost certainly the shells of my own `ssh … powershell -File <probe>.ps1` calls (their command lines
name my scripts), but **a command-line pattern is not an ownership proof**, so I am reporting them for the
lead to decide rather than killing them. Port 5173 is **free**; host free space **16.5 GB**.

### 7.2 A useful confirmation of the runner's own guard

My leftover listener on 5173 caused the runner to refuse with its typed **`FRONTEND_PORT_OCCUPIED`** rather
than reusing a foreign listener — the guard behaved exactly as designed, and it is worth noting that it
**fail-closed correctly** even when the foreign listener happened to be mine.

---

## 8. Two corrections a reader must have

### 8.1 The revision: this measurement is on `757f8414`, and HEAD has since moved

**Every measurement in this report is on `757f8414`** (the revision the dispatch named). While this pass was
running, **HEAD advanced to `04cf8c7a`** — `fix(qa): re-issue the UIA attach in a bounded loop, and report what
the pane step read` (4 files: `common-harness.mjs`, `native-driver.mjs`, `pane-binding.mjs`,
`pane-liveness.test.mjs`). That is the attach-loop lane's work landing. **None of this pass's numbers were
taken on `04cf8c7a`.**

### 8.2 A defect in my own probe's aggregation, corrected against the raw samples

The probe's summary field declared **`maxCount: 3`**. **That number is wrong, and the raw samples are the
authoritative record.** All 11 samples, verbatim:

```
atMs=2215  count=1  [cb71f12e-c415-440b-929a-63135dd74ca7]
atMs=2929  count=2  [cb71f12e…, a6f9a001-66df-4812-846a-e33db77d1681]
atMs=3642  count=2  (unchanged)      atMs=4354  count=2  (unchanged)
atMs=5064  count=2  (unchanged)      atMs=5773  count=2  (unchanged)
atMs=6487  count=2  (unchanged)      atMs=7202  count=2  (unchanged)
atMs=7915  count=2  (unchanged)      atMs=8624  count=2  (unchanged)
atMs=9333  count=2  (unchanged)
```

**Computed maximum = 2**; the declared `3` comes from my PowerShell aggregation line
(`$samples | Measure-Object -Property count -Maximum`) reading the wrong property on an `ArrayList` of ordered
hashtables. (`finalSessions: [None]` is the same class of artifact: `@($null)` after the daemon exited.)
**I am correcting it here rather than leaving it to be cited.** The classification in §4 does not depend on the
buggy field — it rests on the sample list, which shows **1 → 2 and then flat at 2 from 2929 ms**, with the pane
click at **7732 ms** and no change through 9333 ms.

**So the corrected statement of the key fact is:** the daemon's session count is **1, then 2 from ~2.9 s, and
stays exactly 2 across the click and 1.6 s after it** — **no new session was created by the pane step.**
