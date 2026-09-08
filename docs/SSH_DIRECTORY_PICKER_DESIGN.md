# SSH project directory picker

## Scope

Select an existing directory on a saved SSH host and register it through the existing remote
project API. There is no file editor at any product level, file-content browser, SSHFS mount,
clone operation, or new remote agent.

## Interaction

- Add Project > Remote opens the selected host's home directory automatically.
- A saved, enabled host in SSH settings offers Open Project into the same flow.
- The path field is a combobox, prefilled with the actual remote home after connection.
- Typing a trailing separator lists that remote directory immediately. Typing the last
  path segment filters its parent's children in the dropdown directly under the input.
- Windows accepts both slash forms; POSIX treats backslash as a filename character.
- Clicking a candidate or completing it with Tab/Enter appends a separator and lists
  its children. Arrow keys move the active candidate while focus stays in the input.
- Enter without a candidate validates/navigates the typed path, never registers a project.
- Escape closes suggestions without closing the dialog; input focus/click reopens them.
- Home, parent and refresh controls navigate; hidden directories are an explicit toggle.
- There is no separate Go button, filter input, or permanently visible directory browser.
- Add this folder registers only the successfully listed canonical path. Editing a path,
  loading, errors and host changes invalidate that selection.
- Host changes remount the picker, discard its listing cache and invalidate outstanding replies.
- Loading, empty, filtered-empty, permission/transport errors and truncated results are distinct.
- Internal workspace IDs are derived, not editable in the remote form.

## Visual contract

Follow `ui/DESIGN.md` and the incumbent AddProjectDialog. Use semantic card/background/border,
foreground/muted-foreground and destructive colors, existing Button/Input primitives, Lucide
icons, 12px body and 11px supporting copy, 4px spacing grid and h-8 controls.
Remote dialog is max-w-xl with viewport-constrained height. Only its content/list scrolls;
the action footer stays visible. Long paths use a scrollable input and wrapping selected-path
text, and directory names truncate with a full title. No new colors or motion.

## Verification

Regression coverage must exercise actual navigation-to-registration, exact remote paths,
out-of-order responses, host removal/configuration changes, keyboard Enter, refresh/errors,
hidden/filter/cache behavior and settings entry. Real SSH listing checks cover POSIX and
Windows where configured endpoints are available. Desktop interaction is manual per user
instruction; headless browser checks use the real components with a clearly identified IPC fixture.
