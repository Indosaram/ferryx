# Agent session reconnect

Ferryx separates two recovery paths:

- **Warm attachment:** if the daemon still owns the PTY, Ferryx attaches the existing backend and
  preserves the pane, local session, daemon epoch, and output sequence. It does not spawn or resume
  an agent.
- **Cold reconnect:** if the backend PTY is gone but Ferryx retained an agent-authored provider
  reference, the exited pane shows an explicit Reconnect action. Reconnect starts the provider's
  native resume command and rebinds the returned backend to the same local pane/session.

Ferryx never creates provider session IDs. It only accepts references reported by the agent event
pipeline or migrated from an older persisted `agentSessionId` field.

## Transaction contract

Cold reconnect uses a typed `agentResume` daemon startup request rather than writing a command into
a replacement shell. The operation:

1. validates the current workspace, agent type, provider key, provider value, and duplicate claims;
2. synchronously claims the local session and uses one stable client request ID;
3. performs one idempotent daemon spawn;
4. attaches the returned backend before committing the binding;
5. preserves the local and provider identities while replacing the backend ID/daemon epoch;
6. resets stale output sequence state;
7. persists the proposed binding;
8. closes a late or partially attached backend and restores retryable local state on failure.

Concurrent activation or response-loss retry resolves to one daemon PTY. A provider reference that
is already owned by another live backend yields a typed conflict instead of another process.

## Production reconnect support

Ferryx advertises pane-level Reconnect only when it has both a native resume mapping and an
authoritative provider-reference capture path. The current production set is:

- Claude
- Codex
- Copilot
- Cursor / Cursor Agent
- Kimi
- Omo

Omo reports through the bundled event integration. The other listed agents use bounded local
process/session-file discovery; Ferryx does not synthesize a provider ID when discovery fails.

## Validated daemon mappings

The daemon maintains additional validated provider-native argv mappings for future integrations.
These mappings alone do not make an agent visible as reconnectable in the pane UI:

| Agent | Required reference | Native resume argv |
|---|---|---|
| Claude | `session_id` | `claude --resume <id>` |
| Codex | `session_id` | `codex resume <id>` |
| Gemini | `session_id` | `gemini --resume <id>` |
| Antigravity | `conversation_id` | `agy --conversation <id>` |
| OpenCode | `session_id` | `opencode --session <id>` |
| Pi | `session_id` plus transcript path | `pi --session <transcript-path>` |
| Prime Agent | `session_id` plus transcript path | `prime-agent --resume <transcript-path>` |
| Mimo Code | `session_id` | `mimo --session <id>` |
| Droid | `session_id` | `droid --resume <id>` |
| Grok | `session_id` | `grok --resume <id>` |
| Devin | `session_id` | `devin --resume <id>` |
| OMP | `session_id` | `omp --resume <provider-path-or-id>` |
| Omo | `session_id` | `omo --session <id>` |
| Kimi | `session_id` | `kimi --session <id>` |
| Copilot | `session_id` | `copilot --resume <id>` |
| Cursor / Cursor Agent | `session_id` | `cursor-agent --resume <id>` |

In particular, Ferryx does not invoke Omo with `--session-id`; that would mint or inject an identity
instead of resuming the one Omo authored.

## UI and errors

A disconnected resumable pane shows the recognized agent icon, progress state, Reconnect, and Retry
as appropriate. Missing, malformed, unsupported, or duplicate references do not launch a provider
agent. Ordinary non-agent exited panes use an explicit **Open new shell** path and do not imply
conversation recovery.

Errors cross Tauri IPC as structured `{ code, message, details }` values. Callers branch on stable
codes such as invalid resume input, claim conflict, protocol mismatch, missing binary/start failure,
or unavailable capability; they do not parse message prose.

## Persistence and compatibility

- Provider references survive omitted or malformed later status events.
- Semantically invalid agent/key/path reports are discarded without erasing an earlier valid
  reference.
- Persisted reconnect locks, request IDs, progress, and errors are intentionally discarded.
- Legacy workspace records containing only `agentSessionId` normalize to a provider reference when
  the agent mapping is known, and the migrated identity completes the normal reconnect transaction.
- Ambiguous response-loss retries reuse the same transient request ID; it is cleared after success
  and never serialized as provider identity.
- Warm restore, ordinary terminal creation, HMR, pane movement, native focus/input, output replay,
  and remote protocol compatibility retain their existing paths.
- Arbitrary processes cannot survive a daemon/host restart. Cold reconnect is limited to supported
  agents with provider-native resume semantics and a retained provider reference.

## Debug QA procedure

Desktop runtime validation must use exactly `bun tauri dev` and the debug app/build. The complete
manual procedure is recorded in
`.omo/evidence/ferryx-agent-reconnect/task-12-desktop/RUNBOOK.md`.

The runtime proof must show the same provider conversation answering a unique sentinel after only
the daemon PTY is lost, while retaining the local pane identity and acquiring a new backend/epoch.
Direct desktop automation is prohibited, so screenshot and visual behavior claims remain pending
until the user explicitly supplies or confirms them.

## Evidence index

- Warm restore: `.omo/evidence/ferryx-agent-reconnect/task-1-warm-restore.txt`
- Provider mappings/persistence: `.omo/evidence/ferryx-agent-reconnect/task-2-provider-reference.txt`
- Typed daemon startup: `.omo/evidence/ferryx-agent-reconnect/task-3-daemon-contract.txt`
- Daemon claims/idempotency: `.omo/evidence/ferryx-agent-reconnect/task-4-daemon-ownership.txt`
- Structured IPC: `.omo/evidence/ferryx-agent-reconnect/task-5-ipc-errors.txt`
- Event capture: `.omo/evidence/ferryx-agent-reconnect/task-6-session-capture.txt`
- Reconnect lifecycle: `.omo/evidence/ferryx-agent-reconnect/task-7-reconnect-state.txt`
- Transactional reconnect: `.omo/evidence/ferryx-agent-reconnect/task-8-transactional-reconnect.txt`
- Pane UI: `.omo/evidence/ferryx-agent-reconnect/task-9-pane-ui.txt`
- Native rebind: `.omo/evidence/ferryx-agent-reconnect/task-10-native-rebind.txt`
- Cross-layer tests: `.omo/evidence/ferryx-agent-reconnect/task-11-integration-contracts.txt`
- Full persisted-load-to-save integration: `.omo/evidence/ferryx-agent-reconnect/task-11-persisted-load-to-save.txt`
- Final automated gates: `.omo/evidence/ferryx-agent-reconnect/final-automated-gates.txt`
- Preliminary F1 verdict: `.omo/evidence/ferryx-agent-reconnect/preliminary-f1-final.txt`
- Scope cleanup: `.omo/evidence/ferryx-agent-reconnect/scope-drift-cleanup.txt`
- Cursor Agent alias verification: `.omo/evidence/ferryx-agent-reconnect/cursor-agent-authoritative-alias.txt`
- Runtime QA runbook/status: `.omo/evidence/ferryx-agent-reconnect/task-12-desktop/`
- Completion audit: `.omo/evidence/ferryx-agent-reconnect/completion-audit-pending.md`

Task 12 runtime evidence and final independent verification must be completed before this document
is treated as the final delivered behavior record.
