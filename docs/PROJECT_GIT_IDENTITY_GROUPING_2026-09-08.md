# Project grouping by Git identity

## Outcome

Ferryx groups registered checkouts by hosted Git repository identity or by a
shared Git common directory on the same execution host. Folder names no longer
establish project identity. Workspace IDs, repository roots, SSH targets and
terminal ownership are preserved.

The implementation follows Orca's identity principle without migrating Ferryx
to a new Project/Setup database schema.

## Rules

- Hosted remote selection uses `upstream`, then `origin`, then remaining remote
  names in lexical order. Local filesystem remotes are not cross-machine keys.
- GitHub SSH/HTTPS URLs, `.git` suffixes and repository name casing normalize to
  the same key. Other hosting services retain case-sensitive repository paths.
  Distinct HTTP ports remain distinct.
- Registration returns optional `gitCommonDir`. Linked worktrees sharing this
  directory are grouped only on the same execution host. Windows directory
  spelling is normalized separately from case-sensitive POSIX paths.
- Same-named folders with different remotes, or with no shared Git evidence,
  remain separate.
- Group merging checks all members, so a checkout can connect a local linked
  worktree group to a remote clone group regardless of registration order.
- Local secondary checkouts remain visible in the sidebar. Click and keyboard
  navigation retain the owning workspace ID.
- Git metadata survives localStorage loading and session snapshot saves.
  Background local registration enriches previously saved local project rows.
  Remote metadata is refreshed on the existing remote registration/selection
  path, not through unsolicited background SSH connections.
- Remote registration's existing daemon project-store schema is unchanged;
  common-directory metadata is returned to the frontend, avoiding a required
  daemon replacement or protocol migration.

## Verification

Regression tests failed before the relevant changes:

- Grouping and remote DTO tests: 14 failures before implementation.
- Real temporary Git repository registration tests: 3 failures before implementation.
- Session identity preservation: failed before implementation.
- Inactive-project metadata publication: failed before implementation.
- Rendering a secondary grouped local checkout: failed before implementation.

Passing checks:

- Six frontend suites, 97 tests:
  `projectGrouping`, `remoteProject`, `sessionPersistence`,
  `inactiveProjectWorktrees`, `Sidebar`, and `Sidebar.projectIdentity`.
- Three backend registration tests using real temporary Git repositories and
  linked worktrees.
- 59 backend `ssh::` unit tests.
- `bun run --cwd ui build` (TypeScript and Vite).
- `cargo check --manifest-path src-tauri/Cargo.toml`.
- `git diff --check`.
- Actual Windows SSH probe and registration:
  `maho-win`, `C:\Users\sook\code\omo-gateway`.
- Actual Linux SSH probe and registration:
  `omarchy`, `/home/indo/projects/PirateTalk`.

The live integration test is `src-tauri/tests/ssh_project_identity_live.rs`.
It reads an existing remote checkout and writes registration records only to a
local `tempfile` directory. Neither the user's real project list nor the remote
checkout is modified. Both live runs passed and returned the expected remote,
common Git directory, execution path, host ID and host-scoped workspace ID.

Example invocation:

```sh
FERRYX_IDENTITY_HOST=omarchy \
FERRYX_IDENTITY_PATH=/home/indo/projects/PirateTalk \
cargo test --manifest-path src-tauri/Cargo.toml \
  --test ssh_project_identity_live -- --ignored --nocapture
```

## Remaining verification limitations

The full `App.test.tsx` suite finishes with 89 passing and 3 failing tests:

- `checks for a signed update when the native app starts`
- `handles native remote_selection_requested and activates requested worktree context`
- `activates the exact pane after a queued cross-project selection names a leaf entry`

The same three failures occurred before and after the final integration changes.
No failing tests were skipped or weakened. A pristine baseline was not executed,
so this report does not certify these failures as pre-existing.

The LSP daemon was unreachable. TypeScript build and Rust compiler checks
provided compiler validation, but LSP diagnostics could not be obtained.

The user's desktop was not manipulated. Native desktop appearance and terminal
focus after selection still require manual confirmation:

1. Run the debug app using exactly `bun tauri dev`.
2. Register differently named checkouts of the same hosted repository on two
   machines; confirm a single project section contains both execution targets.
3. Register a linked local worktree; confirm it remains visible in that section.
4. Select each row and confirm the correct machine, checkout and terminal open.
5. Register a same-named unrelated repository; confirm it stays separate.
6. Restart the debug app and confirm the grouping is restored.

No release build, daemon restart, real project-list mutation or Git commit was
performed. The requested candidate projects have not been added to the user's
live list by this code-change task.
