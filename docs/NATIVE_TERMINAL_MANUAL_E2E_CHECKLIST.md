# Native terminal (libghostty) manual E2E checklist

**Why this document exists:** the machine gates in this migration prove the Rust engine, the wgpu renderer,
the IPC boundary and the React wiring in isolation. They cannot prove what you see on screen. Desktop GUI
automation is deliberately not used in this project, so the on-screen round trip is verified by you, by hand.
Nothing in this file has been claimed as verified by the agent.

**Build and launch**

```bash
cargo tauri dev --manifest-path src-tauri/Cargo.toml
```

Record for each item: PASS / FAIL, plus what you actually saw when it fails.

---

## A. Every pane is native (criterion 1)

| # | Action | Expected |
|---|---|---|
| A1 | Open a terminal tab and type `printf 'native ok\n'` | The text and the following prompt render in the terminal |
| A2 | Split the pane right (`Cmd+D`) so two panes are visible | BOTH panes show terminal content at the same time, not one blank |
| A3 | Click the left pane, type `echo left`; click the right pane, type `echo right` | Each command lands in the pane you clicked, with no cross-talk |
| A4 | Split down as well so three panes are visible | All three render text simultaneously |
| A5 | Switch to another tab and back | The returning tab still shows its prior scrollback, not an empty screen |

The pre-migration behavior was that only the focused pane used the native surface and only in a dev build.
A2 and A4 are the items that would have failed before this work.

## B. Scroll and scrollback (criterion 2)

| # | Action | Expected |
|---|---|---|
| B1 | Run `seq 1 500` | Output ends at 500 with the prompt visible at the bottom |
| B2 | Scroll up with the mouse wheel / trackpad | Earlier lines appear; the view moves smoothly, no tearing or blank rows |
| B3 | Scroll back to the bottom | Line 500 and the live prompt are visible again |
| B4 | With the view scrolled up, type a character | The view jumps to the bottom and your character appears at the prompt |

## C. Selection, copy, paste (criterion 2)

| # | Action | Expected |
|---|---|---|
| C1 | Drag-select a word of visible output | The selection is visibly highlighted |
| C2 | Press the platform copy shortcut, then paste into any text editor | The pasted text matches exactly what you selected |
| C3 | Copy `hello-from-clipboard` in another app, focus the terminal, press the paste shortcut | The text appears at the prompt and is NOT executed until you press Enter |
| C4 | Run `cat`, paste a multi-line block, then Ctrl+C | Lines arrive intact; bracketed paste does not execute them line by line |

C3 specifically regression-tests a real prior defect: the native pane used to drop Cmd/Ctrl+V silently.

## D. Mouse reporting (criterion 2)

| # | Action | Expected |
|---|---|---|
| D1 | Run a mouse-aware program (`vim`, or `htop` if installed) | It starts and renders |
| D2 | Click inside it | The cursor / selection moves to where you clicked |
| D3 | Scroll inside it | The program scrolls its own view rather than the terminal scrollback |
| D4 | Quit it | The shell prompt returns and normal terminal scrolling works again |

## E. Search (criterion 2)

| # | Action | Expected |
|---|---|---|
| E1 | Run `seq 1 300`, open terminal search, type `25` | Matches are reported and the counter shows a total |
| E2 | Press Enter / next repeatedly | The view moves through matches in order |
| E3 | Search for a string that does not exist | The counter reports 0 with no crash and no stuck highlight |
| E4 | Press Escape | The overlay closes and keystrokes go back to the shell |

## F. Title and bell (criterion 3)

| # | Action | Expected |
|---|---|---|
| F1 | Run `printf '\033]2;my-manual-title\007'` | The tab label becomes `my-manual-title` |
| F2 | Run `printf '\a'` | The tab shows its bell/activity indication (and plays the alert sound if enabled) |
| F3 | Start a long agent/command in tab 1, switch to tab 2, wait for it to finish | Tab 1's activity indicator updates while it is NOT focused |

F3 is the case that receipt-based reporting could not cover: the title changes while you are not typing.
It is the reason title/bell are pushed as events rather than returned from an IPC reply.

## G. Theme, font and settings (criterion 4)

| # | Action | Expected |
|---|---|---|
| G1 | Compare the terminal background with your Ghostty config background | They match |
| G2 | Run `ls --color` or any colored output | ANSI colors match your configured palette, not a generic default |
| G3 | Change the terminal font size in Settings | Glyphs resize and the grid reflows to the new cell size |
| G4 | Check the cursor | It matches your configured cursor style; it is hollow/dimmed when the pane is not focused |
| G5 | Resize the window slowly | Text reflows without tearing, clipping, or leftover pixels at the edges |

## H. CJK, wide glyphs and IME (known Phase-3 gap)

| # | Action | Expected |
|---|---|---|
| H1 | Run `printf '東京 hello 🦀\n'` | Wide glyphs occupy two columns and do not overlap neighbors |
| H2 | Switch to a Korean/Japanese/Chinese IME and type into the shell | The candidate window appears anchored at the terminal cursor, not at the window corner |
| H3 | Commit composed text | It arrives once, not duplicated, and not dropped |
| H4 | Move the pane or resize, then compose again | The candidate window follows the new cursor position |

## I. Overlays and chrome (known Phase-3 gap)

| # | Action | Expected |
|---|---|---|
| I1 | Open Settings over an active terminal | The dialog is fully visible and the terminal does not paint over it |
| I2 | Open the tab-bar `+` menu and the command palette | Each appears above the terminal surface |
| I3 | Close each overlay | The terminal repaints and accepts keystrokes again |
| I4 | Drag a pane divider | The panes resize without the native surface lagging behind the layout |
| I5 | Drag a tab out to a new split | No pane goes blank and no terminal process dies |

## J. Multi-display and DPI (known Phase-3 gap)

| # | Action | Expected |
|---|---|---|
| J1 | Move the window to a display with a different scale factor | Text stays sharp and geometry stays correct |
| J2 | Move it back | Same |

## K. Remote / browser path is untouched (criterion 6)

| # | Action | Expected |
|---|---|---|
| K1 | Enable remote access in Settings and pair a phone or another browser | The remote terminal connects |
| K2 | Type in the browser terminal | Input and output work, still rendered by xterm.js in the browser |
| K3 | Run `printf '東京\n'` there | Wide glyphs are not clipped |

The browser cannot host the native Rust renderer, so the remote path intentionally keeps xterm.js.
K1-K3 confirm the desktop deletion did not break it.

---

## If something fails

Report the item number, what you saw, and whether the terminal recovered. The daemon owns the PTY, so a
renderer or webview problem should not kill your shell: on a GUI-side failure the session is expected to
survive a window reload.
