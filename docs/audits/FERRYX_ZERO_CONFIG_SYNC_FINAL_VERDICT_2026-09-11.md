# Ferryx zero-config relay sync — final remediation review

Date: 2026-09-11  
Branch: `remote-connectivity`  
Reviewed commit: `30555f24e2d69d13f1470f542812470ac4c26258`  
Scope: `1238864c-0dbe-4fcc-b4f8-3c33ac00033c`

## VERDICT: REQUEST CHANGES — NOT APPROVED

The remediation makes substantial progress, but F02 introduces a concrete multi-machine pairing failure and does not establish the claimed control-generation binding. These are reproduced against real loopback relay sockets, not inferred from missing browser or physical-machine coverage. The original daemon-owned GUI/CLI pairing requirement also remains incomplete. The review is complete; this is not release approval.

The six commits were inspected in the requested order: `6261a9f`, `6fce7b2`, `1da5684`, `7ab68c2`, `cb1812a`, `30555f2`. Both previous final verdicts in `docs/audits/` were read to recover the original acceptance criteria.

## 1. High / release blocker: F02 compares two unrelated generations

**Decisive committed source:** `src-tauri/src/remote/relay_server.rs:700–710`, particularly **line 705**. Registration storage is at `:639–644`; authenticated control validation is at `:593–609`.

```rust
p.registration
    .generation
    .is_none_or(|generation| generation == *current)
```

`p.registration.generation` is the optional caller-supplied pairing-attempt number. It is not the authenticated control generation. `register_pairing` validates the genuine generation supplied by its caller, but then stores the wire registration unchanged and discards that genuine generation as registration metadata.

The production coordinator initializes its own counter to zero at `src-tauri/src/remote/relay_client.rs:53`, increments it at `:81–90`, and sends `Some(generation)` at `:91`. Each coordinator therefore starts with pairing attempt 1. In contrast, `src-tauri/src/remote/relay_server.rs:443` allocates control generations from a relay-wide counter; session allocations also advance that counter (`:497`, `:737`). There is no protocol agreement making these numbers equal.

### Concrete availability attack and ordinary failure

An unrelated client authenticates its own machine on a public relay before another machine's first connection. That consumes a server control generation without requiring the victim's private key, PIN or device token. The victim's genuine first pairing still sends client attempt 1. It receives a Ready registration ACK, but exchange returns 404 because its live server control generation is different.

The audit reproduced exactly this through public loopback endpoints: first machine connected and registered; a different machine connected and registered attempt 1; the second machine received Ready followed by HTTP 404 on exchange. No traffic flood or forged victim identity was needed. Ordinary multiple-machine use produces the same result. A later pairing attempt on an unchanged control channel can also fail when its local attempt number advances.

### Concrete stale-registration bypass

The same expression accepts an omitted generation whenever any control channel for that machine is live. The audit registered with `generation: null`, replaced that owner's control connection, and observed the old capability forwarded through the replacement connection.

It separately registered caller-selected generation 2 while the genuine server control generation was 1. The relay acknowledged Ready; exchange initially returned 404. After replacement with server generation 2, the same old registration became claimable and its capability was forwarded. Merely rejecting `None` will not fix the problem: the supplied number remains unauthenticated control metadata.

**The final probes never re-register the old PIN after replacement.** To synchronize with the replacement channel, they register a different PIN and capability, receive its ACK, and then claim the untouched old record. This excludes an accidental reauthorization by the test's synchronization step. Both stricter probes passed and reproduced the bypass.

The identical-registration branch at `relay_server.rs:616–623` separately compares only machine and pairing token, not control ownership or client attempt. Its narrow lost-ACK repair should not be treated as a complete reconnect lifecycle.

**Evidence boundary:** the forwarding observations used a synthetic gateway responder that deliberately returned 409. They prove claim acceptance and delivery of the old capability to the replacement control channel, not unauthorized access to a real user's gateway or terminal. Issuing a real device token would additionally require the gateway to accept the unexpired capability. No victim key or production credential was used.

### Required repair

Store a mandatory, relay-stamped `control_generation` in `RegisteredPairing`, sourced from the authenticated control handler's generation argument. Keep the client attempt generation separate for ACK correlation. Claim against the stored server generation, with no optional-field bypass, and preserve/revalidate that expected generation when opening the forwarding session. The current snapshot releases the channel lock before claiming, and subsequent forwarding selects the then-current channel; do not leave a replacement race after fixing the field mix-up.

Define retry/reconnect behavior explicitly: a same-owner, same-attempt retry must not extend the original lease, and a superseded control owner must not silently retain claim authority. Add actual first pairings from two coordinators on one relay, repeated pairing on one channel, replacement, omitted/arbitrary generation fields and claim-to-dispatch replacement cases.

## 2. Original F01 remains a product/availability gate, beyond the corrected identity path

The `<FERRYX_DATA_DIR>/remote` identity correction is real and credited. It does not make GUI and CLI pairing use one daemon-owned coordinator.

At `src-tauri/src/ipc/remote.rs:247–267`, the GUI still invokes local pairing-code creation. The daemon handler at `src-tauri/src/daemon/server.rs:1713–1717` calls `self.remote_state.auth_manager.create_pairing_code(perm)`; it does not register that PIN with the relay. The CLI at `src-tauri/src/main.rs:245–252` creates and runs its own RelayClient, and aborts it at `:263`. Sharing the corrected machine identity therefore still allows the CLI and running daemon to compete for the same control owner.

The original acceptance criterion was one daemon-owned client/coordinator with GUI and CLI requesting pairing through it. It was not merely equality of identity filenames. The remaining consequences are unavailable GUI relay pairing and competing-client connection disruption, not a newly demonstrated privilege escalation.

There is also a remaining Windows auth-store mismatch: `src-tauri/src/remote/auth.rs:66–76` (`canonical_auth_path`) still uses HOME/USERPROFILE and does not call `canonical_remote_dir` or honor LOCALAPPDATA. PairingCoordinator uses this auth path (`relay_client.rs:53`), whereas the Windows gateway/CLI state resolution uses LOCALAPPDATA/Ferryx/remote. The new identity resolver test does not compare these auth-store entry points. This is source-verified; no Windows runtime was executed in this review.

Route production GUI/CLI pairing through the retained daemon coordinator and share both its immutable identity and AuthManager. Resolve auth/config/identity paths consistently, rather than only correcting the identity resolver.

## 3. Repairs credited and disclosed residuals weighed

| Area | Disposition |
| --- | --- |
| F09b HTTP credentials | The three changed HTTP paths use Authorization rather than query tokens. Do not carry the old HTTP leak forward as unchanged. |
| F08b raw stream generation | Owner and genuine server control generation are recorded. An independent public-endpoint probe confirmed an old raw data half receives HTTP 410 after control replacement. This narrow finding is closed. |
| F08a registration retry | Same-machine/token retry no longer duplicates an entry, consumes an additional slot or refreshes its stored lease. Reconnect/control-generation semantics remain unresolved as described above. |
| F03 persisted ownership | Sequential stale-snapshot enrollment now merges prior disk records. Invalid public keys are rejected on initial store load. These are real fixes. |
| Earlier F04/F07 | Gateway-consulted cold/stale token admission and the relay-side lease cap are present; the current remote suite passes their regressions. The former unissued-bearer cold-cache fail-open is not reported as surviving unchanged. |
| F10 receipt and harness | The receipt now names the executable test/commit and correctly states its limits. Real relay/client/router/capability/PTY coverage is valuable. |

### Disclosed limit 1: direct WebSocket query credentials

At this commit the surviving locations are `ui/src/remote/remoteClient.ts:36` and `ui/src/lib/terminalTransport/remoteTransport.ts:27` (not the older line numbers in the submission). The former helper covers direct/non-host-scoped sockets, including the events path. The host-scoped relay helper uses a one-use ticket.

This remains a credential-disclosure risk on the direct path where request URLs are logged or otherwise exposed: possession of an unrevoked bearer permits reuse. The standard browser WebSocket constructor has URL/protocol parameters, not an arbitrary-header parameter; that explains the implementation constraint, not a security exemption. Direct-gateway ticket issuance or another appropriate credential exchange is still needed before claiming no permanent URL credentials across the product. This review does not relabel that disclosed direct-path limit as a new host-scoped relay regression.

### Disclosed limit 2: in-process F10 coverage

The lack of a browser engine or two physical machines is not, by itself, the decisive blocker here. The test starts with one fresh relay and one coordinator, so client generation 1 happens to equal server generation 1. It therefore passes while the independently reproduced second-machine case fails. Its gateway response identity assertion also checks only nonempty metadata, while subsequent test routes use the hard-coded fixture identity. Keep the honest scope statement and expand the executable contract; do not weaken gateway checks to obtain a pass.

### Disclosed limit 3: ownership concurrency

Reread/merge under an instance/process-local mutex is not a serialized cross-process read-modify-write transaction, including for two processes on the same host. The sequential lost-update reproduction is fixed; a concurrent-writer guarantee is not established. An enforced exclusive single-writer deployment is a valid way to narrow the supported topology. A shared-writer rollout needs interprocess transaction/locking semantics. This review did not run a concurrent filesystem race or establish the production deployment topology.

Atomic rename alone also does not establish crash durability or checked private-file creation: `auth.rs:698–718` has no fsync and ignores permission-setting errors. These limits should not be described as stronger guarantees than the implementation provides. No local-file privilege escalation was executed.

### Disclosed limit 4: constant audience

Both sides still use `relay` as the signing audience. It provides protocol separation, not endpoint-specific relay audience binding. No cross-relay impersonation was executed in this review; this remains a stated trust/deployment limitation, not evidence that such an attack was reproduced here.

## 4. Verification, provenance and reproducibility

The initial worktree contained 92 pre-existing modified files, including formatting differences that shift line numbers in some Rust files. They were not edited or cleaned by this audit. Source anchors above refer to committed `30555f2` and were checked with commit-specific git inspection. Tests ran in the supplied worktree, not in a clean immutable checkout. In particular, the critical relay_server.rs and auth.rs were not in the initial modified-file set.

At the final provenance check, HEAD had advanced externally to `aed0dcaf82c8114082580261329d65914efe779f`. The two intervening commits, `64ad2ee` and `aed0dca`, modify only the existing E2E receipt, adding deployment and identity-migration notes; their complete diff was inspected. They make no product/test source changes and do not remediate the findings above. Their claimed deployment and single-install identity observations were not independently repeated. The verdict remains scoped to the requested product revision `30555f2`, with this documentation-only HEAD drift explicitly recorded.

Observed commands:

| Command | Actual result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::` | 168 passed, 0 failed; exit 0. |
| `bun run --cwd ui test src/remote/` | 134 passed across 11 files; exit 0. |
| `bun run --cwd ui build` | Exit 0. |
| Audit command below | 4 passed, 0 failed; exit 0. Three named defect observations plus one positive repair control, not four security acceptance passes. |

The first audit-test compilation attempt timed out at the command limit without executing tests. Subsequent runs completed successfully. The final audit source strengthens synchronization by using a different replacement-channel PIN, and all four tests passed again. The initial timeout is not represented as a failed security assertion.

Audit observation source: `src-tauri/tests/zero_config_final_audit.rs`  
SHA-256: `4c6732ef6c5aff67e3ec940287281037ec609cc4427bdfe0f5a923cd118870f9`

The observation tests are deliberately ignored by default so defect-confirming assertions do not masquerade as permanent security acceptance tests. Run explicitly:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test zero_config_final_audit -- --ignored --nocapture --test-threads=1
```

Observed results:

```text
omitted generation -> untouched old claim reached replacement control channel
caller value 2 accepted under control 1 -> untouched old claim became usable after replacement
second machine, first pairing attempt 1 -> Ready ACK followed by HTTP 404
old raw data half after replacement -> HTTP 410
4 passed; 0 failed
```

Command evidence: final stricter audit run `6b94af81-4230-428a-ab4f-1dbb7c2e7c63`; previous successful audit run `0f592c45-16f0-4040-a010-e1fab9ea457b`; Rust rerun `2ba339a0-08f1-43d5-a31c-a7d28e3fabba`; frontend rerun `62c28c4d-5709-45dd-8028-797e0b5e8810`; build rerun `d65590b4-5bd3-40ac-a02b-9b4b51d9628f`. Post-report revision-fresh verification is recorded in the structured completion result.

Only this report and the opt-in audit test were authored. No product code was remediated, no deployment was made, no production server was probed, and no real credentials were used. The submitter's historical red-before-green mutations and live deployment receipts were not independently repeated in this round.

Primary external references used for general principles only: RFC 6750, sections 1.2, 2.3 and 5 (bearer possession and query disclosure), and WHATWG WebSockets, the WebSocket interface/constructor. Repository findings above are supported by the named source lines and local command evidence, not by those external references.

## Final approval condition

Keep the verified repairs. Separate client attempt identifiers from relay-owned control generations, close claim-to-dispatch replacement behavior, and make the real GUI/CLI path use the daemon's single pairing authority. The current 168/134 green suites and accurately scoped receipt do not override the newly reproduced multi-machine denial and stale-registration acceptance. **Release approval remains withheld.**
