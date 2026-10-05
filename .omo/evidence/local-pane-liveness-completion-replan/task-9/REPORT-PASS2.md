# Task 9 — PASS 2 consolidated report (`314251e0`)

> ## SUPERSESSION NOTE (added after this report was written)
>
> **Every measurement in this report is bound to `314251e0` and describes `314251e0` only.** While the pass
> was closing, the two defects it routed were repaired and **committed as `5245e6ba`**, which is now HEAD
> (tree clean):
>
> | Commit | Subject | Contents |
> |---|---|---|
> | **`5245e6ba`** | `feat(qa): declare the scenario's fixture kinds to the product and fix the gated control test` | the adapter half of §3's gap (a single `fixtureKindsForScenario` derivation + `FERRYX_QA_FIXTURE_KINDS` in the isolated launch env) **and** the §2 test fix (`RETRY` arm + `scan_and_ack_arms()`) |
>
> Three consequences for pass 3, so nothing stale is cited:
> 1. **`qa_split_producers` is expected 15/15 at `5245e6ba`** — the §2 failure below is **fixed**. Re-measure
>    it; do not carry "14/15" forward as current.
> 2. **§3's blocker is only half-fixed at `5245e6ba`.** The declaration now reaches the product, but the
>    **product-side consumer is still being authored** (lane `st_01a10780`): at `5245e6ba`
>    `grep -rn "FERRYX_QA_FIXTURE_KINDS|fixture_kinds" src-tauri/src/` is still **0 hits**, so the scenarios
>    still cannot pass. Do not read the green adapter half as a working fixture pipeline.
> 3. My read-only call that the in-flight repair was **incomplete** (§7) was **correct and expected** — the
>    consumer half was simply not authored yet.
>
> The lead accepted Phase 1 GREEN, the §3 blocker, the three §5 corrections and the §6 cleanup as written.

Verifier: sole remote verifier (pass 2; successor of `st_01a10728`, whose dispatch the lead closed).
Date: 2026-10-04/05 (+0900).
Candidate: `C=/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`,
branch `work/local-pane-liveness-completion-foundation`, HEAD **`314251e0`**
(`314251e08e1457ebf611953edf259e2178bd9a07`), tree **clean**, unchanged by this pass (no product or
adapter code edited; `.omo/` is untracked).
Chain: `314251e0` (compile fixes) → `799a582d` (barrier role map) → `c34b90fc` → `dd9e6813` → `70eefafe`
→ `39e722ce` → … → base `d82b35e4`.

## HEADLINE

**The Phase 1 compile gate is GREEN — the predecessor's 16-site failure is fully closed.** But the Phase 2
scenarios are still **NOT_RUN_BLOCKED**, now for a *different, reproduced, candidate-caused* reason: **the
isolated profile the runner launches contains zero sessions and no code anywhere constructs one**, so every
scenario dies at `fixture-setup` before any native action.

| Host | Phase 1 (compile/test gate) | Phase 2 (native scenarios) |
|---|---|---|
| linux (omaki) | **PASS with one gated-test FAIL** | structurally-not-run (the runner refuses non-macOS/Windows) |
| Windows (maho-win) | QA **binary build** PASS; QA **lib selectors** not run this pass | **NOT_RUN_BLOCKED ×3** |

Two honesty corrections to the inherited record are recorded below (§5): the `strings` revision proof
**is** available on the QA binary (it is unavailable only on the default binary), and the `split-cancel`
hang mechanism is now pinned.

---

## 1. linux (omaki, `indo@100.91.254.71`), rustc/cargo 1.98.0, 12 cores

cwd `/home/indo/ferryx-pane-completion/source-21dea3c0` @ `314251e0`. Full detail:
`compile-pass2/PHASE1-PASS2-COMPILE-VERDICT.md`; logs `compile-pass2/linux/logs/`.

| # | Exact command | Raw exit | Selected | Asserted line | Verdict |
|---|---|---|---|---|---|
| 1 | `cargo check --manifest-path src-tauri/Cargo.toml --all-targets --features local-split-qa` | **0** | — | `Finished dev profile … in 1m 41s` | **PASS** (was **101** at `c34b90fc`) |
| 2 | `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` | **0** | — | `Finished dev profile … in 19.10s` | **PASS** |
| 3 | `cargo build --manifest-path src-tauri/Cargo.toml` (default) | **0** | — | `Finished dev profile …` | **PASS** |
| 4 | `cargo build --manifest-path src-tauri/Cargo.toml --features local-split-qa` | **0** | — | `Finished dev profile …` | **PASS** — QA binary produced |
| 5 | `--lib --features local-split-qa qa_barrier -- --list` | **0** | **19** | 19 `: test` lines | **PASS** |
| 6 | `--lib --features local-split-qa qa_barrier -- --nocapture --test-threads=1` | **0** | **19** | `test result: ok. 19 passed; 0 failed; … finished in 0.06s` | **PASS** (was 11/0 at `70eefafe`) |
| 7 | `--lib --features local-split-qa qa_producers -- --list` | **0** | **10** | 10 `: test` lines | **PASS** |
| 8 | `--lib --features local-split-qa qa_producers -- --nocapture --test-threads=1` | **0** | **10** | `ok. 10 passed; 0 failed; … finished in 0.00s` | **PASS** |
| 9 | `--lib --features local-split-qa qa_liveness -- --list` | **0** | **13** | 13 `: test` lines | **PASS** |
| 10 | `--lib --features local-split-qa qa_liveness -- --nocapture --test-threads=1` | **0** | **13** | `ok. 13 passed; 0 failed; … finished in 0.67s` | **PASS** |
| 11 | `--lib --features local-split-qa qa_split_producers -- --list` | **0** | **15** | 15 `: test` lines | **PASS** (the lead's source-derived "15" confirmed by execution) |
| 12 | `--lib --features local-split-qa qa_split_producers -- --nocapture --test-threads=1` | **101** | **15** | `FAILED. 14 passed; 1 failed; … 2734 filtered out; finished in 0.00s` | **FAIL — 1 test** (§2) |
| 13 | negative control: `node scripts/qa/pane-liveness.mjs --scenario split-happy --binary <QA binary> --evidence-dir /tmp/t9p2-linux-ev --isolation-root /tmp/t9p2-linux-iso` | **4** | — | `NATIVE_AUTOMATION_UNSUPPORTED: native desktop automation is not implemented for platform linux` / `verdict: BLOCKED` | **structurally-not-run** (the runner is macOS/Windows-only; refuses before any launch) |

**Count correction:** the dispatch predicted "roughly 48 gated tests". Measured: **57 selected /
56 passed / 1 failed** — `qa_barrier` **19** + `qa_producers` 10 + `qa_liveness` 13 + `qa_split_producers`
15. (`qa_barrier` holds 19 `#[test]` fns, not the 10 the earlier source estimate assumed.)

**No QA surface leaks into a default build** (`compile-pass2/linux/marker-scan-default.txt`, default
binary `a89c67c7…`): all 14 QA markers **ABSENT**, both candidate-lineage literals
(`Attach requires the persisted seven-field pane binding`, `Attach binding incarnation cannot be proven`)
**PRESENT** as a positive control.
**The QA binary really carries the surface** (`marker-scan-qa.txt`, `c653b034…`, 964 747 448 B):
**15/15 QA markers PRESENT**.

---

## 2. The one gated test failure (test-only, candidate-caused, routed) — **FIXED at `5245e6ba`**

> **Status update:** this failure is **repaired and committed as `5245e6ba`** (HEAD at the time of writing).
> The fixture now writes the `RETRY` arm and calls `channel.scan_and_ack_arms()`, exactly the identity its
> passing sibling establishes; assertions untouched and `read_control`'s production behaviour unchanged.
> **`qa_split_producers` is expected 15/15 at `5245e6ba` and must be re-measured, not cited as 14/15.**
> Everything below is the pass-2 measurement **at `314251e0`**, kept because it is the evidence that routed
> the fix.

```
cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_split_producers -- --nocapture --test-threads=1

running 15 tests
test ipc::terminal::qa_split_producers::tests::retry_and_batch_controls_are_read_only_for_this_run ...
thread '…' panicked at src/ipc/terminal.rs:3876:17:
assertion failed: read_control(&dir, name, &channel).is_some()
FAILED
test result: FAILED. 14 passed; 1 failed; 0 ignored; 0 measured; 2734 filtered out; finished in 0.00s
error: test failed, to rerun pass `--lib`
```

Reproduced deterministically in isolation (`0 passed; 1 failed` on a single-filter rerun), so it is not a
load artifact.

**Root cause (source-derived, reproduced):** the fixture builds `QaBarrierChannel::new(dir, "qa-run-gui")`
and writes a control echoing the run id plus a nonce, then requires
`read_control(..).is_some()`. It never supplies the operation nonce, and `new()` leaves
`operation_nonce: None` (`qa_barrier.rs:311`). `read_control` refuses anything it cannot correlate
(`let expected = channel.operation_id()?;`), and `operation_id()` (`:523`) falls back to the first *armed*
spec only. With no arm file written and no env nonce, `operation_id()` is `None`, so **every** control is
refused — including the one the test expects honored. Its sibling
`control_files_are_read_only_for_this_run` (`:3446`) writes an arm and calls
`channel.scan_and_ack_arms()` first, and **passes**; that pins the missing step.

**Owning lane: the QA producer/consumer lane that authored `ipc/terminal.rs`'s `qa_split_producers` module**
(test-only; no production behaviour implicated — the runner always exports `FERRYX_QA_OPERATION_ID`, and
the refusal is the intended fail-closed behaviour for an uncorrelatable control).

---

## 3. Windows (maho-win, `sook@100.126.171.58`)

Full detail: `win-pass2/WINDOWS-PASS2-VERDICT.md`; logs `win-pass2/logs/`; raw run artifacts
`win-pass2/runs/`; tooling `win-pass2/tooling/`.

| # | Exact command | Raw exit | Selected | Asserted line | Verdict |
|---|---|---|---|---|---|
| W1 | `cargo build --manifest-path src-tauri/Cargo.toml --features local-split-qa` (staged `314251e0`, ghostty junction, 929 `.rs`/`.toml` touched, stale `ferryx.exe`/`.pdb` deleted) | **0** | — | `Compiling ferryx v2026.928.7` → `Finished dev profile … in 3m 49s`; binary 106 842 112 B, `bbbcdf14…` | **PASS** |
| W2 | `node scripts/qa/pane-liveness.mjs --scenario split-happy --binary … --evidence-dir … --isolation-root …` | **7** | n/a | `"verdict": "BLOCKED"` / `BARRIER_ACK_TIMEOUT: product did not settle barrier fixture-setup receipt[0] within 9000ms` / `cleanupGate.ok: true` | **NOT_RUN_BLOCKED** |
| W3 | same argv, `split-attach-stall` | **7** | n/a | same `BLOCKED` line; `barriers[0] = {name: "attach-handshake", armedAt: 2026-10-04T15:16:01.631Z, heldAt: null, receiptCount: 0}` | **NOT_RUN_BLOCKED** |
| W4 | same argv, `split-cancel` | **7** | n/a | same `BLOCKED` line | **NOT_RUN_BLOCKED** |
| W5 | `--features local-split-qa` **lib test selectors** | — | — | — | **NOT_RUN** this pass (the QA **binary** build was the gate the dispatch asked for; the QA selectors were measured on linux) |
| W6 | `--test daemon_handover_transfer_contract` / `ipc_hardening_contract` | **0** | **0** | targets carry `#![cfg(unix)]` → compile to nothing | **structurally-not-applicable — NOT a pass** |
| W7 | Windows suspension | — | — | `terminal/suspension/windows.rs` returns typed `UnsupportedPlatform` | **structurally-not-run** |

### 3.1 Scenario table with image-reader verdicts

| Scenario | Raw exit | Asserted line | Reached a launch? | Image-reader verdict |
|---|---|---|---|---|
| `split-happy` | **7** | `BLOCKED` / `BARRIER_ACK_TIMEOUT … fixture-setup receipt[0] within 9000ms` | **YES** — `launch.binary` recorded with the isolated env; the product ran and produced a real isolated profile | **none — no capture produced** |
| `split-attach-stall` | **7** | same, plus a real `armedAt` for `attach-handshake` | **YES** — pre-arm then launch; the arm was accepted | **none — no capture produced** |
| `split-cancel` | **7** | same | **YES** | **none — no capture produced** |

**The adapter repair is proven effective**: `split-attach-stall` now pre-arms and launches where it
previously died pre-launch with `ASSERTION_FAILURE: targetRole … got "producer"` (exit 1, no `latest.json`).

### 3.2 The new blocker, with both producers named

**Decisive manual probe** (product launched by hand with the runner's exact env under a fresh root;
`win-pass2/manual-probe/fixture-setup.receipt.jsonl`):

```json
{"fixtureKind":"gui-session-inventory","operationId":"qa-op-manualprobe","producer":"ipc-qa-barrier",
 "producerPid":16472,"runId":"qa-run-manualprobe","sessionId":"","sessions":[],"settledAtMs":1791126373337}
```

`sessions: []`, byte-identical at T+10 / T+20 / T+40 s; no receipt at T+5 s. `daemon.log` shows only the
daemon's own boot + startup-workspace registration; `paired_descriptors.json` = `{}`.

- **Collector (product) — present and honest.** `collect_gui_fixture_sessions`
  (`src-tauri/src/ipc/qa_barrier.rs:896`) enumerates the isolated profile's sessions and refuses to claim
  lifecycle kinds it cannot substantiate. Boot wiring is real (`start_gui_boot_channel` from
  `lib.rs:1359`; `emit_gui_fixture_setup` retries 20 × 250 ms then settles).
- **Constructor (runner/adapters) — MISSING.** `buildIsolatedEnv`
  (`scripts/lib/qa-scenarios/diagnostic-classifier.mjs:27-52`) creates `data`/`runtime`/`home` and launches
  the binary; **nothing creates a session in that profile** —
  `grep -rn "spawnTerminal|spawn_terminals|createSession" scripts/lib/qa-scenarios/*.mjs scripts/qa/pane-liveness.mjs`
  → **0 hits**. `SCENARIO_PLANS` has no fixture-construction step.

The plan (`.omo/plans/local-pane-liveness-completion-replan.md:178`) requires "basic split needs
**source+created target**" and assigns it to the scripts/QA-adapter lane; the **source** half has no
producer on either side. **Owning lane: QA adapter/scripts.**

**Classification: candidate-caused, reproduced, blocking all three Windows scenarios and every mac
scenario.** It is *stronger* than the predecessor's blocker: it survives both a green QA binary and the
repaired role map.

**Two faces of the same gate, both recorded:** when the settlement lands inside the runner's 9 s budget the
receipt arrives with zero sessions → `verdict: FAIL` / `ASSERTION_FAILURE … got 0`; when it misses (load
45–88 %), → `verdict: BLOCKED` / `BARRIER_ACK_TIMEOUT`. Neither is a pass.

---

## 4. Residual classification

| Item | Class | Evidence |
|---|---|---|
| `--features local-split-qa` compile failure, 16 sites, `c34b90fc` | **candidate-caused → CLOSED** | gate 1 exit 0 at `314251e0`; predecessor A/B exit 101 at `c34b90fc` with pre-delta hashes |
| `qa_barrier` gate previously uncompilable (11/0 at `70eefafe`) | **regression CLOSED** | 19 selected, `ok. 19 passed; 0 failed` |
| adapter `BARRIER_ROLES` self-contradiction, **3 of 9** scenarios | **candidate-caused → CLOSED** | `799a582d`; `split-attach-stall` pre-arms, launches, stamps `armedAt`; pre-arm replay test green |
| `qa_split_producers::retry_and_batch_controls_are_read_only_for_this_run` | **candidate-caused, test-only** | §2, deterministic reproduction |
| `fixture-setup` empty inventory (all native scenarios) | **candidate-caused, NEW, reproduced** | §3.2 |
| `split-cancel` runner hang (descendant holds the stdio pipe) | **environment/robustness, reproduced** | §5.2 |
| Default build / default binary gaining QA symbols | **not observed** | default check exit 0; 0/14 QA markers in the default binary |
| `#![cfg(unix)]` integration targets selecting 0 on Windows | **structurally-not-run** | 0 selected is not a pass |
| Windows suspension | **structurally-not-run** | typed `UnsupportedPlatform`; never reached |
| Binary sha256 differs between identical Windows builds | **environment** | respected; no hash used as a revision argument |
| Dispatch's "~48 gated tests" estimate | **corrected** | measured **57** via `--list` on all four selectors |
| `strings`-for-a-symbol revision proof "impossible on maho-win" | **corrected for the QA binary** | QA 14/14 vs default 0/14 QA literals, same method |

---

## 5. Corrections to the inherited record (kept, not buried)

### 5.1 The `strings` revision proof is available on the QA binary

The dispatch and the predecessor both record that the symbol/`strings` proof "does NOT work on this host".
**True for the default binary, false for the QA binary.** Measured with the same byte-scan on the same
host: the **default** binary carries **0/14** QA literals (absent by construction, so it is a valid
*negative* control), while the **QA** binary carries **14/14** (`FERRYX_QA_*` env names, `fixture-setup`,
`split-create`, `attach-handshake`, `cancel-ack`, `held-rpc`, `marker-output`,
`split-concurrent-batch`, `qa_producers`, `qa_liveness`). Rust *symbol* names remain in the absent `.pdb`
and are **not** used as evidence. The predecessor's other note stands: binary sha256 is not a revision
proof on this host.

### 5.2 The `split-cancel` hang is reproduced for all three scenarios, and explained

The predecessor attributed it to "a descendant of the spawned app holding the inherited stdio pipe".
Reproduced here for **all three** scenarios (not just `split-cancel`), and pinned: the runner's cleanup
calls `taskkill /T /PID <pid>` **without `/F`** (`common-harness.mjs` `reapProcess`), which does not
terminate the app's daemon grandchild; the grandchild keeps the inherited stdout/stderr handles open, so
node never drains its event loop. The runner prints its complete `result.json` **before** hanging, so the
log looks finished. Reaping only this session's staged-tree `ferryx.exe` PIDs lets node exit with its real
code (**7**) — that is how §3's raw exits were captured.

---

## 6. Cleanup receipts

| Resource | Teardown | Receipt |
|---|---|---|
| **`ferryx.exe` PID 23672** (inherited leak from the predecessor's hung Windows full `--lib`, started 23:55:58) | identity checked first: `ExecutablePath` under `…\ferryx-pane-completion\source-21dea3c0\…` → **matched the task staging root**, not the user's install; then `taskkill /T /F` | reaped 00:00; follow-up sweep `LEAK_GONE PID=23672`, `TASK_OWNED_ALIVE_COUNT=0` |
| other task-owned processes under the staging root | enumerated by `ExecutablePath`/`CommandLine` | none remained (`TASK_OWNED_ALIVE_COUNT=0`) |
| **foreign processes — untouched, listed** | 9 × `cargo.exe` (other sessions' `cargo test --lib … remote::managed_chat…`), 2 × `node.exe` `omo-ai`, 1 × `node.exe` `ferryx-landing-verify` astro dev, 1 `minio` `cmd.exe` | `FOREIGN_KEPT` for PIDs 18524, 27036, 16196, 20164, 13520, 16168, 10060, 4716, 19780, 11332, 16020, 22124, 2348 |
| this pass's own runner trees (v3/v4/v5/v6/bat drivers) | `taskkill /T /F` per own PID | `KILLV4_DONE`; per-run `bat-reaper-*.log` / `reaper-*.log` |
| manual-probe PID 16472 + 6 children | `taskkill /T /F` | `MANUAL_PROBE_DONE` |
| per-scenario isolation roots | runner's own `cleanupGate` + driver re-removal before each run | `cleanupGate.ok: true`, `directoriesRemoved: true`; `ISO_EXISTS_BEFORE_LAUNlse` per run |
| linux background sessions / monitors | all completed on their own sentinels | `GATE1_QA_CHECK_EXIT=0`, `LINUX_GATES_PHASE_A_DONE`, `LINUX_QABUILD_DONE`, `watcher completed (exit code 0)` |
| linux `/tmp/t9p2-linux-{iso,ev}` | removed by the runner's own cleanup before returning | `cleanupGate.ok` in `compile-pass2/linux/linux-scenario-split-happy.log` |
| staged trees (`source-21dea3c0`, `task9-314251e0`) | **retained** as the evidence source (owned) | — |
| user desktop / production app / production daemon | **never addressed** | — |

---

## 7. Live-state observation at report time (another session's work — since committed as `5245e6ba`)

> **Resolved after this report:** the work described here was **committed as `5245e6ba`**. My "incomplete"
> call below was **correct and expected** — the product-side consumer is still being authored by lane
> `st_01a10780`, so the second half of the gap is genuinely outstanding, not a defect in this commit.

While this pass was closing, **three files went dirty in the shared candidate tree** —
`scripts/lib/qa-scenarios/common-harness.mjs`, `scripts/lib/qa-scenarios/diagnostic-classifier.mjs`,
`src-tauri/src/ipc/terminal.rs` (44 insertions, 3 deletions). They are **another session's in-flight work**,
not this pass's (I edited no product or adapter code). Read-only inspection shows it addresses **exactly
the two defects this pass routed**:

- `terminal.rs` — the `qa_split_producers` fixture now writes a `retry.arm.json` and calls
  `channel.scan_and_ack_arms()` before asserting, i.e. §2's root cause.
- `common-harness.mjs` + `diagnostic-classifier.mjs` — a single `fixtureKindsForScenario` derivation point,
  and a new `FERRYX_QA_FIXTURE_KINDS` env var passed to the product in the isolated launch env, i.e. §3's
  missing declaration.

**But the repair is incomplete as of this writing: the product has no consumer for
`FERRYX_QA_FIXTURE_KINDS`.** `grep -rn "FERRYX_QA_FIXTURE_KINDS|fixture_kinds|fixtureKinds" src-tauri/src/`
→ **0 hits** (the only occurrences are in `scripts/`). Declaring the kinds to the product does not make the
product provision anything, so the `fixture-setup … got 0` gate would still fail if this tree were built
as-is. Flagged for the lead as a live state observation — **not verified by me, not built, not run**.
**Confirmed by the lead as expected:** the consumer half is being authored by lane `st_01a10780` (one
session per requested kind, spawned through the real daemon inside the 9 s `stagePrepareCreateStatusMs`
budget, then `fixture-setup` settled from the real inventory). It is **not** in `5245e6ba`.

**My measurements are unaffected by that dirty tree.** Every artifact above came from `git archive` of the
**committed** `314251e0` blobs, never from the working tree, and the host-side pre/post hashes were checked
against `git cat-file blob 314251e0:<path>` at staging time. Confirmed again at report time:

```
scripts/lib/qa-scenarios/common-harness.mjs   worktree=62634b72c65d0386  committed314251e0=ec15855d53f41d9a
scripts/lib/qa-scenarios/diagnostic-classifier.mjs worktree=b9dc896ac6935a8b committed314251e0=0cda96d79d825910
src-tauri/src/ipc/terminal.rs                worktree=42744ef8a86fa5dc  committed314251e0=776f423d37170cf8
```

---

## 8. What the parked mac half must still cover

### 8.1 Explicit carry-forward into pass 3 (per the lead's direction)

1. **Re-measure the four QA selectors at the new HEAD (`5245e6ba`), including `qa_split_producers` 15/15.**
   The `314251e0` numbers in §1 (including the 14/15) are historical.
2. **The three Windows scenarios on a QA-feature binary once the fixture constructor exists** (the
   `st_01a10780` product half) — same fail-closed discipline, and **no recognition claim unless a capture
   actually exists**.
3. **The OCR instrument must NOT be used as the recognition instrument on mac either.** My recognizer
   (`win-pass2/tooling/win-recognizer.ps1`) **cannot distinguish `FERRYX_SPLIT_READY` (underscores) from the
   space-separated form** — verified by reading its own self-test PNG (`win-pass2/captures/recognizer-selftest.png`),
   which shows literal underscores while OCR returns spaces.
4. **The mac image lane needs a reader that preserves the separator** — a human-equivalent viewer, or a
   segmentation step that keeps punctuation. It must not inherit this instrument.
5. **Disclosure to carry:** the `multimodal-looker` child was **unavailable** to this session (subagent
   nesting depth limit), so the self-test image read in §4 is **mine, disclosed as such** — not an
   independent reader's verdict.

### 8.2 The remaining mac scope

1. **The fixture-construction gap (§3.2) — fix first.** It blocks *every* scenario on *both* platforms, so
   no mac scenario can pass until a real `source` session exists before `fixture-setup` line 0. Owning
   lane: QA adapter/scripts.
2. **Re-run the compile gate on mac** — the mac-only `cfg` arms of `native_terminal/surface_host.rs` /
   `ipc/native_terminal.rs` are not compiled by the linux gate.
3. `diagnostic-classifier` (native **and** the deferred headless smoke, still NOT_RUN per the F1 audit
   H-23), `retained-handover`, `handover-abort`, `stale-binding`, `suspension-ownership` (still blocked by
   the `externally-stopped` fixture-kind gap: `common-harness.mjs:180-190` + `qa_barrier.rs:754/756`).
4. The four split scenarios on a QA-feature binary — three of them were also killed pre-launch by the
   adapter role map before `799a582d`.
5. The independent image-reader lane on real captures. The **Windows** recognition instrument is built and
   self-tested (`win-pass2/tooling/win-recognizer.ps1`, `SELFTEST_MATCH=True` on a synthetic marker);
   mac still needs one. No capture exists yet on either host, so **no recognition verdict is claimed**.
   Two limitations of that instrument are recorded so the parked lane does not inherit it as stronger
   than it is: (a) the pane rectangle it reports is the runner-claimed one, and only the marker's
   position inside it is independently measured; (b) the OCR engine's raw output collapses the `_`
   separator to a space, so it **cannot distinguish `FERRYX_SPLIT_READY` from `FERRYX SPLIT READY`** —
   I verified this by reading the self-test PNG myself (`win-pass2/captures/recognizer-selftest.png`):
   the image really shows underscores while OCR returned spaces. A successor lane should use a reader
   that reproduces the separator exactly. (The `multimodal-looker` child was unavailable to this
   session — subagent nesting depth limit — so that read is mine, disclosed as such.)
6. Task 8's two host-state unknowns (whether the verifier's `pkill` landed; whether the 13 GB `target`
   reclaim finished) — neither is a product finding.
7. The QA **lib test selectors on Windows** (W5) — measured on linux this pass, not on Windows.
