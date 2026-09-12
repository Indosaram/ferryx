# Headless Linux Server Deployment Guide for Ferryx

This guide explains how to deploy and operate Ferryx in headless daemon mode on a Linux server without a graphical display. It documents the exact mechanics implemented in the source code, including process execution, Unix domain sockets, file locks, systemd service configuration, remote access network modes, pairing, and troubleshooting.

---

## 1. Architectural Overview

Ferryx can run in two primary launch modes:

1. **GUI Mode (Default):** Runs the full Tauri desktop application with its native window and graphical interface.
2. **Headless Daemon Mode (`--daemon`):** Launches a background Tokio multi-thread runtime without initializing any desktop windowing, GTK, or display subsystems.

The headless daemon manages long-running PTY terminal processes, workspace state, agent extension hooks, session persistence, and client connections over a local Unix domain socket (UDS). When configured, the daemon also runs the remote gateway HTTP and WebSocket server, allowing web browsers or remote Ferryx desktop clients to connect over a local network, a Tailscale tailnet, or a secure reverse relay tunnel.

### Internal Identifiers and Compatibility Paths

Ferryx maintains compatibility with earlier runtime and branch conventions. The user-facing application name is Ferryx, but several internal filesystem and protocol identifiers retain their existing naming:

* Runtime directory: `/tmp/rorca-<UID>` (where `<UID>` is the numeric user ID returned by POSIX `getuid`).
* Canonical daemon socket: `/tmp/rorca-<UID>/daemon.sock`.
* Agent state socket: `/tmp/rorca-<UID>/agent-state.sock`.
* Persistent user configuration directory: `~/.ferryx/`.
* Legacy session state fallback directory: `~/.local/share/rorca/` or `~/.rorca/`.
* Internal git branch namespace: `orca/<ws-id>/<slug>`.
* Worktree cache directory: `.orca-worktrees/`.

---

## 2. Prerequisites

### Host Environment

* **Architecture:** Linux `x86_64` or `aarch64`.
* **Init System:** `systemd` (standard user session or system service).
* **Operating Account:** A standard, non-root user account. The daemon performs strict ownership and permission validations on startup. Running as `root` is strongly discouraged because session permissions, socket modes, and git user configuration will conflict with standard user workflows.
* **Core Utilities:** `hostname` (used to generate the default machine identity display name), `git`, `curl`, and standard POSIX shell tools.

### Build and Runtime Dependencies

When building from source on Debian or Ubuntu hosts, install these packages:

```bash
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential \
  curl \
  wget \
  file \
  libssl-dev \
  libasound2-dev \
  libgtk-3-dev \
  libwebkit2gtk-4.1-dev \
  pkg-config \
  git
```

> **GTK and WebKitGTK are required for every Linux source build, including the headless
> daemon.** `src-tauri/Cargo.toml:175-177` declares `gtk` and `webkit2gtk` as unconditional
> `cfg(target_os = "linux")` dependencies of the crate, so their `-sys` crates run
> `pkg-config` even when you build only `--bin ferryx`. Omitting `libgtk-3-dev` or
> `libwebkit2gtk-4.1-dev` makes the build fail while resolving `glib-sys`, `gobject-sys`,
> `gio-sys`, `gdk-sys`, or `atk-sys`. These packages supply build-time headers only. The
> headless daemon never opens a display or a webview at runtime: `src-tauri/src/main.rs:35`
> splits `LaunchMode::Daemon` from `LaunchMode::Gui`, and the daemon arm calls
> `run_daemon_headless` (`src-tauri/src/cli.rs:471`), which builds a Tokio runtime and serves
> `DaemonServer` without constructing a Tauri app. Only the GUI arm reaches
> `tauri::Builder::default().run(...)` at `src-tauri/src/lib.rs:1829-1830`.

Bundling a Debian package or running `bun tauri build` additionally requires:

```bash
sudo apt-get install -y --no-install-recommends \
  libxdo-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  patchelf
```

For toolchains, install:
* **Rust:** Stable toolchain (`rustc`, `cargo`).
* **Bun:** Latest stable release (`oven-sh/setup-bun` or official installer).
* **Zig:** Version `0.16.0` (required only if building the `native-terminal` Ghostty VT component).

---

## 3. Building and Installation

### Option A: Install the Debian Package (`.deb`)

Official release builds emit a Debian package:

```bash
sudo dpkg -i Ferryx_amd64.deb || sudo apt-get install -f -y
```

This places the `ferryx` binary in `/usr/bin/ferryx`.

### Option B: Extract from AppImage

If you deploy via `Ferryx_amd64.AppImage`, extract the binary on headless systems that lack FUSE:

```bash
chmod +x Ferryx_amd64.AppImage
./Ferryx_amd64.AppImage --appimage-extract
sudo cp squashfs-root/usr/bin/ferryx /usr/local/bin/ferryx
sudo cp -r squashfs-root/usr/lib/* /usr/local/lib/ 2>/dev/null || true
```

### Option C: Build from Source

You can build either the main `ferryx` binary or the standalone `ferryx-cli` binary.

1. Clone the repository and install frontend dependencies:
   ```bash
   git clone https://github.com/ferryx/ferryx.git
   cd ferryx
   bun install --frozen-lockfile
   bun install --cwd ui --frozen-lockfile
   bun run --cwd ui build
   ```

2. Compile the release binaries:
   ```bash
   cargo build --manifest-path src-tauri/Cargo.toml --release --bin ferryx --bin ferryx-cli
   ```

3. Install the binaries into your system path:
   ```bash
   sudo install -m 0755 target/release/ferryx /usr/local/bin/ferryx
   sudo install -m 0755 target/release/ferryx-cli /usr/local/bin/ferryx-cli
   ```

---

## 4. Daemon Mechanics, Sockets, and File Locking

The daemon adheres to strict POSIX security and concurrency rules. Understanding these paths avoids startup failures.

### Runtime Directory and Ownership Checks

On Unix platforms, the daemon resolves its runtime directory through this sequence:
1. Environment variable `FERRYX_RUNTIME_DIR` if set.
2. Default path: `/tmp/rorca-<UID>` where `<UID>` is the numeric ID of the calling process. In debug builds, the path is `/tmp/rorca-<UID>-dev`.

Before binding sockets, the daemon enforces the following security checks:
* The runtime directory must be owned by the running process's effective UID.
* The runtime directory permissions must be strictly `0700` (`rwx------`). If permissions differ, the daemon attempts `chmod 0700` and exits with an error if the directory does not match `0700`.
* The directory must not be a symbolic link.

### Socket Paths and Permissions

The daemon creates two Unix domain sockets inside the runtime directory:
* **Canonical IPC Socket:** `/tmp/rorca-<UID>/daemon.sock` (mode `0600`, `rw-------`). Handles client commands, terminal spawns, workspace updates, and remote configuration.
* **Agent State Socket:** `/tmp/rorca-<UID>/agent-state.sock` (mode `0600`, `rw-------`). Subscribes agent observation and execution events.

Both socket paths are checked to ensure they are real socket nodes owned by the process UID and are not symlinks.

### Multi-Instance Mutual Exclusion and Locks

To prevent concurrent daemon processes from corrupting state or clobbering live PTYs, Ferryx acquires two separate locks:

1. **Persistent Lock:** Located at `~/.ferryx/locks/daemon.lock` (or `$FERRYX_DATA_DIR/locks/daemon.lock`).
2. **Runtime Lock:** Located at `/tmp/rorca-<UID>/daemon.lock` (or `$FERRYX_RUNTIME_DIR/daemon.lock`).

Both lock files are created with mode `0600` using the `O_NOFOLLOW` flag to prevent symlink attacks. The daemon locks them via `libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB)`. If another process holds the lock, the call fails immediately with:
`Another daemon instance is already holding the lock.`

Only after acquiring both locks does the daemon clean up any stale socket file left behind by an earlier ungraceful shutdown and bind the new listener.

### The `FERRYX_DAEMON_READY` Signal

When started with `--daemon`, the process initializes its runtime, binds `/tmp/rorca-<UID>/daemon.sock`, restores previous session state, starts the agent state listener, and restores any configured remote gateway.

Once all initialization steps complete, the daemon prints this exact token to standard output and immediately flushes the stream:

```text
FERRYX_DAEMON_READY
```

Clients, scripts, and service managers can watch standard output for this token to determine when the daemon is ready to accept IPC requests.

---

## 5. Running as a Systemd Service

Deploying Ferryx under `systemd --user` is recommended. Running as a user service guarantees that the daemon shares the target user's UID, environment, HOME directory, and permissions.

### Step 1: Create the User Service Unit

Create the directory `~/.config/systemd/user/` and save the unit file:

```bash
mkdir -p ~/.config/systemd/user
cat <<'EOF' > ~/.config/systemd/user/ferryx-daemon.service
[Unit]
Description=Ferryx Headless Terminal and Agent Daemon
Documentation=https://github.com/ferryx/ferryx
After=network.target

[Service]
Type=exec
ExecStart=/usr/local/bin/ferryx --daemon
Restart=on-failure
RestartSec=5s

# Standard stream handling
StandardOutput=journal
StandardError=journal

# Process isolation and limits
LimitNOFILE=65535
TimeoutStopSec=15s

[Install]
WantedBy=default.target
EOF
```

*Note on Unit Type:* `Type=exec` tells systemd that the service has completed process execution once the binary is loaded. Because Ferryx remains in the foreground on Tokio, this works cleanly. If your systemd version is older than 240, change `Type=exec` to `Type=simple`.

### Step 2: Enable User Session Lingering

On Linux servers, user systemd instances terminate when SSH sessions disconnect. To keep the daemon alive continuously across logins and reboots, enable user lingering:

```bash
loginctl enable-linger "$USER"
```

### Step 3: Start and Verify the Service

Reload the user daemon, enable the service, and inspect its status:

```bash
systemctl --user daemon-reload
systemctl --user enable --now ferryx-daemon.service
systemctl --user status ferryx-daemon.service
```

Check the journal to confirm the daemon signaled readiness:

```bash
journalctl --user -u ferryx-daemon.service -b --no-pager
```

You should see `FERRYX_DAEMON_READY` in the log stream alongside the listening message:
`rorca daemon listening on /tmp/rorca-<UID>/daemon.sock`.

### Alternative: System-Wide Unit File

If your environment mandates system-wide units under `/etc/systemd/system/`, specify the target user and group explicitly:

```ini
[Unit]
Description=Ferryx Headless Terminal and Agent Daemon
After=network.target

[Service]
Type=exec
User=your-username
Group=your-username
WorkingDirectory=/home/your-username
Environment="HOME=/home/your-username"
ExecStart=/usr/local/bin/ferryx --daemon
Restart=on-failure
RestartSec=5s
LimitNOFILE=65535
TimeoutStopSec=15s

StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

Reload and start via `sudo systemctl daemon-reload && sudo systemctl enable --now ferryx-daemon.service`.

---

## 6. Remote Gateway Configuration

The remote gateway allows external Ferryx clients or web browsers to access your headless server.

### Network Modes

Ferryx supports four remote network modes (`RemoteNetworkMode`):

| Mode | Identifier in JSON | Description | Listen Address |
| --- | --- | --- | --- |
| **Off** | `"off"` | Remote gateway disabled (default). | None |
| **Local Network** | `"localNetwork"` | Binds loopback plus the machine's primary non-loopback LAN IPv4 address (e.g., `192.168.x.x` or `10.x.x.x`). | `127.0.0.1:43821` and `<LAN-IP>:43821` |
| **Tailscale** | `"tailscale"` | Binds loopback plus the detected Tailscale CGNAT IPv4 address (`100.64.0.0/10`). | `127.0.0.1:43821` and `<Tailscale-IP>:43821` |
| **Relay** | `"relay"` | Establishes an outbound reverse WebSocket tunnel to a relay server. Does not open external inbound ports. | `127.0.0.1:43821` (outbound to relay) |

*Important Security Design:* Ferryx NEVER binds to the wildcard address `0.0.0.0`. It binds strictly to `127.0.0.1` and the specific network interface resolved for the active mode.

### Fixed Port Policy

The remote gateway port is fixed in the source code to port `43821` (`REMOTE_GATEWAY_PORT`). While the configuration parser accepts a `"port"` key for wire compatibility, the daemon actively overwrites any custom port with `43821`.

### Configuring the Gateway on Headless Servers

*Source Limitation Note:* The Ferryx command-line interface provides `ferryx remote status` and `ferryx remote pair`, but does NOT provide a CLI subcommand to mutate or enable the network mode (such as `ferryx remote enable`).

To enable remote access on a headless host without an attached graphical desktop, write the configuration file `~/.ferryx/remote/remote-config.json` before starting the daemon (or restart the daemon after editing):

```bash
mkdir -p ~/.ferryx/remote
chmod 0700 ~/.ferryx/remote
```

#### Example 1: Local Network Mode

Save as `~/.ferryx/remote/remote-config.json`:

```json
{
  "mode": "localNetwork",
  "port": 43821,
  "allowControl": true,
  "restartPolicy": "restoreListener"
}
```

#### Example 2: Tailscale Mode

Ensure your host has Tailscale running and has an assigned `100.x.y.z` address. Save as `~/.ferryx/remote/remote-config.json`:

```json
{
  "mode": "tailscale",
  "port": 43821,
  "allowControl": true,
  "restartPolicy": "restoreListener"
}
```

#### Example 3: Relay Mode

Relay mode connects outbound to a relay coordinator. The default public relay URL is `https://relay.checka.cc`. Save as `~/.ferryx/remote/remote-config.json`:

```json
{
  "mode": "relay",
  "port": 43821,
  "allowControl": true,
  "restartPolicy": "restoreListener",
  "relayUrl": "https://relay.checka.cc"
}
```

Set permissions on the file to prevent unauthorized reading:

```bash
chmod 0600 ~/.ferryx/remote/remote-config.json
```

### Verifying Gateway Configuration

Use the CLI to check the active configuration:

```bash
ferryx remote status
```

Output:
```text
status=ok port=43821 mode=localNetwork
```

For JSON format:

```bash
ferryx remote status --json
```

Output:
```json
{"status":"ok","port":43821,"mode":"localNetwork"}
```

When the daemon starts with `mode` set to anything other than `"off"`, it automatically starts the HTTP/WebSocket server on port `43821`.

---

## 7. Pairing and Client Authentication

Ferryx uses a three-tier credential model:
1. **Machine Identity:** A persistent Ed25519 keypair identifying the host server.
2. **Pairing PIN:** A short-lived, 6-digit PIN used for initial client authorization.
3. **Device Bearer Token:** A long-lived, 64-character token issued to paired clients.

### Step 1: Machine Identity Creation

On first startup or pairing request, the daemon generates an Ed25519 keypair and writes it to `~/.ferryx/remote/identity.json` with permissions `0600`. It stores:
* `machineId`: A UUID v4 string.
* `displayName`: Server hostname (read from `/bin/hostname`).
* `publicKey`: Standard base64-encoded 32-byte verifying key.
* `privateKey`: Standard base64-encoded 32-byte signing seed.

### Step 2: Generating a Pairing Code

On the server, run:

```bash
ferryx pair generate
# or: ferryx remote pair generate
```

This command connects to the running daemon over the UDS socket (`/tmp/rorca-<UID>/daemon.sock`) and registers a new 6-digit pairing code.

Output example:
```text
849201
Pairing registered by the running daemon; it holds the relay control connection.
```

If the daemon is running in `relay` mode, the output also includes the pairing URL fragment:
```text
849201
https://relay.checka.cc#pair=ab83cd...
Pairing registered by the running daemon; it holds the relay control connection.
```

### Pairing Constraints and Lifetimes

* **Code Format:** 6 decimal digits (`100000` to `999999`).
* **Expiration Window:** Exactly 60 seconds.
* **Attempt Budget:** Maximum of 5 failed attempts before rate limiting locks the pairing window. Subsequent failed attempts receive HTTP 429 (`pairing_rate_limited`).
* **Single Use:** Once a code is exchanged, it is immediately consumed from memory and cannot be reused.

### Step 3: Client Exchange

The remote client (or browser) sends an HTTP POST request to the server:

```http
POST /api/v1/pair/exchange HTTP/1.1
Host: <server-ip>:43821
Content-Type: application/json

{
  "code": "849201",
  "deviceName": "Alice Laptop",
  "installationId": "optional-client-uuid"
}
```

The server responds with the long-lived bearer token:

```json
{
  "token": "a1b2c3d4e5f6...64-characters...",
  "device": {
    "id": "device-uuid",
    "name": "Alice Laptop",
    "permission": "control",
    "createdAt": 1757635200,
    "lastSeenAt": 1757635200,
    "revoked": false
  },
  "machineId": "host-machine-uuid",
  "displayName": "server-hostname"
}
```

### Pre-approving from the CLI

If you want to approve a PIN explicitly on the server before client exchange:

```bash
ferryx pair approve 849201
```

Output:
```text
Pairing approved for 849201; ready for remote client exchange
```

This creates a device entry named `cli-paired-device` in `~/.ferryx/remote/remote-auth.json` and attaches the generated token to the pairing code. When the client completes the exchange, it receives that pre-approved token.

### Listing Paired Devices

To inspect all authorized devices:

```bash
ferryx pair list
```

Output:
```text
7c8e...-uuid	Alice Laptop	Control
9f1a...-uuid	cli-paired-device	Control
```

### Device Token Lifetime

Paired device tokens remain valid until revoked, or until the device has been idle for longer than 30 days (`DEVICE_IDLE_EXPIRY_SECS = 2,592,000` seconds).

---

## 8. Firewall and Port Configuration

### Inbound Firewall Rules

* **Local Network Mode (`localNetwork`):**
  Open TCP port `43821` on your local subnet:
  ```bash
  # UFW (Ubuntu/Debian)
  sudo ufw allow from 192.168.1.0/24 to any port 43821 proto tcp comment 'Ferryx Local Gateway'

  # firewalld (RHEL/Fedora)
  sudo firewall-cmd --zone=home --add-port=43821/tcp --permanent
  sudo firewall-cmd --reload

  # nftables
  nft add rule inet filter input ip saddr 192.168.1.0/24 tcp dport 43821 accept
  ```

* **Tailscale Mode (`tailscale`):**
  Only traffic arriving across the Tailscale network interface (`tailscale0`) needs access:
  ```bash
  sudo ufw allow in on tailscale0 to any port 43821 proto tcp comment 'Ferryx Tailscale Gateway'
  ```

* **Relay Mode (`relay`):**
  No inbound ports are opened. The daemon initiates an outbound HTTPS/WSS connection to `relay.checka.cc` (port 443 TCP). Ensure your server allows outbound TCP port 443.

---

## 9. Log Locations and Monitoring

### Standard Stream and Journald Logs

When managed by systemd, stdout and stderr are captured in systemd journald:

```bash
# Follow live daemon logs
journalctl --user -u ferryx-daemon.service -f

# View logs from current boot
journalctl --user -u ferryx-daemon.service -b
```

### Persistent Data Locations

Ferryx maintains persistent files across these paths:

* **Remote Gateway State (`~/.ferryx/remote/`):**
  * `remote-config.json`: Active remote network mode, port, and control flags.
  * `remote-auth.json`: Paired device records, active tokens, and transient pairing PINs.
  * `identity.json`: Machine Ed25519 identity keypair and display name.
* **Persistent Locks (`~/.ferryx/locks/`):**
  * `daemon.lock`: Inter-process coordination lock.
* **Session Checkpoints:**
  * `/tmp/rorca-<UID>/remote_sessions.json`: Saved remote terminal session descriptors.
  * `~/.local/share/rorca/session_state.json`: General session layout checkpoint.
* **SSH Host Registry:**
  * `~/.ferryx/ssh_hosts.json`: Configured remote SSH connections.

### Environment Variable Overrides

You can redirect these directories by setting environment variables in your systemd unit or shell profile:

* `FERRYX_RUNTIME_DIR`: Changes `/tmp/rorca-<UID>`.
* `FERRYX_DATA_DIR`: Changes `~/.ferryx/`.
* `FERRYX_SESSION_DIR`: Changes session checkpoint storage.
* `FERRYX_RELAY_URL`: Overrides `https://relay.checka.cc`.
* `FERRYX_MACHINE_TOKEN`: Supplies a pre-shared token for relay tunnels instead of machine identity signatures.

---

## 10. Troubleshooting

### Problem 1: "Another daemon instance is already holding the lock"

**Symptom:** Daemon exits immediately during startup with:
```text
Ferryx daemon error: Another daemon instance is already holding the lock.
```

**Cause:** A running daemon process or an orphaned lock holder is locking `/tmp/rorca-<UID>/daemon.lock` or `~/.ferryx/locks/daemon.lock`.

**Resolution:**
1. Check if a ferryx process is running:
   ```bash
   pgrep -a ferryx
   ```
2. When no process is running, verify which process holds the file lock:
   ```bash
   lsof /tmp/rorca-"$(id -u)"/daemon.lock
   lsof ~/.ferryx/locks/daemon.lock
   ```
3. In situations where an earlier process crashed and the locks were released, the daemon automatically cleans up stale sockets on the next launch once it acquires the flock. Never delete the lock file manually while a live process is running.

### Problem 2: "Daemon runtime directory has mode ..., expected 700"

**Symptom:** Startup fails with:
```text
Daemon runtime directory /tmp/rorca-1000 has mode ..., expected 700
```
or:
```text
Path /tmp/rorca-1000 is a symlink, which is prohibited for daemon runtime
```

**Cause:** Ferryx validates that `/tmp/rorca-<UID>` is a genuine directory owned by your UID with mode `0700`.

**Resolution:**
Fix directory ownership and permissions:
```bash
sudo chown "$(id -u):$(id -g)" /tmp/rorca-"$(id -u)"
chmod 0700 /tmp/rorca-"$(id -u)"
```
If `/tmp/rorca-<UID>` is a symlink, remove it:
```bash
rm /tmp/rorca-"$(id -u)"
mkdir -m 0700 /tmp/rorca-"$(id -u)"
```

### Problem 3: Remote Gateway Fails to Bind on Local Network or Tailscale

**Symptom:** Log displays:
```text
Failed to restore daemon remote gateway listener: no active local network IPv4 interface found
```
or:
```text
Failed to restore daemon remote gateway listener: no active Tailscale IPv4 interface found
```

**Cause:** The interface resolver could not discover an active non-loopback IP address matching the requested mode:
* In `localNetwork` mode, the resolver tests a route probe to `8.8.8.8:80` and inspects network interfaces via `getifaddrs`. If the host lacks a default route or has only loopback interfaces, discovery fails.
* In `tailscale` mode, the resolver scans for an address within the `100.64.0.0/10` CGNAT block. If Tailscale is stopped or logged out, no CGNAT IP exists.

**Resolution:**
1. Under LAN mode, verify your host has a non-loopback IPv4 address:
   ```bash
   ip -4 addr show
   ```
2. For Tailscale mode, verify Tailscale status and interface address:
   ```bash
   tailscale status
   ip -4 addr show tailscale0
   ```
3. When the host temporarily lacks external network access, change `"mode": "off"` in `~/.ferryx/remote/remote-config.json` to allow the daemon to start.

### Problem 4: Pairing Returns "Invalid pairing code" or "Pairing code expired"

**Symptom:** Exchanging a pairing code fails with HTTP 400 or HTTP 401:
```json
{"message":"Pairing code expired"}
```

**Cause:** Pairing PINs expire after 60 seconds (`PAIRING_EXPIRY`).

**Resolution:**
Generate a fresh PIN with `ferryx pair generate` and complete the exchange on the client immediately. If you exceed 5 failed attempts, the window enters rate-limiting (`pairing_rate_limited`, HTTP 429). Wait 60 seconds before trying again.

### Problem 5: Missing `FERRYX_DAEMON_READY` in Automated Scripts

**Symptom:** Startup wrappers or test harnesses hang waiting for `FERRYX_DAEMON_READY`.

**Cause:** The readiness signal is written to standard output (`println!("FERRYX_DAEMON_READY")`) and flushed only when `handover_from` is `None` and the socket listener has successfully bound. If an error occurs prior to listener binding (such as lock failure, permission denial, or invalid data directory), the token is not emitted and the error is printed to standard error.

**Resolution:**
Monitor both stdout and stderr in your script. Capture stderr to identify the root initialization failure if the ready token does not arrive within 10 seconds.
