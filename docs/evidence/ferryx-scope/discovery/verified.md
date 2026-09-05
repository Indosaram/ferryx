# Verified integrated dependency and decision digest

2026-09-05. Read all six discovery reports and rechecked decisive definitions/callers in the dirty working tree.
This is source/executable-contract verification, not implementation or a claim of RED/GREEN/runtime success.
Only this file was written; no provider logs, live PTYs, daemon startup, destructive tests or commits were used.
Line numbers drift under concurrent edits; paths plus symbol names below are the durable references.
D = src-tauri/src/daemon; R = src-tauri/src/remote; I = src-tauri/src/ipc; U = ui/src.
Read docs/DESIGN.md and docs/evidence/ferryx-scope/qa-environment.md: preserve existing UI primitives,
masking/accessibility and exclusions; QA recipes are prepared, not runtime proof.

## 1. Confirmed reuse ledger: definition -> real caller -> limitation

- D/protocol.rs:12,24-59 defines protocol v3, AgentProviderSession and TerminalStartup::AgentResume.
  D/client.rs::spawn_terminal_with_startup constructs Spawn; I/terminal.rs:682 calls it.
  Spawn returns numeric u64 epoch; new public string epochs need an explicit lossless wire adapter.
- D/client.rs:713 preserves AgentResumeInvalid/AgentSessionConflict; D/server.rs:1953-2036 resolves
  workspace/CWD jail, checks provider claims and records spawn request identity before releasing ownership.
  terminal/shell.rs::resolve_startup_command -> resolve_startup_command_pure -> resolve_agent_resume_plan
  is the production argv path called by D/server.rs:1988. Reuse this, not shell-string injection for resume.
- U/lib/agentResume.ts defines normalization/buildResumeArgv/getAgentResumeArgv/capture allowlist;
  U/lib/agentResumeAffordance.ts:74,131,146 calls normalization, capture check and builder.
  Existing-pane reconnect is reusable policy, NOT a historical-list API; reject legacy ID fallback in A3.
- D/proxy.rs::SessionRouter implements R/backend.rs::RemoteSessionBackend; R/server.rs terminal handlers
  call attach_with_sequence/write_input/resize/signal. D/server.rs injects the router into gateway state.
  list_sessions silently omits failed peers; local describe returns workspace_id=None. Repair before scoping.
- R/server.rs::recover_remote_terminal_attachment calls attach_with_sequence; both byte/grid output loops
  call recovery on lag. Reuse sequence/gap semantics; do not rebuild PTY ownership in a browser client.
- D/server.rs::spawn_agent_state_listener sends parsed reports to agent_state_tx; Attach subscribes there.
  It is Unix-only, validates state/provider shape but does not validate registered session membership or
  retain an inventory snapshot. It is an event seam, not complete authoritative cross-platform inventory.
- ssh/config.rs::parse_ssh_config/import_aliases -> I/ssh.rs::cmd_ssh_import_config; ssh/worktree.rs::
  parse_worktree_porcelain -> cmd_ssh_list_remote_worktrees. Reuse bounded parser/DTOs, not full SSH semantics.
  worktree/manager.rs::format_branch_name -> ssh/worktree.rs::remote_add_argv and local create_worktree.
- I/ssh.rs defines eight commands, registered at src-tauri/src/lib.rs:787-794. Registration is NOT a real
  UI consumer: no SSH invoke consumer was established. ssh/exec.rs::interactive_argv has only test callers.
  probe_argv has a real caller in cmd_ssh_test_connection but omits port/key/jump and a terminating command.
  Remote Git builders have real IPC callers but interpolate unquoted paths/base and omit repository cwd.
  load_store hides corruption; save_store uses a fixed temporary path without serialized mutation.
  ConnectTimeout=2.5 is suspect portability/precision, not proven invalid (ssh.md reports local -G accepted
  it as 2); require integer timeout + total deadline. No existing SSH remote-daemon relay is established.
- R/server.rs::create_worktree/delete_worktree are real registered REST handlers using WorktreeManager.
  U/lib/remoteClient.ts::createWorktree declares Promise<Worktree>, but HTTP returns RemoteWorktreeInfo
  (slug/label/attention, not full Worktree). No remote UI consumer was found in remote/state search.
  Its spawnTerminal fabricates remote-${Date.now()}: never count it as an executable spawn API.
- browser/manager.rs::assert_automation_generation -> I/browser.rs::browser_automation_act;
  I/browser.rs::eval_webview -> browser_automation_snapshot/act; browser_cli.rs also calls DOM snapshot.
  These are DOM/generation seams, not pixels. Same-URL reload/resize/zoom need new invalidation coverage.
- U/lib/browserTauri.ts::enqueueBrowserLifecycle -> setBrowserVisible/closeBrowser;
  U/components/BrowserPane.tsx:187 calls visibility. Preserve mask ownership and hide-on-cleanup behavior.
  I/browser.rs::with_webview has real native-handle callers, but no Ferryx native PNG capture caller exists.
  Platform snapshot APIs/upstream Orca code are candidates from design.md, not verified reusable Ferryx APIs;
  their external definitions were not independently re-read in this repo-only verification assignment.
- U/lib/tauri.ts::writeTerminal -> U/lib/terminalTransport/tauriTransport.ts and App.tsx:1348.
  It writes text to backendSessionId; neither image ingestion nor provider acceptance is established.
- R/auth.rs defines validate_token/exchange_pairing_code/revoke_device -> corresponding R/server.rs handlers.
  R/state.rs::new_with_paths_backend -> new_with_backend/new_persistent_with_backend and D/server.rs.
  R/auth.rs::write_private_json -> auth persistence and state::persist_config; remote_data_dir -> persistent
  constructor. Reuse explicit paths, not default storage. Private writer is NOT secure-key storage proof:
  it writes before chmod, ignores chmod errors, and shares a fixed temp name. Harden before VAPID secrets.
- ui/public/sw.js has install/activate/fetch only; R/server.rs::create_remote_router -> start_remote_server
  serves plain HTTP on 0.0.0.0. No existing push/chat/history/capture public API was established by searches.

## 2. Corrections that must override the reports

1. control.md narrows C1 to list/show/input/interrupt, but the accepted plan explicitly requires
   create/list/start/prompt/read/wait/stop plus event subscription and real CLI lifecycle proof.
   Implement that acceptance or obtain explicit scope change; do not label the subset C1 complete.
   Likewise C3 is host/workspace creation/launch/stop/reconnect, not merely background terminal attach.
2. chat.md's active-desktop lock conflicts with C3. All mobile operations must use the shared scoped,
   target-qualified service, independent client selection and revocation checks, not desktop focus.
3. AgentChatOpen cannot start app-server thread/resume while a native TUI still owns the same provider
   conversation. Existing daemon claim fence covers terminal startup only, not this new supervisor.
   Require one provider-session owner shared by A3/C1/C2; explicit handoff or shared provider transport.
   A second process resuming the same ID does NOT prove terminal/chat continuity or shared approvals.
4. Lead direct verification: Codex 0.153.2 stdio initialize id=1 succeeded; probe was killed. This child
   did not rerun it. No thread/turn/approval proof yet; schema/help alone cannot establish those behaviors.
   Exact v2 request methods, callback IDs, questions and continuity need versioned fixtures/runtime proof.
   No Claude permission-card protocol is established; regex/state reports cannot authorize approval cards.
   Use portable framed stdio, NOT unix://-only supervision or a preserved active-desktop lock.
5. History filename/CWD heuristics establish neither message schema nor exact historical identity.
   I/agents.rs OpenCode chooses latest by CWD/prefix; pi chooses latest mtime. GJC is not UUID-only.
   Enable list/read/search/resume independently from versioned provider fixtures; no invented session IDs.
6. Design proposal drops required tag/class/computed CSS/DOM context fields and automatic selection capture.
   Preserve those accepted requirements. Local-only file-reference handoff is a first slice, not full done:
   accepted scope requires readable attachment on remote target host and real agent image-read proof.
   Do not call text-write success image acceptance; freeze target/draft/hash and never silently submit twice.
7. A1/C3/C4 need a common TargetRef = {hostId, ownerId, epoch, backendSessionId}; workspace/worktree and
   tab/leaf mappings are attributes, not identity. control.md lacks host/legacy owner; push.md's tab hash
   and desktop selectContext are stale/ambiguous. Resolve opaque target on tap; expired target never retargets.
   Drafts and attachments must use the same qualified identity, not bare workspace/session storage keys.
8. Push must consume retained A1 transitions/completeness, not subscribe after configure and miss state.
   working->idle is NOT a provider-confirmed completion; unknown/lag/legacy absence must remain explicit.
   Add provider-confirmed completion where supported, dedupe by target+revision, and distinguish unread.
9. Phone Web Push requires trusted HTTPS, permission gesture, supported browser and reachable host/service.
   iOS Home-Screen PWA support (reported 16.4+) and Android background behavior need real device/version
   proof; they were not externally reverified here. Plain HTTP LAN/WS badges/desktop alerts are not C4.
   Revocation must remove local subscriptions immediately: it does NOT cause push service HTTP 404/410.
   Test provider Gone responses separately. Validate subscription endpoints/redirects against SSRF and
   payload URLs against same-origin rules; title-only default, body opt-in, no paths/tokens in payloads.
10. chat.md uses bun test for Vitest suites: use the package script `bun run --cwd ui test ...`.
    Missing imports/tests are NOT behavioral RED; require a collected failing assertion. Broad suites and
    unisolated `cargo run ... -- --daemon` are unsafe instructions for this shared workstation; reject them.

## 3. Decisions and write ownership before implementation

- Decision D1 - Identity/auth/protocol: approve qualified target/provider ownership, lossless epochs, scopes,
  capability negotiation vs v4, control/resize lease, and unknown/incomplete inventory UX.
  Existing sockets capture Control once; View resize, global allow_control and live revoke need enforcement.
- Decision D2 - SSH runtime/trust: approve manual persistent helper, OS/architecture matrix, external known-host
  trust and explicit reconnect. Missing helper/trust/auth/permission failures need distinct evidenced codes.
- Decision D3 - Provider coverage/ownership: recommend A3 claude/codex/omo only after actual schema fixtures,
  Codex structured adapter only after protocol/ownership proof; unsupported providers remain terminal mode.
- Decision D4 - Attachments/delivery: one host-scoped staging contract for Design/C2; phone upload is missing.
  Decide byte/type caps, canonical jail, cancellation/cleanup/retention, explicit submit and delivery receipt.
- Decision D5 - Push deployment/crypto: choose HTTPS, stable VAPID storage and a vetted maintained library;
  no blind cargo add or hand-rolled crypto fallback. Bell push is optional, not substitute completion proof.
- One integrator owns D/{protocol,server,client,proxy}.rs, R/{server,protocol,state,auth,mod}.rs,
  src-tauri/{Cargo.toml,Cargo.lock,src/lib.rs}, I/mod.rs, U/lib/{tauri,types}.ts and RemoteApp.tsx.
  A1/A2/A3 additionally overlap workspaceStore/runtime/restore, Sidebar and project/session persistence.
  Design alone owns browser modules/BrowserPane/Toolbar; SSH, history, chat and push owners own NEW modules.
  Shared-file patches are serialized through integrator after re-read; separate domains are not disjoint writes.
  Foreign notification/native-terminal/RemoteTerminal/RemoteSessionList work is active, not disposable.
- Excluded features stay excluded, not prerequisites: PR/checks/CI logs/GitHub/Linear, full Git GUI/editor,
  voice, Cloud VM recipes/provisioner, E2EE relay, scheduling/usage/workflow plugins and artifact-share service.
  A private SSH helper and bounded attachment staging are necessary execution plumbing, not those products.

## 4. Acyclic per-phase order (proposed; acceptance unchanged)

- P0 shared foundation S: freeze qualified targets, ownership, auth/actions/events and safe fixtures;
  extract internal C1 target/delivery contract here, NOT the entire public C1 release.
- Phase 1: S -> A1 retained inventory + desktop/remote consumers; S -> A2 SSH/helper/project/PTY path;
  S -> A3 fixture history -> exact resume. These tracks run parallel only in their reserved new modules.
- Phase 2: S -> Design inspect/rectangle + CSS -> native capture -> immutable preview -> explicit handoff.
  A2 -> remote-target attachment integration; Design does not depend on full public C1 or C2 release.
- Phase 3: S + A1 -> C1 full lifecycle API/CLI/events; C1 -> C2 provider transport -> chat/cards/files/drafts.
  A1 + A2 + C1 -> C3 independent two-host create/launch/stop/reconnect; C2 UI is NOT a C3 prerequisite.
  A1 + C3 + HTTPS/VAPID -> C4 background delivery + qualified task return.
  Shared attachment/protocol foundations precede both consumers; no Design <-> C1 or C2 <-> C3 cycle.

## 5. Safe executable RED and surface proof contract

Commands below are instructions, NOT run/passing claims. Cargo test/help and bun run/help were executed;
ui/package.json confirms Vitest run --maxWorkers=1. No product validator was executed in this report task.
Markdown diagnostics were attempted but LSP daemon was unreachable; structural/line-cap checks passed.
Implement named tests FIRST; prove each filter selects nonzero tests and record intended failing assertion.
For each NEW isolated Rust module below: `cargo test --manifest-path src-tauri/Cargo.toml --lib FILTER`
with FILTER exactly `control_inventory_contract`, `ssh_contract`, `agent_history_contract`, `browser::design`,
`agent_chat::`, or `push_contract`. These namespaces are proposed, not existing executable APIs.
For NEW UI files: `bun run --cwd ui test src/lib/agentChatTypes.test.ts src/remote/pushDeepLink.test.ts`.
Existing baseline command: `bun run --cwd ui test src/lib/agentResume.test.ts src/lib/agentResumeAffordance.test.ts src/components/CommandPalette.test.tsx`.
After implementation: diagnostics on changed sources, focused tests, `bun run --cwd ui build`, then
`cargo build --manifest-path src-tauri/Cargo.toml`; build success alone is never native/device surface proof.

- A1/C1/C3 fixture: real router + new_with_paths_backend, temp stores, explicit 127.0.0.1:0 listener,
  two disposable PTYs/workspaces/hosts; background blocked without desktop, incomplete legacy owner,
  stale epoch denial, View resize denial, revoke closes stream, repeated request spawns once.
  HTTP surface: `curl --fail-with-body --silent --show-error "$BASE/api/v1/health"`, then
  `curl --fail-with-body --silent --show-error -H "Authorization: Bearer $TOKEN" "$BASE/api/v1/agents"`.
  BASE/TOKEN must come from private fixture; agents route is future. Real CLI full lifecycle proof waits
  for implemented/help-verified grammar; current ferryx --help must not be probed (can launch GUI).
- A2 fixture: runner captures identical nondefault SSH options, corrupt temp store never rewritten;
  isolated guest helper/committed repo, no shared known_hosts. UI Run on -> worktree -> remote cwd/output,
  sever only fixture transport -> reconnect same PID/epoch/replay; changed key/auth/permission/missing helper.
- A3 fixture: older parsed-message sentinel absent from newest; malformed/partial/escaped/missing/large logs;
  test-only ferryx-test-agent resolves to /bin/cat in terminal/shell.rs. Isolated typed spawn proves identity,
  dedupe/conflict; provider-generated disposable logs + real native TUI prove exact cwd/ID resume.
- Design fixture: controlled canvas/WebGL/image/iframe/scroll/click-counter page; deferred native callback
  proves stale/recreate/resize cancellation. Actual three-OS child pixels, crop/hash, no overlay and agent
  image-read receipt prove surface; injected PNG proves plumbing only. No existing fixture launcher claimed.
- C2 fixture: scripted versioned JSON-RPC peer through production supervisor/codec; two clients race one
  approval, stale reply rejected, reconnect outstanding request, upload failure/draft/IME; real isolated
  provider approval/decline/question/interrupt and shared terminal identity are release evidence.
- C4 fixture: fake sender asserts scoped edge/dedupe/revoke; independent 404/410 prunes subscription.
  Real HTTPS iOS/Android lock/background delivery with desktop closed, tap exact target, expired target,
  denied permission and revoked device required; host remains awake/reachable. Capture versions/receipts.
All async tests subscribe before trigger and await exact event/deferred completion with bounded timeout;
no sleeps/polling, shared daemon discovery/adoption/Shutdown, live provider mutation or default-user QA.
QA-environment.md supplies an unexecuted isolated desktop launch recipe; verify actual app/WebKit paths,
QA PID, vacant fixed ports and serialized ui/dist builds. Provider HOME/credentials remain unisolated.
SSH guest/helper and physical-device HTTPS fixtures remain blockers; runtime override alone is insufficient.
