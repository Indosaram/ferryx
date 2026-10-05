# H-18 — the conflicting-fingerprint rejection: authored tests, their placement, and their evidence

**Plan clauses satisfied:**
- `/Volumes/T9-Mac/project/ferryx/.omo/plans/local-pane-liveness-completion-replan.md` line 131
  (Todo 2, QA failure): "old capability, **conflicting request fingerprint**, reused backend with
  differing incarnation and mismatched binding are rejected without closing adopted session."
- same plan, line 141 (Todo 3, QA failure): "…duplicate/**fingerprint conflict**, remote held RPC and
  cancel-before-reply cases; NEW `pane_liveness_pump_generation` filter."
- same plan, line 83 (scenario table, `split-concurrent`): "…drive local typing and16 bounded split
  requests including duplicate/fingerprint conflict pairs."

**Hole closed:** F1 audit `H-18` — `E/F1-PLAN-TO-ARTIFACT-AUDIT.md`, HOLES / Holes B, row H-18 ("No
named test or evidence artifact exercises the conflicting-fingerprint rejection"; also audit row
`J2f`, whose gap reads "No named test for the 'conflicting request fingerprint' rejection is cited in
any evidence artifact").

**Author:** host-independent F1-hole lane (`st_01a108d2`), 2026-10-05. **Nothing was compiled, run
or tested.** The authored test was verified by reading it back on disk; its execution is the next
stage's job.

---

## 1. The production path (read, not assumed)

**Daemon half — the rejection itself.** `src-tauri/src/daemon/session_service.rs`, inside
`pub(crate) async fn create_split` (`:464`):

```rust
:482   let fingerprint = serde_json::to_string(&(workspace, &worktree, &cwd, cols, rows, &shell))
:486   let expected_fingerprint = fingerprint.clone();
:487   crate::ipc::run_blocking(move || {
:488       let journal = super::split_journal::SplitJournal::open(&request_directory) …
:490       if let Some(previous) = journal.load(&request_key) … {
:491           if previous.fingerprint != expected_fingerprint {
:492               return Err(crate::ipc::IpcError::spawn_request_conflict(
:493                   "Split request identity was reused with different parameters"));
```

The typed error is `IpcErrorCode::SpawnRequestConflict` (`src-tauri/src/ipc/error.rs:75`), built by
`IpcError::spawn_request_conflict` (`:270-272`), serialized as `SPAWN_REQUEST_CONFLICT`
(`:222`). The journal that carries the fingerprint is `daemon/split_journal.rs`
(`SplitJournalEntry.fingerprint`, `:15`; `SplitJournal::load` `:105`, `upsert` `:82`), whose own
module doc requires async callers to use `run_blocking` (`:37-39`) — honored here.

**Harness half — the reporting of it.** The runner's `split-concurrent` adapter drives the wave
(`scripts/lib/qa-scenarios/split-scenarios.mjs`, concurrent branch):
`barrierHub.command('split-concurrent-batch', { count: 16, testConflicts: true })`. The product
services it in `run_concurrent_batch_watcher` (`src-tauri/src/ipc/terminal.rs:3035+`), and the batch
settlement is emitted by `batch_payload` (`:2747`) through
`channel.append_receipt(SPLIT_CONCURRENT_BATCH, …)` (`:2956` on admission failure, `:3008` on
settlement), carrying `conflictRejected` (`:2801`) and `conflictRejectionCodes` (`:2805`).

**What already existed, and why it did not close the hole:**

| Existing artifact | What it proves | Why it is not the hole's test |
|---|---|---|
| `src-tauri/src/ipc/terminal.rs:3744` test `concurrent_batch_payload_claims_only_evidence_backed_idempotency` (assertions at `:3776, :3809, :3810`) | the *payload formatter* maps a `rejected` outcome with code `SpawnRequestConflict` into `conflictRejected: 1` / `conflictRejectionCodes: ["SpawnRequestConflict"]` | its `BatchOutcome`s are hand-built; no daemon pre-check runs, and the "rejection" is a literal in the test |
| `src-tauri/src/daemon/client.rs:7012` (test fixture replying `{"type":"error", …, "code":"SPAWN_REQUEST_CONFLICT"}`) | the client surfaces that wire code as an error | it is a *fake daemon reply*: it never exercises `create_split`'s fingerprint comparison |
| `src-tauri/src/ipc/pane_liveness_contract.rs:291` | the code's string round-trip | string mapping only |
| `daemon/session_service.rs` — **no test at all** for the `:491` comparison | — | this is the missing half |

## 2. Decision: author **both** halves (each covers a different half)

- **Daemon rejection** (does the pre-check really reject a reused identity with different parameters?):
  a Rust test in `daemon/session_service.rs`, named for the behaviour.
- **Harness reporting** (does a real `split-concurrent` run surface that rejection as the batch's
  `conflictRejected`/`conflictRejectionCodes`, and is the scenario's PASS bound to it?): an assertion
  in the `split-concurrent` adapter plus a named runner unit test.

The two are not redundant: the Rust test proves the rejection happens; the harness half proves a real
run reports it and cannot pass without it.

### 2a. Harness half — APPLIED

**`scripts/lib/qa-scenarios/split-scenarios.mjs`** (2 changes):

1. New exported assertion, next to `assertSinglePty`:

```js
export function assertConflictWaveReported(batch, label = 'split-concurrent-batch') {
  const rejected = batch?.conflictRejected;
  if (typeof rejected !== 'number' || rejected < 1) { … HarnessError('ASSERTION_FAILURE', …) }
  const codes = Array.isArray(batch?.conflictRejectionCodes) ? batch.conflictRejectionCodes : [];
  if (!codes.includes('SpawnRequestConflict')) { … HarnessError('ASSERTION_FAILURE', …) }
  return true;
}
```

2. `runSplitConcurrentScenario`, new step **7b** after the held-RPC release: awaits
   `barrierHub.awaitReceipt('split-concurrent-batch', 0, BUDGETS.stagePrepareCreateStatusMs)`,
   calls `assertConflictWaveReported(batch)`, and records the settlement as an evidence action
   (`split-concurrent-batch-settlement` with `conflictRejected`, `conflictRejectionCodes`,
   `conflictDistinctSessionIds`).

   The wait is **deliberately not taken from the scenario's attempt budget**: the 16-request batch is
   not part of the measured split attempt, so a slow-but-correct batch must not spend the 15 s
   attempt ceiling. It is still bounded (`BUDGETS.stagePrepareCreateStatusMs` = 9 000 ms), and a
   batch that never settles fails the scenario.

**`scripts/qa/pane-liveness.test.mjs`** (2 changes): `assertConflictWaveReported` added to the
existing static import of `split-scenarios.mjs`, and a new named test:

```
split-concurrent binds PASS to the conflict wave really rejecting the reused fingerprint
```

It asserts the healthy settlement passes and that four mutations each fail: no rejection
(`conflictRejected: 0`), a rejection under another code (`InternalError`), a batch that never settled
(`unsettledReason`), and a missing settlement. The mutations are the reason this test can fail — it is
not a source-text pin.

### 2b. Daemon half — AUTHORED, NOT APPLIED (this lane's writable scope excludes `src-tauri/**`)

The brief for this lane forbids touching `src-tauri/**` except to read, so the Rust test is delivered
here as ready-to-apply code rather than written into the file. It is complete and compiles by
inspection; every fact it depends on is cited so a reviewer can check it in seconds.

**Placement:** `src-tauri/src/daemon/session_service.rs`, inside the existing `#[cfg(test)] mod tests`
(`:2712`, `use super::*;` already imports `DaemonServer`).

**Selector:** `local_split_reliability_conflicting_fingerprint_reuse_is_rejected_without_creating_a_session`
— it matches the plan's own Todo 3 filter `cargo test --manifest-path src-tauri/Cargo.toml --lib
local_split_reliability_ -- --nocapture --test-threads=1`, which is where the plan puts the
"duplicate/fingerprint conflict" cases (REPLAN line 141). It is a behaviour test of the daemon
admission path, which is Todo 3's subject; Todo 2's QA-failure clause (line 131) is satisfied by the
same behaviour.

```rust
    #[tokio::test]
    async fn local_split_reliability_conflicting_fingerprint_reuse_is_rejected_without_creating_a_session() {
        let server = DaemonServer::new();
        let service = server.session_service();
        let request_id = "conflict-fingerprint-request";
        let epoch = 7u64;

        // Seed the record the daemon's own pre-check reads: `split_root()/requests`,
        // keyed by the same request identity and epoch, carrying a DIFFERENT
        // fingerprint than the parameters below will compute.
        let requests_dir = service.split_root().join("requests");
        let key = DaemonSessionService::split_key(request_id, epoch);
        let seeded = super::split_journal::SplitJournalEntry {
            request_id: key.clone(),
            fingerprint: "fingerprint-from-a-different-parameter-set".to_string(),
            expires_at_unix_ms: 0,
            session_id: None,
            cancel_requested: false,
            tombstone: false,
            outcome: None,
        };
        super::split_journal::SplitJournal::open(&requests_dir)
            .expect("open the requests journal")
            .upsert(&seeded)
            .expect("seed the conflicting record");

        let envelope = crate::daemon::protocol::LocalSplitEnvelope {
            origin_epoch: epoch,
            expires_at_unix_ms: 0,
            remaining_ms: 5_000,
        };
        let error = service
            .create_split(
                request_id, "ws-conflict-fingerprint", None, None, 80, 24, None,
                envelope, epoch,
            )
            .await
            .expect_err("a reused request identity with different parameters must be rejected");

        // The typed code, never a string match (AGENTS.md: parse structured errors).
        assert_eq!(error.code, crate::ipc::error::IpcErrorCode::SpawnRequestConflict);
    }
```

**Facts the snippet depends on (all read, all cited):**

| Fact | Source |
|---|---|
| `create_split(&self, request_id, workspace, worktree, cwd, cols, rows, shell, envelope, current_epoch)` | `session_service.rs:464-468` |
| `split_root()` and `split_key()` are private but reachable from the child test module | `session_service.rs:450-457`; `mod tests` is a child of the same module |
| `split_root() == remote_sessions_path.with_file_name("local-split-operations")`, and in `#[cfg(test)]` `DaemonServer::new()` puts that under a `tempfile::tempdir()` | `session_service.rs:450-452`; `server.rs:2039-2048, 2053-2063, 2121-2128` |
| the pre-check reads `split_root()/requests` before any status/create work and returns `Err` on a fingerprint mismatch | `session_service.rs:486-495` |
| `LocalSplitEnvelope { origin_epoch, expires_at_unix_ms, remaining_ms }` | `daemon/protocol.rs:56-60` |
| `SplitJournalEntry` fields (all `pub`) | `daemon/split_journal.rs:13-23` |
| `SplitJournal::open/upsert/load` | `daemon/split_journal.rs:51, 82, 105` |
| `SplitOperationResult` derives `Debug` (needed by `expect_err`) | `daemon/protocol.rs:99-101` |
| `IpcError.code` is `pub`, of type `IpcErrorCode` | `ipc/error.rs:234-238` |
| `IpcErrorCode::SpawnRequestConflict` | `ipc/error.rs:75` |
| `session_service()` returns `&Arc<DaemonSessionService>` (method calls deref) | `server.rs:2309-2311` |
| an existing `#[tokio::test]` in this module already uses `DaemonServer::new()` this way | `session_service.rs:2723+` (`paired_describe_reports_the_incarnation_the_attach_fence_proves`) |
| `"ws-conflict-fingerprint"` is not an SSH/remote id (`is_remote` = prefix match), so the pre-check is reached | `ssh/projects.rs:32-34`; `session_service.rs:471-473` |

**Not established:** the snippet has not been compiled or run. It is authored by reading; if a
compiler disagrees with any cited fact, the citation is what to re-check first.

## 3. The exact commands, and where the evidence lands

| Half | Exact command | Where the evidence should be recorded |
|---|---|---|
| Runner unit test (applied) | `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs` — the plan's canonical runner command (REPLAN line 69 / audit `G9`). Single test: append `-t "split-concurrent binds PASS to the conflict wave really rejecting the reused fingerprint"` | `E/task-7/logs/` (the runner suite's own log) with its exit file; the plan's Task 7 deliverable is `E/task-7/{logs,headless-result,cleanup.json}` |
| Rust test (ready to apply) | `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --nocapture --test-threads=1` | `E/task-3/logs/` next to the other `local_split_reliability_` runs (the plan's Task 3 deliverable is `E/task-3/{logs,request-traces,cleanup.json}`) |
| Scenario-level proof (real daemon, real conflict wave) | `node scripts/qa/pane-liveness.mjs --scenario split-concurrent --binary <APP binary> --evidence-dir <run dir> --isolation-root <run root>` (the plan's scenario invocation, REPLAN line 75) | `E/task-9/split-concurrent/run-UUID/{actions.jsonl,result.json,screenshot.png,cleanup.json}`; the `split-concurrent-batch-settlement` action now carries `conflictRejected`, `conflictRejectionCodes` and `conflictDistinctSessionIds` |

Both commands are recorded here as **what would exercise the tests**; neither was run by this lane.

## 4. Non-claims

- Nothing was compiled, run, or tested. The runner suite was **not** re-run, so this lane does not
  claim the suite is green with the new test in it. The suite's size is deliberately not restated
  here: the tree moved during this lane (`cfb4374b` → `14744c8a`; a concurrent lane added runner
  tests and a delegation-retry suite), so a count taken now would be a different measurement than the
  audit's recorded 28/28. The new test is deterministic (pure function, no I/O, no timing) and its
  four mutations are the reason it can fail.
- **Postscript (2026-10-05).** The concurrent commit `14744c8a` touched
  `scripts/qa/pane-liveness.test.mjs` but not `scripts/lib/qa-scenarios/split-scenarios.mjs`; this
  lane's two edits sit on top of the current content of both files (working tree shows exactly these
  two files modified, +33 and +38 lines).
- The Rust half is not on disk in `src-tauri/**`; it is delivered in this document, by this lane's
  writable scope. Applying it is one paste into `session_service.rs`'s existing test module.
- The pre-existing payload-formatting test (`ipc/terminal.rs:3744`) is **not** being claimed as this
  hole's closure; section 1 states exactly what it does and does not prove.
