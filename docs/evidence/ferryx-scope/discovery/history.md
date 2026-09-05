# A3 implementation contract: historical provider conversations + native resume

Status: executable proposal, not shipped behavior. Bounded source discovery, 2026-09-05.
Only this report was written; no provider logs opened, sessions resumed, tests run, or daemon mutated.
Read root, backend, daemon, UI, lib, components, IPC and terminal AGENTS instructions.
LSP document-symbol and reference requests both failed: daemon unreachable at
`/Users/indo/.omo/lsp-daemon/v0.1.0/daemon.sock`; source reads/searches substitute, not LSP success.
Foreign dirty files are active and read-only. Initial parent-directory find timed out;
targeted instruction lookup completed (no docs-local AGENTS found).

## Verified implementation facts (paths relative to repository root)

- `src-tauri/src/agent_detect/engine.rs:16-21,39-96`: Detection contains activity,
  rule ID and manifest ID, and consumes ScreenInput. It is not a conversation parser.
- `ui/src/components/CommandPalette.tsx:7-28,95-118`: accepts worktrees/tabs,
  filters tab labels and worktree text locally; no historical conversation source.
- `src-tauri/src/daemon/protocol.rs:12,24-59`: wire version is **3** (AGENTS' v2 is stale);
  AgentProviderSession is key/id/optional transcriptPath; TerminalStartup::AgentResume
  carries agentType plus that provider reference, separately from PTY session IDs.
- `src-tauri/src/daemon/protocol.rs:127-178`: Spawn accepts typed startup;
  ListSessions/DescribeSession/Attach and SaveSession/LoadSession are terminal/layout APIs.
  `src-tauri/src/daemon/server.rs:1420-1439` sends terminal byte history, not message history.
- `ui/src/lib/agentResume.ts:28-46,79-126,174-203`: existing builders and normalization
  reject empty/oversized/control-character/leading-dash/`latest` IDs, require matching key,
  and accept absolute transcript paths. New history code must not use the legacy sessionId fallback.
- `ui/src/lib/agentResume.ts:51-68,148-150`: authoritative capture allowlist is narrower
  than builders; capability currently says `uuid` even for non-UUID providers. Do not reuse
  that label as a historical-log capability or enforce UUID shape universally.
- `ui/src/lib/agentResumeAffordance.ts:82-105,116-203`: reconnect gates on capture support,
  valid reference, exited/no-backend state, duplicate claims and transient lifecycle.
  This is an existing-pane affordance, not an API for history rows without panes.
- `src-tauri/src/daemon/client.rs:713-770`: spawn_terminal_with_startup sends Spawn and
  preserves AgentResumeInvalid / AgentSessionConflict with existingSessionId details.
  Existing desktop call site: `src-tauri/src/ipc/terminal.rs:682-690`.
- `src-tauri/src/daemon/server.rs:1900-1950,1953-2036`: spawn lock, request fingerprint,
  workspace/CWD jail, startup validation and provider ownership fence wrap PTY creation.
- `src-tauri/src/terminal/shell.rs:98-246`: resolve_agent_resume_plan maps typed references
  to program/argv; pi/prime-agent require transcriptPath, unknown providers fail.
  Server call site `src-tauri/src/daemon/server.rs:1988-1993` uses resolve_startup_command.

## Provider evidence and capability gates

All paths below are **current adapter contracts in source**, not independently verified
installed-provider schemas. No transcript message schema is established by this scan.
R = existing native resume builder, gated by validated reference and installed CLI;
L = proposed metadata listing after exact identity fixtures; T = read/search transcript
disabled until versioned provider-message fixtures pass. None of L/T is shipped here.

| Provider | Located contract / parser evidence | Proposed capability |
| --- | --- | --- |
| claude | `src-tauri/src/ipc/agents.rs:217,649-659`: `.claude/projects/`, UUID path extraction; test uses JSONL | L gated, T blocked, R |
| codex | `src-tauri/src/ipc/agents.rs:218,233-242`: `.codex/sessions/`, UUID path extraction only | L gated, T blocked, R |
| omo | `src-tauri/src/ipc/agents.rs:171-188,222,801-828`: PI_SESSION_FILE; `.omo/sessions/`, timestamp_UUID JSONL fixture | L gated, T blocked, R |
| pi | `src-tauri/src/ipc/agents.rs:422-485`: environment/open file then `.pi/agent/sessions/<encoded-cwd>/*.jsonl`, latest mtime fallback | L gated, T blocked, R requires actual path + ID |
| gjc | `src-tauri/src/ipc/agents.rs:488-508`: `.gjc/agent/sessions/` or XDG/profile layout; timestamp_short-hex JSONL filename parser | L gated, T blocked, R; not UUID-only |
| opencode | `src-tauri/src/ipc/agents.rs:337-418`: `.local/share/opencode/opencode.db`, session(id,directory,time_updated); CLI JSON parser reads id/directory, validates ses_ ID | L metadata adapter candidate, T blocked, R |
| antigravity | `src-tauri/src/ipc/agents.rs:282-335`: cache/last_conversations.json maps CWD to UUID; `.gemini/antigravity-cli/conversations/` marker at 224 | Recent mapping only, full L/T blocked, R |
| copilot | `src-tauri/src/ipc/agents.rs:219`: `.copilot/session-state/` UUID path marker only | L/T blocked, R |
| cursor / cursor-agent | `src-tauri/src/ipc/agents.rs:207-220,764-775`: alias + `.cursor/chats/`, path fixture chat.json | L/T blocked, R; canonicalize alias before claims |
| kimi | `src-tauri/src/ipc/agents.rs:221`: `.kimi/sessions/` UUID marker only | L/T blocked, R |
| prime-agent, mimo-code, droid, grok, devin, omp | `ui/src/lib/agentResume.ts:34-40,51-64`: builders but absent authoritative capture allowlist | No A3 L/T/resume enablement without new evidence |
| cline / unknown | No builder in `ui/src/lib/agentResume.ts:28-46` | Unsupported, explicit reason |

Do not repurpose discovery heuristics as exact historical identity: OpenCode SQL selects
latest per CWD; CLI parser accepts parent/child prefixes; pi selects latest mtime;
antigravity cache returns one conversation per scope. Historical selection must preserve
the exact provider entry, never rediscover “latest” at resume time.
Existing source uses HOME and macOS ps/lsof (`src-tauri/src/ipc/agents.rs:71-75,191-203`);
portable history readers must resolve platform/configured roots without these process probes.

## Minimal API/types to implement (new names, not claims of existing symbols)

```ts
type HistoryCapabilities = {
  list: boolean; read: boolean; search: boolean; resume: boolean; reason?: string;
};
type HistoryRef = { provider: string; entryKey: string }; // opaque backend locator, NOT agent ID
type HistoryEntry = {
  ref: HistoryRef; providerSession: AgentProviderSession | null;
  title: string | null; updatedAt: string | null; workspaceMatch: boolean;
  capabilities: HistoryCapabilities;
};
type HistoryMessage = { ordinal: number; role: string; text: string };
type HistoryPage<T> = {
  items: T[]; nextCursor: string | null; partial: boolean;
  warnings: { code: string; provider: string }[];
};
// Proposed desktop IPC commands, backend resolves roots and source files:
// history_list({workspaceId, provider?, cursor?, limit}) -> HistoryPage<HistoryEntry>
// history_read({ref, cursor?, limit}) -> HistoryPage<HistoryMessage>
// history_search({workspaceId, provider?, query, cursor?, limit})
//   -> HistoryPage<{entry: HistoryEntry, ordinal: number, snippet: string}>
// history_resume({ref, workspaceId, worktree?, clientRequestId})
//   -> {backendSessionId: string, epoch: number}
```

Backend owns source identity and revalidates entry/path/version immediately before resume.
entryKey/cursors may be Ferryx-issued locators; providerSession.id MUST come from provider
data. Missing/conflicting ID => readable if possible, resume disabled; never mint an agent ID.
Reuse AgentProviderSession and typed TerminalStartup::AgentResume, not shell input strings.
Use daemon spawn_terminal_with_startup and its claim/idempotency contract; same request ID
on an ambiguous retry, new request only for a deliberate new attempt. No auto-resume on restore.
Read/search are side-effect-free, local desktop only. Roots allowlisted, paths canonicalized,
symlink escapes rejected, SQL read-only, query parameterized; malformed/permission errors
reported separately from empty results. Plain text rendering; never execute transcript tools.
Proposed hard bounds: 100 results/page, 1,000 candidate files and 16 MiB read per request,
1 MiB max record; cursor + partial/warnings on limits, explicit refresh, no persistent index.
Search means parsed message text, not just titles; unsupported parser is never “no matches”.

## Assignment and write ownership (future implementation only)

1. Backend history owner: new `src-tauri/src/agent_history/` adapters and fixture tests;
   new `src-tauri/src/ipc/agent_history.rs`; wire registration through existing IPC/app owners.
   Start with claude/codex/omo JSONL once schema fixtures are supplied; keep provider flags independent.
2. Desktop history owner: new history lib/types + reader component; add historical group
   to CommandPalette with explicit Open and Resume actions. Selection only reads by default.
3. Resume integration owner: connect history_resume result to native pane binding, maintaining
   leaf/frontend/backend identities; share existing daemon spawn path, no new PTY owner.
4. Coordinate edits to dirty `ui/src/lib/tauri.ts`, types.ts, workspaceStore.ts, App.tsx and
   `src-tauri/src/ipc/mod.rs` with current owners. No edits to detection rules or persistence
   needed for read/list/search. Provider files remain provider-owned; only explicit resume
   allows the provider process to append its native log. This report grants no live mutation.

## RED seam and literal QA (future execution; not run)

- First RED: fixture-backed history_list/read/search returns an older conversation containing
  a sentinel absent from newest session, preserving provider ID and message ordinal. Current
  scoped source has no history API; create this test before implementation, then observe RED.
- Cover short-hex/ses_ IDs, missing ID/path, partial final JSONL, unknown record kinds,
  escaped roots, inaccessible file, capped scan and cursor consistency under append.
- Resume seam: actual typed spawn boundary with isolated test provider/PTY, assert exact
  provider reference, one spawn for duplicate request, conflict for competing owner,
  and no spawn on invalid/stale entry. No shared daemon or persistence suite.
- Subscribe before actions; bounded event/deferred completion, no sleeps/polling or prose tests.
- Existing focused frontend command (package `ui/package.json:8`; Vitest --help verified):
  `bun run --cwd ui test src/lib/agentResume.test.ts src/lib/agentResumeAffordance.test.ts src/components/CommandPalette.test.tsx`
- Existing parser fixture command (cargo test --help verified; tests read at agents.rs:648-938):
  `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::agents::tests::opencode_session_list_json_parser_matches_cwd`
- Real-surface QA after implementation, in an isolated desktop test profile with disposable
  provider-generated history: open Command palette, search the older sentinel, open result,
  inspect messages/identity, choose Resume, observe native TUI restoring that exact session.
  Trigger Resume twice and confirm one native pane/PTY; try already-owned result and see
  conflict; remove only disposable source file and confirm explicit unavailable state.
  Do not run those mutation actions against the current user's sessions.

## Blockers / remaining decisions

- Obtain redacted real provider version/header/message/branch fixtures; filename evidence
  cannot establish conversation parsing or full-history completeness. No schemas invented.
- Confirm installed CLI resume/help and source-root overrides per supported OS before enabling
  R. Builder argv above is source evidence, NOT validated provider CLI execution guidance.
- Decide initial provider acceptance set: recommend claude/codex/omo, independent capability
  gates for others; transcript branch/active-leaf semantics must follow each supplied schema.
- Confirm bounded ephemeral scans are acceptable and choose missing-workspace UX; recommend
  read-only view with explicit registered-workspace selection before Resume, no implicit CWD.
- Runtime QA, RED/GREEN tests and native pane integration remain unverified. LSP unavailable;
  no test/build success claimed. Report is uncommitted and subject to concurrent work.
