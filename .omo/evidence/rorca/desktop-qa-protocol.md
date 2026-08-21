# rorca desktop QA protocol

Run this only after native/frontend tests and builds are green. Store every
command/action result and final screenshot path in
`.omo/evidence/rorca/desktop-qa-log.md`.

## Launch and identity

1. Stop the baseline dev instance, then launch:
   ```sh
   cargo tauri dev
   ```
2. Locate the rorca PID with `orca computer list-apps --json`.
3. Capture:
   ```sh
   orca computer get-app-state --app pid:<pid> --json
   ```
4. PASS requires a window titled `rorca`, a visible crab-Orca app icon, the
   compact charcoal rail/tab/canvas hierarchy, and no legacy `Orca Lite` /
   `omo bridge` title text. Save the screenshot as
   `.omo/evidence/rorca/final-shell.png`.

## Terminal shortcut and pane controls

For each action, first click a terminal canvas:
```sh
orca computer click --app pid:<pid> --x <canvas-x> --y <canvas-y> \
  --restore-window --json
```

Then issue one literal chord via the OS channel, capture the accessibility tree
and screenshot, and assert the expected visible change:

| Action | Chord / click | Required visible assertion |
| --- | --- | --- |
| new tab | `CmdOrCtrl+T` | A new tab becomes active |
| next / previous tab | configured tab chords | The active tab indicator changes |
| close tab | `CmdOrCtrl+W` | Active tab is removed, with no unexpected PTY on another tab |
| horizontal split | configured horizontal split chord | Two side-by-side pane regions |
| vertical split | configured vertical split chord | Two stacked pane regions |
| unsplit | configured close split chord | One pane remains; no extra PTY was created |
| pane-local split | click a pane header split control | Split affects that pane context |
| raw input | type normal text and Ctrl-C | Input reaches terminal; no app action fires |

The final accessibility tree must contain pane-local controls and must not
contain `Interrupt terminal` or a global `Split terminal` workspace-header
button.

## Workspace and Git actions

1. Click **Add project**, choose `/Users/indo/code/project/orca-lite` through
   the implemented picker, then capture state and screenshot.
2. Click **Add worktree**; verify a dropdown/combobox lists native branch
   values. It must not expose a free-form branch text entry.
3. Choose a branch and create the worktree; capture the resulting worktree
   list and open tab.
4. Click Workspace and Search/command palette; verify each changes visible
   state and that a palette result can activate the workspace/tab.

## Settings and window behavior

1. Open Settings. Capture screenshot and tree.
2. Verify compact left navigation plus right detail sections and show:
   effective Ghostty font family `Noto Sans KR`, Option-as-Alt enabled, and
   source/import state.
3. Modify an explicit rorca terminal override, reopen Settings, and verify it
   wins over the imported value.
4. Drag only the designated titlebar background. PASS requires changed window
   coordinates in `get-app-state`; then click/resize buttons, splitters, and
   inputs to prove their `no-drag` exclusions.
5. Resize the sidebar divider and verify its width changes without initiating
   a window drag.

## Visual comparison

At 1280×850, inspect `final-shell.png` and `final-settings.png` beside the
reference:
`/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/clipboard-2026-08-21-113227-8076FA53.png`.

Record a region-by-region verdict for: titlebar, rail width, search, project
list, tab strip, terminal chrome, canvas, settings nav/detail, type scale,
spacing, borders, and colors. Any material card/modal treatment or mismatched
layout is a FAIL requiring frontend remediation before the completion audit.
