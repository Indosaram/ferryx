# Parent verification: A14 native integration (post-rejection recovery)

The parent rejected the first A14 completion for three concrete defects. This
records which of them are now closed, verified by the parent's own reads and
runs rather than the child's summary.

## Defect 1 - missing typed native command: CLOSED

- `src/ipc/paired_host.rs` defines `#[tauri::command] paired_host_operation`,
  delegating to the daemon client. The module header keeps the boundary explicit:
  renderer-facing sanitized inventory, credentials daemon-owned.
- Registered in the invoke handler at `src/lib.rs:1062`.
- `src/daemon/client.rs:445` implements the request, mapping
  `DaemonResponse::PairedHostOperationOk/Error`; the label table at
  `client.rs:181` names it `pairedHostOperation`.
- `src/daemon/server.rs:1904` dispatches to
  `MachineClient::new().execute(&self.paired_hosts, request)`, so host identity,
  generation fencing, capability gating and cross-host rejection all run in the
  shared client rather than being re-implemented at the IPC edge.

## Defect 2 - missing two-host relay integration proof: CLOSED

`src/paired_host/native_operation_tests.rs` (mounted at `server.rs:863-865`
under `#[cfg(all(test, unix))]`) contains
`native_operations_force_relay_equal_paths_restart`, which the parent read in
full. It:

1. starts a real `relay_server::relay_router` and two gateway coordinators
   (`a`, `b`) sharing one relay origin (forced relay, no direct path);
2. pairs a native daemon client to both and asserts the host IDs are not aliased;
3. registers the **same** repo path on both hosts and asserts equal `repo_root`
   with **different** desktop `workspace_id`s;
4. reads the private `native/paired-hosts.v1.json` and asserts neither device
   token nor the literal `deviceToken` appears in the wire payload;
5. tears down both gateways and the native IPC, rebinds the original relay
   address, reconstructs all three from their own private catalogs, re-issues
   scoped Control/Machine grants through an event-driven readiness barrier, and
   asserts the project mapping is byte-identical after restart.

Parent's own run:
`cargo test --locked --no-default-features --lib paired_host_operation_tests -- --nocapture --test-threads=1`
→ **exit 0**, `test result: ok. 1 passed`, `A14 exercise result=Ok(Ok(()))`,
`A14 cleanup owned tasks joined; relay/gateway listeners refused`,
`private_root=/private/tmp/a12-metadata.HmslIGLO removal_exit=0`.
Evidence: `A12-session-metadata-parent-a14-relay.{log,exit,cleanup}`.

## Defect 3 - ownership validation in native response handling: CLOSED

Enforced in `MachineClient::execute` and covered by
`src/paired_host/client_tests.rs`, which the parent read:
`wrong_authenticated_machine_cannot_be_adopted` (PAIRED_HOST_WRONG_MACHINE),
`capability_absence_fails_closed` (PAIRED_HOST_CAPABILITY_UNAVAILABLE),
`generation_cancels_blocked_http_without_release` (PAIRED_HOST_STALE_GENERATION),
`ambiguous_journal_never_repeats_mutation` and
`pending_mutation_response_retains_request_and_ambiguity` (journal identity and
non-repetition), `real_http_non_json_redirect_and_body_limit` (redirect and body
limits with no relay body leakage), and
`real_machine_catalog_replay_checks_digest_and_preserves_metadata` (replay
equality plus `REQUEST_CONFLICT` on a reused request id with a different path).

## Independent UI verification

`bun run --cwd ui test src/lib/pairedDaemonProject.test.ts
src/lib/pairedDaemonContracts.test.ts src/state/remoteHostStore.test.ts` with
PATH pinned to Node v22.22.3 → **110 passed / 3 files**, exit 0
(`A12-session-metadata-parent-a14-ui.{log,exit}`). The earlier failure was the
Node 25 runner, not the code; no test or storage code was changed to accommodate it.

## Verdict

A14 is accepted as **scoped complete**. Not full-plan acceptance: desktop
tabs/splits lifecycle and Local/SSH/paired coexistence still need manual QA
under `bun tauri dev`, and the final code review is outstanding.
