# A13 repeated owner issuance

## Delivered

The existing daemon-owned PairingCoordinator now retires a Ready, Claimed, or Cancelled offer before registering its replacement. Retirement uses existing Expired cleanup while holding the generation write lock: both PIN and pairing-token capabilities are cancelled before publication. Created/Expired/Consumed issuance remains unchanged; concurrent Registering issuance remains refused. Generation increment, exact generation/PIN/machine/status ACK checks, maximum lifetime, stale expiry fencing, and machine-Control requirement are unchanged. No PairingState protocol edit, replacement coordinator, server/client/protocol metadata edit, or paired-host service edit was made.

Actual CLI path: ferryx-cli pair generate --access machine -> private UDS -> DaemonServer request dispatcher -> the same published coordinator -> real ephemeral relay -> HTTP redemption. The fixture never transitions coordinator state. Each mirror and machine scenario redeems its first PIN, then issues and redeems two explicit machine PINs. Forged View/mirror exchange fields do not change machine Control. Already redeemed PINs fail subsequent relay exchange; existing mirror credential remains mirror and cannot browse/create machine sessions.

## RED / GREEN

All log names below have prefix `A13-issuer-repeat-` in this directory.

- `RED-runtime.log`: successful compilation, live local capability handshake, first mirror owner CLI/redemption, then repeated owner CLI fails. This is behavioral RED.
- `delivered-cli.log`: exact process fixture passes, four repeated explicit machine CLI/redemptions succeed, stale PIN requests refused, mirror and machine projections verified, anonymous/revoked capabilities denied. Final fixture PID 69331 reaped with exit 0; sockets removed and runtimes stopped.
- `delivered-coordinator.log`: 5 coordinator tests pass, including replacement invalidation of both previous local credentials, stale expiry, wrong ACK generation, and in-flight registration refusal.
- `legacy.log`: 13 grant tests pass, including legacy persisted mirror defaults, machine View rejection, forged authority protection, and rejected-generation redemption refusal.
- `stale-final.log`: exact stale_registration_stops_being_claimable_after_control_replacement passes.
- `independent-final.log`: exact second_machine_first_pairing_is_not_blocked_by_an_unrelated_connection passes.
- `final-build.log`: headless ferryx-cli build passes. LSP error diagnostics on all three edited Rust files returned none. Focused git diff --check passes.

The A03 fixture's outdated capability expectations were updated after live mismatch observation: local capabilities are exactly machinePairingV1 + pairedHostInventoryV1 (proxy absent); machine HTTP capabilities now include terminalCreateV1. An empty machine session request now correctly reaches enabled service validation (400 INVALID_REQUEST), while mirror stays 403 MACHINE_ACCESS_REQUIRED. No PTY is launched by this owner fixture.

## Remaining full-target blocker (not caused by this fix)

`generation-final.log`: full relay_pairing_generation_regression target reports 6 passed / 2 failed. The two PTY scenarios fail with Protocol(ResetWithoutClosingHandshake) at line 63; the injected-failure scenario consequently misses its expected sentinel. To distinguish causation, only this lane's production renewal block was temporarily removed and the target rerun: `generation-baseline.log` reports 7 passed / 1 failed, with the same ResetWithoutClosingHandshake in a11_joint_machine_relay_runtime. The production fix was restored, and final build, exact stale-generation regressions, real CLI, and coordinator tests rerun green. This is evidence of a pre-existing nondeterministic stream failure, not permission to retry until green. No tests were removed or skipped, and separately owned stream code was not edited. Full-target all-green acceptance remains blocked on that lane.

Earlier failures are retained honestly: baseline direct test-binary launches lacked Cargo's dylib search environment; initial fixture compilation used unavailable reqwest json helpers (fixed with explicit JSON bytes); `generation.log` encountered concurrent metadata E0004 before its owner completed the match arm. None is behavioral RED. Existing compiler warnings remain visible in logs.

## Isolation and cleanup

Reproducible runner: `A13-issuer-repeat-run.sh LABEL COMMAND...`. It uses env -i; private HOME, FERRYX runtime/data/session/agent socket, XDG and TMP trees; explicit toolchain homes; existing private worktree target; jobs2/debug0/incremental0. Every invocation has .monitor (owned PID, exact command, exit, reaped receipt), .exit and .cleanup (private-root removal exit 0). Cargo supplies the dylib environment to the test; the nested daemon test process clears other ambient environment and preserves only that library lookup plus private roots. The outer harness owns the child process group, kills lingering owned children on failure, and reaps its child. Scenario shutdown stops gateway, UDS and relay; process runtime teardown bounds any detached tasks. Failure logs also contain cleanup receipts. No public relay, canonical daemon, desktop launch, user PTY, secrets, commit or release was used.

## Architectural review / limits

Production responsibility is pairing-offer renewal; the new 34-pure-LOC fixture owns repeated CLI redemption. No new production untrusted boundary, enum variant, escape hatch, defensive type check, one-off production helper, parameter bundle, negative name, log boundary, or post-delete query was introduced. The existing four-argument coordinator API was preserved to avoid widening scope. Existing large modules remain: relay_client.rs 897 pure LOC, A03 fixture 287. They exceed the skill ceiling; a structural split was deliberately not performed because this task explicitly confines production work to the existing coordinator and forbids broader pairing redesign. This size debt is disclosed rather than represented as resolved. Source remains uncommitted in the shared worktree; unrelated pre-existing A03 directory fixture additions were preserved.
