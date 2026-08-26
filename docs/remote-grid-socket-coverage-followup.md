# Follow-up: grid socket integration coverage

## Prior attempts and what they ruled out

Two coordinator attempts to write these tests locally FAILED and were reverted. Read this before
retrying: the obstacle is real and reproducible, not a missing detail.

**Symptom (identical in both attempts):** the WebSocket upgrade genuinely succeeds - the server
returns a real `HTTP/1.1 101 Switching Protocols` with a valid `sec-websocket-accept` header - and
then the connection closes with `UnexpectedEof` before a single frame is sent, in ~0.05s.

**Attempt 1** - a non-lane `src-tauri/examples/` binary using `#[tokio::main]`.
**Attempt 2** - `src-tauri/tests/remote_grid_socket_contract.rs` using `#[tokio::test]`, matching the
passing lib test's PTY command (`sleep 30`) and dimensions (93x27) exactly.

Ruled out by direct verification, so do NOT re-investigate these:

- Dropped state: holding `Arc<RemoteGatewayState>`, `TerminalService`, and `TerminalOutputHub` alive
  in the harness struct did not fix it (this WAS a real bug in attempt 1, fixed, not the cause).
- Session resolution: an explicit `service.get_session(&session_id).is_some()` assert passes before
  the socket is opened.
- `RemoteTerminalMirror::new`: constructs and produces `full_frame()` at 80x24, 93x27, 51x17, 20x3,
  including inside a `flavor = "multi_thread"` Tokio worker via `tokio::spawn`.
- PTY lifetime: `cat` exiting on EOF vs a long-lived `sleep 30` makes no difference.
- Tokio runtime flavor: fails under both `#[tokio::main]` (multi-thread) and `#[tokio::test]`
  (current_thread).
- `cfg(test)` divergence: the gateway path in `src/remote/` has none, so the `--lib` and integration
  binaries link the same code.
- Auth/permission: the device is paired with `DevicePermission::Control` and `allow_control: true`;
  the 403 active-selection gate is passed (upgrade returns 101, not 403).

**The decisive contrast:** the same code path passes as
`test_grid_render_attach_sends_full_frame_with_session_dimensions` inside
`cargo test --lib remote` on every run. Whatever differs lives in the out-of-lib test binary
environment and was not identified. Start there, or write these tests inside `src/remote/tests.rs`
(the lib-internal module, which is lane A's file) where the working pattern already lives.

## Dispatching to a Web worker

Requires a ChatGPT browser surface in cmux (`cmux --json list-pane-surfaces` must report at least one
`"type": "browser"` surface); dispatch with
`delegate_to_chatgpt_web --batch-stdin --json --progress-json`.

Everything below the rule is the verbatim worker prompt.

---

FOLLOW-UP ON YOUR COMPLETED WORK (same scope, same repository: /Users/indo/code/project/orca-lite)

Your backend grid mirror implementation is verified and passing: cargo test --lib remote = 36 passed,
cargo test --lib mirror = 6 passed. A coverage audit then found that the WebSocket grid socket has
exactly ONE integration test (test_grid_render_attach_sends_full_frame_with_session_dimensions),
which only covers the attach path. Three behaviors that a real mobile client exercises on every
session have NO integration coverage. Add them.

TASK: Add three integration tests to src-tauri/src/remote/tests.rs covering the grid socket
behaviors listed below. Change ONLY test code unless a test reveals a genuine product bug.

FILES YOU MAY WRITE:
- src-tauri/src/remote/tests.rs (primary)
- src-tauri/src/remote/server.rs and src-tauri/src/remote/mirror.rs ONLY if a new test exposes a real
  defect. If you change either, say so explicitly and explain the defect in your final message.

OUT OF SCOPE: everything under ui/, src-tauri/src/native_terminal/, src-tauri/vendor/,
src-tauri/tauri.conf.json. Another worker owns the frontend.

TESTS TO ADD (reuse the existing raw-TCP WebSocket helper and pairing/session setup already in
tests.rs, exactly as test_grid_render_attach_sends_full_frame_with_session_dimensions does):

1. LIVE OUTPUT PRODUCES A GRID FRAME AFTER ATTACH
   Attach to the active desktop session with ?render=grid, consume the initial full frame, then write
   input that makes the PTY emit output. Assert a further Text frame arrives whose parsed JSON "type"
   is "grid" or "gridDiff" and whose lines contain the emitted text. This proves the 33 ms coalescing
   loop in handle_terminal_grid_socket actually forwards live output rather than only the snapshot.

2. CLIENT RESIZE RETURNS A FULL FRAME WITH THE NEW GEOMETRY
   Attach with ?render=grid, consume the initial frame, send the client control message
   {"type":"resize","cols":<new>,"rows":<new>} as a WebSocket Text frame, and assert a subsequent Text
   frame parses with "type":"grid" (a FULL frame, not gridDiff) and reports exactly the requested cols
   and rows. This is the path that makes the phone viewport authoritative.

3. LEGACY MODE STAYS BINARY AND SENDS NO GRID FRAMES
   Attach to the same session WITHOUT the render=grid query parameter. Assert the frames received are
   Binary and that no Text frame parsing as "type":"grid" or "type":"gridDiff" arrives. This locks the
   backward-compatibility guarantee that an older client sees a byte-identical socket.

HARD REQUIREMENTS:
- Bound every wait with tokio::time::timeout. Do NOT synchronize with fixed sleeps, sleep-then-assert,
  or polling delays: await the exact frame you expect. A test that passes by timing luck is a defect.
  (The existing `command.arg("sleep 30")` calls are the spawned PTY's own long-lived command, which is
  fine and unrelated.)
- Do NOT weaken, delete, rename, or skip any existing test.
- Do NOT weaken the Active Desktop Lock: ws_terminal_handler must still return 403 for a session that
  is not active_selection.session_id, and focus_watcher must still disconnect on desktop focus change.
- Do NOT create a git commit.

VERIFY (run these yourself from /Users/indo/code/project/orca-lite and paste the real output):
- cargo test --manifest-path src-tauri/Cargo.toml --lib remote   -> exit 0, and the output must list
  your three new test names as passing alongside the existing ones (39 tests total expected).
- cargo test --manifest-path src-tauri/Cargo.toml --lib mirror    -> exit 0, still 6 passed.
- Run the remote suite TWICE in a row and confirm identical pass counts both times, to prove the new
  tests are not order- or timing-dependent.

STOP WHEN: both cargo commands exit 0, the remote suite lists your three new tests as passing in two
consecutive runs, and you have reported the exact commands, exit codes, test names, and whether you
changed any non-test file.
