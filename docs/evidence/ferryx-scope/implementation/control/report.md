# Control implementation receipt

Status: WORKING. Owned modules are not exported or wired into daemon/App/RemoteApp.

## Integration API

- Export `ferryx_scope::control` from the integrator-owned module root.
- Construct one daemon-owned `Arc<control::service::Service>` using existing `AuthManager`.
- Explicitly `grant_hosts(device_id, hosts)` after pairing; unknown grants deny access. Set `allow_control` from gateway policy and call `notify_policy` after policy changes. Route device revocation through `Service::revoke` so pending wait futures wake and reject revoked tokens.
- `register_backend(host, owner, Epoch, complete, Arc<dyn ControlBackend>)` registers local or SSH owner. SSH lane implements the same `create/execute/read` boxed-future trait. No remote backend is fabricated.
- Feed registered authoritative sessions through `inventory.lock().await.insert(Agent)`. Feed qualified waiting/working/idle/exited reports through `report(target, kind, source)`. Only Provider provenance may emit TaskComplete (`turn.completed`); terminal activity does not manufacture completion. Bind report ingress to authenticated owner/session before calling this trusted API.
- Atomic `subscribe()` returns snapshot plus broadcast receiver under inventory lock. `replay(after)` returns retained events or explicit Gap snapshot; readers must reset on lag. Full identity remains `{hostId,ownerId,epoch,backendSessionId}` throughout. Owner replacement invalidates older epochs.
- `control::router::router(service)` returns an Axum Router without opening a listener. Routes: GET/POST `/api/v1/agents`; POST `/api/v1/agents/{base64url-full-target}/start|prompt|stop`; GET target `/messages` and `/wait`; GET `/api/v1/hosts`; GET `/api/v1/events` retained replay. Existing gateway WS integration must use snapshot/replay and policy notifications; this module does not replace its existing WS endpoint.
- `LocalBackend` uses real `RemoteSessionBackend` (SessionRouter or TerminalService) for exact-session prompt and read. Inject `TaskLauncher` for workspace-authoritative allocation/start/stop. `CommandRunner` was not found in inspected source; allocation is deliberately not synthesized. The launcher must allocate a real durable task/session identity and persist creation dedupe with its task record.
- UI exports `AttentionInbox`, `HostSessionSelection`, `InventoryClientState`, `ControlClient`. No workspaceStore/desktop-selection import. Feed snapshots/deltas, render errors/loading, navigate full target via onSelect. Per-client ack and selection remain independent. HTTP base/token are injected; use same-origin gateway in browser. Wire event subscription in integrator (do not poll `events()`).

## Dependencies

No manifests changed. Uses existing serde/serde_json, tokio(full), futures-util, parking_lot, Axum 0.8, base64. Tests use existing reqwest, portable-pty, tempfile. No dependency installation requested.

## Evidence so far

- `bun run --cwd ui test src/features/ferryx/control`: initial UI RED 2 failed; GREEN 2 passed. Selection increment RED 1 failed/2 passed; GREEN 3 passed. Client validation increment RED 1 failed/4 passed; GREEN 5 passed. Logs `ui-*`, `selection-*`, `client-*`.
- Full `tsc --noEmit -p ui/tsconfig.json` initially exposed replaceAll ES library incompatibility; replaced with global regex substitutions. Subsequent exit 0.
- Cargo initial attempts timed out during shared builds; another attempt failed before collection with disk exhaustion. Subsequent attempt failed in foreign notification UNNotificationSound/setSound and remote attention_inventory fields. These are setup failures, not RED.
- Isolated rustc harness using existing `libferryx_lib.rlib` collected 3 tests: 3 behavioral failures before implementation (waiting count, empty delta receiver, Unsupported versus Forbidden). See `isolated-red.log`. This reuses production modules via path inclusion, not a duplicate implementation.
- Recovered LSP: owned Rust directory 5 files/0 errors; UI TSX directory 4 files/0 errors. Earlier LSP was unavailable; not retried until parent reported recovery.
- Impeccable mechanical detector exit 0 (`design-check.json`). Existing theme/primitives retained. Programming/frontend skill files were not found in bounded installed-skill discovery; root/subtree guidance and installed Impeccable were read.

## Remaining evidence and limitations

Full Cargo validation and loopback real-PTY proof are still running/queued. No desktop, browser screenshot, native masking, CLI, phone or remote SSH runtime proof claimed. No user-visible integrated completion claim.
Owner-epoch receipts currently live in service memory; launcher persistence is required for durable worktree creation. Control/resize lease implementation and streaming WS integration remain integration gaps. Local terminal prompt sends exact bytes, not managed-provider semantic turn submission; managed/SSH backends must implement their own adapter. Host workspace-list API and task-create/start/stop UI controls are not supplied by these selection components.

No user daemon launched; no default runtime adopted; no foreign edits, staging, commits, resets or artifact deletion. Work is uncommitted in a concurrent tree.
