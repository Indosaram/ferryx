# A09 blocked before behavioral RED

Status: NOT IMPLEMENTED. No A09 capability or acceptance approval.

The full approved plan and Wave 1 handoff boundaries were read. The inherited
uncommitted composition was preserved; git HEAD was not used to replace it.

## Source delta versus supplied seed

- Added `src-tauri/tests/machine_sessions.rs`: private owner HTTP creation
  assertion, with panic-caught cleanup of owned PTYs, listener and temp root.
- No production source changes. Concurrent terminal-wire files belong to A10.
- Added this note and validation logs. No commit or deployment.

## Commands and results

Working directory: `/Users/indo/code/project/orca-lite-wt/herdr-wave2`.

Initial `git --no-pager status --short` exited 128 because the provisioned
Ghostty submodule path is a symlink. Survey succeeded (exit 0) using
`git -c diff.ignoreSubmodules=all --no-pager status --short --ignore-submodules=all`
and `git --no-pager diff --ignore-submodules=all --stat`.

```
CARGO_BUILD_JOBS=4 RUSTC_WRAPPER= CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite-wt/herdr-wave2/src-tauri/target cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test machine_sessions -- --nocapture
```

Exit 101. Full output: `A09-RED.log`. Despite its intended filename, this is
NOT behavioral RED: linker `write() failed, errno=28 (No space left on device)`
and rustc LLVM output-stream failures prevented test execution. No test
assertion ran. No blind retry was made and no production edit followed.

`df -h .` reported 119 MiB available, 100% capacity. `du -sh src-tauri/target`
reported 6.1 GiB. Both exited 0. The target existed before this task and its
inherited artifacts were not deleted. Freeing space outside this lane or
authorizing removal of inherited artifacts requires parent coordination.

LSP diagnostics on the added test returned no diagnostics.
`git --no-pager diff --ignore-submodules=all --check` exited 0.

## Actual runtime proof and limits

None: the integration executable did not run. There is no shell PID/CWD/epoch
proof, no behavioral RED, no GREEN, and no session/journal implementation.
All requested CRUD, durable spawn, revocation, controller, close/reap and
reconciliation gates remain unexecuted. A10-dependent proofs also remain open.

No private fixture listener, daemon or PTY was launched. Process inspection
after the failed build found no wave2 process other than the inspection shell
and its rg command. Canonical daemons/PTYS and other worktrees were untouched.
See `A09-cleanup.log`. The disk-space blocker must be resolved before the
mandatory actual RED can precede implementation.
