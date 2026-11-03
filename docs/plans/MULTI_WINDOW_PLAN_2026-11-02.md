# Ferryx Multi-Window Plan

- Date: 2026-11-02
- Base: `main` working tree at the time of writing. Every file:line below was located with `rg` against that tree; no code was built or run.
- Demand evidence: Orca #6074 ("Multi-window is table stakes", 35 reactions / 15 comments), see `docs/research/cmux-orca-herdr-user-complaints-2026-10-14.md:949`.

## 1. Goal and non-goals

**Goal.** A user can open a second (third, ...) desktop window, move workspaces or tabs into it, and every window renders, focuses, receives input, notifications and shortcuts correctly on macOS, Windows and Linux (X11 and Wayland). Window set and placement survive an app restart. Daemon sessions are never killed or re-spawned by any window operation.

**Non-goals (v1).**
- The same terminal session shown live in two windows at once (mirroring). A session is attached to exactly one window at a time.
- Detaching a single pane (not a whole tab) into a new window.
- Per-window settings/themes. Preferences stay app-global.

## 2. Current single-window assumptions (inventory)

### 2.1 Hardcoded `"main"` lookups (production code)

| Area | Location | What breaks in window #2 |
|---|---|---|
| Render scheduling on PTY output | `src-tauri/src/native_terminal/surface_host.rs:4590` | Output from a session in window #2 is dispatched to `main`; window #2 never repaints. **Critical.** |
| Terminal IPC commands | `src-tauri/src/ipc/native_terminal.rs`: `cmd_native_terminal_set_bounds` (:1346), `set_focus` (:1526), `set_preedit` (:1560), `dispatch_native_terminal_receipt` (:1818), `scroll` (:2254), `set_scrollbar_overlay` (:2324), `set_attention_frame` (:2383), `select` (:2435), `mouse` (:3749), `clipboard_content` (:3818) | Bounds, focus, IME preedit, scroll, mouse and selection from window #2 act on `main`'s surfaces/NSView. |
| Windows focus bridge | `native_terminal/platform/windows_focus.rs:192`, `:222` | Focus events go to `main`. |
| Shortcut dispatch | `src-tauri/src/shortcut_dispatch.rs:9` | Menu/global shortcuts always hit `main`. |
| Native menu | `src-tauri/src/ipc/native_menu.rs:194`; app menu `window.close` `lib.rs:279` | Cmd+W closes `main` regardless of the focused window. |
| macOS key/mouse monitors | `lib.rs:873`, `:901`, `:998`, `:1051` (NSEvent local monitors, copy-or-interrupt) | Ctrl+C / focus gestures only resolved against `main`'s NSWindow. |
| macOS file drop, window chrome | `lib.rs:931`, `:957` (setup only) | Window #2 gets no file drop destination / chrome tweaks. |
| Notification activation, daemon-ready focus | `lib.rs:1223`, `:1453`, `:1477` | Clicking a notification always raises `main`, not the window owning the session. |
| Preferences background refresh | `src-tauri/src/ipc/preferences.rs:81`, `:188` | Only `main` repaints its backing color on theme change. |
| Clipboard image read | `src-tauri/src/clipboard_image.rs:416` | Uses `main` for main-thread hop; works but wrong owner. |
| Browser panes | `src-tauri/src/ipc/browser.rs:1380`, `:2717`; `ipc/browser_cli.rs:2953` | Browser child webviews always mount into `main`. |
| File preview capability | `src-tauri/src/ipc/file_preview.rs:52` (`ROOT_WINDOW_LABEL = "main"`) | Preview capability server refuses window #2. |
| QA config | `lib.rs:1278`, `:1431` | QA harness only knows `main`. Keep, but must not block extra windows. |

### 2.2 Other global assumptions

- **Event fan-out.** About 60 `app.emit(...)` calls broadcast to every window (16 files, e.g. `surface_host.rs:3477-3483` title/bell/agent-state/scrollbar, `ipc/terminal.rs`, `ipc/worktree.rs`, `ipc/paired_host.rs`). The UI listens globally (`ui/src/lib/tauri.ts:893-1157`). With two windows, each window processes every event: double notifications, double sounds, double badge counts, and stores that mutate for sessions they do not own.
- **Session persistence.** One file via `get_session_file_path` (`src-tauri/src/ipc/session.rs:9`), one `PersistedWorkspaceSession` (`src-tauri/src/session/mod.rs:163`), loaded by `ui/src/state/workspaceRestore.ts:191`. Two windows saving would overwrite each other (last writer wins).
- **Capabilities.** `src-tauri/capabilities/default.json` grants permissions to webview `"main"` only. Window #2 gets IPC permission errors.
- **Window config.** `tauri.conf.json` declares exactly one window; `lib.rs:2916` builds `main` (QA path). No `new window` command or menu item exists.
- **Lifecycle.** `on_window_event` only handles `Destroyed` (`lib.rs:1361`); closing `main` is treated as app exit in several paths.
- **Remote active-desktop lock.** `RemoteActiveDesktopSelection` (`src-tauri/src/remote/protocol.rs`, `remote/state.rs:839`) holds one selection; the remote client follows "the" desktop focus.
- **Surface registry.** `hosts`/`sessions` maps are keyed by session id only (`surface_host.rs:662-663`). Surface creation already takes the caller's `Window<R>` (`:1520`, `:1532`, `:1582`, `:1857`, `:2426`), which is the seam this plan builds on.

## 3. Design

### 3.1 Window identity
- Labels: `main` stays as the first window (keeps QA, capabilities and existing code paths valid). New windows: `ws-<uuid>`.
- A backend `WindowRegistry` (new, `src-tauri/src/window_registry.rs`) holds `label -> WindowRecord { workspace_ids, created_at }` plus `session_id -> window_label` ownership. It is the single source for "which window owns this session".

### 3.2 Ownership rule
- A terminal session, browser pane or file tab is owned by exactly one window.
- Ownership is set when the surface is created (the IPC call already carries the calling `Window<R>`), and changes only through an explicit move (3.5).
- Every `get_window("main")` in 2.1 is replaced by one of:
  - **caller window**: IPC commands take `window: tauri::Window<R>` (Tauri injects it) instead of looking up `main`. Applies to all `cmd_native_terminal_*`, browser, preferences and clipboard commands.
  - **owner window**: background paths (render scheduling `surface_host.rs:4590`, receipts, Windows focus, notification activation) resolve `WindowRegistry::owner(session_id)`.
  - **focused window**: menu and shortcut paths (`shortcut_dispatch.rs:9`, `native_menu.rs:194`, `lib.rs:279`, NSEvent monitors) use the currently focused window, falling back to the most recently focused.
- Add one helper, `fn window_for_session(app, session_id) -> Option<Window<R>>`, and one for the focused window. No other lookups by label.

### 3.3 Events
- **Session-scoped events** (title, bell, agent state, scrollbar, input receipt, stream ended, terminal output/lifecycle, remote status) switch from `app.emit` to `emit_to(owner_label, ...)`.
- **App-scoped events** (worktree changed, paired host inventory, supervisor, settings, disk scan) stay broadcast. Each window only renders its own workspaces, so receiving them is harmless.
- **Notifications, sounds and dock badges** are produced once, in the backend or a single elected window (the focused one, else `main`), never per window. `ui/src/lib/notificationCoordinator.ts` gets a `isNotificationLeader` gate.

### 3.4 Persistence
- `PersistedWorkspaceSession` gains `windows: Vec<PersistedWindow { label, bounds, monitor, workspace_ids, active_workspace_id }>`, schema version bumped, with a migration that maps an old file to a single `main` window.
- Saves go through the backend, which merges per-window patches under one lock. A window sends only its own slice (`cmd_session_save_window`), so windows never overwrite each other.
- On launch the backend recreates the saved windows first; each window then restores only its slice. Live daemon sessions are re-attached by `backendSessionId` as today.

### 3.5 Moving tabs and workspaces between windows
- v1 entry points:
  - "Move tab to new window"
  - "Move workspace to window..."
  - dragging a tab outside the window (creates a new window)
- Steps:
  1. The source window detaches the native surface (unmount the child view, keep the daemon session).
  2. The registry flips ownership.
  3. The target window mounts a new surface for the same `backendSessionId` and replays from the ring buffer.
- This reuses the existing tab transplant and HMR retention path. Daemon PTYs are never closed. This keeps the AGENTS.md rule "never destroy daemon PTY processes ... during pane drag-and-drop".
- While a move is in flight, input to that session is held, not dropped.

### 3.6 Lifecycle
- **New window:** `Cmd/Ctrl+Shift+N` and the menu item "New Window" (empty workspace picker). On macOS, a Dock reopen raises the last focused window.
- **Closing a non-last window:** its sessions are not killed. They move to the most recently focused remaining window, as background tabs, or the user is asked. The default is to move them, never to kill them.
- **Closing the last window:** keep the current behavior.
- **`main` closed while others remain:** another window becomes the primary for QA and setup purposes. The `main` label is only an initial name, not a requirement.

### 3.7 Remote active-desktop lock
- The active selection follows the focused window. When focus switches between windows, it updates the selection the same way switching workspaces does today. No protocol change is needed.

### 3.8 Platform notes
- **macOS:** NSView child surfaces per NSWindow. Install the file drop destination (`lib.rs:931`) and window chrome (`:957`) for every window at creation, not only at setup. NSEvent monitors resolve the event's own `NSWindow`, not `main`.
- **Windows:** child HWND per top-level window (`platform/windows.rs:232` already builds from the given window's HWND). The focus bridge is the one global: `windows_focus.rs:150-152` keeps a single `MONITOR_STATE { root_hwnd, .. }` behind one `WH_MOUSE_LL` hook installed for `main` only (`:192`). Keep the single process-wide hook, but change `MonitorState` to a `HashMap<root_hwnd, ..>` and resolve the clicked point's top-level window with `GetAncestor(WindowFromPoint(pt), GA_ROOT)`.
- **Linux:** X11 child windows and Wayland `wl_subsurface` (`platform/wayland_child.rs`) are created against each window's own parent surface. Moving a surface between windows means destroying and recreating it; reparenting is not attempted.
- **Capabilities:** `capabilities/default.json` uses `"windows": ["main", "ws-*"]` (glob) for both windows and webviews.

## 4. Work breakdown (ordered; each step lands green on its own)

| # | Step | Files | Done when |
|---|---|---|---|
| 1 | `WindowRegistry` + `window_for_session` / focused-window helpers, ownership recorded at surface create | new `window_registry.rs`, `surface_host.rs` create paths | unit tests: owner set on create, cleared on close, moved on transfer |
| 2 | Replace caller-window lookups in all `cmd_native_terminal_*` with the injected `Window<R>` | `ipc/native_terminal.rs` (10 sites) | existing native terminal tests pass unchanged; new test calls each command from a non-`main` mock window |
| 3 | Owner-window render scheduling and receipts | `surface_host.rs:4590`, `ipc/native_terminal.rs:1818`, `windows_focus.rs` | test: output for a session owned by `ws-x` dispatches to `ws-x` |
| 4 | Focused-window menu/shortcut/close/NSEvent paths | `shortcut_dispatch.rs`, `native_menu.rs`, `lib.rs:279/873/901/998/1051` | Cmd+W / Ctrl+C acts on the focused window in a two-window mock |
| 5 | Session-scoped `emit_to`, notification leader | `surface_host.rs:3477+`, `ipc/terminal.rs`, UI `notificationCoordinator.ts`, `tauri.ts` listeners | test: two mock windows, one notification per agent-done event |
| 6 | Capabilities glob, file preview `ROOT_WINDOW_LABEL` → registry check, browser/preferences/clipboard caller window | `capabilities/default.json`, `ipc/file_preview.rs:52`, `ipc/browser.rs`, `ipc/preferences.rs`, `clipboard_image.rs` | IPC from `ws-*` permitted; preview server accepts registered windows only |
| 7 | New Window command + menu + shortcut; per-window setup (file drop, chrome) | `lib.rs`, new `ipc/window.rs`, `ui/src/lib/shortcuts.ts`, menu | a second window opens with a workspace picker on all three OSes |
| 8 | Persistence schema v-next, per-window save/restore, migration | `session/mod.rs`, `ipc/session.rs`, `ui/src/state/workspaceRestore.ts` | old session file restores as one `main` window; two-window session round-trips |
| 9 | Move tab/workspace between windows (detach, re-own, attach, replay) | `surface_host.rs`, UI tab DnD, `workspaceStore.ts` | moving a running agent tab keeps the same daemon session id and scrollback; no PTY exit |
| 10 | Close-window policy (move sessions, never kill), remote active-desktop follows focus | `lib.rs` window events, `remote/state.rs` | closing window #2 leaves its sessions in `main`; remote client follows focus switch |

Steps 1 to 6 are refactors that change nothing visible while only `main` exists, so they can merge to `main` one at a time. Steps 7 to 10 ship the feature and can sit behind a setting (`experimental.multiWindow`) until QA passes.

## 5. Verification

- **Per step:** `cargo test --lib` for the touched modules and `bun run --cwd ui test` on a picked remote build host (`scripts/pick-build-host.sh`), never on the user's MacBook. Clippy with `-D warnings` once at the end.
- **Mock-runtime tests:** use two `WebviewWindowBuilder` windows (`main` and `ws-test`) and cover ownership, render dispatch, emit routing and the notification leader.
- **Real QA on remote hosts:** macOS on maho-mac, Windows on maho-win, Linux X11 and Wayland on a test host, never against the user's production daemon. Each pass runs this scenario:
  1. Open a second window.
  2. Start an agent in it.
  3. Type, using Korean IME as well.
  4. Resize, scroll and select.
  5. Get an agent-done notification and click it. It must raise window #2.
  6. Move the tab to `main`.
  7. Close window #2.
  8. Restart the app and confirm both windows and all sessions come back.
  9. Run a daemon handover with both windows open.
- **Zero-loss check:** count the daemon sessions before and after each QA run. The two counts must match.

## 6. Risks

| Risk | Mitigation |
|---|---|
| A missed `"main"` lookup silently routes to the wrong window | After step 6, a CI grep gate fails on `get_window("main")` / `get_webview_window("main")` outside `window_registry.rs` and tests |
| Duplicate notifications or sounds from broadcast events | Notification leader (step 5) plus a test |
| Wayland subsurface recreation flicker or failure on move | Moves go through detach then recreate with ring-buffer replay, never reparenting; QA covers Wayland explicitly |
| Session file corruption from concurrent saves | Backend-side merge under a lock; atomic write path already used by `save_session_to_path` |
| GPU or memory cost per extra window | One renderer per window is acceptable for v1; measure with the resource monitor during QA |
| Remote client confusion when focus jumps between windows | Selection follows the focused window, same semantics as a workspace switch today |

## 7. Open decisions (defaults chosen; change before step 7 if needed)

1. **Closing a non-last window that has running sessions.** Default: move its tabs to the last focused window. Alternative: ask the user.
2. **Feature flag.** Default: steps 7 to 10 ship behind `experimental.multiWindow` for one release.
3. **Mirroring one session in two windows.** Out of scope for v1. Revisit only if users ask for it.
