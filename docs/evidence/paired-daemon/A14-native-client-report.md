# A14 native client lane report

Implemented typed native HTTP and lossless project mapping in the assigned worktree. Full A14/product completion is NOT claimed: integration/UI/relay and withheld downstream capabilities remain separate lanes. Paired proxy capability was not enabled.

## Exported adapter contract

`paired_host::client::{MachineClient, OperationRequest, Operation, OperationResponse, OperationResult, ClientError}`.
Entry: `MachineClient::new().execute(&PairedHostService, OperationRequest).await`.
Request JSON is `{hostId,generation,operation}`; response is `{hostId,generation,result:{kind,data}}`. Epoch uses existing canonical string DTO. Operation enum has all 14 contract kinds. Mutations use existing machine DTOs as `request`; unregister additionally has `workspaceId`, close additionally has `sessionId`. Reads: directories has nullable `path` and `includeHidden`; worktrees has `workspaceId`; worktreeStatus has `workspaceId` and `worktree:{wsId,slug}`; sessions has nullable `workspaceId`; session has `sessionId,daemonEpoch`; operation has `requestId`.

Errors expose `code,machineError,requestId,ambiguous`, preserving structured machine errors, never arbitrary response text. Project results use `projects::{Project,Projects}`: Project flattens all existing remote metadata with replaced desktop `workspaceId`, plus `remoteWorkspaceId,target`. Unavailable project IDs use the identical SHA256 identity domain. Worktree/session results retain machine DTO identity domains for later owner-qualified adapters; no second inventory/cache/store was introduced.

## Security and reconciliation

The existing daemon service captures a durable-read-verified credential lease before network activity and revalidates the generation before adoption. Watch cancellation is selected during HTTP, including body consumption. Authenticated capabilities must match machine ID and machine/Control authority. Operations require their explicit capability; unknown strings confer no authority. HTTPS policy comes from existing authoritative inventory; loopback HTTP exists only in test constructors. URL path segments/query pairs use reqwest URL API, redirects and ambient proxies are disabled. Connect is bounded at 5 seconds, read/whole body at 10 seconds, whole mutation at 40 seconds; request and ordinary response bodies are 64 KiB, directory responses 256 KiB.

Every mutation submission queries the original device-scoped journal first. Pending, unknown, expired, or unavailable reconciliation never triggers a mutation. Completed journal results alone cannot validate a caller digest (the wire journal has no digest); the identical supplied request is submitted for server-side digest-checked replay instead of blindly adopting a possibly unrelated result. IDs and body bytes are not replaced. Actual service test proves changed input with reused ID gets REQUEST_CONFLICT. No auto-spawn, background retry, or second digest journal exists. Pending/undecodable mutation replies retain request identity and ambiguity.

## Verification (Darwin arm64)

- Behavioral RED: `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib paired_host::client::tests::pending_mutation_response` exited 101: a real HTTP 202 pending reply lost mutation identity/ambiguity. Fixed in production response mapping. Raw: `A14-native-client-red.log`.
- GREEN: `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib paired_host::` exited 0: 21 passed, 0 failed/ignored, including 8 new tests. Raw: `A14-native-client-green.log`.
- LSP before build: all 9 paired_host Rust files scanned, zero errors; one pre-existing cfg inactive-code hint in service.rs.
- `cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay` exited 0. Raw: `A14-native-client-headless.log`.
- `git diff --check -- src-tauri/src/paired_host` exited 0. This directory is untracked in the shared baseline, so hashes below, not git diff, inventory delivered source.

New tests: metadata_and_unavailable_identity_are_lossless; real_http_non_json_redirect_and_body_limit; generation_cancels_blocked_http_without_release; ambiguous_journal_never_repeats_mutation; pending_mutation_response_retains_request_and_ambiguity; wrong_authenticated_machine_cannot_be_adopted; capability_absence_fails_closed; real_machine_catalog_replay_checks_digest_and_preserves_metadata.

Runtime fixture exercises real HTTP, native private inventory, and real DaemonWorkspaceService/catalog/journal handlers. The real-service fixture explicitly supplies capability advertisement withheld in production; this is not proof the production gateway enables those capabilities. Existing unrelated compiler warnings remain unsuppressed (17 test, 19 headless warnings).

## Limitations and cleanup

Native daemon IPC through actual relay for two hosts is integration-lane evidence, not exercised here. No UI, renderer, PTY, live credentials, public relay, canonical daemon, release/deployment/commit was used. Session/worktree typed routes compile but full real-service lifecycle acceptance for those operations is not established by this lane's focused runtime suite. No timing sleeps/poll loops were added; cancellation test subscribes before triggering Forget and uses a bounded join. Every successful fixture aborts and awaits its owned listener task and closes its temporary root via run_blocking. Build artifacts remain in the worktree target directory; no source or other-lane artifacts were removed. The command tool has no supplied run_blocking primitive, so filesystem shell execution was wrapped in a local run_blocking function; Rust filesystem operations use the actual crate::ipc::run_blocking worker.

## Actual changed source and SHA256

- `src-tauri/src/paired_host/client.rs`: `72ddcbcaf81585165cf2f9c45222b3b180a6e657aad53bb05b1368cb50fee718`
- `src-tauri/src/paired_host/projects.rs`: `0e7efc8dba2f7a7af4aad7959aec3709f6d0927d0f6678fd27b21d1a59e43c55`
- `src-tauri/src/paired_host/client_tests.rs`: `0411633aa276a15b8182def9943656d21618053977b290253c5cde399fff8d2d`
- `src-tauri/src/paired_host/projects_tests.rs`: `7c88ab4f78570c58cd702e6910e3d008b99986c2539a18d175a6d60d8e314be0`
- `src-tauri/src/paired_host/mod.rs`: `2c51a1c627385f9a53835c758820f54a18a99299689b2ec8d79181c246147f60`
- `src-tauri/src/paired_host/service.rs`: `0fbb184ace3ed7d47f4845d6efb0e10b5aae610abfce78fa466f2d51072194c1`
