# Integrated daemon/runtime OpenSSH verification

Date: 2026-09-09. Repair worker: st_01a084c1. Executed platform: macOS arm64.

## Result

All three exact commands exited 0 after the harness repair:

| Command | Remote PID | Retained counter | Evidence |
| --- | --- | --- | --- |
| `bun scripts/qa/ssh-process-survival.mjs --scenario transport-loss` | 77232 | 1 -> 2 | `transport-loss.log` |
| `bun scripts/qa/ssh-process-survival.mjs --scenario daemon-restart` | 77465 | 1 -> 2 | `daemon-restart.log` |
| `bun scripts/qa/ssh-process-survival.mjs --scenario reconnect-safety` | 77421 | 1 -> 2 -> 3 | `reconnect-safety.log` |

Each scenario exercised production RemoteRuntime and DaemonServer through their JSON socket surface and real `/usr/bin/ssh`, using existing loopback OpenSSH trust/authentication. Only a unique private TCP relay's QA transports were severed. The helper was a foreground owned Child copied from the existing debug artifact; no production daemon, sshd or user session was restarted.

Transport-loss reconnected automatically to identical backend association, scoped TargetRef, PID and nonce. Daemon-restart sent production Shutdown to only the isolated QA daemon, awaited its child and attachment close events, and started a separate daemon process with the same private persistence. Production startup restored the original process; no retry request, replacement create or resume was issued in either scenario.

Safety rejected outage input and recovered with counter 2, demonstrating that rejected input was not replayed. Eight explicit retry requests while already reconnecting were deduplicated without advancing the generation. A second outage exhausted automatic retries at attempts=5. Eight explicit retry requests after exhaustion started one new generation; counter advanced to 3. Canonical EOF then produced state=expired, failure.kind=missing and retained the original target rather than replacing it. The final safety proof correctly records `noRetryClick: false`, `explicitRetryRequests: 16`, and `automaticRecovery: false` for the complete scenario. It does not represent post-exhaustion recovery as automatic.

## Harness repair

- Removed `src-tauri/tests/ssh_process_survival_qa.rs`; no test was skipped, ignored or environment-gated.
- Moved its fixture-only entrypoint to `src-tauri/examples/ssh_process_survival_qa.rs` with `#[tokio::main]`.
- Runner builds `cargo build --manifest-path src-tauri/Cargo.toml --example ssh_process_survival_qa --message-format=json`, forces dev optimization level 0, resolves the emitted executable, and invokes it directly. It no longer presents an indefinitely serving test function as a passing Rust test.
- Cleanup now takes process-table snapshots, selects exact random fixture-root/nonce commands and owned Child PIDs, traverses their descendants (including production-created SSH children), then asserts no selected PID or matching fixture process remains before deleting the directory. Graph traversal is finite over one snapshot, not polling. Socket-close and Child-close events remain the teardown synchronization. Only owned Child handles are signalled; process-table matches do not authorize arbitrary kills.
- Production source and producer artifacts were read-only. No commits were created.

## Verification

`node --check scripts/qa/ssh-process-survival.mjs` exited 0. All three example builds and direct scenario executions exited 0; full compiler output, including existing warnings, is retained in the named logs.

Related regression command executed once:

```text
cargo test --manifest-path src-tauri/Cargo.toml --lib ssh_process_survival -- --nocapture
running 15 tests
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 823 filtered out; finished in 0.30s
```

Literal full regression output is `/tmp/st_01a084c1-tests.log`. No zero-test command is counted as coverage. The full ordinary Cargo suite was not run; removal of the obsolete integration-test file eliminates its fixture-required panic from ordinary test discovery.

LSP diagnostics were requested for the example and runner, but the daemon was unreachable at `/Users/indo/.omo/lsp-daemon/v0.1.0/daemon.sock`. This is unavailable diagnostics, not a clean LSP result. Actual Rust compilation and JavaScript syntax validation passed.

## Cleanup receipts

Every scenario log includes `exact-fixture-process-absence-audit` with `remainingProcesses: []`, followed by `remainingOwnedChildren: []`, `remainingSockets: 0`, `fixtureRemoved: true`, and `trustUnchanged: true`. PTY PID absence is separately asserted (valid only for this explicit loopback fixture). Fixture deletion requires the exact random ownership marker. Cleanup failure retains the directory and exits nonzero.

Removed fixture roots:

- `/private/tmp/fx-survival-r8eQ96` (transport-loss)
- `/private/tmp/fx-survival-Bcwip9` (daemon-restart)
- `/private/tmp/fx-survival-jm2Gr3` (reconnect-safety)

An independent final process-table search returned no `fx-survival-` processes. Host known_hosts and SSH config hashes remained unchanged in each run; no trust files were edited.

## Explicit missing scope: overall acceptance remains INCOMPLETE

- The example seeds one real process through `DaemonServer.terminal_service().remote().create`, then writes the persistence DTO shape. This bypasses production registered-project Spawn admission/journaling. Subsequent shutdown persistence and startup restoration are production code.
- `pane.json` is a fixture pane/backend association, not frontend workspace persistence or native pane lifecycle. No UI automation, native desktop run, screenshots or agent identity proof is claimed.
- Same PID/nonce/counter establishes continuity; helper-wide spawn counts are not instrumented. No claim that no unrelated target was ever allocated is made.
- Missing/exited-target handling is exercised; helper epoch change, host-key rejection, legacy direct sessions, sequence-gap presentation and remote worktree CWD are not covered by these scenarios. Unchanged trust hashes are not host-key-failure coverage.
- Linux and Windows remote execution are unclaimed. Linux requires trusted loopback OpenSSH and a matching prebuilt helper. Windows returns explicit nonzero UNAVAILABLE before fixture allocation; POSIX paths, signals and process snapshots are not a Windows/ConPTY substitute.
- No release build, production restart, trust change or external message occurred. These three macOS integration passes do not constitute full application/platform acceptance.
