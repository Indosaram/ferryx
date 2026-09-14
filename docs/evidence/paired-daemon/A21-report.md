# A21 - worktree actions, recovery and agent boundaries

## Outcome

AC05 and AC06 are NOT signed off. This packet implements guarded desktop boundaries and paired worktree action wiring, with focused UI/unit evidence; real remote process execution and complete desktop recovery remain unproven.

All edits were confined to `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`. The canonical plan was read as explicitly requested; canonical source, real HOME, running daemons and PTYs were not modified. No Rust files were edited, no cargo/release/deployment command was run, and no commit or destructive git command was used.

## Implemented

- `tauri.ts`: ordinary desktop removal of a `daemon:` reference does not invoke legacy unregister. Explicit remote unregister remains the existing typed adapter operation; it is not conflated with desktop removal.
- `ProjectDialogs.tsx`: paired Add Worktree skips local branch enumeration and SSH creation, using the captured host adapter and remote workspace namespace.
- `pairedWorktreeActions.ts`: typed create/safe delete; deletion uses the owning daemon's status revision and identity. No client-supplied path or force-delete mode. Ambiguous outcomes retain the original request ID within the dialog action instance; subsequent attempts query the existing operation journal and do not resend. Unknown/pending/expired journal results do not become inferred success or deletion.
- `WorktreeDeleteDialog.tsx`: optional owning `project` prop routes paired preview/delete through that service, disables destructive escalation for paired targets, and fails closed when paired owner metadata is absent.
- `TerminalPane.tsx`: while native paired terminal support remains unavailable, paired panes show an unavailable status without mounting local terminal/DAG tooling or offering SSH/local recovery. No unavailable inventory is interpreted as process exit.
- `agentReconnect.ts` and `shellReplacement.ts`: reject paired sessions before local spawning/provider recovery.
- `workspaceStore.ts`: paired sessions do not trigger fallback desktop provider discovery. Authoritative provider events remain the intended source.
- `remoteProject.ts`: image upload rejects non-SSH ownership before invoking SSH clipboard APIs.

## Verification

- `A21-RED.log`: original boundary suite failed 5/5 BEFORE boundary implementation. After implementation, deliberately changed the paired surface assertion from truthy to null; captured the resulting 1/5 failure; restored the assertion. This is assertion-sensitivity evidence, not fault-injection proof for every worktree scenario.
- `A21-GREEN.log`: Node v22.22.3; 10 related test files, 114 tests passing in one run; `bun run --cwd ui build` exit 0. Build retains the existing chunk-size advisory.
- New deterministic tests cover captured remote namespace, cross-workspace results, safe revision use, ambiguous create/delete reconciliation with exactly one mutation, unsupported destructive deletion, removal/image/recovery boundaries. No sleeps, polling, temp repositories, sockets or new fixture files were added.
- LSP diagnostics on all 11 changed/new TypeScript source/test files: no diagnostics.
- Cleanup assertion verified the intentionally broken test assertion was restored. No temporary git repo/worktree/socket/fixture was created, so there was no such resource to remove. Existing shared fixtures were read only.
- No Rust changes: conditional cargo check requirement does not apply. No claim is made about A16's concurrently changing Rust build.
- The known unrelated push client failures were not run or changed.

## Remaining integration and acceptance gaps

1. **A20 handoff:** App owns the WorktreeDeleteDialog caller. Pass the captured owning `RegisteredProject` as `project` (not merely active-host selection). A21 was forbidden to edit App. Until supplied, paired deletion is intentionally unavailable rather than falling back locally.
2. Ambiguous request identity is retained only in the action instance, NOT persisted across dialog dismissal/renderer restart. The daemon journal is durable, but restart recovery UI for those outstanding requests is not implemented here. Do not claim full durable UI mutation recovery. A future continuation must store a reconciliation reference, not introduce a second mutation journal or auto-resend.
3. Remote unregister and explicit close remain existing typed adapter operations, not newly wired desktop confirmations. This packet does not prove remove/re-add preserves live remote sessions through App's snapshot/layout paths.
4. Paired native terminal proxy was parent-reported unavailable at task start. TerminalPane intentionally stays disabled regardless of A16's concurrently developing backend. Connecting its future availability gate and host-scoped native session target requires integration with A16/A20; do not simply route it through local/SSH recovery.
5. AC05 owning-daemon dirty/locked/live-session/root-jail/git subprocess checks were not independently exercised in this packet. Client requests use `WorktreeIdentity`, never override `.orca-worktrees/wt-<slug>` / `orca/<ws-id>/<slug>` paths, but this is not proof of daemon enforcement.
6. AC06 remote agent/manual text launch and authoritative cross-host attribution are unproven. The fallback guard prevents local probing; it does not implement optional remote provider APIs. No real remote terminal, image paste, drag/drop or native menu was exercised.
7. Worktree action tests were added after that new service was written; their green coverage must not be described as failing-first worktree evidence. The mandatory captured failing-first evidence covers the five existing boundary defects specifically.

## Human desktop QA required (after proxy/integration prerequisites)

- On disposable paired Linux project, create a managed worktree and verify exact jail path/branch, dirty/locked/live-session/unmerged protections, revision conflicts and safe explicit deletion.
- Start an installed coding agent and a shell in separate panes. Verify remote process/CWD, text input, manual agent launch and host/pane provider attribution without probing desktop paths.
- Remove/re-add only the desktop reference while both processes remain alive; verify original sessions attach without shell recreation. Separately confirm explicit close/unregister semantics.
- Lose relay response after committed create/delete, reconcile by the original request ID, repeat with dialog dismissal/desktop restart once persistence exists, and prove no duplicate mutation or inferred success from partial/unavailable inventories.
- Verify offline, changed pairing, expired session and conflict recovery against actual owning-daemon responses; verify there is never Local/SSH fallback.
- Verify unavailable remote file/image/DAG affordances, clipboard image and file drag/drop do not transfer desktop paths; inspect real native menus as well as rendered controls.
