# A04 shared headless services - generation 5 final review handoff

Status: parent checks passed; independent current-source review and A04 acceptance remain parent-owned. This evidence-only checkpoint updates the generation-4 request below; it does not approve A04 or A01-A24. No source edits, Cargo runs, desktop launches, daemon launches, or commits were made in this checkpoint.

## Current source identity and parent corrections

The eight SHA-256 values below were recomputed with `shasum -a 256` in `/Users/indo/code/project/orca-lite-wt/herdr-wave1` and match `A04-parent-green-result.json`. Paths are relative to `src-tauri/src/`.

| File | SHA-256 |
| --- | --- |
| `daemon/server.rs` | `5789ef53877df3b97107e1dd0293342de1e595ab7dfacf42095312d9c9b27c91` |
| `daemon/mod.rs` | `f62cac476e47a45f3a6dce95311d194820fd59f15411fe8a18ce54df5328812b` |
| `daemon/workspace_service.rs` | `714a8e4dbceec57aba00aad01bcdc5a23bcaf41284f0b88c75cfa60797930af6` |
| `daemon/session_service.rs` | `1bbd593f77fc575c5b2a717913a30d4db3631df02956ce1e2eb89980c024ac7c` |
| `daemon/a04_shared_services_tests.rs` | `57ed927b9c2fafa157e5f3123fa886b3758b98dc9794be8d37a4b880c67031ba` |
| `remote/state.rs` | `15f3fffbc551b035beb522c7d3db314406cefb9a1f3bbdcb0fd714b4f9aa7af4` |
| `remote/backend.rs` | `9e4c9049421a7cfdcaf0d539c42af5857abc8dd8a768e21fbec3526c34fcd71c` |
| `daemon/remote_ssh_gateway_qa.rs` | `689858c905782aaaead195016d1b38140a6bd673f4bf6bb5cfc5260eae521abc` |

Parent restored the unconditional crate-visible `normalize_process_cwd` compatibility export for the ordinary `ipc/agents.rs` caller and removed the unused `DaemonSessionDetails` import. Parent also explicitly extended test-only scope to fix the existing SSH QA wire mismatch: await the advertised Connected generation, then send typed `RemoteWrite` rather than ignored raw binary input. The output assertion and bounded deadline remain; no production SSH generation fence changed. This eighth source file is part of the review manifest, not an undisclosed production scope extension.

D02 required extraction of domain authority, not repair of an alleged baseline split registry. The baseline already shared handles. The split-authority RED is a controlled alternate gateway, not evidence that the baseline was broken. Machine directory/catalog/session mutation APIs remain unimplemented and capability-gated; sharing services does not complete sections 4-8 of the plan.

## Parent execution results (read, not rerun)

All logs below are preserved under `docs/evidence/paired-daemon/`. Each GREEN log records completed exit code 0 and the listed passing summary, with no ignored tests in these selections. Commands ran in the wave1 worktree with a private target and four jobs. The four exact requested commands are retained in `A04-green-command-request.json`; its null result fields are historical, superseded by `A04-parent-green-result.json` and the completed logs.

| Check | Exit | Passed | Exact log |
| --- | --- | --- | --- |
| Focused shared services / real IPC and HTTP / cleanup | 0 | 4 | `A04-extracted-focused-green.log` |
| Local/session regressions | 0 | 34 | `A04-extracted-session-green.log` |
| `remote::` regressions | 0 | 212 | `A04-extracted-remote-green.log` |
| SSH survival | 0 | 4 | `A04-extracted-ssh-survival-green.log` |
| Legacy handover | 0 | 3 | `A04-extracted-handover-green.log` |
| CLI and relay headless check | 0 | n/a | `A04-extracted-bins-check.log` |
| Actual isolated SSH transport after QA repair | 0 | 1 outer test | `A04-extracted-real-ssh-green.log` |

Exact four core commands (recorded parent executions, not new commands executed here):

```sh
env -u A04_PRIVATE_ROOT -u A04_SPLIT_AUTHORITY -u A04_INJECTION CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite-wt/herdr-wave1/src-tauri/target cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib daemon::server::a04_shared_services_tests:: -- --nocapture
env -u A04_PRIVATE_ROOT -u A04_SPLIT_AUTHORITY -u A04_INJECTION CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite-wt/herdr-wave1/src-tauri/target cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib daemon::server::tests:: -- --nocapture
env -u A04_PRIVATE_ROOT -u A04_SPLIT_AUTHORITY -u A04_INJECTION CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite-wt/herdr-wave1/src-tauri/target cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote::
env -u A04_PRIVATE_ROOT -u A04_SPLIT_AUTHORITY -u A04_INJECTION CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite-wt/herdr-wave1/src-tauri/target cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay
```

The parent manifest records additional selectors exactly as `daemon::server::ssh_survival_tests::`, `daemon::handover::tests::`, and `daemon::server::remote_ssh_tests::direct_ssh_real_transport_registration_and_pty -- --exact --nocapture`. It does not retain complete invocation/environment strings for those three runs; no reconstructed command is presented as an exact execution receipt.

Source chronology matters: focused and remote GREEN ran with server SHA `c35dcaaefa147415733fdeb9e7196fc27e31fc3b4ac0472392edf317b83941b5`, after the compatibility export fix but before unused-import removal. They were not rerun on the final eight-file manifest. The manifest's focused note still says final session regression pending, but its later `localSessions` result and the 34-pass log supersede that note. The SSH repair note also still says execution pending; the later actual SSH GREEN supersedes that historical status. The generation-4 independent report remains a historical NOT ACCEPTED verdict, not a current approval. Unrelated compiler warnings remain visible and unsuppressed.

## RED provenance and failure disclosure

- `A04-red-shared-authority.log` / `A04-parent-red-result.json`: controlled `A04_SPLIT_AUTHORITY=1`, exit 101, `HTTP cannot see original IPC session and workspace`; HTTP200 returned no sessions and registry revisions were IPC 1 / HTTP 0. Parent recorded original owner 83825 and PTY 83858 reaped, listeners joined, root removed and PIDs absent.
- `A04-red-unregistered-g2.log` and `A04-red-cwd-g2.log` / `A04-parent-guard-red-result-g2.json`: actual admission mutations, each exit 101, respectively `unregistered spawn admitted` and `cwd escape admitted`. That manifest preserves the exact common Cargo command, mutated hashes, restoration hash and cleanup receipts. `A04-guards-restored-green.log` is the historical restored baseline exit 0, not extraction GREEN.
- `A04-extracted-real-ssh-failure.log`: exit 101, `actual SSH output through remote WebSocket: Elapsed(())`, caused by the existing QA raw-input mismatch. `A04-ssh-fixture-contract-repair.md` records the parent source comparison against base `a2534ff4` and helper 26061/root `/tmp/fxQ6Bn1C` cleanup. No unmodified-baseline runtime was run to establish that mismatch. The failure remains preserved rather than hidden by the corrected GREEN.

## Post-extraction real headless observations and cleanup

The focused log records one temporary headless server serving actual IPC and authenticated loopback HTTP, without a desktop. The normal scenario owns original daemon PID **14409**, original PTY PID **14482**, session `7a6bc87c-ada3-4715-adec-29d0ee9399c3`, private IPC socket `/private/tmp/a04-gskjP3/fixture.sock`, and port0-assigned HTTP address `127.0.0.1:58869`. IPC metadata and OS process CWD both identify `/private/tmp/a04-gskjP3/project`. HTTP200 lists that same running session in workspace `a04`; IPC and HTTP registry revisions are both 1. HTTP does not itself expose PID/CWD: the fixture correlates its session ID with the original PTY handle and IPC/OS observations.

Arc identity assertions precede the live scenario. The normal child prints session authority `0x1098c9150`, workspace authority `0xacec542e0`, and backend data pointer `0x1098c9150`. Equal backend/session addresses support the assertion of shared authority; revision equality alone is not registry identity proof. Other children print their own addresses, interleaved in the same log. The focused run also passes service-less construction and empty-construction ownership-drop tests; the latter is not a live-session task destruction test.

The same log records IPC/HTTP listener joins, original PTY 14482 reaped, socket removed, child 14409 waited successfully, and private root removed. Normal plus six injected paths (IPC setup, HTTP setup, scenario, cleanup, cancellation, deadline) completed. The cleanup injection deliberately reports one close error and still reaps both original PTYs; this expected asserted error is not a silently swallowed failure.

`A04-parent-green-result.json` independently records all 13 reported focused PIDs absent: 14408, 14425, 14426, 14478, 14409, 14482, 14483, 14522, 14523, 14548, 14581, 14582, 14615. It also records all seven private roots absent: `/private/tmp/a04-wpscMU`, `/private/tmp/a04-gskjP3`, `/private/tmp/a04-NKpAU8`, `/private/tmp/a04-78prHq`, `/private/tmp/a04-uDRtcR`, `/private/tmp/a04-ezTHJ1`, `/private/tmp/a04-JRDoWs`. These are parent observations at teardown, not a fresh PID check susceptible to reuse.

Actual isolated SSH GREEN records helper PID **31418** owned and reaped and both `qa-new-tab` and `qa-split-restore` CWD `/private/tmp/fx99lxCL/remote project's space`. Parent reports helper and roots `/tmp/fxTuXS0W` and `/private/tmp/fx99lxCL` absent. The harness captures the first successful child's stdout and prints it only on failure, so that child's numeric PID receipt is unavailable; outer success establishes its assertions, not an independently visible numeric receipt for every child.

## Remaining limits and checkpoint boundary

Independent current-source review and final A04 approval remain outstanding; parent owns any eventual atomic commit. No whole-plan acceptance is claimed. Windows native-path normalization has a passing test on this macOS run, but Windows compilation/runtime and default/native desktop builds are unverified. No A05 durable catalog/journal, A06 directory API, A09 machine session mutations, UI, deployment or new capabilities are delivered here. The existing unbounded grid queue remains assigned to A10/A23.

Cancellation/deadline fixtures act at the acknowledged-PTY seam, not during the blocking spawn itself. Empty-construction drop proof is not proof of every live async task's lifetime. External SIGKILL/deadlock cleanup is not guaranteed. Current producer work only reads parent evidence and reconciles this document; it launches no owned runtime resources and requires no new teardown. Historical evidence and source are preserved below and in their original files.

---

## Historical generation-4 implementation handoff (preserved verbatim)

# A04 shared headless services - implementation handoff

Status: production extraction exists; final A04 acceptance is pending parent execution and independent review. No whole-plan acceptance or commit is claimed.

## Implementation

- `DaemonWorkspaceService` owns the registry mutation gate and the moved registration validation/canonicalization. IPC registration uses `run_blocking`. Unregistration and validated local spawn share that gate.
- `DaemonSessionService` owns the moved spawn path, provider claim logic, request fingerprints/cache, metadata, close/cleanup, description, and existing SSH session persistence/watch/restore operations. No second PTY manager or registry is created for HTTP.
- Local filesystem/Git probing, startup resolution and PTY spawn execute through `run_blocking`. The owned async spawn lock moves into the blocking operation through metadata/claim publication and lifecycle watcher installation. Cancellation of its awaiter cannot publish a PTY without running that ownership publication. Unregister also takes the spawn lock.
- `DaemonServer` remains the IPC/lifecycle adapter. Its Deref preserves existing internal callers/tests of moved operations; it does not implement a second domain path. The test-only SSH home override remains on the adapter for the existing QA caller.
- Gateway construction installs optional `Arc<MachineServices>` from the same session service, deriving the HTTP backend and registry from that authority. HTTP transport delegates through the existing session router; session metadata projection comes from the service.
- The service has no DaemonServer, gateway, or Tauri AppHandle reference. Its handover reference is weak because handover callbacks retain the gateway. A focused drop test checks service, workspace, gateway, handover and terminal ownership release without a desktop.
- Legacy/test gateway constructors retain `None`. Existing A03 HTTP unavailable responses and empty capabilities remain untouched, including for currently unimplemented APIs with services installed. A03 grants/identity, V06 permission guards, Local/SSH routing and legacy handover remain in their original transports.

## Evidence inherited from parent (not rerun)

The baseline already shared its registry/router/terminal handles. D02's extraction was outstanding, not a baseline split-registry bug. `A04-red-shared-authority.log` records the controlled alternative gateway authority failing visibility (exit 101).

`A04-parent-guard-red-result-g2.json` records actual guard mutations: unregistered admission failed with `unregistered spawn admitted`, and cwd admission failed with `cwd escape admitted` (both exit 101). Both mutations were removed. Its referenced logs are `A04-red-unregistered-g2.log` and `A04-red-cwd-g2.log`.

`A04-guards-restored-green.log` records baseline exit 0, owner PID 638, original PTY PID 683, session `d08882f2-007f-4c7f-a68b-864cf7e9c82e`, IPC socket `/private/tmp/a04-OXW5Lc/fixture.sock`, HTTP `127.0.0.1:52528` status 200, and matching IPC/OS CWD `/private/tmp/a04-OXW5Lc/project`. Registry revisions were both 1. Parent independently reported all PIDs and roots absent.

`A04-parent-cleanup-result-g3.json` and `A04-cleanup-reviewed-g3.log` record exit 0 for normal plus six injected cases on prior fixture SHA `24d8e8f0e61043ec1fe2e8d3984bd26f37c17aa2881717bf959e71436906115d`: IPC setup, HTTP setup, scenario, cleanup, cancellation, deadline. Original-handle retention, error propagation and all three cleanup fixes are retained. These are prerequisite receipts, not post-extraction runtime results.

## Current verification and execution request

`A04-green-command-request.json` pins all seven candidate Rust SHA-256 values and exact commands/log destinations. Parent owns long Cargo execution using its monitor, private target, locked dependencies and four build jobs. No Cargo or daemon process was started by this implementation child.

Executed here: `git diff --check` (exit 0); all seven changed Rust files parsed through `rustfmt --edition 2021 --config skip_children=true --emit stdout` with stdin (exit 0 each, no formatter writes). LSP initially returned no diagnostics for six production files; later fresh requests timed out for session/fixture and the final server request was cancelled. Final session/fixture requests returned no diagnostics. This is not a substitute for compilation.

The fixture now asserts Arc identity for session authority, workspace authority and HTTP backend and prints the addresses. Added pure construction tests check service-less gateway constructors and absence of strong ownership cycles. Existing A03 regression tests cover real HTTP fail-closed responses in the requested `remote::` run. Final focused execution must produce original PID/CWD/IPC/HTTP observations and cleanup receipts for this candidate, followed by independent resource-absence checks.

## Limits and outstanding acceptance

Final extraction GREEN logs, Cargo type/build validation, session/remote regressions and independent candidate review remain pending. No new fixture resources require cleanup from this child. Windows native-path normalization moved intact with its existing test; Windows runtime behavior is unverified. No durable machine catalog/journal, directory API, machine session HTTP mutations, UI or deployment is implemented by A04. A01-A24 remains open.
