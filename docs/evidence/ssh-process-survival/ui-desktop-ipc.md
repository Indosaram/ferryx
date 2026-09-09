# Desktop IPC remote input and command registration

Status: verified with deterministic unit tests and compiler checks.

## Registered commands in `src-tauri/src/lib.rs`

- `cmd_terminal_remote_status { sessionId }`: returns remote connection state and descriptor snapshot.
- `cmd_terminal_remote_retry { sessionId }`: triggers remote reattachment for stored TargetRef.
- `cmd_terminal_remote_write { sessionId, generation, data }`: generation-gated remote write.
- `cmd_terminal_remote_resize { sessionId, generation, cols, rows }`: generation-gated remote resize.
- `cmd_ssh_install_project_helper { workspaceId, localBinary }`: installs the remote helper binary to the host.

## Native input generation gating

`cmd_native_terminal_send_input`, `cmd_native_terminal_paste`, `cmd_native_terminal_mouse`, and `cmd_native_terminal_scroll` now accept `generation: Option<u64>`.

- For local sessions, `generation` is ignored and synchronous write is performed.
- For remote sessions:
  - If `generation` is `Some(gen)`, `daemon_client.write_terminal_at_generation` routes to `RemoteWrite` with that generation.
  - If `generation` is `None` or matches an older generation during reconnect, the input is rejected with a typed error and never queued or replayed.
  - Remote control is not queued during busy states.

## Verification

- `ipc::native_terminal::tests::ssh_reconnect_safety_desktop_native_command_generation_contract`: passed.
- `daemon::client::tests::ssh_reconnect_safety_desktop_remote_control_is_not_queued`: passed.
- `daemon::client::tests::ssh_reconnect_safety_desktop_missing_generation_never_uses_legacy_write`: passed.
- `daemon::client::tests::ssh_reconnect_safety_desktop_encoded_input_retains_generation_and_typed_failure`: passed.
- Native input boundary regressions: 15 passed.
