# URL Double-Click Whole-Selection Fix (2026-09-15)

**Report (Korean):** "url 더블클릭시 전체 선택이 안되는데?" — double-clicking a URL in
terminal output selected only a fragment instead of the whole URL. Scope assumed:
**native terminal output**, not the built-in browser address bar.

## Root Cause

Native terminal selection gestures are delegated to libghostty-vt
(`ghostty_selection_gesture_event`, `src-tauri/src/native_terminal/selection.rs`).
The gesture's double-click behavior is `WORD`, which uses Ghostty's default word
boundary codepoints (`src-tauri/vendor/ghostty/src/terminal/selection_codepoints.zig`):

```
0 ' ' '\t' '\'' '"' '│' '`' '|' ':' ';' ',' '(' ')' '[' ']' '{' '}' '<' '>' '$'
```

`:` is a boundary, so `https://ferryx.dev/docs?tab=a#b` splits at the scheme colon.
Double-clicking the scheme yielded only `https`; clicking the host yielded
`//ferryx.dev/docs?tab=a#b`.

Upstream Ghostty solves this in `Surface.zig` (`clickMouseButton`, click count 2)
by overriding the gesture's word selection with a *link* selection derived from the
configured URL regex. Ferryx embeds `libghostty-vt` only — it has no `Surface`,
no link config, and therefore had no such override. The bug is that missing override.

## Fix

Ferryx now performs the same double-click override on its own side of the FFI.

1. `src-tauri/src/native_terminal/url.rs` (new): `url_span_at(text, char_index)`
   returns the character range of the URL covering a clicked character. Accepted
   shape intentionally mirrors `ui/src/lib/linkRouting.ts` so double-click selection
   and Cmd+click opening agree on where a URL ends, including the trailing-punctuation
   and unbalanced-bracket trimming rules.
2. `src-tauri/src/native_terminal/selection.rs`:
   - `gesture_click_count()` reads `GHOSTTY_SELECTION_GESTURE_DATA_CLICK_COUNT`
     via the new `ghostty_selection_gesture_get` binding.
   - `logical_line_cells()` reconstructs the soft-wrap-aware logical line in
     **screen** coordinates together with the screen cell each character came from,
     bounded to +/-8 rows around the pressed row so a pathological wrapped line cannot
     make a press O(scrollback).
   - `url_selection_at()` maps the URL span back to start/end `GhosttyGridRef`s.
   - In the `Press` branch of `apply_mouse_gesture`, when click count == 2 and the
     press lands inside a URL, the widened selection is installed instead of the
     gesture's word selection. Everything else (single click, drag, triple click,
     word-granular double-click drag) is untouched.
3. `src-tauri/src/native_terminal/search.rs`: the private per-cell grapheme reader was
   made reusable as `screen_cell_text()`; behavior of `search_grid` is unchanged.
4. `sys/ffi.rs` / `sys/types.rs`: added the `ghostty_selection_gesture_get` extern and
   the `GHOSTTY_SELECTION_GESTURE_DATA_CLICK_COUNT = 0` constant (matches
   `vendor/ghostty/include/ghostty/vt/selection.h`).

Selections are installed in screen coordinates, so a URL selected in scrollback
survives scrolling exactly like any other selection.

## Changed Files

| File | Change |
|---|---|
| `src-tauri/src/native_terminal/url.rs` | **new** — URL span detection + unit tests |
| `src-tauri/src/native_terminal/selection.rs` | double-click URL override, click-count read, logical line reconstruction |
| `src-tauri/src/native_terminal/search.rs` | expose `screen_cell_text` (no behavior change) |
| `src-tauri/src/native_terminal/sys/ffi.rs` | `ghostty_selection_gesture_get` binding |
| `src-tauri/src/native_terminal/sys/types.rs` | `GHOSTTY_SELECTION_GESTURE_DATA_CLICK_COUNT` |
| `src-tauri/src/native_terminal/mod.rs` | register `url` module |
| `src-tauri/src/native_terminal/terminal.rs` | 3 behavioral regression tests |
| `src-tauri/tests/native_terminal_input_boundary_contract.rs` | IPC-boundary regression test |

## Red / Green Evidence

**Red** (fix disabled locally with `if false &&` on the click-count guard, then restored) —
`docs/evidence/url-double-click-20260915/red-run.log`:

```
$ cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::terminal::tests::native_terminal_double_click
test native_terminal::terminal::tests::native_terminal_double_click_selects_whole_url ... FAILED

assertion `left == right` failed: double click on the scheme segment must select the whole URL
  left: Some("https")
 right: Some("https://ferryx.dev/docs?tab=a#b")

test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 1296 filtered out
```

**Green** — `docs/evidence/url-double-click-20260915/green-lib.log`:

```
$ cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::
test result: ok. 182 passed; 0 failed; 0 ignored; 0 measured; 1135 filtered out; finished in 5.95s
```

**Green (IPC boundary)** — `docs/evidence/url-double-click-20260915/green-boundary.log`:

```
$ cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_input_boundary_contract
test test_double_click_selection_via_surface_host_boundary ... ok
test test_double_click_on_url_selects_whole_url_via_surface_host_boundary ... ok
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Also run:

- `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_surface_host_contract` — 18 passed, 0 failed.
- `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` — 0 errors.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --lib` — no new warnings in `native_terminal`.
- Full `cargo test --lib` — 1298 passed, 1 failed:
  `daemon::manifest::tests::handover_manifest_update_excludes_other_file_handles`
  (`transaction released lock: "WouldBlock"`). **Pre-existing flock flake, unrelated to
  this change** — it passes when run standalone.

### Regression tests added

| Test | Asserts |
|---|---|
| `native_terminal_double_click_selects_whole_url` | Double click at scheme / host / path / query columns each select `https://ferryx.dev/docs?tab=a#b` |
| `native_terminal_double_click_outside_url_still_selects_word` | Double click on `open` still selects `open` (no over-widening) |
| `native_terminal_double_click_selects_url_across_a_soft_wrap` | 20-column terminal: soft-wrapped URL still selects whole |
| `url::tests::*` (4 tests) | Span covers whole URL from any offset; `None` outside; trailing punctuation and unbalanced `)`/`]` trimmed; Wikipedia-style balanced parens kept; non-http schemes; correct char indices after wide CJK text |
| `test_double_click_on_url_selects_whole_url_via_surface_host_boundary` | Same behavior through the real IPC path (`select_attached_native_terminal_with_mouse` + `copy_attached_native_selection`) |

## Manual Native Desktop Verification Steps

Not performed in this session (no desktop launch, per task constraints). To verify by hand:

1. Build/run the debug desktop app with exactly `bun tauri dev`.
   Do **not** kill or restart the `ferryx --daemon` process — it owns all PTY fds.
2. In any terminal pane, print a URL:
   `echo "open https://ferryx.dev/docs?tab=a#b now"`
3. Double-click on the **scheme** (`https`). Expect the entire
   `https://ferryx.dev/docs?tab=a#b` to be highlighted, not just `https`.
4. Repeat on the **host** (`ferryx.dev`), the **path** (`docs`) and after the `?`.
   All four must highlight the same full URL.
5. Press Cmd+C (or the copy shortcut). Paste elsewhere and confirm the clipboard
   holds the entire URL.
6. Negative check: double-click `open` or `now`. Only that word must highlight.
7. Punctuation check: `echo "see https://ferryx.dev/docs."` then double-click inside
   the URL — the trailing `.` must be excluded.
8. Wrap check: narrow the pane until a long URL wraps to two rows, then double-click
   on the first row — the selection must continue onto the second row.
9. Triple-click still selects the whole line; click-drag still selects cell-by-cell;
   double-click-drag still extends by word.

## Limits / Known Gaps

- URL detection is regex-based and mirrors the UI link router. It does **not** consult
  OSC 8 hyperlink metadata; an OSC 8 link whose visible text is not itself a URL
  still double-click-selects as a word.
- Recognized schemes: `http(s)`, `ftp`, `file`, `ssh`, `git`, `gemini`, `gopher`,
  `ipfs`, `ipns`, `mailto`. Bare `example.com` (no scheme) is not treated as a URL.
- File paths are not widened on double click; only URLs are. Cmd+click file-path
  opening is unchanged.
- The logical-line scan is capped at +/-8 rows around the press, so a URL soft-wrapped
  across more than ~9 visual rows widens only within that window.
- These changes are **uncommitted** in a shared working tree and are vulnerable to
  concurrent sessions. `surface_host.rs` and the other dirty files in
  `git status` belong to other sessions and were left untouched.
