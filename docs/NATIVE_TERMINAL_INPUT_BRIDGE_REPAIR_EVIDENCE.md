# Native Terminal Input Bridge Repair Evidence (macOS)

**Date:** 2026-08-25
**Status:** Implementation + automated gates complete; user manual verification via `cargo tauri dev` is the only remaining gate.

---

## Objective
Fix unmodified printable character input not reaching the Rust/daemon in the macOS native terminal (`NativeTerminalPane`), while preserving lifecycle/visibility detach so native AppKit/Metal sibling views never cover Settings UI or other panes' DOM controls.

---

## Root Cause
`NativeTerminalPane` relied solely on the hidden textarea's `onInput` to forward ordinary characters. In WKWebView, physical-key events do not reliably produce `input` events through the hidden focus sink, so the chain broke at the WKWebView physical-key → input-event boundary. IPC logs confirmed this: only `cmd_native_terminal_set_focus` traffic appeared; zero `cmd_native_terminal_send_input` calls for printable keys.

## Fix
1. **Keydown direct-send path** — `ui/src/components/NativeTerminalPane.tsx`: unmodified printable keydowns (single-char `event.key`, no ctrl/alt/meta, not IME-composing, not Dead/Process, not browser-reserved keys) are now intercepted in `onKeyDown`, `preventDefault()`ed (suppressing double-delivery via onInput), and sent directly as text via `cmd_native_terminal_send_input`. Non-printable/special keys keep the existing `shouldForwardKey` KeyEvent path; IME composition flow is untouched.
2. **Detached-session input guard** — `src-tauri/src/ipc/native_terminal.rs`: `encode_input_for_attached_session` checks `state.snapshot_for_session(session_id)` before `encode_input`; input for a session with no daemon attachment is rejected instead of writing into a detached PTY. This preserves daemon PTY ownership and prevents phantom writes after a lifecycle detach.

## Preserved Invariants
- Daemon remains sole PTY owner; GUI never touches PTY handles.
- Browser/mobile xterm paths untouched.
- Lifecycle/visibility detach keeps native sibling surfaces hidden when Settings opens or another pane owns the session.

---

## Verification Evidence

### UI regression tests — RED→GREEN, 28/28 passing
- `ui/src/components/NativeTerminalPane.test.tsx`: printable keydown dispatches exactly one `cmd_native_terminal_send_input` with `{ text }` and calls `preventDefault`; no duplicate send from onInput.
- `ui/src/components/NativeTerminalPane.lifecycle.test.tsx`: visibility/lifecycle detach behavior unchanged.
- RED was captured by removing the new keydown handler branch; GREEN restored with it.
- `bun run --cwd ui build` passes.

### Rust detached-session guard — RED→GREEN, 3/3 passing
- `src-tauri/tests/native_terminal_input_boundary_contract.rs` (input boundary contracts).
- With guard removed: 2 tests fail (detached-session input accepted). Guard restored: 3/3 pass.

### Compositor contract — 7/7 passing
- `src-tauri/tests/native_terminal_composition_contract.rs`: child NSView frame strictly scoped to terminal viewport; chrome never covered.

### Build
- `cargo check` passes.

---

## Remaining Manual Gate (user-run, desktop automation forbidden)
1. `cargo tauri dev`
2. Active native terminal: type `printf 'native bridge ok\n'` — verify both input reaches shell and output renders.
3. Open Settings: confirm native pixels/cursor are invisible/non-covering and terminal command shortcuts have no effect while Settings is open.
4. Return to terminal: normal input works again immediately.
5. Verify New Tab button and pane split controls remain clickable over/beside the native surface.

## Acceptance Criteria
- [x] Printable keydown → `cmd_native_terminal_send_input` → daemon (automated)
- [x] Detached sessions cannot receive PTY writes (automated)
- [x] Compositor viewport scoping intact (automated)
- [ ] Manual E2E checklist above (user-performed)

---

# Addendum 2026-08-25: True Root Causes Found by Live Debugging

## A. Key input path itself was already fine
With `/tmp/ferryx-native-input-diag-{uid}.log` instrumentation, every user keystroke produced `[send_input] request`, no encode/guard failures, and `DaemonStreamMessage` pump frames with `is_active=true` + `render success`. Probes (fresh UDS connections to `daemon.sock`) proved the daemon core, PTY write path, ring buffer, and echo were healthy end-to-end for every session, including the affected GUI session.

## B. Actual killer: unanswered responses on the GUI's cached main connection

### Observed
- ~29 user keystrokes logged `[send_input] request` with **zero corresponding completions** for ~3.5 minutes.
- ~200 seconds later, the daemon suddenly delivered a **burst** of responses: 4 `write_terminal ok` within 5 ms, followed by a coherent echo/output/render storm. Ring-buffer history confirmed the backlog was flushed at once.
- During the stall, fresh probe connections to the SAME session got `writeOk` instantly and their `^C` marker echoed (740 bytes) — so the daemon was selectively deaf to the GUI's long-lived cached request connection while healthy to every other connection.

### Mechanism in the client (pre-fix)
`DaemonClient::send_request` holds a `tokio::Mutex` guard over the cached connection while awaiting the response. That await had **no timeout**, so one unanswered daemon response parked the guard indefinitely and queued EVERY subsequent request (write/resize/close) silently — matching the exact symptom (keystrokes accepted, never echoed, zero error).

### Fix (src-tauri/src/daemon/client.rs)
- Response read in `conn.request` is now wrapped in `tokio::time::timeout(15s)`; on timeout the cached connection is dropped and the next request opens a fresh connection — the hang becomes a visible, recoverable error instead of a silent process-wide stall.
- Request-sent / response-received / response-timeout events logged to the temporary diag file.

### Post-fix verification (live)
New session after rebuild: key input → write ok → daemon echo → pump `is_active=true` → `render success` — the full chain lights up on screen (user confirmed "something came out"). The remaining defect is only glyph rendering quality (below).

## C. Garbled glyphs: prototype 8×8 bitmap stub in renderer
`src-tauri/src/native_terminal/renderer/rasterizer.rs` rasterized with a hardcoded 8×8 bitmap (ASCII 32..=126 only), plus hand-drawn 東/京 and a crab emoji; every other codepoint rendered as a box. The terminal core itself is libghostty-vt (static FFI in `native_terminal/sys/ffi.rs`) and was always correct — only the text rasterizer was a placeholder. Cell metrics were also a hardcoded `10×20` constant unrelated to any real font, producing first-glyph clipping.

Remediation (delegated, in progress): real TTF/OTF rasterization via pure-Rust `fontdb` + `ab_glyph` with per-codepoint fallback (covers CJK/nerd icons), font family/size consumed from the existing Ghostty auto-import (`terminal/preferences.rs::load_terminal_preferences()`), and cell metrics derived from the loaded primary font. No new settings surfaces.
