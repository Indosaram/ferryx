# Ferryx zero-config remote sync — final remediation audit verdict

Date: 2026-09-10  
Branch: `remote-connectivity`  
Reviewed HEAD: `496cad7bd359c9d862e484407ba93ce4a946d02b`  
Original runtime baseline: `818b68db634d4ebcd3f8c9db4a951e0cdcdf218f`  
Original findings: `docs/audits/FERRYX_ZERO_CONFIG_SYNC_FINAL_REVIEW_2026-09-10.md`

## VERDICT: REQUEST CHANGES — NOT APPROVED

**Do not approve this revision as completing F01–F10 or as ready for unrestricted public zero-config remote access.** The remediation substantially improves the implementation, and all three requested verification commands pass. Nevertheless, production GUI/CLI pairing ownership remains disconnected; socket-ticket issuance accepts a bearer that was never issued to a device; ownership persistence is not safe across independent relay instances; old control-generation stream capabilities remain usable; and direct-path credential release is not bound to a verified machine key/channel. Shared-origin response policy and public admission remain incomplete. The live receipt does not establish the claimed complete daemon/browser workflow.

This verdict does **not** mean every original defect remains unchanged. Ordinary single-instance restart ownership, private first-enrollment gating, actual gateway capability exchange, backend WebSocket bearer forwarding, simple host-scoped terminal routing, and HTML/cookie response hardening are real improvements. The matrix distinguishes these verified repairs from the remaining acceptance requirements. Findings overlap; ten incomplete closure decisions are not ten newly demonstrated exploits.

## 1. Scope and evidence

Git history contains all six submitted commits: `7ad983e`, `8d37f24`, `f4777fd`, `eba8935`, `e9f0983`, and `496cad7`. The original audit was committed at `a405b34`; remediation diffs were inspected against that commit, with the original runtime baseline also checked. Review covered the current relay implementation, related tests, identity/authentication, protocol, coordinator/client, gateway, GUI IPC, daemon pairing handler, CLI, browser application/terminal/store/direct helpers, settings, and the complete live receipt.

Evidence below distinguishes **source inspection**, **executed existing tests**, **independent loopback observations**, and **unverified deployment claims**. Source locations refer to reviewed HEAD, not the older line numbers in the original report. No production relay probing, credential-validity testing, deployment, or service restart was performed. The new observation probes use synthetic identities, temporary ownership files and ephemeral loopback listeners. They do not execute browser scripts or erase browser storage.

The two audit-authored files are this verdict and `docs/audits/zero_config_remediation_observations_2026-09-10.rs`. Product source, existing tests, the original review and the submitted receipt are unchanged. The temporary integration-test entry point was removed. Final raw Git status also lists `docs/plans/FERRYX_REMOTE_CONNECTIVITY_FULL_REVIEW.md`, `src-tauri/target`, `ui/dist` and `ui/node_modules` as untracked entries. The review-plan file was not authored or modified by this audit; build/dependency entries were not cleaned. Their pre-audit provenance was not established. The two-file authorship statement is not a claim of a globally clean worktree; the no-path status helper's empty result is not sufficient for that claim.

## 2. F01–F10 closure matrix

| Finding | Verified improvement | Final disposition / remaining release gate |
| --- | --- | --- |
| F01 — Zero-env startup and daemon ownership | Relay-mode startup now uses `with_identity`, actual bound loopback address and shared gateway auth; the no-machine-token startup regression passes. | **PARTIAL / BLOCKING.** GUI PINs are not relay registrations. CLI is a competing relay owner, and its identity path still differs under `FERRYX_DATA_DIR`. |
| F02 — End-to-end pairing contract | Relay accepts `code`, PIN and capability forms; capability is installed in AuthManager; real gateway exchange succeeds; browser creates/selects hosts and processes second-host QR links. | **CORE CONTRACT FIXED; END-TO-END CLOSURE BLOCKED.** Returned identity is loaded independently of the active relay identity; full GUI/CLI workflow and authenticated identity binding remain incomplete. |
| F03 — Ownership and private enrollment | Durable ownership blocks ordinary restart takeover; unauthorized first enrollment in private mode is gated; protocol/machine fields are signed. | **PARTIAL / BLOCKING.** Two instances overwrite shared ownership snapshots; identity consistency and private-file creation are unchecked; actual relay audience is not bound. |
| F04 — Socket authorization | Missing bearer rejected; bearer is stored with ticket and forwarded into the inner WebSocket handshake. | **PARTIAL / BLOCKING.** Syntactically valid unissued bearer still receives a ticket; device/permission/control-generation authorization and fair admission are absent at issuance. |
| F05 — Terminal transport | Actual terminal component receives host prefix; ticket and grid query reach the relay path; reverse adapter forwards options. | **PARTIAL / BLOCKING.** Gateway-supported `host::session` targets receive 400; options are not ticket-bound; real authenticated grid/event lifecycle remains unverified. |
| F06 — Shared-origin isolation | Cookies and service-worker allowance stripped; HTML becomes plain text; CSP and nosniff imposed; coarse path filtering added. | **PARTIAL / BLOCKING.** Method/path and response contracts remain open-ended; redirects/cache policy/other origin-affecting headers remain daemon-controlled. |
| F07 — Public admission | Five active registrations per machine; matching a known PIN no longer resets the source's failure count. | **PARTIAL / BLOCKING.** One-character, day-long capabilities still register; no fleet/prefix/aggregate registry budget or tenant-fair ticket/session admission. |
| F08 — Generation and lifecycle | Local generation guards ACK and expiry; abandoned ACK waiter is released; local regeneration after expiry/consumption is tested. | **PARTIAL / BLOCKING.** Relay does not bind records/roles to control ownership; old-generation raw halves still attach; idempotent registration, delivery recovery and coherent reconnect/cancel are incomplete. |
| F09 — Direct trust and settings | Probe omits credentials and disallows redirects; known-host machine-ID mismatch prevents upgrade. | **PARTIAL / BLOCKING.** Public string equality is not machine-key/channel authentication; actual health response lacks machineId; legacy query tokens and non-authoritative settings remain. |
| F10 — Release assurance | 25 relay tests, 134 UI tests and production UI build pass; new actual-gateway pairing test and real-terminal UI assertion improve coverage. | **OPEN / BLOCKING.** Receipt is not version-bound full E2E evidence; seven residual behaviors reproduced; broader Rust diagnostic and its isolated failing test are not green. |

## 3. Detailed remaining findings and closure requirements

### F01 — High: shared daemon pairing authority is still not the production entry point

**Source:** `src-tauri/src/remote/server.rs:1855-1867,1910-1928`; `src-tauri/src/ipc/remote.rs:247-267`; `src-tauri/src/daemon/server.rs:1713-1717`; `src-tauri/src/main.rs:234-267,317-337`; `src-tauri/src/remote/auth.rs:22-35`.

Normal Relay-mode startup now establishes an identity-backed, long-lived client without `FERRYX_MACHINE_TOKEN`, shares AuthManager, and uses the actual bound gateway address. This closes the former empty-zip/no-outbound-task defect. The new startup test passes, and Off-mode listener prevention remains intact.

However, startup moves the client into its run task without retaining its coordinator for GUI/daemon IPC. GUI pairing still calls the old `create_pairing_code` and returns only a code plus a fixed sixty seconds. The relay never receives that registration. The production `pairing_coordinator()` consumer remains the CLI, which opens its own control connection, assumes `config.port` is the bound port, and aborts its relay task when the pairing window ends. Sharing an identity then creates competing control owners rather than one daemon-owned pairing operation; daemon reconnect can replace the CLI connection and interrupt its flow.

The claimed universal identity-path unification is incomplete. With `FERRYX_DATA_DIR=D`, gateway/startup use `D/identity.json`; CLI `remote_state_dir()` still uses `D/remote/identity.json`. Windows resolution also differs between the new resolver and the existing LOCALAPPDATA-aware CLI resolver. The default Unix HOME path now agrees, but that does not close the override/platform contract.

**Closure:** retain one daemon-owned client/coordinator/identity and actual bound address; route both GUI and CLI through that authority; use one tested cross-platform resolver. Demonstrate fresh no-token GUI and CLI pairing, repeated pairing, and continued authenticated access after the initial window and CLI exit, without competing control connections.

### F02 — High integration gap: successful exchange is not yet consistently identity-bound

**Source:** `src-tauri/src/remote/auth.rs:333-351`; `src-tauri/src/remote/relay_client.rs:72-126`; `src-tauri/src/remote/relay_server.rs:110-118,461-511,814-852,1720-1759`; `src-tauri/src/remote/server.rs:157-166,203-232`; `ui/src/remote/RemoteApp.tsx:272-295,348-371,410-437`; `ui/src/remote/PairingPage.tsx:20-37`.

The original request-shape and unknown-capability failures are repaired: the relay accepts the browser's `code`, the coordinator installs the capability into the actual auth authority, and the real gateway returns a usable device token. The new gateway test validates that token and single-use consumption. QR parsing supports six-digit PINs and 32-hex capabilities; response metadata creates/selects the host, including when another host is active. These repairs should be retained.

The gateway nevertheless loads machine identity from the global filesystem resolver on every exchange, independently of the identity used by the control connection. The F01 override mismatch can therefore route the exchange to the CLI's machine ID but return the daemon's different ID; subsequent browser requests use the returned route, not necessarily the paired control owner. The new real-gateway test supplies relay identity `real-gateway` but checks only that returned `machineId` is nonempty, not that it equals that identity. No machine-key fingerprint/proof is returned and bound by the browser. GUI relay registration remains absent through F01.

**Closure:** source exchange identity from the same daemon-owned immutable identity that registered the relay route, assert exact equality through pairing and subsequent requests, and bind the paired trust record to the intended machine identity/key. Add the actual GUI/CLI/browser chain, rather than treating the working lower-level exchange fixture as full workflow approval.

### F03 — High residual ownership/persistence risk; ordinary restart repair is verified

**Source:** `src-tauri/src/remote/relay_server.rs:189-243,1114-1169`; `src-tauri/src/remote/auth.rs:38-66,77-104,633-654`; `src-tauri/src/remote/relay_client.rs:231-239`; `src-tauri/src/bin/relay.rs:87-103`.

The key store is now loaded on startup, ownership is persisted before acceptance, corrupt/read/write errors fail enrollment, and private first enrollment requires an authorized enrollment token. Existing restart/private-policy tests pass. The former ordinary single-instance restart takeover should not be reported as still reproduced.

Two independently initialized RelayState instances sharing the same key file each retain a startup snapshot, however. **An independent loopback probe enrolled A through one instance, then B through the other; the saved file contained B and had lost A.** The in-memory mutex cannot serialize different instances/processes or reload their changes. This is a concrete shared-store/multi-instance failure; whether the live topology has that precondition is not established. Either enforce single-writer ownership or implement transactional cross-process storage before claiming safe overlapping rollout/restart behavior.

The reused private JSON writer also ignores both permission-setting errors, writes a predictable temporary file before restricting its mode, and has no exclusive first-creation contract. The identity loader merely deserializes an existing record. **A probe confirmed it accepts a mismatched public/private keypair**; that does not let the mismatched pair authenticate, but fails the required identity-integrity check. Crash durability, serialized concurrent first-start and permission-failure acceptance are not established by a happy-path rename or generic write-failure test.

The transcript includes protocol and machine ID, but both live client and server supply the constant audience `relay`. Different relay endpoints therefore share that audience rather than binding proof to the intended relay origin. No cross-relay attack was executed.

**Closure:** enforce/check private creation, keypair consistency and durable serialized ownership; test independent writers and concurrent identity initialization; bind the actual relay audience. Document private enrollment/revocation and rollout constraints instead of relying on an implicit single-process assumption.

### F04 — High: ticket issuance still does not authorize a device

**Source:** `src-tauri/src/remote/relay_server.rs:565-598,624-693,725-739`; `src-tauri/src/remote/server.rs:883-910,949-977`.

The new header requirement checks only that an Authorization value starts with `Bearer ` and contains permitted token characters. It does not ask the owning gateway whether that credential exists, remains unrevoked, or authorizes the target. It then allocates a ticket from the global one-hundred-entry pool.

**Independent reproduction:** with an online synthetic machine but no gateway and no paired device, missing Authorization receives 401, while a never-issued bearer receives 200 and a ticket. An unrelated Origin does not change acceptance. The fix rejects an absent string, not an unauthenticated device.

Randomness, thirty-second expiry, target/machine binding and atomic consumption remain useful. Forwarding the stored bearer into the backend handshake is also a genuine repair. **Gateway authorization is not bypassed:** the actual gateway still validates tokens and subscribes to device-revocation signals. An invalid bearer can allocate relay resources and trigger backend work, but this audit does not claim it can control a terminal. Tickets still lack device/permission/control-generation binding, and issuance/browser upgrade handlers lack an explicit Origin policy and source/device fairness.

**Closure:** authenticate/authorize against the owner before ticket issuance, bind device, machine, control generation, target and negotiated options, and verify invalid/revoked/View-versus-Control behavior through the real gateway. Preserve existing gateway revocation checks and test revocation during a relayed stream. Authentication, Origin checks and session invalidation are separate controls [E1].

### F05 — High integration blocker: supported session IDs still cannot use tickets

**Source:** `ui/src/remote/RemoteApp.tsx:893-905`; `ui/src/remote/RemoteTerminal.tsx:174-197`; `ui/src/remote/remoteClient.ts:13-37`; `src-tauri/src/remote/relay_server.rs:600-605,613-675`; `src-tauri/src/remote/server.rs:173-182,955-962`.

The actual terminal now receives the host-scoped transport base. The new UI regression renders the real terminal component and verifies the host path, ticket and grid dimensions. Relay parsing accepts and forwards `render`, `cols` and `rows`; the original unconditional grid-query rejection is repaired.

The target validator still excludes colon characters, although the gateway explicitly accepts `<host_id>::<session_id>`. **The independent probe received 400 for this gateway-supported target form.** A simple `audit-terminal` fixture does not cover it.

Terminal options are appended after ticket issuance and are not recorded with the ticket, so consumption does not verify that they match the authorized request. Relay parsing only constrains dimensions to `u16`; gateway geometry checks supply additional bounds. This review does not claim an unbounded allocation through invalid geometry. Direct/legacy transport still places permanent credentials in URLs, discussed under F09.

**Closure:** define a consistent, safely encoded session identifier across browser/relay/gateway, bind and validate terminal options, and exercise a real authenticated grid terminal and event stream, including scoped IDs, input/output, permissions, reconnect and revocation. Do not weaken gateway authorization to make transport tests pass.

### F06 — High: shared-origin response authority is only partially constrained

**Source:** `src-tauri/src/remote/relay_server.rs:773-811,927-1028,1030-1041`; `src-tauri/src/remote/server.rs:1728-1761`; `ui/src/state/remoteHostStore.ts:128-149`.

The original HTML/cookie demonstration is materially mitigated: Set-Cookie and Service-Worker-Allowed are removed; HTML Content-Type is rewritten to plain text; CSP `default-src 'none'` and nosniff are imposed. The audit probe confirms these protective headers and stripping on an allowed proxied route. It does not claim the old executable-HTML attack still succeeds unchanged.

The filter is a set of broad path prefixes rather than exact allowed method/path combinations. **A probe forwarded DELETE on an unknown `workspace/audit.js` path, then received the synthetic daemon's 302 status, JavaScript MIME type, Location and public one-day Cache-Control unchanged.** No script was executed; the observation establishes missing response-contract controls, not a demonstrated remaining credential-stealing script exploit. Sensitive responses also lack an enforced no-store policy.

Source inspection identifies another origin-wide control not stripped: `Clear-Site-Data`. The host inventory and device tokens are stored together in localStorage. For compatible browsers processing a secure shared-origin response, a daemon-controlled storage-clearing header can affect that origin's inventory, not only the daemon's path prefix. This consequence is a specification-based inference from the unfiltered header copy and the origin-scoped storage semantics [E2]; browser deletion was not executed. CSP does not make a path prefix a separate storage authority.

The current prefix policy also does not cover the real gateway's health/sessions/devices API and retained `/api/push/...` routes consistently. Neither the receipt nor the relay router establishes a complete trusted frontend/remote-data deployment boundary.

**Closure:** implement exact method/path and response-type contracts, relay-owned redirect/cache/security-header policy, and rejection of origin-affecting daemon headers. Use separate origins for any arbitrary remote document requirement. Test allowed responses and negative routes in a real browser with another host already paired; a rejected unknown path alone does not establish shared-origin isolation.

### F07 — High: per-machine count and no-reset fixes are not a public fleet budget

**Source:** `src-tauri/src/remote/relay_server.rs:141-169,245-284,419-511,565-598,1107-1112`.

Five active registrations per machine and preservation of source failures on a known-code match repair the two specifically changed controls. However, registration still validates only a nonempty capability and an expiry later than the current time. **A one-character capability with a one-day lifetime received a Ready ACK in the independent probe.** The normal coordinator's stronger generation and sixty-second clamp do not constrain other enrolled clients.

Public enrollment has no aggregate machine/key registry bound; five entries per caller-chosen machine does not bound fleet cardinality. There is no prefix/aggregate guess budget or tenant-fair HTTP/ticket/session admission. Shared one-hundred-entry ceilings limit total allocations but remain cross-tenant exhaustion targets, especially with F04's unissued-bearer acceptance. No distributed live guessing or resource-exhaustion attack was run.

ConnectInfo avoids trusting arbitrary forwarded headers, which is good, but a reverse proxy can make unrelated clients share one raw peer-IP bucket. The receipt does not establish an authenticated trusted-ingress client-IP policy or a direct-origin restriction. Short-code entropy and online rate limiting must be designed together; RFC 8628 section 5.1 supplies relevant guidance, not evidence that Ferryx implements OAuth [E3].

**Closure:** enforce capability format/entropy and maximum lease lifetime at the relay, bound registry cardinality and per-owner resources, and test aggregate/prefix and tenant-fair budgets under the actual ingress topology. Retain the repaired no-reset behavior.

### F08 — High: local generation guards do not establish distributed ownership

**Source:** `src-tauri/src/remote/relay_client.rs:40-126,292-343`; `src-tauri/src/remote/protocol.rs:31-53,78-98`; `src-tauri/src/remote/relay_server.rs:84-108,309-371,419-459,814-852,1195-1254`; `src-tauri/src/remote/auth.rs:585-605`.

Local generation increments, matching ACK validation, stale-expiry protection, additional state transitions and abandoned-waiter cleanup are useful changes. Their unit tests pass. The relay nevertheless echoes the optional registration generation rather than enforcing a monotonic owner generation, and its registration/session/ticket records are not coherently bound to the owning control connection.

**An independent probe replaced an authenticated control connection, then attached both raw halves of a previously issued stream and exchanged a frame.** The prerequisite is knowledge of the unpredictable issued ID; no random-guessing claim is made. The existing cleanup generation prevents removal of a replacement map entry, but does not authorize attaching roles or invalidate the old owner's capabilities.

Identical registration replay is still rejected, as independently observed. The relay emits no PairingPinClaimed notification even though the client parses it, so manual test transitions are not evidence of real coordinator claim/consumption. Cancelled/Ready states do not support a general replacement operation, and reconnect does not coherently invalidate or re-advertise Ready registrations. A failed dispatch leaves a claim without client-bound result retrieval/retry semantics. Issuance persists best-effort rather than returning success only after an acknowledged durable transaction.

**Closure:** make one registration record carry immutable identity, original deadline, client attempt and control generation; implement idempotent ACK/cancel/retry/reconnect semantics. Bind raw halves to separate one-use role capabilities and invalidate pending tickets/sessions on owner replacement. Test actual consumption followed by regeneration rather than manually advancing the enum.

### F09 — High: direct identity checks are unauthenticated, while actual health is incompatible

**Source:** `ui/src/remote/RemoteApp.tsx:443-486`; `ui/src/lib/directPathUpgrade.ts:44-62`; `ui/src/remote/remoteClient.ts:13-37`; `ui/src/remote/RemoteTerminal.tsx:174-197`; `src-tauri/src/remote/server.rs:148-152,186-201`; `ui/src/remote/zeroConfigSecurityProbe.test.tsx:118-135`; `ui/src/components/settings/RemoteAccessSection.tsx:23-31,108,172-194,235-240`.

Omitting credentials from the health probe, rejecting redirects and refusing a mismatched machineId are improvements. But the accepted evidence is still a public identifier in arbitrary health JSON, not proof of the paired private key or an encrypted channel bound to that key. An endpoint that repeats the expected ID passes the application check. Legacy pairing with no expected ID still accepts an OK response. Candidate normalization allows both HTTP and HTTPS private addresses; network reachability is not the missing identity proof.

**The real gateway probe confirms its health response contains no machineId at all.** Thus current known-host upgrades fail against the actual gateway while the new test passes against an invented response shape. The matching-ID test explicitly expects a permanent bearer in the direct events URL; it does not verify elimination of query credentials. Direct terminal helpers and gateway query fallback also remain.

Settings is unchanged: the default relay is a placeholder rather than initial configuration; an empty relay value selects LocalNetwork; relay/control readiness fields exist only as optional TypeScript extensions; QR falls back to the old local PIN; and countdown starts a fixed duration at response receipt rather than an authoritative daemon deadline. These do not establish relay registration readiness.

**Closure:** establish a machine-key-bound secure direct channel before releasing credentials, or remain on relay; remove permanent query credentials on every supported path. Supply real daemon readiness/capability/deadline fields and test settings through actual IPC, including outage, delayed ACK, expiry, regeneration and disable. Adding machineId to unauthenticated health JSON alone is not closure.

### F10 — High release-assurance gap: passing subsets and the receipt are insufficient

**Source:** `src-tauri/src/remote/relay_server.rs:1628-1759`; `ui/src/remote/zeroConfigSecurityProbe.test.tsx:1-135`; `docs/audits/FERRYX_ZERO_CONFIG_SYNC_E2E_RECEIPT_2026-09-10.md:1-39`.

The new real-gateway pairing test is an important improvement over fabricated token responses. The new terminal UI test also renders the actual terminal component. However, the relay terminal bridge test still uses an accepting echo WebSocket gateway, and browser fetch/WebSocket behavior in the UI suite remains mocked. Neither proves authenticated real grid input/output, event delivery, revocation or two paired machines sharing an origin. The real-gateway pairing assertion omits route/returned-identity equality. Seven additional observations reproduce residual gaps despite these passing subsets.

The receipt and the broader Rust failure are evaluated separately below. Neither should be hidden or converted into an unsupported assertion about production exploitation. **Closure requires version-bound real daemon/relay/browser and lifecycle evidence, not only a green named subset.**

## 4. Executed verification

`bun run --cwd ui ...` executes the same package scripts as the requested `cd ui` sequence. All results below are actual completed command outputs, not inferred from source or commit messages. Existing compiler warnings were left unchanged.

| Command | Result | Interpretation |
| --- | --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::relay_server` | **PASS — 25 passed, 0 failed.** | Requested relay subset is green. |
| `bun run --cwd ui test src/remote/` | **PASS — 134 passed across 11 files.** | Includes all six submitted security assertions. |
| `bun run --cwd ui build` | **PASS — TypeScript and Vite production build.** | Build succeeds; not a deployed browser test. |
| `cargo test --manifest-path src-tauri/Cargo.toml --test zero_config_remediation_observations` | **7 observations confirmed; 0 failed.** | Passing means the named remaining behavior was observed, not security acceptance. |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::` | **FAIL — 153 passed, 1 failed.** | Additional broader diagnostic; requested relay subset still passes within this run. |
| Exact isolated `remote::tests::security::sockets::ssh_reconnect_safety_web_raw_status_input_probe` with `-- --exact` | **FAIL — 0 passed, 1 failed.** | Same bounded socket timeout also reproduces outside the full suite. |

The broader and isolated failure is at `src/remote/security_socket_tests.rs:142:10`, `bounded socket frame/close: Elapsed(())`. Root cause and attribution to a particular remediation commit were not established. Do not label it a proven concurrency-only flake or erase it because other suites pass. The source-level and loopback-reproduced release blockers stand independently of this timeout.

Initial command evidence: requested relay `7457e614-1f18-4b9a-a098-806fa3fb5345`; UI tests `23b2a479-6b1c-4ddb-9449-70c39a50a3b8`; UI build `e6bf09fe-adc1-4018-a382-a5158eebcee7`; observations `50afdc0b-193e-4b78-bb1f-1df6f1d79dc5`; broader diagnostic `97861711-7f26-4c71-901c-e44c91b5cf0d`; isolated diagnostic `8dac6552-967f-4b82-8e52-b8ba162f6dc4`. The completion artifact separately records final-revision rerun evidence.

### Reproducible observation source

Archive: `docs/audits/zero_config_remediation_observations_2026-09-10.rs`  
SHA-256: `74314a3ca795b20a269026a4639f959765350da9feb8966849043101f491da75`

The seven tests cover: unissued-bearer ticket acceptance plus scoped-target rejection; old-control-generation raw half attachment; weak/excessively long registration plus non-idempotent ACK; inconsistent persisted identity acceptance; independent relay instances overwriting a shared ownership file; response contract gaps alongside verified CSP/cookie hardening; and actual gateway health without machine identity.

To reproduce in an isolated audit checkout, copy the archive to `src-tauri/tests/zero_config_remediation_observations.rs` and run the integration-target command above. This audit used a temporary module entry point loading the same archived source, then removed that entry point. The archive is intentionally outside automatic discovery: its current assertions observe defects. Invert the relevant assertions when turning remediation work into permanent negative security regressions. No production credentials are contained in the archive.

## 5. Live E2E receipt assessment

Receipt timestamp: `2026-09-10T11:15:04.844Z`. The document names the Omaki relay and narrates successful zero-env control authentication, pairing exchange, ticket/WS authentication, shared-origin policy and generation ACK. Its actual output shows one Ready ACK and one exchange HTTP 200, followed by **`proxyStatus: 403`, `csp: null`, `nosniff: null`** (`:20-39`).

That output can be consistent with rejecting an unknown route; it does not show security headers on a permitted proxied response. Consequently it does not substantiate the narrative's F06 claim. It contains no ticket issuance/consumption or authenticated WebSocket application frames supporting F04/F05, and a registration ACK alone does not establish F08 lifecycle behavior.

The displayed exchange body omits `device`, and the recorded pairing capability is not the 32-hex form consumed by the browser parser. These observations do not prove the receipt is fabricated; they prevent treating it as proof of the exact real gateway/browser contract under review. Credential-like PIN/capability/token values remain in the receipt. They are not repeated here and their validity was not tested. The operator should redact them and revoke any that correspond to real device credentials.

Missing evidence includes probe source/command, deployed binary and commit identity, protocol/configuration identity, actual zero-env daemon and GUI/CLI invocation, ingress/trusted-client-IP topology, authenticated post-pair HTTP, events, grid terminal input/output, permission and revocation rejection, two machines sharing the origin, reconnect/restart recovery, and rollout/rollback semantics. No independent production attestation was performed in this audit.

**Receipt disposition: useful claimed transport smoke evidence, not sufficient release E2E acceptance.**

## 6. Required approval gate

First complete the single daemon owner and identity contract across startup, GUI IPC, CLI, pairing response and browser inventory. Preserve the working low-level capability exchange while adding exact route/identity assertions and daemon-derived readiness/deadlines.

Before opening public enrollment, complete ownership-store concurrency/private-creation guarantees, real device authorization at ticket issuance, shared-origin response ownership and fleet/tenant admission. Close generation-bound registration/stream roles and direct-channel trust without weakening existing gateway authorization or revocation.

Finally execute a reproducible real browser/relay/gateway harness with fresh no-token pairing and post-pair authenticated HTTP, events and grid terminal; repeat with two machines, scoped sessions, revocation, expiry, lost ACK/response, owner replacement and restart. Diagnose the broader Rust timeout and produce a redacted, binary/commit-bound deployment receipt. Required negative cases must fail when the forbidden behavior is accepted.

**Audit complete: remediation approval withheld. The remaining release requirements are substantive code and evidence gaps, not missing commit labels.**

## Primary security references

Repository conclusions are supported by the source locations and executed observations above. These references support only the associated security principles.

[E1] OWASP WebSocket Security Cheat Sheet — authentication, explicit Origin validation, session invalidation, and credential-log hygiene: `https://cheatsheetseries.owasp.org/cheatsheets/WebSocket_Security_Cheat_Sheet.html`.

[E2] W3C Web Application Security Working Group, Clear Site Data, editor's draft dated 2023-11-10, sections 3.1 and 4.2 — origin-scoped storage clearing and authenticated-response preconditions. This is a work-in-progress specification, not a claim of identical behavior in every browser: `https://w3c.github.io/webappsec-clear-site-data/`.

[E3] RFC 8628, section 5.1 — user-code entropy and online rate-limit considerations: `https://www.rfc-editor.org/rfc/rfc8628.html`.
