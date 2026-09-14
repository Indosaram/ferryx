# Parent output socket verification

Status: focused batch GREEN; A10 remains incomplete.

The parent read the complete machine output writer, its three tests, the
new socket integration test, and the changed machine socket admission,
snapshot, live output, control, input and cancellation paths in server.rs.
The referenced machine_output_socket.rs does not exist; the actual integration
is in server.rs and tests/a10_output_socket.rs.

## Independently executed batch

Both commands used `--locked --manifest-path src-tauri/Cargo.toml
--no-default-features` with the existing private target, jobs 2, debug 0,
incremental 0, empty RUSTC_WRAPPER and normal Cargo/Rustup homes.
HOME, FERRYX runtime/data/session paths, XDG config/data/cache and TMPDIR
were isolated under `/tmp/a10-owner-parent.TBGmKH` before library initialization.

- `cargo test ... --lib machine_output_writer -- --nocapture`: exit 0,
  3 passed. Log: A10-output-parent-writer.log.
- `cargo test ... --test a10_output_socket --test a10_output_budget
  --test a10_output_budget_pty --test machine_terminal_stream -- --nocapture`:
  exit 0, 11 passed. Log: A10-output-parent-batch.log.

The parent read the actual final test results and runtime signatures.
The injected publisher panic was caught by its intended cleanup test, not
an unreported failure.

The real PTY publisher delivered 2,097,152 bytes to its progressing subscriber;
the held subscriber peaked at 1,047,560 charged bytes against a 1,048,576 ceiling.
The socket fixture disconnected after unequal explicit hub publishes totaling
1,100,000 bytes, preserved original PID 98373, proved sibling shell PID 98390,
and decoded an explicit replay gap on reconnection.
The machine stream fixture exercised original PIDs 98521 and 98588 across
selection changes, controller replacement, stale input/resize, conflict,
replay/gap, ticket reuse, size bounds and revocation.

Exact PID inspection after the batch found no remaining processes for
98286, 98287, 98373, 98390, 98521 or 98588. The fixture logs report reader
completion, reaping, joined listeners and removed private roots.
The supervisor contained only seven empty directories; `rmdir` removed
those directories and the supervisor, and a subsequent existence check was false.

## Combined live proof accepted by parent

The missing combined scenario was subsequently delivered in
tests/a10_output_live_socket.rs with support/a10_observed_listener.rs.
Parent read both complete files, then added Unix-only fixture gating and
a deadline around exact-peer connection selection. LSP returned no diagnostics.
The transparent observer forwards real TCP I/O and records Pending and Drop;
it does not manufacture backpressure.

Initial combined parent execution stopped at a concurrent journal compiler
error before any tests (A10-live-reactor-parent.log, exit 101). After the owner
corrected the malformed cfg, parent ran:

`cargo test --locked --manifest-path src-tauri/Cargo.toml
--no-default-features --test a10_output_live_socket -- --nocapture`

A10-live-parent-corrected.log records exit 0, one passed in 3.65 seconds.
Original shell PID 15492 emitted 67,108,983 bytes including its completion
marker. Its exact upgraded socket produced real TCP Pending and server Drop
before the ten-second deadline, without client consumption or grant revocation.
Sibling PID 15509 continued, and reconnection with cursor 0 returned an explicit
gap and preserved the original PID. No direct hub publication drove this test.

Both PTYs reported reaped/reader-finished, the listener joined and fixture
root was removed. Parent exact-PID inspection found neither process.
The seven empty directories and supervisor `/tmp/a10-owner-parent.gA6zx8`
were removed with rmdir. This closes the scoped queued output byte budget
repair, not the whole A10 packet or total buffering across relay hops.

## Historical missing proof and remaining ownership

Separate actual PTY saturation, blocked TCP writer and HTTP-upgraded socket
tests are not a single proof of continuous PTY output through a demonstrably
blocked upgraded socket, overflow termination, sibling progress and reconnection.
Worker st_01a0986b was revived to add that combined scenario, preferably in
a new integration target, preserving the existing assertions and distinguishing
protocol failures from expected abrupt overflow transport termination.

The synchronous missing-command spawn reactor panic is owned separately by
st_01a0989f. Final owner retirement and replacement operation reconciliation
are owned by st_01a09869; that worker now also has the narrowly necessary
session API/journal adapter scope, preserving parent contention tests.
Dropped sole PTY receiver cleanup remains an independent open concern.

Compilation warnings remain visible, including an unused semaphore permit
result in the parent journal test. This focused pass is not a claim that the
whole worktree is warning-free, that Windows/Linux composition passes, or that
the full approved plan is complete. All source and evidence remain uncommitted.
