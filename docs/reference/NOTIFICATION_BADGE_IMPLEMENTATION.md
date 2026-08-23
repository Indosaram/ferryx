# Notification Badge Implementation

## Goal

Show the number of unread Ferryx tabs on the application icon without creating
a second unread-state store.

## Design

1. `WorkspaceState.unreadTabIds` remains the sole source of truth. The
   application derives the badge count from entries whose value is `true`; a
   worktree unread marker is intentionally not counted again.
2. Whenever that derived count changes, the desktop runtime invokes the typed
   `cmd_notification_set_badge_count` command. This covers notification-driven
   unread marks, visiting or activating a tab/worktree, closing unread tabs,
   and restored state at startup.
3. The native command accepts only `{ count: u32 }`; it formats the Dock label
   itself. This prevents presentation strings from crossing the IPC boundary.
4. On macOS, the command sets `NSApplication.dockTile.badgeLabel` to the
   decimal count and clears it with `nil` at zero. On other desktop platforms,
   it succeeds as a structured unsupported no-op so unread-state behavior
   remains identical.
5. Failures to invoke the native command follow the existing runtime-error
   reporting path. They do not change or discard unread state.

## Verification

- Rust domain and IPC tests cover positive, zero, serialization, and
  unsupported-platform outcomes.
- Frontend tests cover the typed command payload and synchronization from the
  unread-tab source of truth.
- A minimal runtime driver sends a nonzero count then zero, proving the
  platform adapter returns the expected structured state and accepts clearing.
