# T3 Code - static source analysis (todo 15)

- Repo: https://github.com/pingdotgg/t3code, shallow clone at `256dce7f1f6c6e051c0b5b432376b196e70385d8` (cited as `@256dce7f1f6c`) in `/Volumes/T9-Mac/project/ferryx-wt/.evidence/session-01a1d86e-continuation/src/t3code`.
- Evidence route: OSS_REPO (also PUBLIC_BINARY v0.1.9, see todo 16).
- Citation form: `path:line@sha` followed by a quoted substring of that line; verify with `bun verify-citations.mjs apps/t3code.md --repo <clone>`.
- Method: `git grep` over the clone, tests/fixtures excluded; nothing was installed, built or run.

## (a) Stack / runtime

- Electron desktop shell around a Node server + React web app (monorepo `apps/desktop`, `apps/server`, `apps/web`, `apps/mobile`): `apps/desktop/package.json:25@256dce7f1f6c` "\"electron\": \"44.10.1\""
- Desktop version at the SHA: `apps/desktop/package.json:3@256dce7f1f6c` "\"version\": \"0.1.9\""
- Mobile app is Expo/React Native (uses `expo-notifications`): `apps/mobile/package.json:110@256dce7f1f6c` "\"expo-notifications\": \"~58.1.8\""
- Size (measured with `git ls-files` at the SHA, not a citation): 32,361 tracked files; 25,178 `.ts`, 1,884 `.tsx`; 5,531,466 lines across `.ts/.tsx/.rs/.swift/.kt/.go` (incl. tests and generated code). Effect-TS is the core framework (`effect/...` imports throughout).

## (b) Agent integration

- SDK/protocol-first, not PTY scraping. Claude via the Claude Agent SDK: `apps/server/package.json:39@256dce7f1f6c` "\"@anthropic-ai/claude-agent-sdk\"" imported in `apps/server/src/provider/ClaudeProvider.ts:37@256dce7f1f6c` "from \"@anthropic-ai/claude-agent-sdk\""
- Codex via the Codex `app-server` JSON-RPC protocol (own `effect-codex-app-server` package): `apps/server/src/provider/CodexProvider.ts:19@256dce7f1f6c` "import * as CodexClient from \"effect-codex-app-server/client\""; install probe runs `codex app-server --help`: `apps/server/src/provider/CodexInstallation.ts:504@256dce7f1f6c` "[\"app-server\", \"--help\"]"
- Cursor via its SDK: `apps/server/package.json:40@256dce7f1f6c` "\"@cursor/sdk\": \"1.3.1\""
- ACP (Agent Client Protocol) for other agents (Grok, Kiro, fx, ACP registry), spawned as child processes: `apps/server/package.json:46@256dce7f1f6c` "\"@t3tools/provider-acp\": \"workspace:*\"" and `packages/provider-acp/src/server/AcpSessionRuntime.ts:26@256dce7f1f6c` "import * as ChildProcessSpawner from \"effect/process/ChildProcessSpawner\""
- Provider packages present in `packages/`: provider-acp, provider-acp-registry, provider-cursor, provider-fx, provider-grok, provider-kiro, provider-muse, provider-opencode, provider-pi (directory listing at the SHA).
- Session resume is passed to the SDK, not a CLI flag: `apps/server/src/orchestration-v2/Adapters/ClaudeAdapterV2.ts:2195@256dce7f1f6c` "? { resume: input.nativeThreadId }"

## (c) Worktree / branch isolation

- Threads can run in a git worktree strategy anchored to a base commit: `apps/server/src/mcp/OrchestratorMcpService.ts:2724@256dce7f1f6c` "workspaceStrategy = { type: \"worktree\", baseRef: base.commitSha"
- Real `git worktree add` (optionally `--lock`): `apps/server/src/vcs/GitVcsDriverCore.ts:5134@256dce7f1f6c` "\"worktree\"," / `apps/server/src/vcs/GitVcsDriverCore.ts:5135@256dce7f1f6c` "\"add\"," / `apps/server/src/vcs/GitVcsDriverCore.ts:5136@256dce7f1f6c` "\"--lock\", \"--reason\", lockReason"
- Worktree root is a user setting with migration of previous roots: `apps/server/src/worktreesDirectory.ts:40@256dce7f1f6c` "settings.worktreesDirectory, ...settings.previousWorktreesDirectories"
- Prune: `apps/server/src/vcs/GitVcsDriverCore.ts:5836@256dce7f1f6c` "[\"worktree\", \"prune\"]"

## (d) Terminal implementation

- Server PTYs via `node-pty`: `apps/server/package.json:69@256dce7f1f6c` "\"node-pty\": \"^1.2.0-beta.15\"" spawned at `apps/server/src/terminal/NodePtyAdapter.ts:267@256dce7f1f6c` "nodePty.spawn(input.shell"
- Renderer is libghostty-vt compiled to WASM (not xterm.js): `apps/web/package.json:7@256dce7f1f6c` "\"build:ghostty-wasm\"" and `apps/web/src/terminal/ghostty/README.md:3@256dce7f1f6c` "official `libghostty-vt` C ABI used by Android"; used by the thread terminal drawer `apps/web/src/components/ThreadTerminalDrawer.tsx:57@256dce7f1f6c` "from \"~/terminal/ghostty/surface\""
- The terminal is a side drawer to the SDK-driven chat thread, not the agent's own UI.

## (e) Review / diff / PR flow

- Dedicated review service (branch commits, diff file contents): `apps/server/src/review/ReviewService.ts:11@256dce7f1f6c` "type ReviewBranchCommitsInput"
- Diff rendering via `@pierre/diffs`: `apps/web/package.json:35@256dce7f1f6c` "\"@pierre/diffs\""
- Pull requests: create PR through provider (GitHub, GitLab, Bitbucket, Azure DevOps, Forgejo... packages under `packages/source-control-*`): `packages/source-control-github/src/server/GitHubSourceControlProvider.ts:1084@256dce7f1f6c` "operation: \"createPullRequest\""; PR checkout into a worktree: `apps/server/src/git/GitManager.ts:3318@256dce7f1f6c` "const createPullRequestWorktree"
- AI review guides per PR and viewed-files tracking persisted: `apps/server/src/pullRequest/PullRequestReviewGuideService.ts:39@256dce7f1f6c` "reviewGuideInputFromPullRequest"

## (f) Remote / mobile access and push

- Native mobile app (Expo) with an agent-awareness notification module: `apps/mobile/app.config.ts:359@256dce7f1f6c` "\"expo-notifications\","
- Push path: server publishes agent activity to a T3-hosted relay (`infra/relay`) that sends APNs directly: `apps/server/src/relay/AgentAwarenessRelay.ts:12@256dce7f1f6c` "RelayApi," and `infra/relay/src/agentActivity/ApnsClient.ts:281@256dce7f1f6c` "\"https://api.push.apple.com\""; APNs status flows back: `apps/server/src/relay/AgentAwarenessRelay.ts:252@256dce7f1f6c` "delivery.apnsStatus"
- Pairing links and device auth persisted server-side: `apps/server/src/persistence/AuthPairingLinks.ts:27@256dce7f1f6c` "export const AuthPairingLinkRecord"
- Tailscale integration package for remote reach: `packages/tailscale/src/tailscale.ts:15@256dce7f1f6c` "Bounds `tailscale status` and `tailscale debug prefs`"

## (g) Attention / notification model

- Structured phases from the SDK event stream, not screen scraping: `packages/shared/src/agentAwareness.ts:17@256dce7f1f6c` "export type AgentAwarenessPhase =" with `packages/shared/src/agentAwareness.ts:20@256dce7f1f6c` "\"waiting_for_approval\"" and `packages/shared/src/agentAwareness.ts:21@256dce7f1f6c` "\"waiting_for_input\""
- Subagent waits bubble up: `packages/shared/src/agentAwareness.ts:46@256dce7f1f6c` "export function subagentAwaitsUser("
- Desktop/web: HTML5 `Notification` with snooze logic: `apps/web/src/components/ThreadNotificationCoordinator.tsx:211@256dce7f1f6c` "new Notification(title" / `apps/web/src/components/ThreadNotificationCoordinator.logic.ts:28@256dce7f1f6c` "export function isSnoozedForNotification("

## (h) Checkpoint / rollback

- Per-turn git checkpoints stored as hidden refs (`refs/t3/checkpoints/<thread>/turn/...`) via a temp index + `update-ref`, so the user's branch and index are untouched: `apps/server/src/vcs/GitVcsDriver.ts:1724@256dce7f1f6c` "\"update-ref\", input.checkpointRef, commitOid" and `apps/server/src/storageCleanup.ts:1688@256dce7f1f6c` "refs/t3/checkpoints/"
- Capture per turn and restore: `apps/server/src/orchestration-v2/CheckpointService.ts:288@256dce7f1f6c` ".captureCheckpoint({" / `apps/server/src/orchestration-v2/CheckpointService.ts:539@256dce7f1f6c` "checkpointStore.restoreCheckpoint({"
- Checkpoint diffs are queryable: `apps/server/src/checkpointing/CheckpointDiffQuery.ts` exists (listing).

## (i) Persistence / session recovery

- SQLite (node:sqlite) event store with migrations: `apps/server/src/persistence/Sqlite.ts:27@256dce7f1f6c` "node:sqlite reports extended codes" and `apps/server/src/persistence/OrchestrationEventStore.ts` (event-sourced orchestration, listing).
- Provider session runtime persisted for resume: `apps/server/src/persistence/ProviderSessionRuntime.ts` (listing) + SDK `resume` (axis b).
- Terminal history written per thread/terminal to disk: `apps/server/src/terminal/Manager.ts:1939@256dce7f1f6c` "const historyPath = (threadId: string, terminalId: string)"

## (j) License / telemetry

- MIT: `LICENSE:1@256dce7f1f6c` "MIT License" / `LICENSE:3@256dce7f1f6c` "Copyright (c) 2026 T3 Tools Inc."
- Telemetry ON by default, PostHog via T3 ingest, opt-out env `T3CODE_TELEMETRY_ENABLED`: `apps/server/src/telemetry/AnalyticsService.ts:1@256dce7f1f6c` "forwards to PostHog" and `apps/server/src/telemetry/AnalyticsService.ts:74@256dce7f1f6c` "Config.Boolean(\"T3CODE_TELEMETRY_ENABLED\").pipe(Config.withDefault(true))"
