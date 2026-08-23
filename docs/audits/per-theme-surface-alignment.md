# Per-Theme Surface Alignment

Date: 2026-08-22

## Issue

The Settings screenshot exposed a separate issue from the workspace-tree palette: its navigation rail used `bg-card` while the detail pane used `bg-background`. The `sidebar` token also had independent base colors in Charcoal, Dark, and Light, so left and right full-height surfaces did not match within a selected theme.

## Correction

- Settings navigation now uses `bg-background`, the same base surface as its detail pane.
- `sidebar` and `worktree-sidebar` base tokens are aligned to `background` in every supplied theme:
  - Charcoal: `#23262d`
  - Dark: `#0a0a0a`
  - Light: `#f6f7f9`
- Accent, border, foreground, and active-row tokens remain separate, preserving interaction hierarchy without creating a second base theme.

## Verification

- `npx vitest run src/workspaceThemeContract.test.ts src/components/SettingsDialog.test.tsx src/components/Sidebar.test.tsx src/components/Sidebar.activity.test.tsx` — **39 tests passed**.
- `npm run build` — TypeScript and Vite production build passed.
