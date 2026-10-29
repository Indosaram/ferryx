# PROJECT KNOWLEDGE BASE

**Generated:** 2026-08-28T15:47:10Z
**Commit:** ac103c2
**Branch:** main

## OVERVIEW
Ferryx: multi-workspace terminal and Git worktree manager with desktop and remote web clients.
Rust Tauri v2 + Tokio headless daemon + Axum gateway + libghostty-vt/WGPU native terminal; React + TypeScript hook-based workspace state and native surface bridge.

## STRUCTURE
```
ferryx/
├── src-tauri/          # Rust backend: headless PTY daemon, Tauri IPC, Axum remote server
│   ├── src/daemon/    # UDS protocol v5 daemon; macOS launchd agent owns its lifetime
│   ├── src/ipc/       # Tauri command handlers, run_blocking, 20-byte binary stream framing
│   ├── src/remote/    # Axum gateway, PIN auth, active desktop session lock
│   ├── src/terminal/  # portable-pty lifecycle, 512 KiB sequenced ring buffers, Ghostty config
│   ├── src/native_terminal/ # libghostty-vt FFI and WGPU child view rendering engine
│   ├── src/worktree/  # Git worktree isolation in .ferryx-worktrees/ with root jail (naming.rs)
│   └── src/notification/ # OS notification dispatch, Rodio audio player, Dock badge sync
└── ui/                 # React frontend (Tauri desktop app & standalone remote web client)
    ├── src/components/ # Workspaces, settings modals, and DnD pane split overlays
    ├── src/lib/        # Tauri IPC bridges, native surface lifecycle and transport
    └── src/state/      # React workspace hook, binary pane split trees, layout restoration
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Headless PTY Daemon & Sockets | `src-tauri/src/daemon/` | UDS v5 server at `/tmp/ferryx-{uid}/daemon.sock`, flock sync |
| Binary IPC & Command Layer | `src-tauri/src/ipc/` | Typed Tauri commands, 20-byte stream headers, `run_blocking` |
| Remote Gateway & Auth | `src-tauri/src/remote/` | Axum server, 6-digit PIN pairing, active desktop session lock |
| Terminal Ring Buffer & PTY | `src-tauri/src/terminal/` | `portable-pty`, 512 KiB ring buffer with `ReplayGap` detection |
| Native Terminal Engine | `src-tauri/src/native_terminal/` | `libghostty-vt` FFI, WGPU child view rendering across OS targets |
| Worktree Git Management | `src-tauri/src/worktree/` | `<prefix>/<ws-id>/<slug>` branch allocation (`naming.rs`), root jail safety |
| Native OS Notifications | `src-tauri/src/notification/` | Rodio audio cues, macOS Cocoa Dock badges, preflight checks |
| UI Component Hierarchy | `ui/src/components/` | Multi-pane split views, DnD tab transplants, native surface canvas bridge |
| UI IPC & Output Scheduling | `ui/src/lib/` | Native terminal lifecycle, PTY event bus and transport, IPC error mapping |
| Workspace State & Layouts | `ui/src/state/` | React reducer store, binary pane trees, layout serialization, HMR retention |
| UI Desktop vs Remote Entry | `ui/src/main.tsx` | Checks `window.__TAURI_INTERNALS__` to load Desktop vs RemoteApp |

## CODE MAP
| Symbol | Type | Location | Refs | Role |
|--------|------|----------|------|------|
| `terminalEvents` | Module | `ui/src/lib/terminalEvents.ts` | High | Multiplexes terminal events and output backlog |
| `nativeTerminalLifecycle` | Module | `ui/src/lib/nativeTerminalLifecycle.ts` | High | Manages native terminal surface attachment and visibility |
| `useWorkspaceStore` | Hook | `ui/src/state/workspaceStore.ts` | High | React reducer for binary pane split trees and session IDs |
| `TerminalOutputHub` | Struct | `src-tauri/src/terminal/` | High | 512 KiB circular ring buffer with monotonic sequence numbers |
| `run_blocking` | Function | `src-tauri/src/ipc/` | High | Spawns synchronous disk/git operations onto dedicated blocking threads |

## CONVENTIONS
- **Cross-Platform Premise**: Every implementation must assume cross-platform execution (macOS, Windows, Linux). Never land macOS-only input/IME/shell paths without a portable abstraction; platform-specific code must be isolated behind explicit platform modules with working fallbacks on all other targets.
- **Storage Keys**: Use canonical `ferryx.*` key prefix; auto-migrates legacy `rorca.*` and `orca.*` keys on read.
- **Thread Safety**: All synchronous disk I/O, Git subprocesses, and OS dialogs must run via `crate::ipc::run_blocking`.
- **Identity Triad**: Strict separation between visual `leafId`, frontend `sessionId`, and daemon `backendSessionId`.
- **Worktree Isolation**: Managed worktrees reside in `.ferryx-worktrees/<ws-id>/<slug>` on branch `<prefix>/<ws-id>/<slug>`, where `<prefix>` defaults to `ferryx` and is configurable per repo in `<data>/worktree-settings.json` (`cmd_worktree_settings_get/set`). `src-tauri/src/worktree/naming.rs` is the single source of these names; never hardcode them elsewhere. A repo that already has `.orca-worktrees` keeps the `orca` prefix and dir unless the user sets a prefix. Every parser also accepts the legacy `.orca-worktrees` dir, the `orca/` prefix and the old SSH `.orca-worktrees/wt-<slug>` layout; existing worktrees and branches are never moved or renamed.
- **Agent Worktree Location (dev/verify checkouts)**: Every throwaway checkout an agent creates for development, verification, gates, or QA must be a `git worktree` of the host's canonical clone under ONE fixed root per host, never a loose copy or extracted tarball in `$HOME`, `/tmp`, `%TEMP%`, or ad-hoc names (`ferryx-*-2026xxxx`, `task*`, `wt-*`, `herdr-*`, `plv-*`, `fx-*`):
  | Host | Canonical clone | Worktree root | Cargo target |
  |------|-----------------|---------------|--------------|
  | macOS (this Mac) | `/Volumes/T9-Mac/project/ferryx` | `/Volumes/T9-Mac/project/ferryx-wt/<slug>` | shared `src-tauri/target` or `/tmp/ferryx-<slug>-target` |
  | omarchy (Linux) | `~/code/ferryx` | `~/code/ferryx-wt/<slug>` | `~/code/ferryx-wt/.target` (shared) |
  | maho-win (Windows) | `C:\Users\sook\ferryx` | `C:\Users\sook\ferryx-wt\<slug>` | `C:\Users\sook\ferryx-wt\.target` (shared) |
  - Create with `git -C <canonical> worktree add <root>/<slug> <rev>`; `<slug>` is `<topic>-<yyyymmdd>` (lowercase, `-` only). Create the canonical clone first if it is missing.
  - Remove when the task ends (pass or fail): `git -C <canonical> worktree remove --force <root>/<slug>` then `git -C <canonical> worktree prune`. Delete per-worktree cargo targets with it. Keep logs/evidence only under `<root>/.evidence/<slug>`.
  - Release builds stay in `~/ferryx-releases/` (managed by the release runbook); the relay UI stays in `~/ferryx-relay-web` on omarchy (referenced by `ferryx-relay.service`). Neither is a worktree root.
  - Never leave a worktree behind at the end of a session; before finishing, run `git -C <canonical> worktree list` on every host you touched and report what remains.
- **Build Host Selection (remote builds/tests/gates)**: Before starting any cargo/bun build, test, or gate on a remote host, run `host=$(scripts/pick-build-host.sh)` from the canonical clone on this Mac and use the host it prints; never default to a fixed host. It probes omarchy, noye, hark, maho-mac, maho-win and instabox (instabox last) and prints the least-loaded idle one (load/core < 0.5, free RAM >= 8 GB, free disk >= 15 GB, no cargo/rustc/zig running).
  - The picked host is claimed in `/tmp/ferryx-build-host-locks/<host>`, and other sessions skip it while the claim holds: a lease of `FERRYX_BUILD_CLAIM_TTL` seconds (default 7200), or, with `FERRYX_BUILD_OWNER_PID=<pid>`, for as long as that pid is alive. Run `scripts/pick-build-host.sh --release <host>` as soon as the build ends, pass or fail.
  - Exit code 3 means no idle host: wait and retry, or ask the user before using paid XenoCI. Never start a second build on a host the picker reported as busy or claimed.
- **Binary Stream Framing**: 20-byte fixed header; stream pumps coalesce bursts (`10ms` interval or `32KB` max).

## ANTI-PATTERNS (THIS PROJECT)
- NEVER execute synchronous disk I/O or git commands on async Tokio runtime threads; use `run_blocking`.
- NEVER allow worktree paths outside the repository root jail (`WorktreeIdentity`).
- NEVER destroy daemon PTY processes or unmount native surfaces during pane drag-and-drop or Vite HMR.
- NEVER forcibly restart or kill the background daemon (`ferryx --daemon`) when building a release, replacing the app bundle, or launching the new application unless strictly necessary (e.g. an incompatible daemon protocol version mismatch). The daemon owns all PTY master file descriptors; terminating it immediately kills all active terminal sessions and running agent workflows.
- NEVER hardcode legacy storage prefixes (`orca.*`, `rorca.*`); use `ferryx.*` from `ui/src/lib/storageKeys.ts`.
- NEVER match backend IPC errors with regex string matching; parse structured `{ code, message, details }`.
- NEVER leave native child webviews unmasked during active tab/pane drag gestures.
- NEVER discard, revert, or overwrite uncommitted working-tree changes that your own session did not author — check `git status` / `git diff` first; any uncommitted diff is another session's live work. Forbidden: `git reset --hard`, `git checkout -- <path>` / `git restore <path>`, `git clean`, `git stash`, or rewriting a file back to its HEAD version to "clean up".
- NEVER edit a file from a stale in-memory copy; re-read it immediately before editing so you do not silently clobber changes another session just wrote.

## UNCOMMITTED WORK SAFETY (MULTI-SESSION)
Multiple agent sessions may share this single working tree at the same time. Uncommitted changes belong to whichever session produced them and must survive until their author (or the user) commits or discards them.

1. **Survey before you touch anything.** Run `git status` and `git diff` at session start and before each write phase. Treat every dirty file you did not author in THIS session as another session's active work — read-only.
2. **No destructive git ops while the tree is dirty with foreign work.** `git reset`, `git restore/checkout -- <path>`, `git stash`, `git clean`, and branch switches that would drop changes are forbidden unless the user explicitly asks and the affected files are yours.
3. **Re-read before edit; re-verify after edit.** After any write, confirm your change landed (`rg` the exact lines). If a later check shows your fix vanished, assume a concurrent session reverted it — re-apply and report, do not assume the user is wrong.
4. **Conflicts resolve forward, not backward.** If uncommitted foreign work conflicts with your change, layer your edit on top of the current file content or wait/ask the user. Never resolve a conflict by restoring the HEAD version.
5. **Report risk.** When you finish work that lives uncommitted in a shared tree, tell the user it is uncommitted and vulnerable to concurrent sessions, and offer to commit.

## UNIQUE STYLES
- **Dual Launch Modes**: CLI `--daemon` runs headless Tokio daemon (emits `FERRYX_DAEMON_READY`); GUI attaches via UDS.
- **Dual Runtime Frontend**: `ui/src/main.tsx` dynamically routes to `App.tsx` (Tauri desktop) or `RemoteApp.tsx` (web/mobile).
- **Hardware-Accelerated Native Terminal**: `libghostty-vt` with WGPU child surfaces (`NSView`/`HWND`/`XSubwindow`/`wl_subsurface`).
- **Active Desktop Lock**: Axum WebSocket endpoint binds to current desktop session; switching focus disconnects remote client.

## COMMANDS
JS/TS runs on Bun only: never invoke `node`, `npx`, `npm`, or `NODE_OPTIONS`. Use `bun install`, `bun run`, `bunx`, and run Node-shebang tools on the Bun runtime with `bun --bun <tool>` (the `ui` scripts already do this).

```bash
# Frontend Dev / Build / Test
bun run --cwd ui dev
bun run --cwd ui build
bun run --cwd ui test

# Backend Desktop Dev / Build / Test
cd src-tauri && cargo tauri dev
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib

# Run Headless PTY Daemon
cargo run --manifest-path src-tauri/Cargo.toml -- --daemon
```

## LOCAL RELEASES
- Releases are local-only: GitHub Actions may run pull-request checks, while the site deploys
  separately to Cloudflare Workers; hosted workflows must
  never build, sign, assemble, or publish release artifacts. This boundary is enforced from
  source by `scripts/release-workflow-policy.mjs`.
- Use [`docs/releases/LOCAL_RELEASE_RUNBOOK.md`](docs/releases/LOCAL_RELEASE_RUNBOOK.md) as the
  canonical CLI and safety procedure. Never substitute a tag-triggered or manually dispatched
  hosted release workflow.

## NOTES
- Headless daemon UDS socket lives at `/tmp/ferryx-{uid}/daemon.sock` (`-dev` suffix for debug builds), locked with `flock`.
- For one release the daemon also serves the pre-rename `/tmp/rorca-{uid}/daemon.sock` (same
  protocol, 0600 socket in a 0700 dir, guarded by that dir's `daemon.lock`) so older GUIs still
  connect, and new clients fall back to it when only a pre-rename daemon is running. Under a
  `FERRYX_RUNTIME_DIR` override the legacy dir exists only if `FERRYX_LEGACY_RUNTIME_DIR` names
  one. Remove `get_legacy_runtime_dir` and its callers in the release after.
- PTY ring buffer overflow emits `ReplayGap` to signal terminal re-sync requirements.
