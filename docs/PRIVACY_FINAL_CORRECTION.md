# Privacy Page Final Correction Ledger

Audited 2026-09-13 in `/Users/indo/code/project/orca-lite-wt/sa-docs` for task `st_01a09a8b`.
This ledger records the factual resolutions applied to `site/src/content/docs/privacy.md` based on `docs/DOCUMENTATION_CLAIM_COVERAGE.md` and direct inspection of repository sources. Each resolution replaces unsupported absolute claims with verified implementation facts. No production code was modified.

## Mandatory Coverage Verification

- **Local vs SSH storage:** Resolved under P:13-22 and P:97.
- **Actual SSH app-data path:** Resolved under P:24.
- **Unix vs Windows/socket permissions:** Resolved under P:25.
- **Bounded output replay:** Resolved under P:27-28.
- **Scoped telemetry/CSP:** Resolved under P:32-36 and P:37.
- **Update discovery vs signing:** Resolved under P:38 and P:99.
- **Off auto Relay via pairing:** Resolved under P:44.
- **LAN public/CGNAT not membership:** Resolved under P:49 and P:50.
- **Secure custom relay conditional and relayUrl restoration:** Resolved under P:51 and P:53-54.
- **Metadata field limits:** Resolved under P:60-64.
- **View CAN resize local terminals:** Resolved under P:66-71 and P:73.
- **No proven push sending:** Resolved under P:75.
- **Lazy identity:** Resolved under P:81.
- **Persisted PINs/capabilities and failure-window boundary:** Resolved under P:82-83 and P:84-85.
- **Unresolved base vs missing directory:** Resolved under P:90-93.
- **Token replacement:** Resolved under P:87-88.
- **Nonexclusive external traffic:** Resolved under P:48, P:97, P:100, and P:101.
- **No guaranteed immediate Off stream termination:** Resolved under P:105.
- **Accurate non-exhaustive deletion choices:** Resolved under P:110-111.

## Section Findings and Resolutions

### P:1-9 - Front matter and universal accuracy assertion
- **Source inspected:** `site/src/content/docs/privacy.md:1-9`.
- **Finding:** "Verified absence" and "Every statement reflects actual code" asserted universal claims without qualification.
- **Resolution:** Replaced absolute assertions with scoped wording stating that descriptions reflect the implementation in desktop application, remote gateway, and project website sources.

### P:13-22 - Local sessions/workspaces/preferences and platform session paths
- **Sources inspected:** `src-tauri/src/daemon/server.rs:301-361`, `src-tauri/src/terminal/remote.rs:183-200`, `ui/src/App.tsx:2824-2863`, `ui/src/lib/terminalSettings.ts:90-107,313-320`.
- **Finding:** The page claimed unconditional session paths, omitted remote SSH execution boundaries, and misrepresented Linux fallback composition.
- **Resolution:** Clarified that remote SSH workspace data stays on the remote host. Corrected Linux session resolution to show `XDG_DATA_HOME` precedence over `~/.local/share`, composed `~/.rorca/rorca/session_state.json` fallback, runtime directory fallback, and `rorca-dev` naming. Added explicit citations for UI and terminal setting storage in `localStorage`.

### P:23 - Browsing history
- **Sources inspected:** `ui/src/lib/browserHistory.ts:15-20,66-98`, `ui/src/components/settings/BrowserSection.tsx:239-242`.
- **Finding:** Stated history records locally and can be cleared, but omitted that clearing this list doesn't erase native browser profile cookies or webview cache.
- **Resolution:** Clarified that browsing history tracking requires `rememberBrowsingHistory`, and that clearing application history doesn't remove webview cookies or external site profile data.

### P:24 - SSH host registry path
- **Sources inspected:** `src-tauri/src/ipc/ssh.rs:73-86`, `src-tauri/src/daemon/server.rs:861-873`.
- **Finding:** The document claimed SSH hosts live in `~/.ferryx/ssh_hosts.json`.
- **Resolution:** Replaced the incorrect path with the platform application data base `com.ferryx.app/ssh_hosts.json` (such as `$XDG_DATA_HOME/com.ferryx.app` or `~/.local/share/com.ferryx.app`), noting that `FERRYX_DATA_DIR` overrides the location.

### P:25 - Runtime directory and socket permissions
- **Sources inspected:** `src-tauri/src/daemon/server.rs:126-168,434-439,502-533,1345-1380,1469-1486`.
- **Finding:** The page claimed both sockets have validated 0600 permissions on all platforms and excluded root access.
- **Resolution:** Restricted Unix `/tmp/rorca-<UID>` 0700 and `daemon.sock` 0600 claims to Unix platforms. Clarified that Windows uses a loopback TCP listener and `daemon.port`. Noted that the Unix agent socket (`agent-state.sock`) uses best-effort permissions without failing startup, and that user account permissions do not block privileged administrative or root access.

### P:27-28 - Terminal output buffer and bounded replay
- **Sources inspected:** `src-tauri/src/terminal/output_hub.rs:7,28-69,100-154`, `src-tauri/src/daemon/server.rs:2890-2953`.
- **Finding:** Claimed monotonic sequences allow clients to catch up on missed lines without logging, implying an unbounded replay guarantee.
- **Resolution:** Specified that catch-up applies only to retained chunks within the 512 KiB buffer capacity, as evicted chunks cannot be replayed. Clarified that this in-memory buffer does not guarantee an absence of logs by running tools or child processes.

### P:32-36 - Telemetry and analytics SDK absence
- **Sources inspected:** `src-tauri/Cargo.toml:58-177`, `ui/package.json:13-58`.
- **Finding:** Promoted repository-wide and runtime absence of any analytics as an absolute universal fact.
- **Resolution:** Scoped the statement to inspected product manifests and source code, confirming no third-party analytics SDKs exist in those dependencies.

### P:37 - Content Security Policy scoping
- **Source inspected:** `src-tauri/tauri.conf.json:30`.
- **Finding:** The page claimed the CSP restricts connections to explicitly configured relay domains.
- **Resolution:** Clarified that the desktop shell CSP connect-src permits local dev endpoints, IPC, and hardcoded `checka.cc` relay domains. It doesn't dynamically adapt to arbitrary custom relays, and doesn't govern native Rust networking or guest external webviews.

### P:38 - Update discovery vs signing
- **Sources inspected:** `ui/src/App.tsx:260-262`, `ui/src/lib/updater.ts:54-63,83-105,108-134,154-180`, `src-tauri/tauri.conf.json:34-40`, `src-tauri/src/ipc/updater.rs:5-18`.
- **Finding:** Blurred the distinction between metadata discovery and signature verification, and generalized Store detection.
- **Resolution:** Separated periodic GitHub release metadata queries from signature verification during download. Clarified that Store detection specifically checks for `\windowsapps\` in the executable path rather than checking an external flag.

### P:40 - Website analytics conditional rendering
- **Source inspected:** `site/src/components/SiteAnalytics.astro:4-11`.
- **Finding:** Validated build-time environment variable requirements.
- **Resolution:** Retained conditional disclosure noting that analytics scripts render only when both `PUBLIC_ANALYTICS_SRC` and `PUBLIC_ANALYTICS_DOMAIN` are set at build time.

### P:44 - Remote gateway activation and Off auto-Relay pairing
- **Sources inspected:** `src-tauri/src/remote/state.rs:53-60`, `src-tauri/src/daemon/server.rs:1500-1505,1948-1972`.
- **Finding:** Stated that remote gateway remains inactive until explicitly turned on, omitting pairing auto-configuration.
- **Resolution:** Documented that requesting a pairing code while the gateway is Off auto-configures and starts Relay mode. Added that previously persisted non-Off configuration is automatically restored on startup.

### P:48 - Off mode boundary and nonexclusive external traffic
- **Sources inspected:** `src-tauri/src/remote/server.rs:2266-2268`, `src-tauri/src/daemon/server.rs:2376-2389`, `src-tauri/src/terminal/remote.rs:188-200`, `src-tauri/src/ipc/browser.rs:925-934`.
- **Finding:** Claimed that when Off, no remote traffic leaves the machine.
- **Resolution:** Clarified that Off shuts down the gateway listener and relay task, but does not block independent client traffic such as SSH sessions, embedded webviews, or background update checks.

### P:49 - LocalNetwork interface selection and unauthenticated routes
- **Sources inspected:** `src-tauri/src/remote/state.rs:194-227`, `src-tauri/src/remote/server.rs:325-346,2139-2160,2203-2238,2260-2344`.
- **Finding:** Implied LAN connections are limited to private subnets and that all gateway routes require pairing.
- **Resolution:** Detailed that interface resolution can select a public IPv4 address. Supervising review of `src-tauri/src/remote/state.rs:194-227` corrected the condition: a usable routed address takes priority even when another private interface exists. Specified that paired devices access protected session endpoints, whereas health, UI assets, and pairing exchange routes remain unauthenticated.

### P:50 - Tailscale CGNAT detection vs tailnet membership
- **Sources inspected:** `src-tauri/src/remote/state.rs:110-112,118-143,229-234`, `src-tauri/src/remote/server.rs:2310-2344`.
- **Finding:** Asserted that binding to a Tailscale address restricts connections to your private tailnet.
- **Resolution:** Replaced the claim with verified behavior: the daemon binds to an address matching the `100.64.0.0/10` CGNAT range without verifying Tailscale process health or authenticated tailnet membership. Added that restricting access depends on network firewalling and Tailscale ACLs, with WireGuard encryption provided by Tailscale.

### P:51 - Relay transport encryption and custom HTTP endpoints
- **Sources inspected:** `src-tauri/src/remote/state.rs:20`, `src-tauri/src/remote/server.rs:2270-2293,2347-2374`, `src-tauri/src/remote/relay_client.rs:538-549`.
- **Finding:** Stated that client-to-relay traffic uses TLS without qualifying custom relay schemes.
- **Resolution:** Clarified that default relay traffic uses TLS, while custom relays configured with `http://` or `ws://` run unencrypted.

### P:53-54 - Wildcard prohibition and relayUrl restoration
- **Sources inspected:** `src-tauri/src/remote/server.rs:2260-2344`, `src-tauri/src/remote/state.rs:147-163`, `src-tauri/src/daemon/server.rs:1956-1962`, `src-tauri/src/remote/server.rs:2270-2293`, `src-tauri/src/ipc/remote.rs:304-312`.
- **Finding:** Generalized the wildcard bind prohibition beyond the gateway and claimed `FERRYX_RELAY_URL` is a universal startup override.
- **Resolution:** Scoped the wildcard prohibition to the production gateway listener. Explained that `FERRYX_RELAY_URL` is only read during pairing auto-configuration or IPC fallback, advising operators to set `relayUrl` in persisted `remote-config.json` for deterministic startup restoration.

### P:58 - Outgoing transport and reverse proxy guidance
- **Sources inspected:** `src-tauri/src/remote/server.rs:2203-2238`, `src-tauri/src/remote/relay_client.rs:538-549`.
- **Finding:** Described HTTPS via proxy as guaranteed rather than conditional on proxy configuration.
- **Resolution:** Clarified that direct LAN connections listen on plain HTTP and WebSockets, recommending an operator TLS-terminating reverse proxy on untrusted networks.

### P:60-64 - Outgoing metadata field limits and context payload
- **Sources inspected:** `src-tauri/src/remote/protocol.rs:180-195,232-264`, `src-tauri/src/remote/server.rs:657-767,1450-1473,1830-1845`.
- **Finding:** Listed process names and a pane identifier as guaranteed metadata fields.
- **Resolution:** Corrected metadata fields to project names, worktree branch labels, session IDs, and optional titles. Removed the claims that process names are dedicated metadata fields and that context contains a separate pane ID.

### P:66-71 - Control permission enforcement and View terminal resize capability
- **Sources inspected:** `src-tauri/src/remote/server.rs:963-969,1090-1096,1136-1142,1477-1528,1850-1920`, `src-tauri/src/terminal/session.rs:210-215`.
- **Finding:** Claimed window geometry resizes require Control permission.
- **Resolution:** Explicitly disclosed that terminal resize commands are not gated by control permissions in text or grid handlers, so View clients can resize local terminals. Documented that Control permission gates keystrokes, worktrees, workspace switching, and interrupt signals (VINTR byte 0x03 sent to the PTY line discipline).

### P:73 - View permission boundaries and repository tree browsing
- **Sources inspected:** `src-tauri/src/remote/server.rs:1191-1197,1477-1485,1850-1858,2132-2162`.
- **Finding:** Presented View as completely read-only and implied repository files cannot be accessed.
- **Resolution:** Clarified that while View clients have keystrokes discarded, they can resize terminals and revoke their own tokens. Noted that although the HTTP router has no tree-browsing endpoints, Control sessions can run shell file commands.

### P:75 - Push notification subscriptions and uncalled delivery
- **Sources inspected:** `src-tauri/src/remote/server.rs:2102-2129`, `src-tauri/src/remote/push.rs:16-63`.
- **Finding:** Claimed notification endpoints receive task state alerts with agent details.
- **Resolution:** Removed the delivery guarantee. Clarified that the server accepts and stores push subscriptions in memory, but production code has no sending caller or dispatch mechanism to deliver push alerts.

### P:79 - Pairing scope and bearer transport protection
- **Sources inspected:** `src-tauri/src/remote/server.rs:325-346,332-378,2139-2162`.
- **Finding:** Stated that cryptographic pairing protects every remote connection.
- **Resolution:** Clarified that pairing authorizes access to protected session endpoints, but does not encrypt plain LAN transport or gate public routes like health and pair exchange.

### P:81 - Lazy machine identity creation and filesystem permissions
- **Sources inspected:** `src-tauri/src/remote/auth.rs:75-104,868-910`, `src-tauri/src/daemon/server.rs:1918-1923`, `src-tauri/src/remote/server.rs:332-346,2287-2293`.
- **Finding:** Stated identity is created on first run with universal 0600 permissions, using `/bin/hostname`.
- **Resolution:** Documented that identity creation is lazy, occurring on first pair exchange, identity lookup, or Relay startup. Corrected hostname resolution to `PATH` lookup with fallback to "Ferryx machine". Noted that 0700/0600 permissions apply on initial Unix write without retroactively updating permissions on read, and that Windows uses filesystem ACLs.

### P:82-83 - Pairing PIN numeric bounds, expiry, and in-memory failure budget
- **Sources inspected:** `src-tauri/src/remote/auth.rs:196,269-286,374-393,446-467`, `src-tauri/src/remote/relay_client.rs:131-155`.
- **Finding:** Stated a single PIN generation range and portrayed the 5-attempt failure budget as a permanent brute-force lockout.
- **Resolution:** Distinguished local PINs (100000 to 999999) from relay PINs (000000 to 999999). Clarified that the 5-attempt budget operates within an in-memory window that resets on window expiration or daemon restart, returning HTTP 429 rather than providing a permanent lockout.

### P:84-85 - Device bearer tokens and persisted pairing codes
- **Sources inspected:** `src-tauri/src/remote/auth.rs:208-227,289-298,395-423,545-573,735-779`, `ui/src/lib/remoteClient.ts:6-30`.
- **Finding:** Stated only approved devices are stored in `remote-auth.json`, implying PINs are memory-only.
- **Resolution:** Documented that `remote-auth.json` stores bearer tokens, approved devices, and active expiring pairing codes and relay capabilities on disk. Saves are best-effort; expiry does not guarantee immediate disk erasure and failed saves can leave older records.

### P:86 - Single-use WebSocket connection tickets
- **Sources inspected:** `src-tauri/src/remote/server.rs:254-323`, `src-tauri/src/remote/state.rs:298`.
- **Finding:** Supported with transport qualification.
- **Resolution:** Described single-use tickets as short-lived tokens minted by authenticated clients to avoid long-lived credentials in query strings, noting ticket issuance is plain HTTP unless using TLS.

### P:87-88 - Token replacement, idle expiration, and revocation signaling
- **Sources inspected:** `src-tauri/src/remote/auth.rs:199,482-501,536-563,648-713,797-805`, `src-tauri/src/remote/server.rs:1211-1243`.
- **Finding:** Omitted token replacement on re-pairing and implied manual file deletion immediately terminates streams.
- **Resolution:** Added disclosure that re-pairing the same installation ID replaces and revokes previous tokens. Documented that revocation signals active WebSocket streams via in-memory broadcast.

### P:90-93 - Credential directory resolution and unresolved base behavior
- **Sources inspected:** `src-tauri/src/remote/auth.rs:37-73,868-875`, `src-tauri/src/cli.rs:190-218`, `src-tauri/src/remote/server.rs:332-346,2287-2293`.
- **Finding:** Claimed pairing state stays in memory if the directory does not exist.
- **Resolution:** Clarified that missing directories are created automatically on write with 0700 permissions. State remains in memory only when no per-user base path can be resolved from the environment, in which case CLI operations fail unless `FERRYX_DATA_DIR` is supplied.

### P:97 - External service scenarios and nonexclusive outbound traffic
- **Sources inspected:** `src-tauri/src/terminal/remote.rs:188-200`, `src-tauri/src/remote/relay_client.rs:538-549`, `src-tauri/src/ipc/browser.rs:925-934`, `src-tauri/src/daemon/server.rs:1320-1380`.
- **Finding:** Framed external connections as an exclusive list, omitting SSH sessions and Relay tunnels.
- **Resolution:** Replaced the exclusive claim with operational context examples including SSH sessions and Relay tunnels. Emphasized that setting gateway mode to Off does not block SSH connections, webviews, or updater checks.

### P:99 - Software updater and Microsoft Store execution paths
- **Sources inspected:** `src-tauri/tauri.conf.json:34-40`, `ui/src/App.tsx:260-262`, `ui/src/lib/updater.ts:54-63,154-180`, `src-tauri/src/ipc/updater.rs:5-18`.
- **Finding:** Supported application behavior; Store builds identified by path detection.
- **Resolution:** Documented hourly release queries for direct builds, and path detection (`\windowsapps\`) that delegates updates to the Microsoft Store.

### P:100 - Coding agent processes and external credential handling
- **Sources inspected:** `src-tauri/src/daemon/server.rs:1320-1380`.
- **Finding:** Presented agent credential handling as a Ferryx guarantee rather than external tool behavior.
- **Resolution:** Clarified that agent CLIs run as child processes or report over local sockets, with network calls and credentials governed by those tools and their providers.

### P:101 - Embedded browser webviews and external site requests
- **Sources inspected:** `src-tauri/src/ipc/browser.rs:925-934`.
- **Finding:** Supported browser webview behavior.
- **Resolution:** Documented that navigating to external URLs sends standard HTTP requests to third-party sites, where cookie and session handling is controlled by those sites.

### P:105 - Off mode gateway shutdown and active stream termination bounds
- **Sources inspected:** `src-tauri/src/daemon/server.rs:2376-2389`, `src-tauri/src/remote/server.rs:2179-2196,2225-2237`.
- **Finding:** Implied switching to Off immediately severs all active connections.
- **Resolution:** Clarified that switching to Off stops the listener and aborts relay tasks, using graceful shutdown that does not guarantee immediate forced termination of active local WebSocket streams.

### P:106 - Device revocation methods and CLI pairing capabilities
- **Sources inspected:** `src-tauri/src/cli.rs:152-170,221-289`, `src-tauri/src/ipc/remote.rs:354`, `src-tauri/src/remote/server.rs:1175-1208`, `ui/src/components/settings/RemoteAccessSection.tsx:252-263`.
- **Finding:** Ambiguity regarding CLI revocation capabilities.
- **Resolution:** Documented that device revocation is supported through the desktop settings UI and HTTP API, while the CLI tool supports listing, generating, and approving pairings without a revoke command.

### P:107 - Browser history clearing limits
- **Sources inspected:** `ui/src/lib/browserHistory.ts:90-98`, `ui/src/components/settings/BrowserSection.tsx:239-242`.
- **Finding:** Omitted webview storage boundaries.
- **Resolution:** Specified that clearing history deletes application history records, without clearing external webview cookies or site storage.

### P:110-111 - Non-exhaustive application persistence cleanup and localStorage
- **Sources inspected:** `src-tauri/src/daemon/server.rs:44,126-154,270-298,301-361,861-873`, `src-tauri/src/remote/auth.rs:37-73`, `src-tauri/src/cli.rs:191`, `ui/src/App.tsx:2826`, `ui/src/lib/remoteClient.ts:17`.
- **Finding:** Listed incomplete paths for persistence cleanup and omitted runtime directories, SSH hosts, and overrides.
- **Resolution:** Provided platform-specific deletion paths including `~/.ferryx`, `~/.local/share/rorca`, `com.ferryx.app` app data, `/tmp/rorca-<UID>`, macOS Application Support paths, Windows `%LOCALAPPDATA%` and `%APPDATA%` locations, and environment overrides (`FERRYX_DATA_DIR`, `FERRYX_SESSION_DIR`, `FERRYX_RUNTIME_DIR`). Added explicit guidance to stop running processes before deletion, to clear webview `localStorage` for UI state, and noted that cleanup is non-exhaustive regarding Git worktrees and external tool caches.
