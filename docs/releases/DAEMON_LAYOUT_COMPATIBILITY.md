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

## Additive contract extensions

Todo 10 extends the daemon contract with explicit authorization to edit the
layout portions of `daemon/server.rs` and `protocol.rs`:

- `SCOPED` wraps an intent with the JSON workspace/checkout tuple. Layouts carry
  their scope through persistence and handover. `LayoutGetScoped` selects a scope;
  the original `LayoutGet` still returns the complete window revision.
- `TRANSFER_TAB` moves an entire subtree to another window, reminting destination
  node ids without changing backend session or surface identities. It remains
  inside `LayoutApply`, so the existing admin gate applies. Window adoption is
  published only after the daemon accepts the transfer.
- Optional flattened content fields preserve browser identity/profile, file
  preview ownership and checkout identity. Legacy content without these fields
  continues to deserialize. Existing peers ignore the added fields.
- Ratio intents accept optional seam geometry and isolated mode. Without seam
  geometry the daemon resolves collinear splits. Close intents accept an optional
  replacement tab. Reducer golden fixtures cover both extensions.

Transfers persist the destination before removing the source. A crash between
those two file writes can leave a duplicate reference, but cannot lose the
subtree. This is not a cross-file atomic transaction; integration QA must include
interruption/recovery. Unknown new intent/request variants are not supported by
an earlier daemon that already advertises `daemonLayoutV1`; mixed intermediate
development revisions must be upgraded together.

No build, test, mutation run, QA scenario, remote gate or app launch was run for
this branch, per the implement-first instruction. The integration verification
must run the full UI suite, build and test typecheck and the macOS isolated GUI
scenario from todo 10. This document is not a passing gate.
