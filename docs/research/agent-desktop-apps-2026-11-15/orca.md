# Orca - static source analysis (todo 15)

- Repo: https://github.com/stablyai/orca, shallow clone at `684d36274a029881fb7f997ccaa6079bb5bd85d2` (cited as `@684d36274a02`) in `/Volumes/T9-Mac/project/ferryx-wt/.evidence/session-01a1d86e-continuation/src/orca`.
- Evidence route: OSS_REPO (also PUBLIC_BINARY v1.4.259, see todo 16). Note: `package.json` at the SHA reads `1.4.214`, behind the released 1.4.259 tag name; todo 16 should record the divergence.
- Citation form: `path:line@sha` followed by a quoted substring of that line; verify with `bun verify-citations.mjs apps/orca.md --repo <clone>`.
- Method: `git grep` over the clone, tests/fixtures excluded; nothing was installed, built or run. Plan-referenced prior citation `src/main/ssh/ssh-host-key-store.ts` has moved to `src/main/ssh/host-keys/ssh-host-key-store.ts` at this SHA; `src/main/project-groups/nested-repo-discovery.ts` is unchanged.

## (a) Stack / runtime

- Electron + React + TypeScript, with a separately launched daemon process for PTYs: `package.json:325@684d36274a02` "\"electron\": \"43.8.7\"" and `src/main/daemon/daemon-entry.ts:4@684d36274a02` "import { startDaemon, type DaemonHandle } from './server/daemon-main'"
- Version at the SHA: `package.json:3@684d36274a02` "\"version\": \"1.4.214\""
- Expo mobile app (`mobile/`), and its own cloud relay + push services (`cloud/apps/{relay,push,...}`): `cloud/README.md:3@684d36274a02` "The relay that connects the Orca mobile app to a desktop host."
- Size (measured with `git ls-files` at the SHA, not a citation): 35,564 tracked files; 29,272 `.ts`, 3,488 `.tsx`; 4,076,196 lines across `.ts/.tsx/.rs/.swift/.kt/.go`. Largest codebase of the six.

## (b) Agent integration

- Dual mode: agents run as raw CLIs in PTYs ("Terminal") or as structured sessions ("Chat"), and a conversation can move between them: `README.md:37@684d36274a02` "Run any agent in Chat UI or as the raw CLI"
- Structured Claude via Claude Agent SDK types over stream-json: `package.json:235@684d36274a02` "\"@anthropic-ai/claude-agent-sdk\": \"0.3.319\"" / `src/main/claude/claude-stream-json-connection.ts:4@684d36274a02` "import type * as ClaudeAgentSdk from '@anthropic-ai/claude-agent-sdk'"
- Codex via `app-server`: `src/main/codex-cli/wsl-codex-command.ts:44@684d36274a02` "appServerArgs: readonly string[] = ['app-server']"; generic ACP layer in `src/main/acp/` (listing); ~40 per-agent dirs under `src/main/` (amp, gemini, droid, kimi, opencode, pi, ...).
- Status for PTY agents from two channels: managed per-agent hooks POSTing to a loopback server, and OSC 7501 program-status from the terminal stream; hooks win: `src/main/agent-hooks/README.md:3@684d36274a02` "Orca learns what each agent is doing through two channels." / `src/main/agent-hooks/README.md:11@684d36274a02` "**Precedence:** hooks win, and OSC 7501 fills in wherever they don't speak."

## (c) Worktree / branch isolation

- Real `git worktree add`, with worktree move/prune/removal guards: `src/main/git/worktrees/creation/worktree-add.ts:232@684d36274a02` "'worktree', 'add']" and `src/main/git/worktrees/movement/worktree-move.ts:15@684d36274a02` "['worktree', 'move', oldPath, newPath]"
- Default workspace root `~/orca/workspaces`: `src/shared/constants.ts:65@684d36274a02` "[trimmedHomeDir, 'orca', 'workspaces']"
- Nested-repo discovery for project groups: `src/main/project-groups/nested-repo-discovery.ts:112@684d36274a02` "export async function scanNestedRepos(args: {"

## (d) Terminal implementation

- `node-pty` in a detached daemon (`src/main/daemon/pty-subprocess/`): `package.json:248@684d36274a02` "\"node-pty\": \"^1.1.0\"" / `src/main/daemon/pty-subprocess/native-pty-spawn.ts:84@684d36274a02` "proc = nodePty.spawn(wrapped.file, wrapped.args, {"
- Renderer xterm.js 6 with WebGL; headless xterm for state: `package.json:316@684d36274a02` "\"@xterm/xterm\": \"6.1.0-beta.336\"" / `package.json:315@684d36274a02` "\"@xterm/addon-webgl\"" / `package.json:241@684d36274a02` "\"@xterm/headless\""
- `src/main/ghostty/` only imports Ghostty *themes/config*, not the renderer (files: discovery, parser, theme-mapping).
- Daemon is detached and outlives dev worktrees/windows: `src/main/daemon/launch/daemon-launched-child-spawn.ts:102@684d36274a02` "detached daemons outlive their dev worktrees"

## (e) Review / diff / PR flow

- Diff view with inline review comments (Monaco): `package.json:347@684d36274a02` "\"monaco-editor\": \"^0.55.1\"" and `src/renderer/src/components/diff-comments/DiffCommentCard.tsx` (listing).
- Create PR through `gh pr create` (also Azure DevOps, GitLab, Gitea, Bitbucket dirs): `src/main/github/client/create/create-github-pull-request.ts:99@684d36274a02` "'pr'," / `src/main/github/client/create/create-github-pull-request.ts:100@684d36274a02` "'create',"; PR review comments via API: `src/main/github/client/create/add-pr-review-comment.ts:36@684d36274a02` "/pulls/${args.prNumber}/comments"

## (f) Remote / mobile access and push

- Mobile app reaches the desktop only through Orca's hosted relay (both sides dial out over WebSocket): `cloud/README.md:4@684d36274a02` "desktops never talk to each other directly: each opens an outbound WebSocket"
- Own push service sending APNs directly: `cloud/apps/push/src/providers/apns/apns-client.ts:9@684d36274a02` "production: 'api.push.apple.com',"; mobile uses `expo-notifications` for tokens: `mobile/app.config.js:16@684d36274a02` "plugin === 'expo-notifications'"
- Notifications fan out to phones before desktop early returns: `src/main/notifications/notification-delivery-service.ts:1@684d36274a02` "fan out to phones before desktop early returns"
- SSH remote hosts and ephemeral VMs (`src/main/ssh/`, `src/main/ephemeral-vm/`, listing); accepted host keys are kept in an Orca-side store, user `known_hosts` is read but never written: `src/main/ssh/host-keys/ssh-host-key-store.ts:1@684d36274a02` "consult user known_hosts without writing it"

## (g) Attention / notification model

- Four agent states from hooks/OSC 7501: `src/shared/agent-status/contracts/agent-status-types.ts:29@684d36274a02` "export const AGENT_STATUS_STATES = ['working', 'blocked', 'waiting', 'done'] as const"
- OSC 7501 records projected onto agent status: `src/shared/program-status/agent-status-projector.ts:1@684d36274a02` "Projects OSC 7501 records onto agent-status payloads"
- Tray attention, desktop/away presence and per-OS focus state decide delivery (`src/main/notifications/desktop-away-state.ts`, `macos-focus-state.ts`, listing): `src/main/notifications/notification-delivery-service.ts:1@684d36274a02` "Set tray attention before gates"

## (h) Checkpoint / rollback

- Structured Claude sessions support rewind by forking at a checkpoint: `src/main/claude/structured/claude-structured-agent-definition.ts:11@684d36274a02` "rewind: true," / `src/main/claude/structured/claude-structured-agent-definition.ts:12@684d36274a02` "fork: 'checkpoint',"
- absent: git-level per-turn file snapshots; searched `checkpoint|rollback` in `src/main/git` - 0 hits; rewind is conversation-level through the agent.

## (i) Persistence / session recovery

- Detached PTY daemon keeps sessions across app restarts; cold restore replays scrollback: `src/main/daemon/history/restoration/cold-restore-payload-cache.ts:6@684d36274a02` "scrollback: string"
- Profile state persisted via a loading store and atomic snapshot writer, node:sqlite readers for caches: `src/main/persistence/atomic-snapshot-writer.ts` (listing) and `src/main/sqlite/sqlite-statement.ts:1@684d36274a02` "from 'node:sqlite'"
- Hooks carry the provider session id for resume: `src/main/agent-hooks/README.md:8@684d36274a02` "provider session id (resume and native chat)"

## (j) License / telemetry

- MIT: `LICENSE:1@684d36274a02` "MIT License" / `LICENSE:3@684d36274a02` "Copyright (c) 2026 Lovecast Inc."
- Telemetry: PostHog client with consent; env opt-outs `DO_NOT_TRACK`, `ORCA_TELEMETRY_DISABLED`, CI: `package.json:249@684d36274a02` "\"posthog-node\": \"^5.52.5\"" / `src/main/telemetry/consent.ts:53@684d36274a02` "isEnvVarTruthy('DO_NOT_TRACK')" / `src/main/telemetry/consent.ts:56@684d36274a02` "isEnvVarTruthy('ORCA_TELEMETRY_DISABLED')". Default consent state not determined from this read (banner-based; `consent.ts:6` mentions a "pending-banner" reason).
