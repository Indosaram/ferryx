# Ferryx zero-config relay sync — Gen5 final verdict

Date: 2026-09-11

Scope: `50d8c951-dffe-460a-8be0-c19cff9f1504`

Submitted remediation: `5cf27265e0c0893e2633c50e6af98d4aa51ccce8`

Observed HEAD: `107c35d641959c58673defedc65dca2fa5146543`, branch `remote-connectivity`

## VERDICT: APPROVE — Gen4 B1, B2 and B3 are closed

**Approve the Gen5 remediation and lift the Gen4 hold on the reviewed zero-config relay pairing integration. No remaining concrete release blocker was identified in this review's scope.** The Gen3 relay-generation and claim-to-dispatch repairs retain their previously credited status.

This approval is supported by independent source inspection, six new positive acceptance regressions using production components, the existing relay-generation regressions, and the focused Rust/frontend suites. It is not inferred merely from the old defect-presence probes failing. It is also not blanket certification of every remote-access path, cross-process persistence, production deployment, or the still-limited F10 evidence.

## B1 — CLOSED: requested View now issues View, not Control

The requested permission now survives the complete issuance path:

| Boundary | Source anchor |
| --- | --- |
| Daemon passes its computed `perm` into the relay coordinator | `src-tauri/src/daemon/server.rs:1906–1909` |
| Coordinator registers the capability with that permission | `src-tauri/src/remote/relay_client.rs:149` |
| Auth record stores `default_permission: permission` | `src-tauri/src/remote/auth.rs:415` |
| Exchange copies the stored permission onto the issued device | `src-tauri/src/remote/auth.rs:474` |

The compatibility methods that default to Control do not override a View request on this path. The GUI entrypoint is daemon-backed at `src-tauri/src/lib.rs:769`; its pairing IPC handler forwards the requested permission to that daemon.

The new `daemon_view_and_control_survive_real_relay_gateway_exchange` regression executes both permissions through the actual daemon request handler, real relay, reverse client, and gateway. For each request it requires HTTP 200, checks the issued device's permission, validates the actual bearer against the daemon's AuthManager, and rejects a second redemption of the PIN. **Both View and Control passed.** This is stronger than a synthetic registration ACK or a client-displayed permission label.

The original Gen4 B1 observation now fails directly at `src-tauri/tests/zero_config_gen4_audit.rs:130`: the issued permission is `"view"`, not the defect observation's expected `"control"`.

## B2 — CLOSED: daemon refusal no longer starts a competing standalone owner

At `src-tauri/src/main.rs:247–280`, the CLI retains the daemon call's `Result`. Success returns the daemon's PIN. An error returns an explanatory failure instead of falling through to standalone pairing. Standalone begins at `:284` and is reached only when the socket-existence branch was not entered.

The new `cli_refusal_exits_without_replacing_owner_and_original_pin_redeems` regression first obtains a Ready PIN from the real daemon coordinator. The actual compiled CLI then receives the real daemon handler's `Ready -> Registering` refusal. The test requires a nonzero child exit, no printed standalone PIN, the explicit daemon-ownership error, and no standalone fallback warning. **After the child exits, the daemon's original PIN still redeems through the real relay/gateway into a valid Control bearer.** This positive redemption establishes survival of the original registration rather than inferring it from a timeout or absence of a log message.

Two additional positive cases passed: a successful CLI request prints the daemon's PIN and that PIN remains redeemable after CLI exit; an absent daemon socket with no configured relay URL fails on the missing URL without creating a daemon socket or other runtime-directory artifacts.

The original Gen4 B2 observation now fails directly at `src-tauri/tests/zero_config_gen4_audit.rs:202`: CLI stdout contains zero PIN characters instead of the six-character standalone PIN required to observe the defect.

This finding's closure does not claim that a socket-existence check is a universal liveness or cross-process ownership protocol. The tested refusal, success, and initially absent-socket paths are the boundaries established here.

## B3 — CLOSED: actual relay stop clears only its own coordinator

`PublishedPairing` and its epoch source are at `src-tauri/src/remote/state.rs:263–270`. Publication occurs at `src-tauri/src/remote/server.rs:2068–2077`, after the fallible startup steps. The owning handle retains the state and epoch.

The real `RemoteServerHandle::stop` at `src-tauri/src/remote/server.rs:1891–1903` aborts its relay task and checks the published epoch while holding the slot's write lock. The comparison at `:1900` prevents an older handle from clearing a newer publication. An owning stop clears its slot rather than leaving a dead coordinator selected.

The new `stopped_relay_then_off_allows_real_daemon_local_view_pairing` regression starts a working relay, calls the real handle's stop, verifies the slot is empty, executes the actual daemon Off transition, and obtains a valid local View bearer. **Passed.**

The new `older_real_handle_stop_preserves_newer_publication_and_live_pin` regression starts two real handles on ephemeral loopback ports, verifies distinct epochs, stops the old handle after the new publication, and proves the newer coordinator remains selected and can issue a remotely redeemable View PIN. Stopping the newer handle then clears its own publication. **Passed.** The old owner is stopped before requesting the new PIN, matching the production handover ordering rather than deliberately keeping two reconnecting owners active.

The permanent epoch unit test added in the submission mirrors the cleanup logic rather than calling the production stop method. The new real-handle regression closes that evidence gap. The original Gen4 B3 observation now fails directly at `src-tauri/tests/zero_config_gen4_audit.rs:145` because the slot is no longer `Some` after stop.

## Independent verification

Rust commands below used `--offline --locked --manifest-path src-tauri/Cargo.toml`. All results are from this review, not copied from the submission.

| Check | Observed result |
| --- | --- |
| `cargo test ... --test zero_config_gen5_regression --test relay_pairing_generation_regression -- --nocapture --test-threads=1` | New Gen5 acceptance: **6 passed, 0 failed**. Existing generation regressions: **2 passed, 0 failed**. Exit 0. |
| `cargo test ... --lib remote::` | **174 passed, 0 failed**. Exit 0. |
| `bun run --cwd ui test src/remote/` | **134 passed, 11 files passed**. Exit 0. |
| `bun run --cwd ui test src/remote/zeroConfigSecurityProbe.test.tsx` | **6 passed**. Exit 0. These six also belong to the full UI suite above. |
| `bun run --cwd ui build` | TypeScript and Vite production build completed; exit 0. |
| `cargo test ... --test zero_config_gen4_audit -- --ignored --nocapture --test-threads=1` | **0 passed, 3 failed; exit 101**, expected for the unchanged defect-presence harness. Each failed at the direct defect assertion described above, not by timeout. |

Representative successful command IDs: Gen5/generation `8656c3d2-fb13-4bfe-a680-f0d43139637e`; Rust remote suite `cfdf7a28-b047-497d-a13d-dce405b531d9`; full UI `5765b0e1-c53c-4732-b1ee-8680570a1484`; focused UI `d640ebe2-8d31-4a6b-b09e-4efdb63d556b`; UI build `f11c0b82-48d9-4591-bd97-1ebccface9ca`. The inverted Gen4 observation command was `fddfaa20-26cc-41bd-b86d-672da0432f4f`. The structured completion record also contains the final revision-fresh positive verification after this report was written.

Initial attempts encountered shared Cargo package/build locks and frontend command-lifetime timeouts. Those attempts are not counted as successful verification or product-defect evidence; subsequent complete runs produced the results above. No unrelated process was killed. Compiler warnings remain; these were not warning-free builds. The submitter's historical red-before-green mutations were not independently repeated.

The new regression file is `src-tauri/tests/zero_config_gen5_regression.rs`. It uses temporary workspace-local stores and runtime sockets, loopback networking, and serialized environment overrides. Its CLI tests invoke the actual built executable. Only the compatible IPC handshake metadata is synthetic, avoiding unrelated daemon-upgrade side effects; pairing requests are forwarded to the real daemon handler. No real user's identity, bearer, terminal, production daemon, or public relay endpoint was used by these acceptance tests.

## Relay deployment claim: source consistency credited, host attestation not claimed

The independently read local `src-tauri/src/remote/relay_server.rs` SHA-256 is:

```text
5213297d80db16c67dc205c69a5d38a9efa40ee94b0000811b75a9b28fbd6a9e
```

It matches the submission's reported on-host source hash. The file also matches `5cf2726` exactly. Gen5 changes do not alter that relay implementation or the relay-referenced `verify_control_challenge` and `write_private_json` helpers imported at `src-tauri/src/remote/relay_server.rs:29`.

There is an important precision distinction: `src-tauri/src/bin/relay.rs:15` imports `ferryx_lib`; this is not a separately declared Cargo library containing only `relay_server.rs` and `auth.rs`. The unchanged reachable relay behavior makes an unchanged optimized relay binary plausible. It is **not**, by itself, proof of either a stale deployment or a verified reproducible build.

This review did not access Omaki, independently hash its installed executable, verify its service state, or compare reproducible before/after release builds. The installed binary hash and active-service claim remain submitter-reported. They are not represented here as independently attested. These Gen5 repairs must also be present in the daemon/CLI/gateway binaries that execute them; replacing only the public relay does not deploy those repairs to the affected machines.

## Operational cleanup and unchanged limitations

The B1 repair controls new issuance. `src-tauri/src/remote/auth.rs:550–563` validates an existing bearer by retrieving its stored device; this patch does not retroactively infer a previously intended View permission. **Any known device that received an unintended Control bearer before the fix should be revoked and re-paired with the intended permission.** This review does not establish that such a production device exists. This is cleanup of the original defect's possible consequences, not a new B4 issuance finding or an additional code-approval gate.

The following disclosed limitations remain outside the repaired findings and are not relabeled as completed work: direct-path browser WebSocket query credentials; process-local rather than serialized cross-process persistence transactions and the stated fsync/permission-error handling limits; the constant `"relay"` signing audience; F10's lack of a real browser engine, two physical machines, and its disclosed mid-stream lifecycle cases; and the absence of mutation coverage proving that `canonical_auth_path` delegates to the tested resolver.

No Windows run, full physical-machine/browser acceptance run, production deployment, or independent reproduction of the unrelated macOS sshd and historical helper-artifact failures is claimed. The scoped tests above do not certify the entire platform.

## Revision and change provenance

The worktree contained **88 pre-existing modified tracked files** at entry. They were preserved. Observed HEAD was `107c35d641959c58673defedc65dca2fa5146543`, not the prompt's `5cf2726`; the intervening committed difference is only 58 added lines in the audit receipt document. Before writing this report, an independent comparison again found no differences from `5cf2726` in the eight targeted production files: daemon/server, main, remote/auth, remote/relay_client, remote/server, remote/state, remote/relay_server, and bin/relay.

Execution occurred in the supplied mutable worktree, not a clean immutable checkout. The production-file equality checks support attribution of the reviewed repairs to `5cf2726`; they do not erase unrelated worktree changes.

Only this new Gen5 report and the new positive regression file were authored by this reviewer. The Gen4 report and its opt-in observation harness were left unchanged. No production-code fix, commit, or deployment was performed.

## Final disposition

**B1 closed. B2 closed. B3 closed. APPROVE the Gen5 relay-pairing remediation at `5cf2726` within the original audit scope.** There is no remaining concrete blocker to name for that approval. Retain the disclosed limitations and deployment boundaries, and handle any known previously mis-issued credentials as operational remediation.
