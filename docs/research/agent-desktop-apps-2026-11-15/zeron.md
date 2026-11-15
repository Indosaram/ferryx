# Zeron - static source analysis (todo 15)

- Repo: https://github.com/zeronsh/zeron, shallow clone at `68e460af252ddfb295a4d951012f5d5d7c2568af` (cited as `@68e460af252d`) in `/Volumes/T9-Mac/project/ferryx-wt/.evidence/session-01a1d86e-continuation/src/zeron`.
- Evidence route: OSS_REPO (also PUBLIC_BINARY v0.3.6, see todo 16).
- Citation form: `path:line@sha` followed by a quoted substring of that line; verify with `bun verify-citations.mjs apps/zeron.md --repo <clone>`.
- Method: `git grep` over the clone, tests/fixtures excluded; nothing was installed, built or run.

## (a) Stack / runtime

- Native Rust app with a gpui UI (Zed's GPU UI framework, forked as `zeronsh/zui`), no Electron or Tauri: `ARCHITECTURE.md:4@68e460af252d` "in Rust, with a gpui UI" and `Cargo.toml:119@68e460af252d` "gpui = { git = \"https://github.com/zeronsh/zui\""
- Split into a headless engine daemon + UI viewport; the app spawns `zeron-engine` detached and reconnects: `ARCHITECTURE.md:25@68e460af252d` "**Engine = backend**" and `apps/zeron/Cargo.toml:19@68e460af252d` "name = \"zeron-engine\""
- Cloud side is TypeScript Cloudflare Workers + Durable Objects (`edge/`): `ARCHITECTURE.md:8@68e460af252d` "Durable Objects stay **TypeScript**"
- Native iOS app in Swift (`apps/ios`), Android app (`apps/android`).
- Version at the SHA: `Cargo.toml:41@68e460af252d` "version = \"0.3.6\""
- Size (measured with `git ls-files` at the SHA, not a citation): 5,859 tracked files; 990 `.rs`; 878,729 lines across `.ts/.tsx/.rs/.swift/.kt/.go`.

## (b) Agent integration

- Structured protocols per harness, no PTY scraping. Claude Code: `claude` CLI in `--print` stream-json in/out mode: `crates/harness/src/claude/mod.rs:2@68e460af252d` "stream-json protocol directly" and `crates/harness/src/claude/mod.rs:436@68e460af252d` "\"stream-json\","
- Codex via `codex app-server` JSON-RPC: `crates/harness/src/codex/mod.rs:279@68e460af252d` "cmd.arg(\"app-server\");"
- ACP for Grok, Devin, Hermes, fx, Antigravity: `crates/harness/src/acp/mod.rs:5@68e460af252d` "`grok agent stdio`), Devin" and `crates/harness/src/acp/mod.rs:6@68e460af252d` "`hermes acp`), fx"
- Further harness modules in `crates/harness/src/`: cursor, opencode, pi, muse (directory listing at the SHA).
- Resume handled through the CLI's `--resume`: `crates/harness/src/claude/mod.rs:167@68e460af252d` "The CLI's answer to `--resume`"

## (c) Worktree / branch isolation

- Managed worktrees under `~/.zeron/worktrees/<repo>/<name>` on `zeron/<name>` branches (root configurable, `$ZERON_WORKTREES_DIR`): `crates/engine/src/repos.rs:7@68e460af252d` "~/.zeron/worktrees/<repoName>/<worktreeName>" and `crates/engine/src/repos.rs:1475@68e460af252d` "let branch_name = format!(\"zeron/{name}\");"
- Real `git worktree add -b`: `crates/engine/src/repos.rs:1482@68e460af252d` "\"worktree\"," / `crates/engine/src/repos.rs:1483@68e460af252d` "\"add\","; removal `crates/engine/src/repos.rs:1562@68e460af252d` "[\"worktree\", \"remove\", \"--force\", &git_path]"
- Device-local worktree destination settings: `crates/engine/src/worktree_settings.rs:1@68e460af252d` "Device-local worktree destinations."

## (d) Terminal implementation

- `portable-pty` + `alacritty_terminal` (vte), rendered natively in gpui: `Cargo.toml:173@68e460af252d` "portable-pty = \"0.8\"" / `Cargo.toml:174@68e460af252d` "alacritty_terminal = \"0.26\"" and `crates/term/src/emulator.rs:1@68e460af252d` "The terminal emulator core: `alacritty_terminal`'s `Term`"
- PTY sessions owned by the engine, survive subscriber detach, 1 MB replay window: `crates/engine/src/terminals.rs:4@68e460af252d` "`subscribe` replays a" / `crates/engine/src/terminals.rs:5@68e460af252d` "bounded 1MB window (resumable via `afterSeq`)"
- The experimental `apps/zgx` uses Ghostty on macOS: `apps/zgx/Cargo.toml:161@68e460af252d` "on macOS, Ghostty draws the panel's terminals"
- The terminal is a side panel; the agent itself is rendered as a structured chat.

## (e) Review / diff / PR flow

- Right-pane "Changes" unified-diff viewer over watched checkout diffs: `crates/ui/src/changes.rs:1@68e460af252d` "a unified-diff"
- Per-turn working-tree snapshots ("Latest turn" scope) kept outside the repo's object DB: `crates/engine/src/diff_sync/turn_snapshot.rs:1@68e460af252d` "Working-tree snapshots for the \"Latest turn\" scope"
- PRs: reads PR list/review/checks via `gh pr view` and GitHub GraphQL, cached in SQLite: `crates/engine/src/source_control.rs:1172@68e460af252d` "`gh pr view" and `crates/engine/src/change_request_store.rs:275@68e460af252d` "CREATE TABLE IF NOT EXISTS pull_requests ("
- PR actions are handed to an agent as a staged prompt with `gh` commands, rather than a native create-PR call: `crates/ui/src/pull_request_handoff.rs:1@68e460af252d` "Hand a pull request to an agent"
- absent: native "create pull request" API call; searched `createPullRequest|create_pull_request|"pr", "create"` in `crates/` - 0 hits.

## (f) Remote / mobile access and push

- Multi-device by design: engines link through a Cloudflare DeviceRoom Durable Object relay: `ARCHITECTURE.md:19@68e460af252d` "DeviceRoom DO relay"
- `edge/src/index.ts:87@68e460af252d` "import { DeviceRoom } from \"./device-room\";"
- Native iOS app registers its APNs token with the edge, which decides and sends pushes: `apps/ios/Zeron/App/PushNotifications.swift:5@68e460af252d` "The edge decides and sends them (APNs)" and `edge/src/apns.ts:27@68e460af252d` "production: \"https://api.push.apple.com\","
- Hosted Cloud sandbox device per session (WorkOS auth, `zeron-vault`): `ARCHITECTURE.md:33@68e460af252d` "**Cloud**"

## (g) Attention / notification model

- Four-state session indicator from structured harness events with a 45 s staleness cutoff: `crates/proto/src/view.rs:31@68e460af252d` "pub enum Indicator {" / `crates/proto/src/view.rs:34@68e460af252d` "AwaitingInput," / `crates/proto/src/view.rs:41@68e460af252d` "pub const SESSION_STALE_MS: i64 = 45_000;"
- Engine posts desktop banners (finished, waiting on you, failed, connection lost) even with the window closed; "background only" preference: `crates/engine/src/desktop_notify.rs:1@68e460af252d` "Desktop banners, posted by the engine"
- `crates/engine/src/desktop_notify.rs:2@68e460af252d` "window closed: a run finished, an agent is waiting on you, a run failed," and `crates/notify/src/lib.rs:5@68e460af252d` "macOS: `NSUserNotification` through the ObjC runtime"
- Sidebar is attention-sorted: `ARCHITECTURE.md:27@68e460af252d` "an attention-sorted Sessions list"

## (h) Checkpoint / rollback

- Turn snapshots (`git add -A` into a throwaway index + `write-tree`, separate object dir): `crates/engine/src/diff_sync/turn_snapshot.rs:4@68e460af252d` "A snapshot is `git add -A` into a throwaway index"
- Discard restores tracked paths to the snapshot's HEAD and removes reported untracked paths: `crates/engine/src/diff_sync.rs:1857@68e460af252d` "Restore staged and unstaged tracked paths to the snapshot's HEAD" / `crates/engine/src/diff_sync.rs:1861@68e460af252d` "pub async fn discard_working_tree("
- absent: per-turn rewind to an arbitrary earlier turn; searched `revert.*turn|restore.*turn|undo_turn|rollback` in `crates/engine/src`, `crates/proto/src` - only the discard above and a chat2 migration rollback copy.

## (i) Persistence / session recovery

- Loro CRDT docs persisted locally in SQLite (`{data_dir}/docs.sqlite3`), optionally synced via ChatRoom DOs: `crates/sync/src/store.rs:180@68e460af252d` "SQLite-backed store under a data directory (`{data_dir}/docs.sqlite3`)." and `ARCHITECTURE.md:7@68e460af252d` "Optional sync uses Loro CRDT docs"
- Engine outlives the app window; app reconnects and restarts a lost engine with backoff: `ARCHITECTURE.md:50@68e460af252d` "Closing or quitting the app leaves it running with every session"
- Agent resume via CLI resume (axis b); terminals replay from a bounded buffer (axis d).

## (j) License / telemetry

- MIT: `LICENSE:1@68e460af252d` "MIT License" / `LICENSE:3@68e460af252d` "Copyright (c) 2026 Wing"
- Telemetry: absent: searched `posthog|sentry|mixpanel|amplitude|segment.io|analytics` (case-insensitive) in `crates/`, `apps/zeron`, `apps/ios`, `edge/src` - no product-analytics client; hits are unrelated identifiers (reserved usernames, auth cache entries). Server-side Workers Logs exist on the edge: `edge/src/session-room.ts:970@68e460af252d` "Cold-start telemetry (Workers Logs + /stats)"
