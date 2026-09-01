# Hangul Typing "Appears Late" — Root Cause (2026-08-31)

## Symptom
User report (macOS, native terminal): typing Hangul feels delayed — characters appear on screen
later than expected. English typing does not feel delayed.

## Root cause: IME composition gating + invisible preedit
The native terminal pane sends printable keys directly on keydown, but **only when the key is not
part of an IME composition** (`ui/src/components/NativeTerminalPane.tsx`, sink `onKeyDown` and the
document capture fallback both require `!isComposing`).

- English: keydown → `sendInput({ text })` immediately → PTY echo → render. ~1 round trip (~10–30 ms).
- Hangul (2-set IME): every Jamo keydown has `isComposing=true` → nothing is sent. The composed
  text lives as IME marked text inside the focus-sink `<textarea>`, which is styled
  `pointer-events-none absolute h-px w-px opacity-0` — **the preedit is invisible**.
- The commit only fires on `compositionEnd` — i.e. when the **next** consonant starts a new
  syllable, or on space/Enter/arrow/Escape.

Net effect: each Hangul syllable becomes visible exactly when the user presses the next key
(200–600 ms at normal typing speed), while English appears within one echo round trip. This is the
perceived "늦는" lag. The render pipeline is common to both paths and is NOT the differentiator:
daemon read→send measured p50 ~12.5 ms per batch (2026-08-24 bench), vt parse + WGPU present are
sub-frame, and there is no rAF coalescing on the native-pane output path.

## Why xterm.js panes don't feel this way
xterm.js positions its textarea at the cell cursor and renders the composition inline, so macOS IME
marked text is visible while typing. The native pane has zero preedit support: grep for
preedit/marked-text across `src-tauri/src/native_terminal/`, `src-tauri/src/ipc/`, and `ui/src/`
returns nothing (verified 2026-08-31).

## Fix directions (not implemented)
1. **Cursor-anchored visible preedit (cheap, iTerm/xterm.js-style)**: expose the active cursor cell
   pixel origin from the Rust surface (IPC query or event), position the focus-sink textarea there,
   and make its text visible (transparent background, normal foreground). macOS IME then draws the
   in-progress syllable at the cursor. Needs careful z-order/blur handling and caret hiding.
2. **On-grid preedit (proper)**: forward composition updates to the native surface and render
   marked text on the ghostty-vt grid itself, as upstream Ghostty does. Requires preedit state in
   the vt/renderer — none exists today.

Related but distinct: the 2026-08-31 C-locale gap defect (missing LANG in daemon PTY env) causes
*growing spacing* after commit — different symptom, see memory
`reference/project/ferryx-macos-hangul-spacing-locale.md`.

---

# IMPLEMENTED (2026-08-31, same day) — on-grid preedit

## Design
Preedit is pure GUI-side display state; daemon/vt/PTY untouched (cross-platform by construction).
- `NativeTerminalSession.preedit: Option<String>` (surface_host.rs) + focus-gated injection.
- `apply_preedit_to_snapshot`: overwrites snapshot cells at `cursor.x/y` with `underline: true`,
  default colors; wide chars (Hangul jamo/syllables, CJK, fullwidth — `preedit_char_wide`) get
  `CellWide::Wide` + underlined `SpacerTail`; clamps at row end; cursor struct never modified
  (cursor stays on the first preedit cell, upstream-Ghostty behavior).
- `NativeTerminalSurfaceHostState::set_preedit` mirrors `set_focus`: main-thread mutation via IPC,
  repaint through `RenderScheduleCoordinator::schedule_render` → `dispatch_scheduled_render`.
- IPC `cmd_native_terminal_set_preedit(sessionId, preedit: Option<String>)` registered in lib.rs.
- Frontend (NativeTerminalPane.tsx): `onCompositionUpdate → setPreedit(event.data || null)`;
  `onCompositionEnd → setPreedit(null)` BEFORE the committed-text send; `onBlur → setPreedit(null)`.
  Standard DOM composition events only — zero platform branches (WKWebView/WebView2/WebKitGTK).

## Gates (re-run independently after delegation, all green)
- `cargo test --lib native_terminal`: 77 passed, 0 failed.
- `cargo test --lib preedit`: 5 passed (narrow overwrite, Hangul wide+SpacerTail, row-end clamp,
  empty no-op, full cell replacement).
- `bunx tsc` exit 0; vitest NativeTerminalPane + lifecycle: 95 passed
  (compositionupdate→preedit invoke, end→clear+commit, blur→clear).

## Manual E2E (user, via exactly `bun tauri dev`)
1. Type Korean at a zsh prompt: each in-progress syllable should appear AT THE CURSOR with
   underline the moment its jamo is typed (not one keystroke late).
2. Finish a syllable: preedit disappears and the committed text appears from echo — no double
   rendering, no residue.
3. Blur mid-composition (click another pane / Cmd+Tab back): no stuck underlined text.
4. English typing unchanged; split panes: preedit only renders in the focused pane.
5. Long preedit at the right edge of the window must not wrap or panic (clamped).
