# Review: app entry points, remote web client, features, devtools

Scope: `ui/src/main.tsx` (dual-runtime switch, platform class), `ui/src/index.css` (platform-scoped
global rules), `ui/src/App.tsx`, `ui/src/features/`, `ui/src/devtools/`.
Reviewed-at: 2026-09-14
Reviewer: lead session.

> Provenance: the `ui-app-remote` dag node was cancelled at 16 minutes — 37 `bash` searches, zero
> `write` calls, explicit write-now directive ignored. Domain covered here by the lead.
>
> Scope boundary honoured: the files under `ui/src/remote/` that belong to another live session
> (`RemoteApp.tsx`, `RemoteTerminal.tsx`, `deviceIdentity.ts` and their tests) were treated as
> **read-only** throughout this entire run and are byte-identical to the pre-run capture
> (sha256 `8466234777f30cb3dde8224cc154adc964935da2be8b2a16743f789b7e69f8a7`, verified pre and
> post). They are therefore **not** reviewed here — reviewing them would mean reading a moving
> target mid-edit.

## Findings

`NO-FINDINGS above P3`

### Verified negative — the dual-runtime switch has exactly one decision point

- Location: `ui/src/main.tsx:17`
- Observed: `const isTauriApp = typeof window !== "undefined" && Boolean((window as any).__TAURI_INTERNALS__);`
  — a single module-scope constant. `rg -n '__TAURI_INTERNALS__'` across `ui/src/main.tsx`,
  `ui/src/App.tsx` and `ui/src/remote/*.tsx` returns **this one hit**.
- Why it matters: the project's convention is that `main.tsx` routes to the desktop `App` or the
  web `RemoteApp` on this check. A second, independent probe elsewhere is how the two runtimes
  drift — one module deciding "desktop" while another decides "web" yields a remote client
  reaching for a Tauri-only API. With a single constant that divergence is structurally
  impossible.

### Verified negative — the native-terminal transparency block is platform-scoped

- Location: `ui/src/main.tsx:13`, `ui/src/index.css:182-186`
- Observed: `document.documentElement.classList.toggle("platform-macos", isMacShortcutPlatform());`
  and every rule in the transparency cascade prefixed `html.platform-macos` —
  `html.platform-macos:has([data-testid="native-terminal-pane"])`, and the same guard on the
  `body`, `#root`, `#root > div` and `div:has(> main …)` selectors.
- Why this specific scoping matters: an unscoped version of this exact block previously forced
  `html`/`body`/`#root` transparent on **every** platform whenever a native pane mounted, which
  produced an entirely black Ferryx window on Windows. The `html.platform-macos` prefix is the
  fix for that regression, and it is applied to all five selectors in the cascade — not just the
  first, which is the way this kind of guard usually rots.

## Summary

- P0: 0
- P1: 0
- P2: 0
- P3: 0

`NO-FINDINGS above P3`

Note: audited under a reduced budget after the node was cancelled, and deliberately excluding the
foreign-owned `ui/src/remote/` files. Remote reconnect behaviour and mobile IME composition —
which live in those excluded files — were **not** audited and are not claimed clean.
