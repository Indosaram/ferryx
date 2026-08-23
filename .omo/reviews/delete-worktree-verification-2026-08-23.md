# Delete worktree dialog and deletion verification

Date: 2026-08-23

## Reported behavior

The supplied native Ferryx screenshot showed three concerns:

1. A light delete sheet over an apparently dark application surface.
2. An absolute worktree path wrapped awkwardly inside a sentence.
3. `? ahead · ? behind` while the upstream row said `None`.

## Diagnosis and change

- The sheet was not hard-coded white: `bg-card`, `text-foreground`, and destructive colors correctly resolved from the active light theme. The surrounding mismatch came from hard-coded dark `body` utilities in `ui/index.html`.
- The body now uses semantic `bg-background text-foreground` tokens, letting it follow the same active theme as the dialog.
- `WorktreeDeleteDialog` now presents the full selected path in its own bordered muted, `break-all` block rather than embedding it in the review sentence.
- A branch without an upstream now renders a deliberate `No upstream` divergence state. If an upstream exists but one count is absent, the UI uses an em dash rather than a misleading `?`.
- The destructive surface remains the existing semantic `bg-card` token; no forced dark sheet or raw color token was introduced.

## Actual deletion contract

The UI passes `{ workspaceId, worktree: { wsId, slug }, deleteBranch: true }` to the typed Tauri command. Native IPC resolves that identity only through the registered workspace's managed worktree registry; it does not accept arbitrary paths.

The strengthened `tauri_mock_worktree_commands_use_identity_contract` temporary Git fixture now asserts, after a successful safe IPC deletion:

- the created managed worktree path no longer exists;
- `git branch --list` no longer contains `orca/ws-ipc/task-ipc`;
- the remaining native worktree list has only the primary worktree.

Existing isolated `worktree_safety` coverage additionally proves that safe deletion rejects dirty, unmerged, and writer-held worktrees; destructive deletion removes a clean unmerged managed branch only after the explicit safe-delete `UNMERGED_BRANCH` result.

### Checkout-drift safety repair

Review found an additional native safety flaw: before this repair, identity resolution found the expected managed *slot path* but did not verify that its active checkout branch still matched that identity. A two-slot branch checkout swap could therefore make a stale row delete the wrong slot.

`WorkspaceRegistry::resolve_worktree` now requires the resolved worktree's current branch to exactly equal the requested `orca/<wsId>/<slug>` branch before it returns a deletion target. A mismatch returns the existing `WorktreeIdentityNotFound` path and prevents `delete_worktree_and_branch` from running.

The temporary Git fixture `swapped_checked_out_branches_cannot_delete_a_stale_identity_slot` creates slots A and B, detaches both, checks branch B out into A and branch A into B, then requests deletion for stale identity A. It proves the request returns `WORKTREE_NOT_FOUND` and asserts that both slot directories and both managed branches survive. The test was RED before branch identity validation and GREEN afterward.

No existing user worktree was deleted or modified during this verification. `git worktree list --porcelain` continued to show the pre-existing `rorca-qa/e2e-qa` worktree.

## Verification run

Passed before unrelated concurrent Rust compilation edits landed:

```text
cd ui && bun x vitest run src/components/WorktreeDeleteDialog.test.tsx src/workspaceThemeContract.test.ts
# 12 passed

cd ui && bun run build
# exit 0

cargo test --manifest-path src-tauri/Cargo.toml --lib tauri_mock_worktree_commands_use_identity_contract
# exit 0

cargo test --manifest-path src-tauri/Cargo.toml --test worktree_safety
# 8 passed

cargo test --manifest-path src-tauri/Cargo.toml --test ipc_hardening_contract swapped_checked_out_branches_cannot_delete_a_stale_identity_slot
# 1 passed
```

The focused dialog test proves the full path receives `break-all`, numeric divergence still renders when upstream exists, no-upstream divergence never renders `? ahead · ? behind`, and safe deletion remains scoped to the selected registered workspace identity.

## Current shared-tree build status

The final native deletion targets passed after the concurrent Rust files settled:

```text
cargo test --manifest-path src-tauri/Cargo.toml --test ipc_hardening_contract
cargo test --manifest-path src-tauri/Cargo.toml --test worktree_safety
cargo test --manifest-path src-tauri/Cargo.toml --lib tauri_mock_worktree_commands_use_identity_contract
```

Final revalidation after concurrent SettingsDialog changes settled:

```text
cargo test --manifest-path src-tauri/Cargo.toml --test ipc_hardening_contract
# 7 passed

cd ui && bun x vitest run src/components/WorktreeDeleteDialog.test.tsx src/index-html.test.ts
# 8 passed

cd ui && bun run build
# exit 0
```

The native test runs still emit three pre-existing dead-code warnings for `WriterLeaseGuard` and `acquire_writer_lease`; none originates in this delete-worktree change.

The full `cargo test --manifest-path src-tauri/Cargo.toml` run subsequently reached 136 passing tests but failed the unrelated in-progress daemon test `daemon::server::tests::test_pump_stream_compact_framing_and_exit`. Its assertion expects a compact stream frame containing `"sessionId":"test-session-123"`; the failure is in concurrently modified daemon protocol/server framing code, not the worktree resolver, IPC delete command, or their fixtures. The targeted deletion suites above remain clean.

## Native visual QA boundary

The user prohibits desktop/UI automation, so no native Tauri window was driven. The supplied screenshot was inspected directly; CSS/theme paths and dialog DOM behavior are covered by focused tests. To visually confirm the repaired surface, restart Ferryx and open Delete worktree for an upstream-less worktree: the background and panel should share the active theme, the selected path should be in its own bounded block, and divergence should say `No upstream` rather than show question marks.
