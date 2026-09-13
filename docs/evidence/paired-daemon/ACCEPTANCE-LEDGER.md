# Herdr prompt-to-artifact acceptance ledger

## Authority and use
Approved source: /Users/indo/code/project/orca-lite/.omo/plans/ferryx-herdr-cloud-multi-host-plan.md. This ledger maps every numbered packet, release criterion, named command, required manual scenario and compatibility combination to evidence slots. Full architectural and security requirements in sections 3-8 remain binding and are not reduced to this packet summary. Empty or uncertain evidence means NOT ACCEPTED. Packet completion, test count, a manifest, or a worker PASS is not whole-goal completion.

Current overall verdict: NOT COMPLETE. A01-A05 have inherited scoped acceptance; Wave2 implementation and A13-A24/native/platform/rollback gates remain. No inherited acceptance is automatically treated as fresh composed-tree proof.

## Verified continuation increments
- Seed: 57 of 57 SHA-256 matches; RESUME-01a097f8-seed.json.
- V05 restored candidate: 13 of 13 parent-confirmed source hashes; V05-resume-integration.md and original Wave0 manifest.
- V05 focused candidate: parent parsed V05-resume-focused.json: success true, 17 files, 347 passed, zero failed/pending. V05-resume-build.log records successful build; private ui/dist confirmed.
- V05 settings pixels: V05-image-capable-review.md, eight actual image-bearing Read results and eight visual passes. Not full-app/native evidence; no WCAG ratio measured.
- V05 native/manual still unexecuted; full-suite baseline has three pre-existing push-stub failures, not waived as passes.
- V05 resumed full suite executed: 212 files, 4,399 tests, 4,396 passed, three known PushClient failures, zero pending; exit 1. All 394 UI source hashes stayed identical. See V05-resume-full-review.md and JSON/monitor receipts. This is not a full-suite PASS or final paired implementation evidence.
- A09 producer handoff: parent confirmed 12 owned source hashes plus the lock and two codec inputs; read all five final selector exits (0), actual PID/CWD/replay/crash/capacity evidence and cleanup receipts. See A09-parent-handoff-review.md. Downstream A10 is resumed in the original DAG; aggregate, native and platform acceptance remains open.
- A10 queued output budget repair: parent independently passed 14 writer,
  budget, PTY and machine socket tests, then the combined continuous 64 MiB
  actual PTY/blocked upgraded socket/reconnect scenario. Original PID 15492
  and sibling 15509 were preserved during the scenario and reaped at teardown;
  exact PID checks and empty supervisor removal completed. See
  A10-output-parent-socket-review.md and A10-live-parent-corrected.log.
  This closes only the scoped output budget todo. Retirement, cancellable
  input composition, events, platforms and full A10 remain open.

Current disjoint continuation owners:
- st_01a09869: owner retirement and replacement durable operation reconciliation,
  with session API/journal scope and parent contention regressions preserved.
- st_01a098a6: dropped sole PTY output receiver lifecycle cleanup.
- st_01a098a9: remaining real relay identity/two-root/lost-reply/control proofs.
- st_01a098ac: A12 workspace event snapshots, revisions, availability and Git
  invalidation, with session metadata feed reserved for later composition.
  This worker cannot edit the active retirement owner's service/server files.
One aggregate follows stable composition, not each producer's scheduler status.

## AC01-AC12
Platform repair increments do not close platform acceptance:
- Windows clipboard compiler boundary: parent matched current SHA-256
  `4ee03a13564a2245129eeb7ccba334a5e0147922e6dd78eb77a0b7948288156e`
  to native Windows verified source. Build exit 0, six clipboard conversion
  tests passed, all five command children waited, runtime roots removed.
  This scoped repair is complete. Q4-windows-repaired-verification.md retains
  16 remote and two worktree failures plus integration compilation failure.
  Worker st_01a0984d continues their repair in the private Windows snapshot.
- Linux readiness framing: exact child bytes prove libtest prefix interference;
  candidate remote 254/catalog 4/build passed. Six marker delimiters are now
  composed without replacing existing prunable-preview changes. See
  Q4-linux-readiness-parent-composition.md. Current composed-runtime gate pending.
- A10 remains PARTIAL / NOT READY despite focused GREEN; see
  A10-parent-gap-dispatch.md and A10-legacy-owner-repair-boundary.md.

Journal contention is now runtime-reproduced, not only a source concern:
`A09-journal-contention-RED.log` records a real session GET holding up an
independent HTTP request behind the journal writer's mutex. After moving
list/detail work to run_blocking with retained admission and cancellation,
the same test passed in `A09-journal-contention-GREEN.log`. Both runs joined
their writer/gateway and removed the fixture root. Mutation reconciliation
subsequently reproduced the same starvation and passed after offloading both
reconciliation calls. Six combined HTTP tests now pass, including revocation,
actual10s/40s deadlines before mutex release, retained admission permits,
and final worker drainage. All six passed again after routed HTTP composition;
the real session HTTP/PTY regression also passed. Last-owner retirement exits
the handover fixture before final assertions, so that exit0 is explicitly NOT
GREEN. Remaining socket callers and aggregate acceptance are open.
Earlier compiler/fixture failures
remain recorded separately. See A09-journal-parent-progress.md.

### AC01
Requirement: Add Project offers Local, SSH, and Paired Daemon. A paired machine can browse its own home and other permitted directories without SSH credentials or a remote desktop GUI.

Status: NOT ACCEPTED.
Evidence: pending composed implementation and actual-surface proof.

### AC02
Requirement: Registration returns the remote daemon's canonical project identity and persists on that daemon. Restarting the desktop does not lose the project; restarting the remote daemon retains registration, without inventing replacement terminal processes.

Status: NOT ACCEPTED.
Evidence: pending composed implementation and actual-surface proof.

### AC03
Requirement: Local, SSH, and paired-daemon projects coexist in the Sidebar. Identical folder names, absolute paths, branch names, and raw session IDs on different machines remain separate. Every remote row has a machine label and connection status.

Status: NOT ACCEPTED.
Evidence: pending composed implementation and actual-surface proof.

### AC04
Requirement: A daemon project supports multiple tabs and split panes using the existing `NativeTerminalPane`, layout tree, tab bars, focus model, search, copy/paste text, shortcuts, and native menus. Each new shell runs in the selected remote root or managed worktree.

Status: NOT ACCEPTED.
Evidence: pending composed implementation and actual-surface proof.

### AC05
Requirement: Remote managed worktrees can be created and safely deleted. Branch, dirty-state, locked-worktree, live-session, and root-boundary checks run on the owning daemon. Plain folders remain terminal-only.

Status: NOT ACCEPTED.
Evidence: pending composed implementation and actual-surface proof.

### AC06
Requirement: Coding agents launched in the remote shell run on the remote machine. Their activity and authoritative provider-session references are attributed to the correct host and pane. Remote paths are never probed as local paths.

Status: NOT ACCEPTED.
Evidence: pending composed implementation and actual-surface proof.

### AC07
Requirement: Switching projects, changing the selected machine, opening settings, closing a renderer, or losing the relay connection does not kill other sessions or recreate shells. Reconnection attaches to the original target or reports it expired.

Status: NOT ACCEPTED.
Evidence: pending composed implementation and actual-surface proof.

### AC08
Requirement: A forced-relay test with direct paths disabled passes directory browsing, registration, worktree operations, terminal creation, input, output, resize, interrupt, close, and event reconciliation.

Status: NOT ACCEPTED.
Evidence: pending composed implementation and actual-surface proof.

### AC09
Requirement: Legacy mobile mirror clients retain their existing active-desktop-session policy and path-redacted payloads. They cannot obtain machine privileges by changing a URL, JSON field, user agent, or client-side selection.

Status: NOT ACCEPTED.
Evidence: pending composed implementation and actual-surface proof.

### AC10
Requirement: Revocation, stale epochs, cross-host tickets, replayed tickets, stale controller connections, malformed paths, and operation retries fail safely. No permanent bearer appears in a URL or log.

Status: NOT ACCEPTED.
Evidence: pending composed implementation and actual-surface proof.

### AC11
Requirement: Offline projects and layout snapshots remain visible. An unavailable or incomplete inventory is not interpreted as an authoritative empty inventory. No operation falls back to Local or SSH when a daemon target fails.

Status: NOT ACCEPTED.
Evidence: pending composed implementation and actual-surface proof.

### AC12
Requirement: Compatibility and rollback are demonstrated across relay, remote daemon, local daemon, desktop, and legacy mobile versions, without terminating a developer's or user's unrelated running daemon.

Status: NOT ACCEPTED.
Evidence: pending composed implementation and actual-surface proof.

## A01-A24 packet mappings
### A01 — Baseline fixtures and test boundaries

Dependencies: none.

Files: existing `ui/src/App.remoteHostShortcuts.test.tsx`, `ui/src/components/RemoteDirectoryPicker.test.tsx`, `src-tauri/src/remote/security_socket_tests.rs`, and `src-tauri/tests/daemon_handover_contract.rs`; new `ui/src/test/pairedDaemonFixtures.ts` and `src-tauri/tests/support/paired_daemon_fixture.rs`.

RED: demonstrate that a fixture/harness which keys machines only by raw session ID mixes output for two hosts; demonstrate that the current desktop suppression test suite describes the undesired product behavior. Keep its replacement assertions assigned to A20 rather than deleting the safety tests now.

GREEN / acceptance: fixture routing is fully host-qualified, can simulate latency/revocation/disconnect/ambiguous POST outcomes, and uses temporary data/socket directories. Record baseline Local/SSH/mobile behavior and exact test commands. No production behavior changes in this packet.

Manual: inspect test process/socket/data locations and confirm no fixture touches the user's running daemon, real relay credentials, or repository roots.

Evidence status: INHERITED SCOPED ACCEPTANCE; verify against final composed tree.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A02 — Target, API, and persistence DTO contracts

Dependencies: A01.

Files: `ui/src/lib/scopedContracts.ts`, `src-tauri/src/scoped_contracts.rs`, `ui/src/lib/tauri.ts`, `ui/src/lib/types.ts`; new `src-tauri/src/remote/machine_protocol.rs`, `ui/src/lib/pairedDaemonContracts.ts`, and their unit tests.

RED: paired targets are rejected by the old union; missing host/remote workspace ID, unknown kinds, cross-host collisions, noncanonical u64 strings, and oversized JSON must fail decoding.

GREEN / acceptance: shared fixtures establish all response/error DTOs in sections 3–5, host-qualified ID hashing, native-versus-remote epoch separation, and exhaustive target parsing. No unknown target becomes Local. New DTOs are additive and not enabled by a UI flag alone.

Manual: compare one JSON fixture across Rust and TypeScript, including an epoch larger than JavaScript's exact integer range.

Evidence status: INHERITED SCOPED ACCEPTANCE; verify against final composed tree.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A03 — Machine grants and authenticated capability negotiation

Dependencies: A02.

Files: `src-tauri/src/remote/auth.rs`, `src-tauri/src/remote/protocol.rs`, `src-tauri/src/remote/server.rs`, `src-tauri/src/remote/relay_client.rs`, `src-tauri/src/daemon/protocol.rs`, `src-tauri/src/cli.rs`; existing auth/security tests.

RED: a legacy Control token attempts directory browsing, machine creation, and a forged machine exchange; all must be denied. A stale pairing generation must not obtain the new grant.

GREEN / acceptance: issuer-approved machine purpose survives persistence/exchange; old grants deserialize as mirror; capability endpoint is authenticated; revoked grants fail before any probe. Preserve one PIN coordinator and add additive local-daemon capability discovery.

Manual: issue one mirror PIN and one machine PIN using the owner CLI; verify displayed authority and returned grant scopes without recording secrets in artifacts.

Evidence status: INHERITED SCOPED ACCEPTANCE; verify against final composed tree.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A04 — Shared headless workspace/session services

Dependencies: A02.

Files: `src-tauri/src/daemon/server.rs`, `src-tauri/src/daemon/mod.rs`, `src-tauri/src/remote/state.rs`, `src-tauri/src/remote/backend.rs`; new `src-tauri/src/daemon/workspace_service.rs` and `src-tauri/src/daemon/session_service.rs`.

RED: prove a gateway backed by a different registry/service cannot see a session created through daemon IPC; prove unregistered-root spawn and bypassed cwd validation are rejected.

GREEN / acceptance: one set of shared handles backs IPC and HTTP. Move, rather than duplicate, spawn/canonicalization/claim logic. Test constructors without machine services fail closed. No strong ownership cycle or Tauri AppHandle is required in headless tests.

Manual: run the temporary headless fixture without a desktop process; inspect that one server owns registry, session metadata, and PTYs.

Evidence status: INHERITED SCOPED ACCEPTANCE; verify against final composed tree.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A05 — Durable remote project catalog

Dependencies: A04.

Files: `src-tauri/src/worktree/registry.rs`, `src-tauri/src/daemon/workspace_service.rs`, `src-tauri/src/daemon/server.rs`; new `src-tauri/src/remote/workspace_catalog.rs` and `src-tauri/tests/machine_catalog_persistence.rs`.

RED: register a plain/Git root, restart the isolated daemon, and observe the current in-memory registration loss. Concurrent alias registration, disk-full/write failure, and missing-root restore must be exercised.

GREEN / acceptance: durable catalog commit precedes success/event; alias registration is canonical and atomic; unavailable rows survive; all daemon registry writers share the mutation gate. Both `ssh:` and desktop `daemon:` IDs are refused by local-path registry operations.

Manual: inspect owner-only permissions and restart only the isolated fixture; ensure invalid roots are marked unavailable rather than overwritten or recreated.

Evidence status: INHERITED SCOPED ACCEPTANCE; verify against final composed tree.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A06 — Authenticated directory API

Dependencies: A03, A04.

Files: `src-tauri/src/remote/server.rs`, `src-tauri/src/remote/mod.rs`; new `src-tauri/src/remote/filesystem.rs` and `src-tauri/src/remote/filesystem_tests.rs`.

RED: home/parent/hidden browsing is unavailable in the baseline. Test spaces, quotes, Unicode, symlink directories, files, missing/denied roots, NUL, malformed encoding, huge directories, and unauthorized tokens.

GREEN / acceptance: exact section 4.2 contract, native filesystem listing, bounds/deadlines, no async blocking, and no side effects. Unsupported Windows UNC paths fail explicitly. A canceled old request cannot publish a later picker result.

Manual: browse an isolated remote home, its parent, a directory outside home, and a truncated fixture; verify no local filesystem listing is involved.

Evidence status: SCOPED DARWIN BACKEND EVIDENCE CHECKED; platform and native picker adoption remain open.
Source artifact: src-tauri/src/remote/filesystem.rs and filesystem_tests.rs; inherited source review in WAVE1-resume-acceptance-gaps.md, Q1 source/manifests in Q-batch-parent-before.json and Q-batch-parent-after.json.
RED artifact: A06-budget-resume-RED-lower-burst-saturation.log and A06-budget-resume-RED-upper-refill-v2.log exercise cutoff, burst/refill and saturation mutations; earlier failed attempts remain archived.
GREEN artifact: Q-batch-parent-filesystem-GREEN.log: 13 passed, exit 0; exact command and stable scoped hashes in Q-batch-parent-after.json. Historical direct/relay browsing proof: herdr-wave1/docs/evidence/paired-daemon/BATCH1-generation4-parent-runtime.log, as reconciled in WAVE1-resume-acceptance-gaps.md.
Manual artifact: Actual private HTTP/native-directory and cleanup receipts are in the logs above. Windows/Linux filesystem cases and native picker host-generation adoption remain NOT ACCEPTED.

### A07 — Project registration/list/unregister HTTP APIs

Dependencies: A05, A06.

Files: `src-tauri/src/remote/server.rs`, `src-tauri/src/remote/machine_protocol.rs`, `src-tauri/src/daemon/workspace_service.rs`; new `src-tauri/src/remote/workspace_api.rs` and `src-tauri/src/remote/machine_operation_journal.rs`.

RED: register the same canonical folder concurrently using two aliases/devices; lose a POST reply; request unregister with an active session; submit a root filesystem path. Verify no false success or duplicate ID.

GREEN / acceptance: rich canonical registration, listing completeness, durable operation journal, exact conflict/error mapping, and non-destructive unregister. The remote project ID is authoritative; no client-supplied slug controls ownership.

Manual: add a Git folder and a plain folder; restart the fixture, list both, and confirm unregister leaves disk contents unchanged.

Evidence status: INHERITED SCOPED BACKEND EVIDENCE; A12 events, native adoption and platform acceptance remain open.
Source artifact: Inherited workspace_api.rs, machine_operation_journal.rs and workspace_service.rs were reconciled in WAVE1-resume-acceptance-gaps.md. A09 has since changed shared journal/authority files; final composed verification is required.
RED artifact: Historical producer/fault artifacts must remain associated with their source generation; this continuation has not re-mapped every original A07 RED assertion and does not claim that slot complete.
GREEN artifact: Historical herdr-wave1/docs/evidence/paired-daemon/BATCH1-parent-final-remote.log and BATCH1-parent-final-integration.log cover canonical registration, journal and catalog persistence; BATCH1-crash-barrier-parent-tests.log supersedes the earlier racy crash barrier. See WAVE1-resume-acceptance-gaps.md refs F1/F2/G1/G2 and its exact command/exit attribution.
Manual artifact: Historical BATCH1-generation4-parent-runtime.log and BATCH1-generation4-parent-lost-reply.log capture private owner restart, original canonical identity, lost reply, busy refusal and non-destructive unregister. No actual desktop adoption, authenticated A12 event stream, Windows/Linux or OS-blocked direct network proof is inferred.

### A08 — Worktree list/create/preview/delete

Dependencies: A07.

Files: `src-tauri/src/remote/server.rs`, `src-tauri/src/remote/workspace_api.rs`, `src-tauri/src/daemon/workspace_service.rs`, `src-tauri/src/worktree/registry.rs`; extend `src-tauri/tests/worktree_safety.rs`, add `src-tauri/tests/machine_worktrees.rs`.

RED: current mirror response lacks the rich machine row; test wrong wsId, branch injection, dirty/locked/root deletion, symlink replacement, busy worktree, unmerged branch, and retry after lost response.

GREEN / acceptance: extend existing POST/DELETE rather than mount duplicate routes; GET/status return machine metadata; legacy responses stay redacted; bounded Git jobs and serialized rechecks enforce all deletion rules. Partial branch-cleanup outcomes are recoverable.

Manual: create a feature worktree, modify a file and observe refusal, clean it, explicitly close its sessions, then delete with and without branch deletion.

Evidence status: SCOPED BACKEND AND Q2/Q3 FOLLOWTHROUGH CHECKED; native/events/platform gates remain open.
Source artifact: A08-prunable-parent-source-review.md, A08-prunable-resume.md and Q-batch-parent-before.json / Q-batch-parent-after.json identify the reviewed preview/test changes and stable scoped inputs. Historical transaction/process fixes remain in WAVE1-resume-acceptance-gaps.md.
RED artifact: A08-prunable-resume-RED.log and .exit preserve the missing-preview failure; A08-wire-seams-resume-attempt1.log and .exit preserve the initial wire-fixture failure. The three parent worktree failures caused by repository-contained TMPDIR are separately explained in Q-batch-parent-review.md, not labeled production RED.
GREEN artifact: Q-batch-parent-worktree-authority-wire-attempt1.log: 13 passed, exit 0; Q-batch-parent-worktree-http-safety-GREEN.log: preview 1, machine worktrees 2, safety 9; Q-batch-parent-worktree-manager-GREEN.log: 44 passed. Exact commands/hashes are in the before/after manifests. These close Q2 named wire seams and Q3 rich preview only.
Manual artifact: Actual non-HEAD HTTP 201/exact checkout, Local UDS prune failure, redacted legacy 409, rich missing/locked preview and owned-resource cleanup are captured in those logs. Tauri invocation, A12 events, Windows/Linux and forced-relay network exclusion remain NOT ACCEPTED.

### A09 — Machine session CRUD and idempotent spawn

Dependencies: A03, A04, A05, A07.

Files: `src-tauri/src/daemon/session_service.rs`, `src-tauri/src/remote/server.rs`, `src-tauri/src/remote/machine_operation_journal.rs`; new `src-tauri/src/remote/session_api.rs` and `src-tauri/tests/machine_sessions.rs`.

RED: baseline has no authenticated project-scoped terminal creation route. Test cwd escape, remote SSH startup injection, invalid shell override, split-parent mismatch, lost creation reply, and creation after grant revocation.

GREEN / acceptance: remote shell/cwd/provider validation uses the shared spawn path, the journal returns the same target on retry, and machine-owned metadata is explicit. List/detail/close distinguish unreachable from exited. Close is not implicit on detach.

Manual: create two sessions under different fixture roots and compare `pwd`, process IDs, and remote shell identity; verify no process started in the desktop repository.

Evidence status: PRODUCER HANDOFF CHECKED; aggregate Wave2 and platform acceptance remain open.
Source artifact: A09-resume-source-delta.json lists 12 owned files plus three preserved inputs; parent independently matched all 15 hashes. See A09-parent-handoff-review.md for the reviewed source subset and limitations.
RED artifact: A09-resume-RED.log records actual 503 versus expected 201 before implementation; A09-resume-legacy-RED.log preserves the strict legacy-response regression, repaired without weakening its assertion.
GREEN artifact: A09-resume-final-validation.log records exit 0 for machine_sessions, authority, legacy, absent and shared selectors. Commands and scenario counts are transcribed in A09-resume-implementation.md; A09-parent-handoff-review.md independently checks final result records, not just the report.
Manual artifact: Real HTTP/PTY roots, original PID/CWD/epoch replay, dropped TCP reply, private-owner crash phases, 64-live-session refusal and close/reap/lifecycle cleanup are in the final log and A09-resume-cleanup.log. Native desktop, platform and A10 controlling-socket acceptance are not inferred.

### A10 — Scoped terminal attachment, replay, and controller fencing

Dependencies: A09.

Files: `src-tauri/src/remote/server.rs`, `src-tauri/src/remote/protocol.rs`, `src-tauri/src/remote/security_socket_tests.rs`; new `src-tauri/src/remote/terminal_wire.rs` and `src-tauri/tests/machine_terminal_stream.rs`.

RED: focus change closes a second machine pane under existing mirror rules; reconnect currently has no initial cursor. Test wrong epoch, no-history attach, replay gap, stale-controller input, other-device conflict, View resize, and revocation during input.

GREEN / acceptance: only machine-authorized sessions bypass mirror focus coupling, both admission and watcher are scoped, metadata boundaries/replay are correct, and machine-only sessions are hidden from mirror headless fallback. View cannot resize a PTY.

Manual: attach two fixture sessions, change active desktop mirror selection repeatedly, and verify both machine streams continue while mirror restrictions remain effective.

Evidence status: NOT ACCEPTED; required proof pending.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A11 — Relay method/path/query compatibility

Dependencies: A02, A10.

Files: `src-tauri/src/remote/relay_server.rs`, `src-tauri/src/remote/relay_client.rs`; extend relay inline tests and `src-tauri/tests/relay_pairing_generation_regression.rs`.

RED: forced-relay `fs/directories`, capabilities, and plural sessions currently fail admission; epoch/cursor query fields are rejected. Test DELETE body loss, encoded query paths, cross-machine/replayed ticket, and unknown route rejection.

GREEN / acceptance: exact new method/path/query allowlists, bounded proxy traffic, preserved existing ticket/auth checks, and actual HTTP/WS over the unchanged data channel. No generic filesystem proxy or secret query fallback.

Manual: use a local relay fixture with the gateway unreachable directly, then verify each allowed method and an intentional forbidden route.

Evidence status: NOT ACCEPTED; required proof pending.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A12 — Machine events and snapshot reconciliation

Dependencies: A07, A08, A09, A10.

Files: `src-tauri/src/remote/state.rs`, `src-tauri/src/remote/server.rs`, shared daemon services; new `src-tauri/src/remote/machine_events.rs` and `src-tauri/tests/machine_events.rs`.

RED: mutate during snapshot subscription and overflow a deliberately tiny fixture event buffer; verify that an unfiltered legacy broadcaster would disclose a machine path.

GREEN / acceptance: independent grant-filtered streams, subscribe-before-snapshot ordering, revisioned invalidation, remote agent/session lifecycle, and explicit resync after lag. No periodic full Git inventory polling in idle desktop operation.

Manual: create/remove a worktree from a second authorized fixture client and watch the first reconcile without changing its active pane.

Evidence status: NOT ACCEPTED; required proof pending.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A13 — Native paired host and credential inventory

Dependencies: A03, A11.

Files: `ui/src/state/remoteHostStore.ts`, `src-tauri/src/ipc/mod.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/daemon/protocol.rs`, `src-tauri/src/daemon/client.rs`; new `src-tauri/src/paired_host/mod.rs`, `src-tauri/src/paired_host/inventory.rs`, and `src-tauri/src/ipc/paired_host.rs`.

RED: old frontend serialization leaks the desktop machine bearer and treats token presence as sufficient authority. Test origin-wide legacy tokens, failed migration writes, host forgetting, re-pair generations, and unknown local-daemon capabilities.

GREEN / acceptance: private daemon inventory is authoritative; React receives sanitized records; migration copies/verifies before removing legacy data; offline paired hosts remain listed. Do not migrate mobile browser storage or silently elevate mirror grants.

Manual: pair/re-pair a fixture machine and inspect sanitized UI storage, private-file permissions, and token-redacted diagnostic output.

Evidence status: NOT ACCEPTED; required proof pending.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A14 — Typed paired HTTP client and desktop project mapping

Dependencies: A07, A08, A09, A11, A13.

Files: `src-tauri/src/ipc/paired_host.rs`, `src-tauri/src/daemon/protocol.rs`, `src-tauri/src/daemon/client.rs`; new `src-tauri/src/paired_host/client.rs`, `src-tauri/src/paired_host/projects.rs`, and `ui/src/lib/pairedDaemonProject.ts`.

RED: a changed selected host or an HTTP redirect would send a request/token to the wrong target; a stale host generation would adopt the wrong registration. Test non-JSON relay errors and unknown capabilities.

GREEN / acceptance: every operation captures and verifies host identity/generation, resolves credentials natively, uses host-qualified relay URLs, disables credential redirects, enforces deadlines/body limits, and maps remote IDs to desktop IDs. Operation reconciliation precedes mutation retries.

Manual: register equal paths on two fixture hosts through the native command boundary and verify distinct desktop workspace IDs and correct response provenance.

Evidence status: NOT ACCEPTED; required proof pending.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A15 — Local paired-daemon proxy runtime

Dependencies: A10, A12, A14.

Files: `src-tauri/src/terminal/mod.rs`, `src-tauri/src/terminal/service.rs`, `src-tauri/src/terminal/output_hub.rs`, `src-tauri/src/daemon/proxy.rs`; new `src-tauri/src/terminal/paired_daemon.rs` and runtime tests.

RED: feeding framed remote bytes directly into native output corrupts replay; raw backend IDs collide across hosts; reconnect through Create duplicates a shell. Exercise out-of-order stale callbacks, gap recovery, slow consumers, and local/remote epoch changes.

GREEN / acceptance: separate Create and Reattach APIs, durable proxy descriptor, remote wire parser, local sequence mapping, bounded buffers, controller fencing, and exact routing for write/resize/signal/close. Missing proxy lookup never falls through to a local PTY or SSH runtime.

Manual: force a relay disconnect during an interactive shell, reconnect, and compare the remote process identity before/after; inspect that only a proxy, not another local PTY, exists locally.

Evidence status: NOT ACCEPTED; required proof pending.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A16 — Native IPC, input, lifecycle, and metadata integration

Dependencies: A15.

Files: `src-tauri/src/daemon/server.rs`, `src-tauri/src/daemon/client.rs`, `src-tauri/src/daemon/protocol.rs`, `src-tauri/src/ipc/terminal.rs`, `src-tauri/src/ipc/native_terminal.rs`, `ui/src/components/NativeTerminalPane.tsx`, `ui/src/state/workspaceStore.ts`.

RED: native attach/input/CWD/close paths assume local or SSH backend ownership. Test an attached paired proxy, a detached surface sending input, same raw ID on another host, and remote lifecycle while the workspace is inactive.

GREEN / acceptance: existing native surface APIs operate on proxy IDs; local output framing remains compatible; backend metadata/events map to the correct frontend session; detached surfaces cannot send input. No mobile renderer or xterm dependency is introduced.

Manual: native text selection, search, Unicode/IME input, bracketed paste, interrupt, pane resize, and native menu dispatch work in two paired panes beside an existing local session.

Evidence status: NOT ACCEPTED; required proof pending.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A17 — Version-3 layout migration and restart recovery

Dependencies: A13, A14, A16.

Files: `src-tauri/src/session/mod.rs`, `ui/src/lib/sessionPersistence.ts`, `ui/src/state/workspaceRestore.ts`, `ui/src/lib/projectIdentity.ts`; new paired descriptor persistence tests; extend `src-tauri/tests/daemon_persistence_contract.rs` and `ui/src/lib/sessionPersistence.test.ts`.

RED: v2 validation rejects paired targets; current local-only live inventory can mark them missing. Test local restart, remote restart, graceful legacy-owner handover, offline restore, corrupt one-row data, and backup failure.

GREEN / acceptance: one-time backup and v1/v2-to-v3 migration preserve IDs/layouts; paired descriptors recover only by reattach; incomplete inventory cannot erase rows; remote expiry has an explicit user-visible state. Credential data is never embedded in layout JSON.

Manual: close/reopen the desktop, then restart isolated local and remote fixture daemons separately. Verify project survival, remote process survival where applicable, and no automatic new-shell creation where not applicable.

Evidence status: NOT ACCEPTED; required proof pending.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A18 — Paired Daemon Add Project and generic picker

Dependencies: A14.

Files: `ui/src/components/ProjectDialogs.tsx`, `ui/src/components/RemoteDirectoryPicker.tsx`, `ui/src/lib/remoteDirectories.ts`, `ui/src/state/remoteHostStore.ts`; extend existing dialog/picker tests and add `ui/src/components/ProjectDialogs.pairedDaemon.test.tsx`.

RED: choose Paired Daemon, browse home, switch hosts before response, toggle hidden folders, and submit; assert that no SSH/native-local directory command is called. Test the empty/unpaired/offline/incompatible host states.

GREEN / acceptance: three location choices, generation-keyed cancellable sources, retained keyboard/IME behavior, safe canonical registration, and explicit pairing/upgrade messaging. Old SSH request shape and host revalidation still work.

Manual: use keyboard-only navigation and IME on a folder containing spaces/non-Latin characters; return to Local/SSH and verify unchanged forms.

Evidence status: NOT ACCEPTED; required proof pending.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A19 — Mixed-host Sidebar and worktree caches

Dependencies: A17, A18.

Files: `ui/src/components/Sidebar.tsx`, `ui/src/lib/projectIdentity.ts`, `ui/src/lib/projectGrouping.ts`, `ui/src/lib/worktreeOwnership.ts`, `ui/src/state/workspaceRuntime.ts`; extend `ui/src/state/inactiveProjectWorktrees.remote.test.tsx`, `ui/src/state/projectWorkspaceScope.test.tsx`, and Sidebar identity tests. Inspect the imported inactive-project/snapshot helpers and update their owner-qualified boundaries in the same packet.

RED: local, SSH, and two paired machines share a path/branch/slug; ensure switching, unread indicators, grouping, and root selection cannot pick a neighboring host. Fail a single host refresh and preserve its previous rows.

GREEN / acceptance: explicit paired owners, machine badges, target-aware grouping, offline/stale states, and event-driven worktree refresh. Partial inventories are never reconciled as empty complete lists.

Manual: expand all four projects, switch among their worktrees, and confirm retained layouts and host labels after an inactive project's metadata changes.

Evidence status: NOT ACCEPTED; required proof pending.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A20 — Restore the desktop shell and shortcut/menu behavior

Dependencies: A16, A19.

Files: `ui/src/App.tsx`, `ui/src/components/RemoteHostSwitcher.tsx`, `ui/src/App.remoteHostShortcuts.test.tsx`, `ui/src/App.remote.test.tsx`; add `ui/src/App.pairedDaemon.test.tsx`.

RED: selecting a paired host currently mounts `RemoteHostConnection` and suppresses desktop actions. Add assertions for persistent Sidebar/main, command palette, split/new-tab/menu actions, and unaffected local sessions.

GREEN / acceptance: remove the desktop mirror import/branch and audit all active-host global guards. Route actions by actual owner/capability. Keep mobile `RemoteApp` entry behavior and mobile security tests. Host selection no longer dismisses the command palette or destroys layout.

Manual: with a paired project active, exercise actual macOS menu items and keyboard shortcuts, change host selection in Settings, then perform a local-project action without clearing host inventory selection.

Evidence status: NOT ACCEPTED; required proof pending.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A21 — Worktree/removal actions, recovery UI, and agent boundaries

Dependencies: A08, A16, A19, A20.

Files: `ui/src/components/ProjectDialogs.tsx`, `ui/src/components/WorktreeDeleteDialog.tsx`, `ui/src/components/TerminalPane.tsx`, `ui/src/state/workspaceStore.ts`, `ui/src/lib/remoteProject.ts`, `ui/src/lib/agentSessionDiscovery.ts`; extend worktree, terminal recovery, and remote-project tests.

RED: an ordinary desktop removal reaches the old remote unregister/close path; paired reconnect enters SSH recovery; remote provider discovery probes a macOS path; unsupported image/file actions leak local paths.

GREEN / acceptance: separate remove/detach/unregister/delete/close semantics, typed target-specific mutations and recovery, remote-only provider validation, readable offline/expired/conflict errors, and disabled unsupported affordances. Text terminal input and manual agent launch work without optional agent APIs.

Manual: start an installed coding agent in one remote pane and a shell in another, remove/re-add the desktop reference without killing them, then explicitly close and safely delete a disposable worktree.

Evidence status: NOT ACCEPTED; required proof pending.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A22 — Host management UX, operator guidance, and feature gate

Dependencies: A17, A18, A20, A21.

Files: `ui/src/components/settings/RemoteAccessSection.tsx`, `ui/src/components/RemoteHostSwitcher.tsx`, `src-tauri/src/cli.rs`, repository guidance files `ui/AGENTS.md` and `src-tauri/src/remote/AGENTS.md`; new `docs/paired-daemon-machine-access.md`.

RED: unsupported relay/local-daemon versions or mirror-only grants previously look merely offline or open the mirror. Test feature disablement with existing paired layouts and confirmation on forgetting credentials.

GREEN / acceptance: explicit Pair/Add Project/manage/re-pair/forget controls, version and scope explanations, operator commands for issuing machine PINs, documented native architecture, and the machine-only raw-path exception. A disabled `pairedDaemonProjectsV1` gate preserves data and never restores the hijacking mirror branch.

Manual: read the guide from a clean Linux fixture and complete daemon start, owner-issued PIN, desktop pairing, remote project add, and safe cleanup without SSH.

Evidence status: NOT ACCEPTED; required proof pending.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A23 — Full integration, security, and resource regression

Dependencies: A01, A02, A03, A04, A05, A06, A07, A08, A09, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22.

Files: new `src-tauri/tests/paired_daemon_relay_e2e.rs`, `src-tauri/tests/paired_daemon_native_proxy.rs`, and `ui/src/state/workspaceStore.pairedDaemon.test.tsx`; extend existing remote security, SSH browse/project, native surface, persistence, and handover suites.

RED: inject relay loss after spawn but before response, event lag during snapshot, revocation during queued mutation, resource-limit exhaustion, and the same raw IDs across hosts. Each must demonstrate a meaningful failure in an intentionally broken fixture/implementation before its fix is accepted.

GREEN / acceptance: all AC01–AC12 have recorded evidence; forced relay and direct gateway contract tests agree; legacy mirror, SSH, Local, handover, and native paths pass. Bound queues/processes remain stable under slow consumers. No test depends on public relay availability.

Manual: execute the scenario matrix in section 10 on macOS plus an isolated Linux host; record native-menu checks and real remote process/CWD evidence separately from mocked tests.

Evidence status: NOT ACCEPTED; required proof pending.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

### A24 — Staged rollout and rollback rehearsal

Dependencies: A23.

Files: `docs/paired-daemon-machine-access.md`, existing local release tooling/configuration, and a release evidence note under the project's normal documentation location. Do not introduce hosted release builds or automatic production daemon restarts.

RED: rehearse old-relay/new-client and old-local-daemon/new-UI combinations; verify the release gate catches rejected routes/query fields and v3-file downgrade hazards.

GREEN / acceptance: deploy relay compatibility first, remote daemon services second, local daemon/native proxy third, then enable the desktop feature for canaries. Rehearse disablement and explicit downgrade using the v2 backup with writers stopped. Preserve remote projects/sessions and credential stores during UI rollback.

Manual: complete the compatibility table and rollback scenario in section 11, then sign off only after all required native and Linux evidence is attached.

Evidence status: NOT ACCEPTED; required proof pending.
Source artifact: each named file above must be inspected; existence alone is insufficient.
RED artifact: pending mapping to actual failing assertion.
GREEN artifact: pending mapping to executed test/build exit.
Manual artifact: pending mapping to actual-surface evidence and cleanup receipt.

## Required command evidence
### CMD01
```sh
bun run --cwd ui test src/components/RemoteDirectoryPicker.test.tsx src/components/ProjectDialogs.test.tsx src/state/remoteHostStore.test.ts src/lib/remoteProject.test.ts
```
Result: NOT RUN on final composed implementation.
Artifact: pending full log, exit, source revision and platform.

### CMD02
```sh
bun run --cwd ui test src/App.remoteHostShortcuts.test.tsx src/App.pairedDaemon.test.tsx src/components/ProjectDialogs.pairedDaemon.test.tsx src/state/workspaceStore.pairedDaemon.test.tsx
```
Result: NOT RUN on final composed implementation.
Artifact: pending full log, exit, source revision and platform.

### CMD03
```sh
bun run --cwd ui test src/lib/projectIdentity.test.ts src/lib/projectGrouping.test.ts src/lib/worktreeOwnership.test.ts src/state/workspaceRestore.test.tsx
```
Result: NOT RUN on final composed implementation.
Artifact: pending full log, exit, source revision and platform.

### CMD04
```sh
bun run --cwd ui test
```
Result: NOT RUN on final composed implementation.
Artifact: pending full log, exit, source revision and platform.

### CMD05
```sh
bun run --cwd ui build
```
Result: NOT RUN on final composed implementation.
Artifact: pending full log, exit, source revision and platform.

### CMD06
```sh
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote::
```
Result: NOT RUN on final composed implementation.
Artifact: pending full log, exit, source revision and platform.

### CMD07
```sh
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test machine_catalog_persistence --test machine_worktrees --test machine_sessions --test machine_terminal_stream --test machine_events
```
Result: NOT RUN on final composed implementation.
Artifact: pending full log, exit, source revision and platform.

### CMD08
```sh
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test paired_daemon_relay_e2e --test paired_daemon_native_proxy
```
Result: NOT RUN on final composed implementation.
Artifact: pending full log, exit, source revision and platform.

### CMD09
```sh
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test worktree_safety --test daemon_persistence_contract --test daemon_handover_contract --test remote_project_public_contract --test relay_pairing_generation_regression
```
Result: NOT RUN on final composed implementation.
Artifact: pending full log, exit, source revision and platform.

### CMD10
```sh
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay
```
Result: NOT RUN on final composed implementation.
Artifact: pending full log, exit, source revision and platform.

### CMD11
```sh
cargo check --manifest-path src-tauri/Cargo.toml
```
Result: NOT RUN on final composed implementation.
Artifact: pending full log, exit, source revision and platform.

### CMD12
```sh
git diff --check
```
Result: NOT RUN on final composed implementation.
Artifact: pending full log, exit, source revision and platform.

## Required manual scenario evidence

### MANUAL01 - Fresh Linux machine
Procedure: Start `ferryx-cli --daemon` in an isolated service account; issue `pair generate --access machine`; pair from macOS; Add Project → Paired Daemon → home/folder.
Required evidence: No SSH credentials, no remote GUI; canonical remote path/ID; Sidebar machine badge; no `RemoteHostConnection` in desktop.
Status: NOT ACCEPTED; artifact and owned-resource cleanup pending.

### MANUAL02 - Non-Git folder
Procedure: Register a readable plain folder and open a terminal.
Required evidence: Remote `pwd`; worktree creation disabled with explanation; no implicit Git initialization.
Status: NOT ACCEPTED; artifact and owned-resource cleanup pending.

### MANUAL03 - Multi-pane agent workflow
Procedure: Open root shell, create feature worktree, split horizontally and vertically, launch an installed agent in one pane.
Required evidence: Independent remote process IDs/CWDs, correct agent badge, functioning native tab bar, selection/search/copy/IME/resize.
Status: NOT ACCEPTED; artifact and owned-resource cleanup pending.

### MANUAL04 - Desktop integrity
Procedure: Keep an active local shell and SSH project; operate command palette, native menu, new tab, split, switch workspace, and notification focus with a paired host selected.
Required evidence: Correct owning target on every action; no global shortcut suppression; old local process survives.
Status: NOT ACCEPTED; artifact and owned-resource cleanup pending.

### MANUAL05 - Equal paths/IDs
Procedure: Two fixture daemons expose the same path, workspace label, branch, and raw terminal ID.
Required evidence: Different desktop/proxy IDs; no mixed output, wrong close, overwritten layouts, or crossed unreads.
Status: NOT ACCEPTED; artifact and owned-resource cleanup pending.

### MANUAL06 - Forced relay
Procedure: Disable machine direct upgrade, block direct gateway access from the desktop, use the relay fixture.
Required evidence: Actual browse/register/worktree/session/WS operations succeed over data channels.
Status: NOT ACCEPTED; artifact and owned-resource cleanup pending.

### MANUAL07 - Lost creation reply
Procedure: Drop the connection after remote spawn but before its response. Reopen the pending operation.
Required evidence: Same request resolves to the same remote process; no second shell.
Status: NOT ACCEPTED; artifact and owned-resource cleanup pending.

### MANUAL08 - Offline/reconnect
Procedure: Interrupt relay traffic while local and remote panes exist; restore connectivity.
Required evidence: Cached rows retained, local UI responsive, same remote target reattached, fresh ticket, no replay duplication.
Status: NOT ACCEPTED; artifact and owned-resource cleanup pending.

### MANUAL09 - Desktop/local/remote restart
Procedure: Restart each isolated component separately, then exercise explicit handover.
Required evidence: Projects/layouts persist; remote PTY survival distinguished from expiry; owner epoch honored; no automatic Create during restore.
Status: NOT ACCEPTED; artifact and owned-resource cleanup pending.

### MANUAL10 - Guarded destruction
Procedure: Try deleting dirty, locked, root, wrong-owner, and busy worktrees; then close a disposable session and delete safely.
Required evidence: Typed refusal, revalidation at commit, no unrelated branch/path deletion; branch option respected.
Status: NOT ACCEPTED; artifact and owned-resource cleanup pending.

### MANUAL11 - Remove versus unregister
Procedure: Remove a project from desktop, re-add it, then explicitly unregister an idle disposable project.
Required evidence: Remove is local/detach only; unregister leaves disk; remote sessions are not killed accidentally.
Status: NOT ACCEPTED; artifact and owned-resource cleanup pending.

### MANUAL12 - Auth and legacy mobile
Procedure: Use a mirror token for machine routes/targets; revoke a machine token during live IO; replay a ticket against another host.
Required evidence: Denials, stopped IO after revocation, preserved mirror focus behavior, no raw-path exposure to mirror payloads.
Status: NOT ACCEPTED; artifact and owned-resource cleanup pending.

### MANUAL13 - Slow consumers/limits
Procedure: Eight visible native panes; high-volume output in one, delayed socket consumer in another; hit session/listing limits.
Required evidence: Bounded memory/queues, explicit backpressure/replay gap, no input latency coupling to unrelated panes.
Status: NOT ACCEPTED; artifact and owned-resource cleanup pending.

### MANUAL14 - Host changes
Procedure: Forget/re-pair a host while browsing/adding; return a delayed old request afterward.
Required evidence: Old generation is ignored; no credential cross-use or late Sidebar insertion.
Status: NOT ACCEPTED; artifact and owned-resource cleanup pending.

## Compatibility and rollback evidence

### COMPAT01 - New desktop, old local daemon
Required behavior: Local/SSH continue; paired creation/attachment disabled with capability explanation. No forced process kill or schema assumptions.
Status: NOT ACCEPTED; actual rehearsal artifact pending.

### COMPAT02 - New remote daemon, old relay
Required behavior: Machine gate remains off for that host; explicit relay compatibility failure, not misleading empty directory or silent security downgrade.
Status: NOT ACCEPTED; actual rehearsal artifact pending.

### COMPAT03 - New desktop, old remote daemon
Required behavior: Host remains in inventory as upgrade required; no fallback to full-screen mirror and no invented project registration.
Status: NOT ACCEPTED; actual rehearsal artifact pending.

### COMPAT04 - New daemon, old mirror client
Required behavior: Existing pairing/request/response projections and active-session rules preserved; new machine-only resources stay inaccessible.
Status: NOT ACCEPTED; actual rehearsal artifact pending.

### COMPAT05 - Old mirror token, new desktop
Required behavior: Mirror grant remains mirror; user explicitly obtains a machine-purpose PIN. No silent scope promotion.
Status: NOT ACCEPTED; actual rehearsal artifact pending.

### COMPAT06 - New desktop/local daemon, temporarily offline host
Required behavior: Persisted projects/layouts visible with stale/offline indicators; no mutation queue that executes unexpectedly later.
Status: NOT ACCEPTED; actual rehearsal artifact pending.

### COMPAT07 - UI feature disabled after use
Required behavior: Normal desktop still renders; paired data is retained and marked disabled. Detach proxies as requested, do not delete remote processes/data.
Status: NOT ACCEPTED; actual rehearsal artifact pending.

### COMPAT08 - Downgrade to old desktop
Required behavior: Stop the newer writer, retain v3 data, restore the pre-v3 backup for old parsing, and do not overwrite new paired state with an old empty snapshot.
Status: NOT ACCEPTED; actual rehearsal artifact pending.

## Cross-cutting final gates
- Identity, scope and native-owner contracts: full plan sections 3-8; actual cross-host/epoch/revocation tests and source review required.
- Limits and deadlines: 64 sessions, 8 mutations, 1 Git writer/workspace, 4 listings/device, 10/s burst20 listing requests, request64KiB, input64KiB, control16KiB, output1MiB, read10s/Git30s/native40s/socket10s; exact scenario evidence pending.
- Timing: injected 100ms relay RTT, p95 echo below250ms at light load and no UI-thread listing stall above100ms; measurements pending.
- Platforms: actual macOS native, Linux headless and Windows compatibility; no headless-only native claim.
- Final code review: pending completed implementation and real-surface evidence.
- Rollout: relay -> remote daemon -> local proxy -> desktop capability. Production changes require explicit permission at the last step; no deployment occurred.
- Cleanup: only owned fixture processes/listeners/roots; preserve canonical daemon/PTYS and foreign worktrees. Receipts pending.
- Final completion audit: fill every evidence slot, inspect actual artifacts, reject uncertain/proxy-only proof, then and only then update the goal.
