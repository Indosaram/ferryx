# Linux verbatim retrieval — daemon_persistence_contract single failure

Host: linux (indo@100.91.254.71, hostname `indo`)
Command: `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_persistence_contract -- --nocapture --test-threads=1`
cwd: /home/indo/ferryx-pane-completion/source-21dea3c0
Raw native exit: 101
Log: pass3/linux/logs/daemon_persistence_contract.log (lines 639-676)

## Verbatim

```
running 15 tests
...
test test_daemon_output_sequence_contiguity_and_replay_gap ... 
thread 'test_daemon_output_sequence_contiguity_and_replay_gap' (1638024) panicked at tests/daemon_persistence_contract.rs:1098:9:
assertion `left == right` failed
  left: 0
 right: 1
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
V01 reaped owned daemon 1638025; removing /tmp/fx-v01-JpD5hf
FAILED
...
failures:
    test_daemon_output_sequence_contiguity_and_replay_gap

test result: FAILED. 14 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.97s

error: test failed, to rerun pass `--test daemon_persistence_contract`
```

## Extracted facts

- Failing test name: `test_daemon_output_sequence_contiguity_and_replay_gap`
- Panic site: `tests/daemon_persistence_contract.rs:1098:9`
- Assertion message: `assertion \`left == right\` failed` — left: 0, right: 1
- Selected: 15 (list gate exit 0, count 15)
- Result line: `test result: FAILED. 14 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.97s`
- The two other panics in the log (`test_harness_panic_cleanup_preserves_concurrent_owned_daemon` :669 "injected harness owner panic", `test_private_daemon_cleanup_on_panic` :698 "exercise harness unwind cleanup") are **intentional injected panics** — both tests report `ok`.
