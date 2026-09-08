# DAG pane ownership fix

Date: 2026-09-08

## Outcome

The DAG viewer badge now requires a running project run whose nonempty
`rootSessionId` exactly equals the pane's `providerSessionId`.
Project-path matching still limits the candidate runs; it no longer establishes
ownership. Agent presence, working state, and cached first-claimer ownership do
not authorize a badge.

This removes two false-positive paths: an unresolved run appearing on any agent
pane, and the first working pane retaining an unrelated run. Missing owner IDs
remain hidden until the actual provider session is known. Working/waiting
transitions do not hide an exactly owned running DAG.

## Verification

- Red: all 8 new ownership regression cases failed against the original code,
  including missing/blank/foreign root IDs, an unresolved idle agent, a stale
  claim after session replacement, and duplicate sibling badges.
- Green: 9 related Vitest files, 101 tests passed, exit code 0.
  Command: `bun run --cwd ui test src/components/dag src/components/TerminalPane.test.tsx src/state/dagRunOwnership.test.ts src/state/dagStore.test.ts src/lib/dagTypes.test.ts`.
- `bun run --cwd ui build`: passed, exit code 0, after correcting a widened
  string type in the updated TerminalPane test fixture.
- LSP diagnostics were unavailable because the local LSP daemon could not start.
  The build's TypeScript check passed.
- Headless Chrome rendered the real `DagPaneBadge`, `DagGraphView`, and DAG store
  in an isolated four-pane component harness. Counts were owner 1, unrelated
  agent 0, unresolved session 0, plain shell 0. Opening the badge produced one
  dialog and graph. Replacing the owner session removed both badge and dialog.
  Completing the run removed the badge.
- Screenshot: `docs/evidence/dag-owner-only.png`. It was captured automatically;
  this session's model could not visually inspect the image attachment.
- The temporary component harness and headless browser were removed/stopped.
  The user's desktop app and daemon were not manipulated.

## Desktop confirmation

The component browser check does not establish native desktop rendering.
In the debug app launched with `bun tauri dev`, place an owning DAG session and
an unrelated agent in sibling panes. Confirm the icon appears only in the owner,
opens its DAG viewer, and disappears when that DAG completes.

## Working tree

Only the DAG ownership fix, its tests, and verification evidence belong to this
change. Existing SSH-platform changes were preserved and excluded.
