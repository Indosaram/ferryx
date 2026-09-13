# A09 journal contention: open aggregate review concern

## Source-backed finding

`src-tauri/src/remote/machine_operation_journal.rs:106-125` performs private
JSON replacement and directory sync while its caller retains the state mutex.
`begin` and `commit_spawn` acquire that parking_lot mutex before persistence.
`reconcile`, `session`, `sessions` and `owns_session` acquire the same mutex.

`src-tauri/src/remote/session_api.rs:105,131` calls reconcile synchronously
inside a Tokio async task. The read adapter at lines 164-177 calls machine
detail/list synchronously after admission, without wrapping the read in
timeout/revocation selection. A slow journal writer can therefore make these
readers wait synchronously on a runtime thread. A10's in-progress target
validation also reads the journal and needs to account for this boundary.

This is not a claim that the read methods themselves perform disk I/O.
The concern is their shared lock with a durable writer. No observed latency
number, deadlock or failed runtime test is claimed by this source review.

## Evidence and limits

The parent read the journal, session API, service adapters and the existing
workspace_api admission/execute wrapper. LSP references confirmed session API
reconcile calls. No source was changed; existing A09 ordinary runtime proof
is preserved. A10's worker was informed to avoid the same blocking read on
its input/socket thread, without taking ownership of session_api.rs.

The current A09 passing tests exercise replay, crash, capacity and revocation,
but the inspected scenarios do not hold the durable-write lock while proving
independent HTTP scheduler progress and deadline/revocation response.

## Required aggregate adjudication

The Wave2 verifier must inspect the final composed code, then use an exact
subscribed barrier to hold a fixture-owned journal write while session reads
and mutation replay contend. Verify unrelated health/runtime progress,
bounded timeout/revocation behavior, retained admission permits until the
worker actually drains, and no later result publication after cancellation.

Use a bounded external watchdog and explicit release/join cleanup so a blocked
runtime cannot also prevent its own test timeout from firing. Do not use
sleeps or an unbounded lock-holder. Do not replace journal access with a mock
that removes the contention being asserted.

If reproduced, assign the minimal HTTP/off-thread admission repair plus its
RED/GREEN test to a disjoint repair lane. Do not add a second journal, weaken
durability, or edit shared A10/A12 files concurrently. If final source already
resolves the concern, close it with runtime evidence rather than a source-only
assertion. The tracked item is:
`A09: prove bounded journal contention handling`.
