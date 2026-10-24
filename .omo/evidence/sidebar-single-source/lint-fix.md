# Lint fix: sidebar single source

Branch feat/sss-lint (from 9c6a7143).

## Changes
1. `ui/src/remote/useRemoteSidebarMachines.ts`: the `never` default branch of the `switch (input.kind)` used to `return`, which put the trailing `useMemo` after a conditional return (react-hooks/rules-of-hooks). It now throws an assertNever-style error, so the hook is always called. Runtime behavior is unchanged because the branch can't be reached once the types check.
2. `ui/src/components/WorktreeList.tsx`: removed `EmptyWorktreesHint`, which no production code called, along with the `SidebarWorktreesEmpty`/`SIDEBAR_EMPTY_TEXT` imports that only it used. `WorktreeList.test.tsx` now renders `<SidebarWorktreesEmpty text={SIDEBAR_EMPTY_TEXT.noWorktrees} .../>` directly in that one test.

## Verification (ui/)
- eslint (`bun --bun eslint src`, the `lint` script): base 5fea3008 = 11 errors, head = 11 errors (70 warnings). The remaining errors are in FilePreviewPane.tsx (5), TerminalSplitView.tsx (3), bridgeBroker.test.ts (2) and guestBridgeWindowClose.test.ts (1). useRemoteSidebarMachines.ts no longer reports an error.
- `bun --bun tsc --noEmit`: exit 0
- `bun run test`: exit 0, 480 files / 8346 tests passed
- `bun run build`: exit 0
- `bun run build:mobile`: exit 0
- Note: `typecheck:test` (not required) shows 45 errors both with and without this change, so they are pre-existing and none are in the touched files.
