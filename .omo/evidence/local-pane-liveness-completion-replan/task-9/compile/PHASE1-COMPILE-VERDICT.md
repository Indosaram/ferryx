# Task 9 — Phase 1 compile verification of `c34b90fc` (the QA producer/consumer set)

Verifier: sole remote verifier `st_01a10728`. Date: 2026-10-04 (local +0900).
Candidate: `C=/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`,
branch `work/local-pane-liveness-completion-foundation`, HEAD **`c34b90fc`**
(`c34b90fcdb5ec6ff6ca34c80cada038b921d626a`), tree **clean** (0 dirty paths), commit
`feat(qa): author the native-scenario producer and consumer surface behind the local-split-qa gate`,
14 files, +6753/−93.

**VERDICT: FAIL — the QA producer set does not compile on any host.** Both required feature
combinations were measured on linux (omaki) and the QA combination was measured on Windows
(maho-win). The default build is green on both; the QA build is red on both. **Phase 2 (the three
Windows scenarios) is therefore NOT_RUN_BLOCKED**: no `--features local-split-qa` binary exists to
run them against.

---

## 1. Feature gating (why `--all-targets` alone was not enough)

`src-tauri/Cargo.toml`:

```
[features]
default = ["native-terminal"]
local-split-qa = []                      # NOT default
native-terminal = ["dep:wgpu", ...]
```

The new modules are gated on **both** features — `#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]`
(30 occurrences) — while 40 further blocks use the single-feature form
`#[cfg(feature = "local-split-qa")]`. So the default `cargo check --all-targets` **cannot** compile
the new surface, which is exactly why the producer set had never been compiled before this pass.

## 2. Gate table — linux (omaki, `indo@100.91.254.71`)

Host at start: load 1.84–2.48, 394 GiB free on `/home` (btrfs), 12 cores, rustc 1.98.0.

| # | Exact command (cwd `source-21dea3c0`) | Raw exit | Selected | Result | Verdict |
|---|---|---|---|---|---|
| 1 | `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` | **0** | — | `Finished dev profile` in 2m58s, **errors=0** | **PASS** |
| 2 | `cargo check --manifest-path src-tauri/Cargo.toml --all-targets --features local-split-qa` | **101** | — | **18 error blocks** (`could not compile ferryx (lib) due to 7 previous errors`; `(lib test) due to 9 previous errors`) | **FAIL** |
| 3 | `--lib --features local-split-qa -- --list` (`qa_barrier`, `qa_producers`, `qa_liveness`, `qa_split_producers`) | — | — | **NOT RUN** | **NOT_RUN_BLOCKED** (short-circuited on gate 2) |
| 4 | `--lib local_split_reliability_` | — | — | **NOT RUN in the QA combination**; the default-feature run is in §4 | **NOT_RUN_BLOCKED** |
| 5 | `--lib pane_liveness_` | — | — | same | **NOT_RUN_BLOCKED** |
| 6 | `--test daemon_handover_transfer_contract` | — | — | same | **NOT_RUN_BLOCKED** |
| 7 | full `--lib` | — | — | same | **NOT_RUN_BLOCKED** |

Short-circuit rule applied: gate 2 failed, so gates 3–7 were not attempted in the QA combination.
The identical compilation failure was not repeated on this host.

Logs: `compile/linux/logs/01-default-check.log`, `02-qa-check.log`.

### 2.1 Gate 1 verbatim tail

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2m 58s
[13:54:36] GATE default-check EXIT=0
   errors=0
```

This also discharges the second half of the brief's requirement: **no QA surface leaks into a
normal build** — the default target set compiles with the QA modules absent (§5 corroborates at the
binary level on Windows).

### 2.2 Gate 2 — every error verbatim, with owning lane

18 error blocks = 16 unique sites (the lib and lib-test compilations re-report the production ones).
`(lib)` contributes 7; `(lib test)` adds 2 more (the `#[cfg(test)]` sites).

| # | Code | Site | Message (verbatim) | Owning lane |
|---|---|---|---|---|
| 1 | `E0599` | `src/daemon/qa_producers.rs:372:18` | ``no method named `session_service` found for reference `&DaemonServer` in the current scope`` — help: *field, not a method*; nearest is `session_is_live` (`session_service.rs:2454`) | **daemon lane** (`daemon/qa_producers.rs`, `daemon/server.rs`) |
| 2 | `E0599` | `src/daemon/qa_producers.rs:458:31` | ``no method named `session_service` found for reference `&std::sync::Arc<DaemonServer>` in the current scope`` | **daemon lane** |
| 3 | `E0308` | `src/daemon/server.rs:2923:83` | ``mismatched types`` — `expected &DaemonServer, found Arc<DaemonServer>` at the `describe_incarnation(self, …)` call | **daemon lane** |
| 4 | `E0308` | `src/ipc/terminal.rs:2487:45` | ``mismatched types`` — `operation_state(&operation)` expects `&SplitOperationResult<String>`, found `&SplitOperationResult<u64>` | **daemon lane** (`ipc/terminal.rs`) |
| 5 | `E0308` | `src/ipc/terminal.rs:3205:33` | same mismatch, `operation_state(&operation)` | **daemon lane** |
| 6 | `E0631` | `src/ipc/terminal.rs:3206:37` | ``type mismatch in function arguments`` — `status.as_ref().map(operation_state)`: expected `fn(&SplitOperationResult<u64>) -> _`, found `fn(&SplitOperationResult<String>) -> _` | **daemon lane** |
| 7 | `E0308` | `src/ipc/qa_barrier.rs:1222:34` | ``mismatched types`` — `Arc::clone(channel)` expects `&Arc<_,_>`, found `&QaBarrierChannel` | **stale-trigger lane** (`ipc/qa_barrier.rs`) |
| 8 | `E0599` | `src/daemon/qa_producers.rs:682:26` | ``no method named `path` found for struct `std::path::PathBuf` `` — help: `as_path` | **daemon lane** (test-only) |
| 9 | `E0599` | `src/daemon/qa_producers.rs:708:26` | same | **daemon lane** (test-only) |
| 10 | `E0599` | `src/daemon/qa_producers.rs:738:26` | same | **daemon lane** (test-only) |
| 11 | `E0618` | `src/daemon/qa_producers.rs:771:23` | ``expected function, found `qa_barrier::QaBarrierChannel` `` — the test-local `fn channel(dir: &Path)` at `:619` shadows `pub fn channel()` at `:67`, so `let channel = channel(&dir);` at `:771` calls the *value* | **daemon lane** (test-only) |

Verbatin abort lines:

```
error: could not compile `ferryx` (lib) due to 7 previous errors; 25 warnings emitted
error: could not compile `ferryx` (lib test) due to 9 previous errors; 49 warnings emitted
```

**This is the brief's expected first-pass outcome** ("authored code has never been compiled, so
compile failures are expected findings, not evidence of a broken environment") — but it is a
*finding to route*, not something the verifier repairs. Per the dispatch, product code was not
touched: the tree is still clean at `c34b90fc`.

## 3. Gate table — Windows (maho-win, `sook@100.126.171.58`)

Host: `DESKTOP-1LAPJMP`, Windows 10.0.26200.9457 x64, rustc/cargo 1.97.0, Bun 1.4.0, Node v24.19.0,
uptime 1d12h, load 31–43 %, 29.03 GiB free at start.

| # | Exact command (cwd `source-21dea3c0`) | Raw exit | Result | Verdict |
|---|---|---|---|---|
| W1 | `bun install --cwd ui --frozen-lockfile` | **0** | ok | **PASS** |
| W2 | `bun run --cwd ui build` | **0** | ok | **PASS** |
| W3 | `cargo build --manifest-path src-tauri/Cargo.toml` (default features, stale binary deleted first) | **0** | 185.5 s; `ferryx.exe` sha256 `1b7d5a0c51bb3ee294e1ee1d36a5f3d029e6f87fd2a684878f28adfed1b44fd5` | **PASS** |
| W4 | `cargo build --manifest-path src-tauri/Cargo.toml --features local-split-qa` | **101** | 73.1 s; **6 lib errors**, no binary produced | **FAIL** |

Windows error set (raw log `cargo-build-qa.log`): `E0599` ×2 at `daemon/qa_producers.rs:372:18` and
`:458:31`; `E0308` ×3 at `daemon/server.rs:2923:83`, `ipc/terminal.rs:2487:45`, `ipc/terminal.rs:3205:33`;
`E0631` ×1 at `ipc/terminal.rs:3206:37`; plus the `Arc::clone(channel)` mismatch at
`ipc/qa_barrier.rs:1222:34`.

```
error: could not compile `ferryx` (lib) due to 6 previous errors; 17 warnings emitted
```

The Windows set is the linux production set minus nothing (the 4 `qa_producers.rs` `#[cfg(test)]`
sites are absent only because `cargo build` does not compile test targets). **No
platform-specific error class was found; the failure is identical in shape on both hosts.**

## 4. Previously-green gates that do NOT need the QA feature — regression check

These run under default features, so the QA compile failure does not block them; they were run to
check that the QA-surface commits did not disturb the previously-green surface. All on **linux**
(omaki) at `c34b90fc`, host load 1.92 → 7.82 → 2.93 across the run.

| Exact command | Raw exit | Selected | Result line | Task 8 reference (`70eefafe`) | Verdict |
|---|---|---|---|---|---|
| `--lib local_split_reliability_ -- --nocapture --test-threads=1` | **0** | **15** | `ok. 15 passed; 0 failed; … finished in 0.01s` | 15/0 | **PASS — no regression** |
| `--lib pane_liveness_ -- --nocapture --test-threads=1` | **0** | **54** | `ok. 54 passed; 0 failed; … finished in 1.39s` | 54/0 | **PASS — no regression** |
| `--test daemon_handover_transfer_contract -- --nocapture --test-threads=1` | **0** | **5** | `ok. 5 passed; 0 failed; … finished in 5.81s` | 5/0 in 6.21 s | **PASS — no regression** (base-equivalent runtime) |
| `--lib -- --test-threads=1` (full library suite) | **101** | 2683 | `FAILED. 2646 passed; 31 failed; 6 ignored; … finished in 457.89s` | 2647 / 30 / 6 | **FAIL — one test joined** (below) |

### 4.1 The full-`--lib` delta, set-compared by test name

| | Count |
|---|---|
| failing at `70eefafe` | 30 |
| failing at `c34b90fc` | 31 |
| **left the failing set** | **0** |
| **joined the failing set** | **1** |

**Joined (1):** `remote::server::machine_input_cancellation_tests::input_is_cancelled_when_grant_is_revoked`.

Classification: **flaky, previously recorded as such, not attributable to this commit chain.**

- Task 8's own record names this exact test as *"linux `input_is_cancelled_when_grant_is_revoked` flaky
  on both sides"*, and it is absent from the `39e722ce` failure set as well (checked by name), so it has
  been intermittent across the whole Task 8 window.
- `c34b90fc`'s 14-file diff contains **no file under `src/remote/`** (and neither does `dd9e6813`), so the
  code path this test exercises is not touched by the QA-surface commits.
- A 3-repetition isolated A/B was run to characterise it (results recorded in
  `compile/linux/logs/13-flaky-rep-*.log` and summarised in §4.2).

All 30 remaining failures are **exactly the set** Task 8 classified as pre-existing at `70eefafe`
(`daemon::handover_wire::tests::truncated_control_is_rejected`,
`daemon::server::a03_owner_cli_fixture::a03_private_owner_cli_surface`,
2 × `daemon::server::remote_ssh_tests::*`, 3 × `ipc::worktree::deletion_repair_tests::*`,
2 × `native_terminal::renderer::font_manager::tests::*`,
`remote::relay_server::tests::a_symlinked_artifact_is_not_served`,
2 × `remote::server::machine_input_cancellation_tests::*`,
`remote::tests::security::sockets::a10_pending_machine_input_is_dropped_on_disconnect_and_revoke`,
`rollout_tests::a24_rollback_waits_for_drain_and_preserves_unrelated_owner`,
14 × `ssh::bridge::tests::*`,
`ssh::helper_setup::tests::posix_upload_script_never_kills_live_daemon_and_defers_upgrade`) —
**zero new candidate-caused failures**.

### 4.2 Flaky-test characterisation

See **§2.2 of `PHASE1B-REGRESSION.md`** (same directory) for the 3 isolated repetitions and their
outcomes (1 of 3 failed; panic in the test's own support harness at
`tests/support/machine_input_cancellation.rs:31`).

### 4.3 What remains blocked

`--lib --features local-split-qa qa_barrier` — the fourth of the plan's verbatim Rust gate commands —
**cannot run at `c34b90fc`**: it is inside the QA feature gate that fails to compile. Task 8 measured
it green (`ok. 11 passed; 0 failed`) at `70eefafe`, so this is a **regression of a previously-green
gate**, not a never-measured one. The A/B in **§1 of `PHASE1B-REGRESSION.md`** pins the regression to the
17 modified + 2 new files (exit 0 at `70eefafe` → exit 101 at `c34b90fc`).

### 4.4 Windows equivalent

The Windows default-feature regression gates (pane_liveness 50/0, local_split_reliability 14/0,
split_journal 7/0, runner vitest 29/29) are tabulated in **§3 of `PHASE1B-REGRESSION.md`** and in
`win/WINDOWS-SCENARIO-VERDICT.md`.

## 5. Negative control at the binary level (Windows, default build)

`strings`-equivalent substring scan over the **default-feature** `ferryx.exe`
(sha256 `1b7d5a0c…`):

| Marker | Present in default binary? |
|---|---|
| `qa_producers` | **False** |
| `qa_liveness` | **False** |
| `adopt_runner_bind` | **False** |
| `attach-handshake` | **False** |
| `cancel-ack` | **False** |
| `held-rpc` | **False** |
| `split-create` | **False** |
| `marker-output` | **False** |
| `FERRYX_QA_BARRIER_DIR` | **False** |

Every QA marker is absent from the normal build — the "no QA surface in a shipped/default build"
property holds at the artifact level, on Windows, at this revision.

## 6. Staging and provenance (so no stale-binary trap can be suspected)

1. **Trees.** linux `source-21dea3c0` was confirmed to be at the **`70eefafe`** pre-state before
   staging (`handover_socket.rs` = `4dc252c1bafe10e1…`, the `70eefafe` hash). Windows
   `source-21dea3c0` was confirmed at **`39e722ce`** (`handover_socket.rs` `a5d7e0b4e3ca77de`,
   `server.rs` `163abf5260f201cc`, `ipc/terminal.rs` `3946a80bb6012ba6`, `qa_barrier.rs`
   `5391a9f4170598cc` — all four match the `39e722ce` column of the revision-hash matrix). A
   full-manifest check on Windows then reported **`checked=5086 missing=0 diff=0`** against the
   `c34b90fc` tracked-file manifest, so the tree is byte-identical to the commit.
2. **Deltas.** `git archive` of the changed paths: 19 files `70eefafe→c34b90fc` (linux, 359 715 B) and
   21 files `39e722ce→c34b90fc` (Windows, 373 157 B). No deletions in either range.
3. **mtime trap defeated.** After extraction every extracted file **and** every `.rs`/`.toml` under
   `src-tauri/{src,tests,examples}` plus the crate-root manifests were `touch`ed to the extraction
   instant (`TOUCHED_AT 2026-10-04T22:53:21+09:00` on Windows), and the stale `ferryx.exe`/`.pdb`
   were **deleted** before each of the two Windows builds. Both `cargo build` runs therefore
   recompiled rather than reusing pre-fix objects.
4. **Built-binary revision proof.** The default build's artifact was verified in §5; the QA build
   produced no artifact to verify (`QA_BIN_MISSING`), which is itself the proof that the build
   failed rather than silently reusing the previous binary.
5. **Source hashes on the hosts equal the commit.** linux post-stage:
   `qa_producers.rs aa4300a6…`, `terminal/qa_liveness.rs a96ae6cb…`, `qa_barrier.rs f0c63df2…`,
   `ipc/terminal.rs 51ede9ce…`, `surface_host.rs 24911238…`, `pane-liveness.mjs e2dbceed…` — all six
   identical to the `c34b90fc` manifest rows.

## 7. Residual classification

| Item | Class |
|---|---|
| QA-feature compile failure (16 sites, both hosts) | **candidate-caused** — introduced by `c34b90fc`/`dd9e6813`; the code has never compiled |
| Default-feature build red | n/a — **default build is green on linux and Windows** |
| `--all-targets` under-reporting | **pre-existing property of cargo**, already recorded in Task 8 pass-3: one `--all-targets` run stops scheduling after the first failing target, so the per-target sweep is authoritative. Not re-litigated here. |
| Latent single-feature gate (`#[cfg(feature = "local-split-qa")]` alone in `surface_host.rs` while `qa_barrier` needs both) | **pre-existing**, recorded by the lead before the freeze; not exercised by any gate command |
| Windows suspension | honest-unsupported (`terminal/suspension/windows.rs` → typed `UnsupportedPlatform`); not reached, since no scenario ran |

## 8. What was NOT done, and why

- **Phase 2 (three Windows scenarios) — NOT_RUN_BLOCKED.** No `--features local-split-qa` binary can
  be produced at `c34b90fc`. Per the dispatch, Phase 2 is conditional on Phase 1 compiling.
- **QA selectors (`qa_barrier`, `qa_producers`, `qa_liveness`, `qa_split_producers`) — NOT_RUN.**
  Their modules are inside the QA gate and the gate does not compile; a `--list` would have failed
  at the same 7 errors. They were enumerated from source instead, so the owning lanes can see
  exactly what is waiting behind the gate: **48 `#[test]` functions** — 10 in `daemon/qa_producers.rs`,
  13 in `terminal/qa_liveness.rs`, 10 in `ipc/qa_barrier.rs`, and **15 inside
  `ipc::terminal::qa_split_producers::tests`** (`ipc/terminal.rs:3244`, itself under the same
  both-feature gate at `:1811`). Note `qa_split_producers` is a **module** name, not a test-name
  prefix: libtest filters on the full test path, so that selector would match all 15 of its tests.
  (An earlier draft of this file wrongly said it selected 0; corrected here.)

  > **Correction to a claim that circulated: "`qa_split_producers` selects 0 cases repo-wide" is WRONG,
  > and it was my own transient error.** It came from a bad first search (`fn qa_split_producers`,
  > which matches nothing because the name is a *module*). The true position: `mod qa_split_producers`
  > is declared at `ipc/terminal.rs:1811-1812` under
  > `#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]`, and it contains
  > `#[cfg(test)] mod tests` at `:3244` with **15 `#[test]` functions**. The selector therefore selects
  > **15** tests — it just cannot select them today because the gate does not compile. The `--list`
  > that would have proven this is precisely the gate that short-circuited on the 7 lib errors, so
  > **the count is source-derived, not execution-derived**, and the repair lane should confirm it with
  > `--list` once the tree compiles.
- **mac half — out of scope by instruction** (host parked; the task explicitly forbids waiting on it).
  For the record, `maho-mac` answered ssh during this pass at load 21.01 and was not used.

## 9. Cleanup receipts (Phase 1)

| Resource | Action | Receipt |
|---|---|---|
| linux `task9-c34b90fc/` (owned staging dir) | created; logs preserved | kept as evidence source |
| linux `source-21dea3c0/` (owned staging tree) | delta applied + touched | no foreign tree touched |
| Windows `task9-c34b90fc/` (owned) | created; logs preserved | kept |
| Windows `source-21dea3c0/` (owned staging tree) | delta applied + touched; `ferryx.exe`/`.pdb` deleted then rebuilt (default) | `target/debug/ferryx.exe` present, default build |
| local `/tmp/plv-qa/` | working copies + manifest | kept for the report |
| production daemons / user desktop | **untouched** | no production path was addressed |

## 10. CORRECTION (pass-2 successor, 2026-10-05) — two claims above are wrong or overbroad

Appended rather than rewritten, so the correction is visible next to what it corrects.

1. **The QA-selector count is 57, not 48.** The enumeration in §8 is **source-derived and low**: at
   `314251e0` the four selectors select **57** — `qa_barrier` **19**, `qa_producers` 10,
   `qa_liveness` 13, `qa_split_producers` **15**. My `qa_barrier` figure (10, from `grep -c "#\[test\]"\)
   in that one file) missed that libtest matches the **full test path**, so the selector also catches tests
   declared elsewhere whose path contains the name. **Use 57.**
2. **The `strings` claim in §6 item 4 is overbroad.** It is true for the **default** binary (0/14 QA
   literals — absent by construction, a valid *negative* control) but **false for the QA binary**, which
   carries **14/14** — i.e. the QA build **does** admit a real `strings`-based revision proof. The
   limitation applies only to the default build, which is the only build this dispatch could produce.
   The other provenance legs (source manifest `checked=5086 missing=0 diff=0`, observed recompilation,
   deleted stale artifacts) and the "binary sha256 is not a revision proof on maho-win" note both stand.

Pass-2 also confirmed by execution what §1 of `PHASE1B-REGRESSION.md` proved by A/B:
`cargo check --all-targets --features local-split-qa` is **exit 0** at `314251e0` (was 101 with 16 sites).
