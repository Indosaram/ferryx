# A14 aggregate review

## Verdict: FAIL - outer native mutation IPC remains incomplete

Independent reverification after integration landed, Darwin arm64, 2026-09-13. First FAIL preserved verbatim in `A14-aggregate-reverify-01-first-fail-preserved.md`; original evidence is unchanged. Missing integration and Node25 failures are superseded. Full user goal remains incomplete. Paired proxy capability stays OFF. User desktop QA and OS automation were not run.

## Blocking finding

**Native IPC loses mutation ambiguity/request identity and cuts the budget short.** `daemon/client.rs:406-449` routes paired_host_operation through inventory exchange with a 35-second outer timeout, 32 KiB serialized request cap, and write/read/EOF/parse errors converted to ServiceError::unavailable. The operation method uses `?`, invoking `ClientError::from(ServiceError)` in `paired_host/client.rs:113-117`, which sets requestId=null and ambiguous=false. A lost native response after remote commit therefore loses the explicit reconciliation identity. The inner HTTP mutation allows 40 seconds, preceded by capability and journal reads; the outer inventory timeout can abandon a valid in-progress operation earlier. This is a direct static control-flow finding, NOT a claimed runtime reproduction.

Repair belongs to A14 integration: retain captured mutation ID on potentially delivered IPC failures, mark ambiguous outcomes accurately, preserve no-auto-retry/journal reconciliation, and align operation deadlines with the contract. Align the 32 KiB IPC bound with the 64 KiB machine request contract or provide an explicit typed restriction rather than generic unavailable. Add an event-driven bounded native reply-loss test after admission/commit, reconciling the same original ID, plus controlled-time deadline coverage. Existing tests do not exercise this outer failure. This is not missing A15 functionality.

## Combined scope and evidence

All three producer reports now exist and were consumed, including integration provenance continuation. The actual chain exists: captured UI adapter -> registered Tauri paired_host_operation -> DaemonClient -> additive protocol -> daemon dispatch -> MachineClient -> native credential service -> host-qualified relay HTTP -> remote service. Reverse searches and shared tracked diffs are retained. Shared diffs include A13, metadata and worktree work; those are not attributed exclusively to A14. Producer source contents were read; receipts enumerate every claimed file. No source was edited by this verifier.

| Requirement | Actual evidence inspected and executed | Assessment |
| --- | --- | --- |
| Native credentials, captured identity/generation | service capture_operation/read_verified, inventory leases, UI current checks; native integration excludes credentials from responses | Covered for exercised operations |
| Redirect/non-JSON/limits | no_proxy, Policy::none; real_http_non_json_redirect_and_body_limit | Actual HTTP cases pass; not exhaustive chunked/request/deadline coverage |
| Wrong machine/provenance | wrong_authenticated_machine_cannot_be_adopted; response_provenance_is_fenced_over_http | Real HTTP wrong worktree/create/list session/journal machine matrix passes, not every DTO combination |
| Changed generation | generation_cancels_blocked_http_without_release; native integration exact directory-response event before Forget | Pass, blocked response need not release; second host preserved |
| Unknown capabilities | capability_absence_fails_closed; predecessor_epoch_and_additive_capabilities_are_preserved; UI unknown rejection | Unknown names grant no authority; native tolerates additive names, UI rejects unknown negotiation |
| Reconciliation/errors | pending response identity, ambiguous journal no-repeat, real catalog digest replay | HTTP/service tests pass; native reply-loss gap blocks acceptance |
| Desktop ID mapping | projects.rs SHA256 and unavailable IDs; actual two-host native relay register/list/restart | Equal paths map distinctly, restored maps match |
| Actual integration | native_operations_force_relay_equal_paths_restart | Real Unix IPC, relay control/data channels, production gateway/catalog; NOT A13 pairing substitution |
| Capability OFF | native integration asserts pairedDaemonProxyV1=false | Pass; test middleware adds withheld machineWorkspaceV1, not production readiness |

The fixture runs independent authorities on one Mac, not physical hosts. It reconstructs local and remote service owners and relay from private data; registration/listing survive. Readiness uses PIN publication, cancellation uses an exact gateway event, overall exercise bounded at 120 seconds and shutdown at 10 seconds. It does not invoke the webview/Tauri dispatcher, create PTYs, exercise full worktree/session lifecycle or prove the native renderer. No downstream acceptance is claimed.

## Exact command/output evidence

All commands ran from `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`. New raw logs use prefix `A14-aggregate-reverify-01-`. Complete environment is in environment.json; Git isolation correction in fixture-correction.json. Explicit runtime: `/Users/indo/.nvm/versions/node/v22.23.0/bin/node`, v22.23.0; Bun 1.4.0. Four command workers maximum, two Cargo jobs, 600-second command bounds. No sleeps or test weakening.

- `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib paired_host::`: first exit 101, 23 passed/1 failed, `rust.log`. Failure: `real_machine_catalog_replay_checks_digest_and_preserves_metadata`, `assertion failed: project.metadata.git_root.is_none()`. In-worktree TMPDIR allowed Git to discover the enclosing repository for the plain fixture. Added only `GIT_CEILING_DIRECTORIES=<worktree>/.a14v1` to runner environment. One justified corrected execution: exit 0, **24 passed**, `rust-isolated-git.log`. Original failure retained, source/test unchanged.
- `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib daemon::server::paired_host_operation_tests::native_operations_force_relay_equal_paths_restart -- --exact --nocapture`: exit 0, **1 passed**, `integration.log`. Real two-host mapping, restart, credentials, generation and proxy=false assertions execute.
- `bun run --cwd ui test src/lib/pairedDaemonProject.test.ts src/lib/pairedDaemonContracts.test.ts src/state/remoteHostStore.test.ts`: exit 0, **110 passed** (19/78/13), `ui-tests.log`. Single Node22 run; storage tests unchanged.
- `bun run --cwd ui build`: exit 0, TypeScript/Vite, `ui-build.log`.
- `cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay`: exit 0, `headless.log`; existing 19 warnings unsuppressed.
- `git diff --check`: exit 0, `diff.log`. Untracked source covered by compile and hashes, not diff alone.

No new source diagnostics were run for this report-only lane. Producer diagnostics are historical evidence only. No full regression suite, native/default-feature renderer build, Linux QA, public relay, canonical daemon, desktop interaction, deployment, release, commit or user PTY was used.

## Producer-claimed changed source: current SHA256

Whole-file inventory includes existing shared sections, not exclusive A14 attribution:

- `src-tauri/src/paired_host/client.rs`: `1a6e5b05d0e00e69cddfb3ff93371225b814db89506a22effc527c4cc52c150e`
- `src-tauri/src/paired_host/projects.rs`: `0e7efc8dba2f7a7af4aad7959aec3709f6d0927d0f6678fd27b21d1a59e43c55`
- `src-tauri/src/paired_host/client_tests.rs`: `d812a5f2b5348376d705dcc43bc622b32cc45dd07d663eb29de17c78c1cf5fc8`
- `src-tauri/src/paired_host/projects_tests.rs`: `7c88ab4f78570c58cd702e6910e3d008b99986c2539a18d175a6d60d8e314be0`
- `src-tauri/src/paired_host/mod.rs`: `2c51a1c627385f9a53835c758820f54a18a99299689b2ec8d79181c246147f60`
- `src-tauri/src/paired_host/service.rs`: `0fbb184ace3ed7d47f4845d6efb0e10b5aae610abfce78fa466f2d51072194c1`
- `src-tauri/src/paired_host/native_operation_tests.rs`: `61792592d0b0719ecf7e58dcac7b87204e1cf8e08736c5aa48787c5efe54154c`
- `ui/src/lib/pairedDaemonProject.ts`: `9bf51ced8bd2db11e002d2667517ead9b985417f1e9dd7027acad93169254a3e`
- `ui/src/lib/pairedDaemonProject.test.ts`: `d98d5ee0f26bcc1517e00b3b900fe7798aa980b298f4372ef00b3aef8b872678`
- `src-tauri/src/daemon/protocol.rs`: `5916a5a04040ea3b4dd1e7efb2eb7db521156d219e646d3915bf355d37f1fc32`
- `src-tauri/src/daemon/client.rs`: `6f260618b6310b329986c164c3c7eb175bbd9481b5dbcfcf8c22519a38895f05`
- `src-tauri/src/daemon/server.rs`: `47e13ee08a8089325ca9d4bf253b0f2794b57af96e0223be8ba30ee4d1ee4668`
- `src-tauri/src/ipc/paired_host.rs`: `0a996b54d8f34947b6e5973558733d15196f91aa0d00dfd92b294614c7e9d766`
- `src-tauri/src/lib.rs`: `c5112ebeaa49f49043681203b99b7a3424a71a4cd1222ebd7d5007a7fb1343c4`

## Cleanup and delivered artifacts

All foreground commands completed and were reaped. Integration logs confirm owned task joins and TCP listener refusal on restart and final shutdown; its root closes on the Rust run_blocking worker. No sockets remain under `.a14v1`. Ignored Node compilation caches, empty fixture directories and marker files remain: apply_patch rejected an ignored marker deletion (exit 1), so cleanup stopped without a destructive workaround. No persistent child daemon was launched by the selected test set. Existing build/caches are preserved. The older outside-worktree relay-key remnant reported by the producer was not touched or rechecked.

Filesystem shell execution used a synchronous local run_blocking wrapper because the toolset supplies no run_blocking primitive. Every deliberate report/marker change used apply_patch. Normal build/runtime temporary artifacts are retained or self-cleaned by their owning fixtures. Source before/after delta is empty. Metadata and tunnel source were not altered.

Delivered: this updated review, preserved first FAIL, uniquely labelled command logs, environment/results, scope and shared diffs, read receipts, source hashes and cleanup log. `A14-aggregate-reverify-01-artifacts.sha256` enumerates exact report/log artifact hashes excluding itself. Actionable FAIL remains until the outer native mutation delivery contract and real IPC evidence are corrected; no A15+ repair is requested.
