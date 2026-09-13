# A19 - Sidebar and worktree cache

## Changes

- `ui/src/lib/projectGrouping.ts`: namespace host-local Git-directory grouping by Local / SSH host / paired host; prefer a genuinely local group primary.
- `ui/src/lib/projectIdentity.ts`: paired synthesized roots carry desktop workspace ownership.
- `ui/src/lib/worktreeOwnership.ts`: exclude paired projects from local path/branch inference; preserve captured paired fallback ownership and reject unknown explicit paired owners.
- `ui/src/state/pairedProjectWorktrees.ts` (new): use captured paired adapter context, negotiate capabilities, inspect project availability and unavailable workspace IDs before listing, map remote worktree workspace IDs to desktop IDs, reject cross-workspace results and stale generations. Missing or unavailable metadata never becomes an authoritative empty list; no Local/SSH fallback.
- `ui/src/state/inactiveProjectWorktrees.ts`: paired metadata loading also covers the active paired project; bypass local registration, retain cached rows on failure, refresh on owner-qualified worktree events and host availability/generation changes. Preserve SSH/local cached rows on load errors.
- `ui/src/state/workspaceRuntime.ts`: paired projects never enter local terminal/worktree synchronization while terminal proxy support is absent; filter change events by workspace owner.
- `ui/src/components/Sidebar.tsx`: paired machine labels and native/auth/feature/transport statuses, explicit cached row ownership, synthesized paired roots, no path-only local terminal attention on paired project headers.
- `ui/src/components/WorktreeList.tsx`: paired remote badges/statuses and remote-only reveal/delete behavior; exclude local path-only agent/status/unread data from paired rows.
- New tests: `Sidebar.paired.test.tsx`, `inactiveProjectWorktrees.paired.test.tsx`.

No forbidden files, daemon, PTYs, desktop automation, commits, or release builds were used. The explicitly requested canonical plan was read only. Git status could not run because the pre-existing Ghostty submodule path is a symlink; no Git repair was attempted.

## Evidence

`A19-RED.log` captures both new tests failing before implementation: four hosts collapse to two groups, and a paired path reaches local registration.

`A19-GREEN.log` retains intermediate failures and their fixes (legacy initial local cache shape; readonly tuple inference), followed by a complete passing run under Node v22.22.3:

- 10 test files / 51 tests pass, including both new tests, Sidebar remote/identity/DnD regressions, inactive caches, worktree ownership, project workspace scope and switch-back.
- `bun run --cwd ui build` exits 0. Vite reports its chunk-size warning; it was not suppressed.
- Individual changed source/test LSP diagnostics were clean before final validation; the last recheck of inactiveProjectWorktrees timed out, then the real TypeScript build passed.

Tests prove identical Local/SSH/two paired paths, common directories and branch names do not collapse by host-local identity; paired root selection preserves desktop ownership; partial/unavailable and failed paired refreshes retain prior rows and never call local registration/listing. Existing tests exercise SSH grouping and workspace switch isolation. React surfaces are exercised with Testing Library, not the real desktop.

## Acceptance limits / human QA

Do not mark AC03 or AC11 fully complete from this packet alone. SSH rows retain their existing machine labels but do not gain a connection-status source in this packet. Raw terminal session ID isolation and retained layout/restart recovery are outside these Sidebar tests (A17 owns layouts; paired terminal proxy is unavailable). This packet does not prove end-to-end native paired event delivery or live remote worktree mutations. Incomplete inventories preserve rows, but there is no separate per-refresh-error stale indicator beyond the machine connection state.

Human QA: expand Local, SSH and two paired machines with colliding paths/branches; select each root and child, verify only the intended owner activates, and inspect machine labels/statuses. Change inactive machine metadata and verify native events refresh the correct Sidebar rows. Disconnect one host and verify rows and A17-owned layouts remain; reconnect and verify refresh. Confirm visually that long machine labels/statuses remain usable. No desktop actions were automated.

Assumption: paired metadata is useful without terminal attachment; its worktree cache must remain independent of local terminal synchronization. Matching Git remotes may intentionally share a visual project group, but member rows retain host-qualified workspace identities.
