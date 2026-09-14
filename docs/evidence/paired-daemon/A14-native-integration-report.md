# A14 native integration report

Implemented the missing `paired_host_operation` native command, additive daemon
request/success/structured-error variants, connect-only DaemonClient routing,
daemon-owned MachineClient dispatch, and Tauri command registration. The native
socket authority check now runs through the real `crate::ipc::run_blocking`.
No metadata, machine_events, relay/tunnel, producer client/UI implementation,
Cargo dependency/features, or canonical daemon files were edited by this lane.
Paired proxy capability remains OFF. The full user goal remains incomplete.

## Runtime proof

New private test:
`daemon::server::paired_host_operation_tests::native_operations_force_relay_equal_paths_restart`.

The fixture creates a private local DaemonServer Unix socket and two independent
remote DaemonServer workspace/session service stacks, auth stores, identities,
catalogs and journals. Real RelayClient control/data channels connect both
gateways to the real relay router. Pairing uses its actual scoped PIN exchange.
The client receives only relay-qualified host identities, never gateway addresses.
RegisterProject and Projects traverse the additive native daemon operation IPC,
typed authenticated HTTP, relay allowlist/data tunnel, production gateway router,
and actual durable workspace service. Equal filesystem path strings map to
distinct native desktop IDs on the two hosts. These are separate authorities on
one test machine, not two physical computers or OS containers.

All native/gateway/relay fixture owners are stopped, joined and reconstructed;
the relay rebinds the same origin and all owners reopen their private data.
Projects lists match the original mapped records exactly without re-registration
or copying inventory. PIN publication completion provides the restarted control
channel readiness signal. Credentials read from the private native inventory
are absent from serialized project responses.

For late adoption, the fixture subscribes to an exact directory-response event,
holds that real gateway response indefinitely, then issues Forget on another
native IPC connection. The outstanding operation rejects with
PAIRED_HOST_STALE_GENERATION without releasing the response. A subsequent stale
Projects call also rejects, while the second host remains operational. The join
has a five-second bound; no fixed sleeps or polling were introduced.

## Capability and surface limitations

The production gateway still withholds machineWorkspaceV1. Test-only middleware
adds that capability to a successful authenticated capabilities response; it does
not replace authentication, machine identity, project handlers or any service.
This proves real transport/service composition, NOT production feature readiness.
No production capability is enabled by the fixture. The Tauri command function
and generate_handler registration compile in the built library; runtime proof
calls its DaemonClient/native IPC boundary, not a webview/Tauri invoke dispatcher.
No UI automation, desktop interaction, PTY creation, public relay, deployment,
release or commit occurred. Session/worktree lifecycle and downstream proxy
functionality are not claimed by this project-mapping test.

The producer files/reports were consumed and their actual tests rerun. No broad
native-client provenance audit or mutation-timeout reconciliation repair is
claimed: existing inventory IPC timeout/error mapping is still shared by the
new operation path. Producer tests cover HTTP machine mismatch, redirects,
body bounds, pending mutation identity, journal replay digest conflict and
generation cancellation. Further operation-specific identity validation belongs
to the native producer; this lane did not silently add stubs for missing behavior.

## Exact verification

All commands executed in `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`.

- `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib paired_host -- --nocapture`: exit 0, 26 passed, none failed/ignored. Includes all 21 producer tests, old-daemon compatibility, native relay inventory, separate-process restart fixture and the new operation integration. Raw: A14-native-integration-green.log.
- `bun run --cwd ui test src/lib/pairedDaemonProject.test.ts src/lib/pairedDaemonContracts.test.ts src/state/remoteHostStore.test.ts`: exit 0, 110 passed across 3 files, including 19 adapter tests. Raw: A14-native-integration-ui.log. The earlier producer metadata-contract failure is not present in this run.
- `bun run --cwd ui build`: exit 0, tsc and Vite, 1884 modules. Raw: A14-native-integration-ui-build.log.
- `cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay`: exit 0. Raw: A14-native-integration-build.log. Existing compiler warnings remain visible and unsuppressed.
- `src-tauri/target/debug/ferryx-cli --help`: headless executable ran, printed headless usage, exit 1. Raw: A14-native-integration-cli.log. This CLI does not treat --help as success.
- `src-tauri/target/debug/ferryx-relay --help`: executable ran, rejected unsupported --help, exit 2 before creating server state/listeners. Raw: A14-native-integration-relay.log. These are headless entrypoint checks, not passing help commands; real relay serving is verified by the integration test.
- Fresh final LSP diagnostics: no errors in all six changed Rust files. Initial requests for five files timed out/cancelled; these were not represented as clean results. Final fresh calls succeeded.
- Scoped `git -c diff.ignoreSubmodules=all diff --check`: exit 0. Untracked files are additionally compiled by the test/build.

Initial test construction failed compilation (A14-native-integration-first.log),
then failed during cleanup because the long Unix socket pathname prevented the
fixture from reaching the operation test (A14-native-integration-runtime.log).
The fixture now uses a short unique socket name. Focused runtime subsequently
passed (A14-native-integration-runtime2.log), followed by the expanded 26-test
GREEN with event-driven in-flight cancellation. No behavioral RED for a
production adoption bug is claimed by this lane; producer RED logs remain their
own evidence.

## Cleanup

Passing fixture closes its owned JoinSet with a ten-second bound, confirms every
relay/gateway TCP listener refuses connection, and removes its TempDir on the
blocking worker. Restart uses the same shutdown checks. Existing process tests
reported all three child PIDs reaped and UDS refusal, including injected failure.
No persistent process was intentionally started by this lane; ordinary build
artifacts remain. One failed early fixture left a temporary relay-key file at
`/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/.tmppe0IrO/relay.json`;
its existence was confirmed. It is outside the allowed worktree and was not
manually deleted. No credentials or PIN values were printed in evidence.

## Actual changed source and SHA256

These are observed shared-worktree whole-file hashes, not a claim of exclusive
ownership of unrelated pre-existing sections. Also recorded in
A14-native-integration-hashes.sha256.

| File | SHA256 |
| --- | --- |
| src-tauri/src/daemon/protocol.rs | 5916a5a04040ea3b4dd1e7efb2eb7db521156d219e646d3915bf355d37f1fc32 |
| src-tauri/src/daemon/client.rs | 6f260618b6310b329986c164c3c7eb175bbd9481b5dbcfcf8c22519a38895f05 |
| src-tauri/src/daemon/server.rs | 47e13ee08a8089325ca9d4bf253b0f2794b57af96e0223be8ba30ee4d1ee4668 |
| src-tauri/src/ipc/paired_host.rs | 0a996b54d8f34947b6e5973558733d15196f91aa0d00dfd92b294614c7e9d766 |
| src-tauri/src/lib.rs | c5112ebeaa49f49043681203b99b7a3424a71a4cd1222ebd7d5007a7fb1343c4 |
| src-tauri/src/paired_host/native_operation_tests.rs (new) | 61792592d0b0719ecf7e58dcac7b87204e1cf8e08736c5aa48787c5efe54154c |

All lane-created files are these additions plus this report, the source hash
manifest and A14-native-integration-{first,runtime,runtime2,green,build,cli,relay,ui,ui-build}.log.
Every file creation/edit used apply_patch. No fake shell run_blocking wrapper was
used; production filesystem work uses the Rust worker.

## Corrective provenance continuation

The parent identified operation-specific provenance gaps in A14 itself. This
continuation supersedes the earlier statement deferring those checks to the
producer. Only paired_host/client.rs and paired_host/client_tests.rs changed.

Exact production delta: Operation requires machineWorkspaceV1; Worktrees validates
all returned workspace IDs and non-null worktree owner IDs; WorktreeStatus checks
captured workspace and exact worktree identity; CreateWorktree additionally
requires the returned identity to equal the requested identity. Sessions enforces
the optional workspace filter on rows and unavailable IDs. Session validation
checks machine plus internal workspace/worktree ownership. CreateSession checks
captured workspace and exact optional worktree. Completed journal Session outcomes
validate machine and internal ownership; Worktree outcomes validate internal
ownership; Error outcomes validate request ID. Journal reads do not possess an
original workspace request and therefore cannot compare against one. No gateway
epoch fence was added: routed predecessor owner epochs remain authoritative.
Unknown additive capability strings are tolerated but do not satisfy required
capability membership.

Failing-first real HTTP matrix adopted 10 wrong-owner replies before the fix:
worktree listing/creation/status, session creation/list filtering and nested
journal session machine identity. The RED command selected one test and failed
behaviorally (exit 101), not compilation. Raw evidence:
A14-native-integration-provenance-red.log. The matrix also exercises existing
machine rejection. A positive HTTP test proves an epoch-7 session is accepted
behind epoch-1 capabilities alongside an unknown future capability string.
A machine-consumed route test pins the journal capability requirement.

Final command: `cargo test --locked --manifest-path src-tauri/Cargo.toml
--no-default-features --lib paired_host -- --nocapture`: exit 0, 29 passed,
including the real native/relay restart and in-flight generation fixture.
Raw: A14-native-integration-provenance-green.log.
`cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features
--bin ferryx-cli --bin ferryx-relay`: exit 0; raw:
A14-native-integration-provenance-build.log. Both changed files had clean fresh
LSP diagnostics before these validators. Existing warnings remain unsuppressed.
No UI, metadata, tunnel, PTY, production capability or dependency change occurred.
HTTP fixtures abort and await owned listener tasks and close temporary roots on
run_blocking. No sleeps, retries, persistent processes or live credentials added.

Continuation source hashes:
- `src-tauri/src/paired_host/client.rs`: `1a6e5b05d0e00e69cddfb3ff93371225b814db89506a22effc527c4cc52c150e`
- `src-tauri/src/paired_host/client_tests.rs`: `d812a5f2b5348376d705dcc43bc622b32cc45dd07d663eb29de17c78c1cf5fc8`
