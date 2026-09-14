# Documentation Repair and Source Anchor Evidence

## Executive Summary

This record documents the factual corrections and source anchoring performed on two primary deliverables:
1. `docs/HEADLESS_LINUX_SERVER_DEPLOYMENT_GUIDE.md`
2. `site/src/content/docs/privacy.md`

Audited claims were verified against the codebase in worktree `/Users/indo/code/project/orca-lite-wt/sa-docs`. The eleven known discrepancies and their surrounding sections were mapped to concrete source lines, resolving factual errors and qualifying broad guarantees pending exhaustive repository-wide ledger coverage.

---

## Confirmation and Resolution of Initial Known Findings

### 1. Plain LAN Axum Listener Protocol
* **Previous Inaccuracy:** The privacy declaration stated that traffic between the host and remote clients travels over HTTPS and secure WebSockets.
* **Source Reality:** In `src-tauri/src/remote/server.rs:2209-2226`, the daemon binds a standard TCP listener (`tokio::net::TcpListener::bind`) and serves requests with `axum::serve` without TLS. Direct local network and loopback connections use plain HTTP and WebSockets (`http://` and `ws://`). Only outbound connections in relay mode use TLS (`src-tauri/src/remote/server.rs:2275`).
* **Correction Applied:** Updated `site/src/content/docs/privacy.md` and `docs/HEADLESS_LINUX_SERVER_DEPLOYMENT_GUIDE.md` to state that direct LAN connections use unencrypted HTTP/WS, advising operators on untrusted networks to place a TLS terminating reverse proxy in front of port 43821.

### 2. Software Update Background Probes
* **Previous Inaccuracy:** The privacy declaration claimed that in Off mode no traffic leaves the machine, and that network requests never happen behind the user's back during local terminal use.
* **Source Reality:** In native desktop builds, `ui/src/App.tsx:261` calls `startUpdatePolling()`, which queries `https://github.com/Indosaram/ferryx/releases/latest/download/latest.json` (`src-tauri/tauri.conf.json:36`, `ui/src/lib/updater.ts:167`) on an hourly interval. Update checks are disabled only when updates are managed externally (`src-tauri/src/ipc/updater.rs:6`).
* **Correction Applied:** Scoped the zero-telemetry and Off-mode claims in `privacy.md` to clarify that periodic software update checks run on desktop builds unless running an externally managed release such as the Microsoft Store.

### 3. CLI Pairing Subcommand Scope and Revocation
* **Previous Inaccuracy:** The documentation claimed that users can revoke paired devices through either the interface or the CLI.
* **Source Reality:** `src-tauri/src/cli.rs:152` explicitly restricts subcommands: `PAIR_USAGE = "expected ferryx pair <list|generate|--generate-pin|approve <pin>>"`. The CLI does not provide a revoke command. Device revocation is implemented in the UI and IPC layer (`src-tauri/src/ipc/remote.rs:354`, `remote_revoke_device`).
* **Correction Applied:** Updated both documents to specify that the CLI supports `list`, `generate`, and `approve`. Clarified that device revocation requires the UI or IPC API.

### 4. Local Storage and Data Cleanup Boundaries
* **Previous Inaccuracy:** The documentation claimed that deleting `~/.ferryx` and `~/.local/share/rorca` removes all local configuration, session checkpoints, and keys.
* **Source Reality:** Client interface preferences and authentication bearer tokens are stored in browser and webview `localStorage` (`ui/src/App.tsx:2826`, `ui/src/lib/remoteClient.ts:17`, `ui/src/lib/terminalSettings.ts:316`). Furthermore, macOS uses `~/Library/Application Support/rorca` (`src-tauri/src/daemon/server.rs:341`), Windows uses `%LOCALAPPDATA%\Ferryx` and `%APPDATA%\rorca` (`:348`), and environment variables can override data paths (`FERRYX_DATA_DIR`, `FERRYX_SESSION_DIR`).
* **Correction Applied:** Documented the exact storage paths across platforms and explicitly noted that deleting filesystem data directories does not clear webview or browser `localStorage`.

### 5. Pinned Ghostty Submodule Requirement in Source Builds
* **Previous Inaccuracy:** The headless deployment guide provided source build instructions with `git clone` but omitted submodule initialization.
* **Source Reality:** `src-tauri/Cargo.toml:49` activates the `native-terminal` feature by default. Building the release binaries invokes `build_ghostty::build_ghostty_vt` (`src-tauri/build.rs:36`), which compiles vendored Ghostty source. The build script mandates that `src-tauri/vendor/ghostty` is initialized at pinned commit `6a508fd5e34c7e222c052a6d00bb3891ff3feace` (`src-tauri/native_terminal/build_ghostty.rs:6`) and requires Zig `0.16.0` (`:7`).
* **Correction Applied:** Added `git submodule update --init --recursive` to Option C build instructions and documented the default feature dependency on Ghostty.

### 6. Cargo Manifest Output Target Path
* **Previous Inaccuracy:** The deployment guide instructed installing binaries from `target/release/ferryx`.
* **Source Reality:** Because the repository does not define a root Cargo workspace, running `cargo build --manifest-path src-tauri/Cargo.toml` emits artifacts into `src-tauri/target/release/`.
* **Correction Applied:** Updated the installation commands in the guide to reference `src-tauri/target/release/ferryx` and `src-tauri/target/release/ferryx-cli`.

### 7. Systemd Service ExecStart Path Mismatch
* **Previous Inaccuracy:** The deployment guide provided systemd units using `ExecStart=/usr/local/bin/ferryx --daemon`, while documenting that Debian packages install to `/usr/bin/ferryx`.
* **Source Reality:** The `.deb` package installs to `/usr/bin/ferryx`, whereas source compilation or AppImage extractions place binaries in `/usr/local/bin/ferryx`.
* **Correction Applied:** Added clear explanatory notes and comments in both user and system unit examples, directing operators to configure `/usr/bin/ferryx` for package installs and `/usr/local/bin/ferryx` for manual builds.

### 8. Readiness Signal Precedes Remote Gateway Restoration
* **Previous Inaccuracy:** The deployment guide stated that `FERRYX_DAEMON_READY` is emitted after all initialization completes, including starting the agent state listener and restoring the remote gateway.
* **Source Reality:** In `src-tauri/src/daemon/server.rs:1493-1505`, `ready_tx.send(())` occurs immediately after binding `/tmp/rorca-<UID>/daemon.sock` and restoring session routes. This causes `FERRYX_DAEMON_READY` to print to stdout (`src-tauri/src/cli.rs:503`) before the daemon calls `spawn_agent_state_listener()` (`:1498`) and before it calls `handle_remote_configure()` (`:1500`).
* **Correction Applied:** Documented the exact execution order, clarifying that `FERRYX_DAEMON_READY` verifies that the local UDS socket listener has bound.

### 9. Remote Status Command Reads Persisted JSON
* **Previous Inaccuracy:** The guide implied that `ferryx remote status` reports the live operational state of the running daemon gateway.
* **Source Reality:** In `src-tauri/src/cli.rs:361-391`, `remote_status_output()` reads `~/.ferryx/remote/remote-config.json` directly from disk (`:362`). The command outputs `status=ok` along with the configured port and mode without querying the running daemon over IPC. It still returns `status=ok` even if the daemon is stopped or if the gateway failed to bind due to network interface errors.
* **Correction Applied:** Added a dedicated explanation contrasting persisted status with live socket health, providing operational verification commands (`ss -tulpn`, `curl`, and `journalctl`).

### 10. Remote Gateway Restore Failure Non-Fatal Behavior
* **Previous Inaccuracy:** The documentation did not state what happens when the remote gateway fails to bind during daemon startup.
* **Source Reality:** In `src-tauri/src/daemon/server.rs:1502-1504`, if `handle_remote_configure` returns an error, the daemon logs `tracing::warn!("Failed to restore daemon remote gateway listener: {error}")` and continues running its client accept loop. It does not abort startup or exit.
* **Correction Applied:** Documented this resilient startup behavior in both the daemon mechanics overview and troubleshooting sections.

### 11. User ID Verification Checks Real UID Not Effective UID
* **Previous Inaccuracy:** The guide stated that the daemon checks runtime directory ownership against the process effective UID.
* **Source Reality:** In `src-tauri/src/daemon/server.rs:135` and `:439`, the code calls `libc::getuid()`, which returns the real user ID, not the effective UID (`geteuid`).
* **Correction Applied:** Corrected the ownership validation explanation in Section 1, Section 4, and Section 10 of the deployment guide.

---

## Operational Recommendations versus Implementation Facts

To maintain strict boundaries between code guarantees and operational advice, prose across both deliverables was categorized:

| Category | Description | Examples in Deliverables |
|---|---|---|
| **Implementation Facts** | Mechanics verified directly in Rust or TypeScript source code. | Fixed port 43821 (`REMOTE_GATEWAY_PORT`), real UID check (`libc::getuid`), plain TCP Axum listener (`TcpListener`), 60s pairing timeout (`PAIRING_EXPIRY`), 512 KiB buffer (`DEFAULT_BUFFER_CAPACITY`). |
| **Operational Recommendations** | External administrative practices not enforced by the binary. | Configuring UFW/firewalld/nftables rules, enabling systemd user lingering (`loginctl enable-linger`), terminating TLS via external reverse proxies (Caddy/Nginx). |
| **Withheld Guarantees** | Claims removed or withheld due to lack of codebase support. | Blanket claims of no background network requests (update checks exist), claims of HTTPS for LAN listener, claims of CLI device revocation, claims of end-to-end zero-knowledge encryption. |

---

## Historical Verification Records (Initial Pass)

### 1. Initial Source Anchor Verifier Output
Command executed:
```bash
node scripts/verify-source-anchors.mjs
```
Verbatim result:
```text
OK  144 anchors verified across 3 deliverables
```

### 2. Initial Website Static Build Output
Command executed:
```bash
cd site && bun run build
```
Verbatim result:
```text
$ astro build
[content] Syncing content
[types] Generated 228ms
[build] output: "static"
[build] mode: "static"
[build] Collecting build info...
[build] ✓ Completed in 367ms.
[build] Building static entrypoints...
[vite] ✓ built in 1.42s
[build] ✓ Completed in 1.43s.
[vite] transforming...
[vite] ✓ 3310 modules transformed.
[vite] rendering chunks...
[vite] ✓ built in 1.55s
generating static routes
▶ @astrojs/starlight/routes/static/index.astro
  ├─ /privacy/index.html (+8ms)
[build] 16 page(s) built in 4.28s
[build] Complete!
```

---

## Lead Review Residual Corrections (2026-09-13)

Following lead review, eight concrete adjustments were applied:

1. **Update Check Exclusivity and Artifact Signing:** Removed exclusivity language claiming update checks are the only network requests. Clarified that checks target signed update artifacts (`src-tauri/tauri.conf.json:36`, `ui/src/App.tsx:261`, `ui/src/lib/updater.ts:167`) rather than asserting endpoint signing.
2. **Revocation Manual Edit Removal:** Removed unverified suggestions to edit `remote-auth.json` manually for revocation. Restricted documented revocation mechanisms to the supported user interface and API (`src-tauri/src/ipc/remote.rs:354`).
3. **Runtime Sockets and Cleanup Paths:** Removed instructions advising deletion of active `/tmp/rorca-<UID>` runtime sockets. Explicitly documented standard paths, fallback paths (`~/.rorca` at `src-tauri/src/daemon/server.rs:322`), and environment overrides (`FERRYX_DATA_DIR` at `src-tauri/src/cli.rs:191`, `FERRYX_SESSION_DIR` at `src-tauri/src/daemon/server.rs:44`) without asserting exhaustive cleanup.
4. **Socket Tickets, Revocation Scope, Tree Transmission, Analytics:**
   - Single-use socket tickets: Anchored to `src-tauri/src/remote/server.rs:254` and `src-tauri/src/remote/state.rs:298`, explaining they keep permanent bearer tokens out of URL query strings rather than making sweeping claims about all server logs.
   - Revocation connection closure: Anchored to `src-tauri/src/remote/auth.rs:693` and `src-tauri/src/remote/server.rs:1233`, describing token invalidation and active WebSocket closure signals rather than universal severance guarantees.
   - Route exposure: Anchored to router configuration at `src-tauri/src/remote/server.rs:2150`, noting exposed session and terminal routes without endpoints for repository tree browsing.
   - Website analytics: Narrowed marketing site declaration to state that analytics tags are disabled by default and render only if build variables are set.
5. **Ledger Attribution:** Updated `docs/SOURCE_ANCHORS_VERIFICATION.md` introductory text to attribute rows 15 through 32 to the authoring node pending lead review, rather than asserting supervising session verification for new rows.
6. **Ledger Coverage Qualification:** Replaced claims of exhaustive ledger mapping with qualified statements pending repo-wide coverage.
7. **Readiness Signal Precision:** Replaced "ready for IPC calls" with "bound local socket listener", recognizing that the accept loop starts after remote gateway restoration.
8. **Final Check Capture:** Ran verifier and site build once following all edits, preserving historical initial-pass output above.

---

## Final Verification Records (Post-Review Pass)

### 1. Final Source Anchor Verifier Output
Command executed:
```bash
node scripts/verify-source-anchors.mjs
```
Verbatim result:
```text
OK  160 anchors verified across 3 deliverables
```

### 2. Final Website Static Build Output
Command executed:
```bash
cd site && bun run build
```
Verbatim result:
```text
$ astro build
[content] Syncing content
[content] Synced content
[types] Generated 195ms
[build] output: "static"
[build] mode: "static"
[build] directory: /Users/indo/code/project/orca-lite-wt/sa-docs/site/dist/
[build] Collecting build info...
[build] ✓ Completed in 325ms.
[build] Building static entrypoints...
[vite] ✓ built in 1.43s
[build] ✓ Completed in 1.44s.
building client (vite)
[vite] transforming...
[vite] ✓ 3310 modules transformed.
[vite] rendering chunks...
[vite] computing gzip size...
[vite] ✓ built in 1.54s
generating static routes
▶ @astrojs/starlight/routes/static/index.astro
  ├─ /privacy/index.html (+10ms)
[build] 16 page(s) built in 4.16s
[build] Complete!
```

---

## Residual Limitations

1. **Accept Loop vs Bound Socket:** `FERRYX_DAEMON_READY` confirms that the local Unix domain socket listener has successfully bound and session routes are adopted (`src-tauri/src/daemon/server.rs:1493`). Client connections queue in the socket backlog while the process proceeds to initialize the agent state listener and attempt remote gateway restoration, before the `listener.accept()` loop begins servicing connections.
2. **Selective Anchor Checking:** `verify-source-anchors.mjs` checks every syntactically formatted `file:line` citation across deliverables. It does not enforce exhaustiveness over every factual claim made in unanchored prose.
3. **Pending Lead Review on Extended Rows:** In `SOURCE_ANCHORS_VERIFICATION.md`, rows 1-14 were verified by the supervising session; rows 15-32 were added by the authoring node and await lead verification.
