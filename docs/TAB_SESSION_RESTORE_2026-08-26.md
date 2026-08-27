# Tab Session Restore

## User-visible behavior

Restarting the desktop app preserves every persisted terminal tab and pane split. If its
daemon-backed PTY is still live, the restored tab reconnects to that same process rather than
opening a replacement shell. This includes running agent processes.

The tab's agent activity marker is now persisted with the workspace snapshot as well. A working,
waiting, or completed marker remains visible immediately after restart, including for a
background or parked-worktree tab, until the native terminal's next activity event refreshes it.

## Restore contract

1. `serializeWorkspaceState` writes terminal local IDs, backend PTY IDs, daemon epochs, pane
   ownership, and `activityBySessionId` for session IDs referenced by a persisted layout.
2. `deserializeWorkspaceState` reconciles each backend ID with the daemon's live-session list.
   A matching live PTY keeps its backend ID and is reattached; stale IDs are deliberately
   cleared so recovery can create a new terminal safely.
3. Unreferenced sessions and their activity entries are not persisted or restored.
4. Snapshots created before `activityBySessionId` existed remain valid and restore with no
   activity markers.

## Process-lifetime boundary

PTY processes are intentionally owned by the standalone daemon rather than the desktop window.
The daemon reconnect contract proves a client disconnect leaves live PTYs available for a new
desktop client. If the daemon itself has been restarted or stopped, the original process cannot
be resumed; Ferryx restores the tab layout and starts a new terminal for that tab instead.

## Verification evidence

- `bun run --cwd ui test -- sessionPersistence.test.ts workspaceRestore.test.tsx`
  - validates live backend reconciliation, stale-backend recovery, activity-state round trips,
    orphan filtering, and legacy snapshots.
- `bun run --cwd ui build`
  - validates the TypeScript surface and production bundle.
- `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_persistence_contract -- --test-threads=1`
  - validates daemon-owned PTYs survive desktop-client disconnect and can be listed, reattached,
    read, written, and resized by a new client.

## Required desktop QA

Desktop interaction is intentionally not automated. To verify the shipped app:

1. Open two terminal tabs and split one of them.
2. Start a long-running agent or command in the background tab.
3. Close and reopen the app without stopping the daemon.
4. Confirm the split layout, running process output, and the background tab's agent marker are
   present immediately after startup; then switch to the tab and confirm the process accepts
   input.
