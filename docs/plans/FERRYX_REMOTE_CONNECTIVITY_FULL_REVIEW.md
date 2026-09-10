# Remote connectivity review

## Executive Summary & Verdict

**REQUEST CHANGES — block merge and public deployment.**

Reviewed all 44 files changed by `git diff origin/main..HEAD`: 5,758 additions and 222 deletions, including relay/auth/gateway, UI, SSH/worktree, notification, design-mode and documentation changes.

HEAD: `1f21e66ad577207b3531f708822dfaf26ce10061`.
Local origin/main: `8601bd0f0b2c0f6a7ff020ab4e4bd2b6d13056e2`.
No implementation files changed.

The branch has useful components but is not a working, secure end-to-end multi-host overhaul. Passing unit tests bypass important failing interfaces. Findings below separate actual source-linked relay reproductions, source-extracted UI checks with mocks, and inspection-only operational risks. P1 is merge-blocking; P2 is a material correctness deficiency.

## Critical Blockers

### R1 — P1: Direct hints can disclose the device bearer

`ui/src/remote/RemoteApp.tsx:216–277,386–396`; `ui/src/lib/directPathUpgrade.ts::probeCandidate`.

Any HTTP(S) origin supplied through query parameters `lan`, `ts` or `tailscale` is accepted and persisted. A successful unauthenticated health response selects it; subsequent authenticated events/API requests send the existing device bearer to that origin. An already-paired user following a modified legitimate-origin URL can therefore disclose the token to a public HTTPS candidate. Local-network restrictions do not prevent that case.

The source-extracted UI check reproduced the routing using mocks and a dummy token, without external transmission. Require host-bound authenticated candidate exchange and host-identity proof before sending reusable credentials. Scope credentials/hints per host and test that unrelated origins receive no token. A 200 response or private-IP restriction alone is insufficient.

### R2 — P1: Unauthenticated, unissued relay sessions are accepted

`src-tauri/src/remote/relay_server.rs:164–200,303–322`.

Data/client handlers check neither credentials nor session issuance, owner, role, expiry or single-use status. They pair sockets solely by a global string ID. An actual loopback fixture forwarded a payload under an arbitrary unissued ID without opening any authenticated control connection. The control endpoint itself correctly rejected missing credentials with 401.

This proves unauthorized relay-resource use. A disclosed live ID also permits competing attachment to either role; it does not alone prove unauthenticated terminal control, because gateway device checks remain separate. Require authenticated allocation, role-specific single-use grants bound to host/device/generation, explicit active-session state and duplicate rejection.

### R3 — P1: Production relay integration and browser protocol are missing

`src-tauri/src/remote/relay_client.rs:63–81,127–179`; `relay_server.rs:147–158,218–223`; `ui/src/remote/RemoteApp.tsx`.

Repository references construct RelayClient only in tests and call notify_incoming_session only from a test. Client attachment never selects/notifies the target daemon. The relay router supplies only three tunnel endpoints, not application/pairing/API ingress. Actual fixture requests for `/`, `/api/v1/pair/exchange` and `/api/v1/events` returned 404.

The daemon also writes WebSocket payload bytes directly to a raw TCP HTTP-gateway connection; the browser lacks the corresponding inner HTTP/WebSocket handshake or request adapter. Merely wiring constructors will not resolve this mismatch. Production gateway address is hard-coded to 127.0.0.1:43821; override is test-only.

Implement daemon-owned credentials/lifecycle, host/session dispatch, browser-compatible public ingress and forwarding to the actual bound port. Require public-ingress-to-real-gateway acceptance tests without private notifier calls or echo-server substitutes.

### R4 — P1: WSS support is absent

`src-tauri/Cargo.toml`; `src-tauri/src/remote/relay_client.rs:127–135,185–195`.

HTTPS URLs become WSS, but the resolved tokio-tungstenite feature graph has no TLS implementation. The actual source-linked fixture returned `Url(TlsFeatureNotEnabled)`. URL-rewriting tests do not establish TLS connectivity.

Enable the intended TLS feature/trust store and test valid/invalid certificates, hostname validation and reconnects. Never silently downgrade to plaintext; document relay TLS termination.

### R5 — P1: QR producer and consumers disagree

`ui/src/components/settings/RemoteAccessSection.tsx:33–60`; `ui/src/remote/RemoteApp.tsx:237–270,354–373`.

Settings emits `/#pair=123456&hints=...`; pairing passes everything after `#pair=` as the PIN. Candidate parsing instead expects query parameters lan/ts/tailscale and ignores the generated hints fragment. BoundAddress already includes a port but receives another, producing a malformed loopback candidate such as `http://127.0.0.1:43821:43821`. Desktop loopback is not a valid address to advertise to another device anyway.

These mismatches were reproduced from source. Use one shared versioned URL codec, include host identity, publish reachable listener addresses and test actual producer-to-consumer round trips, not just generated string equality.

### R6 — P1: Host selection does not change the backend

`ui/src/remote/RemoteApp.tsx:432–440,497–528,778–788`; `ui/src/state/remoteHostStore.ts`; `MobileHostDrawer.tsx`.

The drawer changes activeHostId, but refresh, events and terminal retain the prior endpoint/token. No production discovery-to-store population path was found; tests fill the store manually. Once populated, the selected label can disagree with the host receiving requests.

Server code at `src-tauri/src/remote/server.rs:945–950` discards the host prefix before local terminal lookup; that is not host routing and does not replace active-session authorization. Add a host-aware endpoint/credential manager, cancel stale requests on selection changes and verify both displayed state and commands using two distinguishable fixture hosts.

### R7 — P1: Direct upgrade does not migrate terminal traffic or safely roll back

`ui/src/remote/RemoteApp.tsx:386–396,497–528,778–788`; `ui/src/remote/RemoteTerminal.tsx:172–174,402–404`.

Only API/events change origins. RemoteTerminal still uses window.location.host and receives no transport endpoint. Source-extracted checks confirmed different event and terminal origins. The badge changes after health success, before authenticated WebSocket handoff; failed direct connections retry the selected direct path rather than restoring relay. WebSocket construction/open failure is not treated transactionally as upgrade failure.

Keep the original connection until the replacement is authenticated and ready; switch the actual terminal transport without unnecessary renderer/session replacement, and roll back on failure. Test continuous input/output, failed handshake, direct loss and fallback. Derive badges from established connections rather than probes.

## Security & Architectural Findings

Machine/device credentials are separated in memory, but machine keys are initialized empty and omitted from persisted auth state (`src-tauri/src/remote/auth.rs:65–70,100–143`). The relay has a separate configured allowlist, with no complete production provisioning/rotation path. CLI authority is independently broken in R11.

Long-lived tokens remain accepted in query strings, including events and optional relay control authentication. Replace reusable WebSocket URL credentials with scoped short-lived attachment tickets and prevent credential-bearing logs. Permissive CORS/query-token support alone is not proof of bypass; R1 is the concrete new disclosure path.

This is a WebSocket/TCP relay, not implemented WebRTC ICE/DTLS or DERP end-to-end encryption. Document relay trust explicitly; TLS termination at a relay does not protect application content from that relay.

**Positive invariant:** Focused tests passed for rejecting inactive terminal attachments and closing connections on active-desktop changes. Expanded session listing is not proof of inactive-session control. Preserve final-backend enforcement through every new transport.

**Browser correction:** Chrome 142 introduced permission-gated Local Network Access, replacing the old PNA preflight effort. Mixed-content exemptions are conditional. Chromium's 2026 WebSocket rollout documentation also distinguishes WebSockets from fetch, including no equivalent WebSocket targetAddressSpace option. A successful fetch does not establish WebSocket availability. Primary sources: Chrome Developers, “New permission prompt for Local Network Access”; Chromium blink-dev, “Intent to Ship: Local network access restrictions for WebSockets,” February–March 2026.

The prober catches fetch errors and has a bounded timeout, but requests `/health` while the explicit gateway health route is `/api/v1/health`. SPA fallback can return 200 without health or identity validation. Crafted mocked error strings do not exercise native permission prompts, mixed-content rules or authenticated handoff. Chrome/Safari/Firefox compatibility remains unverified.

## Edge Case & Operational Deficiencies

### R8 — P1: Old connection cleanup deletes replacements

`src-tauri/src/remote/relay_server.rs:126–140,164–203,273–302,347–353`.

Both races were reproduced against actual source. A replacement control sender causes its predecessor to exit and unconditionally unregister the replacement. A duplicate same-kind pending half replaces the old sender; predecessor cleanup then removes the replacement, closing both sockets.

Use generation-aware compare-and-remove cleanup, explicit duplicate policy and active/consumed session state. Test reconnect, timeout, replacement and disconnect interleavings.

### R9 — P1 before public exposure: Missing aggregate budgets and structured cancellation

`src-tauri/src/remote/relay_server.rs:84–86,228–235,361–383`; `relay_client.rs:140–150,199–246`.

Control notifications are unbounded; pending sessions lack global/IP/machine admission limits. The reaper limits waiting duration only, not arrival count or active lifetime. Active pairs leave the pending map. Per-session daemon tasks are detached, unbounded and not tied to parent cancellation. Both proxies join directions without a bounded failure/shutdown policy; stalled opposite directions can retain resources. No complete heartbeat/dead-peer policy exists, and I/O errors are often discarded.

Awaited sends and the fixed TCP read buffer provide some real flow control; the claim is missing aggregate/lifecycle bounds, not unlimited individual frame size. No long-running leak measurement was made. Add admission semaphores, bounded queues, explicit frame/byte/deadline limits, task supervision and cancellation propagation; test disable, replacement, slow consumers and reconnect storms with resource counts.

### R10 — P1: Windows direct modes fail; relay hints target unbound interfaces

`src-tauri/src/remote/state.rs:100–107,146–169`; `server.rs::start_remote_server_with_resolver`.

Non-Unix interface enumeration always errors, so Windows LocalNetwork/Tailscale startup fails through the new resolver. Relay mode resolves no external interface and binds loopback only, despite advertising LAN candidates. Unix first-non-CGNAT-IPv4 selection is also not a reliable intended-LAN policy on multihomed systems.

Implement portable interface selection and an explicit consented bind/advertise policy. Advertise actual listeners/ports, support relay-plus-direct where intended and test Windows, multihoming and interface changes.

### R11 — P2: CLI pairing uses a separate, ephemeral authority

`src-tauri/src/main.rs:189–248`; `src-tauri/src/remote/auth.rs:100–111,215–268`.

Generate creates a fresh manager, mints an in-memory PIN and exits. Approve creates a different manager with an empty PIN window. It cannot approve the separate CLI or daemon-owned PIN. The same-object unit test misses this boundary; CLI approval also creates a bearer internally without delivering it to a pending remote client.

Route CLI pairing through the live daemon authority and bind approval to an actual device request. Test separate processes against a fixture daemon.

### R12 — P2: Push registration, auth and ownership do not compose

`ui/src/lib/pushSubscription.ts:28–30,55–65`; `src-tauri/src/remote/server.rs:1681–1714`; `push.rs`.

Registration emits no bearer header or query token, so it cannot pass the newly authenticated handlers; the source-extracted request check confirmed this. The server discards validated device identity and stores subscriptions globally without ownership/quota/revocation cleanup. A device can remove another known endpoint. The helper has no production caller and delivery is not connected end to end.

The ready.catch(register) service-worker fallback also cannot handle an absent active worker: ready can wait indefinitely and never rejects, per MDN's ServiceWorkerContainer.ready documentation. This does not mean the normal registered-worker path always hangs.

Pass the selected host credential, bind subscriptions to devices, cap them, wire delivery/revocation cleanup and implement bounded worker registration/readiness handling.

### Code quality and scope notes

Production poisoned-lock expect/unwrap sites exist in push.rs:41,48,53 and design_mode.rs:32,37; no external panic trigger was demonstrated. Conversely, relay_server.rs:174 checks/removes under one exclusive lock and is not itself evidence of a race.

The host-store transport union omits relay while the drawer widens/casts it. Standardize the type and use exhaustive handling. Additional SSH/worktree and notification diffs were reviewed; no equally strong new blocker was established there, not a claim of exhaustive platform validation. Focused notification/workspace tests passed.

The alleged mdns.rs/ssh_tunnel.rs deletions are absent from this comparison: neither file exists in either revision. Planning documents are not evidence that operational milestones were implemented.

## Test Coverage & Verification Audit

- Rust remote filter: 109 passed. Fresh relay-only rerun: 4 passed, not four additional independent E2E scenarios.
- Six focused UI files: 108 passed. TypeScript noEmit: passed.
- Actual loopback/source-linked relay counterexamples confirmed R2/R4/R8 and missing ingress. Fixtures used dummy credentials and were stopped.
- Source-extracted UI checks with mocked fetch/storage confirmed R1/R5/R7/R12 contract defects; these are not native-browser tests.
- Packaged ferryx-relay build timed out during dependency compilation, without a repository compiler error. Packaged build/runtime not certified.
- No full-project suite, native Chrome/Safari/Firefox run, Windows/Linux build or long soak. Diff whitespace/final status checks passed; HEAD/base unchanged, no tracked review changes, pre-existing build/dependency paths preserved, no active commands or temporary fixtures.

The tests validate useful components but bypass public session allocation, real gateway protocol, QR round trips, actual host switching and native transport handoff.

## Actionable Recommendations

First close credential/candidate and relay-session authorization boundaries, with bounded admission before exposure. Then complete one WSS public-ingress-to-real-gateway path using persistent daemon authority and the real bound port. Next make host selection and terminal transport migration authenticated, transactional and reversible. Finally require cross-platform/browser failure-path tests for revoked/wrong-host/expired grants, duplicate attachment, active-session changes, TLS failures, denied permissions, direct loss, relay/daemon restart and resource cleanup.

**Bottom line:** Do not merge this revision as a completed relay-first multi-host implementation. Security defects and missing production contracts require changes despite passing focused tests.