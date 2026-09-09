# Remote WebSocket recovery and ordered gap handling

Status: verified on raw stream and grid stream WebSocket seams.

## Stream recovery contract

`OutputChunk.replay_gap` carries local sequence gap boundaries. `remote/server.rs` previously ignored empty gap chunks; it now emits an explicit reset/gap frame before following recovered output chunks:
- Raw stream: sends `replayGap` frame with `requestedAfterSequence` and `availableFromSequence`.
- Grid stream: triggers terminal reset and viewport bottom-lock before feeding subsequent chunks.

## Outage input safety

In `src-tauri/src/remote/backend.rs`, `write_input` and `resize` query the active remote session details. If the session is remote, they execute `write_input_operation` and `resize_operation` with the current generation, rejecting outage input without queuing or replaying.

## Verification

- `remote::tests::security::sockets::ssh_reconnect_safety_web_raw_status_input_probe`: passed.
- `remote::tests::security::sockets::ssh_reconnect_safety_web_raw_gap_order`: passed.
- `remote::tests::security::sockets::ssh_reconnect_safety_web_grid_status_input_probe`: passed.
- `remote::tests::security::sockets::ssh_reconnect_safety_web_grid_gap_order`: passed.
