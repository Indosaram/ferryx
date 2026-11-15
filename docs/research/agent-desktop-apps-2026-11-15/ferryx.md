# Ferryx baseline on the ten comparison axes

- Source: committed `origin/main` of `/Volumes/T9-Mac/project/ferryx`, SHA `6c12a7aeac304c8e0a23f7fe3f1379409c9dac16` (cited below as `@6c12a7aeac30`). Read with `git show <sha>:<path>`; nothing below comes from the shared dirty tree.
- Evidence route: OSS_REPO (own repo, source-available license, see axis j).
- Citation form: `path:line@sha` followed by a quoted substring of that line; `verify-citations.mjs` re-opens each one.
- Axes (a)-(j) are those of plan todo 15.

## (a) Stack / runtime

- Native Rust on Tauri v2, no Electron; the README states the terminal core is libghostty: `README.md:13@6c12a7aeac30` "with a libghostty terminal core; no Electron."
- Tauri 2 dependency: `src-tauri/Cargo.toml:123@6c12a7aeac30` "tauri = { version = \"2\""
- App version on main: `src-tauri/Cargo.toml:3@6c12a7aeac30` "version = \"2026.1115.1\""
- Default features ship the remote desktop, Slack/Discord/voice hub integrations: `src-tauri/Cargo.toml:59@6c12a7aeac30` "default = [\"desktop\", \"hub-slack\", \"hub-discord\", \"hub-voice\", \"remote-desktop\"]"
- UI is React 18 + Vite: `ui/package.json:46@6c12a7aeac30` "\"react\": \"^18.3.1\"" and `ui/package.json:76@6c12a7aeac30` "\"vite\": \"^6.4.3\""
- Size (measured with `git ls-tree -r` at the SHA, not a citation): 7,624 tracked files; 1,352 `.rs`, 1,224 `.ts/.tsx`; 1,190,198 lines in `src-tauri/src`, `crates`, `ui/src` combined (includes tests).
- Mobile is a separate Tauri v2 shell that embeds the remote web UI, with no PTYs of its own: `mobile/README.md:3@6c12a7aeac30` "A separate Tauri v2 shell embeds the existing RemoteApp bundle." / `mobile/README.md:4@6c12a7aeac30` "PTYs, the desktop daemon, native GPU terminals"

## (b) Agent integration

- Model: Ferryx runs agent CLIs as plain PTY processes and observes them; it does not embed an SDK or speak ACP. A search of `src-tauri/Cargo.toml`, `ui/package.json`, `crates/*/Cargo.toml` and `src-tauri/src` at the SHA for `agent-client-protocol|agent_client_protocol|claude-agent-sdk|@anthropic-ai/sdk` returned 0 hits.
- One registry table per agent CLI: `src-tauri/src/agent_detect/registry.rs:1@6c12a7aeac30` "One table per coding agent." Table start: `src-tauri/src/agent_detect/registry.rs:93@6c12a7aeac30` "pub const AGENTS: &[AgentSpec] = &["
- 17 agents in the table: claude, codex, copilot, cursor, kimi, omo, maho, gjc, antigravity, opencode, pi, omp, grok, droid, cline, crush, aider. First and last entries: `src-tauri/src/agent_detect/registry.rs:95@6c12a7aeac30` "id: \"claude\"" ... `src-tauri/src/agent_detect/registry.rs:417@6c12a7aeac30` "id: \"aider\""
- Resume is native to each CLI (Claude `--resume`, Codex `resume`): `src-tauri/src/agent_detect/registry.rs:111@6c12a7aeac30` "resume_args: &[\"--resume\"]" and `src-tauri/src/agent_detect/registry.rs:131@6c12a7aeac30` "resume_args: &[\"resume\"]"; the resume argv is built in `src-tauri/src/terminal/shell.rs:146@6c12a7aeac30` "args: vec![\"--resume\".to_string(), validated_id]"
- Activity state comes from per-TUI screen-rule manifests (16 TOML files compiled in): `src-tauri/src/agent_detect/engine.rs:100@6c12a7aeac30` "pub static SHIPPED_MANIFESTS" / `src-tauri/src/agent_detect/engine.rs:104@6c12a7aeac30` "include_str!(\"manifests/claude.toml\")"; a rule example: `src-tauri/src/agent_detect/manifests/claude.toml:32@6c12a7aeac30` "state = \"blocked\""
- Second signal: Ferryx installs an `agent-hook` reporter into the agents' own hook configs, merging, never replacing: `src-tauri/src/daemon/agent_hook_install.rs:1@6c12a7aeac30` "Installs the `ferryx agent-hook` reporter" and `src-tauri/src/daemon/agent_hook_install.rs:78@6c12a7aeac30` "(\"claude\", \".claude\", \"settings.json\", HookShape::ClaudeGroups)"
- Spawn path: `portable-pty` (vendored): `src-tauri/Cargo.toml:130@6c12a7aeac30` "portable-pty = { version = \"0.9\", path = \"vendor/portable-pty\" }" and `src-tauri/src/terminal/pty.rs:614@6c12a7aeac30` ".openpty(pty_size)"

## (c) Worktree / branch isolation

- Managed worktrees at `<repo>/.ferryx-worktrees/<ws>/<slug>` on branch `<prefix>/<ws>/<slug>`: `src-tauri/src/worktree/naming.rs:3@6c12a7aeac30` "New managed worktrees live at `<repo>/.ferryx-worktrees/<ws>/<slug>`" and `src-tauri/src/worktree/naming.rs:18@6c12a7aeac30` "pub const WORKTREE_DIR: &str = \".ferryx-worktrees\";"
- Legacy Orca layout still parsed: `src-tauri/src/worktree/naming.rs:20@6c12a7aeac30` "LEGACY_WORKTREE_DIRS: [&str; 1] = [\".orca-worktrees\"]"
- Created with real `git worktree add -b`: `src-tauri/src/worktree/git.rs:1185@6c12a7aeac30` "pub fn git_worktree_add(" / `src-tauri/src/worktree/git.rs:1199@6c12a7aeac30` "\"add\"", called from `src-tauri/src/worktree/manager.rs:534@6c12a7aeac30` "pub fn create_worktree(" -> `src-tauri/src/worktree/manager.rs:587@6c12a7aeac30` "git_worktree_add("
- Agents can be spawned directly into a worktree: `src-tauri/src/terminal/pty.rs:408@6c12a7aeac30` "pub fn spawn_in_worktree("

## (d) Terminal implementation

- Desktop: native libghostty-vt via FFI with a GPU (wgpu) child-view renderer, not xterm.js: `src-tauri/src/native_terminal/mod.rs:1@6c12a7aeac30` "Safe libghostty-vt Native Terminal Engine Adapter" / `src-tauri/src/native_terminal/mod.rs:3@6c12a7aeac30` "pinned static `libghostty-vt.a` C API"; `src-tauri/Cargo.toml:163@6c12a7aeac30` "wgpu = { version = \"30\", optional = true }"
- Ghostty is a pinned fork submodule: `.gitmodules:3@6c12a7aeac30` "url = https://github.com/Indosaram/ghostty.git"
- Per-OS surfaces exist for macOS, Linux (incl. Wayland), Windows plus a fallback (`git ls-tree` of `src-tauri/src/native_terminal/platform/` at the SHA: fallback.rs, linux.rs, macos.rs, wayland_child.rs, windows.rs).
- Daemon keeps a headless VT mirror so a re-attach ships a screen, not raw bytes: `src-tauri/src/terminal/screen_mirror.rs:1@6c12a7aeac30` "Daemon-side screen mirror: a headless VT"
- Remote/mobile web terminal (`ui/src/remote/RemoteTerminal.tsx`) is an in-house component; `ui/package.json` has no xterm dependency (0 hits for `xterm` in `ui/package.json` at the SHA); xterm.js is only referenced in a comment: `ui/src/remote/RemoteTerminal.tsx:1880@6c12a7aeac30` "(xtermjs/xterm.js#6084)"

## (e) Review / diff / PR flow

- Per-worktree diff, stage, unstage, commit and push in the backend: `src-tauri/src/worktree/changes.rs:1@6c12a7aeac30` "Per-worktree change operations: diff, stage, unstage, commit and push." / `src-tauri/src/worktree/changes.rs:138@6c12a7aeac30` "pub fn git_diff(" / `src-tauri/src/worktree/changes.rs:309@6c12a7aeac30` "\"push\", \"--porcelain\""
- UI: a Changes panel calls those commands: `ui/src/components/ChangesPanel.tsx:8@6c12a7aeac30` "getWorktreeDiff," / `ui/src/components/ChangesPanel.tsx:10@6c12a7aeac30` "pushWorktree,"
- PRs: no in-app PR creation UI; the supervisor panel shows a read-only Pull requests lane from the hub: `ui/src/components/supervisorPanel/PrSection.tsx:1@6c12a7aeac30` "The panel's Pull requests section" ; the hub classifies agent `gh pr create` as a remote VCS effect for gating: `crates/ferryx-hub/src/effect.rs:1221@6c12a7aeac30` "(\"pr\", \"create\""
- Absent: inline review comments on diffs (searched `ui/src/components` file list for diff/review components; only `ChangesPanel.tsx` and file previews).

## (f) Remote / mobile access and push notifications

- Remote gateway is an Axum HTTP/WS server in the daemon: `src-tauri/Cargo.toml:143@6c12a7aeac30` "axum = { version = \"0.8\", features = [\"ws\"] }"
- Pairing by 6-digit PIN: `src-tauri/src/remote/auth.rs:794@6c12a7aeac30` "gen_range(100_000..=999_999)"
- NAT traversal via a public relay that tunnels WebSockets: `src-tauri/src/remote/relay_server.rs:1@6c12a7aeac30` "ferryx-relay: the public relay server that brokers WebSocket tunnels"
- Remote desktop (ferryx-rd, FFmpeg) on by default (see (a) features): `src-tauri/src/remote/remote_desktop.rs:1@6c12a7aeac30` "Daemon-owned ferryx-rd remote desktop host"
- Mobile: same RemoteApp in a Tauri mobile shell (see (a) `mobile/README.md:3`); the UI entry splits desktop vs remote: `ui/src/main.tsx:27@6c12a7aeac30` "__TAURI_INTERNALS__"
- Push on main is Web Push only (VAPID, aes128gcm): `src-tauri/src/remote/push.rs:1@6c12a7aeac30` "Phone push: VAPID keys" / `src-tauri/src/remote/push.rs:539@6c12a7aeac30` "web_push::ContentEncoding::Aes128Gcm"; scope is decision cards only, never progress: `src-tauri/src/remote/push.rs:4@6c12a7aeac30` "Push is for decision cards only"; routes: `src-tauri/src/remote/server.rs:9026@6c12a7aeac30` "\"/api/v1/push/subscribe\""
- The browser client needs `PushManager`, which an iOS WKWebView lacks, so the installed iOS app gets no push on main: `ui/src/lib/pushSubscription.ts:50@6c12a7aeac30` "\"PushManager\" in window"
- In flight (not on main): native APNs. Committed only on branch `apns-ios-native` (`f3704c040cf4`, not an ancestor of main): `mobile/src-tauri/src/apns.rs:1@f3704c040cf4` "Native APNs registration for the iOS shell." The daemon-side APNs sender (`src-tauri/src/remote/apns.rs`, plus edits to `push.rs`) exists only as uncommitted/untracked files in the shared tree; this is the plan's Wave 6 (todos 20-25) and is not counted as shipped.

## (g) Attention / notification model

- Per-session agent state (working/blocked/done) from screen rules (b) plus hooks, held in the daemon: `src-tauri/src/daemon/agent_state.rs:9@6c12a7aeac30` "pub(crate) struct AgentState {"; manifest rule states e.g. `src-tauri/src/agent_detect/manifests/claude.toml:10@6c12a7aeac30` "state = \"working\""
- Native OS notifications with two attention reasons, Waiting and Done: `src-tauri/src/notification/model.rs:56@6c12a7aeac30` "pub enum NotificationAttentionReason {" / `src-tauri/src/notification/model.rs:57@6c12a7aeac30` "Waiting,"; Rust handles the OS side, the frontend decides whether to notify: `src-tauri/src/notification/mod.rs:3@6c12a7aeac30` "Rust decides *how* to talk to the OS safely"
- Custom sound and Dock badge: `src-tauri/src/notification/audio.rs:1@6c12a7aeac30` "Custom notification sound playback." / `src-tauri/src/notification/badge.rs:1@6c12a7aeac30` "Application badge domain logic and native Dock tile updates."
- Phone: Web Push of supervisor escalations only (see (f)).

## (h) Checkpoint / rollback

- Absent for agent edits: no per-turn snapshot or rollback of a worktree. Searched `checkpoint|rollback|undo|git stash|restore point` in `src-tauri/src/worktree`, `src-tauri/src/ipc`, `ui/src/components` at the SHA; hits are only the browser-engine install rollback (`src-tauri/src/ipc/browser_engine.rs`) and supervisor checkpoints below.
- What "checkpoint" means in Ferryx: a portable archive of the supervisor hub database moved between machines, not code state: `crates/ferryx-hub/src/checkpoint.rs:1@6c12a7aeac30` "Portable supervisor checkpoint (export side)." / `src-tauri/src/supervisor_checkpoint.rs:1@6c12a7aeac30` "Supervisor checkpoints between machines"
- Rollback of code relies on the user's git through the Changes panel (e)

## (i) Persistence / session recovery

- A headless daemon owns every PTY and outlives the GUI (closing the app keeps agents running): `README.md:11@6c12a7aeac30` "keep running when you close the app"; daemon socket and single-instance lock: `src-tauri/src/daemon/server.rs:921@6c12a7aeac30` "get_runtime_dir().join(\"daemon.sock\")" / `src-tauri/src/daemon/server.rs:892@6c12a7aeac30` "libc::flock(file.as_raw_fd(), libc::LOCK_EX)"
- 512 KiB per-session output ring with sequence numbers and ReplayGap resync: `src-tauri/src/terminal/output_hub.rs:8@6c12a7aeac30` "const DEFAULT_BUFFER_CAPACITY: usize = 512 * 1024;" / `src-tauri/src/terminal/output_hub.rs:82@6c12a7aeac30` "pub struct ReplayGap {"
- Longer retained history a restarted GUI replays: `src-tauri/src/terminal/retained_history.rs:5@6c12a7aeac30` "keeps the longer, protocol-safe history a restarted GUI replays from scratch."
- Daemon-to-daemon handover keeps PTYs across daemon upgrades: `src-tauri/src/daemon/manifest.rs:14@6c12a7aeac30` "pub struct HandoverManifest {"
- Session lifecycle includes hibernation of idle backing processes: `src-tauri/src/daemon/session_lifecycle.rs:16@6c12a7aeac30` "Hibernated,"
- After a full restart, agents are resumed through each CLI's own resume flag (b).

## (j) License / telemetry

- Source-available custom "Ferryx License", not OSI: `LICENSE:1@6c12a7aeac30` "# Ferryx License"; free for individuals/education/OSS/non-profit/small companies, paid plan for larger companies: `LICENSE:27@6c12a7aeac30` "requires an active paid subscription to the Ferryx Team plan"; wired into the crate: `src-tauri/Cargo.toml:6@6c12a7aeac30` "license-file = \"../LICENSE\""
- Telemetry: absent. A whole-word search for `posthog|sentry|mixpanel|amplitude|segment.io|datadog` in `src-tauri/Cargo.toml`, `ui/package.json`, `mobile/package.json`, `package.json`, `crates`, `src-tauri/src`, `ui/src` at the SHA returned 0 hits; remaining "telemetry" hits are internal metrics (e.g. remote-desktop receiver stats in `crates/ferryx-rd`), not reporting to a third party.

## Not measured

- Runtime behaviour (no local build or run, per plan guardrails).
- Mobile store builds and whether the shipped 2026.1113.2 bundle matches this SHA; the lane-F bundle census (`.omo/ulw-research/20261114-menubar-apps/wave2/lane-F-ferryx.md`, untracked) measured an older build.
