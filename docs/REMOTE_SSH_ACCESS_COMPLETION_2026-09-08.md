# Ferryx Remote persistence and SSH access

## Implemented behavior

- The Remote event WebSocket reconnects with bounded exponential backoff.
  Each connection refreshes workspace state, including a cleared desktop
  selection. Unmounting or clearing the pairing token stops retries. Browser
  online/visibility events trigger recovery without clearing pairing.
- The desktop supplies its actual, profile-aware SSH store path to the daemon
  through the existing active-selection message. The added field is optional
  on the daemon wire; old senders remain readable.
- The gateway includes registered, enabled SSH projects in its project list.
  Listing reads local saved identities; it does not open SSH connections.
  SSH host addresses, credentials, and absolute project paths are not returned.
- Daemon-owned terminal routing retains the workspace ID, including SSH
  identities. Running SSH terminals are listed even when another project is
  active on the desktop.
- The web picker lists each SSH terminal separately and sends its backend
  session ID. The desktop resolves that ID to the existing local session and
  focuses its tab/pane, including across projects, without respawning it.
- Selecting a registered SSH project with no terminal waits for project
  registration/restoration before opening its terminal.
- SSH selections use the SSH identity store rather than bypassing the local
  worktree registry's filesystem jail.
- Selection remains Control-only. WebSocket attachment still requires the
  desktop-confirmed active session. Disabled/deleted SSH targets are rechecked
  before remote describe, attach, input, resize, and signal operations.
- Existing listener restoration, durable pairing tokens, and explicit Disable
  behavior remain in place.

This adds access to registered SSH projects and their terminals. It does not
add an SSH host-registration/settings editor to the web client.

## Changed areas

- `ui/src/remote/RemoteApp.tsx`: event reconnection, resynchronization, session
  selection requests and confirmation.
- `ui/src/remote/RemoteSessionList.tsx`: SSH session options and optimistic
  terminal rendering from an initially empty context.
- `ui/src/App.tsx`: backend-to-frontend session identity resolution and
  registration-aware SSH project selection.
- `src-tauri/src/remote/ssh.rs`: validated SSH project inventory.
- `src-tauri/src/remote/server.rs`: SSH inventory and selection routing.
- `src-tauri/src/remote/state.rs`: desktop-provided SSH store path.
- `src-tauri/src/daemon/proxy.rs`: session workspace ownership and SSH
  authorization checks.
- `src-tauri/src/daemon/{protocol,client,server}.rs` and
  `src-tauri/src/ipc/remote.rs`: trusted desktop/daemon plumbing and ownership
  lifecycle cleanup.

## Verification evidence

Regression failures were observed before the corresponding fixes:

- Event reconnect: one socket remained instead of a replacement connection.
- SSH project inventory: the registered SSH project was absent.
- SSH project selection: HTTP 400 instead of 200.
- SSH session routing: workspace ID was `None`.
- Desktop session selection: the active tab did not change.
- First SSH project selection: no terminal was spawned.
- Actual OpenSSH transport: disabling the host still allowed gateway input.

Successful checks:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib remote:: -- --nocapture
```

89 tests passed.

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib \
  daemon::server::remote_ssh_tests::direct_ssh_real_transport_registration_and_pty \
  -- --exact --nocapture
```

Passed with a real isolated loopback OpenSSH server and daemon-owned SSH PTY.
The added `remote_ssh_gateway_qa.rs` exercise verifies HTTP inventory, rejection
of inactive attachment, session selection events, active attachment, and a
command response received through the real Remote WebSocket. Its output marker
is absent from the echoed input. The test also verifies disabled-host input
rejection.

```sh
cd ui
node node_modules/vitest/vitest.mjs run src/remote src/App.remote.test.tsx \
  src/lib/remoteClient.test.ts src/lib/remoteProject.test.ts --maxWorkers=1
```

133 passed, 1 failed. The failing existing sidebar test is described below.
The three added desktop selection scenarios passed together: same-project
session focus, cross-project existing-session focus, and opening a registered
SSH project without a preexisting terminal.

```sh
bun run --cwd ui build
git diff --check
```

Both passed after the final frontend implementation change.

A separate headless Bun WebView loaded the built Remote client against an
isolated fixture HTTP/WebSocket server. It verified:

- 390-pixel mobile and 1280-pixel desktop layouts had no document-level
  horizontal overflow.
- Selecting the second SSH terminal sent `sessionId: "ssh-two"` and connected
  to that terminal endpoint.
- Closing the event socket produced a replacement connection and workspace
  refresh without deleting the pairing token.
- The desktop-sized picker marked the selected terminal active.

The fixture browser check is not evidence of a physical phone or of the
installed desktop application. The real SSH transport was verified separately
by the Rust test above. No user's desktop was automated or daemon restarted.

## Concurrent workspace blockers

Other sessions were modifying the shared tree during this task. Their changes
were preserved.

- The existing App test named `selects inactive remote roots by workspace
  identity even when a local root has the same path` fails because the expected
  `build SSH` sidebar button is absent. The concurrent sidebar change filters
  empty workspaces through `emptySidebarWorkspaceIds`; this task does not
  modify that filtering or weaken the assertion.
- Default `cargo check` failed in the concurrently modified
  `src-tauri/src/ipc/native_terminal.rs` at lines 568 and 600:
  `subscribe_session_detach` was unavailable and a type could not be inferred.
- `cargo check --no-default-features` also failed in concurrent native-command
  registration: the generated command macros for
  `cmd_native_terminal_set_preedit` were unavailable.
- LSP diagnostics could not run because the LSP daemon was unreachable.
  Compiler/typechecker and test results above are the available evidence.

These failures prevent claiming that the entire shared desktop checkout builds
cleanly. They are outside the Remote/SSH changes made in this task.

## Desktop and phone verification

After the concurrent native build issues are resolved, launch the debug desktop
with exactly `bun tauri dev`. On an already-paired phone:

1. Reload the same Remote address and confirm no PIN is required.
2. Select a registered SSH project and two different SSH terminals, including
   one belonging to a different project. Confirm the intended desktop pane is
   selected and command output comes from the remote host.
3. Briefly disconnect/reconnect the phone network and confirm the project
   selection updates again without re-pairing.

The implementation handoff was verified in a shared working tree. Unrelated
changes from other sessions are excluded from the Remote/SSH commits.
