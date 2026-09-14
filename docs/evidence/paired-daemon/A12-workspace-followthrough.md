# A12 workspace event followthrough

Task st_01a098ac. Worktree `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`.
Workspace-event implementation and scoped runtime verification delivered; **not full A12 or machineEventsV1 readiness**. Session metadata composition remains explicitly separate.

## Delivered authority

- Machine snapshots reuse the HTTP rich `Projects` projection, including canonical roots, Git metadata with credential stripping, completeness, revision and unavailable IDs. Routed `Sessions` envelopes retain owner authority through `machine_sessions_routed`; no replacement inventory or spawn path was introduced.
- Project event revisions use the durable catalog revision rather than event sequence. The additive `publish_revision` seam permits a subsequent metadata owner to supply its authoritative revision. Existing session start/exit call sites remain unchanged and use the compatibility publication method (latest observed workspace revision); their session-specific revision composition belongs to the metadata lane.
- HTTP and local registration publish after durable commit. Local registration uses the same rich projection before committing. Removal publishes after persistence. Availability revalidation persists transitions and publishes `projectAvailabilityChanged` without dropping unavailable rows. Probe failures retain rows and make the envelope partial.
- Worktree creation/removal carries the validated rich record through the existing commit seam. Deleted records are captured before deletion; no post-deletion cache or identity reconstruction. Local IPC, legacy mirror and machine HTTP callers were all updated in worktrees.rs. Existing canonical-path, prunable, transaction, deadline and Git logic was retained.
- Native notify watches live only with subscriptions. Recursive roots plus nonrecursive parent watches cover Git changes and missing/restored roots. One queued wakeup and a cancellation-safe 100ms fixed debounce window bound invalidation work; no idle Git polling. Watch registrations are capped at 2048 per subscription. Event sockets are capped at 16, concurrent snapshots at two, retained events at 64 x 16KiB, snapshot JSON at 1MiB, snapshot/read and write progress at 10 seconds. Oversized events invalidate rather than truncate authoritative records.
- Receiver admission precedes snapshot construction. Lag emits a new authoritative invalidation boundary. Socket close cancels pending snapshot admission; auth revocation still uses the existing server wrapper. No mirror broadcaster receives machine payloads.

## Real RED and GREEN

Initial authorized real WS test was strengthened before implementation with `payload.projects.completeness == "complete"`. Actual command:

```
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test machine_events -- --nocapture
```

Compilation succeeded, assertion failed at tests/machine_events.rs:35: left Null, right "complete"; exit101. Listener joined/refused and fixture root removed even on failure. Supervisor `/tmp/a12-workspace-red.aYaE4E` removed. This initial output is preserved in the task transcript rather than a standalone full RED log (a provenance limitation, not reconstructed output).

Final `A12-workspace-GREEN.log` retains the complete combined commands' output and exit0:

```
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test machine_events --test machine_worktrees --test machine_catalog_persistence --test machine_worktree_transports --test machine_prunable_preview -- --nocapture
cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay
```

Top-level targets: catalog 4, events 1, prunable preview 1, transports 2, worktrees 2 passed. Transport child fixture output includes its separately filtered invocation; no test was deleted or skipped by this lane.

The event test exercises actual ticket-authorized HTTP/WS mutations:

1. Rich initial snapshot and HTTP201 registration payload equality.
2. Concurrent mirror socket receives an ordered marker only, not the earlier machine registration/path. No absence-by-sleep assertion.
3. Native filesystem removal/restoration publishes missing/ready for the same project ID without HTTP inventory polling.
4. Real Git worktree HTTP201/DELETE204 records match committed WS creation/removal payloads.
5. External Git checkout produces an authoritative branch snapshot via debounce.
6. HTTP-created session emits its original target. A fresh ticket reconnect overlaps 70 local durable registrations while both real snapshot permits are held. The receiver is established before those commits. Real broadcast overflow is observed as reason=lag; the resnapshot retains all 71 projects and the original session target.
7. Original live PTY Arc and target survive reconnect/lag, exactly one PTY remains, and explicit HTTP close reaps it.
8. Subscription count reaches zero on close. A further socket closed while both snapshot permits remain held reaches zero before permits are released, proving cancellation rather than eventual timeout.

The race uses the actual 64-slot bounded channel, not a separately tiny fixture channel. PTY continuity is established by original object/target identity and reap, not a new PID/CWD sentinel measurement. Routed legacy-owner process continuity is parent A10 evidence, not claimed as new A12 proof here.

## Failures and tooling

- The local rich-projection addition initially used a temporary current-thread runtime for synchronous callers. `Handle::block_on` did not drive its I/O: catalog tests stalled. `A12-workspace-composed.log` records the 200-second supervisor timeout. Process inspection found no remaining owned Cargo/catalog process, and `/tmp/a12-workspace-composed.RSc5nJ` was removed. Fixed the sync-only fallback to a one-worker runtime; final combined batch passed. No unchanged timing retry.
- An additive semaphore `AcquireError` initially failed conversion to IpcError; fixed to typed MACHINE_SERVICE_UNAVAILABLE in the owned event module.
- LSP requested before builds on all six Rust files. Individual successful responses reported no errors; multiple final parallel requests were cancelled by the server. This is not an all-files clean-LSP claim. Compiler warnings are retained, including concurrent foreign A13 warnings in the composed log.
- `git diff --ignore-submodules=all --check` passed. Plain initial `git status` failed because the inherited Ghostty submodule path is a symlink; subsequent commands used ignore-submodules, without changing that link.

## Isolation and cleanup

Every Cargo execution used fresh private HOME, FERRYX_RUNTIME_DIR, FERRYX_DATA_DIR, FERRYX_SESSION_DIR, XDG_CONFIG_HOME, XDG_DATA_HOME, XDG_CACHE_HOME and TMPDIR before library initialization. Normal `/Users/indo/.cargo` and `/Users/indo/.rustup` were explicit. Shared private worktree target, jobs2, dev/test debug0, incremental0 and empty RUSTC_WRAPPER were retained. Inherited FERRYX agent socket/session variables were unset.

Final supervisor `/tmp/a12-workspace-green.LUeUXD` was removed; the log records listener join/refusal, fixture root removal, and owned PTY close/reap. Earlier first-check/race/watch/payload/lifecycle/final supervisors were likewise removed with receipts in their logs. No canonical daemon, user PTY, desktop, remote host, deployment, release, or repository commit was used. Disposable fixture Git repositories contain test-only base commits. All source remains uncommitted and subject to shared-worktree changes.

## Exact source identity and delta

All paths below are relative to src-tauri. Final SHA-256:

| File | SHA-256 |
|---|---|
| src/remote/machine_events.rs | bfaf255d78a3de9a283ab3b027df3b9056ea334aa599c62da0f344bda888b0a9 |
| src/remote/workspace_api.rs | 96ce6e383629f2115d8d535b48e5624951d587db33fdcd58afc72009a2776d48 |
| src/remote/workspace_api/worktrees.rs | 6ac6e1708abebd6de6e951bb63029a0f002fb5e5b3b3d15e11daf723d12d5a3f |
| src/daemon/workspace_service.rs | 6768523801f141e4174b5764446c54f4bf934b54c3a99a8c0186d43572e94ab5 |
| src/daemon/workspace_watcher.rs | 88447fc3a4f63ab2c6af364b2f18e392176d956d12cce9b2bd4fddac8ccecc49 |
| tests/machine_events.rs | ebe217669ee03b0cc1a368055d4f7e50b1385f6b1fd71cb1cbbd220db13f1df5 |

Initial observed hashes: machine_events 661480c1e25599e137103908bf1d58bf94f1966acb0f7275651caabf7613445a; workspace_api f48c2a8f5050aa53be62b130dfc731cb3377f27aaa6b196c0442ceba8ac27db6; workspace_service 434d2541a32acf3402cbc6a06ec93cb2b0907cf9fa4be49dd0596ef86fbc3dbd; event test d007d0eb6c8c25edc5afb5f5418414387b7ca229f08c4d784b42537aa8e3315d. A baseline worktrees hash was not captured. Exact service before/after inherited diffs and their delta are archived as A12-workspace-service-{before,after,delta}.patch; full before/after patches for inherited untracked files were not archived. Their edit history is in the task transcript. Do not attribute the entire inherited untracked files to this lane.

## Remaining coordinated metadata boundary

No writes to daemon/server.rs, daemon/client.rs, daemon/protocol.rs, lib.rs, daemon/session_service.rs, machine_owner.rs, journal/session_api, terminal, relay, Cargo/dependencies or UI.

Parent findings remain binding: parse_agent_state_report validates state/resume-plan syntax, not provider transcript ownership; AgentStateHub has no authoritative provider claim here. Terminal output pump has no title/CWD feed. machine_detail returns stored journal cwd/provider plus live dimensions/sequences. The later owner-authoritative metadata lane must validate provider ownership and publish title/CWD/provider changes through sessionMetadataChanged, carrying exact routed owner epoch and an authoritative session revision. This lane does not invent those values, and the compatibility session lifecycle publication API remains intact. machineEventsV1 was not enabled. No full A12, A13+, aggregate, platform or release acceptance is asserted.
