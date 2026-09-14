# Review: app chrome components — tab bar, sidebar, palette, shortcuts

Scope: `ui/src/components/TabBar.tsx`, `ui/src/components/Sidebar.tsx`,
`ui/src/components/CommandPalette.tsx`, `ui/src/components/ShortcutHints.tsx`, and the shortcut
matcher they all depend on (`ui/src/lib/shortcuts.ts`), plus storage-key handling
(`ui/src/lib/storageKeys.ts`).
Reviewed-at: 2026-09-14
Reviewer: lead session.

> Provenance: the `ui-comp-chrome` dag node failed with `"Model returned an empty response
> twice"` (transient — the gateway re-probed 200 in 3.0s, zero stream-budget hits, only 30 tool
> calls). Its domain is covered here by the lead. Every citation below was opened and read.

## Findings

`NO-FINDINGS above P3`

This lane was audited against the three failure modes the project's own AGENTS.md names for app
chrome — layout-dependent shortcut matching, hardcoded legacy storage keys, and raw
`localStorage` access in components. The first two are clean; the third is a documented style
deviation with no behavioral consequence, recorded below rather than inflated.

### Verified negative — shortcut matching uses physical `code`, not layout-dependent `key`

- Location: `ui/src/lib/shortcuts.ts:492-526`
- Observed: every branch tests `event.code` **first** and only falls back to `event.key`:
  `` ecode === `Key${key.toUpperCase()}` || ekey.toLowerCase() === key.toLowerCase() `` for
  letters (`:522-523`), `` ecode === `Digit${key}` || ecode === `Numpad${key}` `` for digits
  (`:520`), and explicit `BracketRight`/`BracketLeft`/`Equal`/`Minus`/`Comma` handling for
  punctuation (`:505-518`).
- Why this matters here: a Korean 2-set layout reports `event.key` as a Hangul jamo (physical V
  yields `"ㅍ"`), so a matcher comparing only `key` silently drops every shortcut while the user
  is in Hangul mode — a defect this project has hit before. Leading with `code` is the correct
  construction.
- The IME guard at `:469` is the necessary companion and is present: it rejects
  `event.isComposing`, legacy `keyCode === 229`, and `key === "Process"`/`"Dead"`, so a
  composition keystroke whose physical `code` happens to match a binding is not stolen from text
  conversion. Both halves of the contract are implemented.

### Verified negative — no hardcoded legacy `orca.*` / `rorca.*` keys in app chrome

- Query run: `rg -n "orca\.|rorca\." ui/src/components/{TabBar,Sidebar,CommandPalette,ShortcutHints}.tsx`
  → no matches.
- All three sidebar persistence sites read through the migration helper:
  `getMigratedItem(SIDEBAR_WORKTREE_ORDER_STORAGE_KEY)` (`Sidebar.tsx:982`),
  `…COLLAPSED_PROJECTS…` (`:1025`), `…WIDTH…` (`:1045`), with the canonical `ferryx.*` constants
  imported from `ui/src/lib/storageKeys.ts` (`Sidebar.tsx:38`).
- Related: the migration helper those reads depend on had a real P1 — it copied legacy values
  forward without consuming them, so every `reset*()` was resurrected on reload. **Fixed** this
  session (`storageKeys.ts:42-56`); see the consolidated report, fix 15. That fix is what makes
  the sidebar's reads safe going forward, not the call sites themselves.

### [P3] Sidebar writes `localStorage` directly instead of through a domain helper

- Location: `ui/src/components/Sidebar.tsx:1000`, `:1037`, `:1055`
- Observed: `window.localStorage.setItem(SIDEBAR_WORKTREE_ORDER_STORAGE_KEY, …)` and the two
  analogous calls, while the matching reads go through `getMigratedItem`.
- Why it is only P3: `ui/AGENTS.md` lists "Raw Storage Access — never invoke
  `localStorage.getItem`/`setItem` directly in components; use domain helper modules in
  `src/lib/`" as an anti-pattern, so this is a genuine convention violation. But it is
  **write-side only**, it uses the canonical `ferryx.*` constants, and the read path already
  normalises legacy values — so there is no user-visible defect today. The cost is future drift:
  a write-side concern added later (quota handling, cross-tab eventing, a `setMigratedItem`
  counterpart) has to be retrofitted at three call sites instead of one.
- Minimal fix: add a `setMigratedItem`/domain-helper counterpart in `ui/src/lib/` and route the
  three writes through it. Not done in this pass — it is a refactor with no behavioral defect
  behind it, and this run's budget belongs to defects.

## Summary

- P0: 0
- P1: 0 (the one P1 reachable from this lane's dependencies — legacy-key resurrection — was
  found via `ui-comp-subdirs` and is fixed; see consolidated report fix 15)
- P2: 0
- P3: 1 (recorded, not fixed, with reason)

Note: this lane was audited under a reduced budget after its node failed. Drag-and-drop state
machines for tab transplants and sidebar reordering were **not** exhaustively audited and are
not claimed clean.
