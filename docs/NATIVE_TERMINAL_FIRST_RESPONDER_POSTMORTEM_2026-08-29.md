# Postmortem: Native Terminal Input Death — Per-Frame First-Responder Steal

**Date:** 2026-08-28 → 2026-08-29 KST
**Status:** fixed (guard in `restore_webview_first_responder`), live-verified in the rebuilt release app
**Full incident timeline:** `docs/LIVE_TYPING_DEAD_CONCURRENT_AGENT_HMR_2026-08-28.md`

## Symptom (all the same defect)
- "터미널에서 입력이 안 된다" — typing produced nothing at all.
- "한글자만 쳐짐" — exactly one character per mouse click.
- After app relaunch or long occlusion: dead until the first click, then one char per click again.

## Root cause
`SurfaceHost::render_snapshot` (`src-tauri/src/native_terminal/surface_host.rs`) calls
`restore_first_responder(window)` after **every** `frame.present()`.
`platform/macos.rs::restore_webview_first_responder` then unconditionally called
`ns_window.makeFirstResponder(WKWebView NSView)` (the outer container view).

The fatal loop:
1. Click → the webview's inner key-handling view becomes first responder → key 1 reaches
   the DOM and is sent to the PTY.
2. The PTY echo triggers a repaint → present → `makeFirstResponder` on the container →
   **first responder is stolen from the inner view** on the very frame that echoed key 1.
3. Every later keypress never reaches the DOM. Once the container holds first responder,
   further restore calls are AppKit no-ops, so the dead state persists until the next click.

## Evidence (weakest to strongest)
1. Trace forensics: `/tmp/ferryx-switch-debug.jsonl` contains **no multi-key typing burst
   in its entire history** — every successful capture (19:22, 19:59, 21:08) is 1–3 isolated
   keys: the one-per-click signature was present all along.
2. Release-build reproduction (no Vite, no HMR, no agent churn — clean environment):
   typing produced **zero** DOM `input.capture`/`input.sent` and zero ring growth;
   clicking then typing produced exactly one key (`ㅁ`, activeElement `BODY/`, ring 8→11),
   then silence again.
3. Post-fix live verification: after rebuilding with the guard, sustained typing flows —
   daemon ring grew `facf4a41` 11→70 and `fce6267e` 7→23 in one session.

## Fix
`restore_webview_first_responder` now reads `ns_window.firstResponder()` and returns early
when it is an `NSView` equal to or a descendant of the webview NSView. The restore still
fires when focus is genuinely outside the webview (or nil), so its recovery role is kept.
Verified: `cargo check`, 353 `cargo test --lib`, UI `tsc`, live typing round-trip.

## False leads that cost hours (do not repeat)
1. **The offscreen AppKit probe "proved" the restore harmless** and got a guard explicitly
   documented as RETIRED. The probe's window was never key, so it never exercised the real
   key path — its "no-op / load-bearing" conclusion was invalid. When a probe contradicts a
   live reproduction, the live reproduction wins.
2. **The evening HMR-storm correlation** (a concurrent agent session's git ops ↔ trace
   freezes) explained webview reload churn in the dev app, but not input death — the same
   symptom reproduced in the release build with none of that present.
3. **Daemon/PTY suspicion** recurred throughout; the daemon was healthy every time
   (`scripts/verify-terminal-typing.mjs` answers in ms). Suspect the daemon only after the
   3-signal check points there.

## Prevention rules
1. **Never assert focus/first-responder behavior from offscreen or non-key-window probes.**
   Only live keystrokes against the real window are evidence.
2. **Diagnose input with the 3-signal check in one pass:** DOM document-capture event →
   `input.sent` → daemon ring `endSequence`. The first missing signal names the dead layer:
   OS/AppKit → pane fallback → IPC/daemon.
3. **Audit per-frame native mutations** (first responder, layer reorders, alpha changes)
   for compounding with the terminal echo loop: output → repaint → side effect → input dies.
4. **Absence of input events is evidence only if a human actually typed** in that window —
   check run duration and human presence before inferring a defect from an empty trace.
5. **Retired-fix notes are decisions too** — when retiring a fix, record what evidence would
   refute the retirement, so a future session can re-open it quickly.
