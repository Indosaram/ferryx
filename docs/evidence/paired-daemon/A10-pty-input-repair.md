# A10 production PTY input candidate - Unix GREEN / portable repair NOT READY

Expanded task st_01a0983c. Only terminal/session.rs, terminal/pty.rs and the new
tests/pty_input_cancellation.rs were edited in this continuation. A12/A11 socket,
metadata, protocol, module and machine-stream fixture files were not edited.
Inherited A09 pty.rs spawn/close changes remain intact. No dependency, Cargo.lock,
clipboard, canonical daemon, UI, other-worktree or remote-host writes.

## Delivered candidate

`PtySession::write_input_cancellable(&[u8])` is a new async seam. On Unix it
uses a nonblocking owned PTY descriptor registered with Tokio AsyncFd, serialized
async admission, a 64 KiB input bound and a ten-second operation deadline.
Dropping the future drops the operation itself: no input worker or late queue.
Already accepted kernel bytes cannot be withdrawn; partial failure must never
be retried as the whole frame.

The underlying portable-pty descriptors share nonblocking flags. The reader
therefore handles WouldBlock through blocking poll in its existing owned reader
task. The existing synchronous Local input method retains its blocking behavior
through poll on WouldBlock; it is NOT the cancellable seam and must not be used
by the machine socket integrator. SSH source is unchanged. Existing PTY close,
interrupt/escalation and sibling-survival tests pass.

This candidate is NOT the complete portable repair. On non-Unix the new async
seam returns PTY_CANCELLABLE_INPUT_UNSUPPORTED without writing. Existing Windows
Local input remains unchanged. portable-pty 0.9 ConPtyMasterPty exposes its input
pipe only as Box<dyn Write + Send>, not a cancellable pipe HANDLE. Its writable
FileDescriptor is private. CancelSynchronousIo operates on a thread, not this
opaque writer, and a cancellation-before-syscall race is not solved by moving
write_all into a thread. A genuinely cancellable Windows implementation requires
an exposed pipe/overlapped I/O surface or a separately reviewed native ConPTY
adapter. Dependency edits and Windows execution were not authorized here.
No Windows compile/runtime or Linux runtime claim is made.

The socket integrator must explicitly call the new seam after coordinated
composition. No remote integration was changed while A12 owns those files.
Socket output-byte-budget and legacy-owner epoch gaps remain OPEN.

## Actual RED before production changes

Command:
`cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test pty_input_cancellation -- --nocapture`

`A10-pty-input-RED.log`, exit101. Real private child PID28538 configured its PTY
raw/no-echo and waited on a private Unix control socket instead of reading stdin.
A 1 MiB call to the production synchronous write remained blocked at the bounded
100ms observation. Time was the behavior under test, not readiness guessing.
The fixture then explicitly released the child, joined the writer, verified all
bytes, reaped the child, joined output drain and removed the root before failing:
`production PTY write blocked on nonreading child and could not be cancelled`.
This is an actual runtime RED, not a build or source-search failure.

## GREEN and focused regressions

The same command passed in A10-pty-input-GREEN-attempt1.log (exit0, two tests),
then the fixture was improved to catch assertion panics and run teardown and to
check oversized admission/original PID. Final source commands:

- `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test pty_input_cancellation -- --nocapture`: exit0, 2 tests.
- `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib terminal::pty::tests -- --nocapture`: exit0, 5 tests.
- `git diff --ignore-submodules=all --check`: exit0.

Final logs: A10-pty-input-final.log. No aggregate, remote/socket or platform tests
were run. LSP returned no errors for session.rs and no diagnostics for pty.rs and
the new fixture. Compilation retained existing warnings without suppression.

Final original PID32647, fixture CWD `/tmp/a10-input-final.8hvx8U/tmp/.tmpulirtf`.
Child sends its PID only after configuring raw/no-echo; parent reads this exact
signal before saturation. Direct writes fill the actual kernel queue to
WouldBlock: 1022 bytes accepted. The new production async seam is polled pending
with LATE bytes, then its scope is dropped before the control socket releases
the child. A fresh valid sentinel write completes; the same original child
counts exactly1023 bytes, not1027. No worker exists to resume cancelled input.
Oversized65537-byte input is refused. Original PID is checked again before close.
Child reaped, output drain joined, root removed; supervisor
`/tmp/a10-input-final.8hvx8U` removed. This tests future-drop cancellation, not an
actual socket controller generation, and does not claim the ten-second timeout
has separately been exercised under controlled time.

All runs used private src-tauri/target, CARGO_BUILD_JOBS=4, RUSTC_WRAPPER=,
CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0, CARGO_INCREMENTAL=0,
fresh /tmp HOME/FERRYX_RUNTIME_DIR/FERRYX_DATA_DIR/FERRYX_SESSION_DIR/XDG/TMPDIR,
and explicit original /Users/indo/.cargo and /Users/indo/.rustup toolchain homes.
No process discovery or canonical daemon connection occurred.

## Exact current candidate hashes

SHA-256:

- terminal/session.rs: `6855f72f39c8988c181985c64b05a1d6eb39567897d65c2754940f1212500023`
- terminal/pty.rs: `b87e68d6cad56ac9b359f3c7bf60eb2b8178a1679f28d4ad1477ba48d68ec123`
- tests/pty_input_cancellation.rs: `4ce8522764943fe32406ed54509a8bb28d310a2c487de5dce7371d0f66642cf1`

Paths are relative to src-tauri/src for the two production files and src-tauri
for the test. All edits remain uncommitted. Unix candidate is runtime-proven;
portable completion and integration are NOT READY and must not be inferred
from these focused passes.
