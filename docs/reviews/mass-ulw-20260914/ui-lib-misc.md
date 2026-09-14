# UI lib misc review — storage keys, shortcuts, persistence queue, timeouts

Scope reviewed: `ui/src/lib/storageKeys.ts`, `shortcuts.ts`, `persistenceQueue.ts`, `withTimeout.ts`,
`worktreeOwnership.ts`, `projectIdentity.ts`, `linkRouting.ts`, `browserHistory.ts`, `uuid.ts`
(plus `shortcutDiagnostics.ts` and `switchDebug.ts` read to confirm call sites).

Note on working-tree state: `ui/src/lib/storageKeys.ts` is being edited by another live session while this
review ran. The legacy-key consumption bug (migration copied the legacy value forward but left the legacy
entry in place, so `reset*()` helpers resurrected pre-upgrade settings) is **already fixed in the working
tree** at `storageKeys.ts:53` (`storage.removeItem(legacyKey);`) and is therefore not reported below.

---

### [P1] Shortcut matching by physical `event.code` mis-routes chords on non-QWERTY layouts (Cmd+, closes the active tab on Dvorak)

- Location: `ui/src/lib/shortcuts.ts:523` (with `ui/src/lib/shortcuts.ts:517`, loop at `ui/src/lib/shortcuts.ts:417` and early `return` at `ui/src/lib/shortcuts.ts:447`)
- Observed: letter bindings match on the physical key position as well as the typed character —
  `return ecode === \`Key${key.toUpperCase()}\` || ekey.toLowerCase() === key.toLowerCase();`
  — and punctuation does the same: `return ecode === "Comma" || ekey === "," || ekey === "<";`.
  `useShortcuts` iterates `for (const shortcut of SHORTCUTS)` in declaration order and `return`s on the
  first match, and `tab.close` (`binding: { key: "w", mod: true }`, `shortcuts.ts:84-87`) is declared long
  before `settings.toggle` (`binding: { key: ",", mod: true }`, `shortcuts.ts:334-337`).
- Why it is wrong: on a Dvorak layout the character `,` is produced by the physical QWERTY-W key, so
  pressing Cmd+`,` emits `{ key: ",", code: "KeyW" }`. The `tab.close` binding matches first via
  `ecode === "KeyW"`, the loop `preventDefault()`s and returns, and the user's active terminal/browser tab
  is **closed instead of Settings opening**. The running shell's tab disappears on a keystroke that should
  have been inert for tabs. The same cross-talk exists for every letter/punctuation binding on any layout
  where the character and the QWERTY position disagree (AZERTY, Dvorak, Colemak, QWERTZ), and it is
  asymmetric: the physically-earlier declaration always wins.
- Minimal fix: make the match two-pass instead of single-pass. In `useShortcuts`, run the `SHORTCUTS` loop
  once with a character-only matcher (`event.key` comparisons only) and, only if nothing matched, run it
  again allowing the `event.code` positional fallback. That keeps the code fallback for layouts where
  `event.key` is unusable while guaranteeing a character the user actually typed outranks a physical
  position claimed by another binding.

---

### [P2] `installCryptoPolyfill` makes `safeRandomUUID` call itself — every ID recurses ~4.5k frames and swallows a `RangeError`

- Location: `ui/src/lib/uuid.ts:7-9` and `ui/src/lib/uuid.ts:75`
- Observed: the polyfill installs the same function it guards on:
  `Object.defineProperty(globalThis.crypto, "randomUUID", { value: safeRandomUUID, ... })` (`uuid.ts:75`),
  while `safeRandomUUID` opens with
  `if (typeof globalThis.crypto?.randomUUID === "function") { try { return globalThis.crypto.randomUUID(); } catch {} }`
  (`uuid.ts:7-9`). `installCryptoPolyfill()` runs at module load (`uuid.ts:87`).
- Why it is wrong: in exactly the environment the polyfill exists for — a webview with `crypto.getRandomValues`
  but no `crypto.randomUUID` (older WKWebView, non-secure origins) — the installed `randomUUID` is
  `safeRandomUUID` itself, so every call re-enters itself until the JS stack is exhausted. The `catch {}` at
  `uuid.ts:10-11` silently absorbs the `RangeError` and the function then falls through to the manual byte
  path, so the bug is invisible in tests (`uuid.test.ts:104-108` exercises precisely this path and passes)
  while costing a full stack blow-up per ID. I reproduced the logic verbatim under Node: one call reached a
  recursion depth of 4579 before unwinding, and 200 IDs cost ~20 ms of pure stack thrash. It also means any
  code calling `crypto.randomUUID()` directly (see next finding) inherits the recursion, and a genuine
  `RangeError` from a deep caller stack is indistinguishable from the swallowed one.
- Minimal fix: bind the polyfill to a non-recursive generator. Capture the native implementation once at
  module load (`const nativeRandomUUID = globalThis.crypto?.randomUUID?.bind(globalThis.crypto)`) and have
  `safeRandomUUID` call `nativeRandomUUID` instead of re-reading `globalThis.crypto.randomUUID`; install the
  pure-fallback generator (the byte path) as the polyfill value.

---

### [P3] `getOrCreateInstallationId` bypasses `safeRandomUUID` and can mint a non-UUID installation id

- Location: `ui/src/lib/storageKeys.ts:68-70`
- Observed: `const next = typeof crypto !== "undefined" && typeof crypto.randomUUID === "function" ? crypto.randomUUID() : \`${Date.now()}-${Math.random().toString(36).slice(2)}\`;`
  `storageKeys.ts` imports nothing from `./uuid`, so the module's polyfill side effect is not guaranteed to
  have run when this executes.
- Why it is wrong: two defects in one line. If `uuid.ts` has been loaded, `crypto.randomUUID` is the recursive
  polyfill from the previous finding; if it has not, the persisted installation id is
  `1757800000000-k3j1x`, not a UUID — a shape no consumer of a "remote installation id" expects, and it is
  written once and never re-derived, so the odd value is sticky for that install.
- Minimal fix: `import { safeRandomUUID } from "./uuid";` and use `safeRandomUUID()` for `next`, deleting the
  inline `Date.now()`/`Math.random()` branch.

---

### [P3] `writeBrowserHistory` persists a truncated list but broadcasts the untruncated one

- Location: `ui/src/lib/browserHistory.ts:57` and `ui/src/lib/browserHistory.ts:62`
- Observed: the write truncates —
  `storage?.setItem(BROWSER_HISTORY_STORAGE_KEY, JSON.stringify(entries.slice(0, BROWSER_HISTORY_LIMIT)));`
  — while the event that drives the UI does not:
  `window.dispatchEvent(new CustomEvent<BrowserHistoryEntry[]>(BROWSER_HISTORY_EVENT, { detail: entries }));`
- Why it is wrong: the two views of history can disagree. Today the only caller
  (`recordBrowserHistory`, `browserHistory.ts:84`) pre-slices, so nothing is user-visible; the divergence is
  latent and will surface the first time a caller passes a longer array — the toolbar history dropdown
  (`components/BrowserToolbar.tsx:89`) would list entries that vanish on the next reload.
- Minimal fix: compute the truncated array once and use it for both the `setItem` and the event detail
  (`const persisted = entries.slice(0, BROWSER_HISTORY_LIMIT);`).

---

## Checked and clean (no finding)

- `withTimeout.ts:17-19`: the timer is cleared in `.finally()` on the raced promise, which settles as soon as
  the operation settles, so the timeout handle never leaks and the rejection can never fire after a win.
- `persistenceQueue.ts:7-9`: `chain.current` is always reassigned to the `.catch(() => undefined)` wrapper, so
  a failed persist cannot poison the chain and later writes are still serialized behind it — no dropped write.
- `storageKeys.ts:21-31`: the keys absent from `LEGACY_STORAGE_KEY_MAP` (`ferryx.sidebar.worktreeOrder`,
  `ferryx.settings.appearance`, `ferryx.settings.browser`, `ferryx.browser.history`, `ferryx.ssh.configPath`,
  `ferryx.update.dismissedVersion`) never shipped under an `orca.`/`rorca.` name — `git log -S` over `ui/`
  returns zero commits for each legacy spelling — so the missing entries are correct, not an omission.

---

Summary: P0: 0, P1: 1, P2: 1, P3: 2
