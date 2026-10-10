# Task 9 — PASS 3 consolidated report (`1df40271`)

Verifier: sole remote verifier (pass 3; successor of the pass-2 run). Date: 2026-10-05 (+0900).
Candidate: `C=/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`,
branch `work/local-pane-liveness-completion-foundation`, HEAD **`1df40271`**
(`1df402717ad952e5ae38a22d3eca5c859967b2a7`), tree **clean**, unchanged by this pass (no product or
adapter code edited; `.omo/` is untracked).
Chain: `1df40271` (product half) → `5245e6ba` (adapter half + gated-test fix) → `314251e0` → `799a582d` →
`c34b90fc` → `dd9e6813` → `70eefafe` → … → base `d82b35e4`.

## HEADLINE

**The two never-compiled commits compile under `cargo check` — 954 new lines, 0 errors.** The QA selectors
now select **61** tests, the pass-2 test fix is confirmed (`qa_split_producers` **15/15**), and the fixture
constructor is real: `fixture_creation_uses_the_real_daemon_paths` passes against a real daemon in
**0.19 s**.

**But `cargo build --features local-split-qa` FAILS with `E0275`** — a recursion-limit overflow in `Sync`
evaluation through `wgpu-30.0.1` types, present on **both** hosts, only with the QA feature, only in
**codegen** (`cargo check` passes). **A/B-proven candidate-caused by `1df40271`'s `qa_barrier.rs` alone.**
No QA binary can be produced, so **Phase 2 is `NOT_RUN_BLOCKED` ×3 for a third, new reason.**

| Phase | Verdict |
|---|---|
| 1. Compile + selectors | **FAIL** — `cargo check` 0, but `cargo build --features local-split-qa` **101 (E0275)**; 4th selector run **FAIL** 22/23; other three selectors PASS (10/10, 13/13, **15/15**) |
| 2. Three Windows scenarios | **NOT_RUN_BLOCKED ×3** — no QA binary exists (E0275). The author's flagged daemon-session-vs-GUI-pane gap **could not be reached**, so it is not yet confirmable as the next blocker |
| 3. Budget question | **answered with a measurement**: creation test **0.19 s** for 3 kinds vs a 3.5 s deadline and a 9 s await; the live `fixtureCreationElapsedMs` is **not obtainable** at this revision (no binary) |

---

## 1. linux (omaki, `indo@100.91.254.71`), rustc/cargo 1.98.0, 12 cores

cwd `/home/indo/ferryx-pane-completion/source-21dea3c0` @ `1df40271`. Logs: `compile-pass3/linux/logs/`.

| # | Exact command | Raw exit | Selected | Asserted line | Verdict |
|---|---|---|---|---|---|
| 1 | `cargo check --manifest-path src-tauri/Cargo.toml --all-targets --features local-split-qa` | **0** | — | `Finished dev profile [unoptimized + debuginfo] target(s) in 1m 11s`, `errors=0` | **PASS** — the 954 new lines compile |
| 2 | `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` (default) | **0** | — | `Finished dev profile …` | **PASS** |
| 3 | `cargo build --manifest-path src-tauri/Cargo.toml` (default) | **0** | — | `Finished dev profile …` | **PASS** — 0/19 QA markers in the produced binary (§1.1) |
| 4 | `--lib --features local-split-qa qa_barrier -- --list` | **0** | **23** | 23 `: test` lines | **PASS** (was 19 at `314251e0` — the fixture-creation tests landed) |
| 5 | `--lib --features local-split-qa qa_barrier -- --nocapture --test-threads=1` | **101** | **23** | `test result: FAILED. 22 passed; 1 failed; … finished in 0.54s` | **FAIL — 1 test** (§3) |
| 6 | `--lib --features local-split-qa qa_producers -- --list` / run | **0** | **10** | `ok. 10 passed; 0 failed` | **PASS** |
| 7 | `--lib --features local-split-qa qa_liveness -- --list` / run | **0** | **13** | `ok. 13 passed; 0 failed` | **PASS** |
| 8 | `--lib --features local-split-qa qa_split_producers -- --list` / run | **0** | **15** | `ok. 15 passed; 0 failed` | **PASS** — **the pass-2 `14/15` is fixed; 15/15 confirmed** |
| 9 | `cargo build --manifest-path src-tauri/Cargo.toml --features local-split-qa` | **101** | — | `error[E0275]: overflow evaluating the requirement 'validation::NumericDimension: Sync'` → `could not compile 'ferryx' (lib) due to 1 previous error; 78 warnings emitted` | **FAIL — BLOCKS PHASE 2** (§2) |
| 10 | `cargo check --features local-split-qa` (**lib+bin only**, the same target set as `cargo build`) | **0** | — | `Finished dev profile …` | **PASS** — isolates the failure to **codegen**, not the target set |
| 11 | `cargo build --features local-split-qa --lib` | **101** | — | same `E0275` | **FAIL** — the rlib alone reproduces it |
| 12 | **A/B:** `qa_barrier.rs` restored to its `5245e6ba` content, everything else unchanged, then `cargo build --features local-split-qa` | **0** | — | `Finished dev profile [unoptimized + debuginfo] target(s) in 52.87s` | **PASS** — the regression is that one file (§2.2) |
| 13 | `--lib --features local-split-qa fixture_creation_uses_the_real_daemon_paths -- --nocapture --test-threads=1` | **0** | 1 | `test result: ok. 1 passed; 0 failed; … finished in 0.19s` | **PASS** — real daemon, real UDS socket, 3 kinds |

**Selected total: 61** (23 + 10 + 13 + 15).

### 1.1 No QA surface leaks into the default build

`compile-pass3/linux/marker-scan-default.txt` — default binary `fb7c4113…`, 929 162 904 B:

| Marker class | Default binary |
|---|---|
| `FERRYX_QA_BARRIER_DIR`, `FERRYX_QA_OPERATION_ID`, `FERRYX_QA_FIXTURE_KINDS`, `FERRYX_QA_FIXTURE_SETUP_UNSETTLED`, `FERRYX_QA_RETRY_REFUSED`, `FERRYX_QA_STALE_BINDING_UNSERVICED`, `FERRYX_QA_FIXTURE_KIND_UNSUPPORTED`, `FERRYX_QA_FIXTURE_CREATE_FAILED`, `FERRYX_QA_FIXTURE_CLAIM_REFUSED` | **ABSENT 9/9** |
| `collect_gui_fixture_sessions`, `start_gui_boot_channel`, `qa_producers`, `qa_liveness`, `marker-output`, `split-create`, `attach-handshake`, `cancel-ack`, `held-rpc`, `fixture-setup` | **ABSENT 10/10** |
| `Attach requires the persisted seven-field pane binding`, `Attach binding incarnation cannot be proven` | **PRESENT 2/2** (candidate-lineage positive control) |

**0 of 19 QA markers** in the shipped configuration, with the positive control present — the new
`FERRYX_QA_FIXTURE_KINDS` surface does not leak.

---

## 2. The blocker: `E0275` — no QA binary can be produced

### 2.1 Verbatim (linux, identical text on Windows)

```
error[E0275]: overflow evaluating the requirement `validation::NumericDimension: Sync`
     |
     = help: consider increasing the recursion limit by adding a `#![recursion_limit = "256"]` attribute to your crate (`ferryx_lib`)
note: required because it appears within the type `NumericType`
    --> …/wgpu-core-30.0.1/src/validation.rs:107:12
note: required because it appears within the type `InterfaceVar`        (:125)
note: required because it appears within the type `validation::Varying`  (:154)
note: required because it appears within the type `PhantomData<validation::Varying>`
note: required because it appears within the type `alloc::raw_vec::RawVec<validation::Varying>`
note: required because it appears within the type `Vec<validation::Varying>`
note: required because it appears within the type `validation::EntryPoint` (:285)
     = note: required for `hashbrown::raw::RawTable<…>` to implement `Sync`
note: required because it appears within the type `Interface` (:300) → `ShaderMetaData` (:317)
note: required because it appears within the type `wgpu_core::pipeline::ShaderModule` (:69)
     = note: required for `Arc<ShaderModule>` to implement `Send`
note: … `RenderPipeline` (:918) → `ExclusivePipeline` (binding_model:749)
     = note: required for `OnceLock<ExclusivePipeline>` to implement `Send`
note: required because it appears within the type `BindGroupLayout` (binding_model:811)
     = note: required for `Arc<BindGroupLayout>` to implement `Sync`
note: … `PipelineLayout` (:1029) → `RenderPipelineState` (pipeline:912)
note: required because it appears within the type `ResourceState<RenderPipelineState>` (resource:95)
     = note: the full name for the type has been written to '…/ferryx_lib.long-type-…txt'

error: could not compile `ferryx` (lib) due to 1 previous error; 78 warnings emitted
```

### 2.2 What is established, and by which measurement

| Question | Answer | Evidence |
|---|---|---|
| Both hosts? | **yes** | linux `05-qa-build.log` and `08-b-build-lib.log`; Windows `01-qa-build.log` — same `E0275`, same chain |
| Feature-specific? | **yes** | default `cargo build` = **0** on linux (gate 3); the QA feature is required |
| Codegen-specific? | **yes** | `cargo check --features local-split-qa` = **0** on **lib+bin** (gate 10) while `cargo build` on the same target set = **101** |
| Candidate-caused? | **yes, A/B** | gate 12: reverting **only** `qa_barrier.rs` to `5245e6ba` (`e0c23509…`) → **0**; the `1df40271` content (`5ff9a265…`) → **101**; hash restored and re-verified after the run |
| Which commit? | **`1df40271` alone** | `git diff --name-only 314251e0 1df40271` is 4 files, but the A/B swaps only `qa_barrier.rs`, so the scripts and `terminal.rs` changes are exonerated |
| Load/timeout artifact? | **no** | the failure is returned in ~1–3 min with `errors=1`; it is a hard compile error, not a deadline |

**The mechanism, stated as an observation and not as a proven cause:** the chain is the compiler proving a
`Send`/`Sync` auto-trait **through the WGPU object graph** (`NumericDimension` → … → `ResourceState`) for a
concrete type that only materialises when the lib is **codegen'd**. The `1df40271` diff is what forces that
proof: its new non-test code holds Tauri managed state
(`app.state::<NativeTerminalSurfaceHostState>()`, `qa_barrier.rs:1443`, `:1498`) inside an async future
spawned with `tauri::async_runtime::spawn` (`:1386`). `NativeTerminalSurfaceHostState` transitively owns
`Arc<GpuWorker>` → the WGPU pipeline objects, so the spawn now requires `Send + 'static` for a type whose
`Sync` proof overflows the default recursion limit. **I did not prove that last step** (doing so would mean
editing product code, which this pass does not do); it is recorded as the leading hypothesis, with the two
non-test sites named, for the owning lane to confirm. The compiler's own suggestion
(`#![recursion_limit = "256"]`) is a **symptom-level** lever, and whether it is the right fix is the
author's call — raising the limit without understanding the cycle risks masking a real `!Send` type.

### 2.3 Repetition discipline

The identical failure was observed on each host more than once, but **never as a blind retry** — each
repetition had a discriminating purpose (check-vs-build target set; `--lib` alone; the A/B swap). No
further repetition was performed after gate 12 settled the attribution.

---

## 3. The one gated test that FAILS

Command (gate 5), verbatim:

```
cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier -- --nocapture --test-threads=1

test ipc::qa_barrier::tests::fixture_creation_uses_the_real_daemon_paths ... ok
test ipc::qa_barrier::tests::fixture_kind_claims_follow_the_daemons_own_reply ...
thread 'ipc::qa_barrier::tests::fixture_kind_claims_follow_the_daemons_own_reply' (3445297) panicked at src/ipc/qa_barrier.rs:2669:9:
assertion `left == right` failed
  left: "idle"
 right: "source"
FAILED
test result: FAILED. 22 passed; 1 failed; 0 ignored; 0 measured; 2730 filtered out; finished in 0.54s
error: test failed, to rerun pass `--lib`
```

**Root cause (source-derived).** Line 2669 is:

```rust
assert_eq!(classify_fixture_session(&idle_facts(), None, None).kind, "source");
```

`classify_fixture_session` (`qa_barrier.rs:1181-1227`) returns `observation` **unchanged** when
`intended` is `None` (`let Some(intended) = intended else { return observation };`), and
`observation_classification` yields `"idle"` for `idle_facts()` (no veto applies). The assertion expects
`"source"`. **The test's expectation contradicts the implementation's contract as written** — either the
`None` case should collapse to `source`, or the assertion is wrong. Both are test/contract-level, not
production-path: nothing in the failing assertion exercises a session the product actually created.

**Owning lane: the product-half author (`1df40271`).** Not repaired by me.

---

## 4. The budget question — measured, per the dispatch

| Quantity | Value | Source |
|---|---|---|
| Runner's `fixture-setup` await | **9 000 ms** (`BUDGETS.stagePrepareCreateStatusMs`) | `common-harness.mjs:245` |
| Product's creation deadline | **3 500 ms** (`FIXTURE_CREATE_BUDGET_MS`) | `qa_barrier.rs:61`, applied at `:1479` |
| The old 20 × 250 ms inventory retry loop (~5 s of the budget) | **REMOVED — confirmed by grep** (`FIXTURE_BOOT_ATTEMPTS` / `FIXTURE_BOOT_RETRY_MS` → absent) | `qa_barrier.rs` |
| **Measured creation cost** (3 kinds: `source`, `idle`, `created`, against a real in-process daemon over a real UDS socket) | **0.19 s total** for the whole test, including daemon setup | gate 13, `09-creation-time.log` |
| **Live `fixtureCreationElapsedMs`** from a real receipt | **NOT MEASURABLE at this revision** | no QA binary can be produced (§2) |

**Reading of those numbers, stated as numbers rather than a verdict:** the creation step's own unit
measurement (0.19 s for 3 kinds) sits an order of magnitude inside the 3.5 s deadline and two orders
inside the 9 s await, and the ~5 s retry loop that accounted for most of the pass-2 lateness is gone. That
makes the pass-2 T+10 s observation **very unlikely to recur**, but it is **not a live measurement** — the
receipt field `fixtureCreationElapsedMs` can only be read from a running GUI, and none can be built until
§2 is fixed. **The budget question therefore remains open pending a buildable revision**, with the unit
measurement as the best available evidence.

---

## 5. Windows (maho-win, `sook@100.126.171.58`)

Full detail: `win-pass3/WINDOWS-PASS3-VERDICT.md`; logs `win-pass3/logs/`.

| # | Exact command | Raw exit | Asserted line | Verdict |
|---|---|---|---|---|
| W1 | `cargo build --features local-split-qa` (staged `1df40271`, 8/8 `STAGED_OK`, 929 `.rs`/`.toml` touched, stale `ferryx.exe`/`.pdb` deleted) | **101** | `error[E0275]: overflow evaluating the requirement 'validation::NumericDimension: Sync'` → `could not compile 'ferryx' (lib) due to 1 previous error; 90 warnings emitted`; `BIN_MISSING` | **FAIL — same blocker as linux** |
| W2 | `cargo check --all-targets --features local-split-qa` (the cross-host A/B for the check/build split) | **0** | `Finished dev profile [unoptimized + debuginfo] target(s) in 2m 54s` | **PASS** — reproduces linux exactly: **check 0, build 101** |
| W3 | `cargo build --manifest-path src-tauri/Cargo.toml` (default), **first attempt** | **101** | `failed to build archive at …\libferryx_lib.rlib: 디스크 공간이 부족합니다. (os error 112)` at **2.01 GB free** | **CONTAMINATED — not a verdict** (§5.2) |
| W4 | `cargo build --manifest-path src-tauri/Cargo.toml` (default), after reclaiming this task's own `target/` | **0** | `Finished dev profile [unoptimized + debuginfo] target(s) in 5m 21s`; binary 101 938 176 B, `b601d187…`; **19/19 QA markers ABSENT**, 2/2 lineage literals PRESENT | **PASS** |
| W5 | the three scenarios | — | — | **NOT_RUN_BLOCKED ×3** — no QA binary (W1) |

### 5.1 The three scenarios

| Scenario | Raw exit | Asserted line | Image-reader verdict | Verdict |
|---|---|---|---|---|
| `split-happy` | **NOT_RUN** | — | **none — no capture exists** | **NOT_RUN_BLOCKED** |
| `split-attach-stall` | **NOT_RUN** | — | **none — no capture exists** | **NOT_RUN_BLOCKED** |
| `split-cancel` | **NOT_RUN** | — | **none — no capture exists** | **NOT_RUN_BLOCKED** |

The exact argv was prepared and its driver staged (`win-pass3/tooling/`), but no scenario was launched
because the binary it must run does not exist. **No recognition verdict is claimed**, and the author's
flagged structural gap (fixtures are daemon sessions, not GUI panes) **was not reached** — so it is **not
yet confirmable** as the next blocker; it becomes testable the moment a QA binary builds.

### 5.2 Windows disk — a real environment finding, and what I did about it

The first default build failed with `failed to build archive at …libferryx_lib.rlib: 디스크 공간이
부족합니다. (os error 112)` at **2.01 GB free** — i.e. **not** `E0275`, so that result is contaminated and
is not cited as a default-build verdict. The space is consumed by **foreign sessions' caches** (the host
carries `source-base`, `task8-172baa87`, `task8-21dea3c0`, `task8-5464da0d`, `task9-c34b90fc`,
`task9-314251e0`, `task9-win-70eefafe`). I removed **only this task's own**
`source-21dea3c0\src-tauri\target` (49.31 GB, holding my own `ferryx_lib.lib` 3.46 GB and four `.pdb`
files of 0.4–0.9 GB each), taking free space **2.01 → 43.35 GB**, and left every foreign tree intact.
**The re-run then returned 0 (W4)**, so the first failure was environmental. Worth flagging for whoever
schedules the next Windows run: **check free space before launching a build**, and expect a cold rebuild
after any reclaim (the reclaim is why W4 took 5m 21s).

### 5.3 The default configuration is clean on this host too

W4's default binary (`b601d187…`, 101 938 176 B) carries **19 of 19** QA markers **ABSENT** — including
the new `FERRYX_QA_FIXTURE_KINDS`, `FERRYX_QA_FIXTURE_KIND_UNSUPPORTED`, `FERRYX_QA_FIXTURE_CREATE_FAILED`
and `FERRYX_QA_FIXTURE_CLAIM_REFUSED` — with **2 of 2** candidate-lineage literals **PRESENT** as a
positive control. Artifact: `win-pass3/marker-scan-default.txt` (producer: `tooling/win-scan3.ps1`).

---

## 6. Residual classification

| Item | Class | Evidence |
|---|---|---|
| `E0275` on `cargo build --features local-split-qa`, both hosts | **candidate-caused, A/B-proven, `1df40271`** | gate 9/11 vs gate 12 (revert `qa_barrier.rs` → 0); gates 1/10 show `check` passes |
| `qa_barrier::tests::fixture_kind_claims_follow_the_daemons_own_reply` | **candidate-caused, test/contract-level** | §3, `left: "idle"` / `right: "source"` at `:2669` |
| `qa_split_producers` 14/15 at `314251e0` | **FIXED by `5245e6ba`, verified 15/15** | gate 8 |
| Fixture constructor absent (pass-2 blocker) | **FIXED — verified working** | gate 13: 3 kinds created through the real daemon paths, 0.19 s, no failures |
| QA surface leaking into the default build | **not observed** | gate 3 + 0/19 markers in the default binary |
| Old 20 × 250 ms retry loop consuming ~5 s | **REMOVED, confirmed** | grep absent |
| Windows `os error 112` (disk full) on the first default build | **environment** | 2.01 GB free; foreign caches; reclaimed my own `target/` only |
| Binary sha256 as a revision proof on maho-win | **environment** (predecessor's finding, respected) | no hash used as a revision argument |
| `#![cfg(unix)]` targets selecting 0 on Windows | **structurally-not-run** | 0 selected is not a pass |
| Windows suspension | **structurally-not-run** | typed `UnsupportedPlatform`; never reached |

---

## 7. Cleanup receipts

| Resource | Teardown | Receipt |
|---|---|---|
| linux background gate sessions / monitors | all completed on their own sentinels | `GATE1_QA_CHECK_EXIT=0`, `LINUX_PHASE_A_DONE`, `AB_SCRIPT_DONE`, `watcher completed (exit code 0)` |
| linux scratch (`linux-ab.sh`, `ab-qabarrier-5245e6ba.tar.gz`, `/tmp/t9p2-linux-*`) | removed | `LINUX_TEARDOWN_DONE`; 0 own processes |
| linux staged tree `source-21dea3c0` | **retained** as the evidence source; `qa_barrier.rs` restored to the candidate hash `5ff9a265…` after the A/B | `qa_barrier.rs=5ff9a265c87b1f293b73e565edca67b66cb18eba436b245511464b3d42195709` |
| **windows `source-21dea3c0\src-tauri\target` (49.31 GB)** | removed deliberately to restore disk (2.01 → 43.35 GB) | `FREE_BEFORE_GB=2.01` → `FREE_AFTER_GB=43.35` |
| windows scenario drivers/sidecars | **never launched** (no binary), so none spawned | drivers staged in `win-pass3/tooling/` only |
| windows task-owned `ferryx.exe` / `node.exe` | sweep at close | `TASK_OWNED_ALIVE_COUNT=0` |
| **foreign trees/processes on both hosts** (`source-base`, `task8-*`, `task9-c34b90fc`, `task9-314251e0`, `task9-win-70eefafe`, the 9+ foreign `cargo.exe`) | **untouched** | listed, not modified |
| user desktop / production app / production daemon | **never addressed** | — |

---

## 8. What the parked mac half must still cover

1. **Fix `E0275` first (§2)** — it blocks the QA binary on **every** platform, so no mac scenario can run
   either. This is now the single gate in front of the whole native matrix.
2. **Fix or re-derive the `qa_barrier` test contract (§3)** — one assertion, `:2669`.
3. **Then re-measure the budget live (§4)** — read `fixtureCreationElapsedMs` from a real receipt.
4. **Then the three Windows scenarios**, including the author's flagged daemon-session-vs-GUI-pane gap,
   which is still untested.
5. **Re-run the compile gate on mac** — the mac-only `cfg` arms of `native_terminal/surface_host.rs` /
   `ipc/native_terminal.rs` are not compiled by the linux or Windows gates.
6. **`externally-stopped` and `adopted` are not constructible from this lane** (author-reported, and
   confirmed in source: `FixtureCreation::Unsupported(hook)` for both, with the exact hook each needs — a
   real external stop plus local-describe ownership attribution for the former, a real retained handover
   for the latter). Therefore **`suspension-ownership`, `retained-handover` and `handover-abort` will fail
   at `fixture-setup` truthfully** until those hooks exist; their failure is honest, not a defect to paper
   over. The remaining mac scenarios (`diagnostic-classifier` incl. the deferred headless smoke, still
   NOT_RUN per F1 H-23, `split-concurrent`, `stale-binding`) are gated only by items 1–4.
7. **An image reader that preserves the `_` separator** — the OCR instrument built in pass 2 cannot
   distinguish `FERRYX_SPLIT_READY` from `FERRYX SPLIT READY` and must not be used on mac either.
