---
title: Self-Hosted Relay Operations Guide
description: Complete operations guide for building, deploying, configuring, and maintaining your own private Ferryx relay and account service.
---

Ferryx connects remote clients and headless daemons over WebSocket tunnels brokered by `ferryx-relay`. You can run your own relay instance on any standard Linux server, VPS, or container environment.

This guide provides an end-to-end operational reference for building the binary from source, deploying it securely behind a reverse proxy with TLS, managing environment variables, enrolling desktop and headless daemons, connecting mobile browsers, and maintaining daemon uptime.

## Architecture and Separation of Concerns

The Ferryx network topology separates authentication authority from data transport:

```text
+-------------------------------------------------------------+
|                     REMOTE CLIENT                           |
|       (Mobile browser, desktop remote view, or web)         |
+------------------------------+------------------------------+
                               |
            1. Authenticate &  |  3. Attach via WebSocket
            Request Grant      |     /tunnel/client/:sessionId
                               v
+-------------------------------------------------------------+
|                 FERRYX SELF-HOSTED STACK                     |
|                                                             |
|  +-------------------------------------------------------+  |
|  |             Account Service (Authority)               |  |
|  |  * Origin: FERRYX_ACCOUNT_ORIGIN                      |  |
|  |  * Routes: /api/account/v1/*                          |  |
|  |  * Storage: FERRYX_ACCOUNT_DATA_DIR                   |  |
|  |  * Magic link email / webhook / file mailer           |  |
|  |  * Ed25519 signing key for device grants              |  |
|  +---------------------------+---------------------------+  |
|                              | Advertises relay origin      |
|                              v                              |
|  +-------------------------------------------------------+  |
|  |              Relay Server (Transport)                 |  |
|  |  * Origin: FERRYX_ACCOUNT_RELAY_ORIGIN                |  |
|  |  * Control: GET /tunnel/control                       |  |
|  |  * Data:    GET /tunnel/data/:sessionId               |  |
|  |  * Client:  GET /tunnel/client/:sessionId             |  |
|  |  * Machine token / Ed25519 challenge auth             |  |
|  +-------------------------------------------------------+  |
+------------------------------^------------------------------+
                               |
                               | 2. Outbound control tunnel &
                               |    per-session data channels
                               |
+------------------------------+------------------------------+
|                   FERRYX DAEMON HOST                         |
|            (Desktop machine or headless server)             |
+-------------------------------------------------------------+
```

### The Two Origins

1. **Account Origin (`FERRYX_ACCOUNT_ORIGIN`)**: The canonical web address where users sign in, receive magic links, register machines, and obtain signed grant offers.
2. **Relay Origin (`FERRYX_ACCOUNT_RELAY_ORIGIN`)**: The public WebSocket endpoint where daemons maintain outbound control tunnels and clients attach to active terminal sessions.

In a unified single-binary deployment, both roles run inside `ferryx-relay` on port 8787. In that setup, both origins can share the same domain (e.g., `https://relay.example.com`). If you run the standalone `ferryx-account` service on an isolated machine, set `FERRYX_ACCOUNT_RELAY_ORIGIN` to point the account service to the independent relay host.

---

## Building and Installing the Binaries

The repository provides two dedicated server binaries defined in `src-tauri/Cargo.toml`:

- `ferryx-relay`: Unified server containing both the tunnel proxy and the embedded account service router.
- `ferryx-account`: Standalone account management service (if run independently).

### Prerequisites

- Rust stable (at least 1.82), Zig 0.16.0, and initialized repository submodules for the pinned Ghostty build
- The Linux system packages required by Tauri v2, including GTK 3 and WebKitGTK 4.1, plus `pkg-config` and a C compiler; the relay shares the Tauri crate's dependencies

### Compilation

Build the release binaries from the project root:

```bash
# Build the unified relay binary
cargo build --manifest-path src-tauri/Cargo.toml --release --bin ferryx-relay

# Build the headless client CLI if needed (from the same checkout)
cargo build --manifest-path src-tauri/Cargo.toml --release --bin ferryx-cli
```

The compiled binaries will be located at:
- `src-tauri/target/release/ferryx-relay`
- `src-tauri/target/release/ferryx-cli`

Install `ferryx-relay` to `/usr/local/bin`:

```bash
sudo install -m 755 src-tauri/target/release/ferryx-relay /usr/local/bin/ferryx-relay
```

---

## Configuration and Environment Variables

`ferryx-relay` reads configuration from command-line arguments and environment variables:

| Variable / Option | CLI Argument | Default | Description |
| :--- | :--- | :--- | :--- |
| `FERRYX_DEPLOYMENT_MODE` | none | unset (required) | Deployment mode: must be set explicitly to `selfhost` for private relays or `commercial` for managed relays. |
| `FERRYX_RELAY_PORT` | `--port <u16>` | `8787` | TCP port for the HTTP and WebSocket listener. |
| `FERRYX_ACCOUNT_ORIGIN` | none | `https://relay.checka.cc` | Canonical public HTTPS URL for account auth and magic links. |
| `FERRYX_ACCOUNT_RELAY_ORIGIN` | none | Value of `FERRYX_ACCOUNT_ORIGIN` | Public WebSocket relay URL advertised in machine records. |
| `FERRYX_ACCOUNT_DATA_DIR` | none | `~/.ferryx/account-data` | Directory where signing keys, accounts, and mail are stored. |
| `FERRYX_RELAY_MACHINE_TOKENS` | `--machine-token <tok>` | (empty) | Comma-separated list of pre-shared machine tokens. |
| `FERRYX_RELAY_ACCOUNT_PUBLIC_KEY` | `--account-public-key <key>` | unset | Base64-encoded public key of the embedded account authority to enable signed grant submission; unset leaves that route closed. |
| `FERRYX_RELAY_DATA_DIR` | none | `~/.ferryx/relay` | Durable machine key registry; override for a dedicated service user. |
| `FERRYX_UI_DIST_DIR` | none | packaged/build-dependent | Directory containing the built remote web app (`ui/dist`); required when the server binary has no bundled UI assets. |

### Deployment Mode Requirement (`FERRYX_DEPLOYMENT_MODE=selfhost`)

`ferryx-relay` requires an explicit deployment mode so that server operators and users have clear guarantees regarding billing, license checks, and network boundaries:

- **Self-Hosted Mode (`FERRYX_DEPLOYMENT_MODE=selfhost`)**:
  - **Required for all private and internal relay deployments.**
  - Completely disables commercial billing routes (`/api/account/v1/billing/*`), Lemon Squeezy webhooks, machine quotas, and license suspension checks.
  - All connected daemons and clients operate with full capabilities perpetually free of charge.
  - If omitted on startup, `ferryx-relay` will exit immediately with an error instructing the operator to declare `FERRYX_DEPLOYMENT_MODE=selfhost` or `FERRYX_DEPLOYMENT_MODE=commercial`.

The deployment mode is read from the environment only — there is no CLI flag for it, and an
unrecognized argument fails startup with `unrecognized argument: <arg>` and exit status 2. Set
the variable for the service or shell that launches the relay:

```bash
# Linux / macOS
export FERRYX_DEPLOYMENT_MODE="selfhost"
ferryx-relay --port 8787
```

```powershell
# Windows PowerShell
$env:FERRYX_DEPLOYMENT_MODE = "selfhost"
ferryx-relay --port 8787
```

```bat
REM Windows Command Prompt
set FERRYX_DEPLOYMENT_MODE=selfhost
ferryx-relay --port 8787
```

### Machine Authentication Modes

You can authenticate daemons connecting to `/tunnel/control` in two ways:

1. **Operator machine tokens**: Set `FERRYX_RELAY_MACHINE_TOKENS` on the relay and set a distinct `FERRYX_MACHINE_TOKEN` on each daemon. Never share a token between hosts: a later control connection evicts the earlier one. Keep tokens out of URLs and logs.
2. **Account enrollment and machine identity**: Sign in on the desktop and issue an enrollment code, then run `ferryx-cli account enroll --code ... --origin ...` on the daemon host. Alternatively, `ferryx-cli account login --email ... --origin ...` starts a headless login using an email link. When the Ed25519 machine identity matches the enrollment record, the relay accepts its control connection without `FERRYX_MACHINE_TOKEN`. The warning printed when no static tokens are configured does not disable this account-enrolled route.

### Mail Delivery Setup

When users log in, the account service generates a time-limited magic link. Three mailer backends are supported via environment variables:

#### 1. Resend API
```bash
export RESEND_API_KEY="re_123456789"
export FERRYX_MAIL_FROM="Ferryx <login@yourdomain.com>"
```

#### 2. HTTP Webhook
The service issues an HTTP POST request with `{"to": "user@example.com", "url": "https://..."}`:
```bash
export FERRYX_MAIL_WEBHOOK_URL="https://hooks.yourdomain.com/send-email"
export FERRYX_MAIL_WEBHOOK_TOKEN="optional-secret-bearer-token"
```

#### 3. File Mailer (Default Fallback)
If neither Resend nor a webhook is configured, the server creates one `.txt` file per login request under `$FERRYX_ACCOUNT_DATA_DIR/mail/`:
```bash
# Example file path: /var/lib/ferryx/account/mail/magic-link-<timestamp>-<uuid>.txt
```
Permissions are locked down to `0600`. Administrators can inspect the directory or attach a custom watcher script to distribute links.

---

## Production Deployment with Systemd

Create a dedicated system user and data directory:

```bash
sudo useradd -r -s /bin/false -d /var/lib/ferryx ferryx
sudo mkdir -p /var/lib/ferryx/account-data /var/lib/ferryx/relay
sudo chown -R ferryx:ferryx /var/lib/ferryx
sudo chmod 700 /var/lib/ferryx/account-data /var/lib/ferryx/relay
```

Create `/etc/systemd/system/ferryx-relay.service`:

```ini
[Unit]
Description=Ferryx Self-Hosted Relay & Account Service
After=network.target

[Service]
Type=simple
User=ferryx
Group=ferryx
WorkingDirectory=/var/lib/ferryx
Environment=FERRYX_DEPLOYMENT_MODE=selfhost
Environment=FERRYX_RELAY_PORT=8787
Environment=FERRYX_ACCOUNT_ORIGIN=https://relay.example.com
Environment=FERRYX_ACCOUNT_RELAY_ORIGIN=https://relay.example.com
Environment=FERRYX_ACCOUNT_DATA_DIR=/var/lib/ferryx/account-data
Environment=FERRYX_RELAY_DATA_DIR=/var/lib/ferryx/relay
Environment=FERRYX_UI_DIST_DIR=/var/lib/ferryx/ui-dist
# Supply a mail delivery provider for users outside this host, through a
# private EnvironmentFile (RESEND_API_KEY + FERRYX_MAIL_FROM, or webhook keys).
EnvironmentFile=-/etc/ferryx/relay.env
ExecStart=/usr/local/bin/ferryx-relay
Restart=always
RestartSec=5
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
```

Enable and start the service:

Build the remote web client with `bun run --cwd ui build` and copy the resulting `ui/dist`
into `/var/lib/ferryx/ui-dist` with permissions readable by the `ferryx` service account.
Without those assets the account API can work but the browser UI will not be usable. Provide a
private `/etc/ferryx/relay.env` for mail credentials, or use the file mailer for an explicitly
local test; do not include real secrets in the unit file. Ensure port 8787 is reachable only by
the proxy on the relay host: this binary itself binds `0.0.0.0`, not `127.0.0.1`.

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now ferryx-relay
sudo systemctl status ferryx-relay
```

---

## Reverse Proxy Setup (HTTPS and WebSocket)

The relay requires HTTPS and WebSocket connection upgrades. Do not expose port 8787 directly over cleartext HTTP on public networks.

### Nginx Configuration

```nginx
map $http_upgrade $connection_upgrade {
    default upgrade;
    ''      close;
}

server {
    listen 80;
    server_name relay.example.com;
    return 301 https://$host$request_uri;
}

server {
    listen 443 ssl http2;
    server_name relay.example.com;

    ssl_certificate /etc/letsencrypt/live/relay.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/relay.example.com/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;

    # Protect internal headers
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;

    # Maximum body limit for enrollment and account operations
    client_max_body_size 10M;

    location / {
        proxy_pass http://127.0.0.1:8787;
        proxy_http_version 1.1;

        # WebSocket support
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection $connection_upgrade;

        # Disable buffering for low-latency terminal streaming
        proxy_buffering off;
        proxy_read_timeout 3600s;
        proxy_send_timeout 3600s;
    }
}
```

### Caddy Configuration

```caddy
relay.example.com {
    reverse_proxy 127.0.0.1:8787
}
```

---

## Connecting Desktop and Headless Clients

### 1. Desktop App GUI Configuration

1. Open Ferryx Desktop.
2. Open **Settings → Remote → Access to This Machine**.
3. In **Relay / Signaling Server URL**, enter your self-hosted URL:
   ```text
   https://relay.example.com
   ```
4. Click **Apply relay URL** to enable or update the local daemon's reverse tunnel. The switch also
   enables access with the entered URL when it is off. Wait for **Relay Ready** in connection details;
   applying a value means it is configured, not that the network tunnel is connected.

The outbound account login/server address is separate: in **Remote**, set the **Account server URL**
to your own account authority before signing in. In a single-binary deployment, enter the same
`https://relay.example.com` for both. In a split deployment, enter the account URL in the account
field and the transport URL in the relay field. Changing the inbound gateway alone cannot move
already enrolled machines or account credentials to another authority.

The gateway relay URL is saved in `remote-config.json` under your operating system's application data directory. The desktop account-server URL is saved separately in local frontend storage (`ferryx.account.origin`); changing it signs out the previous account session.

### 2. Headless Daemon Setup (`remote-config.json`)

On headless Linux or Windows servers, configure the daemon without launching the GUI.
The local gateway port is fixed at `43821`: the daemon ignores any other persisted `port` value
and uses `43821` when restoring its listener. Use a separate machine for a second daemon if
that port is already occupied; changing the JSON port will not isolate it.

Locate the configuration file path:
- **Linux/macOS**: `~/.ferryx/remote/remote-config.json` (or `$FERRYX_DATA_DIR/remote/remote-config.json`)
- **Windows**: `%LOCALAPPDATA%\Ferryx\remote\remote-config.json`

Write the following JSON configuration:

```json
{
  "mode": "relay",
  "port": 43821,
  "allowControl": true,
  "relayUrl": "https://relay.example.com"
}
```

Create the `remote` directory first and ensure file permissions remain restricted
(`chmod 700 ~/.ferryx/remote && chmod 600 ~/.ferryx/remote/remote-config.json` on Linux/macOS).

Check the desired configuration using `ferryx-cli remote status` (this command reports saved gateway configuration; it does not prove the daemon's control connection is online):

```bash
ferryx-cli remote status
```

Enrollment alone does not enable the local gateway. After writing this configuration, start the
headless daemon with `ferryx-cli --daemon` on a host without another daemon using `43821`. Check
the relay account health with `curl -i https://relay.example.com/api/account/v1/health`, then
probe `https://relay.example.com/host/<machineId>/api/v1/capabilities` to confirm the control
tunnel (see [Host Capability Probe](#host-capability-probe)). For an already running daemon, use
the desktop's **Apply relay URL** control instead of launching a second daemon or killing the
process that owns live sessions.

### 3. Account Enrollment on Headless Machines

If you are using Account Enrollment rather than fixed machine tokens:

1. Sign in to your Ferryx account on your primary workstation.
2. Generate an enrollment code from the settings menu.
3. On the headless host, run:
   ```bash
   ferryx-cli account enroll --code <ENROLLMENT_CODE> --origin https://relay.example.com
   ```
   The desktop binary also supports `ferryx account enroll` if installed on the host.

To log in directly from a headless terminal without copying codes:
```bash
ferryx-cli account login --email admin@example.com --origin https://relay.example.com
```
The command waits for approval through the link sent to that email address. Start the daemon in
a dedicated service or terminal after persisting `remote-config.json`; never stop an existing live
daemon simply to apply a documentation example.

---

## Browser and Phone Access

When accessing your sessions remotely:

1. Open your phone or laptop browser and navigate to your relay URL:
   ```text
   https://relay.example.com
   ```
2. The web client loads the standalone remote interface.
3. If logging in via account, enter your email to receive a magic link pointing back to your self-hosted instance.
4. Sign in to the account hosted at that same origin, select your enrolled machine, then open its
   session. `ferryx pair generate` and `ferryx remote pair generate` no longer issue PINs; do not
   use them as onboarding steps.

The web client connects to `GET /tunnel/client/:session_id` using the negotiated session credentials.

---

## Health Checks and Authentication Probes

You can monitor the operational health of your self-hosted relay using standard HTTP tools:

### Account Service Health Probe
```bash
curl -f -s https://relay.example.com/api/account/v1/health
# Expected response: {"ok":true}
```

### Account Public Key Verification
```bash
curl -f -s https://relay.example.com/api/account/v1/public-key
# Expected response: {"publicKey":"<base64-encoded-ed25519-public-key>"}
```

### Host Capability Probe
To check whether a daemon with a known machine ID is reachable through the relay:
```bash
curl -i https://relay.example.com/host/<machineId>/api/v1/capabilities
```
- **401 Unauthorized**: The machine is connected and registered on the relay control channel, but requires valid authentication.
- **404 Not Found**: The machine is currently disconnected or has not established an active tunnel.
- **504 Gateway Timeout**: The relay attempted to contact the daemon, but the daemon's control channel timed out before responding.

---

## Operational Safety and Daemon Persistence

### Daemon Ownership of PTYs

In Ferryx, pseudoterminals (PTYs) belong strictly to the background daemon process (`ferryx --daemon`), not the GUI shell:

- Closing the desktop window, quitting the GUI app, or disconnecting a mobile web session does **not** stop running terminal sessions.
- Commands like `pkill -f ferryx` or terminating background daemon PTY master file descriptors will abruptly kill all running agent workflows, compilers, and active child shells.
- Never restart or terminate the background daemon during ordinary operations.
- Upgrade through the supported rolling-handover procedure in the [local release runbook](https://github.com/Indosaram/ferryx/blob/main/docs/releases/LOCAL_RELEASE_RUNBOOK.md); do not restart a session-owning daemon as a configuration shortcut.

### User Data Directories

Ensure the following paths are protected in your host backup strategy:

- Relay host account data: `/var/lib/ferryx/account-data` (contains user accounts, sessions, and machine records).
- Daemon host credentials: `~/.ferryx/remote/` (contains `identity.json`, `remote-config.json`, and `remote-auth.json`).

Never check `identity.json` or `signing-key.json` into source control. These files hold Ed25519 private keys that authenticate machine identities.

---

## Troubleshooting

### 1. `ACCOUNT_ORIGIN_INSECURE`
- **Cause**: `FERRYX_ACCOUNT_ORIGIN` was set to an `http://` URL on a non-loopback interface.
- **Fix**: Ferryx strictly enforces HTTPS for all remote communication outside `127.0.0.1` and `localhost`. Configure TLS on your reverse proxy and update `FERRYX_ACCOUNT_ORIGIN` to use `https://`.

### 2. `data_pairing_timeout` (HTTP 504)
- **Cause**: A client requested a connection via `/tunnel/client/:sessionId`, but the daemon did not connect to `/tunnel/data/:sessionId` within 30 seconds.
- **Fix**: Check whether the daemon host has lost its outbound network connection to the relay, or whether high system load delayed the daemon from opening its data channel. Inspect daemon logs at `~/.ferryx/logs/daemon.log`.

### 3. Magic Link Email Fails (`MAIL_FAILED`)
- **Cause**: Invalid Resend API key, unreachable webhook URL, or a missing/unwritable parent `$FERRYX_ACCOUNT_DATA_DIR` when using the file mailer.
- **Fix**: Create `$FERRYX_ACCOUNT_DATA_DIR` before starting the relay, owned and writable by its service user (for example the systemd setup above). Without an external email provider, inspect its `mail/` file spool for the generated link.

### 4. Multiple Daemons Competing for the Same Token
- **Cause**: Reusing the same pre-shared machine token across multiple hosts.
- **Fix**: Control channels are keyed to the machine token. If two hosts present the same token, the newest connection terminates the older one. Provide a distinct token for every machine in `FERRYX_RELAY_MACHINE_TOKENS`.
