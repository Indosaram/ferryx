# Ferryx zero-config sync: adversarial architecture and security review

Date: 2026-09-10

## Summary verdict: REQUEST CHANGES

The product direction is sound, but this plan is not implementation-ready for an Internet-facing relay. Removing manual provisioning must automate authentication, not remove it. The missing pieces are protocol and authorization decisions, not minor implementation details: machine ownership, public PIN admission, atomic rendezvous, an actual HTTP/WebSocket reverse transport, and persistent host-scoped browser credentials.

Review target: `.omo/plans/ferryx-paseo-herdr-zero-config-sync.md`, all 188 lines. Reviewed plan SHA-256: `6ac3d6b6aa9757dfd229b2196bd28ae461671abf160188d5114ad091bd5c4d82`. The worktree was initially clean. This review adds only this report; it does not change the plan or product implementation, deploy a relay, or assert that the proposed vulnerabilities were exploited in production. Paseo/Herdr parity is treated as the requested user experience, not evidence about either product's internal security design.

The existing daemon has useful protections worth preserving: atomic single-use PIN consumption, monotonic expiry, separate machine/device credential tiers, revocation signaling, generation-safe relay cleanup, and bounded pending relay frames. The review distinguishes these working controls from missing controls at the new public boundary. See `src-tauri/src/remote/auth.rs:184-263`, `src-tauri/src/remote/auth_security_tests.rs:4-193`, and `src-tauri/src/remote/relay_server.rs:238-277`.

### Answers to the five requested review questions

| Question | Finding |
| --- | --- |
| Rendezvous and timing | Yes: publishing before registration ACK, concurrent claims, reconnect replay, and PIN reuse are unspecified. The current daemon's expiry check is sound, but a relay lease cannot replace it. |
| HTTP versus WebSocket proxying | The existing relay is not an HTTP proxy. An outbound reverse transport and HTTP/upgrade adapter are necessary. A custom Tower service is optional integration, not a substitute for that transport. |
| Machine/PIN collisions | A UUID is not ownership proof. PIN registration must atomically reserve a unique code and bind it to an authenticated machine, control generation, and registration ID. |
| Public brute-force protection | The daemon's five-failure budget does not protect PINs missed by the relay lookup. Public admission must count misses before routing, with fleet-wide risk controls. |
| Ten-task decomposition | Incomplete and internally inconsistent: 11 wave/dependency entries, 10 numbered todos, and no numbered drawer task. HTTP/WS, CLI advertisement, credential migration, and negative security tests lack adequate ownership. |

## Critical risks and vulnerabilities

### R1 — P0: public machine IDs are not authenticated machine ownership

**Evidence.** The plan's task 2 replaces the mandatory token with `MachineIdentity.machine_id`; task 3 permits replacement with a generation increment. Today the relay validates an allowlisted bearer before registering a control channel, and its control map is already keyed per credential. See `.omo/plans/ferryx-paseo-herdr-zero-config-sync.md:83-118`, `src-tauri/src/remote/relay_server.rs:124-185`, and `src-tauri/src/remote/server.rs:1830-1850`.

**Failure scenario.** A public client learns a victim's machine ID from an intended route, then claims that ID on a new control connection. If the relay accepts caller-supplied IDs without an ownership proof, incrementing a generation merely makes the attacker's replacement internally consistent. Device tokens later routed to that replacement can be disclosed. Random UUID collision resistance does not address deliberate reuse or identity cloning.

**Required change.** Define a persistent asymmetric machine key and a proof-of-possession handshake using a fresh challenge bound to the relay audience, protocol version, machine identity, and control connection. Install the registry entry only after verification. Prefer a self-certifying public routing ID derived from the public key; retain a UUID as installation metadata if useful. If UUIDv4 must remain the routing ID, specify durable UUID-to-key enrollment that survives relay restart. An in-memory first-claim-wins map does not supply durable ownership.

Reject a different key claiming an existing identity; reconnect the same verified owner under a new generation and fence the old connection. A copied identity file represents the same machine cryptographically: reject competing live clones or require an explicit reset/re-enrollment policy, rather than silently moving sessions between them. Specify rotation and loss recovery without trusting a display name.

Keep public self-registration and private allowlist modes explicit. A private relay with an empty/malformed allowlist must remain fail-closed. Legacy tokens remain secrets sent in headers, not public machine IDs or URL segments. Update the standalone configuration as well as the daemon; `src-tauri/src/bin/relay.rs:22-94` still constructs an allowlist-based relay.

### R2 — P0: PIN-only fleet rendezvous bypasses the existing guessing budget

**Evidence.** `AuthManager` generates values in `100_000..=999_999`, giving 900,000 possible codes, and charges invalid daemon exchanges against a five-failure window. The planned relay first looks up the PIN to decide which daemon to contact. A miss therefore never reaches that daemon budget. See `src-tauri/src/remote/auth.rs:184-235` and `.omo/plans/ferryx-paseo-herdr-zero-config-sync.md:101-127`.

For an illustrative fixed population of 1,000 distinct active PINs, 1,000 uniformly random guesses with replacement have probability `1 - (1 - 1000/900000)^1000`, approximately **67.1%**, of finding at least one active PIN. This is a fleet attack model, not a production measurement. Separately, 1,000 independent draws before collision rejection have approximately **42.6%** probability of at least one collision, using the birthday approximation `1 - exp(-1000*999/(2*900000))`. A per-daemon "five attempts" claim cannot be transplanted to a global rendezvous namespace.

**Required change.** Apply bounded request-body/schema validation and admission limits before PIN lookup, WebSocket upgrade, or daemon stream allocation. Count unknown, expired, and consumed PIN submissions as attempts. Combine source/IP and prefix budgets, per-registration claim limits, per-machine registration/session quotas, and a global admission budget with a documented maximum active-PIN population. Bound and expire the limiter maps themselves. Code rotation, reconnects, successful guesses, fresh attacker identities, and different guessed PINs must not trivially reset the relevant budgets. Do not solve abuse by a global five-failure switch that lets anyone lock out every user.

Use an unguessable QR pairing capability, for example at least 128 random bits, as the preferred automatic flow. Keep six-digit manual pairing only with a defensible aggregate risk budget and additional protection such as explicit daemon-side approval of a client-bound request. IP throttling alone is not a complete distributed-attack defense. RFC 8628 section 5.1 provides directly relevant short-user-code reasoning, although Ferryx is not required to implement OAuth [E2].

Behind Cloudflare, derive source identity only through an explicitly trusted ingress chain. Cloudflare documents `CF-Connecting-IP` and Worker-related behavior [E5]; accepting arbitrary direct-origin requests with a client-chosen forwarding header would invalidate the limiter. Restrict the origin listener/network accordingly. The current binary binds a wildcard address; the actual deployment firewall was not inspected (`src-tauri/src/bin/relay.rs:94-107`).

### R3 — P0: the HTTP and browser WebSocket path is not defined by the current tunnel

**Evidence.** `relay_router` exposes only three GET WebSocket routes. `RelayClient::handle_session` opens a data WebSocket and a plain TCP connection to the loopback HTTP gateway. `proxy_ws_to_tcp` writes decoded message payloads into that TCP socket. See `src-tauri/src/remote/relay_server.rs:280-287` and `src-tauri/src/remote/relay_client.rs:189-268`.

An ordinary browser POST does not become an internal tunnel request by adding a routing map. Similarly, the browser's WebSocket HTTP upgrade terminates at the relay; its subsequent terminal messages are not a fresh HTTP upgrade on the daemon's TCP connection. Forwarding those payloads into Axum's HTTP listener cannot establish the intended terminal session. The existing relay/client tests exercise frame forwarding and raw TCP echo, not this missing handshake.

The exchange contract also disagrees with the source. The daemon requires JSON `code` **and** `deviceName`, and returns `token` and `device`, not `machine_id`. The plan's `{code}` example omits a required field, and its curl example omits the JSON content type. See `src-tauri/src/remote/server.rs:157-226` and `.omo/plans/ferryx-paseo-herdr-zero-config-sync.md:119-136`.

**Required change.** Specify and implement the transport described below, including authenticated HTTP after pairing, not just the exchange endpoint. Freeze one wire naming convention for machine metadata. Distinguish terminal session IDs from fresh relay stream IDs and specify who allocates each; a browser cannot invent an issued relay session by appending IDs to a URL.

### R4 — P1: registration, claim, expiry, and retry lack a shared state machine

**Evidence.** Task 4 sends `RegisterPairingPin { pin, expires_in }` but defines no response or claim state. Settings immediately exposes a generated code and QR link. The daemon's current exchange correctly rejects `elapsed >= 60 seconds` under a transaction; the browser fires a one-shot exchange and ignores its result on effect cancellation. See `.omo/plans/ferryx-paseo-herdr-zero-config-sync.md:110-118`, `ui/src/components/settings/RemoteAccessSection.tsx:133-160`, `src-tauri/src/remote/auth.rs:208-263`, and `ui/src/remote/RemoteApp.tsx:393-418`.

Concrete failures include a scan before the relay receives registration; a late registration or replay incorrectly renewing sixty seconds; two clients concurrently forwarding the same PIN; an old control channel canceling a newer registration; and a successful exchange whose response is lost, leaving the legitimate browser without the token. Retrying that last request against the current single-use API cannot recover the result.

**Required change.** Adopt the lifecycle and deadline rules below. The relay is a routing/admission authority, while the daemon remains the final code and device-authorization authority. A bounded cleanup sweep is not an expiry check. Guard every lookup/claim with a deadline check and never hold a registry lock over a network await.

PIN registration must use atomic insert-if-absent or equivalent reservation. Resolve collisions before displaying a remotely usable code; retry generation without stealing another machine's mapping. Include an opaque registration ID to fence reuse. A short quarantine for recently retired PINs reduces stale typed-code confusion; QR links should carry the high-entropy registration capability and expected machine identity rather than only a recycled six-digit value.

### R5 — P0: session IDs cannot safely authenticate both public tunnel roles

**Evidence.** `WaitingHalf` contains no owning machine or control generation. Both data and client handlers reserve a half using only the session ID and requested half kind. This relies on the old documented assumption that IDs are handed out only over trusted control channels. See `src-tauri/src/remote/relay_server.rs:17-27`, `src-tauri/src/remote/relay_server.rs:77-99`, and `src-tauri/src/remote/relay_server.rs:238-263`.

Once the browser learns the ID, it can try to occupy the daemon data half first unless the new design adds role authorization. Adding a machine ID path segment alone does not fix that. Likewise, a global session table must not resolve a valid stream under another machine path or reconnect generation.

**Required change.** Bind stream ownership to `(machine_id, control_generation, stream_id, purpose)` and distinguish client authorization from daemon data-channel authorization. Require a role-specific, short-lived, single-use data capability or authenticated machine proof for data attachment. Bind client admission to the daemon-issued device authorization and intended operation/terminal. Prevent ticket replay, cross-machine reuse, role swapping, and old-generation attachment. Retain pre-upgrade duplicate reservation and generation-safe cleanup.

Use per-machine fairness within global resource limits; the current 100-entry global budget is a safety cap, not fair multi-tenant admission. Bound control connections, PIN registrations, pending HTTP responses, data tasks, bytes, and unauthenticated socket staging. Revocation and Remote Access shutdown must cancel the corresponding active streams, not just remove a registry entry.

### R6 — P1: the no-token-in-URL guardrail is already violated

**Evidence.** Workspace HTTP, events, and terminal URLs include the device bearer. `RemoteClient` also retains global-token/global-origin behavior. See `ui/src/remote/RemoteApp.tsx:274-287`, `ui/src/remote/RemoteApp.tsx:361-378`, `ui/src/remote/RemoteTerminal.tsx:177-177`, and `ui/src/lib/remoteClient.ts:42-66` / `ui/src/lib/remoteClient.ts:118-123`. The plan prohibits this at `.omo/plans/ferryx-paseo-herdr-zero-config-sync.md:36-39`, but assigns no comprehensive migration task.

Use `Authorization: Bearer` for browser HTTP. The browser WebSocket constructor accepts a URL and subprotocols, not an arbitrary Authorization-header parameter [E3]. Specify either a carefully scoped cookie design with CSRF/Origin controls, or an authenticated HTTP-issued short-lived socket ticket delivered in the first WebSocket message. Prefer the latter here: restrict unauthenticated socket count, bytes, and lifetime, and do not attach privileged backend streams until the ticket is validated. The daemon must still validate the device and enforce View/Control permissions; loopback source is not authorization.

Do not replace the bearer with a hash accepted as another reusable URL bearer. Redact PINs, pairing capabilities, device credentials, and stream capabilities from edge, relay, daemon, traces, and test evidence. Use no-store responses for pairing and credential exchange. Origin checking, session invalidation, message limits, and secret-free logging need explicit tests [E4].

### R7 — P1: multi-host routing and persistence are missing below the drawer

**Evidence.** `RemoteApp` converts the selected address to `.origin`, discarding any `/host/{machine_id}` prefix; its event URL similarly rebuilds from `base.host`. The host store is memory-only and its transport union does not include relay. The drawer only renders/selects that store. See `ui/src/remote/RemoteApp.tsx:274-318`, `ui/src/state/remoteHostStore.ts:1-45`, `ui/src/state/remoteHostStore.ts:52-142`, and `ui/src/remote/MobileHostDrawer.tsx:149-166`.

Persisting a token alone will not restore machine routing after refresh. Different machines on the same relay origin must not share the fallback `local:<origin>` identity. A new pairing hash also needs handling while an existing remote host is selected; currently URL pairing is gated by `readUrlHints`.

**Required change.** Define a persistent host record keyed by `(relay_origin, machine_id)`, including the pinned machine key/fingerprint, device identity, display metadata, routing base, and schema version. Store credentials and host metadata consistently, hydrate before choosing the active host, and migrate legacy same-origin credentials without guessing their machine ownership. Centralize HTTP/events/terminal URL construction and credential selection across RemoteApp, PairingPage, RemoteTerminal, and RemoteClient.

Switching A to B must close A's sockets and fence/abort A's delayed responses without clearing B's credentials. Test two hosts on one origin with identical terminal IDs, reload, repeated pairing, offline hosts, per-host revocation, and denied storage. Show only previously paired/authorized hosts. Without an account or other authenticated inventory protocol, "dynamically discovered relay hosts" cannot mean enumerating every connected machine or automatically syncing a host list between browsers.

### R8 — P1: direct-path reachability is not host authentication; shared-origin trust is unstated

**Evidence.** Direct candidate validation restricts addresses to private/overlay/loopback locations, but `probeCandidate` accepts any successful health response. RemoteApp then switches authenticated traffic to the selected URL. See `ui/src/lib/directPathUpgrade.ts:45-64`, `ui/src/lib/directPathUpgrade.ts:98-140`, and `ui/src/remote/RemoteApp.tsx:429-445`.

Where browser policies permit the connection, private-IP reuse or an attacker-controlled private endpoint answering 200 can redirect a long-lived bearer to the wrong machine. Preserve address restrictions, but require an authenticated encrypted channel bound to the paired machine before sending credentials. A signed health response over otherwise plaintext HTTP is insufficient against a forwarding/interception attacker. Fall back to the relay when identity validation, mixed-content rules, local-network access policy, or CORS prevents a safe direct path.

The proposed TLS-terminating HTTP proxy is a **trusted relay**, not end-to-end encryption: it can see the PIN, token response, and terminal traffic. State that trust assumption explicitly. If relay compromise must not expose commands or bearer credentials, this is a different protocol requiring end-to-end authenticated encryption and a trustworthy client distribution model.

The inspected relay router also has no root SPA/static route, while the daemon has an HTML fallback (`src-tauri/src/remote/server.rs:1717-1747`). The deployment may serve the frontend elsewhere, but the plan must specify it. Serve trusted relay-owned UI assets once. Do not generically proxy daemon-controlled HTML, JavaScript, cookies, or service workers onto the shared relay origin: paths do not isolate origins, and that could expose credentials for other paired hosts. Allowlist daemon API routes, methods, response types, and headers; do not expose arbitrary loopback targets or CONNECT.

### R9 — P1: CLI advertisement and deployment topology are not covered

**Evidence.** The CLI's `generate` command is valid, but creates a separate file-backed AuthManager rather than asking the running daemon's control connection to register a PIN. The daemon reloads shared auth state during transactions, which permits later local exchange but does not emit a relay advertisement. See `src-tauri/src/main.rs:154-174`, `src-tauri/src/main.rs:191-251`, `src-tauri/src/remote/auth.rs:414-453`, and `src-tauri/src/daemon/server.rs:1713-1717`.

Move remote pairing orchestration into a daemon-owned PairingCoordinator used by GUI IPC and the headless CLI. Keep synchronous auth/persistence independent of WebSocket ownership. Return registration state and actual remaining lifetime, not a hardcoded sixty-second readiness claim (`src-tauri/src/ipc/remote.rs:245-268`). A local-only code may still be generated offline, but it must not be presented as a ready relay rendezvous.

Specify the in-memory relay's deployment boundary: one authoritative relay process/ownership shard for this release. Ordinary round-robin replicas can place control registration, PIN exchange, and data attachment in different processes. Client-IP stickiness does not colocate the daemon and browser. Multi-replica availability needs an explicit owner-routing/coordinating design; prohibiting Redis does not remove that requirement. A single-instance initial release is acceptable if restart behavior, quotas, reconnect jitter, and availability limitations are documented.

## Architecture and protocol refinements

### A. Freeze the routing and trust contract first

Use a public exchange endpoint and explicit host-prefixed authenticated browser APIs. Example external route families:

```text
POST /api/v1/pair/exchange
HTTP /host/{machine_id}/api/v1/{*path}   # allowlisted routes/methods, not an unrestricted proxy
WS   /host/{machine_id}/api/v1/events
WS   /host/{machine_id}/api/v1/terminal/{terminal_id}
WS   /tunnel/control/{machine_id}       # verified machine ownership
WS   /tunnel/data/{machine_id}/{stream_id} # daemon-role capability
```

Explicitly map legacy push routes under `/api/push/...` if retained. Do not let the root exchange handler become a generic unauthenticated host proxy. Use Axum 0.8 brace captures, not literal old-style `:machine_id` syntax; its Router documentation also explains optional Tower services and prefix stripping [E1]. The browser response should consistently include token, device, machine ID, display name, and key fingerprint, with identity supplied by the authenticated machine binding rather than untrusted client fields.

### B. Build a real reverse HTTP/upgrade transport

A practical initial implementation can retain one outbound data WebSocket per HTTP request or long-lived WebSocket stream. The control channel carries only bounded notices, acknowledgements, cancellation, and liveness, not terminal bulk data.

For HTTP, the relay admits the request, resolves its machine/claim, allocates a generation-bound stream, and sends an `OpenStream` notice. The daemon authenticates its data attachment and connects only to the configured loopback gateway. At the relay, adapt the ordered binary data-WebSocket payloads into an AsyncRead/AsyncWrite byte stream and run a mature HTTP client connection over it. Construct/forward the correct HTTP method, path, safe headers, and bounded body; parse and return the daemon's status, headers, and body. The daemon's existing raw TCP forwarding is a reusable transport primitive, not the complete proxy.

A normal Axum handler can invoke this broker/client; a Tower service can encapsulate it. Axum's official reverse-proxy example uses a handler plus an HTTP client [E6]. Neither style supplies the outbound NAT tunnel automatically. There is no requirement to open a WebSocket back into the relay's own public `/tunnel/client` endpoint; an internal broker is simpler.

For browser WebSockets, terminate the public handshake and validate the short-lived client admission ticket, then perform a separate authenticated HTTP upgrade to the daemon over the tunnel, or implement an explicit daemon-side WS-aware stream kind. Preserve inner upgrade parser leftovers, masking/framing semantics, close propagation, and message ordering. A TCP read is not a WebSocket message boundary. Do not treat a tunnel stream ID as the terminal ID or as a device credential.

Specify stripping of hop-by-hop and connection-nominated headers for normal HTTP forwarding, deliberate handling of upgrades, canonical Host/path construction, and prevention of cross-host connection-pool reuse. Apply connect, response, idle/send, and total deadlines; bounded body/header/frame sizes; backpressure; cancellation; and per-host/global budgets. Use an HTTP library rather than hand-concatenating HTTP framing. Reject arbitrary destinations and unapproved redirects. Do not automatically retry non-idempotent application requests on transport failure.

### C. Define a linearizable pairing lifecycle

Use states `Created -> Registering -> Ready -> Claimed -> Consumed`, with terminal `Expired` and `Cancelled` paths. These are protocol states, not just UI labels.

A registration contains a protocol version, authenticated control generation, opaque registration ID, proposed PIN, and remaining lifetime. The relay atomically reserves the code, returning an ACK for that exact registration or a collision/rejection. An exact replay can return the same ACK without extending the deadline; a different registration cannot replace it. The daemon installs/persists the code before remote usability is acknowledged. Report LocalReady/RelayPending separately while disconnected, and display/copy a relay-ready QR only after ACK.

A claim atomically binds one active registration to a high-entropy client attempt identifier and a client-held delivery secret or key. A rival attempt cannot consume or retrieve the result. The daemon atomically verifies current registration and expiry, consumes it, creates at most one device credential, and records a recoverable result for that bound attempt. Require checked persistence before claiming durable success; the current best-effort writer is not sufficient for that guarantee (`src-tauri/src/remote/auth.rs:440-453`).

The daemon's original deadline remains authoritative. Relay relative TTL is a bounded routing lease; delayed messages can leave a temporarily stale relay entry but must never extend daemon acceptance. At `now >= deadline`, a new claim fails even if the sweeper has not run. A credential committed before the deadline may be delivered afterward only as the same bounded, authenticated result retrieval, not a new PIN exchange.

Do not promise exactly-once HTTP delivery. Provide at-most-once device issuance plus idempotent result retrieval for the same client-bound attempt, or explicitly require re-pairing after an ambiguous failure. Never return a cached permanent token to anyone who merely repeats the six-digit PIN. Lost ACKs, reconnects, cancellation, and cleanup must match registration ID and generation; re-advertise only still-valid registrations with remaining lifetime. Define whether outstanding claims survive reconnect; do not silently rebind them to a new owner.

### D. Make limits and failure behavior testable

Before implementation, record concrete limits for request size, admission attempts, registration rate, active codes per machine, total active codes, unauthenticated WS staging, pending/active streams, queue bytes, and retention. Select rate limits from the code-space/active-population risk model, not an unexplained "five attempts." Keep malformed, absent, expired, and used-code failures non-enumerating where practical, while allowing rate-limit responses independent of PIN existence. Expose metrics without recording secrets.

Use deterministic barriers, controlled clocks, injected ID/PIN generators, and forced transport failures. Required cases include claims immediately before/at/after expiry, forced duplicate codes, wrong-key machine takeover, role-swapped data attachment, old-generation cleanup, lost exchange response, relay restart, client disconnect before upgrade, oversized messages, distributed misses, unauthorized origins, and token revocation during active streams.

## Concrete action items and corrected task decomposition

### Defects in the current decomposition

The waves/dependency matrix have 11 entries, including drawer task 3.3, but the numbered list contains only ten and omits that drawer work. The dependency row for live verification lists only 3.1 and 4.2, even though other rows claim settings/drawer block it. A new HTTP proxy cannot be completed independently of the daemon-side transport contract. Settings depends on real registration readiness, and the frontend depends on persistent machine-scoped transport, not just root POST routing. See `.omo/plans/ferryx-paseo-herdr-zero-config-sync.md:49-79` and `.omo/plans/ferryx-paseo-herdr-zero-config-sync.md:128-172`.

`cargo check` without an environment variable does not exercise the runtime environment lookup. The binary test command currently executes zero tests because the relay behavior lives in the library. Frontend tests use Vitest: invoke the package's test script, not Bun's separate built-in test runner. An active systemd process proves neither deployed protocol version nor working HTTP/WS pairing. The final "real manual QA" item also conflicts with the earlier zero-human-intervention statement; name automated browser QA and separately identify any human acceptance gate.

### Replacement ten-task plan

The following is a replacement decomposition, not a claim that these features already exist. Contract fixtures can support parallel development, but the stated dependencies are completion gates.

| ID | Deliverable and dependencies | Verifiable acceptance criteria |
| --- | --- | --- |
| T01 | Protocol, threat model, routes, schemas, error/state semantics, limits and single-instance topology. Depends: none. | Reviewable wire fixtures specify ownership proof, registration ACK, claims/retries, HTTP/WS kinds, ticket roles, revocation, trusted-relay boundary, and same-origin asset policy. No unresolved routing/auth decisions remain. |
| T02 | Persistent machine identity, authenticated control enrollment, zero-env runtime startup, explicit private/legacy policy. Depends: T01. | Concurrent first boot yields one protected identity; restart preserves it; corrupt/read-only state fails safely; wrong-key claim and challenge replay fail; private empty allowlist rejects; clean no-token runtime establishes authenticated control. |
| T03 | Owned relay registries, collision handling, public admission limits and resource quotas. Depends: T01, T02. | Injected PIN collision cannot overwrite; unknown guesses spend budgets; new identities cannot bypass global caps; machine/path/role/generation mismatches fail; registry and limiter memory remain bounded. |
| T04 | Daemon PairingCoordinator, ACK/expiry/claim lifecycle, durable client-bound retries, GUI IPC and CLI integration. Depends: T02, T03. | CLI and GUI both obtain an advertised registration; delayed/lost ACK never produces false Ready; deadline-boundary and simultaneous-claim tests issue at most one credential; lost response cannot reveal a cached token to a rival; reconnect/rotate/disable clean up correctly. |
| T05 | Reverse HTTP transport, root exchange and all allowlisted authenticated APIs; trusted frontend serving contract. Depends: T03, T04. | A real loopback gateway exchanges valid JSON and returns bound machine metadata; GET/POST/DELETE reach the correct machine; 401/403/429, body limits, cancellation, header policy and disallowed destinations are tested. No daemon HTML/cookies execute under the shared UI origin. |
| T06 | Browser WS adapter, daemon data-role authentication, socket tickets, events/terminal forwarding, permission/revocation propagation. Depends: T02, T03, T05. | Real browser and daemon upgrades complete; terminal input/output and events work; unauthenticated, replayed, cross-host and role-swapped tickets fail; View cannot control; revoke/disable closes active streams; buffers and timers remain bounded. |
| T07 | Unified host-scoped browser transport and persistent inventory, RemoteApp/PairingPage/RemoteTerminal/RemoteClient, MobileHostDrawer and authenticated direct upgrades. Depends: T04, T05, T06. | Two hosts at one relay with identical terminal IDs remain isolated across switch/reload; adding B while A is active works; no credential appears in a URL; late A responses cannot update B; wrong-machine direct health success never receives credentials. |
| T08 | Settings default-relay UX, opt-in and LocalReady/RelayPending/RelayReady/Expired states, QR/PIN lifecycle. Depends: T02, T04. | No manual token prompt; no remote connection while disabled; configured default does not silently enable access; QR becomes ready only after ACK; countdown uses remaining lifetime; reconnect, expiry, rotate and revoke render correctly. |
| T09 | Deterministic security/integration suite, real browser end-to-end tests, command corrections and operating documentation. Depends: T02–T08. | All negative cases above and real two-daemon isolation pass with nonzero named test counts, no sleep-based synchronization, and redacted evidence; no-env startup is tested at runtime; no legacy hardening regression. |
| T10 | Staged deployment, compatibility canary, rollback and authorized live end-to-end verification. Depends: T09. | Deployed binary/build ID and protocol version match tested artifacts; private/public modes and old-client behavior are explicit; generate/register/exchange/HTTP/events/terminal/revoke work; restart recovery and rollback are exercised; QA devices/capabilities are revoked afterward. |

Original numbered tasks 1–2 map to T02; task 3 spans T02–T03; task 4 becomes T04; task 5 expands into T05–T06; task 6 becomes T07 with T06 transport support; task 7 becomes T08; task 8 becomes T09; deployment/live tasks 9–10 merge into T10. The missing drawer wave item is explicitly included in T07. T01 is the new prerequisite that makes safe parallel implementation possible.

## Verification performed and limitations

These commands were executed in the scoped worktree with no product changes:

```sh
cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --lib remote::auth::
cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --lib remote::relay_server::tests:: -- --nocapture
cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --lib remote::relay_client::
cargo test --locked --offline --manifest-path src-tauri/Cargo.toml --bin ferryx-relay
bun run --cwd ui test src/remote/RemoteRouting.test.tsx src/remote/MobileHostDrawer.test.tsx src/components/settings/RemoteAccessSection.test.tsx src/lib/directPathUpgrade.test.ts
```

Results: 13 auth, 3 relay-server, and 4 relay-client Rust tests passed; 70 frontend tests across four files passed. The standalone relay binary target exited successfully but ran **zero tests** and supplies no behavior coverage. Existing unrelated Rust warnings remain. The full regression suite, production frontend build, real-browser relay integration, and production deployment were not run in this review. The passing routing tests mock fetch/WebSocket/terminal behavior and do not validate the proposed proxy (`ui/src/remote/RemoteRouting.test.tsx:1-71`). The package's actual runner is declared at `ui/package.json:6-11`.

The probability examples were calculated from the current code range and stated assumptions, not by attacking the live relay. Production Cloudflare routing, origin firewall, service files, and frontend asset deployment were not inspected. Their absence from the inspected code is a planning gap, not a claim that the live environment lacks them.

## Release decision

Do not approve removal of the provisioning gate or public deployment until authenticated identity ownership, public PIN admission, the registration/claim lifecycle, complete HTTP/WS forwarding, and machine-scoped credential isolation are implemented and negatively tested. Preserve the existing protections, but elevate the plan's risk from medium to high: it changes the public trust boundary and transport protocol together. Zero-config can remain the user experience; cryptographic credentials and explicit authorization must still exist underneath it.

## External primary references

These sources inform protocol/framework statements and recommendations; repository findings above are independently anchored to inspected source. Consulted 2026-09-10.

- [E1] Axum Router documentation, captures, nesting and `route_service`: `https://docs.rs/axum/latest/axum/struct.Router.html`
- [E2] RFC 8628 section 5.1, user-code brute forcing and rate limiting: `https://www.rfc-editor.org/rfc/rfc8628.html`
- [E3] WHATWG WebSockets standard, constructor and handshake: `https://websockets.spec.whatwg.org/`
- [E4] OWASP WebSocket Security Cheat Sheet, Origin validation, message authentication, session invalidation, limits and logging: `https://cheatsheetseries.owasp.org/cheatsheets/WebSocket_Security_Cheat_Sheet.html`
- [E5] Cloudflare HTTP headers reference, client-IP and forwarding-header semantics: `https://developers.cloudflare.com/fundamentals/reference/http-headers/`
- [E6] Official Axum reverse-proxy example, ordinary handler plus HTTP client: `https://github.com/tokio-rs/axum/blob/main/examples/reverse-proxy/src/main.rs`
