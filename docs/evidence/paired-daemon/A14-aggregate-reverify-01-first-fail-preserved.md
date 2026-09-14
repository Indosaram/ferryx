# A14 aggregate review

## Verdict: FAIL - combined implementation incomplete

Independent audit on Darwin arm64, 2026-09-13, in the assigned isolated worktree. This is an actionable acceptance failure, not readiness or full-product completion. Paired proxy capability remains OFF. User-level desktop QA was not run; no OS automation.

### Blocking findings

1. **A14 integration producer is missing.** Only `A14-native-client-report.md` and `A14-ui-adapter-report.md` exist at audit close. There is no third producer file inventory, complete report, or named A14 native-IPC/two-host-relay project test. The composition contract explicitly requires this lane. No substitute A13 pairing/inventory test counts as A14 project integration.
2. **The actual command chain stops at the UI invoke.** `pairedDaemonProject.ts:126` invokes `paired_host_operation`; `ipc/paired_host.rs` has inventory commands only, `lib.rs:1060` registers those only, daemon protocol/client/server have no operation request/response/handler. `MachineClient` callers are its unit tests only. The native integration owner must connect typed OperationRequest/OperationResponse/ClientError through all four boundaries, then exercise two equal-path hosts through the real relay/gateway. This is A14 work, not missing downstream UI mounting or A15 proxy functionality.
3. **Required Rust test command does not compile.** Four errors in `tests/support/machine_input_fixture.rs:39,42`: E0599 (`reqwest::Response::json` unavailable) and consequent E0282 errors. No Rust tests executed. This is an existing/concurrent A10 fixture outside this verifier's writes, not a native-client acceptance regression proved here. Route repair to its owner; do not suppress the test.
4. **Required combined UI tests fail in the recorded isolated environment.** 32 failed / 78 passed: adapter 19 failed, host store 13 failed, contracts 78 passed. Failures are `storage?.getItem is not a function` and `localStorage.clear is not a function`. The clean PATH selects Node v25.8.0 and Bun 1.3.14; the interactive PATH selects Node v22.22.3. The log includes Node's invalid localstorage-file warning; setup uses jsdom but does not replace Node's storage global. This is a runner/environment compatibility failure, not proof the adapter's functional assertions failed. Record a supported explicit Node path in the producer's isolated runner and obtain a reliable single run. No blind retry, test skip, source edit, or failure suppression was performed. The earlier metadata contract failure is not reproduced: all 78 contracts pass in this run.
5. **Runtime coverage is not complete even before integration.** Native `capability_absence_fails_closed` exercises missing managed-worktree capability, not unknown capability strings. Unknown capability rejection appears only in injected UI tests. The native suite checks wrong capability machine identity but does not runtime-test wrong worktree/project ownership or wrong-machine completed journal session outcomes. `map_result` checks direct session machine/epoch identity but passes worktree rows and completed journal outcomes through without equivalent ownership checking. Add boundary-specific runtime assertions and validate applicable captured ownership in native code; UI recursive provenance alone does not prove native provenance.

## Prompt/plan-to-file and evidence coverage

Binding inputs read: A14 composition contract; binding plan architecture D01-D05, HTTP/identity/capability/journal contracts, sections 5-8 and A14 acceptance/verification/limitations. No scope expansion into downstream implementation.

| Requirement | Actual implementation / test inspected | Acceptance assessment |
| --- | --- | --- |
| Native-only credentials, explicit host/generation | client execute -> service capture_operation -> inventory read_verified/capture; current_generation before response | Present statically; runtime blocked this audit |
| Host-qualified HTTPS relay URL, redirects disabled | client http uses stored host_id plus URL segments; inventory production origin policy; no_proxy/Policy::none | Present; real loopback redirect/error/body test source exists, not executed successfully here |
| Wrong machine, stale host | capability identity check; session detail target check; generation cancellation test uses mpsc arrival before Forget and bounded join | Partial real HTTP test source; no two-host operation IPC proof |
| Deadlines/body bounds | 5s connect, 10s reads/body, 40s mutation; 64KiB requests/general responses, 256KiB directory responses | Constants/branches inspected; oversized response test exists; no complete deadline/chunked/request-limit runtime matrix |
| Unknown capabilities | native allowlist membership per operation; UI rejects unknown names | Unknown names cannot grant native authority; runtime unknown-capability transport case missing |
| Reconcile before retry | journal read before mutation, pending blocks POST; completed submits identical digest-checked request; actual catalog replay test | Real service handler test source exists; pending and 202 cases use real HTTP controlled handlers; lost-response native/relay integration missing |
| Native desktop ID mapping | projects.rs -> machine_protocol::desktop_workspace_id SHA256 compact tuple, mapped unavailable IDs | Lossless pure Rust two-host mapping test; UI equal-path test uses injected invoke. Neither is real two-host native relay registration |
| Structured error preservation | ClientError fields; UI safeError | Raw non-JSON body dropped; machine message/details preserved natively, not independently redacted here; no blanket sanitization claim |
| Shared 14-operation contract | Rust enum and TS union/wrappers inspected, reports agree | Native IPC connection absent; Rust operation journal has no explicit capability requirement whereas TS requires machineWorkspaceV1 |
| Proxy remains disabled | daemon client paired_host_capabilities returns pairedDaemonProxyV1 false; server advertises inventory only | Static OFF confirmed, no gate writes by verifier |

Read every source file claimed by the two available producers in full: client.rs, projects.rs, client_tests.rs, projects_tests.rs, mod.rs, service.rs, pairedDaemonProject.ts, pairedDaemonProject.test.ts. Also read inventory.rs/inventory_tests.rs/process_tests.rs, machine_protocol.rs, pairedDaemonContracts.ts, remoteHostStore.ts and the failing machine_input_fixture.rs. Read relevant daemon request/client/handler and command registration sections; reverse searches for MachineClient, adapter and operation command are in A14-aggregate-scope.log. No third producer source set can be certified because its report is absent. A13 process tests prove their stated inventory concern only and were not run as a replacement.

## Exact command results

All commands ran once from the assigned worktree with the complete replacement environment in `A14-aggregate-environment.json`, derived from the A13 integration fixture environment. Each command had a 600-second bound. Independent commands ran in a four-worker pool; Cargo build jobs were limited to two. No fixed sleeps/polling were used. Full stdout/stderr and exit codes are retained, including warnings.

- `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib paired_host::`: exit **101**. Raw: `A14-aggregate-rust.log`.
- `bun run --cwd ui test src/lib/pairedDaemonProject.test.ts src/lib/pairedDaemonContracts.test.ts src/state/remoteHostStore.test.ts`: exit **1**. Raw: `A14-aggregate-ui-tests.log`.
- `bun run --cwd ui build`: exit **0**. Raw: `A14-aggregate-ui-build.log`.
- `cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay`: exit **0**. Raw: `A14-aggregate-headless.log`.
- `git diff --check`: exit **0**. Raw: `A14-aggregate-diff.log`.

Actual new A14 IPC relay integration test: **NOT RUN - no producer report/test name or operation integration exists**. No zero-test run is represented as passing. Headless check and UI build pass, but neither proves runtime command registration, native renderer, relay mutation preservation, or desktop QA. `git diff --check` passes; untracked files require hashes rather than diff coverage. Initial unrestricted git status failed on the existing ghostty symlink/submodule condition; no git repair was attempted.

## Actual producer source inventory and hashes

These are producer-claimed changed files, not verifier source changes. Fresh SHA256 values follow; snapshot JSON includes the additional integration boundary files. The verifier cannot attribute pre-existing shared-worktree changes to an absent producer.

- `src-tauri/src/paired_host/client.rs`: `72ddcbcaf81585165cf2f9c45222b3b180a6e657aad53bb05b1368cb50fee718`
- `src-tauri/src/paired_host/projects.rs`: `0e7efc8dba2f7a7af4aad7959aec3709f6d0927d0f6678fd27b21d1a59e43c55`
- `src-tauri/src/paired_host/client_tests.rs`: `0411633aa276a15b8182def9943656d21618053977b290253c5cde399fff8d2d`
- `src-tauri/src/paired_host/projects_tests.rs`: `7c88ab4f78570c58cd702e6910e3d008b99986c2539a18d175a6d60d8e314be0`
- `src-tauri/src/paired_host/mod.rs`: `2c51a1c627385f9a53835c758820f54a18a99299689b2ec8d79181c246147f60`
- `src-tauri/src/paired_host/service.rs`: `0fbb184ace3ed7d47f4845d6efb0e10b5aae610abfce78fa466f2d51072194c1`
- `ui/src/lib/pairedDaemonProject.ts`: `9bf51ced8bd2db11e002d2667517ead9b985417f1e9dd7027acad93169254a3e`
- `ui/src/lib/pairedDaemonProject.test.ts`: `d98d5ee0f26bcc1517e00b3b900fe7798aa980b298f4372ef00b3aef8b872678`

Before/after source delta: `{}`.

## Cleanup, limitations, and delivered files

The verifier wrote only A14-aggregate reports/raw logs/manifests and an isolated temporary fixture directory under the evidence directory. All five foreground commands completed and were reaped. Rust never reached test execution, so it created no fixture daemon, listener, or PTY in this audit. The owned fixture tree was inspected for sockets and removed; exact entries and existence check are in `A14-aggregate-cleanup.log`. Build products and existing shared caches were retained. No canonical daemon/default data, public relay, real credentials, user desktop, commit, release, deployment, destructive git operation, or other lane's source was intentionally touched.

Filesystem shell execution used a synchronous local `run_blocking` wrapper because no run_blocking tool/executable exists in the supplied toolset. Normal build tooling may update its existing linked dependency caches; those were not deleted. No source edits or targeted patches were performed, so source diagnostics are not claimed as verifier-run. Producer diagnostics are historical evidence only.

All actual verifier artifacts and their SHA256 values are enumerated in `A14-aggregate-artifacts.sha256` (excluding the checksum file itself). The review is complete as FAIL. Required followthrough belongs to implementation/fixture owners: deliver missing native operation integration and report, fix the unrelated headless test compile blocker, pin a working isolated UI runtime, close the real-transport coverage gaps, then obtain a fresh aggregate acceptance run. Missing A15+ functionality is expressly not a fix request from this audit.
