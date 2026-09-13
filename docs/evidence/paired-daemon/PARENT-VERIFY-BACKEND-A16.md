# Parent backend verification after A15 + A16

Run by the parent, with real exit codes (no pipes), after the Rust tree was free
of concurrent editors.

```
which cargo -> /Users/indo/.cargo/bin/cargo

cargo test --locked --manifest-path src-tauri/Cargo.toml \
  --no-default-features --lib paired_host:: -- --test-threads=1
PH_EXIT=0
test result: ok. 30 passed; 0 failed; 0 ignored; 995 filtered out; finished in 0.68s

cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib
LIB_EXIT=0

cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features \
  --bin ferryx-cli --bin ferryx-relay
BIN_EXIT=0
```

A16's claim of "30 passed, 0 failed" is confirmed. The suite grew 27 -> 28 (A15)
-> 30 (A16) across this wave, and never went red.

## Two false signals the parent had to clear first

Both were defects in the parent's own verification harness, not in the code. They
are recorded because each one, taken at face value, would have produced a wrong
verdict.

**1. False RED: `resource path '../ui/dist' doesn't exist`** - every cargo command
returned 101. The cause was not a compile error: the delete-dialog repair child
removed `ui/dist` as "generated build output" during cleanup, and Tauri's build
script requires it. Rebuilding with `bun run --cwd ui build` (exit 0) restored it.
Lesson now propagated to later packets: clean up only what you created; `ui/dist`,
`node_modules` and `src-tauri/target` are shared build inputs, not scratch.

**2. False GREEN, then false RED, from the harness itself**:
- `(cargo ... | tail -6); echo $?` reports **tail's** exit status, so a failing
  cargo run printed `PH_EXIT=0`. Exit codes must be captured by redirecting to a
  file and testing `$?` directly.
- Overriding PATH to force Node 22 dropped `/Users/indo/.cargo/bin`, so cargo
  became `127: command not found`. The correct PATH carries both:
  `/Users/indo/.cargo/bin:/Users/indo/.local/bin:/Users/indo/.bun/bin:/opt/homebrew/bin:/usr/bin:/bin`.

## What this verification does NOT establish

A16 self-reported, and the parent accepts, that the following remain unproven:
real UDS/renderer end-to-end integration, frontend surface fencing,
metadata/events, automatic recovery, durable restoration, and remote close
integration. `pairedDaemonProxyV1` is still advertised **false**, and the two
contract assertions that pin it were left untouched - correctly, since the full
path was not proven.

Therefore **AC08 is still open**. A green paired_host suite is not AC08.

A16 also disclosed a HOME-isolation lapse on its first compile, corrected on
subsequent runs. Disclosed rather than hidden, and it does not affect the results
above, which the parent ran itself in this worktree.
