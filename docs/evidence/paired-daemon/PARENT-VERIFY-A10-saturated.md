# Parent verification: A10 saturated socket controller isolation

Child `st_01a09922` reported completion. The parent verified it independently
before accepting; the child's summary alone was not used as evidence.

## Hash integrity (no drift since the child's run)

`shasum -a 256 -c docs/evidence/paired-daemon/A10-saturated-source.sha256`
executed from the repo root exits 0. All ten entries OK:
`remote/server.rs`, `remote/security_socket_tests.rs`,
`remote/machine_input_probe.rs`, `tests/support/machine_input_cancellation.rs`,
`tests/support/machine_input_fixture.rs`, plus the RED/FINAL/controller/related/
headless logs. The files on disk are exactly what was tested.

## Failing-first evidence is real

- `A10-saturated-RED.log`: `test result: FAILED. 2 passed; 3 failed` with real
  panics in `input_is_cancelled_when_full_queue_precedes_close`,
  `input_is_cancelled_when_generation_is_replaced`, and
  `sibling_pty_is_responsive_when_first_input_is_saturated`.
- `A10-saturated-FINAL.log`: `test result: ok. 5 passed; 0 failed`.
- `A10-saturated-controller-FINAL.log`: `ok. 3 passed`.
- `A10-saturated-related-FINAL.log`: `ok. 1 passed` (machine_terminal_stream) and
  `ok. 3 passed` (pty_input_cancellation).
- `A10-saturated-headless-check.log`: `Finished dev profile` for the headless
  CLI/relay binaries.

## Source review (parent read the code, not the summary)

- `src/remote/server.rs:1288-1290` declares `mod machine_input_probe` under
  `#[cfg(test)]`, and `../../tests/support/machine_input_cancellation.rs` under
  `#[cfg(all(test, unix))]`.
- Both probe call sites are individually `#[cfg(test)]` gated
  (`server.rs:1383-1384` queue-full marker, `server.rs:1407-1409` pending-write
  observation). No test instrumentation compiles into shipping builds, which the
  passing headless binary check corroborates.
- The receive loop takes `machine_controllers.lock()`, re-checks device,
  generation and `disconnected`, subscribes to the `cancelled` authority, then
  **drops the guard before awaiting** `write_input_cancellable`. This is the
  claimed repair: per-poll authority instead of a controller lock held across
  pending PTY I/O, with `validate_machine_target` still enforced per message.
- The send loop keeps one bounded in-flight frame plus a live lookahead read so
  `Close`/EOF cancels a saturated writer; excess input ends the socket instead of
  being buffered for replay after reconnect.

## Verdict

A10 is accepted as **scoped complete**. This is not full-plan acceptance.
Still unverified for this packet: Linux, Windows, sanitizer/Miri execution.
Changes remain uncommitted in the isolated continuation tree.
