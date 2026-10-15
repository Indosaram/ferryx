---
title: Keyboard Shortcuts
description: Every verified Ferryx keyboard shortcut, covering terminal and browser tabs, split panes, workspace switching, terminal search, and the command palette.
---

These are the default bindings defined in `ui/src/lib/shortcuts.ts`. On Windows and Linux, <kbd>⌘</kbd> maps to <kbd>Ctrl</kbd>. You can change most of them; see [Remapping shortcuts](#remapping-shortcuts).

## Tabs

| Keys | Action |
| --- | --- |
| <kbd>⌘T</kbd> | New terminal tab |
| <kbd>⌘⇧B</kbd> | New browser tab |
| <kbd>⌘W</kbd> | Close active tab |
| <kbd>⌘⇧]</kbd> | Next terminal tab |
| <kbd>⌘⇧[</kbd> | Previous terminal tab |
| <kbd>⌃Tab</kbd> / <kbd>⌃⇧Tab</kbd> | Cycle tabs |
| <kbd>⌃1..9</kbd> | Select terminal tab N |
| <kbd>⌘1..9</kbd> | Select workspace N |

## Terminal Panes

| Keys | Action |
| --- | --- |
| <kbd>⌘D</kbd> | Split terminal right |
| <kbd>⌘⇧D</kbd> | Split terminal down |
| <kbd>⌘⌥D</kbd> | Close split view |
| <kbd>⌘]</kbd> | Focus next pane |
| <kbd>⌘[</kbd> | Focus previous pane |
| <kbd>⌘F</kbd> | Find in terminal |
| <kbd>⌘⇧X</kbd> (<kbd>Ctrl+Shift+X</kbd> on Windows/Linux) | Toggle terminal copy mode |

## Global

| Keys | Action |
| --- | --- |
| <kbd>⌘K</kbd> / <kbd>⌘P</kbd> | Command palette |
| <kbd>⌘B</kbd> | Toggle sidebar |
| <kbd>⌘O</kbd> / <kbd>⌘N</kbd> | Add project |
| <kbd>⌘,</kbd> | Settings |
| <kbd>⌘⇧N</kbd> / <kbd>⌘⇧I</kbd> | Toggle notification center |
| <kbd>⌘=</kbd> | Zoom in |
| <kbd>⌘-</kbd> | Zoom out |
| <kbd>⌘0</kbd> | Reset zoom |

## Remapping shortcuts

Open **Settings > Shortcuts** to change bindings. Each action row has:

- **Edit**: record a new key combination for the action. The new chord replaces the default and any alternate chords for that action.
- **Unbind**: remove the binding, so the action has no shortcut.
- **Reset**: restore the default binding for that action.

**Reset all** restores every default at once.

A chord that is already in use is refused, and the row names the action that owns it (for example, "⌘T is already used by New terminal tab."). Unbind or remap the owner first to free the chord.

Some chords are fixed and cannot be assigned to another action or remapped themselves:

- Terminal clipboard chords: <kbd>⌘C</kbd>, <kbd>⌘V</kbd>, <kbd>⌘⇧V</kbd> on macOS; <kbd>Ctrl+V</kbd>, <kbd>Ctrl+Shift+V</kbd> on Windows/Linux; <kbd>Ctrl+Shift+C</kbd> and <kbd>Shift+Insert</kbd> on all platforms.
- macOS app menu items: <kbd>⌘Q</kbd> (Quit), <kbd>⌘H</kbd> (Hide), <kbd>⌘M</kbd> (Minimize).

## Terminal copy mode

Copy mode lets you select and copy terminal text from the keyboard. Press <kbd>⌘⇧X</kbd> on macOS or <kbd>Ctrl+Shift+X</kbd> on Windows and Linux to enter it.

While copy mode is active, the viewport stays frozen: new output does not scroll the view. Paste is disabled, and keys go to copy mode instead of the running program.

| Keys | Action |
| --- | --- |
| <kbd>h</kbd> <kbd>j</kbd> <kbd>k</kbd> <kbd>l</kbd> (or arrow keys) | Move left, down, up, right |
| <kbd>w</kbd> / <kbd>b</kbd> / <kbd>e</kbd> | Next word start / previous word start / word end |
| <kbd>0</kbd> / <kbd>^</kbd> / <kbd>$</kbd> | Line start / first non-blank character / line end |
| <kbd>gg</kbd> / <kbd>G</kbd> | Top / bottom of the scrollback |
| <kbd>Ctrl-u</kbd> / <kbd>Ctrl-d</kbd> | Half page up / down |
| <kbd>H</kbd> / <kbd>M</kbd> / <kbd>L</kbd> | Top / middle / bottom of the visible screen |
| <kbd>v</kbd> / <kbd>V</kbd> / <kbd>Ctrl-v</kbd> | Character / line / block selection |
| <kbd>y</kbd> | Copy (yank) the selection |
| <kbd>Esc</kbd> / <kbd>q</kbd> | Exit copy mode |
