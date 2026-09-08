# Modifier-held shortcut hints

## Interaction contract

Show existing shortcut labels at their actual UI targets, not in a central
cheatsheet and not as sequential key navigation. Hold Command, Control, or
Option on macOS (Control or Alt elsewhere) for 300 ms. Additional held modifiers
refine the visible set; labels include the complete existing binding. Aliases
come from the same registry as execution.

Use existing popover, text, ring, radius, shadow, and compact typography tokens.
Badges are fixed-position, pointer-transparent, aria-hidden annotations. They
never move application layout, focus an element, consume a key, reserve terminal
space, or act as dialogs. Hover-only control locations may show their labels
without revealing or enlarging the pane drag toolbar.

Only actual, enabled targets in the active shortcut context qualify. Tab and
workspace numbers follow execution order, not an independently numbered visual
list. Text editing, composition, AltGraph, dialogs, inactive pane scopes, hidden
and offscreen targets must not advertise unavailable actions.

Release, an action key, pointer interaction, window blur, visibility changes,
scroll, resize, or target layout changes dismiss the annotations.

## Verification

Implemented in `ui/src/components/ShortcutHints.tsx`, mounted once by `App.tsx`.
Target metadata is attached to Sidebar, WorktreeList, TabBar/SortableTab,
TerminalSplitView, BrowserToolbar, and EmptyWorkspaceView. Existing bindings and
execution handlers are unchanged. Actions without an existing on-screen target
do not receive invented controls.

- RED: the initial no-op implementation failed 10 of 12 behavioral assertions.
- GREEN: the final hint suite passes 14 tests; the existing shortcut suite passes
  63 tests. Command: `bun run --cwd ui test src/components/ShortcutHints.test.tsx src/lib/shortcuts.test.tsx`.
- Related UI regression run: 9 files / 176 tests passed before the two additional
  platform/context cases were added. Targets: TabBar, Sidebar, WorktreeList,
  BrowserToolbar, EmptyWorkspaceView, TerminalSplitView, paneHandleReach,
  ShortcutHints, and shortcuts.
- `bun run --cwd ui build`: final exit code 0, 1,872 modules transformed.
  An earlier unrelated concurrent edit lacked `visible` in
  `nativeTerminalVisibility.test.tsx`; its author corrected it during this task.
  This session did not edit that file.
- `git diff --check`: clean.
- LSP diagnostics were attempted automatically on changed files but the shared
  LSP daemon was unreachable. The final TypeScript build is the compiler check.

## Browser evidence and limits

An isolated Vite fixture uses the real Sidebar, TerminalSplitView (including
TabBar and pane controls), BrowserToolbar, and ShortcutHints components. Tauri IPC
is stubbed and terminals are exited fixtures: no live user PTYs, native compositor,
native menus, or embedded child-browser keyboard forwarding were exercised.
Fixture entry: `ui/qa-shortcut-hints.html`; configuration:
`ui/qa-shortcut-hints.config.mjs` (loopback port 5187, HMR disabled).

Bun.WebView exercised DOM keyboard events with MutationObserver subscriptions
before each action and bounded timeouts, not fixed sleeps:

- macOS dark, 1280 x 850: Command produced 13 badges; Control produced tab
  selectors; Option produced the browser-back alias and terminal-unsplit binding.
- Command + Shift refined the set; modifier release removed all badges.
- Command + T invoked the real shortcut hook and updated the fixture's new-tab
  counter exactly once, with no remaining badge.
- Target DOM rectangles were identical before and after hint display.
- macOS light, 800 x 600: 13 badges, no badge intersections, viewport overflow,
  or text clipping by measured DOM geometry.
- Linux dark, 800 x 600: Ctrl labels fit within the viewport without text
  clipping. Existing Ctrl+digit tab bindings take precedence over colliding
  workspace bindings, so misleading workspace badges are omitted.
- Screenshots reside in `docs/evidence/shortcut-hints/`. PNG capture succeeded;
  the current model could not receive image pixels, so no human-equivalent
  aesthetic verdict is claimed from these screenshots.

## Desktop check requested

Using the debug app started with exactly `bun tauri dev`, hold Command, Control,
and Option while the terminal has focus. Check that badges are visible near
the appropriate targets, release cleanly, and do not disturb terminal output or
Hangul input. Also try while an embedded browser page has focus, switch between
split panes, and verify the indicated action operates on the indicated target.
Native desktop verification remains manual; browser-fixture success is not
proof of native input delivery or native-surface layering.

Changes are uncommitted in a shared working tree. Foreign edits were preserved.
