# A1 / C1 / C3 bounded implementation contract

Discovery: 2026-09-05; source inspected in the shared dirty working tree, not HEAD.
Only this report was written. No product edits, tests, daemon launches, or live session mutations.
Applicable root/backend/daemon/remote/UI/lib AGENTS were read. User explicitly overrides the old
remote background-attach prohibition, NOT authentication, path privacy, or daemon PTY ownership.
LSP document-symbol lookup failed: daemon unreachable at /Users/indo/.omo/lsp-daemon/v0.1.0/daemon.sock.
Symbols/call sites below were checked by source reads and rg instead; no successful LSP claim.
Paths below are repository-relative; D=src-tauri/src/daemon, R=src-tauri/src/remote, U=ui/src/remote.

## Verified current boundary
- D/protocol.rs:12 uses protocol **3**, despite AGENTS' older v2 description.
  D/protocol.rs:160-176,264-272: ListSessions returns epoch + IDs; DescribeSession returns details.
  D/protocol.rs:87-97: details contain workspace/worktree/CWD, size, running, sequence bounds, NOT activity.
- D/server.rs:1396-1402 dispatches ListSessions to SessionRouter; D/proxy.rs:537-550 unions local
  and legacy IDs, deduplicates, but silently omits failed peers. R/backend.rs:18-23 cannot express
  list failure (Vec, not Result). Global inventory must not turn this into a false complete empty list.
- D/server.rs:870-895 injects SessionRouter into the daemon-owned RemoteGatewayState.
  D/proxy.rs:554-625 routes describe/attach locally or to a legacy peer. Local describe loses workspace
  metadata; D/server.rs:2199-2226 separately reads session_metadata. Reuse that authoritative mapping,
  not path-derived ownership alone. D/proxy.rs:435-455 forwards output/exit, not AgentState.
- D/server.rs:972-1028 accepts working/blocked/idle extension reports and broadcasts them; no retained
  activity snapshot is updated there. D/server.rs:1446-1451 subscribes only when attaching a stream.
  D/protocol.rs:385-395 AgentStateReport has sessionId/state/agent/providerSession, no generation/time.
  Thus an unattached/background agent's last waiting state is not a queryable daemon inventory today.
- R/server.rs:467-505 get_active_running_sessions returns zero or ONE desktop-selected running session;
  :507-573 both /sessions and /workspace/state reuse it. R/server.rs:280-302 ranks blocked/waiting alike,
  but :326-340 rollup reads only selected-context terminal_tabs, not all daemon agents.
- R/server.rs:575-658 select_workspace emits a desktop selection request; it does not remotely select
  independently. :833-847 forbids non-active attach; :984-1011 and :1279-1290 watch focus to disconnect.
  R/server.rs:1420-1447 registers existing HTTP/WS routes; there is no public agent route or spawn route.
- R/auth.rs:9,12-32,82-151: one-use 60-second PIN -> device token, View/Control only, no resource scopes.
  R/server.rs:823-828 validates attach token. :937-969 and :1224-1260 capture Control once for input/signal;
  resize is not gated, including pre-upgrade geometry at :850-857. Global allow_control is a config
  field (R/state.rs:43-56), not checked in these socket handlers. Revocation only removes token/device
  (R/auth.rs:162-175); live socket loops do not reauthorize. Device revoke accepts any valid token
  without ownership/admin check (R/server.rs:754-771). These are real C1/C3 authorization seams.
- src-tauri/src/main.rs:243-269 dispatches browser CLI, --daemon, or GUI; no agent CLI/help dispatch.
  Do NOT run ferryx --help to probe: unknown arguments currently fall through to GUI (:187-199).
- U/RemoteApp.tsx:328-398 waits for desktop selection events; :435-436 selects optimistic/active session.
  U/RemoteSessionList.tsx:202-246 intentionally refuses arbitrary multi-session selection.
  ui/src/lib/remoteClient.ts:93-101 spawnTerminal fabricates a timestamp ID: NOT a reusable spawn API.
  U/RemoteTerminal.tsx:174,361,402 uses real grid WS + resize; preserve its existing rendering contract.
- D/server.rs:125-164 honors FERRYX_RUNTIME_DIR on Unix/Windows; runtime default differs by dev build.
  :430-453 validates Unix ownership/type/0700 parent. R/server.rs:1464-1474 binds every enabled mode
  to 0.0.0.0, not loopback. A private runtime directory alone does NOT isolate remote network exposure.

## Minimal implementation assignment (proposed, not shipped)
1. A1: add a daemon-owned retained agent inventory, keyed by (daemon epoch, backend session ID).
   Record validated reports before broadcasting; resolve registered session membership at ingress.
   Maintain monotonic revision and remove/mark exited on terminal exit; never infer waiting from silence.
   Normalize blocked -> waiting; working -> working; idle -> idle; absent evidence -> unknown.
   Retain rawState/source for interpretation; do not manufacture provider IDs or translate idle to done.
   Aggregate local + legacy inventory with explicit completeness/failures; legacy unsupported activity
   remains unknown, not idle. Add a capability-negotiated request or version bump, not a breaking v3 edit.
2. Proposed camelCase DTOs (one Rust/TS wire contract, safe remote projection):
   SessionRef = { epoch: string, sessionId: string }; // sessionId is backend ID, never leafId/tabId
   AgentInventoryRow = { ref: SessionRef, workspaceId: string|null, worktreeSlug: string|null,
     agentType: string|null, state: 'unknown'|'idle'|'working'|'waiting'|'exited',
     rawState: string|null, source: 'extension'|'unknown', revision: string }.
   AgentInventory = { revision: string, complete: boolean, unavailableOwners: string[],
     agents: AgentInventoryRow[] }; // opaque owner IDs; no socket paths, CWD, transcript paths
   InventoryChanged = { revision: string }; subscribe before snapshot; refetch on revision/gap, no poll.
3. C1 bounded first public surface: GET /api/v1/agents, GET /api/v1/agents/{sessionId},
   POST /api/v1/agents/{sessionId}/input {epoch,dataBase64},
   POST /api/v1/agents/{sessionId}/signal {epoch,signal:'interrupt'}.
   Reuse SessionRouter write_input/signal (D/proxy.rs:628-670), never expose raw DaemonRequest forwarding.
   Structured errors {code,message}; 401 invalid token, 403 denied, 404 missing, 409 stale epoch,
   503 unavailable owner. Do not blindly retry input/signal after ambiguous transport failure.
   Proposed CLI grammar: ferryx agent list|show|input|interrupt, JSON output and nonzero structured errors;
   explicit endpoint + private token-file configuration, implemented before GUI dispatch, real --help.
   These are design names, NOT commands available for execution yet. Spawn/close are not silently added;
   remote agent launch, if required by C1 acceptance, needs a separate approved request/idempotency contract.
4. C3: enumerate authorized running daemon sessions, independent of desktop selection. Browser keeps its
   own selected SessionRef and attaches directly; selection must not emit desktop focus requests.
   Keep existing desktop-follow behavior as an explicit optional mode, not an authorization boundary.
   Replace both focus watchers with session lifecycle/permission watchers; preserve replay-gap recovery
   via recover_remote_terminal_attachment (R/server.rs:135-143). Desktop close/focus cannot end attach.

## Ownership and write scopes
- Daemon owns PTYs, inventory revisions and workspace/session association; clients own only selection.
- Local trusted owner controls pairing/configuration. Agent credentials get explicit workspace/session
  read/input/interrupt scopes, not blanket View/Control inheritance; existing device migration must be explicit.
- Authorize inventory rows, detail, event payloads AND each attach/write against the same resource mapping.
  Unknown workspace/owner is denied for scoped credentials. Guessed IDs never grant access.
- Global allow_control AND Control/specific action grant required for input, signal, and PTY resize.
  Viewport-only mirror scroll is read-local. View attach must not resize the shared PTY.
  Subscribe to revocation/control-policy changes before accepting a socket; terminate live streams on loss.
- Device management: self-revoke permitted; revoke-other/list-all restricted to local owner/admin grant.
  Use bearer headers for HTTP; WS token handling needs redaction/origin policy (wildcard CORS today).
  Recommend one explicit remote resize controller per session; reject conflicts, never silently steal focus.
- Implementation write ownership: daemon owner D/{protocol,server,proxy,client}.rs; gateway owner
  R/{backend,protocol,state,auth,server,tests}.rs; CLI owner main.rs; remote UI owner RemoteApp/SessionList,
  remoteClient.ts and related tests. RemoteTerminal.tsx/tests are foreign-dirty: coordinate before edits.
  Desktop A1 consumer is outside this read assignment; negotiate its inventory adapter, not a second store.

## RED seam and verification contract
- Add focused `control_inventory_contract` tests (new name), using real router + in-memory auth/backend
  or isolated real PTYs; R/tests.rs:513-572 has HTTP/WS helper signatures but reads lack bounded timeout.
  Wrap exact response/event awaits; subscribe before action. No sleeps/polling. Existing grid fixtures use
  `sleep 30` (:1720,1788,1822); do not copy them. No broad remote/daemon test sweep or persistence test.
- RED assertions: background blocked report with zero desktop attachments appears waiting; two workspaces
  remain visible with desktop selection None; failed legacy owner yields incomplete/error, not empty success;
  stale epoch cannot write; unauthorized rows/events hidden; View attach cannot resize; revoked live socket
  closes; independent socket survives focus change; input reaches only selected QA PTY with exact sentinel.
- Use real production router on an explicitly bound loopback port and subscribe for readiness; proposed
  fixture must avoid start_remote_server's wildcard bind. Keep config/auth/session storage and runtime
  endpoints private; no shared daemon auto-start, manifest adoption, extension install, or Shutdown.
- Future focused commands (NOT run; new test filter must exist and select nonzero tests):
  `cargo test --manifest-path src-tauri/Cargo.toml --lib control_inventory_contract`
  `bun run --cwd ui test src/remote/RemoteAttention.test.tsx src/remote/RemoteUI.test.tsx`
  `bun run --cwd ui build`
  Cargo flags checked against cargo test --help; UI commands are declared package scripts (ui/package.json:6-9).

## Isolated real-surface QA (literal commands/actions; NOT executed)
Precondition: implementation supplies an isolated loopback QA instance, private auth/config/runtime,
two disposable sessions in different workspace IDs, known QA-only SessionRefs and explicit ownership.
Its launcher is NOT yet help-verified; do not substitute the shared daemon or run --daemon casually.
Use its reported loopback origin as BASE and QA pairing PIN as PIN; no production token reuse.
Commands below use curl options verified with `curl --help all`; values come from that QA instance:
```sh
curl --silent --show-error "$BASE/api/v1/health"
curl --silent --show-error --request POST "$BASE/api/v1/pair/exchange" --header 'Content-Type: application/json' --data "{\"code\":\"$PIN\",\"deviceName\":\"control-QA\"}"
curl --silent --show-error "$BASE/api/v1/agents" --header "Authorization: Bearer $TOKEN"
curl --silent --show-error "$BASE/api/v1/sessions" --header "Authorization: Bearer $TOKEN"
curl --silent --show-error "$BASE/api/v1/agents" --header 'Authorization: Bearer invalid-QA-token'
```
Take TOKEN only from QA exchange; redact responses/screenshots/logs containing credentials.
In browser at BASE: pair View and Control separately; pick QA background session, observe retained waiting
row; attach, then change/close QA desktop window. Session must stay attached with identical backend ID.
Before typing, subscribe/capture output; type `printf 'CONTROL_QA_SENTINEL\n'` only in disposable QA shell;
assert exact output there and no input/output change in other QA session. View must not input/resize PTY.
Revoke only QA device through authorized owner surface; await WS close, verify subsequent HTTP 401.
Capture independent terminal rendering, selection identity, authorization statuses and no leaked paths.
Cleanup only recorded QA sessions/listener/process; never issue shared daemon Shutdown or touch user sessions.

## Remaining decisions / blockers
- Approve bounded C1 existing-agent control vs launch/resume/close requirements; latter expands this contract.
- Approve waiting normalization and scoped-token migration/default grants; choose single resize-controller policy.
- Decide legacy inventory capability/version negotiation and unknown-state UX; global cannot mean desktop tabs.
- Safe standalone loopback launcher/storage isolation and cross-platform agent-report ingress remain unverified;
  Unix-only listener (:970-998) is insufficient evidence of Windows waiting support. These block live QA,
  not this report. No tests/build/runtime success is claimed. Report is uncommitted in a concurrent dirty tree.
