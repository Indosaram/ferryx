# Task 9 — Windows half PASS 2: the three native scenarios at `314251e0`

> **Supersession:** every measurement here is bound to **`314251e0`**. Two things changed after this was
> written, both committed as **`5245e6ba`**: the adapter half of §3.3 landed (a single
> `fixtureKindsForScenario` derivation + `FERRYX_QA_FIXTURE_KINDS` in the isolated launch env), and the
> `314251e0` gated-test failure was fixed. **The scenarios still cannot pass at `5245e6ba`** — the
> product-side consumer of `FERRYX_QA_FIXTURE_KINDS` is still being authored (lane `st_01a10780`), so
> `grep -rn "FERRYX_QA_FIXTURE_KINDS" src-tauri/src/` is still 0 hits. The §3.3 blocker stands until that
> lands; re-run these three scenarios only then, and re-measure rather than citing this file's numbers.

Verifier: sole remote verifier (pass 2, successor of `st_01a10728`). Date: 2026-10-05 (+0900).
Host: **maho-win** (`sook@100.126.171.58`, `DESKTOP-1LAPJMP`, Windows 10.0.26200.9457 x64, rustc/cargo
1.97.0, Node v24.19.0, Bun 1.4.0). Staging root `C:\Users\sook\ferryx-pane-completion\source-21dea3c0`.
Boot time `2026-10-03 10:33:16` (unchanged all pass); free space 24.5–24.8 GiB; host load 41–88 %
recorded per run.

**VERDICT: all three Windows scenarios are `NOT_RUN_BLOCKED` — the OLD blocker is CLOSED and a NEW,
reproduced, candidate-caused blocker stands in its place.**

| Old blocker (bound to `c34b90fc`) | Status at `314251e0` |
|---|---|
| `cargo build --features local-split-qa` exit 101, **no binary produced** | **CLOSED** — `QA_BUILD_EXIT=0`, binary produced (§1) |
| `split-attach-stall` dies pre-launch: `ASSERTION_FAILURE: targetRole … got "producer"` | **CLOSED** — it now pre-arms, launches and settles a real run (§3.1) |
| No capture exists, so no image-reader verdict is claimable | **still true**, for a different reason (§4) |

---

## 1. Staging and the QA-feature build

| Check | Result |
|---|---|
| Pre-state identity (before extraction) | all 7 delta files hashed on the host against `git cat-file blob c34b90fc:<path>` — **7/7 match**, including `server.rs df27c535fefdb55b` (the predecessor's own A/B anchor) |
| Delta | `git archive` of exactly the 7 paths `c34b90fc..314251e0` (7 files, +90/−24); `tar.exe -xzf`, `TAR_EXIT=0` |
| Post-extract identity | all 7 files re-hashed against `314251e0` — **`STAGED_OK` 7/7, `STAGED_BAD_COUNT=0`** |
| mtime trap | **929 `.rs`/`.toml` files touched** (421 under `src-tauri\src`) **plus every extracted file**; stale `src-tauri\target\debug\ferryx.exe` **and** `ferryx.pdb` **deleted** before the build |
| Observed recompilation | log shows a real `Compiling ferryx v2026.928.7 (…\source-21dea3c0\src-tauri)` then `Finished dev profile [unoptimized + debuginfo] target(s) in 3m 49s` — never `Fresh` |
| Ghostty pin | junction `src-tauri\vendor\ghostty -> C:\Users\sook\task2-ghostty-6a508fd5` @ `6a508fd5e34c7e222c052a6d00bb3891ff3feace` |

**Exact command, raw exit, produced binary** (`logs/01-qa-build.log`):

```
cd C:\Users\sook\ferryx-pane-completion\source-21dea3c0
cargo build --manifest-path src-tauri/Cargo.toml --features local-split-qa
QA_BUILD_EXIT=0
BIN_PATH=C:\Users\sook\ferryx-pane-completion\source-21dea3c0\src-tauri\target\debug\ferryx.exe
BIN_BYTES=106842112
BIN_SHA=bbbcdf143bd540578d14d0e9e81e08fbd6fb83c1d3ad5f70434f365ee211999d
```

### 1.1 Revision proof — and a correction to the predecessor's honesty note

The dispatch records, and the predecessor repeated, that "the suggested `strings`-for-a-new-symbol proof
does NOT work on this host (MSVC keeps names in a `.pdb` and none exists)". **That is true of the default
build and false of the QA build** — and that distinction is what makes a positive proof available:

| Marker class | Default binary (`fcab1abe…`, predecessor's control) | QA binary (`bbbcdf14…`, this pass) |
|---|---|---|
| `FERRYX_QA_BARRIER_DIR`, `FERRYX_QA_OPERATION_ID`, `FERRYX_QA_RUN_ID`, `FERRYX_QA_FIXTURE_SETUP_UNSETTLED`, `FERRYX_QA_RETRY_REFUSED`, `FERRYX_QA_STALE_BINDING_UNSERVICED`, `FERRYX_QA_ARM_REJECTED` | **absent** | **present 7/7** |
| `fixture-setup`, `split-create`, `attach-handshake`, `cancel-ack`, `held-rpc`, `marker-output`, `split-concurrent-batch` | **absent** | **present 7/7** |
| `qa_producers`, `qa_liveness` | absent | **present** |
| `qa_split_producers`, `start_gui_boot_channel`, `collect_gui_fixture_sessions`, `split-happy` | absent | **absent** (Rust *symbol* names / a non-embedded literal — still in the absent `.pdb`; **not used as evidence**) |

Same byte-scan method, same host: **QA binary 14/14 QA literals, default binary 0/14.** So the revision
proof now rests on (a) source-manifest identity 7/7, (b) observed recompilation of the `ferryx` crate,
(c) deleted stale artifacts, **and (d) a genuine 14-vs-0 QA-literal differential between the two
binaries**. The predecessor's caveat is preserved as **correct for the default binary**, where the literals
are absent by construction and the scan is a valid *negative* control rather than a revision proof.

The other predecessor note stands and is respected: **binary sha256 is not a revision proof on this host**
(two identical builds hashed differently). No hash is used as a revision argument anywhere above.

---

## 2. Scenario table — exact command, raw exit, asserted line

Exact argv (the brief's shape, verbatim paths):

```
node scripts/qa/pane-liveness.mjs --scenario <name> \
  --binary C:\Users\sook\ferryx-pane-completion\source-21dea3c0\src-tauri\target\debug\ferryx.exe \
  --evidence-dir C:\Users\sook\ferryx-pane-completion\task9-314251e0\evidence\<name> \
  --isolation-root C:\Users\sook\ferryx-pane-completion\task9-314251e0\runtime\<name>
```

| Scenario | Raw exit | Selected count | Asserted line (verbatim) | Reached a launch? | Image-reader verdict | Verdict |
|---|---|---|---|---|---|---|
| `split-happy` | **7** | n/a (scenario, not a test filter) | `"verdict": "BLOCKED"`, `"code": "BARRIER_ACK_TIMEOUT"`, `"message": "BARRIER_ACK_TIMEOUT: product did not settle barrier fixture-setup receipt[0] within 9000ms"`, `"cleanupGate": {"ok": true}` | **YES** — `launch.binary` action recorded with the isolated env; the product ran and produced a real isolated profile | **none — no capture produced** | **NOT_RUN_BLOCKED** |
| `split-attach-stall` | **7** | n/a | same `BLOCKED` / `BARRIER_ACK_TIMEOUT` line; `barriers[0] = {name: "attach-handshake", armedAt: 2026-10-04T15:16:01.631Z, heldAt: null, receiptCount: 0}` | **YES** — `barriers.prearmed: ["attach-handshake"]` then `launch.binary`; the arm file was really written **and accepted** (real `armedAt`) | **none — no capture produced** | **NOT_RUN_BLOCKED** |
| `split-cancel` | **7** | n/a | same `BLOCKED` / `BARRIER_ACK_TIMEOUT` line | **YES** | **none — no capture produced** | **NOT_RUN_BLOCKED** |

Exit code 7 is the runner's own documented mapping (`BARRIER_ACK_TIMEOUT → EXIT.barrierUnsupported = 7`),
captured from `node`'s real exit code via a `.bat` wrapper that writes `%ERRORLEVEL%` **after** the command
(`logs/bat-*.exit` = `7`). The predecessor recorded `split-cancel` as raw exit 1 and correctly labelled it
a **kill artifact**; that artifact is reproduced and now explained in §5.

### 2.1 The same gate has two faces — both recorded, neither hidden

Across the 13 scenario runs captured in `win-pass2/runs/`, the same `fixture-setup` gate settles in exactly
two ways, and the tally is **6 × `FAIL`/`ASSERTION_FAILURE` and 7 × `BLOCKED`/`BARRIER_ACK_TIMEOUT`**:

Under the earlier drivers (which copied the isolation root on a 200 ms interval) the gate surfaced
as an assertion failure instead of a timeout, with **verdict `FAIL`**:

```
"verdict": "FAIL",
"error": {"code": "ASSERTION_FAILURE",
          "message": "ASSERTION_FAILURE: scenario <name>: fixture-setup requires at least one source session, got 0"}
```

and `barriers: []` / `barriers[0].heldAt: null`. Both faces are the **same** root cause (§3): the receipt
either arrives inside the runner's 9 s budget carrying **zero** sessions (`FAIL`), or it misses the budget
because the product spends ~5 s retrying before settling (`BLOCKED`). Which face appears is
load-dependent; both are nonzero and neither is a pass.

**Measured raw exits:** the `.bat` wrapper captured `%ERRORLEVEL%` **after** the command for the three
`BLOCKED` runs — `logs/bat-{split-happy,split-attach-stall,split-cancel}.exit` each contain **`7`**, the
runner's own `BARRIER_ACK_TIMEOUT → EXIT.barrierUnsupported` mapping. The `FAIL`-face runs' exit is the
runner's `scenarioFailure = 1` **by mapping only** — I did not capture it from a process, so it is stated
as a mapping and not as a measurement.

### 2.2 Negative controls

| Control | Result |
|---|---|
| linux, exact scenario argv, **QA** binary | exit **4** / `NATIVE_AUTOMATION_UNSUPPORTED: native desktop automation is not implemented for platform linux` / `verdict: BLOCKED` — the runner refuses before any launch; the matrix is macOS/Windows-only by construction |
| Windows, **default** binary (`c34b90fc`, predecessor) | exit 7 / `BLOCKED` / `BARRIER_ACK_TIMEOUT … within 9000ms` / `cleanupGate.ok: true` — no false PASS reachable |
| Marker differential | default **0/14** QA literals, QA **14/14** (§1.1) |

---

## 3. The NEW blocker — reproduced, with both producers named

### 3.1 `split-attach-stall` really reaches a launch now

The `799a582d` adapter repair works: the runner records
`{"action":"barriers.prearmed","barriers":["attach-handshake"],…}` **before** `launch.binary`, and the
product accepts the arm (the runner's own `armedAt` is stamped). The pre-launch
`ASSERTION_FAILURE: targetRole must be 'predecessor' or 'successor' … got "producer"` is **gone**.

### 3.2 Decisive manual probe (independent of the runner)

I launched the QA binary by hand with exactly the env the runner builds — `FERRYX_DATA_DIR`,
`FERRYX_RUNTIME_DIR`, `FERRYX_QA_BARRIER_DIR`, `FERRYX_QA_RUN_ID`, `FERRYX_QA_OPERATION_ID`, all under a
fresh empty root — and read the product's own receipt:

```json
{"fixtureKind":"gui-session-inventory","operationId":"qa-op-manualprobe","producer":"ipc-qa-barrier",
 "producerPid":16472,"runId":"qa-run-manualprobe","sessionId":"","sessions":[],"settledAtMs":1791126373337}
```

`sessions: []`, **byte-identical at T+10 s, T+20 s and T+40 s**; no receipt existed at T+5 s. The isolated
data dir confirms it independently: `data\remote\paired_descriptors.json` = `{}`, no session store, and
`daemon.log` holds only the daemon's own boot plus its startup-workspace registration.

**So the product's own collector really observes zero sessions in a fresh isolated profile and says so
honestly rather than fabricating a fixture.** It is behaving correctly.

### 3.3 The two producers — one exists, one does not

1. **Collector (product, GUI lane) — present and honest.** `collect_gui_fixture_sessions`
   (`src-tauri/src/ipc/qa_barrier.rs:896-941`) enumerates `daemon_client.list_sessions()` in the isolated
   profile and classifies each session from the daemon's own reply; its own comment is explicit that the
   lifecycle kinds needing a record it cannot read (`created`, `adopted`) **are never claimed**. It also
   cannot claim `source` for a session that does not exist. Its boot path is wired
   (`start_gui_boot_channel` called from `lib.rs:1359`, `emit_gui_fixture_setup` retries
   `FIXTURE_BOOT_ATTEMPTS=20 × FIXTURE_BOOT_RETRY_MS=250` = ~5 s, then settles whatever it has).
2. **Constructor (runner/adapters) — MISSING.** `buildIsolatedEnv`
   (`scripts/lib/qa-scenarios/diagnostic-classifier.mjs:27-52`) creates `data`, `runtime` and `home`, then
   launches the binary. **Nothing anywhere creates a session in that profile.** Measured:

   ```
   grep -rn "spawnTerminal|spawn_terminals|createSession" scripts/lib/qa-scenarios/*.mjs scripts/qa/pane-liveness.mjs
   → 0 hits
   ```

   `SCENARIO_PLANS` has no fixture-construction step, and the only runner code that writes into the
   barrier dir is `BarrierHub`'s own arm/command files.

The plan states the intended contract at `.omo/plans/local-pane-liveness-completion-replan.md:178`:
*"fixture setup is scenario-specific: basic split needs **source+created target** … Never require all four
fixtures for every split smoke, and never count unsupported fixture as passed."* The **target** (`created`)
half is produced by the scenario's own split action, which is fine — but the **source** session the same
sentence requires has **no producer on either side**.

**Owning lane: the QA adapter/scripts lane** (`scripts/qa/pane-liveness*`, `scripts/lib/qa-scenarios/**`),
which the plan assigns exactly this work at line 178. The product side is not at fault.

> **Post-report update (committed as `5245e6ba`):** the adapter half landed — `fixtureKindsForScenario` is
> now the single derivation point and the isolated launch env carries `FERRYX_QA_FIXTURE_KINDS`
> (comma-separated, lower case). **The blocker is not closed:** the product-side consumer that provisions
> one session per requested kind is still being authored (lane `st_01a10780`), so at `5245e6ba`
> `grep -rn "FERRYX_QA_FIXTURE_KINDS" src-tauri/src/` is **0 hits** and the `got 0` gate would still fire.
> Re-run these three scenarios only after that lands.

**Classification: candidate-caused, reproduced, and it blocks all three Windows scenarios — and the same
gate blocks every scenario on the parked mac side too.** It is a *stronger* block than the predecessor's:
it survives both a green QA binary and the repaired role map.

**Not repaired here** — the verifier does not edit product or adapter code.

### 3.4 Why this was invisible before

The predecessor never obtained a QA binary, so no scenario ever reached `fixture-setup` with an installed
channel; its negative control used the **default** binary, which cannot install the channel at all and so
timed out at `BARRIER_ACK_TIMEOUT` (exit 7 / `BLOCKED`) rather than settling an empty inventory. The two
failures look identical from outside and are different: the old one was "no channel", the new one is
"channel works, inventory empty, settlement marginal".

---

## 4. Screenshot lane: nothing to read, and the instrument is nevertheless ready

No scenario reached a native action: every run died at `fixture-setup`, before `driver.split`,
`driver.typeMarker` or `driver.capture`. **No `screenshot.png` exists for any scenario, so no
`multimodal-looker` verdict is claimed** — inventing one is the exact fabrication the brief forbids. The
`capture-ready.json` → `marker-recognition.json` handshake remains **untested on this host**.

**The recognition instrument is built and self-tested, so the parked lane is not a blank slate.** It is a
Windows OCR recognizer (`tooling/win-recognizer.ps1`, live copy at
`C:\Users\sook\ferryx-pane-completion\win-recognizer.ps1`) using `Windows.Media.Ocr` (engine language
`ko`, the only recognizer installed). Self-test on a synthetic image of the marker:

```
SELFTEST_RAW=[FERRYX SPLIT READY]
SELFTEST_NORM=[FERRYX SPLIT READY]
SELFTEST_MATCH=True
```

**Independent verification of that self-test image, and the instrument's sharpest limitation.** The
self-test PNG is kept at `captures/recognizer-selftest.png` and I read it myself through the image channel
(the `multimodal-looker` child was unavailable — subagent depth limit — so this is my own read, disclosed
as such rather than delegated):

| Field | Independent read |
|---|---|
| Text visible | `FERRYX_SPLIT_READY` — white monospace text, left-aligned, roughly mid-height, on a black field |
| Exact string present | **TRUE** |
| Separator form | **literal underscores** `_` in both positions, not spaces |
| Readability | fully readable, not cropped |

So the **image really contains the underscore form**, while the OCR engine's **raw** output collapses it to
spaces (`FERRYX SPLIT READY`). **Consequence, stated plainly: this OCR instrument cannot distinguish
`FERRYX_SPLIT_READY` from `FERRYX SPLIT READY`** — it matches only under a declared normalization rule
(`[_ whitespace]+ → one space`) and writes the *declared literal* into the artifact's `text` field rather
than its raw reading. That is acceptable for the runner's own `text === MARKER_TEXT` check only because the
provenance file carries the raw OCR text alongside it; it would **not** be sufficient as the independent
recognition lane on its own, and a successor should prefer a reader that reproduces the separator exactly
(a human-equivalent viewer, or a segmentation step that keeps punctuation). This limitation is recorded so
the parked mac lane does not inherit the instrument believing it is stronger than it is.

It waits for `capture-ready.json`, OCRs the owned-window capture, applies a **declared** normalization
(uppercase; runs of `[_ whitespace]` → one space; trim — OCR renders the `_` separator as a space),
locates the marker token bounding box, **requires the marker's bbox center to fall inside the
runner-claimed target pane rectangle (exit 9 otherwise)**, and writes `marker-recognition.json` plus an
`ocr-provenance.json` carrying the raw OCR text, the normalization rule, the bbox, both rectangles and the
screenshot hash. It is **not used for a verdict in this pass** because no capture was produced; it is
recorded as a ready instrument with a self-test, not as evidence of recognition.

**Honest limitation to carry:** the pane *rectangle* it reports is the runner-claimed one; only the
marker's position within it is independently measured. Detecting the pane boundary itself would need a
second, independent segmentation step, which does not exist.

---

## 5. Residual classification (Windows)

| Item | Class | Evidence |
|---|---|---|
| QA-feature build exit 101 / no binary at `c34b90fc` | **candidate-caused, CLOSED** | `QA_BUILD_EXIT=0`; 106 842 112-byte binary; `Compiling ferryx` observed |
| `split-attach-stall` pre-launch `targetRole … "producer"` | **candidate-caused, CLOSED** | `barriers.prearmed: ["attach-handshake"]` + real `armedAt`, then a launch |
| All three scenarios die at `fixture-setup` (`got 0` / `BARRIER_ACK_TIMEOUT`) | **candidate-caused, NEW, reproduced → adapter half FIXED at `5245e6ba`; product half still outstanding** | §3 — `sessions: []` in the product's own receipt, stable over 40 s; **0** fixture constructors in `scripts/`; `5245e6ba` adds the declaration but `FERRYX_QA_FIXTURE_KINDS` still has **0** consumers in `src-tauri/src/` |
| `split-cancel` runner hang (descendant holds the inherited stdio pipe) | **environment/robustness observation, reproduced** | §5.1 |
| Binary sha256 differs between identical builds | **environment** (predecessor's finding, respected) | no hash used as a revision argument |
| `#![cfg(unix)]` integration targets selecting 0 on Windows | **structurally-not-run** | 0 selected is not a pass |
| Windows suspension | **structurally-not-run** | `terminal/suspension/windows.rs` returns typed `UnsupportedPlatform`; never reached |
| `strings`-for-a-symbol proof "impossible on maho-win" | **corrected for the QA binary** | QA 14/14 vs default 0/14 QA literals, same method (§1.1) |
| Marker recognition | **instrument ready, no verdict claimable** | §4 |

### 5.1 The `split-cancel` hang, reproduced and explained

The predecessor recorded `split-cancel`'s node process never exiting after printing its complete result,
and attributed it to "a descendant of the spawned app holding the inherited stdio pipe". That reproduces
here for **all three** scenarios, and the mechanism is now pinned: the runner's own cleanup calls
`taskkill /T /PID <pid>` **without `/F`** (`common-harness.mjs` `reapProcess`), which does not terminate
the app's daemon grandchild; the grandchild keeps the inherited stdout/stderr handles open, so node's
event loop never drains and `process.exitCode` is set but never reached. The runner prints its full
`result.json` **before** the hang, so the log looks complete. Reaping only this session's staged-tree
`ferryx.exe` PIDs lets node exit with its real code (**7**) — which is how the raw exits in §2 were
captured. This is a genuine runner/cleanup robustness observation owned by the **QA adapter/scripts lane**;
`split-happy` reproduces it here, contrary to the predecessor's note that it happened not to.

---

## 6. Cleanup receipts

| Resource | Identity check | Teardown | Receipt |
|---|---|---|---|
| **`ferryx.exe` PID 23672** — the inherited leak from the predecessor's hung Windows full `--lib` (started 2026-10-04 23:55:58) | `ExecutablePath` = `…\ferryx-pane-completion\source-21dea3c0\src-tauri\target\debug\ferryx.exe` → **under the task staging root**, not the user's own install → **identity matched** | `taskkill /T /F /PID 23672` (children 10996 terminated; 11368 reported un-terminable, then gone) | reaped 2026-10-05 00:00; the follow-up sweep recorded `LEAK_GONE PID=23672` and `TASK_OWNED_ALIVE_COUNT=0` |
| Other task-owned processes under the staging root | enumerated by `ExecutablePath`/`CommandLine` filter | none remained | `TASK_OWNED_ALIVE_COUNT=0` |
| **Foreign processes — left alone, listed** (12: 9 × `cargo.exe` from other sessions' `cargo test --lib … remote::managed_chat…`, 2 × `node.exe` `omo-ai` CLI, 1 × `node.exe` `ferryx-landing-verify` astro dev server, plus a `minio` `cmd.exe`) | **not under this task's staging root** | **untouched** | `FOREIGN_KEPT` lines for PIDs 18524, 27036, 16196, 20164, 13520, 16168, 10060, 4716, 19780, 11332, 16020, 22124, 2348 |
| This pass's own runner trees (v3/v4/v5/v6/bat drivers: `cmd`, `node`, `ferryx`) | matched by `pane-liveness.mjs` / `source-21dea3c0` command line | `taskkill /T /F` per own PID | `KILLV4_DONE` with an empty "remaining task-owned" sweep; each run's `logs/bat-reaper-*.log` and `logs/reaper-*.log` |
| Manual-probe process PID 16472 + 6 children | own probe | `taskkill /T /F` | `MANUAL_PROBE_DONE`, all six children reported terminated |
| Isolation roots per scenario | owned | removed by the runner's own `cleanupGate` **and** re-removed by the driver before each run | `cleanupGate.ok: true`, `directoriesRemoved: true` on every run; `ISO_EXISTS_BEFORE_LAUNCH=False` recorded per run |
| `task9-314251e0\preserved\<scenario>` snapshots | owned, kept deliberately (the runner's own cleanup deletes the barrier dir before it can be read) | retained as evidence | files listed in `logs/scenario-*.log` |
| user desktop / production app / production daemon | **never addressed** | — | — |

---

## 7. What the parked mac half must still cover

1. **Fix the fixture-construction gap first (§3)** — it blocks *every* scenario on *both* platforms, so no
   mac scenario can pass until a real `source` session exists before `fixture-setup` line 0.
2. **Re-run the compile gate on mac** — the mac-only `cfg` arms of `native_terminal/surface_host.rs` /
   `ipc/native_terminal.rs` are not compiled by the linux gate.
3. `diagnostic-classifier` (native + the deferred headless smoke, still NOT_RUN per the F1 audit H-23),
   `retained-handover`, `handover-abort`, `stale-binding`, `suspension-ownership` (still blocked by the
   `externally-stopped` fixture-kind gap the lead scoped at `common-harness.mjs:180-190` +
   `qa_barrier.rs:754/756`).
4. The four split scenarios on a QA-feature binary — three of them were also killed pre-launch by the
   adapter role map before `799a582d`.
5. The independent image-reader lane on real captures; the Windows recognition instrument is ready and
   self-tested (§4), mac still needs one.
6. Task 8's two host-state unknowns (whether the verifier's `pkill` landed; whether the 13 GB `target`
   reclaim finished) — neither is a product finding.
