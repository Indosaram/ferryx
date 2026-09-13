# A10 saturated machine socket report

## Result

macOS arm64 actual WebSocket/PTY reproduction confirmed and repaired global-controller input serialization and full-input-queue cancellation starvation. This is scoped A10 evidence, not full-plan or cross-platform acceptance.

Production changes are confined to machine-control blocks in `src-tauri/src/remote/server.rs`. No changes by this task to session_service, PTY adapter/vendor/dependencies, metadata, output framing/budgets, relay, protocol/client, paired-host or UI. A11 async target validation remains intact.

## Mechanism and repair

Before repair, receive retained the whole machine_controllers mutex through target validation and the pending `PtySession::write_input_cancellable` await. Replacement needed that mutex before sending cancellation. A saturated session therefore blocked both replacement and sibling input. The socket reader also awaited capacity-one input admission without observing Close.

Receive now subscribes to the admitted generation under the map mutex and releases the map before asynchronous work. Each operation poll holds that generation's watch read guard; replacement's existing `send_replace(true)` takes the corresponding write guard. Consequently an old-generation IO poll either precedes fencing or observes cancellation; it cannot pass a stale check and perform IO after fencing. No global map guard survives a pending input await. The outer lease cancellation remains responsible for dropping the operation immediately.

Reader retains bounded ordered backpressure but selects one lookahead read while admission is pending. Close/EOF cancels immediately; any further frame beyond the bounded window ends the socket rather than buffering input for later replay. The existing oversized-text close behavior is retained.

## Actual reproduction

`tests/support/machine_input_cancellation.rs` is compiled as a library test module from server.rs so a cfg(test) observer can report the exact pending poll of the *real* PTY future. `machine_input_probe.rs` never gates, mocks or replaces production IO. Tests use actual HTTP creation, authenticated WS upgrade, a real owned shell exec'd into a nonreading Perl child, raw terminal mode, a private Unix control socket, and libc writes until actual kernel WouldBlock. The child acknowledges its original PID before filling; it reads nothing until explicitly released, reports drained byte count and forbidden-byte count, then exits under owned cleanup.

The former SocketBackend-gated `a10_pending_machine_input_is_dropped_on_disconnect_and_revoke` now invokes those real production scenarios. Its obsolete fake boundary is removed, not skipped to hide a failure. Native Windows saturation is not covered by this Unix fixture; the old disconnected mock did not provide Windows production evidence either. Dedicated scenarios are retained in support rather than an independently auto-discovered `tests/machine_input_cancellation.rs`: the exact-poll observer must compile into the library under cfg(test), not leak as a production public API. Test helper entrypoints do not inflate test counts.

## RED

Command, through `A10-saturated-run.sh`:

```
--no-default-features --lib machine_input_cancellation_tests -- --nocapture --test-threads=1
```

`A10-saturated-RED.log`:

```
A10 kernel WouldBlock original_pid=2261 accepted=1022
A10 real socket input pending scenario=SaturatedQueue
cancellation must drop pending input without waiting for PTY timeout: Elapsed(())
A10 kernel WouldBlock original_pid=2292 accepted=1022
A10 real socket input pending scenario=Replace
new generation admission must not wait for saturated PTY: Elapsed(())
sibling PTY must not serialize behind saturated machine input: Elapsed(())
test result: FAILED. 2 passed; 3 failed
```

Plain disconnect and revocation already passed RED with received=1023 and forbidden=0. This refutes an adapter cancellation leak in those observed cases; the adapter was not changed.

The RED fixture cleanup initially raced natural child removal after a failed assertion; cleanup now handles the lifecycle's concurrent removal explicitly. A first candidate using immediate try_send broke ordinary resize/ping bursts; existing machine_terminal_stream caught it. A second candidate accidentally changed oversized text from close to error; that same existing suite caught it. Both regressions were repaired without changing its assertions. Intermediate failure logs are retained, not presented as acceptance.

## Final GREEN

`A10-saturated-FINAL.log`, same five scenario tests:

```
input_is_cancelled_when_full_queue_precedes_close ... original_pid=15002 accepted=1022 received=1023 forbidden=0
input_is_cancelled_when_generation_is_replaced ... original_pid=15036 accepted=1022 received=1023 forbidden=0
input_is_cancelled_when_grant_is_revoked ... original_pid=15073 accepted=1022 received=1023 forbidden=0
input_is_cancelled_when_socket_disconnects ... original_pid=15114 accepted=1022 received=1023 forbidden=0
A10 sibling original_pid=15170 executed while first saturated
A10 drained original_pid=15149 received=1023 forbidden=0
test result: ok. 5 passed; 0 failed; 0 ignored
```

Every scenario used exact readiness/pending/drop/drain signals with bounded failure deadlines, no fixed sleep or polling delay. Replacement also attempted stale-owner Z input after new attachment; drained suffix remained absent. Original child PID matched before saturation and after draining. The sibling's actual shell-generated PID marker arrived before releasing the first child.

Additional final verification:

- `A10-saturated-related-FINAL.log`: full existing machine_terminal_stream passed; full pty_input_cancellation passed (three harness entries, one is child entrypoint, not three independent behavioral claims).
- `A10-saturated-controller-FINAL.log`: controller generation/reservation boundary, actual HTTP-close authority transfer, and repaired disconnect/revoke test all passed (3 tests).
- LSP final diagnostics: no errors for server.rs, security_socket_tests.rs, machine_input_probe.rs, machine_input_fixture.rs; support scenario diagnostics also clean. Earlier transient diagnostic timeouts were not treated as passes.
- `A10-saturated-headless-check.log`: `cargo check --no-default-features --bin ferryx-cli --bin ferryx-relay` exit 0, after diagnostics. Existing unrelated warnings remain; none suppressed.
- Actual user surface exercise is the authenticated WS + HTTP + owned original PTY scenarios above; no canonical daemon or desktop was launched.

An intermediate lib compile was blocked by another lane's in-flight paired_host/native_operation_tests errors (`A10-saturated-lookahead.log`). Those were resolved by their owner before the final reliable runs, not patched here.

## Cleanup and provenance

Runner clears inherited environment with env -i before library initialization and supplies private HOME, FERRYX runtime/data/session, XDG config/data/cache/state/runtime, TMP, CODEX_HOME and CLAUDE_CONFIG_DIR. All test execution cwd is private. Cargo/Rustup caches alone reference installed toolchains. Each server binds loopback port zero; each control path is unique and unlinked after accept. Owned PTYs are closed/reaped, lifecycle tasks drained, server JoinSet shut down, and private roots removed even after captured assertions. Final exact PIDs and roots are listed in `A10-saturated-cleanup.log`; no final owned PIDs/root paths remained. No SSH, user desktop, canonical daemon, commit, release or deployment action occurred.

`A10-saturated-source.sha256` hashes the modified source and final/RED logs. Shared files contain other lanes' changes, so hashes are a time-specific composition snapshot, not exclusive authorship claims. Work remains uncommitted.

## Review and limitations

- New files each own one concept: poll observation (42 pure LOC), cancellation scenarios (79), owned WS/PTY fixture (114). Existing shared server (2770) and security socket tests (677) exceed the normal size ceiling; task's exact-block/no-whole-file-refactor ownership restriction takes precedence. No broad extraction attempted across concurrent lanes.
- Existing typed protocol parsing stays at the socket boundary; no unknown data is passed into the input core. Cancellation variants are exhaustively matched. No new production unsafe, casts, unwraps, suppressions, logging, defensive helper layers, or parameter bags.
- Fixture's one libc write uses live bounded bytes and retained PTY ownership with SAFETY comment. It exercises real OS PTY/WS operations, not Miri-compatible emulation. Miri installation was observed, but Miri/sanitizer verification was not run; no UB-clean claim.
- Linux and Windows real saturated WS behavior remains unverified by this task. No platform-wide completion claim.
