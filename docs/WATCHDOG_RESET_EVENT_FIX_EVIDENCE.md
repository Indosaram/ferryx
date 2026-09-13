# Watchdog reset command/event fix evidence

Task st_01a09b1f; 2026-09-13.

## Scope and mechanism

Production change: `src-tauri/src/ipc/agents.rs` awaits successful daemon reset with `?` before native mutation/emission, then returns `Ok(())`. The only other code is its test module declaration and `src-tauri/src/ipc/agents_reset_event_tests.rs`. No frontend or logging implementation was edited by this task.

Tests invoke the actual generic Tauri command with managed DaemonClient and NativeTerminalSurfaceHostState, a subscribed Tauri native event observer, and real native attachment pump. The wire fixture validates handshake/reset requests and withholds its response until the request observer runs. It never instantiates a daemon server or accesses/signals the live daemon. Unique socket directories are inside this worktree and creation/removal uses run_blocking. No home/data directory is needed, no environment variables are changed, and matching daemon version prevents upgrade. No sleeps or polling delays: oneshot request/reply barriers and a native snapshot event bound completion. Duplicate Working replay verifies retained native activity via the production deduplication behavior; success must emit one manual-reset Idle and make Working a transition again.

## Diagnostics

## Independent supervisor GREEN

After the logging owner corrected the compilation errors, the supervisor ran:

```sh
cd /Users/indo/code/project/orca-lite-wt/sa-watchdog
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::agents::reset_event_tests -- --nocapture
```

Monitor `mon_9EGX485GYSBW3A90`, command session `bash_315`, completed exit 0.
The full unfiltered session output contains 17 existing warnings, with no
compilation error. Its decisive output is:

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.46s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/ferryx_lib-8e70f89d07f5770a)

running 2 tests
test ipc::agents::reset_event_tests::success_resets_native_activity_and_emits_idle ... ok
test ipc::agents::reset_event_tests::rejection_preserves_native_activity_and_emits_no_idle ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 1046 filtered out; finished in 0.15s

RESET_EVENT_LEAD_EXIT=0
```

This closes P1's focused GREEN gap. Logging lifecycle remediation and the final
combined review remain separate work; this is not a full integration verdict.

## Historical child completion boundary

Behavioral RED is established (both command/event cases fail for premature native emission), but GREEN and build remain blocked by concurrent, unowned `src-tauri/src/daemon/logging.rs` errors E0277 at lines 36 and 70. `IpcError` does not implement `StdError`, so the two `?` conversions into `anyhow::Error` fail. This task did not alter that file or suppress errors. The parent/logging owner must resolve those errors, then run the focused command below and append its complete output before considering this fix verified. No full unrelated suites, GUI automation, daemon launches, signals, or commits were performed. The patch remains uncommitted in the shared worktree.

Remaining focused validator: `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::agents::reset_event_tests -- --nocapture`.

Exact authored delivery scope: `src-tauri/src/ipc/agents.rs` (6 insertions, 1 deletion), new `src-tauri/src/ipc/agents_reset_event_tests.rs` (246 lines), and this evidence file. Earlier dirty frontend repairs and the other child's logging work are preserved. Temporary standalone test and raw transcript files were removed after embedding their full output here.

LSP on agents.rs: only inactive-code hint at line 607 (unix configuration). LSP on agents_reset_event_tests.rs: no diagnostics. Initial integration test diagnostics also reported none, but Cargo correctly rejected its access to crate-private run_blocking; the fixture was moved into a unit module without changing that visibility.

`rustfmt --edition 2021 --check src-tauri/src/ipc/agents_reset_event_tests.rs`: exit 0, no output.
`git diff --check`: exit 0, no output.
`find . -maxdepth 1 -name "reset-*"`: exit 0, no output after RED fixture cleanup.

## Command transcripts

All commands ran from `/Users/indo/code/project/orca-lite-wt/sa-watchdog`. Output is complete, including existing compiler warnings. The initial compile failure and zero-test harness attempt are NOT claimed as behavioral RED. Two apply_patch limitations (unsupported Move/Delete directives) required relocating the test via Add File and removing only the temporary file authored by this task.

### Initial harness compile (not behavioral RED)

Command: `cargo test --manifest-path src-tauri/Cargo.toml --test watchdog_reset_event_contract -- --nocapture`

```text
   Compiling ferryx v2026.908.1 (/Users/indo/code/project/orca-lite-wt/sa-watchdog/src-tauri)
warning: unused import: `Manager`
  --> src/ipc/notifications.rs:21:24
   |
21 | use tauri::{AppHandle, Manager, Runtime, State};
   |                        ^^^^^^^
   |
   = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unnecessary `unsafe` block
  --> src/macos_file_drop.rs:79:28
   |
79 |             let location = unsafe { info.draggingLocation() };
   |                            ^^^^^^ unnecessary `unsafe` block
   |
   = note: `#[warn(unused_unsafe)]` (part of `#[warn(unused)]`) on by default

warning: unnecessary `unsafe` block
  --> src/macos_file_drop.rs:80:25
   |
80 |             let point = unsafe { self.convertPoint_fromView(location, None) };
   |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:102:46
    |
102 |     let pasteboard: Retained<NSPasteboard> = unsafe { info.draggingPasteboard() };
    |                                              ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:105:25
    |
105 |     if let Some(list) = unsafe { pasteboard.propertyListForType(&legacy) } {
    |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:122:25
    |
122 |     if let Some(text) = unsafe { pasteboard.stringForType(file_url_type) } {
    |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:123:28
    |
123 |         if let Some(url) = unsafe { NSURL::URLWithString(&text) } {
    |                            ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:124:33
    |
124 |             if let Some(path) = unsafe { url.path() } {
    |                                 ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:169:5
    |
169 |     unsafe { view.registerForDraggedTypes(&types) };
    |     ^^^^^^ unnecessary `unsafe` block

warning: variable does not need to be mutable
   --> src/native_terminal/renderer/font_manager.rs:155:13
    |
155 |         let mut buffer = vec![0u8; total_pixels];
    |             ----^^^^^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

warning: field `app` is never read
  --> src/ipc/notifications.rs:28:5
   |
27 | pub struct TauriNotificationBackend<R: Runtime> {
   |            ------------------------ field in this struct
28 |     app: AppHandle<R>,
   |     ^^^
   |
   = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: method `register_pairing_capability` is never used
   --> src/remote/auth.rs:396:19
    |
317 | impl AuthManager {
    | ---------------- method in this implementation
...
396 |     pub(crate) fn register_pairing_capability(&self, token: &str) {
    |                   ^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: method `wait_and_reap` is never used
   --> src/terminal/session.rs:285:19
    |
 61 | impl PtySession {
    | --------------- method in this implementation
...
285 |     pub(crate) fn wait_and_reap(&self) -> Result<Option<i32>, PtyError> {
    |                   ^^^^^^^^^^^^^

warning: struct `WriterLeaseGuard` is never constructed
  --> src/worktree/manager.rs:81:19
   |
81 | pub(crate) struct WriterLeaseGuard {
   |                   ^^^^^^^^^^^^^^^^

warning: associated items `new`, `canonical_path`, and `owner_id` are never used
   --> src/worktree/manager.rs:88:8
    |
 87 | impl WriterLeaseGuard {
    | --------------------- associated items in this implementation
 88 |     fn new(registry: WriterLeaseRegistry, canonical_path: PathBuf, owner_id: String) -> Self {
    |        ^^^
...
 96 |     pub(crate) fn canonical_path(&self) -> &Path {
    |                   ^^^^^^^^^^^^^^
...
100 |     pub(crate) fn owner_id(&self) -> &str {
    |                   ^^^^^^^^

warning: method `acquire_writer_lease` is never used
   --> src/worktree/manager.rs:333:19
    |
129 | impl WorktreeManager {
    | -------------------- method in this implementation
...
333 |     pub(crate) fn acquire_writer_lease(
    |                   ^^^^^^^^^^^^^^^^^^^^

warning: `ferryx` (lib) generated 16 warnings (run `cargo fix --lib -p ferryx` to apply 2 suggestions)
error[E0603]: function `run_blocking` is private
  --> tests/watchdog_reset_event_contract.rs:7:46
   |
 7 | use ferryx_lib::ipc::{cmd_agent_state_reset, run_blocking, IpcErrorCode};
   |                                              ^^^^^^^^^^^^ private function
   |
note: the function `run_blocking` is defined here
  --> /Users/indo/code/project/orca-lite-wt/sa-watchdog/src-tauri/src/ipc/mod.rs:26:1
   |
26 | / pub(crate) async fn run_blocking<T, F>(operation: F) -> Result<T, error::IpcError>
27 | | where
28 | |     T: Send + 'static,
29 | |     F: FnOnce() -> Result<T, error::IpcError> + Send + 'static,
   | |_______________________________________________________________^

error[E0282]: type annotations needed
  --> tests/watchdog_reset_event_contract.rs:17:21
   |
17 |       let directory = run_blocking(|| {
   |  _____________________^
18 | |         Ok(tempfile::Builder::new()
19 | |             .prefix("reset-")
20 | |             .tempdir_in(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
21 | |             .expect("local fixture"))
22 | |     })
23 | |     .await
   | |__________^ cannot infer type

error[E0282]: type annotations needed
   --> tests/watchdog_reset_event_contract.rs:194:5
    |
194 | /     run_blocking(move || {
195 | |         directory.close().expect("remove local fixture");
196 | |         Ok(())
197 | |     })
198 | |     .await
    | |__________^ cannot infer type

Some errors have detailed explanations: E0282, E0603.
For more information about an error, try `rustc --explain E0282`.
error: could not compile `ferryx` (test "watchdog_reset_event_contract") due to 3 previous errors
warning: build failed, waiting for other jobs to finish...

EXIT_CODE=101
```

### Unwired module attempt (zero tests; not RED)

Command: `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::agents::reset_event_tests -- --nocapture`

```text
warning: unnecessary `unsafe` block
  --> src/macos_file_drop.rs:79:28
   |
79 |             let location = unsafe { info.draggingLocation() };
   |                            ^^^^^^ unnecessary `unsafe` block
   |
   = note: `#[warn(unused_unsafe)]` (part of `#[warn(unused)]`) on by default

warning: unnecessary `unsafe` block
  --> src/macos_file_drop.rs:80:25
   |
80 |             let point = unsafe { self.convertPoint_fromView(location, None) };
   |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:102:46
    |
102 |     let pasteboard: Retained<NSPasteboard> = unsafe { info.draggingPasteboard() };
    |                                              ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:105:25
    |
105 |     if let Some(list) = unsafe { pasteboard.propertyListForType(&legacy) } {
    |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:122:25
    |
122 |     if let Some(text) = unsafe { pasteboard.stringForType(file_url_type) } {
    |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:123:28
    |
123 |         if let Some(url) = unsafe { NSURL::URLWithString(&text) } {
    |                            ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:124:33
    |
124 |             if let Some(path) = unsafe { url.path() } {
    |                                 ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:169:5
    |
169 |     unsafe { view.registerForDraggedTypes(&types) };
    |     ^^^^^^ unnecessary `unsafe` block

warning: unused variable: `super_key`
   --> src/native_terminal/input.rs:366:13
    |
366 |         let super_key = KeyModifiers {
    |             ^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_super_key`
    |
    = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: variable does not need to be mutable
   --> src/native_terminal/renderer/font_manager.rs:155:13
    |
155 |         let mut buffer = vec![0u8; total_pixels];
    |             ----^^^^^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `token`
    --> src/remote/auth.rs:1340:14
     |
1340 |         let (token, device) = auth
     |              ^^^^^ help: if this is intentional, prefix it with an underscore: `_token`

warning: field `app` is never read
  --> src/ipc/notifications.rs:28:5
   |
27 | pub struct TauriNotificationBackend<R: Runtime> {
   |            ------------------------ field in this struct
28 |     app: AppHandle<R>,
   |     ^^^
   |
   = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: function `no_auth_query` is never used
    --> src/remote/server.rs:2622:12
     |
2622 |         fn no_auth_query() -> AuthQuery {
     |            ^^^^^^^^^^^^^

warning: method `wait_and_reap` is never used
   --> src/terminal/session.rs:285:19
    |
 61 | impl PtySession {
    | --------------- method in this implementation
...
285 |     pub(crate) fn wait_and_reap(&self) -> Result<Option<i32>, PtyError> {
    |                   ^^^^^^^^^^^^^

warning: struct `WriterLeaseGuard` is never constructed
  --> src/worktree/manager.rs:81:19
   |
81 | pub(crate) struct WriterLeaseGuard {
   |                   ^^^^^^^^^^^^^^^^

warning: associated items `new`, `canonical_path`, and `owner_id` are never used
   --> src/worktree/manager.rs:88:8
    |
 87 | impl WriterLeaseGuard {
    | --------------------- associated items in this implementation
 88 |     fn new(registry: WriterLeaseRegistry, canonical_path: PathBuf, owner_id: String) -> Self {
    |        ^^^
...
 96 |     pub(crate) fn canonical_path(&self) -> &Path {
    |                   ^^^^^^^^^^^^^^
...
100 |     pub(crate) fn owner_id(&self) -> &str {
    |                   ^^^^^^^^

warning: method `acquire_writer_lease` is never used
   --> src/worktree/manager.rs:333:19
    |
129 | impl WorktreeManager {
    | -------------------- method in this implementation
...
333 |     pub(crate) fn acquire_writer_lease(
    |                   ^^^^^^^^^^^^^^^^^^^^

warning: `ferryx` (lib test) generated 17 warnings (run `cargo fix --lib -p ferryx --tests` to apply 3 suggestions)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.53s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/ferryx_lib-8e70f89d07f5770a)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1045 filtered out; finished in 0.00s


EXIT_CODE=0
```

### Behavioral RED before production edit

Command: `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::agents::reset_event_tests -- --nocapture`

```text
   Compiling ferryx v2026.908.1 (/Users/indo/code/project/orca-lite-wt/sa-watchdog/src-tauri)
warning: unnecessary `unsafe` block
  --> src/macos_file_drop.rs:79:28
   |
79 |             let location = unsafe { info.draggingLocation() };
   |                            ^^^^^^ unnecessary `unsafe` block
   |
   = note: `#[warn(unused_unsafe)]` (part of `#[warn(unused)]`) on by default

warning: unnecessary `unsafe` block
  --> src/macos_file_drop.rs:80:25
   |
80 |             let point = unsafe { self.convertPoint_fromView(location, None) };
   |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:102:46
    |
102 |     let pasteboard: Retained<NSPasteboard> = unsafe { info.draggingPasteboard() };
    |                                              ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:105:25
    |
105 |     if let Some(list) = unsafe { pasteboard.propertyListForType(&legacy) } {
    |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:122:25
    |
122 |     if let Some(text) = unsafe { pasteboard.stringForType(file_url_type) } {
    |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:123:28
    |
123 |         if let Some(url) = unsafe { NSURL::URLWithString(&text) } {
    |                            ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:124:33
    |
124 |             if let Some(path) = unsafe { url.path() } {
    |                                 ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:169:5
    |
169 |     unsafe { view.registerForDraggedTypes(&types) };
    |     ^^^^^^ unnecessary `unsafe` block

warning: unused variable: `super_key`
   --> src/native_terminal/input.rs:366:13
    |
366 |         let super_key = KeyModifiers {
    |             ^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_super_key`
    |
    = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: variable does not need to be mutable
   --> src/native_terminal/renderer/font_manager.rs:155:13
    |
155 |         let mut buffer = vec![0u8; total_pixels];
    |             ----^^^^^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `token`
    --> src/remote/auth.rs:1340:14
     |
1340 |         let (token, device) = auth
     |              ^^^^^ help: if this is intentional, prefix it with an underscore: `_token`

warning: field `app` is never read
  --> src/ipc/notifications.rs:28:5
   |
27 | pub struct TauriNotificationBackend<R: Runtime> {
   |            ------------------------ field in this struct
28 |     app: AppHandle<R>,
   |     ^^^
   |
   = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: function `no_auth_query` is never used
    --> src/remote/server.rs:2622:12
     |
2622 |         fn no_auth_query() -> AuthQuery {
     |            ^^^^^^^^^^^^^

warning: method `wait_and_reap` is never used
   --> src/terminal/session.rs:285:19
    |
 61 | impl PtySession {
    | --------------- method in this implementation
...
285 |     pub(crate) fn wait_and_reap(&self) -> Result<Option<i32>, PtyError> {
    |                   ^^^^^^^^^^^^^

warning: struct `WriterLeaseGuard` is never constructed
  --> src/worktree/manager.rs:81:19
   |
81 | pub(crate) struct WriterLeaseGuard {
   |                   ^^^^^^^^^^^^^^^^

warning: associated items `new`, `canonical_path`, and `owner_id` are never used
   --> src/worktree/manager.rs:88:8
    |
 87 | impl WriterLeaseGuard {
    | --------------------- associated items in this implementation
 88 |     fn new(registry: WriterLeaseRegistry, canonical_path: PathBuf, owner_id: String) -> Self {
    |        ^^^
...
 96 |     pub(crate) fn canonical_path(&self) -> &Path {
    |                   ^^^^^^^^^^^^^^
...
100 |     pub(crate) fn owner_id(&self) -> &str {
    |                   ^^^^^^^^

warning: method `acquire_writer_lease` is never used
   --> src/worktree/manager.rs:333:19
    |
129 | impl WorktreeManager {
    | -------------------- method in this implementation
...
333 |     pub(crate) fn acquire_writer_lease(
    |                   ^^^^^^^^^^^^^^^^^^^^

warning: `ferryx` (lib test) generated 17 warnings (run `cargo fix --lib -p ferryx --tests` to apply 3 suggestions)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 17.36s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/ferryx_lib-8e70f89d07f5770a)

running 2 tests

thread 'ipc::agents::reset_event_tests::success_resets_native_activity_and_emits_idle' (4915457) panicked at src/ipc/agents_reset_event_tests.rs:232:5:
native reset must await daemon success
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

thread 'ipc::agents::reset_event_tests::rejection_preserves_native_activity_and_emits_no_idle' (4915456) panicked at src/ipc/agents_reset_event_tests.rs:222:9:
rejection must emit no native reset event: [Object {"manifestId": String(""), "ruleId": String("manual-reset"), "sessionId": String("reset-session"), "state": String("idle")}]
test ipc::agents::reset_event_tests::success_resets_native_activity_and_emits_idle ... FAILED
test ipc::agents::reset_event_tests::rejection_preserves_native_activity_and_emits_no_idle ... FAILED

failures:

failures:
    ipc::agents::reset_event_tests::rejection_preserves_native_activity_and_emits_no_idle
    ipc::agents::reset_event_tests::success_resets_native_activity_and_emits_idle

test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 1045 filtered out; finished in 0.05s

error: test failed, to rerun pass `--lib`

EXIT_CODE=101
```

### First GREEN attempt blocked by concurrent logging compile errors

Command: `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::agents::reset_event_tests -- --nocapture`

```text
   Compiling ferryx v2026.908.1 (/Users/indo/code/project/orca-lite-wt/sa-watchdog/src-tauri)
error[E0277]: `?` couldn't convert the error: `ipc::error::IpcError: StdError` is not satisfied
  --> src/daemon/logging.rs:36:17
   |
34 |           let file = crate::ipc::run_blocking(|| {
   |  ____________________-
35 | |             open_log().map_err(|e| crate::ipc::error::IpcError::internal(format!("daemon log initialization: {e}")))
36 | |         }).await?;
   | |                -^ the trait `StdError` is not implemented for `ipc::error::IpcError`
   | |________________|
   |                  this has type `Result<_, ipc::error::IpcError>`
   |
note: `ipc::error::IpcError` needs to implement `StdError`
  --> src/ipc/error.rs:63:1
   |
63 | pub struct IpcError {
   | ^^^^^^^^^^^^^^^^^^^
   = note: the question mark operation (`?`) implicitly performs a conversion on the error value using the `From` trait
   = note: required for `anyhow::Error` to implement `std::convert::From<ipc::error::IpcError>`

error[E0277]: `?` couldn't convert the error: `ipc::error::IpcError: StdError` is not satisfied
  --> src/daemon/logging.rs:70:25
   |
70 |         self.task.await??;
   |         ----------------^ the trait `StdError` is not implemented for `ipc::error::IpcError`
   |         |
   |         this has type `Result<_, ipc::error::IpcError>`
   |
note: `ipc::error::IpcError` needs to implement `StdError`
  --> src/ipc/error.rs:63:1
   |
63 | pub struct IpcError {
   | ^^^^^^^^^^^^^^^^^^^
   = note: the question mark operation (`?`) implicitly performs a conversion on the error value using the `From` trait
   = note: required for `anyhow::Error` to implement `std::convert::From<ipc::error::IpcError>`

warning: unnecessary `unsafe` block
  --> src/macos_file_drop.rs:79:28
   |
79 |             let location = unsafe { info.draggingLocation() };
   |                            ^^^^^^ unnecessary `unsafe` block
   |
   = note: `#[warn(unused_unsafe)]` (part of `#[warn(unused)]`) on by default

warning: unnecessary `unsafe` block
  --> src/macos_file_drop.rs:80:25
   |
80 |             let point = unsafe { self.convertPoint_fromView(location, None) };
   |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:102:46
    |
102 |     let pasteboard: Retained<NSPasteboard> = unsafe { info.draggingPasteboard() };
    |                                              ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:105:25
    |
105 |     if let Some(list) = unsafe { pasteboard.propertyListForType(&legacy) } {
    |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:122:25
    |
122 |     if let Some(text) = unsafe { pasteboard.stringForType(file_url_type) } {
    |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:123:28
    |
123 |         if let Some(url) = unsafe { NSURL::URLWithString(&text) } {
    |                            ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:124:33
    |
124 |             if let Some(path) = unsafe { url.path() } {
    |                                 ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:169:5
    |
169 |     unsafe { view.registerForDraggedTypes(&types) };
    |     ^^^^^^ unnecessary `unsafe` block

warning: unused variable: `super_key`
   --> src/native_terminal/input.rs:366:13
    |
366 |         let super_key = KeyModifiers {
    |             ^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_super_key`
    |
    = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: variable does not need to be mutable
   --> src/native_terminal/renderer/font_manager.rs:155:13
    |
155 |         let mut buffer = vec![0u8; total_pixels];
    |             ----^^^^^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `token`
    --> src/remote/auth.rs:1340:14
     |
1340 |         let (token, device) = auth
     |              ^^^^^ help: if this is intentional, prefix it with an underscore: `_token`

For more information about this error, try `rustc --explain E0277`.
warning: `ferryx` (lib test) generated 11 warnings
error: could not compile `ferryx` (lib test) due to 2 previous errors; 11 warnings emitted

EXIT_CODE=101
```
