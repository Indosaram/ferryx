# Settings Page Full Remediation — mass-ulw plan

Source of defects: `SETTINGS_REVIEW.md` (this repo, written 2026-08-28).
Tier: HEAVY. Orchestration: native `dag` tool, two waves + verification wave.

## Topology lock — 6 independent components, disjoint write scopes

| # | Component | Owns (WRITE) | Criteria |
|---|---|---|---|
| 1 | Runtime appearance wiring | `ui/src/lib/settingsRuntimeBridge.ts`, `ui/src/lib/appearanceSettings.ts`, `ui/src/main.tsx`, `ui/src/App.tsx`, `ui/src/appearanceThemeContract.test.ts`, `ui/src/lib/settingsRuntime.test.ts` | C1, C2 |
| 2 | Terminal section + settings lib | `ui/src/components/settings/TerminalSection.tsx`, `ui/src/lib/terminalSettings.ts`, `ui/src/components/settings/types.ts`, new `ui/src/components/settings/TerminalSection.test.tsx` | C3, C4, C5 |
| 3 | Dialog shell / a11y | `ui/src/components/SettingsDialog.tsx`, new `ui/src/components/SettingsDialog.escape.test.tsx` | C6 |
| 4 | Remote access | `ui/src/components/settings/RemoteAccessSection.tsx`, new `ui/src/components/settings/RemoteAccessSection.test.tsx` | C7, C8a |
| 5 | Notifications + Agents | `ui/src/components/settings/NotificationsSection.tsx`, `ui/src/components/settings/AgentsSection.tsx`, new co-located tests | C9, C8b |
| 6 | Design system pass | `ui/src/components/settings/primitives.tsx`, `AppearanceSection.tsx`, `GeneralSection.tsx`, `BrowserSection.tsx`, `ShortcutsSection.tsx`, `WorkspaceSection.tsx`, new `ui/src/components/settings/designTokens.test.ts` | C10 |

Write scopes are disjoint by construction: no file appears in two rows.
`SettingsDialog.tsx` is owned solely by component 3; component 6 must NOT touch it
(it only edits section files + primitives).

## Wave order

**Wave 1 (parallel, 5 nodes)** — components 1,2,3,4,5. No shared files, no ordering
between them. Each node owns its change AND its failing-first test.

**Wave 2 (1 node)** — component 6, depends on ALL of wave 1. Reason: the design pass
edits `primitives.tsx` plus five section files; several of those files are edited in
wave 1 (`RemoteAccessSection`, `NotificationsSection`, `AgentsSection` are wave-1
owned). Running it after wave 1 keeps write scopes serialized instead of conflicting.

**Wave 3 (verification, 1 node)** — depends on wave 2. Runs the real commands
(`bun run --cwd ui test`, `bun run --cwd ui build`) and reports captured output.

## Category routing

| Node | Category | Reason |
|---|---|---|
| runtime-appearance | `unspecified-high` | crosses lib + app root + two pinned test files; the removal must not break `appearanceThemeContract` |
| terminal-section | `unspecified-low` | 3 files, real judgment on the scrollback contract |
| dialog-shell | `quick` | single file + one new test, mechanical event-scoping fix |
| remote-access | `unspecified-low` | security-adjacent pairing behavior change + error surfacing |
| notifications-agents | `quick` | two files, disabled-state gating and error rendering, pattern-following |
| design-system | `visual-engineering` | frontend design-token and layout consolidation |
| verify-all | `quick` | runs two literal commands and reports output |

Only one node leaves the low rungs (`runtime-appearance`), and no node is `deep`.

## Per-node contract (summarized; full text lives in the dag definition)

Every node prompt carries TASK / DELIVERABLE / SCOPE / VERIFY / STOP WHEN, states the
exact file paths it may write, forbids touching any other file, and requires the
failing-first test captured RED before the production edit.

Shared invariants pasted into every node:
- Do NOT commit. The lead commits.
- Do NOT run `bun run --cwd ui test` for the whole suite; run only the targeted test
  file with `bun run --cwd ui test -- <path>` to avoid cross-node interference.
- Never delete, skip, `.only`, or `.skip` an existing test to go green.
- TypeScript is strict; no `any`, no `@ts-ignore`.

## Verification wave

`verify-all` runs, in the repo root:
1. `bun run --cwd ui test` — PASS = exit 0, zero failed tests, no new skips.
2. `bun run --cwd ui build` — PASS = exit 0.
It reports the tail of both, plus the failing file list if any.

The lead re-runs both itself; node claims are not evidence.

## Real-surface proof (lead-owned, C12)

After the run settles and the suite is green, the lead builds the UI and captures
rendered evidence of the settings page with an applied Appearance change, proving C1
end-to-end on a real surface rather than only in jsdom.
