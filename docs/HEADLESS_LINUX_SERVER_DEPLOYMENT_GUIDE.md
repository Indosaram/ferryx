# Headless Linux Server Deployment Guide for Ferryx

This guide explains how to deploy and operate Ferryx in headless daemon mode on a Linux server without a graphical display. It documents mechanics confirmed in the source code, including process execution, Unix domain sockets, file locks, systemd service configuration, remote access network modes, pairing, and troubleshooting. External operational recommendations, such as unit definitions and package installation commands, require adaptation to your environment.

---

## 1. Architectural Overview

Ferryx can run in two primary launch modes:

1. **GUI Mode (Default):** Runs the full Tauri desktop application with its native window and graphical interface (`src-tauri/src/main.rs:37`).
2. **Headless Daemon Mode (`--daemon`):** Launches a foreground Tokio multi-thread runtime without initializing any desktop windowing, GTK, or display subsystems (`src-tauri/src/main.rs:35`, `src-tauri/src/cli.rs:471-518`). This process runs in the foreground rather than self-daemonizing.

Background terminal PTY processes, workspace state, agent extension hooks, session persistence, and client connections are coordinated by the daemon over a local Unix domain socket (`src-tauri/src/daemon/server.rs:980-1093,1306-1380,1456-1523`). Live child PTY processes do not survive host reboots or daemon restarts. Session layouts and remote session descriptors are saved to disk (`src-tauri/src/daemon/server.rs:1013-1017,1896-1913`).

When configured, the daemon also runs the remote gateway HTTP and WebSocket server (`src-tauri/src/remote/server.rs:2132-2161,2203-2380`). Web browsers or remote Ferryx desktop clients can connect over a local network, a Tailscale tailnet, or a reverse relay tunnel.

### Internal Identifiers and Compatibility Paths

Ferryx maintains compatibility with earlier runtime and branch conventions. The user-facing application name is Ferryx (`src-tauri/tauri.conf.json:3-5`), but several internal filesystem and protocol identifiers retain their existing naming:

* Runtime directory: `/tmp/rorca-<UID>` for release builds and `/tmp/rorca-<UID>-dev` for debug builds, where `<UID>` is the numeric user ID returned by POSIX `libc::getuid` (`src-tauri/src/daemon/server.rs:135`). Note that `libc::getuid` returns the real user ID, not the effective UID (`geteuid`). The path can be overridden with `FERRYX_RUNTIME_DIR` (`:128`).
* Canonical daemon socket: `/tmp/rorca-<UID>/daemon.sock` (`src-tauri/src/daemon/server.rs:159`).
* Agent state socket: `/tmp/rorca-<UID>/agent-state.sock` (`src-tauri/src/daemon/server.rs:831`).
* Persistent user configuration directory: `~/.ferryx/` (`src-tauri/src/daemon/server.rs:271`), overridden by `FERRYX_DATA_DIR`.
* Session state path resolution (`src-tauri/src/daemon/server.rs:301-361`): Linux prioritizes `$XDG_DATA_HOME/rorca/session_state.json`, falling back to `$HOME/.local/share/rorca/session_state.json`. If neither base is found, `dirs_fallback()` appends `rorca/session_state.json` to `$HOME/.rorca`, producing `$HOME/.rorca/rorca/session_state.json`. If no home base can be resolved, it falls back to `<runtime_dir>/session_state.json`. Debug runtime appends `session_state.dev.json` or uses `rorca-dev`. The directory can be redirected with `FERRYX_SESSION_DIR` (`:302`).
* SSH host registry: Linux defaults to `$XDG_DATA_HOME/com.ferryx.app/ssh_hosts.json` or `$HOME/.local/share/com.ferryx.app/ssh_hosts.json` (`src-tauri/src/daemon/server.rs:861-873`), or in debug mode `.../com.ferryx.app/dev/ssh_hosts.json`. Only when `FERRYX_DATA_DIR` is set does it relocate to `<FERRYX_DATA_DIR>/ssh_hosts.json`.
* Internal git branch namespace: `orca/<ws-id>/<slug>` (`src-tauri/src/worktree/manager.rs:350-382`).
* Worktree cache directory: `.orca-worktrees/` inside git repositories (`src-tauri/src/worktree/manager.rs:15`).

---

## 2. Prerequisites

### Host Environment

* **Architecture:** Linux `x86_64` is the primary CI-tested and release target (`.github/workflows/build-test.yml:53-66`, `scripts/lib/release-platforms.mjs:576-624`). Linux `aarch64` is supported as a source-build target (`src-tauri/native_terminal/build_ghostty.rs:129-132,315-319`), not as an audited official binary.
* **Init System:** `systemd` is recommended as an external process manager, not a runtime daemon prerequisite.
* **Operating Account:** A standard, non-root user account is recommended. The daemon verifies that the runtime directory is owned by the process real UID (`src-tauri/src/daemon/server.rs:434-439`), without explicitly blocking `root`. Running as `root` carries operational risks, including root-owned cache and worktree files conflicting with unprivileged workflows.
* **Core Utilities:** `hostname` is resolved via `PATH` lookup with fallback to `"Ferryx machine"` (`src-tauri/src/remote/auth.rs:86-94`). Git is used for worktree operations (`src-tauri/src/worktree/manager.rs:1-5`). Shell tools like `curl`, `ss`, `lsof`, and `ip` are suggested diagnostic aids.

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

Shared library requirements:
`src-tauri/Cargo.toml:175-177` declares `gtk` and `webkit2gtk` as unconditional `cfg(target_os = "linux")` dependencies of the crate, so their `-sys` crates run `pkg-config` even when building only `--bin ferryx`.
These dependencies link shared libraries into the binary. Even though headless launch bypasses display initialization (`src-tauri/src/main.rs:35`), the compiled executable dynamically links against GTK 3 and WebKitGTK 4.1 (`src-tauri/tauri.conf.json:90-98`). The dynamic linker requires these runtime shared libraries (`libgtk-3.so.0`, `libwebkit2gtk-4.1.so.0`) on the host system at process startup.
Package commands in this guide are external recommendations, not tested minimal package sets.

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
* **Zig:** Version `0.16.0` (`src-tauri/native_terminal/build_ghostty.rs:7`, `REQUIRED_ZIG_VERSION`), required to compile the vendored `native-terminal` Ghostty VT component.

---

## 3. Building and Installation

### Option A: Install the Debian Package (`.deb`)

Release pipelines build Debian packages targeting amd64 (`scripts/lib/release-platforms.mjs:609-622`, `src-tauri/tauri.conf.json:51-58,90-99`):

```bash
sudo dpkg -i Ferryx_amd64.deb || sudo apt-get install -f -y
```

This installation expects to place the executable in `/usr/bin/ferryx`. Verify the installed path with `command -v ferryx`.

### Option B: Extract from AppImage

AppImage packages bundle required runtime dependencies. If you deploy on headless hosts without FUSE, extract the bundle:

```bash
chmod +x Ferryx_amd64.AppImage
./Ferryx_amd64.AppImage --appimage-extract
```

Extracted files remain in `squashfs-root`.
Do not copy libraries from `squashfs-root/usr/lib/*` directly into `/usr/local/lib/` or suppress copy errors. Blindly copying libraries bypasses system package tracking and linker configuration.
Instead, install runtime shared libraries (`libgtk-3-0`, `libwebkit2gtk-4.1-0`) through your system package manager. Alternatively, keep the extracted directory intact in an application root such as `/opt/ferryx` and invoke the binary using `LD_LIBRARY_PATH=/opt/ferryx/usr/lib /opt/ferryx/usr/bin/ferryx --daemon`.

### Option B2: One-Line Installer (Recommended for `ferryx-cli`)

```bash
curl -fsSL https://relay.ferryx.dev/install.sh | bash
```

The installer detects the OS/architecture, downloads the matching standalone `ferryx-cli`
binary from the relay (falling back to the GitHub release), verifies the Linux ELF header,
and installs to `~/.local/bin` (or `/usr/local/bin` when run as root). Override the target
directory with `FERRYX_INSTALL_DIR`, or the download origin with `FERRYX_ORIGIN`.
The same script is served from `https://ferryx.dev/install.sh`.

### Option C: Build from Source

You can build either the main `ferryx` binary or the standalone `ferryx-cli` binary.

1. Clone the repository and initialize submodules:
   ```bash
   git clone https://github.com/Indosaram/ferryx.git
   cd ferryx
   git submodule update --init --recursive
   bun install --frozen-lockfile
   bun install --cwd ui --frozen-lockfile
   bun run --cwd ui build
   ```
   The `native-terminal` feature is active by default (`src-tauri/Cargo.toml:49`).
   Building the release binaries invokes `build_ghostty::build_ghostty_vt` (`src-tauri/build.rs:36`), which compiles the vendored Ghostty source.
   The submodule must be initialized at the pinned commit (`src-tauri/native_terminal/build_ghostty.rs:6`, `EXPECTED_GHOSTTY_SHA`).

2. Compile the release binaries:
   ```bash
   cargo build --manifest-path src-tauri/Cargo.toml --release --bin ferryx --bin ferryx-cli
   ```

3. Install the binaries into your system path:
   ```bash
   sudo install -m 0755 src-tauri/target/release/ferryx /usr/local/bin/ferryx
   sudo install -m 0755 src-tauri/target/release/ferryx-cli /usr/local/bin/ferryx-cli
   ```
   Because there is no root Cargo workspace file, Cargo emits build artifacts under `src-tauri/target/release/` unless redirected by `CARGO_TARGET_DIR` or `--target`.

### Web Assets for Remote Gateway

The remote gateway server locates frontend web assets through `resolve_dist_dir_from` (`src-tauri/src/remote/server.rs:1956-2008`). It searches relative to the binary (`<exe_dir>/ui/dist`, `<exe_dir>/resources/ui/dist`, `<exe_dir>/../Resources/ui/dist`), compile-time manifest dir, and working directory (`<cwd>/ui/dist`).
Installing only the `ferryx` binary to `/usr/local/bin/ferryx` leaves the web server without web assets. When assets are missing, the server responds with fallback placeholder HTML (`src-tauri/src/remote/server.rs:2070`).
To serve the browser UI, copy the built `ui/dist` folder into a searched location:

```bash
sudo mkdir -p /usr/local/bin/ui
sudo cp -r ui/dist /usr/local/bin/ui/
```

Or run the service with a working directory containing `ui/dist`.

---

## 4. Daemon Mechanics, Sockets, and File Locking

The daemon adheres to strict POSIX security and concurrency rules. Understanding these paths avoids startup failures.

### Runtime Directory and Ownership Checks

On Unix platforms, the daemon resolves its runtime directory through this sequence:
1. Environment variable `FERRYX_RUNTIME_DIR` if set (`src-tauri/src/daemon/server.rs:128`).
2. Default path: `/tmp/rorca-<UID>` where `<UID>` is the numeric ID of the calling process (`src-tauri/src/daemon/server.rs:135`). In debug builds, the path is `/tmp/rorca-<UID>-dev`.

Before binding sockets, the daemon enforces the following security checks:
* Runtime directory ownership must match the real UID of the calling process returned by `libc::getuid` (`src-tauri/src/daemon/server.rs:439`), not the effective UID.
* Directory permissions must be strictly `0700` (`rwx------`). Mode validation masks access bits with `0777` (`src-tauri/src/daemon/server.rs:479`). If permissions differ, the daemon attempts `chmod 0700` and exits with an error if the result does not match `0700`.
* The directory must not be a symbolic link (`src-tauri/src/daemon/server.rs:469`).

### Asymmetric Socket Paths and Validation

The daemon creates two Unix domain sockets inside the runtime directory with different validation levels:

* **Canonical IPC Socket:** `/tmp/rorca-<UID>/daemon.sock` (`src-tauri/src/daemon/server.rs:159`). Handles client commands, terminal spawns, workspace updates, and remote configuration. It undergoes strict validation: stale socket removal after lock acquisition (`:1456`), safe ownership and non-symlink verification (`:1482`), and fatal exit if chmod `0600` fails (`:1484-1486`).
* **Agent State Socket:** `/tmp/rorca-<UID>/agent-state.sock` (`src-tauri/src/daemon/server.rs:831`). Acts as an ingest receiver for newline-delimited agent reports (`:1360-1375`). It does not receive equivalent strict validation: `spawn_agent_state_listener` (`:1345-1380`) unconditionally removes any existing file, binds the socket, logs a warning on chmod failure without exiting, and returns nonfatally on bind failure. It is not an IPC subscription endpoint for clients.

### Multi-Instance Mutual Exclusion and Locks

To prevent concurrent daemon processes from clobbering state, Ferryx acquires file locks:

1. **Persistent Lock (Optional):** Located at `~/.ferryx/locks/daemon.lock` (or `$FERRYX_DATA_DIR/locks/daemon.lock` at `src-tauri/src/daemon/server.rs:271`). If neither `FERRYX_DATA_DIR` nor `HOME` resolves, the persistent lock is skipped (`:699`).
2. **Runtime Lock:** Located at `/tmp/rorca-<UID>/daemon.lock` (or `$FERRYX_RUNTIME_DIR/daemon.lock` at `src-tauri/src/daemon/server.rs:168`).

Lock files are opened with mode `0600` (`src-tauri/src/daemon/server.rs:547`) using `libc::O_NOFOLLOW` (`:548`) to prevent symlink attacks. The daemon locks them via `libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB)`. If `libc::flock` returns any nonzero code, startup fails immediately with:
`Another daemon instance is already holding the lock.`
This error message is emitted for any flock failure, including operating system errors.

Only after acquiring locks does the daemon clean up any stale canonical socket file from an earlier ungraceful shutdown (`src-tauri/src/daemon/server.rs:1456`) and bind the new listener.

### Readiness Notification via `FERRYX_DAEMON_READY`

When started with `--daemon`, the process initializes its runtime, binds `/tmp/rorca-<UID>/daemon.sock`, and restores previous session routes (`src-tauri/src/daemon/server.rs:1456-1492`).

The daemon signals readiness over an internal oneshot channel (`src-tauri/src/daemon/server.rs:1493`).
Next, the CLI wrapper awaits that signal on an independent Tokio task and prints this exact token to standard output (`src-tauri/src/cli.rs:536`):

```text
FERRYX_DAEMON_READY
```

The CLI then calls `flush()` on standard output. Any flush error is ignored.

Readiness ordering and listener restoration:
The internal oneshot signal is sent before spawning the agent listener and restoring the remote gateway (`src-tauri/src/daemon/server.rs:1493-1505`). Because the CLI emits the token on an independently scheduled task, stdout delivery does not guarantee wall-clock ordering before remote gateway restoration or client accept loops. The token confirms that the local UDS socket listener has bound.
Gateway restoration failure logs a warning (`src-tauri/src/daemon/server.rs:1503`) and continues running the client accept loop without terminating the daemon.

---

## 5. Running as a Systemd Service

Running Ferryx under `systemd --user` provides process supervision under your user account. Operational examples below illustrate this setup.

### Step 1: Create the User Service Unit

Create the directory `~/.config/systemd/user/` and save the unit file:

```bash
mkdir -p ~/.config/systemd/user
cat <<'EOF' > ~/.config/systemd/user/ferryx-daemon.service
[Unit]
Description=Ferryx Headless Terminal and Agent Daemon
Documentation=https://github.com/Indosaram/ferryx
After=network.target

[Service]
Type=exec
# Set /usr/bin/ferryx for .deb package installs, or /usr/local/bin/ferryx for source builds
ExecStart=/usr/local/bin/ferryx --daemon
Restart=on-failure
RestartSec=5s

# Standard stream handling
StandardOutput=journal
StandardError=journal

# Process limits
LimitNOFILE=65535
TimeoutStopSec=15s

[Install]
WantedBy=default.target
EOF
```

*Binary Location:* Set `ExecStart` according to your installation method. Official Debian packages place the binary in `/usr/bin/ferryx`. Source builds place it in `/usr/local/bin/ferryx`.

*Unit Type:* `Type=exec` considers startup complete once the process is executed. It does not wait for `FERRYX_DAEMON_READY`. If your systemd version is older than 240, use `Type=simple`.

*Environment:* The systemd user service environment does not automatically inherit interactive shell environment variables from `.bashrc` or `.zshrc` (such as customized `PATH`, SSH agent sockets, or Ferryx overrides). Define necessary environment variables directly in the unit file if required.

### Step 2: Enable User Session Lingering

Depending on system logind configuration, user manager processes may terminate when your last login session closes. To ensure the user service manager starts at boot and persists after logout, enable lingering:

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

You should see `FERRYX_DAEMON_READY` in the log stream.
Headless daemon execution initializes a dedicated, bounded file subscriber for agent-state diagnostics (`src-tauri/src/daemon/logging.rs:33-80`, `src-tauri/src/cli.rs:479`), capped at 1 MiB under `~/.ferryx/logs/daemon.log` or `$FERRYX_DATA_DIR/logs/daemon.log`. General daemon tracing log statements remain silenced to stdout/journald. Fatal startup errors appear on standard error (`src-tauri/src/main.rs:39`).

### Alternative: System-Wide Unit File

If your server mandates system-wide units under `/etc/systemd/system/`, specify user and group explicitly:

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

Reload and start via `sudo systemctl daemon-reload && sudo systemctl enable --now ferryx-daemon.service`. Substitute your actual username and paths.

---

## 6. Remote Gateway Configuration

The remote gateway allows external Ferryx desktop clients or web browsers to connect to your headless server.

### Network Modes

Ferryx supports four remote network modes (`RemoteNetworkMode` at `src-tauri/src/remote/state.rs:20-60`):

| Mode | Identifier in JSON | Description | Listen Address |
| --- | --- | --- | --- |
| **Off** | `"off"` | Remote gateway listener disabled (`src-tauri/src/remote/server.rs:2266-2268`). Does not affect outbound connections like SSH sessions or updater checks. | None |
| **Local Network** | `"localNetwork"` | Binds loopback plus the machine primary non-loopback IPv4 address (`src-tauri/src/remote/state.rs:194-227`). If the primary address is public, it binds that public IP rather than an RFC1918 private address. | `127.0.0.1:43821` and `<Resolved-IP>:43821` |
| **Tailscale** | `"tailscale"` | Binds loopback plus an active address within the `100.64.0.0/10` CGNAT block (`src-tauri/src/remote/state.rs:110-112,229-234`). Matches IP range, not interface name or daemon login status. | `127.0.0.1:43821` and `<Tailscale-IP>:43821` |
| **Relay** | `"relay"` | Opens an internal loopback listener and establishes an outbound reverse WebSocket tunnel to a relay coordinator (`src-tauri/src/remote/server.rs:2270-2304`). Opens no external inbound ports. | `127.0.0.1:43821` (outbound tunnel) |

Wildcard address scope: The production gateway listener binds loopback and resolved addresses, never `0.0.0.0` (`src-tauri/src/remote/server.rs:2299-2344`). Other subsystems may use wildcard sockets; the outbound route probe binds UDP `0.0.0.0:0` (`src-tauri/src/remote/state.rs:152`), and the standalone `ferryx-relay` binary binds TCP `0.0.0.0` (`src-tauri/src/bin/relay.rs:95`).

### Fixed Port Policy and Transport Protocol

The remote gateway port is fixed in the source code to `43821` (`src-tauri/src/daemon/server.rs:2444`, `REMOTE_GATEWAY_PORT`). While the configuration parser accepts a `"port"` key for wire compatibility, the daemon overwrites custom values with `43821`.

Direct connections to port 43821 on loopback or local networks use plain HTTP and WebSockets over standard TCP (`src-tauri/src/remote/server.rs:2209`).
Ferryx does not terminate TLS on this internal listener (`:2225`). For untrusted networks, place a TLS-terminating reverse proxy in front of the server.

### Configuring the Gateway on Headless Servers

The Ferryx command-line interface provides `remote status` and `remote pair`, but does not offer a dedicated `remote enable` subcommand (`src-tauri/src/cli.rs:300-325`).
Running `ferryx pair generate` while the gateway is in `Off` mode auto-configures the daemon to `Relay` mode (`src-tauri/src/daemon/server.rs:1948-1972`). This pairing side effect enables remote relay access without manual file edits.

To configure other modes on a headless host, write `~/.ferryx/remote/remote-config.json` before starting the daemon (or restart the daemon after editing):

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

The `"allowControl"` key is stored in configuration (`src-tauri/src/remote/state.rs:48`), but server request authorization evaluates individual `device.permission` (Control vs View) rather than this configuration flag (`src-tauri/src/remote/server.rs:1477-1528`).

#### Example 2: Tailscale Mode

Ensure your host has an assigned CGNAT address in `100.64.0.0/10` (`100.64.0.0` through `100.127.255.255`). Save as `~/.ferryx/remote/remote-config.json`:

```json
{
  "mode": "tailscale",
  "port": 43821,
  "allowControl": true,
  "restartPolicy": "restoreListener"
}
```

#### Example 3: Relay Mode

Relay mode connects outbound to a relay coordinator. The default public relay URL is `https://relay.checka.cc` (`src-tauri/src/remote/state.rs:20`). Save as `~/.ferryx/remote/remote-config.json`:

```json
{
  "mode": "relay",
  "port": 43821,
  "allowControl": true,
  "restartPolicy": "restoreListener",
  "relayUrl": "https://relay.checka.cc"
}
```

Specify `"relayUrl"` in this configuration file for deterministic restoration across daemon restarts. `FERRYX_RELAY_URL` is only used when auto-configuring from Off during pairing (`src-tauri/src/daemon/server.rs:1956`), not during standard startup restoration (`src-tauri/src/remote/server.rs:2270`).

Set permissions on the configuration file:

```bash
chmod 0600 ~/.ferryx/remote/remote-config.json
```

### Verifying Gateway Configuration

Use the CLI to inspect the persisted configuration:

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

Persisted status versus live health:
The `ferryx remote status` command reads static configuration from `~/.ferryx/remote/remote-config.json` (`src-tauri/src/cli.rs:361`).
It prints `status=ok` along with the configured port and mode, but does not query the running daemon over IPC. It returns `status=ok` even if the daemon is stopped or if the gateway failed to bind due to network interface errors.
To confirm that the remote gateway is actively listening, inspect open TCP sockets directly:
```bash
ss -tulpn | grep 43821
```
Or probe the loopback health endpoint:
```bash
curl -i http://127.0.0.1:43821/api/v1/health
```
Probing checks local TCP and loopback HTTP response, not relay tunnel connectivity or overall application health.

---

## 7. Pairing and Client Authentication

Ferryx uses a three-tier credential model:
1. **Machine Identity:** A persistent Ed25519 keypair identifying the host server (`src-tauri/src/remote/auth.rs:75-104`).
2. **Pairing PIN:** A short-lived 6-digit PIN used for initial client exchange (`src-tauri/src/remote/auth.rs:374-393`).
3. **Device Bearer Token:** A long-lived 64-character token issued to paired clients (`src-tauri/src/remote/auth.rs:545-573`).

### Step 1: Machine Identity Creation

Machine identity generation is lazy (`src-tauri/src/remote/auth.rs:75-104`). The daemon generates an Ed25519 keypair when first handling a client pairing exchange (`src-tauri/src/remote/server.rs:336`) or establishing signed Relay tunnels (`:2287-2293`), not unconditionally on every daemon launch.
When created, it writes `~/.ferryx/remote/identity.json` with permissions `0600` (`src-tauri/src/remote/auth.rs:868-910`). Reading an existing file does not check or repair its permissions (`:75-84`). It stores:
* `machineId`: A UUID v4 string.
* `displayName`: Hostname resolved via `PATH` lookup (`src-tauri/src/remote/auth.rs:86-94`), falling back to `"Ferryx machine"`.
* `publicKey`: Standard base64-encoded 32-byte verifying key.
* `privateKey`: Standard base64-encoded 32-byte signing seed.

### Step 2: Generating a Pairing Code

On the server, run:

```bash
ferryx pair generate
# or: ferryx remote pair generate
```

This command connects to the daemon over `/tmp/rorca-<UID>/daemon.sock` (`src-tauri/src/cli.rs:221-280`).
If the gateway is currently `Off`, this call auto-configures the daemon to `Relay` mode and establishes a relay connection (`src-tauri/src/daemon/server.rs:1948-1972`).

Output streams:
The PIN and pairing URL are written to standard output (`src-tauri/src/cli.rs:260-274`). Informational text is written to standard error.

Local network mode output:
```text
849201
```
Stderr: `Pairing registered by the running daemon; it holds the relay control connection.`

Relay mode output:
```text
849201
https://relay.checka.cc#pair=ab83cd...
```
Stderr: `Pairing registered by the running daemon; it holds the relay control connection.`

### Pairing Constraints and Lifetimes

* **Code Range:** Direct local pairing generates 6 decimal digits from `100000` to `999999` (`src-tauri/src/remote/auth.rs:376`). Relay pairing generates zero-padded 6 decimal digits from `000000` to `999999` (`src-tauri/src/remote/relay_client.rs:145`).
* **Expiration Window:** Codes expire 60 seconds from creation timestamp (`src-tauri/src/remote/auth.rs:383`).
* **Attempt Budget:** An in-memory failure counter allows up to 5 failed attempts in its window (`src-tauri/src/remote/auth.rs:269-286`). Subsequent attempts return HTTP 429 (`pairing_rate_limited` at `src-tauri/src/remote/server.rs:360`). This counter resets if the daemon restarts.
* **PIN Persistence:** Active pairing codes are included in best-effort saves to `~/.ferryx/remote/remote-auth.json` (`src-tauri/src/remote/auth.rs:768-779`). Consumption and expiry pruning occur during pairing operations (`src-tauri/src/remote/auth.rs:446-467`), not through guaranteed immediate deletion from disk at the expiry deadline. A failed save can leave older state on disk.

### Step 3: Client Exchange

The client sends an HTTP POST request to `/api/v1/pair/exchange`:

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

On success, the server responds with HTTP 200:

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

Error responses return plain text for authentication errors (`src-tauri/src/remote/server.rs:354-368`):
* Invalid PIN returns HTTP 400 with plain text `Invalid pairing code`.
* Expired PIN returns HTTP 401 with plain text `Pairing code expired`.
* Rate limiting returns HTTP 429 with JSON `{"code":"pairing_rate_limited"}`.

### Pre-approving from the CLI

To approve a PIN explicitly on the server before client exchange:

```bash
ferryx pair approve 849201
```

Output:
```text
Pairing approved for 849201; ready for remote client exchange
```

This creates a device entry named `cli-paired-device` in `~/.ferryx/remote/remote-auth.json` (`src-tauri/src/cli.rs:282`, `src-tauri/src/remote/auth.rs:579-645`). When the client exchanges the PIN, it receives the approved token. This approval targets local PIN exchange; it does not approve a separate relay token fragment (`src-tauri/src/remote/relay_client.rs:147-155`).

### Listing Paired Devices

To inspect authorized devices:

```bash
ferryx pair list
```

Output:
```text
7c8e...-uuid	Alice Laptop	Control
9f1a...-uuid	cli-paired-device	Control
```

### Pairing Subcommands and Revocation Scope

The Ferryx CLI supports `list`, `generate`, and `approve` subcommands (`src-tauri/src/cli.rs:152`).
The CLI does not have a device revocation command. Revoking a paired device requires the desktop GUI settings (`ui/src/components/settings/RemoteAccessSection.tsx:252-263`) or calling the HTTP revocation API (`POST /api/v1/devices/{id}/revoke` at `src-tauri/src/remote/server.rs:1175-1208`). The web remote interface does not offer a device revocation control.

### Device Token Lifetime and Invalidation

Device bearer tokens do not have a fixed expiration date while actively used. Tokens are invalidated in four ways (`src-tauri/src/remote/auth.rs:482-563,648-713,797-805`):
1. **Explicit Revocation:** Through the desktop UI or HTTP revocation API.
2. **Idle Expiration:** Devices inactive for longer than 30 days (`DEVICE_IDLE_EXPIRY_SECS = 2,592,000` seconds at `:651`) are pruned. Validation updates `lastSeenAt`, with writes throttled at `:664-667`.
3. **Re-pairing Same Installation:** If a device with an existing `installationId` pairs again, prior tokens for that device are deleted (`:493`).
4. **Auth State Deletion While Stopped:** Removing `~/.ferryx/remote/remote-auth.json` before the next daemon start prevents those persisted credentials from being loaded. Deleting it while the daemon is running is not revocation: a missing file leaves the in-memory state intact, and a later save can recreate it (`src-tauri/src/remote/auth.rs:735-779`). Use the revocation API for a running daemon.

### Control versus View Boundaries

Device permissions determine terminal and workspace capabilities:
* Devices with `Control` permission can send keyboard input and interrupts (`src-tauri/src/remote/server.rs:1477-1528,1850-1920`), create and delete worktrees (`:963-969,1090-1096`), and switch active workspaces (`:1136-1142`).
* Terminal resize requests are not restricted to Control permission (`:1511-1516,1885-1895`). View devices can resize terminals.
* Interrupt actions send the byte `0x03` (VINTR) to the PTY line discipline (`src-tauri/src/terminal/session.rs:210-215`). This relies on terminal line discipline handling rather than direct SIGINT delivery to every running child process.

---

## 8. Firewall and Port Configuration

Firewall rules below are external operational examples. The Ferryx daemon binds to resolved IP addresses and does not configure firewall chains or interfaces directly.

### Inbound Firewall Rules

* **Local Network Mode (`localNetwork`):**
  Open TCP port `43821` on your local network:
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
  Restrict ingress to the Tailscale interface:
  ```bash
  sudo ufw allow in on tailscale0 to any port 43821 proto tcp comment 'Ferryx Tailscale Gateway'
  ```

* **Relay Mode (`relay`):**
  Relay mode binds an internal loopback listener on `127.0.0.1:43821` (`src-tauri/src/remote/server.rs:2299-2304`) and initiates an outbound WebSocket connection to the relay server. No external inbound ports are opened. Ensure outbound connections to the relay URL are permitted. The default public relay uses TCP port 443 (`https://relay.checka.cc`), but custom relay URLs can use HTTP, WS, or nonstandard ports (`src-tauri/src/remote/relay_client.rs:538-549`).

---

## 9. Log Locations and Storage Categories

### Standard Stream and Journald Logs

When managed by systemd, stdout and stderr are captured in journald:

```bash
# Follow live daemon logs
journalctl --user -u ferryx-daemon.service -f

# View logs from current boot
journalctl --user -u ferryx-daemon.service -b
```

The headless daemon entry point initializes a filtered subscriber strictly for agent-state release events (`src-tauri/src/daemon/logging.rs:33-80`, `src-tauri/src/cli.rs:479`), recording to a private, bounded log file (`daemon.log`, max 1 MiB, mode 0600) rather than stdout. Journald captures the readiness token `FERRYX_DAEMON_READY` on stdout (`:536`) and fatal startup errors on stderr (`src-tauri/src/main.rs:39`).

### Storage Categories on Disk

Ferryx stores data across several categories:

* **Remote Gateway State (`~/.ferryx/remote/` or `$FERRYX_DATA_DIR/remote/`):**
  * `remote-config.json`: Persisted desired network mode, port, and control flags (`src-tauri/src/remote/state.rs:419-433`).
  * `remote-auth.json`: Paired devices, active tokens, and transient pairing PINs (`src-tauri/src/remote/auth.rs:735-779`).
  * `identity.json`: Machine Ed25519 identity keypair and display name (`src-tauri/src/remote/auth.rs:75-104`).
* **Persistent Locks (`~/.ferryx/locks/` or `$FERRYX_DATA_DIR/locks/`):**
  * `daemon.lock`: Inter-process coordination lock (`src-tauri/src/daemon/server.rs:270-298`).
* **Session Checkpoints:**
  * Session layout state: `$XDG_DATA_HOME/rorca/session_state.json` or `$HOME/.local/share/rorca/session_state.json` (`src-tauri/src/daemon/server.rs:301-361`). Fallback is `$HOME/.rorca/rorca/session_state.json`.
  * Remote session descriptors: `/tmp/rorca-<UID>/remote_sessions.json` (`src-tauri/src/daemon/server.rs:1013-1017`).
* **SSH Host Registry:**
  * Linux default: `$XDG_DATA_HOME/com.ferryx.app/ssh_hosts.json` or `$HOME/.local/share/com.ferryx.app/ssh_hosts.json` (`src-tauri/src/daemon/server.rs:861-873`). Only relocated to `<FERRYX_DATA_DIR>/ssh_hosts.json` when `FERRYX_DATA_DIR` is explicitly set.
* **Worktree Caches:**
  * `.orca-worktrees/` inside git repositories (`src-tauri/src/worktree/manager.rs:15`).
* **Client Browser Local Storage:**
  * Client web browsers and desktop webviews store bearer tokens and UI preferences in `localStorage` (`ui/src/App.tsx:2826`, `ui/src/lib/remoteClient.ts:17`). These tokens remain on the client machine and are not affected by server directory deletion.

### Storage Deletion Scope

To reset server state, stop the daemon first to prevent it from rewriting files.
Deleting `~/.ferryx/`, `$HOME/.local/share/rorca/` (or `$XDG_DATA_HOME/rorca/`), and `$HOME/.local/share/com.ferryx.app/` (or `$XDG_DATA_HOME/com.ferryx.app/`) removes configurations, keys, checkpoints, and SSH host records.
Also remove `/tmp/rorca-<UID>/` to clear runtime session descriptors and sockets.
Deleting server directories does not clear browser `localStorage` on client machines.

### Environment Variable Overrides

Set environment variables in your systemd unit or shell profile:

* `FERRYX_RUNTIME_DIR`: Relocates `/tmp/rorca-<UID>` (`src-tauri/src/daemon/server.rs:128`).
* `FERRYX_DATA_DIR`: Relocates `~/.ferryx/` (`src-tauri/src/daemon/server.rs:271`) and SSH host registry (`:862`). Does not relocate session checkpoints.
* `FERRYX_SESSION_DIR`: Relocates session checkpoint storage (`src-tauri/src/daemon/server.rs:44,302,1013`).
* `FERRYX_RELAY_URL`: Used during Off-to-Relay pairing (`src-tauri/src/daemon/server.rs:1956`) and IPC fallback (`src-tauri/src/ipc/remote.rs:307`). Headless servers should configure `"relayUrl"` in `remote-config.json` for deterministic startup restoration.
* `FERRYX_MACHINE_TOKEN`: Supplies a pre-shared token for relay tunnels instead of machine identity signatures (`src-tauri/src/remote/server.rs:2283-2293,2347-2365`).

---

## 10. Troubleshooting

### Problem 1: "Another daemon instance is already holding the lock"

**Symptom:** Daemon exits during startup with:
```text
Ferryx daemon error: Another daemon instance is already holding the lock.
```

**Cause:** A running process holds the flock on `/tmp/rorca-<UID>/daemon.lock` or `~/.ferryx/locks/daemon.lock` (`src-tauri/src/daemon/server.rs:575-582`). File locks are held by open file descriptors in the kernel; terminated processes cannot hold flocks. Any nonzero return code from `libc::flock` emits this error message.

**Resolution:**
1. Check if a ferryx process is running:
   ```bash
   pgrep -a ferryx
   ```
2. Identify which process holds the file lock:
   ```bash
   lsof /tmp/rorca-"$(id -u)"/daemon.lock
   lsof ~/.ferryx/locks/daemon.lock
   ```
3. If an earlier process crashed, its flock was released automatically by the operating system. The daemon cleans up stale sockets once it acquires the lock. Never remove lock files manually while a process is running.

### Problem 2: "Daemon runtime directory has mode ..., expected 700"

**Symptom:** Startup fails with:
```text
Daemon runtime directory /tmp/rorca-1000 has mode ..., expected 700
```
or:
```text
Path /tmp/rorca-1000 is a symlink, which is prohibited for daemon runtime
```

**Cause:** Ferryx validates that `/tmp/rorca-<UID>` is a real directory owned by your process real UID (`src-tauri/src/daemon/server.rs:439`). If mode access bits differ from `0700`, the daemon automatically attempts `chmod 0700` (`:479`). The error is raised only if the chmod call fails or if permissions still differ.

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

**Symptom:** Startup completes without listening on port 43821.

**Cause:** The interface resolver could not discover an active non-loopback IP address matching the requested mode (`src-tauri/src/remote/state.rs:194-235`):
* In `localNetwork` mode, the resolver tests a route probe to `8.8.8.8:80` and falls back to inspecting network interfaces via `getifaddrs`. Lack of a default route does not force failure if an active interface has an IPv4 address.
* In `tailscale` mode, the resolver scans for an address within the `100.64.0.0/10` CGNAT block (`100.64.0.0` through `100.127.255.255`). CGNAT detection does not prove that the Tailscale daemon is running or authenticated.

The daemon continues running its local UDS client accept loop (`src-tauri/src/daemon/server.rs:1500-1515`). Remote gateway restoration failure does not terminate the daemon process. Headless mode filters tracing to agent-state release events only, so gateway restoration warnings are not sent to journald.

**Resolution:**
1. In LAN mode, verify that your host has an active non-loopback IPv4 address:
   ```bash
   ip -4 addr show
   ```
2. In Tailscale mode, verify Tailscale status and interface address:
   ```bash
   tailscale status
   ip -4 addr show tailscale0
   ```
3. A failed gateway restore does not prevent local daemon operation. For a future start without remote access, set `"mode": "off"` in `~/.ferryx/remote/remote-config.json`. Avoid restarting a live daemon solely to clear this condition.

### Problem 4: Pairing Returns "Invalid pairing code" or "Pairing code expired"

**Symptom:** Exchanging a pairing code fails with HTTP 400 or HTTP 401.

**Cause:** Pairing PINs expire 60 seconds after creation (`src-tauri/src/remote/auth.rs:383`).
The server returns plain text error bodies (`src-tauri/src/remote/server.rs:354-368`):
* HTTP 400 returns `Invalid pairing code`.
* HTTP 401 returns `Pairing code expired`.
Expired codes may be pruned before lookup, resulting in `Invalid pairing code`.

**Resolution:**
Generate a fresh PIN with `ferryx pair generate` and complete the exchange immediately on the client.
If you exceed 5 failed attempts, the window enters rate limiting (`pairing_rate_limited`, HTTP 429). Waiting 60 seconds allows the rate-limiting window to expire, but does not revive an expired PIN. Generate a new code once the window clears.

### Problem 5: Missing `FERRYX_DAEMON_READY` in Automated Scripts

**Symptom:** Startup wrappers or test harnesses hang waiting for `FERRYX_DAEMON_READY`.

**Cause:** The CLI emits `FERRYX_DAEMON_READY` to stdout only when `handover_from` is `None` and the internal oneshot signal is received from the server task (`src-tauri/src/cli.rs:480-505`). The signal is sent after the local UDS listener binds, routes are adopted, and remote session descriptors are restored (`src-tauri/src/daemon/server.rs:1493`). Errors prior to listener binding or during route restoration prevent token emission. Fatal errors are printed to standard error (`src-tauri/src/main.rs:39`). Stdout printing runs on a separately scheduled task, so the token confirms a bound local listener rather than completed remote gateway restoration.

**Resolution:**
Monitor both stdout and stderr in your script. Capture stderr to identify initialization errors if the ready token does not arrive. A 10-second wait is an operator timeout choice, not an implementation guarantee.
