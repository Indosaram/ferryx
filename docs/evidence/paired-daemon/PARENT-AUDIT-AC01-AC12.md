# Parent completion audit: AC01-AC12

Every row records only evidence the parent produced or re-ran itself. Child
summaries and DAG "completed" states are not accepted as evidence. Status is
deliberately conservative: anything not demonstrated end to end is NOT MET.

| AC | Status | Evidence the parent holds |
| --- | --- | --- |
| AC01 Add Project offers Local/SSH/Paired Daemon, browse without SSH creds | PARTIAL | Backend path proven: `Operation::Directories` with capability gating and `PAIRED_HOST_CAPABILITY_UNAVAILABLE` fail-closed (`paired_host/client_tests.rs`), directory op exercised inside the forced-relay test. The Add Project **UI surface** is unverified - needs desktop QA. |
| AC02 Canonical identity, persists, survives desktop and daemon restart | MET (backend) | `native_operations_force_relay_equal_paths_restart` run by the parent: exit 0, `A14 exercise result=Ok(Ok(()))`. It tears down both gateways and the native IPC, rebinds the relay, reconstructs from private catalogs and asserts the mapping is byte-identical. Also `machine_catalog_persistence` target exists; `HEAD` commit "persist project catalog across isolated restarts". |
| AC03 Local/SSH/paired coexist; identical names and IDs stay separate; machine label + status | PARTIAL | Identity separation proven at both layers: Rust `projects_tests.rs::metadata_and_unavailable_identity_are_lossless` (same raw payload under two hosts yields different workspace IDs), and the forced-relay test asserts equal `repo_root` with different desktop `workspace_id`. UI store/adapter covered by 110 passing tests. **Sidebar rendering, labels and status chips unverified** - desktop QA. |
| AC04 Multiple tabs, split panes, focus, search, copy/paste, shortcuts, native menus | NOT MET | No automated coverage possible; `NativeTerminalPane` behavior requires the desktop app. OS automation is prohibited here. Needs user QA under `bun tauri dev`. |
| AC05 Remote worktrees create/delete with branch, dirty, lock, live-session, root-boundary checks | LIKELY MET (backend) | `--test machine_worktrees` passed in the parent's aggregate run. `workspace_api/worktrees.rs` + `worktree_authority_tests.rs` + `worktree_wire_proof_tests.rs` exist and are hash-pinned by `A08-publication-refresh-source.sha256`. Not re-run per-guard by the parent. |
| AC06 Agents run on the remote machine; activity and provider session attributed to host/pane; no local path probing | NOT VERIFIED | `session_metadata_*` modules and `machine_session_metadata` target exist, but the parent has not run them to completion, and agent attribution is a desktop-visible behavior. |
| AC07 Project/machine switch, settings, renderer close, relay loss do not kill sessions; reconnect attaches or reports expired | PARTIAL | Controller-isolation half is proven: A10 five saturated-socket scenarios pass, `pty_input_cancellation` and `machine_terminal_stream` pass, generation fencing cancels blocked work (`generation_cancels_blocked_http_without_release`). Desktop-side switching/close is unverified. |
| AC08 Forced-relay run covering browse, register, worktree ops, terminal create/input/output/resize/interrupt/close, reconciliation | PARTIAL | Forced relay itself is proven (parent-run, direct paths unused, two hosts over one relay origin) for browse + register + restart + reconciliation. **Worktree ops, resize, interrupt and close are not inside that single forced-relay test**; they are covered by separate targets. A single end-to-end forced-relay pass over the full list is still owed. |
| AC09 Legacy mobile mirror keeps active-desktop policy and redacted payloads; cannot escalate via URL/JSON/UA | NOT VERIFIED | `machine_worktree_legacy_bounds` and legacy-owner evidence exist but the parent has not re-run them. |
| AC10 Revocation, stale epoch, cross-host ticket, replay, stale controller, malformed path, retries fail safely; no bearer in URL/log | LARGELY MET | Parent-read tests: `wrong_authenticated_machine_cannot_be_adopted`, `capability_absence_fails_closed`, `generation_cancels_blocked_http_without_release`, `real_http_non_json_redirect_and_body_limit` (no relay body leak), `real_machine_catalog_replay_checks_digest_and_preserves_metadata` (replay equality + `REQUEST_CONFLICT`). Forced-relay test asserts no device token appears in the wire payload. UI rejects cross-host results and malformed remote IDs (110 tests). |
| AC11 Offline projects/layouts stay visible; incomplete inventory is not authoritative-empty; no Local/SSH fallback | PARTIAL | `metadata_and_unavailable_identity_are_lossless` keeps `completeness: partial` and `unavailableWorkspaceIds` host-qualified; UI asserts unavailable IDs survive. Offline **visibility** in the running app is unverified. |
| AC12 Compatibility and rollback across relay, remote daemon, local daemon, desktop, legacy mobile, without killing unrelated daemons | NOT MET | No rollback demonstration exists. Cross-version work was never run. |

## Open defects the parent found (both in repair now)

1. **Vacuous handover green.** `daemon/handover.rs:399` `std::process::exit(0)` kills
   the test binary mid-test, so `machine_owner_handover` reports success without
   printing a harness summary or its own cleanup lines, and every assertion after
   `commit_handover` is never evaluated. Repair `st_01a0996d`; mutation-RED plus
   non-vacuous GREEN demanded.
2. **Native IPC drops mutation ambiguity.** `paired_host/client.rs:113-117`
   `From<ServiceError> for ClientError` calls `local()`, forcing
   `request_id: None, ambiguous: false`, while `daemon/client.rs:406-449` converts
   every transport failure into `ServiceError::unavailable`. A mutation that may
   already have been applied remotely is reported as a clean failure with no
   request identity, defeating the durable journal. Repair `st_01a0996f`.
   Independently this is the A14 aggregate review's own blocking FAIL.

## Verification state of the A14 aggregate itself

`A14-aggregate-results.json` records `--lib paired_host::` **exit 101** and the UI
command **exit 1**, with `ui-build`, `headless` and `git diff --check` at 0. The
review verdict is **FAIL**. The DAG reporting "4 completed" therefore does not
mean acceptance. The UI exit 1 is a runner artifact: with PATH pinned to Node
v22.22.3 the same three files give 110 passing tests (parent-run).
`shasum -c A14-aggregate-artifacts.sha256` passes for 11 of 12 files;
`A14-aggregate-review.md` mismatches because it was updated after the manifest.

## Security clause coverage (parent-enumerated, not taken from reports)

The objective requires "scope/revocation/epoch/controller/replay security". The
parent enumerated the actual test functions in `src-tauri/src` and
`src-tauri/tests` rather than trusting any child's summary. 77 distinct
security-relevant functions exist. The ones that directly discharge each axis:

- **Replay**: `gateway_issues_single_use_socket_tickets_and_rejects_a_replayed_one`,
  `real_machine_catalog_replay_checks_digest_and_preserves_metadata`,
  `dropped_http_reply_replays_original_process_and_controller_fences_close`
- **Controller fencing**: `rejects_competing_controller_until_expiry_or_revoke`,
  `a10_controller_generation_disconnect_and_exact_reservation_boundary`,
  `acquire_machine_controller`
- **Revocation**: `input_is_cancelled_when_grant_is_revoked`,
  `a10_pending_machine_input_is_dropped_on_disconnect_and_revoke`,
  `re_pair_forget_and_revoke_cancel_only_captured_host_across_restart`,
  `prune_revoked_and_idle_devices`,
  `legacy_revoked_tombstones_are_pruned_when_the_store_is_loaded`,
  `cmd_remote_device_revoke`
- **Epoch**: `epoch_rejects_noncanonical_or_out_of_range_wire_values`,
  `malformed_epochs_and_oversized_json_are_rejected`,
  `identity_is_host_qualified_and_preserves_large_epoch`,
  `predecessor_epoch_and_additive_capabilities_are_preserved`,
  `global_waiting_and_stale_epoch_and_completion_provenance`
- **Scope / admission**: `bounded_git_rejects_large_output_and_expired_or_revoked_admission`

This is genuine coverage of the security axes, and it is the strongest-evidenced
part of the plan. It does NOT by itself discharge AC08, which additionally needs
the terminal path over a forced relay - still blocked on the proxy.

## CORRECTION: the gap is unimplemented packets, not missing QA

An earlier draft of this table framed AC01/AC03/AC04/AC06/AC07/AC11/AC12 as "needs
manual QA". That was wrong and is corrected here. The parent read the plan's packet
list: **A15-A24 are not implemented at all.**

```
A15 Local paired-daemon proxy runtime                     (plan line 639)
A16 Native IPC, input, lifecycle, and metadata integration      (651)
A17 Version-3 layout migration and restart recovery             (663)
A18 Paired Daemon Add Project and generic picker                (675)
A19 Mixed-host Sidebar and worktree caches                      (687)
A20 Restore the desktop shell and shortcut/menu behavior        (699)
A21 Worktree/removal actions, recovery UI, and agent boundaries (711)
A22 Host management UX, operator guidance, and feature gate     (723)
A23 Full integration, security, and resource regression         (735)
A24 Staged rollout and rollback rehearsal                       (747)
```

The decisive proof that these are missing rather than merely untested:

- `paired_host::Operation` (in `src-tauri/src/paired_host/client.rs`) has exactly
  these variants: `Capabilities`, `Directories`, `Projects`, `RegisterProject`,
  `UnregisterProject`, `Worktrees`, `WorktreeStatus`, `CreateWorktree`,
  `DeleteWorktree`, `Sessions`, `Session`, `CreateSession`, `CloseSession`,
  `Operation`. There is **no terminal attach, input, output, resize, interrupt or
  event-stream operation**.
- `pairedDaemonProxyV1` is asserted **false** as a deliberate contract in two Rust
  tests (`paired_host/process_tests.rs:121`,
  `paired_host/native_operation_tests.rs:162`).

So AC08's "terminal creation, input, output, resize, interrupt, close" cannot be
driven over the paired path today - that transport is packet A15/A16 and does not
exist. A dedicated child (`st_01a09973`) was dispatched to write the full AC08
forced-relay test, inspected the call chain, found the same wall, and correctly
stopped WITHOUT editing production files or fabricating a green. It changed zero
files.

AC -> owning unimplemented packet:
AC01 -> A18 - AC03 -> A19 - AC04 -> A20 - AC05/AC06 -> A21 -
AC08/AC10 -> A23 - AC11 -> A17/A19 - AC12 -> A24.

## Blocked on the user

AC01 (Add Project UI), AC03 (Sidebar rows/labels/status), AC04 (tabs, splits,
focus, search, copy/paste, shortcuts, menus), AC06 (agent attribution), AC07
(desktop switching/close), AC11 (offline visibility) and AC12 (rollback) require
manual QA in the debug desktop app via `bun tauri dev`. OS automation of the
user's desktop is prohibited by standing instruction, so these cannot be closed
by this session alone.
