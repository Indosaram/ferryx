# Paseo - static source analysis (todo 15)

- Repo: https://github.com/getpaseo/paseo, shallow clone at `1c175b36b3cdf139c048b6eba8101e170282e0df` (cited as `@1c175b36b3cd`) in `/Volumes/T9-Mac/project/ferryx-wt/.evidence/session-01a1d86e-continuation/src/paseo`.
- Evidence route: OSS_REPO (also PUBLIC_BINARY v0.15.4, see todo 16). Note: repo HEAD package versions read `0.16.0-beta.2`, ahead of the 0.15.4 binary.
- Citation form: `path:line@sha` followed by a quoted substring of that line; verify with `bun verify-citations.mjs apps/paseo.md --repo <clone>`.
- Method: `git grep` over the clone, tests/fixtures excluded; nothing was installed, built or run.

## (a) Stack / runtime

- Electron desktop shell + Node daemon (`packages/server`) + one Expo/React Native app (`packages/app`) for iOS, Android and web: `packages/desktop/package.json:43@1c175b36b3cd` "\"electron\": \"44.2.0\"" and `packages/app/package.json:102@1c175b36b3cd` "\"expo-notifications\": \"~57.0.23\""
- Version at the SHA: `packages/desktop/package.json:3@1c175b36b3cd` "\"version\": \"0.16.0-beta.2\""
- Product description includes voice control: `package.json:5@1c175b36b3cd` "voice-controlled development environment for local AI coding agents"
- Size (measured with `git ls-files` at the SHA, not a citation): 6,850 tracked files; 4,478 `.ts`, 909 `.tsx`; 1,288,543 lines across `.ts/.tsx/.rs/.swift/.kt/.go`.

## (b) Agent integration

- SDK/protocol adapters per provider, no PTY scraping of agent TUIs. Claude via the Claude Agent SDK `query()`: `packages/server/package.json:110@1c175b36b3cd` "\"@anthropic-ai/claude-agent-sdk\": \"0.3.246\"" and `packages/server/src/server/agent/providers/claude/query.ts:2@1c175b36b3cd` "import { query, type Options"
- Codex via `codex app-server`: `packages/server/src/server/agent/providers/codex-app-server-agent.ts:7836@1c175b36b3cd` "\"app-server\"]"
- ACP via the official SDK, used for Copilot, Cursor, Kimi, Kiro, Trae and generic ACP agents (`*-acp-agent.ts` files in `packages/server/src/server/agent/providers/`): `packages/server/package.json:109@1c175b36b3cd` "\"@agentclientprotocol/sdk\": \"^0.17.1\"" / `packages/server/src/server/agent/providers/acp-agent.ts:65@1c175b36b3cd` "from \"@agentclientprotocol/sdk\""
- OpenCode via its server/bridge (`opencode-agent.ts`), plus `pi` and `omp` provider dirs (listing).

## (c) Worktree / branch isolation

- Real `git worktree add` with a new `-b <branch> --no-track <base>`: `packages/server/src/utils/worktree.ts:1391@1c175b36b3cd` "[\"worktree\", \"add\", worktreePath, ...sourcePlan.addArguments]" and `packages/server/src/utils/worktree.ts:1506@1c175b36b3cd` "addArguments: [\"-b\", branchName, \"--no-track\", baseRef]"
- Paseo-managed worktrees are tagged with metadata so foreign worktrees are left alone: `packages/server/src/utils/checkout-git.ts:1148@1c175b36b3cd` "hasPaseoWorktreeMetadata(wt.path)"
- Data root `~/.paseo` (overridable `PASEO_HOME`): `packages/server/src/server/paseo-home.ts:15@1c175b36b3cd` "env.PASEO_HOME ?? \"~/.paseo\""
- Archive-on-merge automation exists (`packages/server/src/server/auto-archive-on-merge/`, listing).

## (d) Terminal implementation

- Daemon PTYs via `node-pty`, each mirrored in `@xterm/headless` for serialize/restore: `packages/server/package.json:135@1c175b36b3cd` "\"node-pty\": \"1.2.0-beta.15\"" / `packages/server/src/terminal/terminal.ts:823@1c175b36b3cd` "pty.spawn(spawnCommand, spawnArgs" / `packages/server/src/terminal/headless-terminal.ts:2@1c175b36b3cd` "import xterm, { type Terminal } from \"@xterm/headless\";"
- Client renderer xterm.js 6 (WebGL addon) on desktop/web, with a separate native grid renderer on mobile: `packages/app/package.json:82@1c175b36b3cd` "\"@xterm/xterm\": \"6.1.0-beta.336\"" / `packages/app/package.json:81@1c175b36b3cd` "\"@xterm/addon-webgl\""
- PTYs run in a worker process pool (`terminal-worker-pool.ts`, listing); restore ships bounded scrollback: `packages/server/src/terminal/terminal-restore.ts:29@1c175b36b3cd` "scrollbackLines: Math.min("

## (e) Review / diff / PR flow

- Checkout diff manager streams `checkout_diff_update` to clients: `packages/server/src/server/checkout-diff-manager.ts:23@1c175b36b3cd` "type: \"checkout_diff_update\""
- Native create-PR through a forge abstraction (GitHub, Gitea...): `packages/server/src/services/forge-service.ts:488@1c175b36b3cd` "createPullRequest(options: CreatePullRequestOptions)" called from `packages/server/src/server/session/checkout/checkout-session.ts:976@1c175b36b3cd` "await createPullRequest("

## (f) Remote / mobile access and push

- Hosted relay by default, end-to-end encrypted with Noise NK between client and daemon: `packages/server/src/server/config.ts:39@1c175b36b3cd` "const DEFAULT_RELAY_ENDPOINT = \"relay.paseo.sh:443\";" and `packages/relay/src/secure-channel.ts:12@1c175b36b3cd` "Noise_NK_25519_ChaChaPoly_SHA256"
- QR pairing offers (`pairing-qr.ts`, `pairing-offer.ts`, listing).
- Push via Expo push service (not direct APNs): `packages/server/src/server/push/push-service.ts:24@1c175b36b3cd` "const EXPO_PUSH_URL = \"https://exp.host/--/api/v2/push/send\";"; the app fetches an Expo push token: `packages/app/src/push-notifications/internal/subscriptions.ts:52@1c175b36b3cd` "Notifications.getExpoPushTokenAsync({ projectId })"

## (g) Attention / notification model

- Three attention reasons from structured agent events: `packages/protocol/src/agent-attention-notification.ts:3@1c175b36b3cd` "export type AgentAttentionReason = \"finished\" | \"error\" | \"permission\";"
- Presence-aware routing: a focused, visible client on the same agent suppresses the notification; otherwise in-app to one client or push if no client present (3-minute presence window): `packages/server/src/server/agent-attention-policy.ts:3@1c175b36b3cd` "export const PRESENCE_THRESHOLD_MS = 180_000;" / `packages/server/src/server/agent-attention-policy.ts:21@1c175b36b3cd` "A present, app-visible client focused on the attention target suppresses the" / `packages/server/src/server/agent-attention-policy.ts:42@1c175b36b3cd` "export function computeNotificationPlan({"
- Client-side controller: `packages/app/src/utils/agent-attention.ts:5@1c175b36b3cd` "attentionReason: \"finished\" | \"error\" | \"permission\""

## (h) Checkpoint / rollback

- Rewind per message in three modes (conversation, files, both) when the provider supports it: `packages/server/src/server/agent/rewind/rewind.ts:3@1c175b36b3cd` "export type RewindMode = \"conversation\" | \"files\" | \"both\";"
- Claude file rewind uses the SDK's file checkpointing: `packages/server/src/server/agent/providers/claude/agent.ts:635@1c175b36b3cd` "enableFileCheckpointing: options.enableFileCheckpointing === true" / `packages/server/src/server/agent/providers/claude/agent.ts:3189@1c175b36b3cd` "async revertFiles(input: { messageId: string })"
- Workspace archive has an undo path (`workspace-archive-undo.e2e.test.ts`, listing).

## (i) Persistence / session recovery

- Agents stored as JSON records under the daemon's data dir: `packages/server/src/server/agent/agent-storage.ts:478@1c175b36b3cd` "entry.name.endsWith(\".json\")"
- Restart recovery re-dispatches a resume prompt to agents that were running when the daemon stopped: `packages/server/src/server/agent/agent-storage.ts:62@1c175b36b3cd` "resumeOnStartup: z.boolean().optional()," and `packages/server/src/server/agent/restart-recovery/index.ts:10@1c175b36b3cd` "Candidates are chosen at startup"
- Terminals restore from the headless xterm mirror (axis d).

## (j) License / telemetry

- Apache-2.0 for Paseo's own code, third-party parts under their own licenses: `LICENSE:7@1c175b36b3cd` "Except for those third-party components, Paseo is licensed under the" / `LICENSE:8@1c175b36b3cd` "Apache License, Version 2.0"
- Telemetry: absent: searched `telemetry|posthog|sentry|analytics` in `packages/server/src`, `packages/desktop/src`, `packages/app/src/app` - no analytics client; only code comments about provider "telemetry" events. The hosted relay (`relay.paseo.sh`) is a default network dependency, E2E-encrypted.
