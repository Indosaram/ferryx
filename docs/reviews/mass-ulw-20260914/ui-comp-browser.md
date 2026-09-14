# Review: embedded browser React components

Scope: `ui/src/components/BrowserPane.tsx`, `ui/src/components/BrowserToolbar.tsx`,
`ui/src/lib/browserSettings.ts` (`normalizeBrowserAddress`, `isHttpUrl`), and the guest bridge
they depend on (`src-tauri/src/browser/guest.rs`).
Reviewed-at: 2026-09-14
Reviewer: lead session.

> Provenance: the `ui-comp-browser` dag node was cancelled after 39 minutes — it ran 52 `bash`
> searches with **zero** `write` calls and ignored two explicit write-now directives. Its domain
> is covered here by the lead instead. Every citation below was opened and read.

## Findings

### [P0] Guest bridge accepted synthetic events — FIXED (cross-referenced)

The most serious defect reachable from a browser pane is not in the React layer but in the
injected guest bridge, and it is recorded in full in `rust-notify-browser.md` and the
consolidated report. Summarised here because it is the browser pane's primary attack surface:
`src-tauri/src/browser/guest.rs` installed capture-phase `click`/`keydown`/`drop` listeners with
no provenance check, so any page — including one in a hidden background tab — could
`document.dispatchEvent(new KeyboardEvent(...))` and drive privileged app chrome (spawn terminal
tabs, close the active surface and its session, open the palette). **Fixed** by capturing
`Object.getOwnPropertyDescriptor(Event.prototype,'isTrusted').get` at document start and gating
all three listeners. GREEN: `/tmp/ulw-massreview/guest-green.log`, 65 passed / 0 failed.

### Verified negative — omnibox cannot navigate to a `javascript:` or `data:` URL

- Location: `ui/src/lib/browserSettings.ts:281-293` (`normalizeBrowserAddress`), reached from
  `ui/src/components/BrowserToolbar.tsx:168-174` (`navigateFromAddress`) and `:259-261`
  (`navigateFromHistory`)
- Observed: every navigation the toolbar performs routes through `normalizeBrowserAddress`. Its
  allow-list shape is what makes it safe: it returns input unchanged **only** for `about:blank`
  or an explicit `http://`/`https://` prefix (`:287`), prepends `http://` for
  `localhost`/`127.0.0.1` (`:288`), prepends `https://` for a bare host-looking token (`:291`),
  and otherwise falls through to `searchUrlFor(...)` (`:292`), which percent-encodes the input
  into a search URL.
- Why this is sound: a hostile scheme such as `javascript:alert(1)` matches none of the
  passthrough branches — it contains no `.`-host and no `http(s)://` prefix — so it is treated as
  a **search query**, not a URL. The dangerous default (passthrough for unknown schemes) is
  absent; the default here is to search. Both toolbar entry points and the history-click path
  share this one function, so there is no bypass route.

### Verified negative — native webview masking during drag is wired correctly

- Location: `ui/src/components/BrowserPane.tsx:87-88`, `:204`, `:250`
- Observed: `const surfaceVisible = useNativeTerminalVisibility();` and
  `const maskAwareVisible = visible && surfaceVisible;`. The visibility effect gates on
  `if (!maskAwareVisible || liveTab.loadError)` (`:204`) and lists `maskAwareVisible` in its
  dependency array (`:250`), so a drag that flips the shared native-surface visibility signal
  re-runs the effect and hides the child webview.
- Why it matters: the project's components AGENTS.md names "never leave native child webviews
  unmasked during active tab/pane drag gestures" as an anti-pattern, because a native webview
  swallows DOM pointer events and breaks the drop. The masking is not open-coded per call site —
  it is a single derived value consumed by the effect, which is why it cannot drift out of sync.

## Summary

- P0: 1 (fixed — full detail in `rust-notify-browser.md`)
- P1: 0
- P2: 0
- P3: 0

Note: this lane was reviewed under a reduced budget after its node was cancelled. The omnibox
scheme handling and the drag-masking contract were audited directly; deeper areas the original
lane prompt named — private-history isolation and duplicate-tab state races — were **not**
exhaustively audited here and are not claimed clean.
