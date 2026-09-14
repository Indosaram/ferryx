# Rust remote gateway / pairing review — 2026-09-14

Scope: `src-tauri/src/remote/` (gateway router, socket auth, relay server, pairing coordinator).

### [P1] Terminal WebSocket is accepted and then immediately torn down when the desktop selection has no focused session
- Location: src-tauri/src/remote/server.rs:1550 (text path) and src-tauri/src/remote/server.rs:1941 (grid path)
- Observed: the upgrade gate admits the request when the desktop selection declares no session id — at `server.rs:1333` the `if let Some(declared_active_id) = active.session_id.as_deref()` arm is skipped for `session_id: None`, and only `!is_session_valid` (server.rs:1327) produces a 403. The socket handler then runs the focus watcher, whose first check is `if active_session_rx.borrow().as_deref() != Some(target_session_id.as_str()) { return; }`. The watch channel carries `selection.session_id` verbatim (`state.rs:852: self.active_session_tx.send_replace(session_id);`), so for a selection with `session_id: None` the borrow is `None`, the comparison fails, the future returns instantly, and `tokio::select!` (server.rs:1561, server.rs:1953) completes and drops the socket. A selection with `session_id: None` but a `tabId`/`terminalTabs` is a normal desktop state — `ipc/remote.rs:490` passes `session_id: request.session_id` straight through and only maps the fully empty request to `None`.
- Why it is wrong: the remote client completes the HTTP upgrade (so it sees a successful connection), then the server closes it before sending any frame. The user gets a blank terminal that reconnect-loops for as long as the desktop's selection carries a tab but no session id; the upgrade gate and the focus watcher disagree about what "active" means.
- Minimal fix: make the watcher agree with the gate — when the watch value is `None`, do not treat it as "focus moved away". In both watchers, replace the early `return` with a wait: exit only once the watch value becomes `Some(other_id)` that differs from `target_session_id`.

### [P1] Host-scoped session IDs can never redeem a socket ticket (always 401)
- Location: src-tauri/src/remote/server.rs:1283
- Observed: the handler supports `"<host_id>::<session_id>"` addressing (`server.rs:1276: let session_id = parse_host_scoped_session_id(&requested_session_id)`), then builds the ticket audience from the unwrapped id: `&format!("/api/v1/terminal/{session_id}")`. The ticket was minted against the target the client sent, which is the scoped path — `valid_socket_target` (server.rs:249) explicitly permits `:` in the id, and the shipped client mints and connects with the same string (`ui/src/lib/terminalTransport/remoteTransport.ts:48-49`). `consume_socket_ticket` compares `issued_target != target` (server.rs:301) and returns `None`, so the upgrade fails with `"Missing auth token"`.
- Why it is wrong: any remote client addressing a worktree session by its host-scoped id gets a 401 on every terminal connect — and the ticket has already been consumed, so retrying with it cannot succeed either. The scoped-id path is dead in the ticket flow while appearing supported.
- Minimal fix: compute the ticket target from the original request path before unwrapping: `&format!("/api/v1/terminal/{requested_session_id}")`, and keep the unwrapped `session_id` only for backend lookups.

### [P2] Sockets attached while no desktop selection exists are never revoked on a later focus change
- Location: src-tauri/src/remote/server.rs:1542 (and the identical src-tauri/src/remote/server.rs:1933)
- Observed: `let has_active_selection = state.active_selection.read().is_some();` samples the selection once, at socket-handling time; the watcher then does `if !has_active_selection { std::future::pending::<()>().await; return; }` (server.rs:1546-1548).
- Why it is wrong: a remote client that attaches while the desktop has published no selection (headless start, or between `clear_active_selection` and the next `set_active_selection`) parks on `pending()` forever. When the desktop later focuses a different session, that older socket is never closed and keeps streaming output of a session that is no longer the active desktop session — the invariant `tests.rs:2086` (`test_connected_terminal_websocket_closed_when_active_selection_changes`) asserts for the already-selected case.
- Minimal fix: drop the `has_active_selection` snapshot and drive the watcher purely from `active_session_watch_rx()`: keep waiting on `changed()` and exit when the value becomes `Some(id)` with `id != target_session_id`.

### [P3] A socket ticket is consumed before its target/expiry is validated
- Location: src-tauri/src/remote/server.rs:300
- Observed: `let (token, issued_target, expiry) = state.socket_tickets.lock().remove(ticket)?;` removes the entry first, and only afterwards checks `if issued_target != target || expiry <= unix_now_secs()`.
- Why it is wrong: a single mismatched or slightly-late upgrade attempt destroys an otherwise-valid credential, so the client must mint a fresh ticket (an extra authenticated round trip) instead of retrying. The relay-side twin has the same shape (`relay_server.rs:965`). No security impact — tickets are UUIDs and single-use by design — but it makes the P1 failure above unrecoverable without a re-mint.
- Minimal fix: peek with `get` and validate target/expiry before `remove`, removing only on a successful match (or on expiry, as cleanup).

### [P3] Doc comment advertises a `token` query parameter the code does not accept
- Location: src-tauri/src/remote/server.rs:311
- Observed: ``/// legacy `token` query parameter remain accepted so existing clients keep working`` — but `struct AuthQuery` (server.rs:179-186) has no `token` field and `socket_credential` (server.rs:313-326) only tries the ticket, then the `Authorization` header.
- Why it is wrong: the comment describes a compatibility path that does not exist, so a reader debugging a 401 from an old client looks for a fallback that was removed. The behavior itself (header or ticket only) is correct.
- Minimal fix: delete the "legacy `token` query parameter" sentence from the doc comment.

## Not found (checked)
- Reachable endpoints missing auth: every route in `create_remote_router` (server.rs:2143-2166) except `/api/v1/health` and `/api/v1/pair/exchange` calls `extract_token`/`socket_credential` + `validate_token` as its first step; mutating routes additionally require `DevicePermission::Control` (server.rs:963, 1095, 1141).
- Secrets in logs: no `tracing::*` call in `server.rs`, `relay_server.rs`, `relay_client.rs` or `auth.rs` formats a token, ticket, PIN or pairing code.
- Lock held across `.await`: both candidates are clean — `state.rs:743` releases `snapshot_cache.read()` at the end of its block before the `.await` on line 775, and `relay_client.rs:186` explicitly `drop(current_generation)` at line 201 before the next `.await`.

Summary: P0=0, P1=2, P2=1, P3=2
