# PROJECT KNOWLEDGE BASE

**Generated:** 2026-08-28T15:47:10Z
**Commit:** ac103c2
**Branch:** main

## OVERVIEW
Ferryx: multi-workspace terminal and Git worktree manager with desktop and remote web clients.
Rust Tauri v2 + Tokio headless daemon + Axum gateway + libghostty-vt/WGPU native terminal; React + TypeScript + Zustand + xterm.js / native surface bridge.

## STRUCTURE
```
orca-lite/
├── src-tauri/          # Rust backend: headless PTY daemon, Tauri IPC, Axum remote server
│   ├── src/daemon/    # UDS protocol v2 daemon and launchd service agent
│   ├── src/ipc/       # Tauri command handlers, run_blocking, 20-byte binary stream framing
│   ├── src/remote/    # Axum gateway, PIN auth, active desktop session lock
│   ├── src/terminal/  # portable-pty lifecycle, 512 KiB sequenced ring buffers, Ghostty config
│   ├── src/native_terminal/ # libghostty-vt FFI and WGPU child view rendering engine
│   ├── src/worktree/  # Git worktree isolation in .orca-worktrees/ with root jail
│   └── src/notification/ # OS notification dispatch, Rodio audio player, Dock badge sync
└── ui/                 # React frontend (Tauri desktop app & standalone remote web client)
    ├── src/components/ # Workspaces, settings modals, and DnD pane split overlays
    ├── src/lib/        # Tauri IPC bridges, native surface bridge, rAF output scheduler
    └── src/state/      # Zustand store, binary pane split trees, layout restoration
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Headless PTY Daemon & Sockets | `src-tauri/src/daemon/` | UDS v2 server at `/tmp/rorca-{uid}/daemon.sock`, flock sync |
| Binary IPC & Command Layer | `src-tauri/src/ipc/` | Typed Tauri commands, 20-byte stream headers, `run_blocking` |
| Remote Gateway & Auth | `src-tauri/src/remote/` | Axum server, 6-digit PIN pairing, active desktop session lock |
| Terminal Ring Buffer & PTY | `src-tauri/src/terminal/` | `portable-pty`, 512 KiB ring buffer with `ReplayGap` detection |
| Native Terminal Engine | `src-tauri/src/native_terminal/` | `libghostty-vt` FFI, WGPU child view rendering across OS targets |
| Worktree Git Management | `src-tauri/src/worktree/` | `orca/<ws-id>/<slug>` branch allocation, root jail safety |
| Native OS Notifications | `src-tauri/src/notification/` | Rodio audio cues, macOS Cocoa Dock badges, preflight checks |
| UI Component Hierarchy | `ui/src/components/` | Multi-pane split views, DnD tab transplants, native surface canvas bridge |
| UI IPC & Output Scheduling | `ui/src/lib/` | rAF-coalesced PTY buffer flushing, xterm host lifecycle, IPC error mapping |
| Workspace State & Layouts | `ui/src/state/` | Zustand store, binary pane trees, layout serialization, HMR retention |
| UI Desktop vs Remote Entry | `ui/src/main.tsx` | Checks `window.__TAURI_INTERNALS__` to load Desktop vs RemoteApp |

## CODE MAP
| Symbol | Type | Location | Refs | Role |
|--------|------|----------|------|------|
| `terminalOutputScheduler` | Module | `ui/src/lib/terminalOutputScheduler.ts` | High | Schedules rAF-coalesced PTY chunk flushes to xterm |
| `terminalHostManager` | Singleton | `ui/src/lib/terminalHostManager.ts` | High | Manages xterm.js instances and DOM WebGL canvas attachment |
| `useWorkspaceStore` | Store | `ui/src/state/workspaceStore.ts` | High | Zustand store for binary pane split trees and session IDs |
| `TerminalOutputHub` | Struct | `src-tauri/src/terminal/` | High | 512 KiB circular ring buffer with monotonic sequence numbers |
| `run_blocking` | Function | `src-tauri/src/ipc/` | High | Spawns synchronous disk/git operations onto dedicated blocking threads |
| `NativeTerminalEngine` | Struct | `src-tauri/src/native_terminal/` | High | `libghostty-vt` FFI to WGPU child view rendering bridge |

## CONVENTIONS
- **Cross-Platform Premise**: Every implementation must assume cross-platform execution (macOS, Windows, Linux). Never land macOS-only input/IME/shell paths without a portable abstraction; platform-specific code must be isolated behind explicit platform modules with working fallbacks on all other targets.
- **Storage Keys**: Use canonical `ferryx.*` key prefix; auto-migrates legacy `rorca.*` and `orca.*` keys on read.
- **Thread Safety**: All synchronous disk I/O, Git subprocesses, and OS dialogs must run via `crate::ipc::run_blocking`.
- **Identity Triad**: Strict separation between visual `leafId`, frontend `sessionId`, and daemon `backendSessionId`.
- **Worktree Isolation**: Managed worktrees must reside in `.orca-worktrees/wt-<slug>` using branch `orca/<ws-id>/<slug>`.
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
```bash
# Frontend Dev / Build / Test
bun run --cwd ui dev
bun run --cwd ui build
bun test --cwd ui

# Backend Desktop Dev / Build / Test
cargo tauri dev --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib

# Run Headless PTY Daemon
cargo run --manifest-path src-tauri/Cargo.toml -- --daemon
```

## LOCAL RELEASES
- Releases are local-only: GitHub Actions may run pull-request checks and deploy Pages, but must
  never build, sign, assemble, or publish release artifacts. This boundary is enforced from
  source by `scripts/release-workflow-policy.mjs`.
- Use [`docs/releases/LOCAL_RELEASE_RUNBOOK.md`](docs/releases/LOCAL_RELEASE_RUNBOOK.md) as the
  canonical CLI and safety procedure. Never substitute a tag-triggered or manually dispatched
  hosted release workflow.

## NOTES
- Headless daemon UDS socket lives at `/tmp/rorca-{uid}/daemon.sock` locked with `flock`.
- PTY ring buffer overflow emits `ReplayGap` to signal terminal re-sync requirements.
