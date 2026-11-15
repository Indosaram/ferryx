# MonoCode - static source analysis (todo 15)

- Repo: https://github.com/hardbeat920/monocode, shallow clone at `09d04436babb39ad72198cdf22f735067027fc04` (cited as `@09d04436babb`) in `/Volumes/T9-Mac/project/ferryx-wt/.evidence/session-01a1d86e-continuation/src/monocode`.
- Evidence route: OSS_REPO (also PUBLIC_BINARY v0.16.0 Tauri + native-v0.16.0-preview.14 GPUI, see todo 16).
- Citation form: `path:line@sha` followed by a quoted substring of that line; verify with `bun verify-citations.mjs apps/monocode.md --repo <clone>`.
- Method: `git grep` over the clone, tests/fixtures excluded; nothing was installed, built or run.

## (a) Stack / runtime

- Two desktop lines in one repo: the shipping Tauri v2 app (React + Rust) and an experimental native Rust + GPUI app (`crates/monocode-gpui`): `src-tauri/Cargo.toml:26@09d04436babb` "tauri = { version = \"2\"" and `crates/monocode-gpui/Cargo.toml:16@09d04436babb` "gpui = { package = \"gpui-pre\", version = \"=0.3.9\"" / `README.md:12@09d04436babb` "our experimental Rust + GPUI app for macOS, Linux and Windows"
- A Node "host" package (`host/`) runs sessions on remote machines; mobile is a separate Expo app (`mobile/`): `README.md:53@09d04436babb` "The mobile app is a separate Expo project"
- Version at the SHA: `package.json:4@09d04436babb` "\"version\": \"0.16.0\","
- Size (measured with `git ls-files` at the SHA, not a citation): 2,591 tracked files; 1,097 `.ts`, 707 `.rs`, 380 `.tsx`; 888,471 lines across `.ts/.tsx/.rs/.swift/.kt/.go`.

## (b) Agent integration

- Structured protocols per provider, spawned as child processes with piped stdio (no TUI scraping): Claude Code in stream-json mode: `src/integrations/harness/providers/claude/claudeProtocol.ts:256@09d04436babb` "\"stream-json\","; Codex via `codex app-server`: `src/integrations/harness/providers/codex/codex.ts:653@09d04436babb` "[\"app-server\"],"; Cursor via `cursor-agent acp`: `src/integrations/harness/providers/cursor/cursor.ts:308@09d04436babb` "[\"acp\"],"
- Same harness ported to Rust for the native line: `crates/monocode-core/src/provider.rs:1414@09d04436babb` "command.arg(\"app-server\");" / `crates/monocode-core/src/provider.rs:1140@09d04436babb` "if kind == \"acp\" {"
- Supported agents per README: `README.md:14@09d04436babb` "Works with your subscriptions on Claude Code, Codex, Cursor"
- Resume through the CLI flag: `src/integrations/harness/providers/claude/claudeProtocol.ts:291@09d04436babb` "if (input.resume) args.push(\"--resume\", input.resume);"

## (c) Worktree / branch isolation

- Real `git worktree add` (existing branch or new `-b` from a commit): `host/git-worktrees.ts:188@09d04436babb` "[\"worktree\", \"add\", \"--\", path, branch]" / `host/git-worktrees.ts:192@09d04436babb` "[\"worktree\", \"add\", \"--no-track\", \"-b\", branch, \"--\", path, commit]"
- Default root is a sibling `<repo>-worktrees` directory: `src-tauri/src/worktrees.rs:127@09d04436babb` "main.with_file_name(format!(\"{name}-worktrees\"))"
- Worktree lifecycle module: `src-tauri/src/worktree_lifecycle.rs` (listing).

## (d) Terminal implementation

- Tauri line: `portable-pty` (vendored) + xterm.js 6: `src-tauri/Cargo.toml:54@09d04436babb` "portable-pty = { version = \"0.9\", path = \"../vendor/portable-pty\" }" and `package.json:70@09d04436babb` "\"@xterm/xterm\": \"^6.0.0\""; view `src/features/terminal/ui/TerminalView.tsx:1@09d04436babb` "@xterm/addon-web-links"
- Native line: `alacritty_terminal` grid: `crates/monocode-gpui/Cargo.toml:25@09d04436babb` "alacritty_terminal = \"0.26\""
- Terminal is a side tool; agent sessions render as chat tabs.

## (e) Review / diff / PR flow

- Per-turn change recordings back a review card of files the session's structured edit tools touched: `src-tauri/src/checkpoint/turn.rs:1@09d04436babb` "Git-backed recordings of workspace changes during a turn"
- Tool-level diff previews: `src/features/sessions/ui/ToolDiffPreview.tsx` (listing). GitHub PR diffs in an inbox: `src/features/inbox/ui/InboxPrDiff.tsx:3@09d04436babb` "parsePrPatch"
- PR creation shells out to `gh pr create`: `host/workspace.ts:828@09d04436babb` "exec(\"gh\", [\"pr\", \"create\", \"--fill\"]" / `host/workspace-commands.ts:607@09d04436babb` "[\"pr\", \"create\", \"--title\""

## (f) Remote / mobile access and push

- Phone pairs directly with the desktop over mutually authenticated QUIC on a fixed port; no vendor relay: `crates/mobile-link/src/lib.rs:1@09d04436babb` "Direct, mutually authenticated QUIC used by desktop and the Expo native module." / `crates/mobile-link/src/lib.rs:26@09d04436babb` "pub const PORT: u16 = 38416;"
- The phone controls only through the owning window, no raw command access: `src-tauri/src/mobile_access.rs:1@09d04436babb` "A paired phone controls the workspace through its owning app window."
- Push via Expo's push service, sent by the desktop itself, per category (approvals/questions, finished...): `crates/monocode-core/src/mobile/notify.rs:1@09d04436babb` "sent through Expo's push service" / `crates/monocode-core/src/mobile/notify.rs:7@09d04436babb` "const ENDPOINT: &str = \"https://exp.host/--/api/v2/push/send\";" and `mobile/src/features/notifications/SessionNotifications.tsx:123@09d04436babb` "Notifications.getExpoPushTokenAsync({"
- Remote hosts over SSH: the desktop installs a host package as a background service and tunnels to it; sessions survive SSH loss: `docs/remote-access.md:3@09d04436babb` "losing the SSH tunnel does not stop a host session"

## (g) Attention / notification model

- Desktop notifications for turns that end or stall while the window is in the background (macOS `UNUserNotificationCenter`, click jumps to the session): `src-tauri/src/notifications.rs:1@09d04436babb` "Desktop notifications for turns that end or stall while the window is in" / `src-tauri/src/notifications.rs:4@09d04436babb` "macOS goes through `UNUserNotificationCenter` directly"
- Per-category preferences shared with mobile push: `src/features/notifications/model/notificationPreferences.ts:5@09d04436babb` "{ id: \"agentInput\", label: \"Agent approvals and questions\" },"

## (h) Checkpoint / rollback

- Per-turn git checkpoints under hidden refs `refs/monocode/checkpoints/<session>/`, with undo of a turn: `src-tauri/src/checkpoint/turn.rs:236@09d04436babb` "refs/monocode/checkpoints/{session_id}/" / `src-tauri/src/checkpoint/turn.rs:442@09d04436babb` "pub(super) fn undo_turn("
- Undo boundary is the first tool-start event; some changes are review-only: `src-tauri/src/checkpoint.rs:111@09d04436babb` "The first tool-start event owns the safe undo boundary." / `src-tauri/src/checkpoint.rs:178@09d04436babb` "deliberately not undoable"

## (i) Persistence / session recovery

- SQLite (WAL) session store: `src-tauri/src/session_store.rs:42@09d04436babb` "let conn = Connection::open(&path)" / `src-tauri/src/session_store.rs:43@09d04436babb` "PRAGMA journal_mode = WAL;"
- Remote host owns provider processes and session DB, so sessions outlive the desktop: `docs/remote-access.md:3@09d04436babb` "The host owns the provider processes and session database."
- Agent conversation resume via CLI `--resume` (axis b).

## (j) License / telemetry

- MIT: `LICENSE:1@09d04436babb` "MIT License" / `LICENSE:3@09d04436babb` "Copyright (c) 2026 Nick"
- Telemetry: absent: searched `posthog|sentry|aptabase|mixpanel|amplitude` (case-insensitive) in `src`, `src-tauri/src`, `crates`, `host`, `mobile/src`, `package.json` - hits are only unrelated substrings (e.g. `FsEntry` matching `sentry`). Expo push routes notification payloads through Expo's servers (axis f).
