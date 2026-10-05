# Task 9 — PASS 4 consolidated report (`91d447e1`)

Verifier: sole remote verifier (pass 4). Date: 2026-10-05 (+0900).
Candidate: `C=/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`,
branch `work/local-pane-liveness-completion-foundation`, HEAD **`91d447e1`**
(`91d447e1f432f827dbcd07ff279a22bff66c6e02`), tree **clean**, unchanged by this pass.
Chain: `91d447e1` (E0275 + contract fix) → `1df40271` → `5245e6ba` → `314251e0` → `799a582d` → `c34b90fc` → … → `d82b35e4`.

## HEADLINE

**The `E0275` blocker is CLOSED on both hosts, the contract test is FIXED, and the pass-2/pass-3 fixture blocker is
CLOSED with a real daemon session.** The live budget is finally measured and lands **inside** the await. The
three Windows scenarios now reach a real launch and get **past `fixture-setup`** — and fail on a **new,
different** blocker in the **native-driver/scripts lane**: no owned *visible* window when driven over SSH,
and, once that is worked around, a **non-unique split selector**.

| Gate | Verdict |
|---|---|
| `cargo build --features local-split-qa` (linux 1.98.0 + Windows 1.97.0) | **PASS — exit 0, binary produced on both** |
| Default check / build / marker scan | **PASS — 0/0, 0/19 QA markers** |
| Four QA selectors | **PASS — 61 selected, 61/61 green** (incl. the corrected `:2669` contract test) |
| Live `fixtureCreationElapsedMs` + 9 000 ms await | **PASS — 103 ms creation, ~1.3–1.4 s to receipt, inside budget** |
| Three Windows scenarios | **FAIL — raw exit 1** (the typed `NATIVE_AUTOMATION_UNSUPPORTED`→4 mapping is overridden by `cleanupGate.ok=false`); reached a launch and passed `fixture-setup` |
| Image-reader lane | **no capture exists → no recognition verdict claimed** (correctly unclaimed, not failed) |

**Answer to pass 3's open question: daemon-sessions-vs-GUI-panes is NOT the next blocker.** No scenario
ever failed for a missing pane binding. The next blocker is the native driver's window/selector lane.

---

## 1. linux (omaki, `indo@100.91.254.71`), rustc/cargo 1.98.0, 12 cores

| # | Exact command | Raw exit | Selected | Asserted line | Verdict |
|---|---|---|---|---|---|
| 1 | `cargo build --manifest-path src-tauri/Cargo.toml --features local-split-qa` | **0** | — | `Finished dev profile [unoptimized + debuginfo] target(s) in 1m 47s` | **PASS** — was **101** at `1df40271` |
| 2 | `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` (default) | **0** | — | `Finished dev profile … in 23.14s` | **PASS** |
| 3 | `cargo build --manifest-path src-tauri/Cargo.toml` (default) | **0** | — | `Finished dev profile …` | **PASS** |
| 4 | `--lib --features local-split-qa qa_barrier -- --list` / run | **0** | **23** | `test result: ok. 23 passed; 0 failed` | **PASS** |
| 5 | `--lib --features local-split-qa qa_producers -- --list` / run | **0** | **10** | `ok. 10 passed; 0 failed` | **PASS** |
| 6 | `--lib --features local-split-qa qa_liveness -- --list` / run | **0** | **13** | `ok. 13 passed; 0 failed` | **PASS** |
| 7 | `--lib --features local-split-qa qa_split_producers -- --list` / run | **0** | **15** | `ok. 15 passed; 0 failed` | **PASS** |
| 8 | `--lib --features local-split-qa fixture_kind_claims_follow_the_daemons_own_reply` (inside gate 4) | **0** | 1 | `test … fixture_kind_claims_follow_the_daemons_own_reply ... ok` | **PASS** — the pass-3 `:2669` failure is **fixed** |

**Selected total 61, passed 61, failed 0.** QA binary: 964 870 536 B, `dc7ef194a1…`, **13/13** QA markers
present (`marker-scan-qa.txt`).

### 1.1 The `recursion_limit` claim, falsified as stated

`grep -c recursion_limit` → **0** in `src-tauri/src/ipc/qa_barrier.rs` and **0** in `src-tauri/src/lib.rs`.
The fix removes the auto-trait proof rather than raising the limit, exactly as the author described:

- `pub trait SurfaceHostObservations` (`qa_barrier.rs:1052`) with `impl<R: tauri::Runtime> … for tauri::AppHandle<R>` resolving
  `self.state::<NativeTerminalSurfaceHostState>().session_liveness_observation(..)` **in one statement, no `await`**;
- `collect_gui_fixture_sessions<H: SurfaceHostObservations>` (`:1097`) is generic over it;
- `emit_gui_fixture_setup` takes an **owned** `AppHandle` (`:1553`);
- the stale-binding watch moved to a **`std::thread` + current-thread runtime + `block_on`** (`:1459`), with
  `FERRYX_QA_STALE_BINDING_WATCH_UNSTARTED` on failure.

**My pass-3 hypothesis was correct and is now discharged**: the diff's own rationale names the same
mechanism (holding `State<'_, NativeTerminalSurfaceHostState>` across an `await` forcing the `Sync` proof
through the WGPU graph). Two residual `app.state::<…>()` uses remain — `:1520` (inside the thread-driven
watcher, no `Send` obligation) and `:2017` (test-only) — both consistent with the fix.

---

## 2. Windows (maho-win, `sook@100.126.171.58`), rustc/cargo 1.97.0

| # | Exact command | Raw exit | Asserted line | Verdict |
|---|---|---|---|---|
| W1 | `cargo build --manifest-path src-tauri/Cargo.toml --features local-split-qa` (8/8 `STAGED_OK`, 752 `.rs`/`.toml` touched, stale `ferryx.exe`/`.pdb` deleted) | **0** | `Finished dev profile [unoptimized + debuginfo] target(s) in 4m 58s`; `BIN_BYTES=106915840`, `BIN_SHA=6ad63c5afdcbb8348fccd0253d0cdb0066fc054bda99b41387b5bc5fbeb5b042` | **PASS** — was **101** at `1df40271` |
| W2 | default `cargo build` | **0** | `Finished dev profile …` | **PASS** |
| W3 | three scenarios, exact argv | **1** | `NATIVE_AUTOMATION_UNSUPPORTED: powershell exited 1: … NO_OWNED_WINDOW` → `verdict: FAIL`, `cleanupGate.ok: false` | **FAIL — reached a launch** (§3) |

### 2.1 Windows disk — environment, handled

Free space had fallen to **7.75 GB** (my pass-3 rebuild). I reclaimed **only this task's own**
`source-21dea3c0\src-tauri\target` and left every foreign tree intact; the build then completed.

---

## 3. The three scenarios — a real launch, and a NEW blocker

All three now settle `fixture-setup` from a **real daemon session** and continue. Verbatim from
`split-happy`'s `actions.jsonl`:

```json
{"action":"fixture-setup","sessions":[{"backendSessionId":"09e999ac-7a67-4ed1-bfb6-79f06948025c","kind":"source",
 "ownershipReceipt":{"backendSessionId":"09e999ac-…","cwd":"…\\barriers\\fixture-workspace\\",
 "daemonEpoch":"1791132040020","fixtureKindBasis":"daemon-reports-running","incarnation":"9e869318-6be2-4ecb-aaeb-354e401e4bf0",
 "kernelStopped":false,"qaCreated":true,"readerPaused":false,"registrySuspended":null,"running":true,
 "suspended":false,"suspensionSource":null,"workspaceId":"qa-fixture"}}]}
```

`qaCreated:true`, a real incarnation and a real daemon epoch — **the fixture constructor works end to end**.
`split-attach-stall` also records `barriers.registered: ["attach-handshake"]` after the fixture.

| Scenario | Raw exit | Asserted line | Reached `fixture-setup`? | Image-reader verdict | Verdict |
|---|---|---|---|---|---|
| `split-happy` | **1** | `NATIVE_AUTOMATION_UNSUPPORTED: … NO_OWNED_WINDOW` | **YES** | **none — no capture exists** | **FAIL** |
| `split-attach-stall` | **1** | same | **YES** (+ arm registered) | **none — no capture exists** | **FAIL** |
| `split-cancel` | **1** | same | **YES** | **none — no capture exists** | **FAIL** |

**No recognition claim is made** — no scenario reached the marker/capture step, so no screenshot exists.

### 3.1 Root cause of `NO_OWNED_WINDOW`: SSH lands in Windows session 0

Decisive probe (`win-window-probe.ps1`, launching the QA binary directly with the runner's env):

```
INTERACTIVE_SESSION=False
SESSION_ID=0
MAINWINDOWHANDLE_FIRST_NONZERO_MS=NEVER_WITHIN_20000
OWNED_TOPLEVEL_WINDOWS=3
    hwnd=24838986 pid=3924 visible=False class='T' title='F'
    hwnd=107151794 pid=3924 visible=False class='T' title=''
    hwnd=8389492  pid=3924 visible=False class='C' title='C'
```

and the session landscape:

```
 SESSIONNAME  USERNAME  ID  STATE   TYPE
>services               0  Disc
 console      sook      1  Active
explorer.exe pid 8712 → SessionId 1 ; winlogon 1360 → 1 ; dwm 1968 → 1
MY_SESSION=0
```

**The app does create three top-level windows, but every one is `visible=False`** because the process runs
in **session 0** (the SSH/service session), where no window can be shown. The driver's
`AutomationElement::FromHandle($proc.MainWindowHandle)` then sees `MainWindowHandle = 0` → `NO_OWNED_WINDOW`.
**This is an environment/harness condition, not a product defect.**

### 3.2 Working around it exposes the NEXT blocker: `SPLIT_RIGHT_NOT_UNIQUE`

Driving the same run **into the interactive console session 1** (a scheduled task with `/it`, which is the
authorized isolated-host GUI path) gets the driver much further — the app owns a real window and the UIA
enumeration succeeds:

```
"error": {"code":"NATIVE_AUTOMATION_UNSUPPORTED",
          "message":"… SPLIT_RIGHT_NOT_UNIQUE … at $items.Count -ne 1 { throw \"SPLIT_RIGHT_NOT_UNIQUE\" }"}
```

**So the driver found MORE THAN ONE element named `Split pane right`.** That is the next blocker, and it is
**in the native-driver/scripts lane** (`native-driver.mjs` `windowsDriver`, which asserts
`$items.Count -ne 1`), not in the product.

**Provenance of this session-1 result (one gap, stated).** The run's verdict and error above come from its
**stdout** (`logs/s1-split-happy.out`, 1 874 B, produced by the scheduled task). **No raw exit code is
available for this run**: the batch's `echo %ERRORLEVEL%` line was never reached because node hung on the
inherited stdio pipe after printing its result — the known runner/cleanup hang from passes 2–3 — so
`s1-split-happy.exit` does not exist (`EXISTS=False`), and the tree was reaped during teardown. The verdict
itself is unambiguous from the printed JSON; only the exit code is missing.

### 3.3 Raw exit codes — CORRECTED, and why they are 1 rather than 4

**A correction to this pass's own first reporting.** I initially read the three scenarios' raw exit as
**4**. That was wrong: I misread a **4-byte file length** in a directory listing as the file's *contents*.
The `.exit` files actually contain **`1`** (`"1 "` + CRLF = 4 bytes). Measured directly:

```
bat-split-happy.exit        → 1
bat-split-attach-stall.exit → 1
bat-split-cancel.exit       → 1
```

**Why 1 and not the runner's typed 4.** `pane-liveness.mjs`'s `finally` block ends with:

```js
if (!gate.ok) {
  result.verdict = 'FAIL';
  result.cleanupGate = { ...gate, gateFailed: true };
  exitCode = EXIT.scenarioFailure;      // 1 — overrides the typed mapping
} else { result.cleanupGate = { ...gate, gateFailed: false }; }
```

`NATIVE_AUTOMATION_UNSUPPORTED` maps to `EXIT.nativeAutomationUnsupported = 4`, but **`cleanupGate.ok` was
`false` on all six runs (both attempts), so the exit is overridden to `1`**. `result.json` still carries the
typed `code: NATIVE_AUTOMATION_UNSUPPORTED` — only the process exit differs.

### 3.4 The cleanup-gate failure is NOT my instrument — tested and exonerated

All six runs report `cleanupGate.ok=false, directoriesRemoved=false`: the runner could not remove the
isolation root. Because I had run a 150 ms-interval snapshot sidecar in passes 3–4 (added to preserve
barrier evidence the runner deletes), I **suspected my own instrumentation** and tested it rather than
asserting either way — a run of `split-happy` with **no snapshot and no reaper sidecar**, the only actors
being the runner and the product:

```
NODE_EXITED=False EXIT=HUNG
verdict=FAIL code=NATIVE_AUTOMATION_UNSUPPORTED
gate.ok=False dirsRemoved=False procsReaped=True
ISO_STILL_EXISTS=True
--- descendants holding the tree ---
  PID=24632 PPID=13520 ferryx_lib-edbc0e6372dc8389.exe
  PID=25412 PPID=27356 ferryx.exe
AFTER_REAP_EXITED=True ... ISO_REMOVED_AFTER=True
```

**The gate still failed without my sidecars, so my instrument is exonerated.** The real cause is the harness
defect already documented in passes 2–3: the runner's own reap is `taskkill /T` **without `/F`**
(`common-harness.mjs` `reapProcess`), which leaves the app's daemon descendant alive; that process holds the
isolation root (the fixture sessions' `cwd` is `…\barriers\fixture-workspace` inside it), so `rmSync` fails.
The same survivor holds the inherited stdio pipe, which is why node hangs. **Both symptoms are one harness
cause**, and both belong to the scripts lane now working on the driver.

**Ordered conclusion for the lead:** the pane-binding question is answered **no**; the real sequence is
(1) session-0 invisibility → (2) non-unique split selector. Both are harness-lane.

---

## 4. The live budget question — MEASURED

| Quantity | Measured | Source |
|---|---|---|
| **`fixtureCreationElapsedMs`** (from a real `fixture-setup` receipt) | **103 ms** | `win-budget-probe.ps1`, receipt line 0 (printed verbatim in this report; its isolation root was reclaimed after the evidence was read) |
| **`fixtureCreationElapsedMs`, second independent run** | **86 ms** | `window-probe-fixture-setup.receipt.jsonl` — the window probe's own receipt, kept as an artifact |
| launch → `fixture-setup` action (`split-happy`) | **1 353 ms** | runner `actions.jsonl` |
| launch → `fixture-setup` action (`split-attach-stall`) | **1 400 ms** | runner `actions.jsonl` |
| launch → `fixture-setup` action (`split-cancel`) | **1 326 ms** | runner `actions.jsonl` |
| Runner's await | 9 000 ms | `BUDGETS.stagePrepareCreateStatusMs` |
| Product's creation deadline | 3 500 ms | `FIXTURE_CREATE_BUDGET_MS` |
| **Inside the 9 000 ms await?** | **YES — by ~6.4×** | all three |

The pass-2 T+10 s lateness does **not** recur: the removed 20 × 250 ms retry loop was the cause, and the
real path costs ~1.3 s end to end. **Two independent runs agree** (103 ms and 86 ms), so the figure is not
a single sample.

**Producer naming note (artifact hygiene):** the preserved receipt file in this bundle is
`window-probe-fixture-setup.receipt.jsonl` (86 ms) — it comes from the **window** probe, not the budget
probe. It was briefly mis-named during retrieval and has been corrected. The 103 ms budget-probe receipt
is quoted verbatim above from its live console output; its isolation root was reclaimed after the evidence
was read. Both receipts carry `"producer":"ipc-qa-barrier"`.

**Honest limitation on both probes:** they run the product **directly** with the runner's env, not through
`pane-liveness.mjs`. In both, the fixture workspace registration failed for a reason that is an artifact of
my probe root (`repo_root … must be the canonical repository root '\\\\?\\C:\\Users\\sook'`), because the
probe placed the barrier dir under `task9-91d447e1\…`. In the **real runner runs the registration
succeeded** (\u00a73, `qaCreated:true`), so the probe's `fixtureCreationElapsedMs` measures the *attempt* cost,
which is what the budget question asks, and the `sessions:[]` in the probe receipts is **not** a product
failure. The real runs' launch\u2192`fixture-setup` timings (1 326\u20131 400 ms) are the end-to-end evidence.

---

## 5. Self-inflicted contamination — disclosed in full

My pass-3-style diagnostic (**add `#![recursion_limit = "256"]` to `lib.rs` on the linux staging copy only,
to distinguish "deep but satisfiable" from "genuinely `!Sync`"**) **overlapped the gate script's selector
phase** on the same host. Consequences, and what I did:

- The first linux `QA_BUILD_EXIT=101` and the first `04-list-*` runs of this pass are **contaminated** and
  are **not cited**. The 101 was **my probe's** `lib.rs` under a concurrent build, not the candidate.
- I killed the probe and the gate script, then restored `lib.rs` from the **authoritative committed blob**
  and verified: `sha256 = b32b3d3dbc274934c921cbcbc5d4cddc6418964edb9dafbf8e341c8dc8b93d57`,
  `recursion_limit` count **0**, head `pub mod account;`.
- I then **re-ran the whole linux gate cleanly**; every linux number in §1 comes from that clean re-run.
- Lesson for pass 5: never patch the shared staging tree while a gate script is running against it; the
  probe belonged on a separate copy.

Also environmental, and re-run rather than reported as a verdict: the first default `cargo check` failed
with `error writing dependencies to /tmp/sccache…/deps.d: Disk quota exceeded (os error 122)`; the clean
re-run is **exit 0** (§1 gate 2).

---

## 6. Residual classification

| Item | Class | Evidence |
|---|---|---|
| `E0275` on `cargo build --features local-split-qa` | **candidate-caused → FIXED at `91d447e1`, both hosts** | gates 1 / W1 exit 0 |
| `:2669` contract test failure | **candidate-caused → FIXED** | gate 8 `… ok` |
| pass-2/3 fixture blocker (`fixture-setup … got 0`) | **FIXED — real session created** | §3 receipt `qaCreated:true` |
| Live budget / 9 s await | **PASS — 103 ms creation, ~1.3 s to receipt** | §4 |
| `NO_OWNED_WINDOW` on the three scenarios | **environment/harness — SSH lands in Windows session 0**; the app's 3 windows are all `visible=False` | §3.1 |
| Raw exit `1` rather than the typed `4` | **harness — `cleanupGate.ok=false` overrides `EXIT.scenarioFailure=1`**; `result.json` still carries the typed code | §3.3 |
| `cleanupGate.ok=false` / `directoriesRemoved=false` on all six runs | **harness — the runner's reap is `taskkill /T` without `/F`, leaving the daemon descendant holding the isolation root**; **my sidecar is exonerated by a no-sidecar control run** | §3.4 |
| My first report of "raw exit 4" for the three scenarios | **verifier error — a 4-byte file *length* misread as the exit *code*; corrected to 1 with the cause** | §3.3 |
| `SPLIT_RIGHT_NOT_UNIQUE` once run in session 1 | **harness-lane (native-driver/scripts)** — selector matches >1 element | §3.2 |
| daemon-sessions-vs-GUI-panes (pass 3's open question) | **ANSWERED: not a blocker** — no scenario failed for a missing pane binding | §3 |
| QA surface leaking into the default build | **not observed** | 0/19 markers in the default binary |
| `recursion_limit` used as the fix | **claim falsified as stated — 0 occurrences** | §1.1 |
| `os error 122` (linux `/tmp` quota) on the first default check | **environment** | clean re-run exit 0 |
| `os error 112` (Windows disk) | **environment** | reclaimed own cache; build then 0 |
| My probe contaminating the first linux run | **verifier error, self-corrected and disclosed** | §5 |
| `externally-stopped` / `adopted` | **structurally-not-run** — `FixtureCreation::Unsupported(hook)`; these scenarios failing truthfully at `fixture-setup` is the **expected** outcome, not a defect | author-confirmed + source |

---

## 7. What the parked mac half must still cover

1. **The session-0 / interactive-session question on mac** — mac has no equivalent of Windows session 0
   for a logged-in user, but the harness must still prove it drives a **visible** owned window; record the
   session/desktop identity the way §3.1 does.
2. **The `SPLIT_RIGHT_NOT_UNIQUE` selector** — this is now the first native blocker and is harness-lane; it
   should be fixed before any scenario can pass on either platform. The driver must either scope the search
   to the intended pane's toolbar or assert on a genuinely unique affordance.
3. **Then re-run the three Windows scenarios** (they are blocked only by items 1–2) and, on mac, the full
   nine-scenario matrix.
4. **Re-run the compile gate on mac** — the mac-only `cfg` arms of `native_terminal/surface_host.rs` /
   `ipc/native_terminal.rs` are still not compiled by either remote gate; note the E0275 class of failure is
   compiler-version sensitive (linux 1.98 vs Windows 1.97 behaved differently at `1df40271`).
5. `diagnostic-classifier` (native + the deferred headless smoke, still NOT_RUN per F1 H-23),
   `retained-handover`, `handover-abort`, `suspension-ownership`, `split-concurrent`, `stale-binding`.
6. **An image reader that preserves the `_` separator** — the pass-2 OCR instrument cannot distinguish
   `FERRYX_SPLIT_READY` from `FERRYX SPLIT READY` and must not be used on mac either. No capture exists yet
   on either host, so **no recognition verdict is claimed** in this pass.

---

## 8. Cleanup receipts

| Resource | Teardown | Receipt |
|---|---|---|
| linux gate script + my recursion probe | killed; `lib.rs` restored from the committed blob and hash-verified | `LIB_HASH_RESTORED=b32b3d3d…` = expected; `recursion_limit_count=0` |
| linux scratch (`linux-restore.sh`, probe script, `/tmp/lib.rs.orig`, `lib.rs.authoritative`) | removed / superseded | `RESTORE_DONE` |
| **windows own `src-tauri\target`** | reclaimed for disk (needed for the QA build) | `FREE_BEFORE_GB=7.75` → build succeeded |
| windows scheduled task `ferryx-t9p4-split-happy` | deleted after the run | `schtasks /delete` in `logs/s1-split-happy.log` |
| windows scenario processes (all three drivers, probe binaries, budget/window probes) | `taskkill /T /F` on this session's own staged-tree PIDs only | per-run `reaper4-*.log`; `BUDGET_PROBE_DONE`, `WINDOW_PROBE_DONE` |
| **foreign trees/processes on both hosts** | **untouched** | listed, not modified |
| staged trees (`source-21dea3c0`, `task9-91d447e1`) | **retained** as the evidence source (owned) | — |
| user desktop / production app / production daemon | **never addressed** | the session-1 task ran the *test* binary from the staging tree only |
