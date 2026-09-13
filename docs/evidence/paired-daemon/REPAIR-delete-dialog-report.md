# Delete dialog owner wiring repair

## Delivered

App now resolves `deleteOwnerId` with the existing `resolveWorktreeOwnerId(deleteTarget, projects, activeProject.workspaceId)` expression, finds the owning registered project, and passes it to WorktreeDeleteDialog. The onDeleted fallback selection reuses those same resolved values. No new ownership algorithm was introduced.

The dialog's default-service guard now uses the existing combined `paired` classification rather than checking only the selected service workspace ID. A paired dialog without a paired owner reports `PAIRED_OWNER_REQUIRED`, leaves deletion disabled, and cannot silently select Local/SSH services even when a non-paired row workspace ID masks the paired dialog workspace ID. Keeping the optional prop preserves existing local and injected-service callers; the runtime fail-closed contract is smaller and also protects malformed runtime inputs, which a required TypeScript prop alone would not.

## Reproduction: important correction to the reported mechanism

The supplied worktree already contained a guard in createDefaultServices that rejects daemon workspace IDs. Thus the missing App project prop was real, and prevented createPairedWorktreeActions from being called, but an ordinary paired row failed closed rather than invoking native deletion. It would be inaccurate to claim native deletion was reproduced for an ordinary paired row.

Before production edits, the new App regression rendered the real App and real delete dialog, with a Sidebar fixture triggering App's onDeleteWorktree callback. The paired owner was inactive while a local project was active. The assertion that createPairedWorktreeActions received the paired project failed with zero calls. The dialog factory is mocked only at the action boundary, leaving App ownership and dialog service selection real.

A second failing-first test rendered the dialog without project, with a paired dialog workspace and a non-paired row workspace. This genuinely called native preview with workspaceId `local-row`; the no-native-call assertion failed. This is the remaining silent fallback hole in the original guard. Missing row IDs and daemon row IDs already failed closed and remained passing controls.

Both original failures and their exit codes are preserved verbatim in REPAIR-delete-dialog-RED.log. Tests were not weakened after RED.

## Verification

- Node v22.22.3, with the requested PATH and a temporary HOME/TMPDIR inside the isolated worktree.
- All four changed TypeScript/TSX files: language-server diagnostics reported no diagnostics.
- One green invocation: 5 test files, 25 tests passed (App.pairedDaemon, WorktreeDeleteDialog.owner, existing WorktreeDeleteDialog, pairedWorktreeActions, worktreeOwnership).
- The App regression exercises preview, clicks the actual delete dialog button, asserts paired safe deletion, observes dialog closure, and asserts no native preview/safe/destructive deletion calls.
- New tests use React act to flush the synchronous fixture actions and resolved/rejected promises; no sleeps or polling were introduced.
- `bun run --cwd ui build`: exit 0. Vite reported its chunk-size warning; no warning suppression was added.
- `git diff --check`: exit 0.
- Commands, results, and build warning are preserved in REPAIR-delete-dialog-GREEN.log.

## Limits and open gaps

No live daemon, network mutation, desktop automation, or PTY was exercised. App coverage uses a Sidebar trigger fixture and mocked paired action services; this proves the actual App-to-dialog wiring, not native transport integration. The related action-service tests also passed but are not live-daemon evidence. No full UI-suite claim is made; the parent-reported pre-existing ferryx/push/client.test.ts failures were neither run nor modified. Existing dialog tests retain their pre-existing Testing Library wait patterns; no changes were made to that file.

The A21 reconciliation gap remains: pending mutation request references live in the createPairedWorktreeActions instance, not durable storage. They do not survive a renderer restart (or disposal of that action instance). This repair does not change that behavior.

The service injection seam is trusted and unchanged. Registered paired projects are assumed to carry the existing valid target and remoteWorkspaceId contract; malformed projects are not repaired here.

## Scope and cleanup

Work was limited to the isolated herdr-resume-01a097f8 worktree. Production edits: App.tsx owner wiring and WorktreeDeleteDialog.tsx guard. Test edits: App.pairedDaemon.test.tsx and new WorktreeDeleteDialog.owner.test.tsx. Evidence: this report plus RED/GREEN logs. Existing unrelated source changes were retained. No Rust edits, commits, daemon/PTY operations, native release builds, deployment, or desktop automation occurred. Temporary HOME/TMPDIR and generated ui/dist were removed after verification. Pre-existing node_modules symlink was retained; test result caching was disabled.
