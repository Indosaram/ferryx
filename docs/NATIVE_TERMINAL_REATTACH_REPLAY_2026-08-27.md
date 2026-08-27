# Native Terminal Reattach Replay

## Defect

When a native terminal pane unmounted during tab or pane switching, its terminal session correctly
continued to receive daemon output in the background. On remount, however,
`cmd_native_terminal_attach` opened another daemon attachment with no sequence cursor. The daemon
returned its complete retained history, and the native surface host appended that history to the
already live Ghostty terminal. The result was a duplicated agent startup/output block.

## Fix

The native surface host now reactivates a retained session locally when its existing daemon stream
and pump tasks are both alive. It restores the compositor bounds without opening a second daemon
attachment or replaying history.

If the retained stream is no longer healthy, the normal daemon recovery attachment still runs. In
that path, the native terminal resets before it receives the daemon's authoritative history, so
stale local rows cannot be appended to the recovered output.

## Regression coverage

- `native_terminal_reattach_preserves_retained_output_without_replaying_history` proves a
  backgrounded terminal retains output and contains its startup block exactly once after reattach.
- `native_terminal_recovery_attach_replaces_stale_terminal_history` proves a recovery attachment
  replaces stale native history rather than appending to it.

## Manual confirmation

1. Open a terminal that starts an agent and wait until its startup output is visible.
2. Switch to another tab, then return to the agent tab several times.
3. Confirm the initial agent block appears once and background output continues while the tab is
   inactive.
