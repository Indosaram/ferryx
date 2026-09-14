# A12 watch refresh cancellation

Scope: plan A12 / 6.1 / 8, catalog-contended machine-event native-watch refresh only. Not full A12 completion. Worktree: `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`; platform Darwin arm64.

## Repair

The old machine event loop awaited `WorkspaceWatch::refresh` before entering either snapshot admission or the cancellable 10-second read deadline. Each real socket could therefore retain a subscription and a native watcher in a catalog-mutex-blocked blocking job indefinitely.

Watch admission, creation and refresh now run inside the existing read deadline / socket-close select. A separate authority-owned two-permit semaphore bounds concurrent refresh work, not live sockets (the existing 16-socket admission stays unchanged). `WorkspaceWatch::refresh_bounded` moves its owned permit into the watcher before blocking work starts; after cancellation the blocking closure/result retains it until the native watcher is dropped. On normal completion it releases the refresh permit, so healthy idle sockets do not monopolize refresh admission. An unavailable snapshot emits the existing partial boundary and terminates the subscription rather than continuing without its watcher. No publication/revision semantics were changed.

Parent authorized narrow workspace_watcher ownership after finishing recursive-mode promotion. That existing promotion test is unchanged. The only observer addition is cfg(test) invocation of the existing transaction probe immediately before the actual catalog read.

## Behavioral RED

`A12-watch-red.log`, exit 101: two executed tests, two expected behavioral failures, 11.08s.

- `client close could not release subscription while actual catalog mutex held: Elapsed(())`
- `10-second read budget did not include catalog-contended watch refresh: Elapsed(())`

Both panic paths logged writer joined, subscriptions zero, gateway joined and private root removed before resuming the assertion panic. RED source hashes: `A12-watch-red.sha256`. The unused watch semaphore existed for resource assertions, but no production admission/cancellation repair existed yet.

The test subscribes to a oneshot proving the real catalog mutex is held, an exact bounded refresh-request channel before dialing, and the subscription watch before triggering socket closure. No sleep or polling decides correctness. A second blocked refresh fills the worker budget; three more real reconnect/close cycles must not create another refresh worker. Deadline tests use actual elapsed time because the 10-second budget is the behavior under test.

## Commands and isolation

All commands run from the worktree above. Each execution family creates its own shell-owned root before process initialization:

```bash
run=$(mktemp -d /tmp/a12-watch-green.XXXXXX)
mkdir -p "$run"/{home,runtime,data,config,cache,tmp}
export HOME="$run/home" XDG_RUNTIME_DIR="$run/runtime"
export XDG_DATA_HOME="$run/data" XDG_CONFIG_HOME="$run/config" XDG_CACHE_HOME="$run/cache"
export TMPDIR="$run/tmp" FERRYX_RUNTIME_DIR="$run/runtime" FERRYX_DATA_DIR="$run/data"
export CARGO_HOME=/Users/indo/.cargo RUSTUP_HOME=/Users/indo/.rustup
export CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib machine_event_cancellation -- --nocapture
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test machine_events -- --nocapture
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib parent_watch_promoted_to_project -- --nocapture
```

RED used prefix `/tmp/a12-watch-red`; the initially compile-blocked integration/promotion attempt used `/tmp/a12-watch-integration`. The final serial GREEN / integration / promotion run shared the private `/tmp/a12-watch-green` environment. Actual roots are recorded in the adjacent `*-root.txt` files. Commands run in monitored background shells; logs and numeric exit receipts are adjacent. Existing `src-tauri/target` is used; no daemon CLI, user daemon, desktop or foreign PTY is invoked. New tests use ephemeral loopback listeners, scoped machine grants, private per-test authorities, and `crate::ipc::run_blocking` for filesystem work.

## Concurrent build blockers (not RED)

Initial GREEN compile overlapped metadata schema changes: `session_service.rs:1075` missing new `agent_type` and `title` fields (`E0063`), preserved in `A12-watch-metadata-build-block.log`. Next attempt overlapped metadata extraction: wrong `super` module imports in `session_metadata_events.rs:2` (`E0432`, downstream `E0282`), preserved in `A12-watch-metadata-extraction-block.log`. Integration attempt hit the same issue (`A12-watch-integration-build-block.log`). Those files belong to another worker and were not changed or bypassed here.

## GREEN / final cleanup

| Command selector | Result | Test elapsed | Log |
| --- | --- | --- | --- |
| `--lib machine_event_cancellation` | 2 passed, exit 0 | 20.09s | `A12-watch-green.log` |
| `--test machine_events` | 1 passed, exit 0 | 10.00s | `A12-watch-integration.log` |
| `--lib parent_watch_promoted_to_project` | 1 passed, exit 0 | 0.04s | `A12-watch-promotion.log` |

GREEN receipts explicitly show both close/deadline variants releasing subscriptions with the catalog still held, retaining workers 1 then 2, and three more reconnects producing zero extra refresh workers. Both actual catalog writers were released and joined; acquiring both watch permits awaited blocking-worker/native-watch drainage; subscriptions reached zero; private gateway tasks joined and per-test roots were removed. Integration exercised real machine/mirror sockets, filesystem availability, external Git checkout, worktree HTTP changes, 70 overlapping registrations, lag reconciliation, original PTY identity preservation and explicit PTY reaping. Its cleanup receipt includes listener refusal and all owned PTYs closed.

All three changed Rust files have clear LSP diagnostics. `git diff --check --ignore-submodules=all` passed. File sizes are 119/98/110 pure LOC for machine_events / cancellation tests / workspace_watcher. Successful test compilation reports 17 lib-test / 19 lib warnings in existing unrelated code; none were suppressed. The two compile-blocker attempts above remain explicitly recorded rather than being represented as test failures.

Final source hashes are in `A12-watch-green.sha256` and were checked against current source after tests. Log hashes are in `A12-watch-logs.sha256`. `A12-watch-cleanup.txt` records removal of exactly the three owned shell supervisor roots. The final background supervisor (PID 34117) exited after all numeric exit receipts were recorded; unrelated concurrent Cargo processes were left untouched.

Architectural review: responsibilities remain narrow; no new parsing boundary, unsafe code, typed-domain escape, variant discrimination, logging, destructive production action or revision arithmetic was introduced. Existing typed authorization and publication remain unchanged. `refresh_bounded` has three parameters including self and uses the existing refresh implementation, avoiding a duplicate watcher algorithm. No parent mode test, metadata test or foreign source was weakened. No new prose-pinning tests. This is focused Darwin headless cancellation evidence, not native-renderer, Linux, relay or complete A12 acceptance.
