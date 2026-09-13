# A10 combined continuous PTY -> pending upgraded socket -> recovery proof

2026-09-13; st_01a0986b; macOS arm64. This completes the combined scenario missing from A10-output-socket-integration.md. New integration target and test-only transport observer only. No production changes; parent scoped batch files remained untouched during its concurrent run. Existing successful evidence is preserved.

## Exact scenario

`a10_output_live_socket::continuous_pty_overflow_cancels_pending_upgraded_socket_and_reconnects` creates real daemon-owned shells through machine HTTP project/session APIs, authenticates actual HTTP-upgraded machine WebSockets, and records original process identity. The held socket's TCP receive buffer is configured to 1024 bytes before connecting. Its accepted server TCP connection is identified by the client's exact local address, not by acceptance order.

A transparent accepted-IO wrapper forwards every AsyncRead/AsyncWrite operation directly to the actual TcpStream. Only when the real `poll_write` returns Pending does it publish a watch state; Drop publishes a separate completion state. It neither injects Pending nor gates/throttles/waits in the writer. The watch receiver is selected for the exact connection and checked clear before triggering PTY input.

The machine WS sends a real shell command: dd writes 64MiB continuously from /dev/zero, then the same shell emits an executed completion sentinel. There are NO direct hub publishes in this scenario. An already-subscribed legacy raw output receiver concurrently drains the actual PTY output, counts its bytes and waits for the executed sentinel. The held WS is never read during saturation. In parallel the test awaits actual TCP write Pending, then server-side transport Drop, with a five-second watchdog. Neither a message-count guess nor absence of client reads counts as proof of a pending write.

Transport drop must occur before the production ten-second input/write deadlines; no grant revocation, replacement attachment, malformed input or process close occurs during this phase. The shell continues through its full output and sentinel. This isolates byte-overflow cancellation from the ten-second deadline. Controlled-time exact ten-second stalled-write proof remains in the previously passing production writer test; this combined test proves overflow on the actual upgraded route.

Only after observed server Drop and actual publisher completion does the test drain buffered WS frames. Complete terminal frames must decode successfully. Accepted abrupt termination is explicitly limited to EOF/Close, tungstenite ResetWithoutClosingHandshake, and IO ConnectionReset/UnexpectedEof. Every other protocol/transport error panics. The new normal-message helper uses expect on protocol errors, not `.and_then(Result::ok)`. Existing helper in a10_output_socket.rs was not modified during the parent's batch; this new target does not use it.

The sibling socket then executes a shell PID marker and proves its independent real process still progresses. Reconnect uses the same original target with explicit afterSequence=0; attached boundary is parsed, replay binary metadata must carry a gap, and the backend original PID must be unchanged. No new session/create retry occurs.

## Actual execution

Command:

```
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test a10_output_live_socket -- --nocapture
```

Log: A10-output-live-socket.log. First run passed, 1 passed / 0 failed / 3.67s. LSP returned no errors in both new files before execution. No redundant build or existing batch rerun.

```
A10_OUTPUT_LIVE_SOCKET original_pid=5791 sibling_pid=5850 pty_bytes=67108983 actual_tcp_pending=true server_transport_dropped=true before_deadline=true reconnect_cursor=0 reconnect_gap=true
A10_OUTPUT_SOCKET_CLEANUP pid=Some(5850) reaped=true reader_finished=true listener_joined=true root_removed=true
A10_OUTPUT_SOCKET_CLEANUP pid=Some(5791) reaped=true reader_finished=true listener_joined=true root_removed=true
```

The test catches scenario panics and revokes its private device before closing owned sessions, joins its ephemeral listener and removes its fixture root. Both captured PTY sessions reported reaped/reader_finished. An exact PID query afterward returned neither5791 nor5850. No broad kill, foreign process or canonical daemon action occurred. This invocation used fresh /tmp/a10-ws-live.* private HOME/runtime/data/session/XDG/TMPDIR, removed afterward; existing private target; jobs2/debug0/incremental0/RUSTC_WRAPPER empty; explicit /Users/indo/.cargo and /Users/indo/.rustup. Parent supervisor unchanged.

The observed reader completion flag is not a claim that the public close method itself owns/joins the lifecycle watcher's JoinHandle. That existing lifecycle concern remains parent-owned.

## Review / hashes

New scenario owns live socket overflow recovery (146 pure LOC); new support file owns transparent accepted TCP observation (50 pure LOC). No production API/signature edits, unsafe, timing sleeps, detached producer, manual polling delay, prose assertions or swallowed protocol errors. Standard Rust polling in the transparent IO implementation is the exact runtime observation, not a test readiness polling loop. Listener accept failure is explicit. Watch states are per-connection and bounded accepted observations are per-run.

After-source SHA-256:

| Path | SHA-256 |
| --- | --- |
| src-tauri/tests/a10_output_live_socket.rs | b93295df2ecb92dfa8d4515557b0676ca6f18b1bddcb66e4cd69b41b5147faa4 |
| src-tauri/tests/support/a10_observed_listener.rs | 9af47a1c6c51f39648fc50c603c7970f4e1d0d45f7a01483609c052a12b88135 |
| src-tauri/src/remote/server.rs (unchanged this task) | c59fb9f97d404606be024335045c8d60ad8f2429ba1347853f87bc22a746f063 |
| src-tauri/src/remote/machine_output_writer.rs (unchanged this task) | 27a052576b45352747aca46879ab4133af26e0a6e0b95ef677df684ced5fb115 |

No Linux/Windows run or multi-hop total buffering claim. Source remains uncommitted for parent integration.
