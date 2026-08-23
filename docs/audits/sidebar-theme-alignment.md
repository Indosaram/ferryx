# Sidebar Theme Alignment

Date: 2026-08-22

## Root cause

`Sidebar` uses the `worktree-sidebar` semantic token family. Those values had diverged from the shared application palette:

- Application background: `#23262d`
- Sidebar background before this change: `#2a2a2a`
- Sidebar selected-row accent before this change: `#353535`

That dedicated, brighter sidebar palette made the left pane look like a different theme.

## Correction

The `worktree-sidebar` tokens in `ui/src/index.css` now use the shared application palette:

- Background: `#23262d` / `35 38 45`
- Accent: `#404040` / `64 64 64`

The component keeps semantic sidebar tokens, so its tree states and borders retain their intent without a separate base color theme.

## Verification

- `npx vitest run src/workspaceThemeContract.test.ts src/components/Sidebar.test.tsx src/components/Sidebar.activity.test.tsx` — **22 tests passed**.
- `npm run build` — TypeScript and Vite production build passed.
