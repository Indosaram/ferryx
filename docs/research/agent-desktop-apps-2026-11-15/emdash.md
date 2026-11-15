# Emdash - static source analysis (todo 15)

- Repo: https://github.com/generalaction/emdash, shallow clone at `ccc6cce4a41627deb6888db6763f3de5ff0e4e08` (cited as `@ccc6cce4a416`) in `/Volumes/T9-Mac/project/ferryx-wt/.evidence/session-01a1d86e-continuation/src/emdash`.
- Evidence route: OSS_REPO (also PUBLIC_BINARY v1.2.11, see todo 16). Scope is the open-source desktop app ("Emdash Classic" on the site); the vendor's team/cloud product is not in this repo.
- Citation form: `path:line@sha` followed by a quoted substring of that line; verify with `bun verify-citations.mjs apps/emdash.md --repo <clone>`.
- Method: `git grep` over the clone, tests/fixtures excluded; nothing was installed, built or run.

## (a) Stack / runtime

- Electron + React + TypeScript (Nx/pnpm monorepo), plus a separate `workspace-server` daemon for local or SSH-remote hosts: `apps/emdash-desktop/package.json:120@ccc6cce4a416` "\"electron\": \"40.10.2\"" and `apps/workspace-server/src/index.ts:13@ccc6cce4a416` "import { startDaemon } from './daemon/start';"
- Version at the SHA: `apps/emdash-desktop/package.json:3@ccc6cce4a416` "\"version\": \"1.2.11\""
- Product statement: `README.md:21@ccc6cce4a416` "Emdash is a desktop app for running AI coding agents in parallel."
- Size (measured with `git ls-files` at the SHA, not a citation): 5,459 tracked files; 4,084 `.ts`, 836 `.tsx`; 597,543 lines across `.ts/.tsx/.rs/.swift/.kt/.go`.

## (b) Agent integration

- Two runtimes: TUI agents (CLI in a PTY, status via hooks) and ACP agents (structured): `packages/core/src/runtimes/tui-agents/node/hooks/hook-server.ts:6@ccc6cce4a416` "export class TuiHookServer {" and `apps/emdash-desktop/package.json:52@ccc6cce4a416` "\"@agentclientprotocol/sdk\": \"^1.1.0\"" used in `packages/core/src/runtimes/acp/node/agent-ports/agent-client.ts:20@ccc6cce4a416` "} from '@agentclientprotocol/sdk';"
- Auto-detects installed CLIs; installs marker-tagged hooks in agent configs for status/notifications/resume: `README.md:53@ccc6cce4a416` "Emdash detects installed provider CLIs automatically." / `README.md:57@ccc6cce4a416` "For agents with lifecycle-hook support, Emdash installs marker-tagged entries in the agent's"
- Resume via each CLI's resume flag, declared per plugin: `packages/core/src/services/agent-plugins/api/plugins/helpers/standard-command.ts:31@ccc6cce4a416` "Flag for session resume, e.g. '--resume'."
- absent: Claude Agent SDK / Codex app-server; searched `claude-agent-sdk|app-server` in `apps/*/package.json`, `packages/*/package.json` - 0 hits.

## (c) Worktree / branch isolation

- One git worktree + branch per task: `README.md:33@ccc6cce4a416` "Keep every agent isolated in its own Git worktree and branch."
- Real `git worktree add` (existing branch, or `--no-track -b <branch>` from a base ref), with prune and forced removal on failure: `packages/core/src/runtimes/workspace-registry/node/create-worktree.ts:161@ccc6cce4a416` "exec.exec(['worktree', 'add', execution.worktreePath, execution.branch])" / `packages/core/src/runtimes/workspace-registry/node/create-worktree.ts:169@ccc6cce4a416` "'--no-track'," / `packages/core/src/runtimes/workspace-registry/node/create-worktree.ts:299@ccc6cce4a416` "['worktree', 'prune']"
- Worktree root is a per-host setting: `apps/emdash-desktop/src/core/features/machines/browser/components/host-settings-card.tsx:32@ccc6cce4a416` "const [worktreeRoot, setWorktreeRoot] = useState('');"

## (d) Terminal implementation

- `node-pty` in the core runtime, xterm.js 6 renderer: `apps/emdash-desktop/package.json:77@ccc6cce4a416` "\"node-pty\": \"1.1.0\"" / `packages/core/src/services/pty/api/node/node-pty-spawner.ts:41@ccc6cce4a416` "const proc = nodePty.spawn(invocation.executable, args, {" / `apps/emdash-desktop/package.json:113@ccc6cce4a416` "\"@xterm/xterm\": \"^6.0.0\""
- PTYs can live in the `workspace-server` daemon on a remote host reached over SSH (`ssh2`): `apps/emdash-desktop/package.json:81@ccc6cce4a416` "\"ssh2\": \"^1.17.0\""

## (e) Review / diff / PR flow

- Diff view with a changes panel (stage/discard) and create-PR modal: `apps/emdash-desktop/src/core/features/source-control/browser/diff-view/changes-panel/components/pr-entry/create-pr-modal.tsx:183@ccc6cce4a416` "await client.createPullRequest({"; PR engine: `apps/emdash-desktop/src/core/services/pull-requests/node/engine/pull-request-engine.ts:64@ccc6cce4a416` "client.data.createPullRequest(input, signal)"
- CI checks and merge from the app: `README.md:36@ccc6cce4a416` "Review diffs, create pull requests, inspect CI checks, and merge from one place."
- Issue intake from Linear, GitHub, Jira, GitLab, etc.: `README.md:34@ccc6cce4a416` "Send issues and tickets from Linear, GitHub, Jira, GitLab, Asana, Featurebase,"

## (f) Remote / mobile access and push

- Remote = the user's own machines over SSH/SFTP, running the workspace-server daemon: `README.md:37@ccc6cce4a416` "Work locally or on your own remote machines over SSH/SFTP."
- absent: mobile app and push notifications; searched `expo|apns|getExpoPushToken|push-notif` (case-insensitive) in `apps/**/*.ts(x)`, `packages/**/*.ts` - only build-config file hits, no mobile client or push sender.

## (g) Attention / notification model

- TUI agent state from hook events: `packages/core/src/runtimes/tui-agents/api/schemas.ts:106@ccc6cce4a416` "export const tuiAgentStateStatusSchema = z.enum([" with `packages/core/src/runtimes/tui-agents/api/schemas.ts:109@ccc6cce4a416` "'awaiting-input',"
- Attention = Claude-style notification types (permission, idle, elicitation): `packages/core/src/runtimes/tui-agents/node/runtime/agent-state.ts:13@ccc6cce4a416` "'permission_prompt'," / `packages/core/src/runtimes/tui-agents/node/runtime/agent-state.ts:14@ccc6cce4a416` "'idle_prompt',"
- Delivered as an in-app notification feed, sound sink and an Electron OS banner: `apps/emdash-desktop/src/main/host/notifications/system-notification-sink.ts:20@ccc6cce4a416` "const banner = new Notification({" and `apps/emdash-desktop/src/core/services/notifications/node/sinks/sound-sink.ts` (listing).

## (h) Checkpoint / rollback

- absent: per-turn checkpoints or rewind; searched `checkpoint|rewind|undo turn|revert` (case-insensitive) in `apps/emdash-desktop/src/core/features/conversations`, `packages/core/src/runtimes/conversations` - only a DB-row `revertUntrack` helper. Rollback is git-level only: irreversible discard of selected files in the changes panel: `apps/emdash-desktop/src/core/features/source-control/browser/diff-view/changes-panel/unstaged-section.tsx:102@ccc6cce4a416` "const handleDiscardSelection = () => {"

## (i) Persistence / session recovery

- SQLite via `better-sqlite3` + Drizzle; DB file `emdash4.db`: `apps/emdash-desktop/src/main/db/database-file.ts:3@ccc6cce4a416` "import Database from 'better-sqlite3';" / `apps/emdash-desktop/src/main/db/default-path.ts:5@ccc6cce4a416` "export const CURRENT_DB_FILENAME = 'emdash4.db';"
- Workspace-server runs as a pid-file daemon (`already-running` vs `started`), so remote sessions outlive the desktop connection: `apps/workspace-server/src/daemon/start.ts:11@ccc6cce4a416` "status: 'already-running' | 'started';"
- Agent sessions resumed via stored provider session ids (axis b).

## (j) License / telemetry

- Apache-2.0: `LICENSE.md:1@ccc6cce4a416` "Copyright 2026 General Action, Inc." / `LICENSE.md:4@ccc6cce4a416` "Apache License"
- Telemetry ON by default (PostHog), off when the env value is `false/0/no` or the user opts out; disabled in Vite dev builds: `apps/emdash-desktop/src/main/bootstrap/core/config.ts:39@ccc6cce4a416` "const normalized = (value ?? 'true').toLowerCase();" / `apps/emdash-desktop/src/main/lib/telemetry.ts:63@ccc6cce4a416` "this.userOptOut !== true &&" / `apps/emdash-desktop/src/main/lib/telemetry.ts:232@ccc6cce4a416` "private async posthogCapture("
