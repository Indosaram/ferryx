# Daemon layout rollout and rollback

The first release containing `daemonLayoutV1` must keep writing the legacy
window-scoped session layout as well as sending daemon layout intents. The
currently checked-in app version is `2026.1101.1`; no release containing this
capability has been published as part of todo 10. The release operator must
record the actual first shipping version here before removing the dual write.

- First shipping version: not yet released.
- Removal point: the release immediately following that first shipping version,
  not the first capability-bearing release itself.
- Until then, `persistSessionStrict` continues serializing the current UI layout
  into the legacy session blob so a rolled-back GUI can restore it.
- Daemons without `daemonLayoutV1` use the existing session/snapshot/HMR path.

## Integration blockers discovered in todo 9's protocol

The UI bridge deliberately does not change `daemon/server.rs` or `protocol.rs`.
These daemon-side gaps need a serialized lane repair before todo 10 is complete:

1. `LayoutApply` accepts only a window, revision and intent. It has no workspace
   or worktree scope and no intent selecting a parked layout. `LayoutGet` returns
   `parkedLayouts` but does not identify the root workspace. A UI workspace switch
   cannot safely select or mutate that workspace's layout. The UI currently
   hydrates explicitly keyed parked layouts and declines root writes for them;
   new workspaces and worktree switching require a daemon scope-selection contract.
2. There is no cross-window tab/subtree transfer intent. `planTabMove` and the
   existing backend window/session transfer cannot atomically move the daemon
   layout or preserve its node identities in the target window.
3. File content lacks preview ownership/backend metadata, browser content lacks
   browser identity, and changes content lacks checkout identity. Existing local
   handles can be retained across revisions and HMR, but a fresh GUI cannot
   reconstruct those surfaces from daemon content alone. This needs either an
   enriched content contract or an explicit frontend reattachment registry.
4. `SetPaneRatio` does not implement the frontend's seam/collinear resizing
   semantics. `CloseTab` does not represent replacement tabs; optimistic close
   replacement must be sent as a following add. These require parity tests.

No build, test, mutation run, QA scenario, remote gate or app launch was run for
this branch, per the implement-first instruction. The integration verification
must run the full UI suite, build and test typecheck and the macOS isolated GUI
scenario from todo 10. This document records blockers, not a passing gate.
