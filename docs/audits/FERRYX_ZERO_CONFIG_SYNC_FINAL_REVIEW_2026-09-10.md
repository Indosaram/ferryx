# Ferryx zero-config sync — final code and security audit

Date: 2026-09-10

## Verdict: REQUEST CHANGES

**Do not approve this revision for public zero-config remote access.** The implementation adds useful protocol and transport primitives, but does not connect them into the real daemon/browser workflow. It also leaves exploitable trust-boundary failures: machine ownership is forgotten at relay restart, private allowlisting does not restrict public-key enrollment, socket tickets are unauthenticated, and an enrolled daemon can serve arbitrary same-origin content through the shared relay.

Reviewed HEAD: `818b68db634d4ebcd3f8c9db4a951e0cdcdf218f`. Reviewed the last ten commits, the complete 2,149-line relay implementation and its 21 tests, relevant identity/gateway/coordinator/protocol/CLI/IPC code, browser transport/store/settings/drawer code, the previous R1–R9 report, and the submitted deployment receipt. **None of R1–R9 is fully closed end to end.** Several individual controls are implemented correctly; the closure matrix distinguishes them from unresolved requirements.

This is an audit, not an implementation patch. The deliverables are this report and two archived, executable observation-probe sources. Runtime product source, the original review, and the deployment receipt were not edited. Tests used synthetic keys and ephemeral loopback listeners; no production attacks, credentials, deployments, or service restarts were performed. Severity below describes the reviewed implementation and its stated shared-relay deployment model, not a claim of exploitation in production.

## 1. R1–R9 closure matrix

| Original finding | Implemented improvement | Final status / remaining blocker |
| --- | --- | --- |
| R1 — Machine ownership | Ed25519 key generation, strict signature verification, fresh challenge, same-process wrong-key rejection | **PARTIAL / OPEN.** UUID-to-key ownership is RAM-only; private allowlisting is bypassed by the no-bearer enrollment path; signed data excludes machine ID, audience and protocol version. See F03. |
| R2 — Public PIN admission | Unknown PINs count toward a five-consecutive-failure IP lockout; connection attempts have an IP cap | **PARTIAL / OPEN.** No fleet/prefix/machine registration budget; known-PIN claims reset failures; tickets and HTTP miss common admission; proxy deployment can collapse all clients into one peer bucket. See F07. |
| R3 — Reverse HTTP/WS transport | Real HTTP bytes traverse a data WebSocket, framing is parsed, and a backend WS handshake adapter exists | **PARTIAL / OPEN.** Actual gateway cannot exchange the coordinator secret; backend WS has no device auth; browser/gateway/relay schemas disagree; arbitrary daemon responses violate shared-origin isolation. See F02, F04, F06. |
| R4 — Pairing timing and transactions | State enum, atomic relay claim, ACK wait, timeout and local expiry timer | **PARTIAL / OPEN.** Coordinator is not the gateway's auth authority, has no registration generation or retry binding, cannot regenerate, and is not wired into GUI IPC. See F01, F02, F08. |
| R5 — Tunnel role/session ownership | Target- and machine-bound, expiring, atomically consumed browser ticket | **PARTIAL / OPEN.** Ticket issuance validates no device; ticket lacks device/control-generation binding; raw halves still use session-ID knowledge alone and survive control replacement. See F04, F08. |
| R6 — Credential transport | Host-scoped HTTP and event helper send Authorization; browser-ticket routes reject unknown query fields | **PARTIAL / OPEN.** Main terminal path drops host scope and sends a permanent query token; legacy/direct paths and forwarded HTTP still accept/query tokens; Origin/revocation integration is incomplete. See F04, F05, F09. |
| R7 — Multi-host browser isolation | Persistent `(relay origin, machine ID)` inventory and host-scoped HTTP/events helpers | **PARTIAL / OPEN.** Pairing never creates a new host; pairing a second host is ignored when one is selected; terminal path and geometry are incompatible with relay. See F02, F05. |
| R8 — Direct-path and shared-origin trust | Existing private-address filtering remains; settings gained a countdown and generation guard | **OPEN.** A health 200 is still sufficient to send credentials to an unverified private host; daemon HTML/cookies are forwarded on the shared origin. Settings cosmetics do not close this original security finding. See F06, F09. |
| R9 — CLI ownership/deployment topology | CLI now holds its own authenticated relay client during a pairing window | **PARTIAL / OPEN.** Normal daemon startup does not connect without a legacy token; CLI is not daemon-owned IPC, uses a different identity location, and drops relay access after expiry. Topology and real deployment evidence remain insufficient. See F01, F10. |

## 2. Release-blocking code findings

### F01 — High: normal zero-env startup and GUI pairing are not integrated

**Evidence:** `src-tauri/src/remote/server.rs:1840-1870`, `src-tauri/src/remote/server.rs:1919-1926`, `src-tauri/src/ipc/remote.rs:247-267`, `src-tauri/src/daemon/server.rs:1713-1717`, `src-tauri/src/main.rs:234-267`.

Without `FERRYX_MACHINE_TOKEN`, normal gateway startup generates an identity, logs that challenge-authentication support is awaited, and discards that identity. Relay task creation still uses `relay_url.zip(relay_token)` and `RelayClient::with_gateway`. With no token, the zip is empty: **there is no outbound relay task**, although local gateway startup succeeds. Removing the hard error is not equivalent to implementing autonomous registration.

GUI pairing still invokes the old `AuthManager::create_pairing_code` through daemon IPC and returns only `code` and a fixed `expiresInSeconds: 60`. It neither calls the new coordinator nor advertises that PIN. Searching the production code for `pairing_coordinator` finds its consumer in the CLI, not the daemon/GUI authority.

There are two identity locations: startup passes the Ferryx base directory directly to `load_or_generate_machine_identity`, whereas CLI resolution appends `remote`. Consequently, normal startup and CLI can use different machine IDs for the same installation. The CLI also opens a separate control connection, assumes the configured port is the actual gateway port, and aborts its relay task when the initial pairing window expires. It does not provide permanent remote connectivity after pairing, and a future shared identity would make competing control owners replace each other.

**Fix and acceptance:** Keep one long-lived, daemon-owned identity/relay client/coordinator using the existing canonical remote data-directory resolver and actual bound loopback address. Route GUI and headless CLI requests through that daemon's IPC. Test fresh startup with both legacy token variables unset, acknowledged registration on a local relay, GUI-generated and CLI-generated pairing, and continued authenticated HTTP/events/terminal access after sixty seconds and after the CLI exits. Remote Off must prevent unsolicited exposure; creating identity metadata alone may be local-only.

### F02 — High: pairing cannot complete across the actual browser, relay and gateway

**Evidence:** `src-tauri/src/remote/relay_client.rs:70-102`, `src-tauri/src/remote/relay_server.rs:110-116`, `src-tauri/src/remote/relay_server.rs:717-749`, `src-tauri/src/remote/server.rs:153-222`, `ui/src/remote/RemoteApp.tsx:338-348`, `ui/src/remote/RemoteApp.tsx:386-409`, `ui/src/remote/PairingPage.tsx:20-37`.

The three contracts are incompatible:

| Boundary | Current behavior | Consequence |
| --- | --- | --- |
| Coordinator to gateway authority | Coordinator invents a six-digit PIN and 128-bit capability without registering either in `AuthManager` | Relay Ready does not mean the gateway can issue a device token. |
| Browser to relay | Automatic and manual pairing send `{code, deviceName}`; relay expects `{pin, deviceName}` or `{pairingToken, deviceName}` | The relay sees neither credential and returns 404. |
| Relay to gateway | Relay translates the registered capability into `code` | Real gateway looks up an unknown auth code and returns 400, `Invalid pairing code`. |
| QR to browser | CLI prints a 32-hex-character capability; automatic parser accepts only six decimal digits | Capability QR links are ignored. |
| Pair response to inventory | Gateway returns `{token, device}` without machine identity; client processes only `token` | Pairing does not add a persistent machine record or bind a key fingerprint. |
| Second-machine pairing | `readUrlHints` is false while a host is selected | A new pairing fragment for B is ignored while A is active. |

**Reproduced:** `observes_coordinator_secret_cannot_pair_real_gateway` starts the real gateway router and real `RelayClient`, obtains a coordinator ACK, observes 404 for the browser payload and 400 for the relay's expected PIN payload, and verifies that no device was created. Frontend probes confirm ignored capability links and missing host creation even when a mock response supplies machine metadata. These are contract failures, not speculative network races.

**Fix and acceptance:** Define one typed pairing request/response contract across Rust IPC, relay and TypeScript. Install the registration into the actual daemon auth authority before Ready; make capability exchange consume that same registration. Return authenticated machine identity and display metadata with the device credential, and create/select the host atomically. Support distinct manual PIN and QR-capability inputs, including a new pairing link while another host is selected. Run the real browser parser, public relay and real gateway together; do not fabricate the token response in a relay fixture.

### F03 — Critical: ownership does not survive restart; private allowlist is not private

**Evidence:** `src-tauri/src/remote/auth.rs:10-94`, `src-tauri/src/remote/relay_server.rs:141-191`, `src-tauri/src/remote/relay_server.rs:968-1092`, `src-tauri/src/bin/relay.rs:15-113`.

The Ed25519 primitive is used with strict verification, but the public machine ID is an independent caller-chosen string. The UUID-to-public-key map exists only in `RelayState`. A different key claiming an enrolled ID is rejected while that state exists, then accepted as the first claimant after a restart. A client that subsequently sends its old device bearer to that ID can be routed to the replacement. Random UUID generation does not prevent intentional reuse of a known route identifier.

The no-bearer control path accepts any otherwise valid Ed25519 enrollment even when `machine_tokens` is nonempty. An enterprise allowlist therefore restricts only callers that voluntarily present a legacy token. It does not restrict public-key enrollment. There is no explicit fail-closed private mode in the binary.

The signed transcript is only `nonce:timestamp`; it does not authenticate machine ID, relay audience or protocol version. A fresh nonce limits simple replay, but does not supply those missing bindings. The local identity loader also lacks serialized first-creation, keypair consistency validation, and a checked owner-private creation contract; the shared temporary-path writer is not safe to reuse as an uncoordinated identity initializer. See `src-tauri/src/remote/auth.rs:568-590`.

**Reproduced:** One probe enrolls owner key A, observes rejection of key B for that ID, restarts with a new `RelayState`, and observes B accepted. Another configures a nonempty private token allowlist and successfully enrolls an unrelated public-key machine without a bearer.

**Fix and acceptance:** Use a self-certifying machine ID or durable authenticated UUID/key enrollment, plus a domain-separated transcript bound to identity, relay audience, version and challenge. Define explicit public/private policy before upgrade and test unauthorized Ed25519 enrollment in private mode. Add restart takeover, key rotation, cloned identity, corrupt/inconsistent keypair, concurrent first-start and permission-failure tests. Preserve the already-working same-process wrong-key rejection.

### F04 — High: socket tickets do not authorize a device, and backend WS authentication is omitted

**Evidence:** `src-tauri/src/remote/relay_server.rs:507-559`, `src-tauri/src/remote/relay_server.rs:579-672`, `src-tauri/src/remote/server.rs:872-900`, `src-tauri/src/remote/server.rs:938-964`.

`socket_ticket_handler` extracts state, route and JSON, but no Authorization header or authenticated device. A request naming an online machine and permitted target receives a ticket even when no device has ever paired. The browser does send a bearer to this endpoint; the server ignores it. Anyone knowing an online machine ID can consume the shared 100-ticket budget and trigger backend work without device authorization.

Tickets correctly have random IDs, a thirty-second expiry, machine/target binding and atomic single-use removal. However, their stored record contains no device ID, permission or control generation, so those properties cannot be checked or revoked from this record.

The WS adapter then calls `client_async("ws://localhost{target}", client_io)` without an Authorization header. The actual gateway requires a valid device token before either events or terminal upgrade. A relay-side 101 does not demonstrate backend authorization or a functioning terminal; the connection closes when the backend handshake receives 401.

**Reproduced:** Unauthenticated ticket issuance succeeds. A separate probe proves a valid device can open the real gateway's event socket directly, then supplies the same bearer to relay ticket issuance and observes the relayed browser socket terminate without an application stream. The daemon authorization check is not bypassed; rather, the relay loses the credential and makes the legitimate feature unusable.

**Fix and acceptance:** Validate device authorization through the owning daemon before issuing a purpose-bound ticket, and carry the resulting authority into the backend upgrade. Bind device, machine, control generation, target and negotiated options; revoke tickets and active streams appropriately. Add Origin validation, per-source and per-device admission, and bounded unauthenticated staging. Test missing/invalid/revoked credentials, View versus Control, device revocation during an active real relay stream, reconnect replay and cross-machine/generation reuse. Do not fix this by weakening gateway authentication. OWASP's WebSocket guidance covers authentication, Origin checks, session expiry and log hygiene [E1].

### F05 — High: terminal host routing and wire parameters defeat the new ticket path

**Evidence:** `ui/src/remote/RemoteApp.tsx:838-852`, `ui/src/remote/RemoteTerminal.tsx:175-196`, `ui/src/remote/remoteClient.ts:13-39`, `src-tauri/src/remote/relay_server.rs:536-559`.

The application computes a host-scoped `transportBaseUrl` for HTTP/events but passes unscoped `transport.url` to `RemoteTerminal`. The terminal consequently takes its legacy branch, connects at the relay root, and puts the long-lived device token in the query. It never reaches the intended `/host/{machine_id}/...` ticket flow.

Correcting that prop alone is insufficient. The terminal appends `render=grid`, `cols` and `rows` to its ticket URL. Relay `SocketQuery` has `deny_unknown_fields` and accepts only `ticket`, so the correctly scoped terminal request is rejected with 401. Relaxing that check alone would still not bind/forward grid options into the backend handshake. The target validator also excludes the `::` host-scoped session form understood by the gateway; any supported session-ID encoding must be explicit.

**Reproduced:** A frontend probe shows events receiving the machine prefix while the actual `RemoteApp` terminal prop is only the origin. A loopback socket probe obtains a valid ticket and receives 401 after adding exactly the grid parameters used by `RemoteTerminal`.

**Fix and acceptance:** Give HTTP/events/terminal one host-scoped transport object. Put terminal mode and bounded geometry in the authenticated ticket request and store/forward them with the ticket, or explicitly allow and validate identical bound upgrade parameters. Preserve rejection of credential query keys. Mount the real `RemoteTerminal` with the real relay/gateway in the test; a helper-only ticket URL test cannot establish integration.

### F06 — Critical: arbitrary daemon content is trusted on the shared browser origin

**Evidence:** `src-tauri/src/remote/relay_server.rs:682-712`, `src-tauri/src/remote/relay_server.rs:770-921`, `src-tauri/src/remote/relay_server.rs:924-935`, `ui/src/state/remoteHostStore.ts:141-152`.

The public host HTTP route is an `any` wildcard. It does not restrict method/path combinations to the supported API surface or enforce a response-content contract. The proxy strips hop-by-hop framing headers but otherwise forwards daemon response bodies and headers, including HTML, Set-Cookie and Service-Worker-Allowed.

A malicious enrolled daemon can therefore serve active content through a URL on the same scheme/host/port as the paired browser application. Host path prefixes do not create browser-origin isolation; same-origin storage remains accessible across paths [E2]. Under the stated shared-origin deployment, visiting attacker-controlled HTML can expose credentials for other paired hosts. This does not require the attacker to know a victim's device token in advance.

**Reproduced:** An unauthenticated HTTP request to a synthetic enrolled machine received an inert HTML body, `Content-Type: text/html`, a root-path Set-Cookie, and a root service-worker allowance unchanged. The relay emitted no sandbox CSP. The probe deliberately did not execute script or exfiltrate credentials. External edge response policies and actual frontend hosting were not inspected; they must not be assumed to repair the code's trust boundary.

The router also contains no trusted frontend-serving route, and the wildcard does not cover retained `/api/push/...` routes. HTTP buffering and a thirty-second operation timeout are useful, but they do not establish complete API routing or shared-origin safety. The custom framing parser needs more coverage for ambiguous/truncated/oversized responses and disconnects; no specific request-smuggling exploit is claimed here.

**Fix and acceptance:** Serve trusted relay-owned assets separately from daemon data. Allowlist API methods and paths, enforce response types, strip or tightly control cookies, redirects and service-worker headers, and use appropriate no-store/nosniff/response policies. If arbitrary remote documents are a requirement, isolate them on a genuinely separate origin with no access to paired-host credentials. Test a malicious machine's HTML/JavaScript, redirects, cookie scope, service-worker scope and unknown-route fallback against a browser containing another machine's credentials. Prefer a maintained HTTP framing implementation rather than expanding an ad hoc parser without a clear threat model.

### F07 — High: consecutive failures are not a fleet-wide brute-force or resource budget

**Evidence:** `src-tauri/src/remote/relay_server.rs:195-234`, `src-tauri/src/remote/relay_server.rs:373-459`, `src-tauri/src/remote/relay_server.rs:507-534`, `src-tauri/src/remote/relay_server.rs:1000-1005`, `src-tauri/src/bin/relay.rs:103-111`.

The new limiter is a genuine improvement over daemon-only guessing protection: unknown relay PINs consume a source bucket and five failures lock it. However, `claim_pairing` resets that bucket's failures as soon as a registered PIN/capability is found, before the daemon successfully authenticates the exchange. Because public enrollment and PIN advertisement have no per-machine registration budget, a caller controlling its own registrations can interleave known claims with unrelated guesses. This follows directly from the reset and registration paths; a distributed live guessing attack was not performed.

There is no fleet-wide guess budget, prefix budget, per-machine fairness, or bounded cardinality for machine-key and PIN registries. Registration checks only that expiry is in the future, not that the original lifetime is at most sixty seconds. It accepts any nonempty pairing capability. The production coordinator generates a strong 128-bit value, but the relay accepts weaker or far longer-lived registrations from other enrolled callers. A probe successfully registered a one-character capability for a day. Unknown-PIN result variation also reveals lookup outcomes.

The thirty-attempt connection bucket applies to control and raw data/client endpoints, not uniformly to host HTTP or ticket issuance. The global 100-session and 100-ticket ceilings bound some allocations but are shared exhaustion targets, not fair tenant admission. Map sweeps on requests can add lock contention as public registry cardinality grows.

Source identity needs a deployment-specific design. The binary correctly supplies `ConnectInfo` and ignores spoofed forwarding headers, but raw TCP peer IP behind a local reverse proxy can be the same for every browser and daemon. Under that topology, five guesses from one browser can lock everyone sharing the origin peer, and ordinary data-channel creation shares the thirty-connection budget. The receipt does not establish the actual edge/origin configuration. Cloudflare documents client-IP forwarding behavior, which must be trusted only through an authenticated, restricted ingress path [E3].

**Fix and acceptance:** Separate connection abuse, pairing guesses, registration issuance and authenticated stream budgets. Do not reset guessing allowance through caller-controlled successful registrations. Bound registry count, lifetime and per-owner resources; add prefix and aggregate admission with a documented active-PIN risk budget. Preserve protected state when limiters evict entries and handle deployment restarts explicitly. Test distributed sources, success-interleaved guesses, cardinality exhaustion, one tenant filling all tickets/sessions, and trusted-proxy versus direct-origin peer identity. Short-code entropy and rate limits must be designed together; RFC 8628 section 5.1 is relevant guidance, not a claim that this is an OAuth implementation [E4].

### F08 — High: lifecycle and stream generation guarantees stop at individual primitives

**Evidence:** `src-tauri/src/remote/protocol.rs:29-89`, `src-tauri/src/remote/relay_client.rs:41-102`, `src-tauri/src/remote/relay_client.rs:254-341`, `src-tauri/src/remote/relay_server.rs:84-108`, `src-tauri/src/remote/relay_server.rs:249-371`, `src-tauri/src/remote/relay_server.rs:373-459`, `src-tauri/src/remote/relay_server.rs:1095-1192`.

The coordinator's state enum does not prove linearizable pairing across processes. Registration and ACK identify only PIN and machine; no immutable registration ID, owning control generation or client attempt is carried. Identical registration replay is rejected rather than acknowledged idempotently. A local ACK timeout does not retract a relay registration, and a lost ACK leaves `run_control_session`'s pending registration occupied until a reply or disconnection. Ready registrations are not re-advertised or invalidated coherently on reconnect.

The enum permits entry to Registering only from Created. Once Ready, Expired or Cancelled, the same coordinator cannot generate another PIN. A local expiry task is not a renewable per-registration state machine. The client parses `PairingPinClaimed`, but the relay control implementation sends no such notification; the existing coordinator test advances Claimed and Consumed manually. See `src-tauri/src/remote/relay_client.rs:474-510`.

Relay claim changes Ready to Claimed before network dispatch. A failed gateway response, cancellation or lost result leaves no client-bound retry/result-retrieval contract. Gateway issuance still uses best-effort persistence, not an acknowledged durable transaction. Claims and expiry use supplied wall-clock timestamps rather than the full original monotonic authorization deadline. Registration collision rejection exists, but there is no bounded retry/lease identity to prevent stale messages from interacting with a later reused PIN.

Stream cleanup generations also do not establish owner generations. `WaitingHalf` has no owning machine/control-generation or separate daemon/client capability. Both raw half routes still admit a caller who knows the issued stream ID. Old issued sessions remain attachable after an authenticated control replacement. The ticket registry similarly lacks generation binding. The existing generation guard protects removal of a replacement map entry; it does not authorize the attaching role.

**Reproduced:** Duplicate registration is rejected; a consumed/active coordinator cannot regenerate; and a probe replaces a control connection, then attaches both raw halves of a previously issued session without any role proof and exchanges a frame. This last scenario requires knowledge of an issued unpredictable stream ID; the audit does not claim that random outsiders can guess such IDs.

**Fix and acceptance:** Make the real auth authority own immutable, generation-bound registrations with original deadlines, idempotent ACKs, explicit cancellation/reconnect semantics and bounded client-bound delivery recovery. Serialize expiry, rotation, claim and consumption in one registration record, persisting issuance before durable success. Bind pending streams to machine/control generation and use separate one-use data-role credentials. Test exact expiry boundaries, delayed/lost ACKs, collision/reuse, same-PIN concurrent claims, failure after issuance, generation replacement, old-role attachment and repeated pairing through the same daemon instance.

### F09 — High: direct-path credential release remains unsafe; settings only simulate readiness

**Evidence:** `ui/src/lib/directPathUpgrade.ts:45-139`, `ui/src/remote/RemoteApp.tsx:358-375`, `ui/src/remote/RemoteApp.tsx:417-431`, `ui/src/remote/remoteClient.ts:20-37`, `ui/src/lib/remoteClient.ts:113-119`, `src-tauri/src/remote/server.rs:184-190`, `ui/src/components/settings/RemoteAccessSection.tsx:23-31`, `ui/src/components/settings/RemoteAccessSection.tsx:108-108`, `ui/src/components/settings/RemoteAccessSection.tsx:140-205`, `ui/src/components/settings/RemoteAccessSection.tsx:235-240`, `ui/src/components/settings/RemoteAccessSection.tsx:282-288`.

Direct probing still treats a health 200 from an allowed private address as identity verification. The next request sends the paired device bearer to that address. A private IP can identify a different machine after network change or address reuse; it is not a cryptographic identity. Browser policies can block some direct connections, but they do not make an allowed wrong-host connection safe.

A frontend observation probe supplies a successful health response at a private HTTPS candidate and verifies that the paired host's Authorization bearer is then sent there. Its event connection also includes a permanent query token. The test proves the application credential-release decision, not an actual LAN interception. Require an encrypted authenticated channel tied to the paired machine key before releasing credentials; keep relay fallback when verification is unavailable. Merely signing a plaintext health response does not protect later traffic from an active intermediary.

Long-lived query tokens also remain in the terminal/direct/legacy helpers and in the gateway's extraction fallback. Host HTTP proxying forwards arbitrary queries. Strict ticket-query parsing on two relay WS routes is therefore not closure of R6 across the application.

Settings has useful countdown cleanup and stale-render guards, but its default URL is only an input placeholder. `relayUrl` starts empty and the toggle selects LocalNetwork when empty. The new `relayConnected`, `controlChannelConnected`, `machineId` and pairing capability fields exist only as optional TypeScript extensions, not in the actual IPC response. QR generation falls back to the old local PIN and is not gated by relay registration ACK. The countdown starts a returned duration at receipt time instead of rendering a daemon deadline; IPC always returns sixty seconds. No live status feed makes those optional booleans authoritative.

**Fix and acceptance:** Complete authenticated direct-path identity/channel establishment and remove permanent query credentials from all supported paths. Initialize the intended relay default as configuration, retain explicit Local-only opt-in, and derive readiness/capability/deadline from daemon IPC. Test real settings-to-daemon-to-relay pairing, ACK delay, outage, regeneration, disable during pending generation, deadline expiry, and same private address with the wrong machine key. Continue to describe the TLS-terminating relay as trusted unless actual application-layer end-to-end encryption is implemented.

### F10 — High release-assurance gap: passing fixture tests and the receipt do not establish T09/T10 completion

**Evidence:** `src-tauri/src/remote/relay_server.rs:1395-1506`, `src-tauri/src/remote/relay_server.rs:1536-1703`, `ui/src/remote/RemoteRouting.test.tsx:7-11`, `ui/src/remote/RemoteRouting.test.tsx:55-105`, `docs/audits/FERRYX_ZERO_CONFIG_SYNC_E2E_RECEIPT_2026-09-10.md:1-14`.

There are exactly 21 relay module tests, and they pass. Their actual assertions are narrower than their names suggest. The fleet test checks one IP's consecutive failures. The role-swap test checks missing tickets, a malformed control proof and an unissued ID, not a valid issued data/client role crossover. Pair-exchange success writes a fabricated token response into the data channel. WS success uses an unauthenticated echo gateway. Frontend routing tests replace `RemoteTerminal`, while the terminal-ticket assertion invokes only the URL helper. These choices explain how the tests pass despite the reproduced real-gateway and actual-component integration failures.

The 14-line receipt records a timestamp, a claimed control handshake/registration ACK, and an HTTP 200. It does not include the probe source or deployed binary/commit hash, protocol/build identity, service configuration, authenticated post-pair API call, terminal input/output, event delivery, permission checks, revocation, restart recovery or rollback. Its displayed response uses `success` and `deviceToken`, whereas the actual gateway response is `token` and `device`. A 101 alone also precedes the application's Ed25519 authentication result. The recorded observations may describe a useful transport smoke probe, but do not prove real Ferryx end-to-end synchronization. This is an evidence insufficiency, not an accusation that the receipt was fabricated.

The receipt includes credential-like values that should not be propagated into further audit text. Redact them and revoke any that correspond to real devices; the review did not test their validity. No deployment was verified independently in this task. With RAM-only ownership and routing, multi-process rollout, restart and rollback require explicit semantics; service liveness is not enough.

**Fix and acceptance:** Add a real gateway/relay/browser harness with deterministic readiness barriers and adversarial cases from F01–F09. Make negative tests fail when the forbidden behavior is accepted. Produce a reproducible, redacted staging/live receipt bound to a binary and commit, with fresh no-env pairing, authenticated HTTP/events/grid terminal, two machines sharing one origin, revoked-device rejection and reconnect/restart recovery. Do not certify deployment from an arbitrary HTTP 200.

## 3. T01–T10 and commit-level disposition

| Task / commit | What exists | Completion decision and required gate |
| --- | --- | --- |
| T01 — `a19352b` | Serializable wire structs and transition matrix | **Partial.** Define one deployed protocol including generation, identity binding, authenticated tickets, real exchange schema and retry semantics; test runtime interoperability, not only JSON round trips. |
| T02 — `2c0eda8` | Key generation/persistence and removal of startup hard failure | **Not complete.** Wire `with_identity` into normal startup, unify storage, preserve private policy, and prove an actual no-env relay connection. |
| T03 — `5764081` | Challenge handler, machine registry, source limiter | **Partial / security-blocked.** Durable ownership, private enrollment gating, fair fleet admission and bounded registry lifetime are missing. |
| T04 — `3aee981` | Coordinator and temporary CLI relay owner | **Not complete.** Use daemon/GUI/CLI shared authority, install exchange state in AuthManager, support subsequent registrations and reconnect/retry/cancel. |
| T05 — `f8b7df3` | Reverse HTTP byte transport, framing and ConnectInfo | **Partial / security-blocked.** Real pairing fails; request/response allowlists, shared-origin isolation, full API scope and failure semantics need implementation. |
| T06 — `f83238c` | Ticket primitive and WS-over-tunnel adapter | **Not complete.** Authorize issuance, bind device/generation, authenticate backend handshake, propagate grid options and revoke active streams. |
| T07 — `43ef6f9` | Persisted host inventory, scoped HTTP/events, drawer labels | **Partial.** Implement new-host pairing and second-host flow, use scoped terminal transport, eliminate credential leakage and verify direct identity. |
| T08 — `3178c9c` | QR readability, countdown and stale UI generation guard | **Partial.** Actual default relay, daemon readiness/deadline/capability contract and registration-aware QR generation are absent. |
| T09 — `fbbc5ff` | Five added security tests; 21 total relay tests | **Insufficient.** All 21 pass but miss the reproduced release failures; the requested broader Rust suite is not reliably green. |
| T10 — `818b68d` | Narrative deployment/smoke receipt only | **Not verified.** Require a version-bound real daemon/browser receipt and rollback/restart evidence; do not treat the document's conclusion as a deployment attestation. |

## 4. Executed verification and limitations

All commands ran in the scoped worktree. Existing Rust compiler warnings were left unchanged. Commands using `bun run --cwd ui` execute the same package scripts as the requested `cd ui` sequence.

| Command / observation | Actual result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::` — first run | **Exit 101: 145 passed, 1 failed.** `ssh_reconnect_safety_web_raw_status_input_probe` timed out at `security_socket_tests.rs:142`. |
| Same requested command — second run | **Exit 101: 145 passed, 1 failed.** `scoped_remote_inventory_lists_registered_projects_without_desktop` failed creating a PTY at `remote/tests.rs:623`. The previous socket test passed in this run. |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib remote:: -- --test-threads=1` | **Exit 101: 145 passed, 1 failed.** The SSH raw socket timeout recurred; serialization did not establish a green suite. |
| Exact isolated SSH raw socket test | **Exit 0: 1 passed.** This does not erase its failures in complete runs. |
| Existing relay module | **21 tests**, all passed within the executed full runs. This is primitive/fixture coverage, not proof of real E2E closure. |
| `bun run --cwd ui test src/remote/` | **Exit 0: 128 passed across 10 files.** |
| `bun run --cwd ui build` | **Exit 0.** TypeScript and Vite production build succeeded. |
| Additional settings/direct-path suites | **20 settings + 42 direct-path tests passed.** Their mocks do not validate the absent backend contract. |
| Audit-only Rust observation probes | **8 passed, including a repeat run.** Pass means the documented defect was observed, not that a security requirement passed. |
| Audit-only frontend observation probes | **5 passed.** Pass means the documented application behavior was observed using controlled fetch/socket fixtures. |

Do not attribute the broader Rust failures to a specific new commit without further diagnosis. The changing failures and isolated success suggest test/runtime sensitivity, but neither parallel contention nor an implementation regression was established. The code-level and reproduced zero-config failures above independently require changes even with a stable test environment.

Harness command evidence: first full Rust `c914dfbe-3ce5-4009-b30d-5592f91bb61a`; second full Rust `741dace1-4413-45d1-aed3-dbdd23ec5638`; serial diagnostic `50657a38-157a-4e22-b0f9-e5ca8b23de1c`; isolated SSH `3c6666a8-7252-47d0-9763-1fc90d68831d`; frontend suite `e97ca3f9-01fb-4a4b-b43c-0e0a434fcffe`; frontend build `aaa2608a-c625-4bf7-abf1-6cfc2ea040f7`; repeated Rust probes `6ea21463-2f80-4fe4-90ec-bab35421e9a9`; UI probes/settings/direct tests `f0a54407-f32a-4478-8a83-940f2732e062`.

### Reproducible observation sources

`docs/audits/zero_config_final_audit_probe_2026-09-10.rs` contains eight loopback observations: ownership after restart, public enrollment in private mode, unauthenticated tickets/grid-query rejection, real gateway pairing failure, real backend WS auth failure, shared-origin response forwarding, old-generation raw halves, and excessive registration lifetime/non-idempotent ACK. SHA-256: `3a954ace344206249b2781c028e7935fd0560005195fe24202202b2284c326b5`.

`docs/audits/zero_config_final_audit_probe_2026-09-10.test.tsx` contains five UI observations: ignored capability QR, code/pin mismatch and missing host creation, ignored second-host pairing, unscoped terminal prop, and private-health-triggered credential release. SHA-256: `c2b2512fb543a621f7f8ea0c248b506cba4b58d4c88a54ffce50c3b917fff609`.

These files were temporarily executed at `src-tauri/tests/zero_config_final_audit_probe.rs` and `ui/src/remote/ZeroConfigFinalAudit.probe.test.tsx`, then archived outside automatic test discovery. To reproduce, copy them back to those exact locations in an isolated audit worktree and run the named Cargo integration target and Vitest file. They deliberately assert the observed insecure/broken behavior; invert the assertions and expand coverage when converting them into permanent security regression tests. Do not count their current passing status as remediation.

## 5. Required remediation order and approval gate

First freeze the runtime protocol and daemon ownership model. Fix normal startup, shared identity storage, GUI/CLI authority and real auth-state installation together; otherwise improvements to individual relay fixtures cannot make the product pair. In parallel, fix durable machine ownership, explicit private admission, shared-origin response isolation and public resource budgets before allowing untrusted enrollment.

Next complete authenticated ticket issuance/backend upgrades and host-scoped terminal routing, including grid negotiation and revocation. Implement generation-bound registrations, collision/reuse handling and bounded client-bound retry semantics without weakening the existing daemon's atomic single-use/expiry/revocation controls. Finish browser host creation, second-host pairing, credential migration, identity-authenticated direct upgrades and settings derived from actual daemon state.

Finally run one real browser/relay/gateway chain with no manually provisioned machine tokens, then two machines on the same relay origin, and exercise negative authorization, expiry, restart, disconnect and revocation cases. Resolve the broader test instability, require named nonzero test counts, and record a deployment receipt tied to a known binary/commit. Tests must not replace the authenticating gateway with an accepting echo socket or a handwritten success response.

**Approval requires working end-to-end behavior and security invariants, not simply the presence of T01–T10 commits or a green subset of fixture tests. This review is complete; implementation approval is withheld.**

## Primary security references

These references support security reasoning, not repository/deployment facts. Repository claims are anchored above to the reviewed source and executable observations.

[E1] OWASP, WebSocket Security Cheat Sheet — authentication, Origin validation, session invalidation and sensitive logging: `https://cheatsheetseries.owasp.org/cheatsheets/WebSocket_Security_Cheat_Sheet.html`.

[E2] MDN, Same-origin policy — scheme/host/port origin boundary and origin-scoped browser storage: `https://developer.mozilla.org/en-US/docs/Web/Security/Defenses/Same-origin_policy`.

[E3] Cloudflare, HTTP headers — client-IP forwarding and trusted-ingress considerations: `https://developers.cloudflare.com/fundamentals/reference/http-headers/`.

[E4] RFC 8628, section 5.1 — user-code entropy and online rate-limit considerations: `https://www.rfc-editor.org/rfc/rfc8628.html`.
