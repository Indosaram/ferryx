# Atlas renderer contract baseline - 2026-09-06

The exact existing contract **failed**. It executed one test, not zero: **0 passed, 1 failed, 0 ignored, 0 measured, 24 filtered out**, with Cargo exit code **101**. The failure is the initial atlas accounting assertion, before the repeated dirty-update loop.

## Invocation and execution

Working directory: `/Users/indo/code/project/orca-lite`.

Executed exactly once, without retries or extra flags:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_renderer_contract dirty_update_atlas::test_renderer_dirty_update_sequence_and_bounded_glyph_atlas_cache -- --exact --nocapture
```

- Started: `2026-09-06T14:28:19Z`.
- Finished: `2026-09-06T14:29:43Z`.
- Compilation: existing Cargo cache, test profile; reported `1m 22s`.
- Test execution: reported `0.39s`.
- The literal command ran under a Bash-3-compatible shell `monitor` function; completion was awaited using `wait`, with an enclosing 1,800-second tool timeout. No polling or sleeps were used.

## Actual failure evidence

Verbatim relevant raw output:

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1m 22s
     Running tests/native_terminal_renderer_contract.rs (src-tauri/target/debug/deps/native_terminal_renderer_contract-fe01df4e136ffbc1)

running 1 test

thread 'dirty_update_atlas::test_renderer_dirty_update_sequence_and_bounded_glyph_atlas_cache' (6997259) panicked at tests/native_terminal_renderer_contract/dirty_update_atlas.rs:29:5:
atlas allocated bytes must not exceed max capacity
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test dirty_update_atlas::test_renderer_dirty_update_sequence_and_bounded_glyph_atlas_cache ... FAILED

failures:

failures:
    dirty_update_atlas::test_renderer_dirty_update_sequence_and_bounded_glyph_atlas_cache

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 24 filtered out; finished in 0.39s

error: test failed, to rerun pass `--test native_terminal_renderer_contract`
```

The existing test at `src-tauri/tests/native_terminal_renderer_contract/dirty_update_atlas.rs:29` asserts:

```rust
initial_stats.allocated_bytes <= initial_stats.max_capacity_bytes
```

The test source was read after execution to identify this assertion. The initial render, width assertion, and positive atlas-entry-count assertion precede it; the 50-update loop and final boundedness assertions follow it and were not reached. This reproduces the accounting contract violation. The output does not print the two numeric byte values and does not by itself establish the proposed entry-overhead explanation, actual GPU memory exhaustion, or native display/pixel correctness.

## Evidence artifacts

Artifact directory:

```text
/Users/indo/code/project/orca-lite/.omo/ulw-loop/rendering-review-20260906/baseline/
```

| Artifact | Evidence |
| --- | --- |
| `atlas-contract.log` | Complete combined stdout/stderr, including compilation warnings and failure output |
| `atlas-contract.exit-code.txt` | Observed Cargo exit code `101` |
| `atlas-contract.invocation.txt` | Exact command |
| `atlas-contract.monitor.txt` | UTC timestamps, owned PID, exit status, wait/reap receipt, final process check |
| `atlas-contract.before.state.txt` | HEAD, HEAD tree, and SHA-256 of tracked working diff before invocation |
| `atlas-contract.after.state.txt` | Same identity capture after invocation |

Both identity snapshots agree:

- HEAD: `b8f82d707f0cb99907e3d79c0c9cdc75053ef931`.
- HEAD tree: `7c1bb89729859ef59cb928fb99008f31174338f5`.
- SHA-256 of `git diff --no-ext-diff --binary HEAD`: `785c7b5e0838edc451b9a247e4bad8ffc38f9de0acdcc99ff4876dc3db144153`.

These hashes cover tracked changes against HEAD, not untracked or ignored files. No production code, tests, manifests, or previous baseline report was changed. No diagnostics, broad checks, desktop launch, cache cleanup, or repair was performed. The requested Cargo command generated its normal test-build artifacts.

## Owned-process cleanup

Owned Cargo PID `97748` was awaited and reaped with exit `101`. The subsequent `ps -p 97748 -o pid,ppid,state,command` returned only its header with exit `1`, confirming that PID was no longer present. No signals were sent and no foreign process was killed or restarted. The monitor completed normally; raw evidence is retained. This report is uncommitted.
