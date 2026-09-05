# C2 Discovery: Structured Mobile Chat / Approvals / Questions + Attachment & Draft

Bounded implementation contract. All `file:line` facts verified against working tree on 2026-09-05
(branch `main`, foreign dirty work active — read-only survey; LSP is globally unavailable — official
daemon startup failed `owner_changed_during_cleanup` (dead prior owner) at
`/Users/indo/.omo/lsp-daemon/v0.1.0/daemon.sock`; no further LSP calls attempted, structural/source
fallback (`rg`) used for all reference checks).

## 1. Provider protocol determination (verified locally)

Installed CLIs (PATH resolves both to cmux shims; real binaries used for all checks):
`/Users/indo/.local/bin/codex` = codex-cli 0.153.2; `/Users/indo/.local/bin/claude` = Claude Code 2.1.251.

**Codex app-server = smallest real structured path.** From `codex --help` / `codex app-server --help`:
- `app-server [experimental]`; `--listen` supports `stdio://` (default), `unix://PATH`, `ws://IP:PORT`, `off`;
  subcommands `daemon start|stop|enable-remote-control`, `generate-ts --out`, `generate-json-schema --out`.
- Schema bundle generated this session (`codex app-server generate-json-schema --out <tmp>`, exit 0) proves:
  threads/turns: `thread/start`, `thread/resume`, `thread/list`, `thread/items/list` (cursor+limit+sortDirection),
  `turn/start`, `turn/steer`, `turn/interrupt` (params `threadId`+`turnId`);
  streaming notifications `item/started`, `item/completed`, `item/agentMessage/delta`, `turn/completed`;
  **real approval requests** (server→client): `CommandExecutionRequestApprovalParams` (fields `approvalId`
  (opaque callback UUID), `command`, `commandActions` parsed-for-display, `cwd`) with decision enum
  `accept | acceptForSession | acceptWithExecpolicyAmendment | ...` (`ExecCommandApprovalResponse.ReviewDecision`);
  plus `FileChangeRequestApproval*`, `PermissionsRequestApproval*`, legacy v1 methods `execCommandApproval`,
  `applyPatchApproval`; **real questions**: `ToolRequestUserInputParams/Response` (EXPERIMENTAL; map of
  question id → `{answers: string[]}`); MCP elicitation (`McpServerElicitationRequestResponse`).
- UNKNOWN: exact v2 method-string for the approval server→client requests (bundle carries type names; v1
  literals verified). Confirm via `generate-ts` bindings during implementation. Marked, not inferred.

**Claude Code stays terminal-adapter for now.** `claude --help` 2.1.251: `-r, --resume [value]`,
`--session-id <uuid>`, `-p --input-format stream-json --output-format stream-json`, `--include-partial-messages`,
`--replay-user-messages`, `--permission-mode` (choices acceptEdits|auto|bypassPermissions|manual|dontAsk|plan).
`--permission-prompt-tool` is ABSENT from this help; no `@anthropic-ai` SDK package in repo `node_modules`;
actual-tool-approval-over-stream-json is therefore UNVERIFIED → no permission cards for Claude until a verified
control protocol exists. Resume identity is fine (`agentResume.ts:29`).

## 2. Terminal adapters vs structured controlled sessions (existing code)

- **Terminal adapter (today, keep as fallback):** screen-regex state inference — `agent_detect/engine.rs:24,39`
  (`DetectionEngine::detect`), shipped manifests e.g. `manifests/codex.toml:7` (`osc_title_blocked` "Action
  Required"), `manifests/claude.toml:48` (`bash_permission_prompt` matches "do you want to proceed?" screens).
  Input = raw PTY bytes over remote WS (`ui/src/remote/RemoteTerminal.tsx:573`), interrupt = `signal` frame
  (`RemoteTerminal.tsx:596-601`). Approvals are keystrokes; per C2 these must NEVER render as permission cards.
- **Authoritative agent-state seam (existing):** PTY children get `FERRYX_SESSION_ID` +
  `FERRYX_AGENT_STATE_SOCKET` env (`src-tauri/src/terminal/pty.rs:136-141`), socket path from
  `daemon/server.rs:795-800`; lifecycle extension installed into `~/{.omo,.pi,.omp}/agent/extensions`
  (`daemon/agent_extension.rs:17-23`) reports newline-JSON `{type:"agentState", sessionId, state, agent,
  providerSession}` (`resources/agent-extensions/ferryx-agent-state.ts:38`). Provider identity is the agent's
  own, never minted (`ui/src/lib/types.ts:72-85`; `ui/src/lib/agentResume.ts:28-30` resume argv:
  claude `--resume <id>`, codex `resume <id>`); discovery via PID walk + open-file store regexes
  (`ui/src/lib/agentSessionDiscovery.ts:12-13`); daemon request `DiscoverAgentSession`
  (`daemon/protocol.rs` enum at :111; handler `daemon/server.rs:1282`).
- **Structured controlled session (proposed):** daemon-supervised `codex app-server` child on a
  `unix://` socket; Ferryx maps JSON-RPC approval/question requests to remote cards keyed by request id and
  routes answers back as JSON-RPC results. Session identity via `thread/resume` + existing
  `AgentProviderSession` contract (wire: `daemon/protocol.rs:27-36`; TS: `ui/src/lib/types.ts:53-58`).

## 3. Minimal proposed API/types

Daemon (UDS, additive to `DaemonRequest` at `daemon/protocol.rs:111`; version bump decision below):
- `AgentChatOpen { backend_session_id, agent_type, provider_session: AgentProviderSession } -> { chat_id }`
  (daemon spawns `codex app-server --listen unix://<per-chat socket>`, `thread/resume` when provider session
  exists, else `thread/start`).
- `AgentChatSend { chat_id, text, attachment_paths: Vec<String> } -> { turn_id }` (`turn/start`).
- `AgentChatDecide { chat_id, approval_id, decision }`; `AgentChatAnswer { chat_id, answers }`;
  `AgentChatInterrupt { chat_id, turn_id }`; `AgentChatClose { chat_id }`.
- Responses reuse structured-error `{code,message,details}` style (`ui/src/lib/agentReconnect.ts:29-33`).

Remote gateway (Axum; mirror existing patterns):
- One new WS `GET /api/v1/agent-chat/{sessionId}` registered in `create_remote_router`
  (`remote/server.rs:1420-1444`), same token auth + active-desktop lock as `ws_terminal_handler`
  (`remote/server.rs:816-845`, FORBIDDEN at :845). Frames are typed JSON in the
  `ClientControlMessage`/`ServerControlMessage` tag style (`remote/protocol.rs:135-155`):
  client→host `agentChat.send|decide|answer|interrupt`; host→client `agentChat.item` (transcript delta),
  `agentChat.approvalRequest {requestId, approvalId, kind, command, cwd}`,
  `agentChat.questionRequest {requestId, questions}`, `agentChat.state`, `agentChat.error`.
- Reuse `RemoteEventMessage` (`remote/protocol.rs:155`) only if push-to-idle clients is added later.

UI (remote/web):
- `ui/src/remote/AgentChat.tsx` + `ui/src/lib/agentChatTypes.ts` (frame codecs) beside existing
  `RemoteTerminal.tsx`; mode toggle mounted in `ui/src/remote/RemoteApp.tsx:526` region.
- Unsent draft persisted per workspace+session under `ferryx.remote.chatDraft.<wsId>.<sessionId>`
  (canonical `ferryx.*` prefix, `ui/src/lib/storageKeys.ts:1-10` convention); attachments are host-side
  paths resolved by the daemon (never send local phone paths). Request IDs like
  `agent-reconnect-<uuid>` (`ui/src/lib/agentReconnect.ts:22-23`); stale/duplicate decision replies are
  dropped server-side by requestId.

## 4. Ownership / write scopes

- NEW: `src-tauri/src/daemon/agent_chat.rs` (app-server child supervisor + JSON-RPC codec);
  `ui/src/remote/AgentChat.tsx`; `ui/src/lib/agentChatTypes.ts`.
- EDIT: `src-tauri/src/daemon/protocol.rs` (request/response variants); `src-tauri/src/daemon/mod.rs`;
  `src-tauri/src/remote/server.rs` + `remote/protocol.rs` (route + frames); `ui/src/remote/RemoteApp.tsx`.
- READ-ONLY reuse (do not edit): `ui/src/lib/agentResume.ts`, `agentSessionDiscovery.ts`, `types.ts`;
  `src-tauri/src/agent_detect/*` (unchanged; terminal adapter stays default for unstructured agents).
- FORBIDDEN here: daemon persistence tests, shared daemon `Shutdown`, live user session mutation,
  agent_detect changes, git commits.

## 5. RED test seam (write tests first; all currently fail — symbols absent)

- Rust: `cargo test --manifest-path src-tauri/Cargo.toml agent_chat::` — serde round-trips for new
  `DaemonRequest`/WS frame variants + supervisor driven by a fake JSON-RPC server on a tmp Unix socket
  (scripted `CommandExecutionRequestApprovalParams` → assert `AgentChatDecide` routes the decision; no real
  codex binary in unit tests).
- TS: `bun test --cwd ui src/lib/agentChatTypes.test.ts` — frame parse/mapping, stale-request drop,
  draft save/restore; `bun test --cwd ui src/remote` — chat surface contract following
  `ui/src/remote/RemoteTerminal.contract.test.tsx` style. No fixed sleeps; await frame/event signals.

## 6. Real-surface QA (literal commands/actions)

1. `codex app-server generate-json-schema --out <tmpdir>` (verified exit 0 this session) — snapshot diff of
   protocol types per codex version; fail the build on approval-type drift.
2. Sandbox probe (isolated HOME/cwd, never the user's live sessions): pipe an `initialize` JSON-RPC handshake
   into `/Users/indo/.local/bin/codex app-server --listen stdio://`, then `thread/start` +
   a turn that triggers `CommandExecutionRequestApproval`; assert the approval request frame arrives as JSON,
   not as screen text.
3. `cargo check --manifest-path src-tauri/Cargo.toml` then `cargo test --manifest-path src-tauri/Cargo.toml --lib agent_chat::`.
4. `bun test --cwd ui src/lib/agentChatTypes.test.ts && bun test --cwd ui src/remote`.
5. End-to-end on an isolated daemon (never `/tmp/rorca-{uid}/daemon.sock` of the live user daemon; needs
   socket-path isolation — see blockers): phone/web client opens chat on a codex pane, receives an approval
   card, declines, and asserts the decline surfaces in the terminal transcript of the same thread.

## 7. Blockers, unknowns, remaining decisions

- BLOCKED (environment): no isolated daemon instance available; live daemon must not be touched (foreign
  dirty work + no daemon persistence/Shutdown allowed). QA step 5 waits for a sandboxed run.
- UNKNOWN: exact v2 method strings for approval server→client requests; Claude approval control protocol
  (undocumented in 2.1.251 help) — both marked, must be probed, not assumed.
- UNKNOWN: codex auth reachability from a launchd-context daemon (keychain) — probe in step 2 environment.
- DECISION 1: daemon `DAEMON_PROTOCOL_VERSION` is 3 (`daemon/protocol.rs:12`); additive variants + capability
  flag vs bump to 4 — recommend additive with handshake capability announcement.
- DECISION 2: per-chat `unix://` app-server child (recommended: fits daemon ownership, mirrors agent-state
  socket) vs one shared app-server daemon (`codex app-server daemon start`).
- DECISION 3: attachment transfer — host-side staged file path + size cap vs base64 in `AgentChatSend`
  (recommend staged path; daemon already owns host FS scope).
- Codex app-server is `[experimental]`: schema snapshot check (QA 1) is the drift guard; pin tested
  codex version in docs.

## 8. Verification notes

- Every symbol above re-checked by `rg` against current source this session; LSP was down (reported, not
  silently skipped). CLI facts from the real binaries' `--help` and a generated schema bundle only; no live
  agent/daemon sessions were started, and no files outside this report were written.
