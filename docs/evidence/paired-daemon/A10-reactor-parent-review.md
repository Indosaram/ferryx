# Parent reactor regression acceptance

The parent read the actual pty.rs diff, input fixture and child repair report.
The Unix pre-spawn entered-runtime check returns a typed SpawnError rather than
panicking before the fallible spawn API can respond. The existing synchronous
missing-command worktree test and its assertions remain unchanged.
No child is created on this missing-runtime path.

Independent command:

`cargo test --locked --manifest-path src-tauri/Cargo.toml
--no-default-features --test worktree_safety --test pty_input_cancellation
-- --nocapture --test-threads=1`

A10-reactor-parent-GREEN.log records exit 0, three input target passes
(including its child helper) and nine worktree safety passes.
The actual saturated input scenario used original PID 21777: 1,022 bytes
were accepted before WouldBlock; the child received exactly 1,023 after the
cancelled future was dropped and a new sentinel was sent. Cancelled bytes
did not arrive. The fixture reaped the child, joined the output drain and
removed its root. Worktree tests exercised synchronous spawn failure,
multiple interactive PTYs, five-worktree lifecycle isolation and natural exit.

All library initialization occurred under private HOME/FERRYX/XDG/TMP paths
in `/tmp/a10-owner-parent.La2KBZ`. Cargo/Rustup homes remained explicit,
with the existing private target, jobs 2, debug 0, incremental 0 and empty
RUSTC_WRAPPER. Parent read the actual final assertions/results, checked exact
PID 21777 absent, and removed seven empty directories plus supervisor using
rmdir. Existing compiler warnings remain visible.

This closes `A10 repair: preserve synchronous spawn failure contract`.
It does not close portable saturated input, controller-generation cancellation,
Windows composition, receiver-drop cleanup or the full A10 contract.
The no-runtime error identifies the absent runtime, not executable resolution;
an entered runtime constructed without I/O enabled is outside this repair.
No canonical daemon, desktop, user PTY, commit or release operation occurred.
All changes remain uncommitted.
