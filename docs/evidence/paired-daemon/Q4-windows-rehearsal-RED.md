# Q4 Windows frozen-backend rehearsal — RED (failing-first) record

The pre-fix `build.out` was overwritten by the successful green rerun on the remote
host, so this file preserves the original compiler error verbatim, captured from the
failing run in this session (macOS-side relay of `Select-String` over
`C:\Users\sook\ferryx-winbuild\r9-win-build.out`, before the fix).

## Original defect

```
error: future cannot be sent between threads safely
    --> src\remote\server.rs:1271:10
     |
1271 |         .on_upgrade(move |socket| async move {
     |          ^^^^^^^^^^ future created by async block is not `Send`
     |
     = help: within `{async block@src\remote\server.rs:1271:35: 1271:45}`, the trait
       `std::marker::Send` is not implemented for `*mut ()`
note: future is not `Send` as this value is used across an await
    --> src\terminal\session.rs:264:82
     |
 258 |                 let writer=self.writer.try_lock().ok_or_else(|| PtyError::IoError("PTY_INPUT_BUSY".into()))?;
     |                     ------ has type `parking_lot::lock_api::MutexGuard<'_, parking_lot::RawMutex, std::option::Option<Box<dyn std::io::...`
...
 264 |                 if n==0 {tokio::time::sleep(std::time::Duration::from_millis(1)).await;} else {bytes=&bytes[n..];}
     |                                                                                  ^^^^^ await occurs here, with `writer` maybe used later
note: required by a bound in `WebSocketUpgrade::<F>::on_upgrade`
    --> axum-0.8.9\src\extract\ws.rs:350:36
     |
 350 |         Fut: Future<Output = ()> + Send + 'static,
```

The `#[cfg(windows)]` `write_input_cancellable` pump is Windows-only, so the macOS
suite never compiled it — this defect was invisible to every macOS-only verification
and was surfaced by the frozen-backend Windows rehearsal.

## Fix

`src-tauri/src/terminal/session.rs`: the per-iteration lock acquisition moved into a
plain synchronous helper `write_input_slice()`; all guards are acquired and released
inside that ordinary frame, so nothing non-Send is live across the caller's
`.await`. Semantics preserved: same guard order, same state checks, same error codes,
same 1 ms backpressure sleep.

## Green

`Q4-windows-build.out` (post-fix, `BUILD_EXIT=0`) and `Q4-windows-test.out`
(`33 passed; 0 failed` for the `paired_host::` suite; the remaining 3 of 36 are
cfg-gated out on Windows) plus `Q4-windows-rehearsal.log`.
