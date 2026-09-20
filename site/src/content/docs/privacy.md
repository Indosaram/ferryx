---
title: Ferryx Privacy Declaration
description: "How Ferryx handles data: local-first storage, remote gateway traffic, pairing tokens, consent-gated website analytics, and zero desktop telemetry."
lastUpdated: 2026-09-19
prev: false
next: false
---

This declaration explains how Ferryx handles your data, based on the implementation in the desktop application, remote gateway, and project website.

## Local-first operation

Ferryx stores your data on your own machine. Local terminal sessions, workspaces, Git worktree configurations, and application preferences remain stored on disk. When you connect to remote SSH hosts, workspace files and command execution remain on the remote host (`src-tauri/src/terminal/remote.rs:183-200`).

Local state lives in predictable paths:

- **Session and workspace layout:** The background daemon persists running sessions to disk:
  - Linux checks `$XDG_DATA_HOME/rorca/session_state.json` first, falling back to `~/.local/share/rorca/session_state.json` (`src-tauri/src/daemon/server.rs:301-361`). If neither base resolves, it composes `~/.rorca/rorca/session_state.json` (`:334,359`) or writes to the runtime directory (`:329-335`). Development builds use `rorca-dev` or `session_state.dev.json` (`:325-326`).
  - macOS defaults to `~/Library/Application Support/rorca/session_state.json` (`src-tauri/src/daemon/server.rs:341-344`).
  - Windows defaults to `%APPDATA%\rorca\session_state.json` (`src-tauri/src/daemon/server.rs:346-349`).
  - You can override the session directory using `FERRYX_SESSION_DIR` (`src-tauri/src/daemon/server.rs:44`).
- **Interface preferences:** Saved sidebar visibility, active workspace selections, and terminal font settings stay in webview or browser `localStorage` (`ui/src/App.tsx:2826`, `ui/src/lib/terminalSettings.ts:90-107,313-320`).
- **Browsing history:** Embedded browser tabs record visited URLs locally in application storage only when `rememberBrowsingHistory` is enabled (`ui/src/lib/browserHistory.ts:15-20,66-98`). You can toggle this setting off or clear the history list at any time (`ui/src/components/settings/BrowserSection.tsx:239-242`). Clearing this list doesn't erase webview cookies or external site profile data.
- **SSH connections:** Configured SSH hosts live in the platform application data directory under `com.ferryx.app/ssh_hosts.json` (`src-tauri/src/ipc/ssh.rs:73-86`, `src-tauri/src/daemon/server.rs:861-873`), or under `FERRYX_DATA_DIR/ssh_hosts.json` if set. On Linux, this resolves to `$XDG_DATA_HOME/com.ferryx.app` or `~/.local/share/com.ferryx.app`.
- **Runtime files:** On Unix, the runtime directory (`/tmp/rorca-<UID>`) resolves from the process real UID via POSIX `libc::getuid` (`src-tauri/src/daemon/server.rs:126-138,434-439`), created with 0700 permissions (`:502-533`). The canonical daemon socket (`daemon.sock`) is validated and secured with 0600 permissions (`:1482-1486`). Unix agent sockets (`agent-state.sock`) apply best-effort 0600 permissions without halting startup on error (`:1345-1380`). Windows builds bind a loopback TCP listener and record the port in `daemon.port` (`:140-163,1469-1480`). These permissions restrict access to your local user account against unprivileged local users, without preventing root or administrative access.

- **Diagnostic logs:** In headless daemon mode, agent activity release reasons (`manual_reset`, `foreground_agent_to_shell`) are recorded to a bounded local log file at `~/.ferryx/logs/daemon.log` or `$FERRYX_DATA_DIR/logs/daemon.log` (`src-tauri/src/daemon/logging.rs:33-80`). This log is created with 0600 permissions, bounded to a maximum of 1 MiB (truncating oldest records when exceeded), and fed through a non-blocking 256-record queue. It records only session identifiers, previous activity states, and release triggers without logging terminal output or command history.

Terminal output stays in an in-memory ring buffer with a default capacity of 512 KiB per session (`src-tauri/src/terminal/output_hub.rs:7,100-154`). Monotonic sequence numbers let reattaching clients catch up on retained chunks without writing raw terminal streams to persistent logs (`src-tauri/src/daemon/server.rs:2890-2953`). Chunks evicted past the buffer capacity can't be replayed, and this in-memory buffer doesn't prevent running tools or agents from keeping their own logs.

## Zero desktop telemetry

The desktop application and the project website are separate: the sections below describe the app, and [Website analytics](#website-analytics) describes the site. The desktop app contains no analytics of any kind, and nothing on the website changes that.

Inspecting `src-tauri` and `ui` source code confirms the absence of desktop analytics tooling:

- Product dependency manifests contain no metrics SDKs such as Sentry, PostHog, or Google Analytics (`src-tauri/Cargo.toml:58-177`, `ui/package.json:13-55`).
- The desktop shell Content Security Policy permits local dev endpoints, internal IPC, and hardcoded `checka.cc` relay domains (`src-tauri/tauri.conf.json:30`). It doesn't dynamically adapt to arbitrary custom relays, nor does it govern native Rust requests or external webviews.
- Software update checks: In native desktop builds, Ferryx periodically queries GitHub release metadata on startup and on an hourly schedule (`ui/src/App.tsx:260-262`, `ui/src/lib/updater.ts:54-63,154-178`). When an update is downloaded, the updater verifies the artifact signature against the configured public key (`src-tauri/tauri.conf.json:34-40`, `ui/src/lib/updater.ts:108-134`). Builds running from `WindowsApps` paths skip these updater checks (`src-tauri/src/ipc/updater.rs:5-18`), leaving delivery to the Microsoft Store.

## Website analytics

This section covers `https://ferryx.dev` only. The desktop application is unaffected by every choice described here and sends no analytics either way.

The website can use Google Analytics 4, and only with your explicit permission:

- **Off until you choose:** The page ships an inert `<template>` with the consent choice and nothing else. No Google script is requested, no `dataLayer` is created, and nothing is written to `localStorage` before you press a button (`site/src/lib/analyticsRuntime.ts:100-137,230-244`).
- **Equal choice, reversible:** "Allow analytics" and "Decline analytics" are the same control with the same styling and size. Your answer is stored under `ferryx.site.analytics-consent` in `localStorage`, and an "Analytics choice" button at the end of every page reopens the panel so you can change your mind. Withdrawing consent sets `ga-disable-<measurement id>` and sends a `denied` consent update, which stops further measurement for the page (`site/src/lib/analyticsRuntime.ts:186-220`).
- **Declining costs you nothing:** Every part of the site behaves identically with analytics declined.
- **Builds without an ID measure nothing:** The tag only exists when the site is built with a valid `PUBLIC_GA_MEASUREMENT_ID` (format `G-XXXXXXXXXX`). Local development builds, forks, and previews without that variable ship no consent panel and make no Google requests (`site/src/lib/analytics.ts:53-56`, `site/src/components/SiteAnalytics.astro:11-13,112-119`).

When you allow analytics, the site records:

- **`page_view`:** Sent once per page load, replacing the automatic page view so the reported URL can be sanitized first (`site/src/lib/analyticsRuntime.ts:161-184`).
- **`download_click`:** Sent when you click a release artifact link (including `.dmg`, `.exe`, `.AppImage`, `.deb`, and versioned `ferryx-cli` artifacts), with the parameters `platform`, `asset_id`, `destination`, and `link_location` (`site/src/lib/analytics.ts`). The classifier also recognises Microsoft Store links, but the current menu offers the GitHub Windows installer, not a Store listing. Opening the GitHub releases listing page is not recorded as a download.

What is deliberately not collected:

- **URL query strings:** Before any measurement is sent, the page URL is rebuilt with only the campaign parameters `utm_source`, `utm_medium`, `utm_campaign`, `utm_term`, `utm_content`, and `utm_id`. Every other query parameter is discarded, so search terms, e-mail addresses, or tokens that ride along in a link never reach Google (`site/src/lib/analytics.ts:19-26,88-104`).
- **Advertising and cross-site signals:** Consent defaults deny `ad_storage`, `ad_user_data`, `ad_personalization`, and `analytics_storage`; accepting grants `analytics_storage` only. The tag is configured with `allow_google_signals: false` and `allow_ad_personalization_signals: false` (`site/src/lib/analyticsRuntime.ts:152-176`).
- **Outbound link rewriting:** Download links are never rewritten with tracking parameters; the URL you click is the URL in the page.

Google Analytics receives the request metadata any web request carries, including your IP address, which Google uses for coarse geolocation before discarding it, and a first-party cookie identifier set only after you accept. Clearing site data for `ferryx.dev` removes both the cookie and your stored choice, and the panel will ask again.

A self-hosted, cookieless analytics script is also supported for forks: it renders only when both `PUBLIC_ANALYTICS_SRC` and `PUBLIC_ANALYTICS_DOMAIN` build environment variables are set (`site/src/components/SiteAnalytics.astro:106-110`). Neither variable is required by, nor related to, the Google Analytics path above.

Search engine verification meta tags (`google-site-verification`, `msvalidate.01`) render only when `PUBLIC_GSC_VERIFICATION` or `PUBLIC_BING_VERIFICATION` are set at build time (`site/src/components/SiteAnalytics.astro:104-105`). They are static ownership tokens and collect nothing.

## Remote gateway and network modes

Remote access lets you monitor or control terminal sessions from another device. The gateway starts Off by default (`src-tauri/src/remote/state.rs:53-60`). You can enable it through network configuration, or by requesting a pairing code, which auto-configures and starts Relay mode if the gateway was Off (`src-tauri/src/daemon/server.rs:1948-1972`). Persisted non-Off configuration restores automatically when the daemon starts (`:1500-1505`).

Four network modes determine connection behavior:

- **Off:** The gateway shuts down its listener and aborts active relay tasks (`src-tauri/src/remote/server.rs:2266-2268`, `src-tauri/src/daemon/server.rs:2376-2389`). Turning off the gateway stops remote listening, but it doesn't block independent outbound client traffic such as SSH sessions, embedded browser tabs, or background updater queries.
- **LocalNetwork:** Ferryx binds port 43821 on loopback and a selected network interface (`src-tauri/src/remote/server.rs:2260-2344`, `src-tauri/src/remote/state.rs:194-227`). Resolution prefers a usable route-probe address, which can be public even when another interface has a private address. Paired devices can access protected session endpoints, while health, UI assets, and pair exchange routes remain unauthenticated (`src-tauri/src/remote/server.rs:325-346,2139-2160`). Direct connections use plain HTTP and WebSockets over TCP (`:2203-2238`).
- **Tailscale:** The daemon binds port 43821 to loopback and any interface address matching the CGNAT range `100.64.0.0/10` (`src-tauri/src/remote/state.rs:110-112,229-234`, `src-tauri/src/remote/server.rs:2310-2344`). Matching a CGNAT address doesn't verify Tailscale daemon health or authenticated tailnet membership; restricting access relies on network firewall rules and Tailscale ACLs. WireGuard transport encryption is handled by Tailscale, not Ferryx TLS.
- **Relay:** The daemon opens an outbound WebSocket to a relay server, defaulting to `https://relay.checka.cc` (`src-tauri/src/remote/state.rs:20`, `src-tauri/src/remote/relay_client.rs:538-549`). Connections use TLS encryption when directed to secure `https://` or `wss://` endpoints, while custom endpoints configured with `http://` or `ws://` run unencrypted.

The gateway listener never binds to the wildcard `0.0.0.0` address (`src-tauri/src/remote/server.rs:2260-2344`). Operators deploying a custom relay should set `relayUrl` in persisted `remote-config.json` for deterministic restoration across restarts, because `FERRYX_RELAY_URL` is only read during pairing auto-configuration or IPC fallback (`src-tauri/src/daemon/server.rs:1956-1962`, `src-tauri/src/remote/server.rs:2270-2293`, `src-tauri/src/ipc/remote.rs:304-312`).

## What leaves your machine during remote access

Enabling remote access exposes specific workspace data to paired devices. Traffic through the default relay uses TLS (`src-tauri/src/remote/relay_client.rs:538-549`). For direct LAN connections without a reverse proxy, the internal server listens on plain HTTP and WebSockets (`src-tauri/src/remote/server.rs:2203-2238`). Operators on untrusted networks should place a TLS-terminating proxy in front of the port.

Authorized remote clients receive:

- **Workspace metadata:** Project names, worktree branch labels, session identifiers, and optional titles (`src-tauri/src/remote/protocol.rs:180-195,232-264`, `src-tauri/src/remote/server.rs:657-767`). Process names are not sent as dedicated metadata fields.
- **Desktop context:** Identifiers for the active workspace, worktree, session, and tab, without a separate pane identifier (`src-tauri/src/remote/protocol.rs:180-195`).
- **Terminal output:** Live text, ANSI escape codes, snapshots, and recent chunk buffers from running sessions (`src-tauri/src/remote/server.rs:1450-1473,1830-1845`).

Client permissions govern available actions:

- **Keystrokes and input:** Raw text chunks sent to active PTY processes require Control permission (`src-tauri/src/remote/server.rs:1477-1485,1850-1858`).
- **Window geometry:** Terminal resize events can be sent by clients with either Control or View permission (`src-tauri/src/remote/server.rs:1511-1516,1885-1895`). The server does not gate resize commands on control permissions in text or grid handlers.
- **Control signals:** Interrupt signals (VINTR byte 0x03) require Control permission (`src-tauri/src/remote/server.rs:1520-1528,1912-1920`, `src-tauri/src/terminal/session.rs:210-215`), directing SIGINT through the PTY line discipline to the foreground process group rather than guaranteeing child process termination.
- **Worktree actions:** Creating or removing Git worktrees requires Control permission (`src-tauri/src/remote/server.rs:963-969,1090-1096`).

Clients paired with View permission have input keystrokes discarded (`src-tauri/src/remote/server.rs:1477-1485,1850-1858`), but can still trigger terminal resizes and revoke their own tokens (`:1191-1197,1511-1516,1885-1895`). The gateway HTTP router exposes routes for session streaming and metadata without endpoints for repository tree browsing (`:2132-2162`), though Control sessions can execute arbitrary shell commands inside terminals.

Remote gateway endpoints accept and store web push subscription payloads in memory (`src-tauri/src/remote/server.rs:2102-2129`, `src-tauri/src/remote/push.rs:16-63`). The codebase currently includes no dispatching caller to send push notifications to external notification services.

## Pairing codes, tokens, and credentials

Pairing authorizes access to protected session endpoints (`src-tauri/src/remote/server.rs:332-378,2148-2162`). Public endpoints like health, static assets, and pair exchange do not require tokens (`:325-346,2139-2160`), and pairing alone doesn't encrypt plain LAN transport.

Defensive layers protect pairing credentials:

- **Machine identity:** The host generates a unique machine UUID and an Ed25519 keypair lazily on the first pair exchange, identity lookup, or Relay startup (`src-tauri/src/remote/auth.rs:75-104`, `src-tauri/src/daemon/server.rs:1918-1923`, `src-tauri/src/remote/server.rs:332-346,2287-2293`). These reside in `identity.json`. Hostname discovery queries `PATH` with fallback to "Ferryx machine" (`src-tauri/src/remote/auth.rs:86-94`). Unix writes set 0700 parent and 0600 file permissions (`:868-910`), without retroactively updating permissions on read (`:75-84`). Windows relies on inherited filesystem ACLs.
- **Pairing PINs:** Local pairing generates a 6-digit PIN between 100000 and 999999 (`src-tauri/src/remote/auth.rs:374-393`), while relay pairing generates zero-padded codes from 000000 to 999999 (`src-tauri/src/remote/relay_client.rs:131-155`). Each PIN expires 60 seconds after creation (`src-tauri/src/remote/auth.rs:196`). Active PINs and relay pairing capabilities are included in best-effort saves to `remote-auth.json` (`src-tauri/src/remote/auth.rs:395-423,768-779`). Expiry rejects use; it does not guarantee immediate disk erasure, and failed saves can leave older records.
- **Rate limiting:** Failed pairing attempts are counted in an in-memory window (`src-tauri/src/remote/auth.rs:269-286,446-467`). After 5 failures, subsequent attempts return HTTP 429 until the window resets or the daemon restarts. This failure budget is not persisted across restarts.
- **Device tokens:** Successful pairing mints a 64-character alphanumeric bearer token (`src-tauri/src/remote/auth.rs:208-227,545-573`). Approved devices and tokens are stored in `remote-auth.json` (`:289-298,735-779`). Clients store the token in browser or webview `localStorage` (`ui/src/lib/remoteClient.ts:6-30`).
- **Socket tickets:** Connecting to WebSockets can use a short-lived single-use ticket minted by an authenticated client (`src-tauri/src/remote/server.rs:254-323`, `src-tauri/src/remote/state.rs:298`), avoiding long-lived tokens in query strings.
- **Expiration and revocation:** Inactive devices expire after 30 days of inactivity (`src-tauri/src/remote/auth.rs:199,648-675`). Re-pairing an existing installation replaces and revokes its previous bearer tokens (`:482-501,536-563`). Revoking a device removes its tokens and signals its active WebSocket streams to terminate (`:693-713,797-805`, `src-tauri/src/remote/server.rs:1211-1243`).

Credential storage on Linux and macOS resolves under `~/.ferryx/remote/` (`src-tauri/src/cli.rs:207`, `src-tauri/src/remote/auth.rs:55`). Windows installations place credentials in `%LOCALAPPDATA%\Ferryx\remote\` or `%USERPROFILE%\.ferryx\remote\` (`src-tauri/src/cli.rs:198`, `src-tauri/src/remote/auth.rs:49`). Operators can override this directory using `FERRYX_DATA_DIR` (`src-tauri/src/cli.rs:191`, `src-tauri/src/remote/auth.rs:41`). Missing directories are created automatically on write (`:868-875`). Pairing state remains memory-only only when no per-user base directory can be resolved from the environment (`:37-73`), in which case the CLI returns an error if run without `FERRYX_DATA_DIR` (`src-tauri/src/cli.rs:211-212`).

## External services and tools

Ferryx connects to external servers in several operational contexts:

- **Remote SSH sessions:** Outbound SSH connections connect to configured remote hosts (`src-tauri/src/terminal/remote.rs:188-200`).
- **Relay connections:** In Relay mode, the daemon establishes an outbound connection to the relay server (`src-tauri/src/remote/relay_client.rs:538-549`).
- **Software updates:** Direct native desktop builds query GitHub release metadata on an hourly schedule (`src-tauri/tauri.conf.json:34-40`, `ui/src/App.tsx:260-262`, `ui/src/lib/updater.ts:54-63,154-178`). Executables running from `WindowsApps` paths skip updater checks (`src-tauri/src/ipc/updater.rs:5-18`), leaving updates to the Microsoft Store.
- **Coding agents:** CLI tools like Claude Code, Codex, or custom agents execute as terminal child processes or report status via local sockets (`src-tauri/src/daemon/server.rs:1320-1380`). Their network traffic, credential use, and API communications depend on their own tooling and provider configurations.
- **Embedded web views:** Navigating to an external URL inside a browser tab loads assets from that site (`src-tauri/src/ipc/browser.rs:925-934`). Remote sites receive standard HTTP request details and set cookies under their own policies.

Setting the gateway mode to Off does not block SSH connections, external web views, or updater queries.

## Your choices and data deletion

You retain control over the data Ferryx stores:

- You can change or withdraw the website analytics choice at any time using the "Analytics choice" button at the end of any page on `https://ferryx.dev`. This affects the website only; the desktop application never sends analytics.
- You can stop remote access by switching the gateway mode to Off (`src-tauri/src/remote/server.rs:2266-2268`, `src-tauri/src/daemon/server.rs:2376-2389`). This stops the listener and aborts relay tasks, using graceful shutdown that doesn't forcibly sever active local WebSocket streams immediately.
- Paired devices can be revoked through the desktop settings interface (`ui/src/components/settings/RemoteAccessSection.tsx:252-263`) or HTTP API (`src-tauri/src/ipc/remote.rs:354`, `src-tauri/src/remote/server.rs:1175-1208`). The CLI tool lets you list, generate, and approve pairings (`src-tauri/src/cli.rs:152-170,221-289`), but doesn't include a revoke command.
- Clearing browsing history removes the application history list (`ui/src/lib/browserHistory.ts:90-98`, `ui/src/components/settings/BrowserSection.tsx:239-242`), but doesn't erase webview cookies or site storage.
- To remove persistent state, stop any running Ferryx processes first, then delete local paths according to your platform:
  - Linux: `~/.local/share/rorca` (or `$XDG_DATA_HOME/rorca`), `com.ferryx.app` app data (containing SSH hosts), `~/.ferryx` (containing remote auth, machine identity, and remote config), `/tmp/rorca-<UID>` (runtime socket and locks), and legacy `~/.rorca` if present (`src-tauri/src/daemon/server.rs:135,321,867`, `src-tauri/src/remote/auth.rs:55`).
  - macOS: `~/Library/Application Support/rorca`, `~/Library/Application Support/com.ferryx.app` (SSH hosts), `~/.ferryx`, and `/tmp/rorca-<UID>` (`src-tauri/src/daemon/server.rs:341,867`, `src-tauri/src/remote/auth.rs:55`).
  - Windows: `%LOCALAPPDATA%\Ferryx` (remote auth), `%APPDATA%\rorca` (sessions), `%APPDATA%\Ferryx` (persistent lock), `%APPDATA%\com.ferryx.app` (SSH hosts), `%USERPROFILE%\.ferryx`, and `%LOCALAPPDATA%\Ferryx\runtime` (`src-tauri/src/daemon/server.rs:284,348,867`, `src-tauri/src/remote/auth.rs:49`).
  - Custom overrides: Check any directories configured by `FERRYX_DATA_DIR` (`src-tauri/src/cli.rs:191`, `src-tauri/src/daemon/server.rs:863`), `FERRYX_SESSION_DIR` (`src-tauri/src/daemon/server.rs:44`), or `FERRYX_RUNTIME_DIR` (`src-tauri/src/daemon/server.rs:128`).
  - Web storage: Clear webview or browser `localStorage` (`ui/src/App.tsx:2826`, `ui/src/lib/remoteClient.ts:17`) to purge interface preferences and cached client tokens. Clearing site data for `ferryx.dev` also removes the website analytics choice stored under `ferryx.site.analytics-consent` and any Google Analytics cookie set after you accepted.
  - This cleanup list is non-exhaustive. External assets like Git worktrees created inside project directories (`.orca-worktrees`), shell command logs, and browser profile cookies remain subject to their own storage locations.
