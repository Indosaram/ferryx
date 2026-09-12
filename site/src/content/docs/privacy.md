---
title: Ferryx Privacy Declaration
description: "Plain declaration of Ferryx data handling: local-first storage, remote gateway transmissions, pairing tokens, and verified absence of telemetry."
lastUpdated: 2026-09-12
prev: false
next: false
---

This declaration explains how Ferryx handles your data. Every statement below reflects the actual code running in the application, its remote gateway, and the project website.

## Local-first operation

Ferryx keeps your data on your own machine. Your terminal sessions, workspaces, Git worktree configurations, and application preferences remain stored locally on disk.

Local state lives in predictable paths:

- **Session and workspace layout:** The background daemon persists running sessions to `~/.local/share/rorca/session_state.json` (or `~/.rorca/session_state.json` on fallback Unix paths).
- **Interface preferences:** Saved sidebar visibility, active workspace selections, and terminal font settings stay in browser `localStorage`.
- **Browsing history:** Embedded browser tabs record visited URLs locally only when enabled. You can toggle this setting off or clear the history list at any time.
- **SSH connections:** Configured SSH hosts live in `~/.ferryx/ssh_hosts.json`.
- **Runtime files:** A private runtime directory (`/tmp/rorca-<UID>`) protects sockets (`daemon.sock`, `agent-state.sock`) with `0700` and `0600` permissions, restricting access to your user account.

Terminal output stays in a bounded in-memory ring buffer. Monotonic sequence numbers let reattaching clients catch up on missed lines without writing your raw terminal stream to persistent logs.

## Zero telemetry

Ferryx does not collect analytics. You won't find tracking libraries, crash reporting SDKs, or background telemetry calls anywhere in the desktop codebase.

Inspecting `src-tauri` and `ui` confirms the absence of third-party metrics tooling:

- No Sentry, PostHog, Google Analytics, or similar SDKs exist in the repository.
- The desktop Content Security Policy restricts connections to local application endpoints and explicitly configured relay domains.
- Network requests don't happen behind your back during local terminal use.

On the marketing site, analytics scripts load only if the site deployer sets optional environment variables during build time. Default deployments and local builds load zero tracking tags.

## Remote gateway and network modes

Remote access lets you monitor or control terminal sessions from another device. This gateway remains inactive until you explicitly turn it on.

Four network modes determine connection behavior:

- **Off:** The server stays shut down. No network port opens, and no traffic leaves the machine.
- **LocalNetwork:** Ferryx listens on port 43821, binding loopback (`127.0.0.1`) and your local network interface. Devices on your local network can connect if paired.
- **Tailscale:** The daemon binds port 43821 to loopback and your Tailscale interface address (`100.64.0.0/10`), restricting connections to your private tailnet.
- **Relay:** An outbound secure WebSocket connects to a relay server (defaulting to `https://relay.checka.cc`). Forwarding proxies encrypted web traffic between your remote client and your local gateway.

The gateway never binds to `0.0.0.0`. Instead, it binds only specific, resolved IP addresses. You can run your own relay instance using the open-source server implementation and supply its address through settings or the `FERRYX_RELAY_URL` environment variable.

## What leaves your machine during remote access

Enabling remote access exposes specific workspace data to paired devices. Traffic between the host and remote clients travels over HTTPS and secure WebSockets.

Authorized remote clients receive:

- **Workspace metadata:** Project names, worktree branch labels, session identifiers, and process names.
- **Desktop context:** Identifiers for the currently focused workspace, tab, and pane.
- **Terminal output:** Live text, ANSI escape codes, and recent scrollback buffers from running sessions.

Remote clients can send commands back only when paired with `Control` permission:

- **Keystrokes and input:** Raw text chunks sent to the active PTY process.
- **Window geometry:** Terminal column and row resize events.
- **Control signals:** Interrupt signals (`SIGINT`) sent to running processes.
- **Worktree actions:** Requests to create or remove Git worktrees.

Clients paired with `View` permission can observe terminal output and metadata, but the server discards any input keystrokes they submit. Repository source files and directory trees don't travel over the gateway API.

When you enable web push notifications in the remote web client, notification endpoints receive task state alerts with the agent name, status label, and summary text.

## Pairing codes, tokens, and credentials

Cryptographic pairing protects every remote connection. Several defensive layers safeguard these credentials:

- **Machine identity:** The host generates a unique machine UUID and an Ed25519 keypair on first run. These reside in `identity.json` with `0600` permissions.
- **Pairing PINs:** Devices pair using a randomly generated 6-digit PIN. Each PIN expires after 60 seconds, and a strict limit of 5 failed attempts locks out brute-force attacks.
- **Device tokens:** Successful pairing mints a 64-character alphanumeric bearer token. The host stores approved devices in `remote-auth.json`, while the client stores the token in browser `localStorage`.
- **Socket tickets:** Connecting to a WebSocket requests a single-use ticket. This keeps long-lived credentials out of URLs, query parameters, and server access logs.
- **Automatic expiration:** Inactive devices expire after 30 days. Revoking a device in settings immediately severs all active connections.

On Linux and macOS, remote credentials live under `~/.ferryx/remote/`. Windows installations place them in `%LOCALAPPDATA%\Ferryx\remote\` or `%USERPROFILE%\.ferryx\remote\`. If no per-user directory exists, Ferryx holds pairing state in memory only and drops it on exit.

## External services and tools

Ferryx connects to external servers only in specific scenarios:

- **Software updates:** Direct builds check GitHub releases for signed updates. Microsoft Store builds rely entirely on the Windows Store update pipeline.
- **Coding agents:** Tools like Claude Code, Codex, or custom CLI agents make outbound network calls directly from their own processes. They communicate with provider APIs using your personal credentials, outside Ferryx's control.
- **Embedded web views:** Navigating to an external website inside a browser tab loads assets from that site. The remote site receives standard HTTP request details and manages cookies under its own rules.

## Your choices and data deletion

You retain full control over the data Ferryx stores:

- Stop remote access whenever you want by switching the gateway mode to `Off`.
- Revoke individual paired devices through the interface or CLI.
- Clear browsing history from the browser settings pane.
- Delete the `~/.ferryx` and `~/.local/share/rorca` directories to remove all local configuration, session checkpoints, and keys.
