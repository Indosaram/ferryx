# Settings Page Remediation — Verification Report (2026-08-28)

Fixing commit: `f1dbab4 fix(ui): make every settings control take real effect`
Review source: [`SETTINGS_REVIEW.md`](../SETTINGS_REVIEW.md)
Plan: [`docs/plans/settings-page-full-remediation-mass-ulw.md`](plans/settings-page-full-remediation-mass-ulw.md)
Rendered proof: [`docs/evidence/settings-panel-2026-08-28/`](evidence/settings-panel-2026-08-28/)

## Method

Every fix was locked by a test **verified to fail without it**. Each production file was
reverted in isolation (`git stash push` on the production file only, keeping the new test),
the targeted test was re-run, and the failure was confirmed to be a real assertion failure
naming the defect — not an import or setup error. A test that cannot fail for the regression
it names is not evidence.

## Criteria

| # | Criterion | Evidence | Status |
|---|---|---|---|
| C1 | Appearance applies live | `useApplyAppearanceSettings()` at `ui/src/App.tsx:166` (previously zero call sites) | PASS |
| C2 | Bridge stops patching React-owned DOM | `settingsRuntimeBridge.ts` 132 → 24 lines; no `textContent=`, no `querySelectorAll`, no `change` listener, no `"Reset to defaults"` dispatch | PASS |
| C3 | Scrollback is honest | Control removed from Terminal section; persisted field kept in `terminalSettings.ts` so no user data is dropped | PASS |
| C4 | Font family commits on blur/Enter | RED: `expected "spy" to be called +0 times, but got 5 times` | PASS |
| C5 | Empty numeric input never coerces to 0 | RED: `expected "spy" to not be called at all, but actually been called 1 times` | PASS |
| C6 | Escape scoped to inner controls | Bubble phase + `defaultPrevented` / input-value / `[data-state="open"]` guards; initial focus; `aria-current="page"` | PASS |
| C7 | No auto-minted pairing code | RED: `expected "spy" to not be called at all, but actually been called 1 times`; `generatePairing` now has exactly one call site, user-triggered | PASS |
| C8 | Silent catches surfaced | `actionError` (Remote Access) and `detectionError` (Agents) rendered via `Alert` with `role="alert"` | PASS |
| C9 | Notifications master-off gates dependents | 7 `disabled` bindings gated on `!settings.enabled`, including Send Test, Browse, Preview | PASS |
| C10 | Design token consistency | Zero `text-[9px]` / `text-[10px]` / bare `text-xs` in settings sources; `SettingsGroup` adopted by Appearance, Notifications, Browser, Workspace | PASS |
| C11 | Suite + build green | 118 files / 1027 tests and `tsc && vite build` pass **on the committed tree in isolation** | PASS |
| C12 | Real-surface rendered proof | 9/9 sections captured headlessly, see below | PASS |

RED evidence for C3 was `expected <input type="number"> to be null` — the orphaned Scrollback input.

## C12 — rendered proof

Captured against a dedicated Vite server on port 5199 driven by headless `agent-browser`.
No desktop application was manipulated.

`general.png`, `appearance.png`, `terminal.png`, `keyboard-shortcuts.png`, `workspace.png`,
`agents.png`, `browser.png`, `notifications.png`, `remote-access.png`

Three carry load-bearing proof:

- **`terminal.png`** — no Scrollback control remains; only Font family, Font size, and
  macOS Option as Alt, all of which genuinely reach the native runtime.
- **`remote-access.png`** — the gateway is **enabled** yet the QR panel shows an empty state
  with a "Generate QR Code" button instead of a live PIN. Pre-fix, opening this tab minted and
  displayed a control-permission pairing code.
- **`appearance.png`** — `SettingsGroup` header with right-aligned Reset, unified type scale,
  and a visible focus ring on "Back to app" confirming the C6 initial-focus fix.

Harness cleanup receipt: `PASS: Port 5199 is dead (lsof -iTCP:5199 -sTCP:LISTEN is empty).`

## Adversarial review round (commit `e87029b`)

An independent hostile review of `f1dbab4` found two MAJOR defects that the first round of
tests could not catch, plus one MINOR. All three are fixed.

| Finding | Defect | Fix |
|---|---|---|
| MAJOR | Font size still committed on every keystroke. Typing `14` from an empty field committed `1`, the clamp rewrote it to `10` mid-edit, so two-digit sizes below the minimum were impossible to type. | Commit on blur/Enter, matching the font-family contract. RED: `expected '10' to be '1'` |
| MAJOR | Escape was dead across most of the dialog. The guard skipped `role="combobox"`, but Radix keeps that role on a *closed* `SelectTrigger`; the non-empty-input rule also trapped Escape in any prefilled field. | Key on the open state; scope the text-input exception to search fields, and mark the shortcuts search `type="search"`. RED: `expected "spy" to be called at least once` |
| MINOR | A whitespace-only font family stayed in the field on blur. | Revert to the persisted value. RED: `expected '   ' to be 'MesloLGS NF'` |

**Why the first tests missed these.** They mocked the commit callback with a bare `vi.fn()`.
With no parent to clamp the value and feed it back as a prop, the very interaction loop the fix
was meant to protect never executed. The replacement tests drive a parent that clamps exactly
like `normalizeTerminalSettings` — a mock must preserve the behavior being asserted, or the
test cannot fail for the regression it names.

One prior assertion actively encoded the bug: the escape test asserted that a **closed**
combobox swallows Escape. It now asserts both directions — swallowed while open, closing once
shut.

Post-review state: full ui suite **120 files / 1032 tests** and the production build pass on the
committed tree in isolation.

## Deliberate non-changes

- `scrollback` remains in `terminalSettings.ts` storage and normalization. Only the UI control
  was removed, so previously persisted values survive.
- `main.tsx` keeps `installSettingsRuntimeBridge()` for the pre-React first paint, which is also
  pinned by `ui/src/appearanceThemeContract.test.ts`.
- `ui/vitest.config.ts` gained an `@` → `./src` alias. This is an enabling change, not scope
  creep: the new co-located tests import shadcn atoms via `@/components/ui/*`, which previously
  only Vite (not Vitest) resolved.

## Known, out of scope

The Paired Devices row in Remote Access renders a blank device name with only a "Last active"
timestamp. Cosmetic, pre-existing, and unrelated to the reviewed defect list — recorded here
rather than fixed under this change.
