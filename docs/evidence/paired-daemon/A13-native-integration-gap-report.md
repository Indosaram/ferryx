# A13 native gap closure

Closes the two explicitly unverified surfaces in A13-native-integration-report.md. This is not full A13 UI/native/proxy acceptance.

## Paused authenticated network adoption

`paired_host::service::native_transport_tests::paused_authenticated_response_cannot_adopt_after_repair_or_forget` uses real ephemeral HTTP exchange/capability handlers. Before starting each delayed pairing, an mpsc receiver is subscribed. The authenticated capability request publishes a one-shot release gate and waits with a five-second deadline. Only after that exact request event does the concurrent native operation re-pair the same host; the second iteration re-pairs and forgets it before release. Both delayed operations return PAIRED_HOST_STALE_GENERATION. The forgotten tombstone prevents resurrection; a separately paired host's exact generation/readback is unchanged. Listener refusal and root deletion are checked. No timing sleeps/polling. This transport fixture is deterministic, not an actual relay; actual relay pairing is covered by the process fixture and existing roundtrip.

## Separate daemon processes

`daemon::server::paired_host_process_tests::paired_host_separate_process_restart` executes the Rust test binary with one exact child-test filter; it never executes the GUI binary. Child env is cleared before initialization, with explicit private HOME/FERRYX/XDG/TMP paths and only Cargo dynamic-loader seams preserved. Each child constructs the actual DaemonServer and serves actual DaemonClient requests over private UDS. Readiness/shutdown travel over a pre-bound private control UDS, not polling.

- PID 43032 pairs through an actual ephemeral relay and actual remote gateway, writes native private authority, then exits and is reaped.
- PID 43037 constructs a fresh DaemonServer over that private directory, lists and durably reads the exact generation, then authenticates the retained credential through the real relay using idempotent migration. Private record bytes remain identical. It exits and is reaped.
- PID 43042 exercises injected parent-action failure; orderly shutdown and reap still complete.
- Each process's daemon UDS refuses connections after reap. Relay listener refuses connections after shutdown. Root deletion is recorded. All processes are owned test subprocesses; no canonical daemon/credentials or GUI command was used in this continuation.

The fixture source is `src-tauri/src/paired_host/process_tests.rs`, privately included by daemon/server.rs under cfg(all(test, unix)); it requires private server fields and is not a standalone Cargo integration-test crate.

## Results and failed attempts

- gap-tests.log: blocked by concurrent foreign metadata import/method compilation. No native behavior ran.
- gap-green.log: paused network test passed. Process test failed at dynamic loader because env_clear omitted Cargo's Ghostty dylib search path. PID 34802 was reaped; UDS and relay refusal/root cleanup recorded. Fixed narrowly in harness by preserving Cargo loader variables.
- gap-acceptance.log: blocked by foreign issuer fixture using disabled reqwest JSON helpers; parent fixed it. No native behavior ran.
- gap-final.log: 17 passed, 0 failed, 0 ignored; includes the previous 14-test gate and three new test entries (one is the inert exact-filter child entry).
- gap-check.log: headless cargo check exit 0.
- gap-build.log: headless cargo build exit 0.
- LSP on paired_host directory and process_tests.rs returned no diagnostics before tests. Final parallel LSP requests timed out in shared daemon; compiler check/build succeeded.

No production logic changes were needed for the generation or process persistence proofs. Existing generation snapshot/tombstone check rejected stale adoption correctly. Existing warnings remain unchanged. Source/log SHA-256 values accompany this report. Parent owns repeated issuer PIN investigation and final A13 composition.
