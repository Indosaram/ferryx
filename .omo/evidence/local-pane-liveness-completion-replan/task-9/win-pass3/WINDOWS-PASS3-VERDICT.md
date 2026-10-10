# Task 9 — Windows half PASS 3: the three native scenarios at `1df40271`

Verifier: sole remote verifier (pass 3; successor of the pass-2 run). Date: 2026-10-05 (+0900).
Host: **maho-win** (`sook@100.126.171.58`, `DESKTOP-1LAPJMP`, Windows 10.0.26200.9457 x64, rustc/cargo
1.97.0, Node v24.19.0, Bun 1.4.0). Staging root `C:\Users\sook\ferryx-pane-completion\source-21dea3c0`.
Boot `2026-10-03 10:33:16`; load 41–65 % during the runs. Logs: `logs/`.

**VERDICT: all three Windows scenarios are `NOT_RUN_BLOCKED` — for a THIRD, new reason: the QA binary
cannot be built at this revision.** `cargo build --features local-split-qa` fails with `E0275` on this host
too, so there is no binary for the scenarios to run.

| Blocker, by pass | Status at `1df40271` |
|---|---|
| pass 1: `cargo build --features local-split-qa` exit 101, 6 lib errors (`c34b90fc`) | **CLOSED** |
| pass 2: `fixture-setup … got 0` — no session constructor | **CLOSED — verified working** (`fixture_creation_uses_the_real_daemon_paths` passes; 3 kinds in 0.19 s on linux) |
| pass 3: `E0275` on `cargo build --features local-split-qa` | **OPEN — blocks the binary on both hosts** |

---

## 1. Staging and provenance

| Check | Result |
|---|---|
| Pre-state identity (before extraction) | 8 files hashed on the host against the `c34b90fc` blobs — **4 `PRE_ALREADY`**, **4 `PRE_DIFFERS`** (the expected set: `qa_barrier.rs`, `terminal.rs`, `common-harness.mjs`, `diagnostic-classifier.mjs`) |
| Delta | `git archive` of exactly the 8 paths `c34b90fc..1df40271` (183 316 B); `tar.exe -xzf`, `TAR_EXIT=0` |
| Post-extract identity | **`STAGED_OK` 8/8, `STAGED_BAD_COUNT=0`** against the `1df40271` blob hashes |
| mtime trap | **929 `.rs`/`.toml`** files touched **plus every extracted file**; stale `ferryx.exe` **and** `ferryx.pdb` deleted before every build |
| Observed recompilation | `Compiling ferryx v2026.928.7 (…\source-21dea3c0\src-tauri)` → `Finished dev profile … in 2m 54s` on the check run; never `Fresh` |
| Ghostty pin | junction `src-tauri\vendor\ghostty -> C:\Users\sook\task2-ghostty-6a508fd5` @ `6a508fd5e34c7e222c052a6d00bb3891ff3feace` |

---

## 2. Gate table

| # | Exact command (cwd `source-21dea3c0` @ `1df40271`) | Raw exit | Asserted line | Verdict |
|---|---|---|---|---|
| W1 | `cargo build --manifest-path src-tauri/Cargo.toml --features local-split-qa` | **101** | `error[E0275]: overflow evaluating the requirement 'validation::NumericDimension: Sync'` → `error: could not compile 'ferryx' (lib) due to 1 previous error; 90 warnings emitted`; `BIN_MISSING` | **FAIL — the Phase 2 blocker** |
| W2 | `cargo check --manifest-path src-tauri/Cargo.toml --all-targets --features local-split-qa` | **0** | `Finished dev profile [unoptimized + debuginfo] target(s) in 2m 54s` | **PASS** — the cross-host A/B for the check/build split |
| W3 | `cargo build --manifest-path src-tauri/Cargo.toml` (default), **first attempt** | **101** | `failed to build archive at …\libferryx_lib.rlib: 디스크 공간이 부족합니다. (os error 112)` at **2.01 GB free** | **CONTAMINATED — not a verdict** (§4) |
| W4 | `cargo build --manifest-path src-tauri/Cargo.toml` (default), after reclaiming this task's own `target/` | **0** | `Finished dev profile [unoptimized + debuginfo] target(s) in 5m 21s`; binary 101 938 176 B, `b601d187…`; **19/19 QA markers ABSENT**, 2/2 lineage literals PRESENT | **PASS** |
| W5 | the three scenarios | — | — | **NOT_RUN_BLOCKED ×3** (W1) |

**W1 vs W2 is the decisive cross-host result:** the same revision, the same feature set, the same host —
`cargo check` **0**, `cargo build` **101**. That reproduces the linux split exactly and localises the
failure to **codegen**, not to the target set, the feature set, or the host.

### 2.1 Scenario table

| Scenario | Raw exit | Asserted line | Reached a launch? | Image-reader verdict | Verdict |
|---|---|---|---|---|---|
| `split-happy` | **NOT_RUN** | — | **no** — the binary does not exist | **none — no capture exists** | **NOT_RUN_BLOCKED** |
| `split-attach-stall` | **NOT_RUN** | — | **no** | **none — no capture exists** | **NOT_RUN_BLOCKED** |
| `split-cancel` | **NOT_RUN** | — | **no** | **none — no capture exists** | **NOT_RUN_BLOCKED** |

The exact argv and a working driver were staged (`tooling/run-scenario3.bat`, `tooling/win-scenario7.ps1`,
plus the `win-reaper3.ps1` / `win-snap3.ps1` sidecars that solved the pass-2 node-exit hang), but **no
scenario was launched**, because launching would only have re-measured a missing binary. **No recognition
verdict is claimed.**

### 2.2 The author's flagged structural gap — NOT REACHED

The dispatch asked me to watch for fixtures being daemon sessions rather than GUI panes. **I cannot
report on it**: every scenario dies before `fixture-setup` completes, and now before the binary even
exists. It becomes testable the moment a QA binary builds; it is **not yet confirmable as the next
blocker**.

---

## 3. The `E0275` blocker (identical to linux; full chain in `../REPORT-PASS3.md` §2)

```
error[E0275]: overflow evaluating the requirement `validation::NumericDimension: Sync`
     = help: consider increasing the recursion limit by adding a `#![recursion_limit = "256"]` attribute to your crate (`ferryx_lib`)
…
error: could not compile `ferryx` (lib) due to 1 previous error; 90 warnings emitted
```

Attribution (A/B on linux, one file swapped, hash-verified): reverting **only** `qa_barrier.rs` to its
`5245e6ba` content makes `cargo build --features local-split-qa` return **0**; the `1df40271` content
returns **101**. Both hosts fail with the same message, only with the QA feature, and only in codegen.

---

## 4. Windows disk — a real environment finding

| Reading | Value |
|---|---|
| Free space when W3 ran | **2.01 GB** |
| Cause | **foreign sessions' caches**: the host carries `source-base`, `task8-172baa87`, `task8-21dea3c0`, `task8-5464da0d`, `task9-c34b90fc`, `task9-314251e0`, `task9-win-70eefafe` |
| This task's own `source-21dea3c0\src-tauri\target` | **49.31 GB** (my own `ferryx_lib.lib` 3.46 GB, `ferryx_lib-*.pdb` 924 MB + 834 MB, `ferryx.pdb` 685 MB, `ferryx_cli.pdb` 492 MB, `ferryx_relay.pdb` 386 MB) |
| Action taken | removed **only this task's own** `target/` (2.01 → **43.35 GB** free) |
| Foreign trees | **untouched**, listed above |

So the W3 `os error 112` is **environment, not product** — the same command returns **0** once space is
available (W4), exactly as it does on linux. This is the second pass in which this host's disk pressure has
influenced a result (pass 2 recorded 24.6 GB free; it is now consumed), and it is worth flagging for
whoever schedules the next Windows run: **check free space before launching a build**, and expect a cold
rebuild after any reclaim.

### 4.1 The default configuration is clean on this host too

W4's default binary (`b601d187…`, 101 938 176 B) carries **19 of 19** QA markers **ABSENT** — including
the new `FERRYX_QA_FIXTURE_KINDS`, `FERRYX_QA_FIXTURE_KIND_UNSUPPORTED`, `FERRYX_QA_FIXTURE_CREATE_FAILED`
and `FERRYX_QA_FIXTURE_CLAIM_REFUSED` — while **2 of 2** candidate-lineage literals are **PRESENT** as a
positive control. So the new fixture-construction surface does not leak into the shipped configuration on
either host. Artifact: `logs/marker-scan-default.txt` (producer: `tooling/win-scan3.ps1`).

---

## 5. Cleanup receipts

| Resource | Teardown | Receipt |
|---|---|---|
| this task's own `source-21dea3c0\src-tauri\target` (49.31 GB) | removed deliberately, to restore disk for the W4 rebuild | `FREE_BEFORE_GB=2.01` → `FREE_AFTER_GB=43.35`; foreign trees listed and kept |
| all scenario driver/sidecar processes (v3/v7 drivers, reaper, snapshot, recognizer) | never launched this pass (no binary), so none spawned | driver scripts staged but not executed |
| task-owned `ferryx.exe` / `node.exe` under the staging root | sweep at close | `TASK_OWNED_ALIVE_COUNT=0` |
| foreign processes | **untouched** | pass-2's 13 foreign processes remain other sessions' |
| staging trees (`source-21dea3c0`, `task9-1df40271`) | **retained** as the evidence source (owned) | — |
| user desktop / production app / production daemon | **never addressed** | — |
