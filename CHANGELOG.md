# Changelog

## Unreleased

### Changed

- Managed worktrees now use Ferryx names: new worktrees are created in `.ferryx-worktrees/<workspace>/<slug>` on branch `ferryx/<workspace>/<slug>`. The branch prefix can be changed per repository (lowercase letters, digits, `.`, `_`, `-`; up to 31 characters) and is stored in `worktree-settings.json` in the Ferryx data directory.
- Existing worktrees are not moved or renamed. A repository that already has `.orca-worktrees` keeps the `orca/` prefix and that directory for new worktrees until you set a prefix, so one repository does not end up with two layouts. Ferryx keeps recognizing `.orca-worktrees`, `orca/` branches and the old SSH `.orca-worktrees/wt-<slug>` layout.
- If your scripts match `orca/*` branches or `.orca-worktrees` paths, update them to also match `ferryx/*` and `.ferryx-worktrees` (or your configured prefix) for repositories without legacy worktrees.
- Remote (SSH) worktrees now use the same `<dir>/<workspace>/<slug>` layout as local ones instead of `.orca-worktrees/wt-<slug>`.
