# Guide Final Correction Ledger

This document maps all 51 audit items from the deployment guide coverage ledger in `docs/DOCUMENTATION_CLAIM_COVERAGE.md` to their final dispositions in `docs/HEADLESS_LINUX_SERVER_DEPLOYMENT_GUIDE.md`.

Dispositions use three categories:
* **Corrected:** Factual inaccuracies, path errors, misleading architectural claims, or omitted behavioral side effects updated to match source code.
* **Removed:** Erroneous promises or unsupported assertions eliminated from the deployment guide.
* **Supported recommendation:** Retained source-supported identifiers or illustrative schemas, and external operational advice requiring operator adaptation. Source-supported facts in this category are distinguished from unexecuted external procedures in each item's authority and resolution.

---

## 1. Summary of Dispositions

| Item Range | Section | Total Items | Corrected | Removed | Supported Recommendation |
| --- | --- | --- | --- | --- | --- |
| G:1 to G:26 | Introduction and Architecture | 5 | 3 | 0 | 2 |
| G:34 to G:83 | Prerequisites | 4 | 2 | 0 | 2 |
| G:89 to G:137 | Building and Installation | 4 | 3 | 0 | 1 |
| G:145 to G:192 | Daemon Mechanics, Sockets, Locks | 5 | 5 | 0 | 0 |
| G:197 to G:291 | Systemd Service Configuration | 5 | 2 | 1 | 2 |
| G:299 to G:411 | Remote Gateway Configuration | 6 | 5 | 0 | 1 |
| G:417 to G:531 | Pairing and Client Authentication | 8 | 6 | 1 | 1 |
| G:537 to G:562 | Firewall and Network Ports | 2 | 1 | 0 | 1 |
| G:566 to G:609 | Logs, Storage, and Overrides | 7 | 5 | 0 | 2 |
| G:615 to G:709 | Troubleshooting | 5 | 5 | 0 | 0 |
| **Total** | | **51** | **37** | **2** | **12** |

---

## 2. Detailed Audit Item Mappings

### Section 1: Introduction and Architectural Overview

#### G:1-3,9-14 - Launch Architecture
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/main.rs:6-46`, `src-tauri/src/cli.rs:451-518`, `src-tauri/src/lib.rs:1823-1831`
* **Resolution:** Replaced claims of a background daemon with explicit documentation that `--daemon` launches a foreground Tokio multi-thread runtime. Removed claims of complete absence of graphical libraries; noted that dynamic linkers still require runtime libraries. Excluded untested systemd examples from source code mechanics.

#### G:14 - Process Lifetime and State Persistence
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/daemon/server.rs:980-1093,1013-1017,1306-1380,1456-1523,1896-1913`, `src-tauri/src/remote/server.rs:2132-2161,2203-2380`
* **Resolution:** Qualified that child PTY terminal processes do not survive host reboots or daemon restarts. Session layout checkpoints and remote descriptors are persisted to disk, but live child processes terminate when the daemon exits.

#### G:18-23 - Naming and Compatibility Paths
* **Disposition:** Supported recommendation
* **Authority:** `src-tauri/tauri.conf.json:3-5`, `src-tauri/src/daemon/server.rs:126-168,270-298,830-836`, `src-tauri/src/remote/auth.rs:37-73`
* **Resolution:** Retained default literal paths (`/tmp/rorca-<UID>`, `daemon.sock`, `agent-state.sock`, `~/.ferryx/`). Documented that `FERRYX_RUNTIME_DIR`, debug suffix (`-dev`), and `FERRYX_DATA_DIR` qualify these locations. Noted that remote gateway state is isolated under `<data>/remote/`.

#### G:24 - Session State Path Resolution
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/daemon/server.rs:301-361`
* **Resolution:** Corrected session state resolution logic. On Linux, `dirs_next()` checks `$XDG_DATA_HOME/rorca/session_state.json` before `$HOME/.local/share/rorca/session_state.json`. `dirs_fallback()` appends `rorca/session_state.json` to `$HOME/.rorca` (yielding `$HOME/.rorca/rorca/session_state.json`). If no home base resolves, it falls back to `<runtime_dir>/session_state.json`. Debug runtime uses `session_state.dev.json` or `rorca-dev`. Clarified that this is normal Linux resolution, not merely legacy fallback.

#### G:25-26 - Branch and Cache Identifiers
* **Disposition:** Supported recommendation
* **Authority:** `src-tauri/src/worktree/manager.rs:15,350-382`
* **Resolution:** Retained `.orca-worktrees/` and `orca/<ws-id>/<slug>` naming conventions. Documented that these identify git worktree cache folders and branch namespaces rather than expendable logs.

---

### Section 2: Prerequisites

#### G:34 - Architecture Scope
* **Disposition:** Corrected
* **Authority:** `src-tauri/native_terminal/build_ghostty.rs:129-132,315-319`, `.github/workflows/build-test.yml:53-66`, `scripts/lib/release-platforms.mjs:576-624`
* **Resolution:** Classified `x86_64` as the official CI-tested and prebuilt release architecture. Classified `aarch64` as a supported source-build compilation target rather than an audited official binary.

#### G:35-37 - Systemd, User Accounts, and Utilities
* **Disposition:** Supported recommendation
* **Authority:** `src-tauri/src/daemon/server.rs:371-403,434-439`, `src-tauri/src/remote/auth.rs:86-94`, `src-tauri/src/worktree/manager.rs:1-5`
* **Resolution:** Reclassified systemd as an operational supervisor choice rather than a hard prerequisite. Replaced assertions that running as root will conflict with an operational risk assessment. Documented that `hostname` is resolved via `PATH` lookup with fallback to `"Ferryx machine"`, git is used for worktree operations, and `curl`, `ss`, and `lsof` are operational inspection tools.

#### G:41-79 - Unconditional GTK and WebKit Runtime Shared Libraries
* **Disposition:** Corrected
* **Authority:** `src-tauri/Cargo.toml:58-103,175-177`, `src-tauri/tauri.conf.json:90-98`, `src-tauri/src/main.rs:35`, `src-tauri/src/cli.rs:471-518`
* **Resolution:** Corrected assertion that GTK and WebKit packages supply build-time headers only. Even though headless mode bypasses display initialization, the binary dynamically links against GTK 3 and WebKitGTK 4.1 shared libraries (`libgtk-3.so.0`, `libwebkit2gtk-4.1.so.0`). The system dynamic linker requires these libraries at process launch.

#### G:81-83 - Build Toolchains
* **Disposition:** Supported recommendation
* **Authority:** `src-tauri/native_terminal/build_ghostty.rs:6-7,26-55`, `.github/workflows/build-test.yml:73-77,98-107`
* **Resolution:** Retained toolchain recommendations. Noted that Zig `0.16.0` prefix is strictly validated by `native_terminal/build_ghostty.rs`, while stable Rust and Bun represent verified CI toolchain policies.

---

### Section 3: Building and Installation

#### G:89-97 - Debian Package Layout
* **Disposition:** Supported recommendation
* **Authority:** `scripts/lib/release-platforms.mjs:609-622`, `src-tauri/tauri.conf.json:51-58,90-99`
* **Resolution:** Labeled package installation commands and `/usr/bin/ferryx` path as expected packaging conventions. Recommended checking the installed executable with `command -v ferryx`.

#### G:99-108 - AppImage Extraction and Shared Libraries
* **Disposition:** Corrected
* **Authority:** External AppImage mechanics, Linux dynamic linker (`ld.so`)
* **Resolution:** Replaced naive advice copying `squashfs-root/usr/lib/*` to `/usr/local/lib/` with suppressed errors. Recommended installing distribution runtime packages (`libgtk-3-0`, `libwebkit2gtk-4.1-0`) or running from an application root like `/opt/ferryx` using `LD_LIBRARY_PATH`.

#### G:110-137 - Source Build Mechanics and Target Artifact Directory
* **Disposition:** Corrected
* **Authority:** `src-tauri/Cargo.toml:16-26,48-56`, `src-tauri/build.rs:36-40`, `src-tauri/native_terminal/build_ghostty.rs:6,59-110`, `.github/workflows/build-test.yml:117-126`, `src-tauri/tauri.conf.json:6-10`
* **Resolution:** Documented that release artifacts emit to `src-tauri/target/release/` unless overridden by `CARGO_TARGET_DIR` or `--target`. Emphasized initializing submodules at the pinned commit before building.

#### G:102-107,130-137 plus G:14,297 - Web Assets for Remote Gateway
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/remote/server.rs:1956-2008,2043-2070`, `src-tauri/tauri.conf.json:47-50`
* **Resolution:** Added documentation that installing only the `ferryx` binary to `/usr/local/bin` leaves the remote web server without frontend assets. `resolve_dist_dir_from` searches `<exe_dir>/ui/dist`, `<exe_dir>/resources/ui/dist`, compile-time manifest dir, and working directory (`cwd/ui/dist`). If missing, it serves placeholder HTML. Instructed operators to install `ui/dist` into a searched directory for browser deployments.

---

### Section 4: Daemon Mechanics, Sockets, and Locks

#### G:145-154 - Runtime Directory Ownership and Mode Masking
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/daemon/server.rs:126-138,371-403,434-439,469,479,502-533`
* **Resolution:** Clarified that ownership checks validate real UID via `libc::getuid`, not effective UID. Documented that permissions check masks access bits with `0777` and attempts automatic `chmod 0700` repair before failing. Prohibited symlinks are checked via `symlink_metadata`.

#### G:156-162 - Asymmetric Socket Protection and Agent Socket Role
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/daemon/server.rs:831,1345-1380,1456-1486`
* **Resolution:** Corrected false claims that both sockets receive identical validation. The canonical IPC socket enforces strict validation: stale socket removal, safe ownership/non-symlink checks, and fatal exit on chmod `0600` failure. In contrast, the agent socket unconditionally removes existing files, binds, logs warnings on chmod failure without exiting, and returns nonfatally on bind failure. Clarified that the agent socket acts as an ingest receiver for newline-delimited agent reports, not a client subscription endpoint.

#### G:164-174 - Persistent Lock Scope and Contention Reporting
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/daemon/server.rs:168,270-298,537-583,696-730,1456-1460`
* **Resolution:** Documented that the persistent lock (`~/.ferryx/locks/daemon.lock`) is optional and skipped if neither `FERRYX_DATA_DIR` nor `HOME` resolves. Noted that `libc::flock` failure returns the identical error message (`Another daemon instance is already holding the lock`) for any nonzero code, including operating system errors.

#### G:176-191 - Readiness Token and Asynchronous Stdout Delivery
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/daemon/server.rs:1488-1515`, `src-tauri/src/cli.rs:471-518`
* **Resolution:** Documented that the internal oneshot signal is emitted after UDS listener binding and session restoration. The CLI wrapper awaits this signal on a separate task and prints `FERRYX_DAEMON_READY` to stdout. Because stdout printing is scheduled independently, stdout emission does not establish wall-clock ordering before remote gateway restoration or client accept loops. Noted that flush failures on stdout are ignored.

#### G:192 - Gateway Restoration Failure Handling
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/daemon/server.rs:1502-1504`
* **Resolution:** Documented that remote gateway bind errors log a warning and continue into the client accept loop without terminating the process. Noted that this warning is silenced in headless mode because no tracing subscriber is initialized.

---

### Section 5: Systemd Service Configuration

#### G:197 - User Service Environment Boundaries
* **Disposition:** Corrected
* **Authority:** systemd user manager design, environment isolation
* **Resolution:** Removed false claims that `systemd --user` guarantees the interactive shell environment. Documented that user service managers do not inherit interactive environment variables (such as customized `PATH`, SSH agent sockets, or Ferryx overrides) from `.bashrc` or `.zshrc`.

#### G:199-233 - Unit Directives and Execution Lifetime
* **Disposition:** Supported recommendation
* **Authority:** `src-tauri/src/cli.rs:471-518`
* **Resolution:** Labeled unit directives (`Type=exec`, restart settings, limits) as operational choices. Clarified that `Type=exec` considers startup complete on process launch and does not wait for `FERRYX_DAEMON_READY`.

#### G:235-241 - Lingering and Session Termination
* **Disposition:** Corrected
* **Authority:** systemd logind architecture
* **Resolution:** Replaced assertions that user processes always terminate on SSH disconnect with a conditional statement about logind session management. Explained that lingering retains user managers across session disconnects.

#### G:243-260 - Headless Tracing Subscriber Absence and Journal Logs
* **Disposition:** Removed
* **Authority:** `src-tauri/src/main.rs:35-45`, `src-tauri/src/bin/cli.rs:36-49`, `src-tauri/src/lib.rs:1823-1829`, `src-tauri/src/cli.rs:471-518`
* **Resolution:** Removed promises that `rorca daemon listening on ...` or tracing restoration warnings appear in the journal. Headless daemon launch does not initialize `tracing_subscriber`. Journald captures only `FERRYX_DAEMON_READY` on stdout and fatal errors on stderr.

#### G:262-291 - System-Wide Unit Alternative
* **Disposition:** Supported recommendation
* **Authority:** systemd administration conventions
* **Resolution:** Retained system-wide unit as an untested external operational example requiring operator substitution of user and group placeholders.

---

### Section 6: Remote Gateway Configuration

#### G:299-310 - Network Modes and Listener Scopes
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/remote/state.rs:20-60,99-112,194-235`, `src-tauri/src/remote/server.rs:2260-2344`
* **Resolution:** Clarified network mode scopes: `localNetwork` binds loopback and the primary IPv4 address, which may be public; `tailscale` matches any IP in `100.64.0.0/10` regardless of interface name or Tailscale daemon status; `relay` opens an internal loopback listener on `127.0.0.1:43821` alongside outbound reverse tunnels; `off` disables inbound gateway listeners without affecting client connections like SSH.

#### G:310 - Wildcard Address Binding Scope
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/remote/server.rs:2299-2344`, `src-tauri/src/remote/state.rs:147-163`, `src-tauri/src/bin/relay.rs:94-95`
* **Resolution:** Qualified that prohibition of `0.0.0.0` applies to the production remote gateway listener. Outbound UDP route probes bind `0.0.0.0:0`, and the standalone `ferryx-relay` binary binds TCP `0.0.0.0`.

#### G:312-317 - Fixed Port and Transport Protocols
* **Disposition:** Supported recommendation
* **Authority:** `src-tauri/src/daemon/server.rs:2366`, `src-tauri/src/remote/state.rs:20-24,419-433`, `src-tauri/src/remote/server.rs:2203-2238`
* **Resolution:** Retained port 43821 documentation. Noted that plain HTTP and WebSockets are used without TLS termination on internal listeners. Recommended TLS-terminating reverse proxies for untrusted networks.

#### G:319-328 - CLI Limitations and Off-to-Relay Auto-Configuration
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/cli.rs:300-325`, `src-tauri/src/daemon/server.rs:1948-1972`, `src-tauri/src/remote/state.rs:238-248,419-433,668-684`
* **Resolution:** Disclosed that while the CLI lacks a `remote enable` subcommand, running `ferryx pair generate` while the gateway is in `Off` mode auto-configures the daemon to `Relay` mode. Documented editing `remote-config.json` while stopped or restarting as the procedure for other modes.

#### G:330-374 - Configuration Examples, allowControl, and Tailscale Ranges
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/remote/state.rs:26-76,238-248`, `src-tauri/src/remote/server.rs:1477-1528`
* **Resolution:** Corrected that `"allowControl"` is stored in configuration, but server request authorization checks individual `device.permission` (Control vs View) rather than this configuration flag. Narrowed Tailscale IP examples from `100.x.y.z` to `100.64.0.0/10` (`100.64.0.0` through `100.127.255.255`). Instructed setting `"relayUrl"` in configuration for reliable startup restoration.

#### G:376-411 - Status Commands and Live Health Checks
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/cli.rs:350-412`, `src-tauri/src/remote/server.rs:325-330,2139`
* **Resolution:** Documented that `ferryx remote status` reads static configuration from disk without querying the running daemon over IPC. Noted that `ss` and `curl` probe local TCP and HTTP availability, not relay tunnel connectivity or overall application health. Removed claims about journal warnings.

---

### Section 7: Pairing and Client Authentication

#### G:417-428 - Lazy Identity Generation and PATH Hostname Resolution
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/remote/auth.rs:11-19,75-104,196-227,305-312,868-910`, `src-tauri/src/remote/server.rs:332-346,2287-2293`
* **Resolution:** Documented that machine identity generation is lazy, occurring on first pair exchange or signed Relay startup rather than every daemon launch. Clarified that `hostname` is resolved through standard `PATH` lookup with fallback to `"Ferryx machine"`. Noted that reading existing identity files does not repair permissions.

#### G:430-452 - Pair Generate Command and Stream Separation
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/cli.rs:152-170,221-280`, `src-tauri/src/daemon/server.rs:1948-1972`
* **Resolution:** Documented stream separation: PIN and pairing URL are emitted to stdout, while informational status messages are emitted to stderr. Disclosed that generating pairing codes while in `Off` mode auto-configures Relay mode.

#### G:454-459 - PIN Number Ranges and Rate Limiting Windows
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/remote/auth.rs:269-286,374-393,446-467`, `src-tauri/src/remote/relay_client.rs:131-155,217`, `src-tauri/src/remote/server.rs:354-368`
* **Resolution:** Differentiated PIN numeric ranges: direct local PINs span `100000` to `999999`, while Relay PINs span zero-padded `000000` to `999999`. Documented that expiration occurs 60 seconds from creation timestamp. Clarified that rate limiting allows up to 5 failed attempts in an in-memory window before returning HTTP 429 (`pairing_rate_limited`). Budget resets on daemon restart.

#### G:459 and G:584 - Disk Persistence of Pairing Codes
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/remote/auth.rs:289-298,331-338,374-393,446-467,735-779`
* **Resolution:** Corrected claims that pairing PINs exist solely in memory. Active pairing codes are included in best-effort saves to `~/.ferryx/remote/remote-auth.json`. Pairing operations consume or prune them; expiry does not guarantee immediate disk erasure, and failed saves can leave older persisted state.

#### G:461-493 - Pair Exchange Response Schema
* **Disposition:** Supported recommendation
* **Authority:** `src-tauri/src/remote/server.rs:332-378`, `src-tauri/src/remote/auth.rs:208-227,469-573`
* **Resolution:** Retained exchange request and response examples as illustrative schemas. Documented that authentication error responses return plain text.

#### G:495-508 - CLI Pre-approval and Relay Token Limitations
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/cli.rs:282-289`, `src-tauri/src/remote/auth.rs:482-533,579-645`, `src-tauri/src/remote/relay_client.rs:147-155`
* **Resolution:** Documented that `ferryx pair approve` pre-approves the local PIN in `remote-auth.json`. Clarified that this does not approve a separate relay token fragment.

#### G:510-527 - Revocation Scope and Web Interface Absence
* **Disposition:** Removed
* **Authority:** `src-tauri/src/cli.rs:152-170,221-236`, `src-tauri/src/ipc/remote.rs:338-373`, `src-tauri/src/remote/server.rs:1156-1208,2153-2154`, `ui/src/components/settings/RemoteAccessSection.tsx:252-263`
* **Resolution:** Removed claims of a web user interface revocation control. Device revocation requires the desktop GUI settings or calling `POST /api/v1/devices/{id}/revoke`. Noted that the CLI does not provide a revocation subcommand.

#### G:529-531 - Token Lifetime and Invalidation Alternatives
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/remote/auth.rs:482-501,536-563,648-675,693-713,735-805`
* **Resolution:** Documented explicit revocation, 30-day idle expiration, re-pairing with the same `installationId`, and authentication file deletion while the daemon is stopped. Deleting the file while the daemon runs does not clear in-memory tokens and can be undone by its next save. Noted that `lastSeenAt` is updated on validated API requests with throttled writes, not continuous socket pings.

---

### Section 8: Firewall and Port Configuration

#### G:537-560 - Inbound Firewall Rules
* **Disposition:** Supported recommendation
* **Authority:** `src-tauri/src/remote/state.rs:194-235`, `src-tauri/src/remote/server.rs:2310-2344`
* **Resolution:** Retained UFW, firewalld, and nftables rules as external operational examples. Noted that the Ferryx daemon binds to resolved IP addresses and does not inspect firewall configuration.

#### G:561-562 - Relay Outbound and Internal Listener
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/remote/server.rs:2299-2304`, `src-tauri/src/remote/relay_client.rs:538-549`
* **Resolution:** Documented that Relay mode maintains an internal loopback listener on `127.0.0.1:43821` alongside outbound reverse tunnels. Clarified that while default public relay uses TCP port 443, custom relay URLs can use HTTP, WS, or nonstandard ports.

---

### Section 9: Logs, Storage Categories, and Overrides

#### G:566-576 - Journald Logs and Stream Capture
* **Disposition:** Supported recommendation
* **Authority:** `src-tauri/src/cli.rs:503-504`, `src-tauri/src/main.rs:39-40`
* **Resolution:** Retained journalctl commands as external inspection advice. Documented that standard streams capture `FERRYX_DAEMON_READY` on stdout and fatal errors on stderr.

#### G:580-590 - File Inventory and Desired State
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/remote/state.rs:419-433,668-684`, `src-tauri/src/remote/auth.rs:37-104,289-298`, `src-tauri/src/daemon/server.rs:270-298,301-361,1013-1017,1064-1093`
* **Resolution:** Documented that `remote-config.json` stores desired rather than active state. Clarified that remote descriptors default to `/tmp/rorca-<UID>/remote_sessions.json` and move with `FERRYX_SESSION_DIR`. Documented Linux session path honoring `$XDG_DATA_HOME`.

#### G:591-592 - SSH Host Registry Path
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/daemon/server.rs:861-873`, `src-tauri/src/ipc/ssh.rs:73-86`, `src-tauri/tauri.conf.json:5`
* **Resolution:** Corrected the claimed default path. On Linux, the SSH host registry defaults to `$XDG_DATA_HOME/com.ferryx.app/ssh_hosts.json` or `$HOME/.local/share/com.ferryx.app/ssh_hosts.json` (debug: `.../dev/ssh_hosts.json`), not `~/.ferryx/ssh_hosts.json`. Only explicit `FERRYX_DATA_DIR` redirects it to `<dir>/ssh_hosts.json`.

#### G:594-599 - Storage Categories and Deletion Scope
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/daemon/server.rs:44,270-298,301-361,861-873,1013-1017`, `ui/src/lib/remoteClient.ts:6-30`, `ui/src/App.tsx:2824-2863`
* **Resolution:** Categorized storage across six areas: daemon config/locks, session layout state, SSH host registry, runtime descriptors/sockets, worktrees, and client browser `localStorage`. Instructed stopping the daemon before deletion to avoid state recreation. Clarified that client browser tokens and preferences are independent of server directory deletion.

#### G:601-607 - Directory Overrides
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/daemon/server.rs:43-46,126-154,270-298,301-337,861-873,1013-1017`, `src-tauri/src/remote/auth.rs:37-63`
* **Resolution:** Documented that `FERRYX_DATA_DIR` relocates `~/.ferryx/` and SSH host registry, but does not relocate session checkpoints (which require `FERRYX_SESSION_DIR`) or runtime sockets (which require `FERRYX_RUNTIME_DIR`).

#### G:608 - FERRYX_RELAY_URL Startup Restore Behavior
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/daemon/server.rs:1956-1962`, `src-tauri/src/remote/server.rs:2270-2293`, `src-tauri/src/ipc/remote.rs:304-312`
* **Resolution:** Corrected claims that `FERRYX_RELAY_URL` universally overrides startup configuration. The daemon reads `config.relay_url` from `remote-config.json` when restoring listeners at startup without checking the environment variable. `FERRYX_RELAY_URL` is only used when auto-configuring from Off during pairing or as an IPC response fallback. Instructed operators to persist `"relayUrl"` in `remote-config.json`.

#### G:609 - Machine Token Pre-shared Credential
* **Disposition:** Supported recommendation
* **Authority:** `src-tauri/src/remote/server.rs:2283-2293,2347-2365`
* **Resolution:** Retained `FERRYX_MACHINE_TOKEN` documentation as a pre-shared credential for relay tunnels that bypasses machine identity keypair loading.

---

### Section 10: Troubleshooting

#### G:615-634 - Lock Contention and Flock Holding Mechanics
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/daemon/server.rs:575-582,589-600,1456-1460`, `src-tauri/src/main.rs:39-40`
* **Resolution:** Corrected explanation of lock contention. Dead processes cannot hold open file descriptors or kernel flocks. Contention indicates a live process or an unreleased file handle. Documented that any nonzero `libc::flock` return code emits the generic error message.

#### G:636-659 - Runtime Directory Permissions and Automatic Repair
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/daemon/server.rs:371-403,465-485,502-533`
* **Resolution:** Documented that if runtime directory permissions differ from `0700`, the daemon automatically attempts `chmod 0700` before failing. Startup fails only if chmod fails or permissions still do not match. Retained diagnostic commands as external suggestions.

#### G:661-688 - Resolver Fallbacks and CGNAT Matching Scope
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/remote/state.rs:147-163,194-235`, `src-tauri/src/daemon/server.rs:1500-1515`
* **Resolution:** Corrected causal assertions: lack of a default route does not force LAN failure because interface enumeration acts as a fallback; CGNAT detection in Tailscale mode checks address range (`100.64.0.0/10`) without verifying Tailscale service state. Documented that gateway restoration failure does not terminate the daemon and that warnings are silenced in headless mode.

#### G:690-700 - Plain Text Authentication Errors and Expiration Windows
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/remote/server.rs:354-368`, `src-tauri/src/remote/auth.rs:196-199,269-286,446-467`
* **Resolution:** Replaced JSON error examples with plain text error bodies (`Invalid pairing code` for HTTP 400, `Pairing code expired` for HTTP 401). Clarified that only HTTP 429 returns JSON. Noted that expired codes may be pruned before lookup, yielding HTTP 400. Documented that waiting 60 seconds allows the rate-limiting window to expire but does not revive an expired PIN.

#### G:702-709 - Readiness Signal Scheduling and Operator Timeouts
* **Disposition:** Corrected
* **Authority:** `src-tauri/src/daemon/server.rs:1490-1515`, `src-tauri/src/cli.rs:480-518`, `src-tauri/src/main.rs:39-40`
* **Resolution:** Corrected ordering description: the internal oneshot signal is sent when local UDS listener binding and session restoration complete. Stdout readiness is printed on a separately scheduled task, so it does not establish wall-clock ordering before remote gateway restoration or client accept loops. Documented that initialization errors prior to internal readiness suppress token emission and print fatal errors to stderr. Clarified that 10 seconds is an operator timeout choice rather than a guaranteed bound.
