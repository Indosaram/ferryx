# L7 — Daemon, Transport, Discovery, Locking, Handover Audit

Scope: `src-tauri/src/daemon/`, `src-tauri/src/remote/`, `src-tauri/src/ipc/` (framing/`run_blocking`).
Out of scope: launchd packaging/autostart (lane 6), shell spawning internals (lane 8).

Baseline fact confirmed from current code: the Windows daemon transport **is** implemented and
reachable — `src-tauri/src/daemon/server.rs:1102` binds a loopback `TcpListener` and
`src-tauri/src/daemon/client.rs:373-378` reads the port back from a `daemon.port` file and connects
over TCP. The `DaemonLockFile` type also has a real Windows implementation using
`LockFileEx`/`UnlockFileEx` (`src-tauri/src/daemon/server.rs` Windows `impl` block). This is not an
ungated `UnixListener` compile blocker; the platform split is intentional and mostly complete.
The defects below are in the *quality and completeness* of that Windows path (no auth, weaker trust
checks, missing subsystems) and in a few unconditionally-POSIX helper calls that silently degrade
functionality on Windows.

### Windows daemon transport has no authentication token
- **ID**: L7-DAEMON-IPC-1
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/server.rs:1102` — `let listener = TcpListener::bind("127.0.0.1:0")`
- **Why it breaks**: On Unix the daemon socket is protected by filesystem trust: a 0700 runtime directory, a 0600 socket file, ownership checks (`validate_runtime_socket_path_for_uid`), and `flock`-based single-instance locking. On Windows the equivalent transport is a loopback TCP port with **no protocol-level auth/token/handshake secret** — any other process running as the same Windows user (or, if `%TEMP%`/`LOCALAPPDATA` ACLs are ever misconfigured or the port number leaks via `daemon.port`, any local process that can read that file) can connect and issue arbitrary `DaemonRequest`s including `Write`, `Spawn`, and `Shutdown`. The only gate is `validate_runtime_socket_path_for_uid` for `not(unix)` (`server.rs:456-463`), which does not check ownership at all (`_expected_uid` is unused) and only verifies the path is not a symlink and not a directory.
- **Fix**: Add a per-boot random shared-secret token written alongside `daemon.port` (e.g. `daemon.token`, 0600-equivalent via `LOCALAPPDATA` ACL) and require it in the `Handshake` request on the `#[cfg(not(unix))]` path in `handle_client` (`server.rs`), rejecting connections that omit or mismatch it before processing any other `DaemonRequest` variant.
- **Status**: OPEN

### Windows runtime-path validation performs no ownership/ACL check
- **ID**: L7-DAEMON-IPC-2
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/server.rs:462` — `validate_safe_ownership_and_type_for_uid(runtime_dir, RuntimeNodeKind::Directory, 0)?;`
- **Why it breaks**: `validate_runtime_socket_path_for_uid` for `not(unix)` (`server.rs:456-465`) hard-codes UID `0` and the underlying `validate_safe_ownership_and_type_for_uid` `not(unix)` branch (`server.rs:371-398`, confirmed by reading the function body) never compares an owner at all — it only rejects symlinks and wrong node types. The Unix path performs a real UID-equality check (`meta.uid() != expected_uid`) before ever trusting the socket. On Windows there is no equivalent check against the Windows SID/owner of the runtime directory or port file, so the "port file trust" story relies entirely on default NTFS ACLs of `LOCALAPPDATA`/`%TEMP%` never having been loosened (e.g. by a misconfigured multi-user machine or roaming profile share).
- **Fix**: On Windows, use `GetNamedSecurityInfoW`/`GetFileSecurityW` (via `windows-sys`, already a dependency for `LockFileEx`) to read the file/directory owner SID and compare it against the current process token's SID in `validate_safe_ownership_and_type_for_uid`'s `not(unix)` branch, mirroring the Unix UID check instead of accepting any owner.
- **Status**: OPEN

### Agent-state extension socket is never bound on Windows
- **ID**: L7-DAEMON-IPC-3
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/server.rs:1001` — `#[cfg(unix)]` (guarding `fn spawn_agent_state_listener`)
- **Why it breaks**: `spawn_agent_state_listener` (a `UnixListener` bound at `agent-state.sock`) is only compiled and only invoked under `#[cfg(unix)]` (call site `server.rs:1130` right after `#[cfg(unix)]` at `server.rs:1129`). There is no `#[cfg(not(unix))]` TCP/named-pipe fallback. Meanwhile every spawned terminal on every platform unconditionally exports `FERRYX_AGENT_STATE_SOCKET=<path>` pointing at that same never-created path (`src-tauri/src/terminal/pty.rs:136-137`, consumed by `crate::daemon::agent_state_socket_path()`). On Windows, agent CLIs (Claude, Codex, etc.) that try to report `working`/`blocked`/`idle` state over that socket will always fail to connect, so the daemon's `AgentStateReport` stream (used by `parse_agent_state_report`) never receives real agent-state updates — the UI silently falls back to screen-scraping inference for every agent on Windows.
- **Fix**: Add a `#[cfg(not(unix))]` variant of `spawn_agent_state_listener` that binds a Windows named pipe (`\\.\pipe\ferryx-agent-state-<uid-or-session>`) or a second loopback `TcpListener` with the same line-delimited `AgentStateReport` JSON protocol, and change `agent_state_socket_path()` (`server.rs:802`) to return that pipe/port descriptor instead of a `.sock`-style path on Windows.
- **Status**: OPEN

### Agent-state extension installer only checks `HOME`, never installs on Windows
- **ID**: L7-DAEMON-IPC-4
- **Severity**: LOW
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/agent_extension.rs:25` — `std::env::var_os("HOME").map(PathBuf::from)`
- **Why it breaks**: The body of `home_dir()` reads only `std::env::var_os("HOME")` with no Windows fallback. `HOME` is typically unset for native Windows processes (the per-user equivalent is `USERPROFILE`/`%APPDATA%`). `extension_dirs()` calls `home_dir()` and returns an empty `Vec` when it is `None`, so `install_agent_state_extension()` (invoked unconditionally at `server.rs:1132` right after the `#[cfg(unix)]`-gated agent-state listener) silently installs nothing on Windows — compounding L7-DAEMON-IPC-3, since even if the transport were fixed, the extension file that talks to it would never be deployed into `~/.omo`, `~/.pi`, `~/.omp` equivalents.
- **Fix**: In `home_dir()`, add `.or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))` (matching the pattern already used correctly in `src-tauri/src/remote/state.rs`'s `DATA_DIR_SOURCES` for Windows) so `extension_dirs()` resolves a real per-user directory on Windows.
- **Status**: OPEN

### Agent-provider process discovery hard-codes `/bin/ps`, unconditionally no-ops on Windows
- **ID**: L7-DAEMON-IPC-5
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/agents.rs:72` — `let output = crate::util::no_window_command("/bin/ps")`
- **Why it breaks**: `process_table_entries()` shells out to the absolute Unix path `/bin/ps` with BSD/GNU-style flags (`-axwwo pid=,ppid=,args=`). This function backs `discover_agent_session_id`, which is called directly by the daemon's `DaemonRequest::DiscoverAgentSession` handler (`src-tauri/src/daemon/server.rs`, `Ok(DaemonRequest::DiscoverAgentSession { .. })` arm) to find a descendant agent CLI PID for provider-session resume. On Windows, `/bin/ps` does not exist, so the spawn fails and `.ok()?` turns it into `None` — `discover_agent_session_id` degrades to `None` for every agent type on every call, meaning provider-session resume (e.g. reattaching to a Claude/Codex conversation ID after a daemon restart) is unconditionally disabled on Windows with no diagnostic surfaced to the user.
- **Fix**: Add a `#[cfg(windows)]` implementation of `process_table_entries()` (or an OS-specific trait) using the `windows-sys` `CreateToolhelp32Snapshot`/`Process32NextW` APIs (already partially available as a dependency) to enumerate `(pid, ppid, args)` tuples, or shell out to `wmic process get ProcessId,ParentProcessId,CommandLine` / PowerShell `Get-CimInstance Win32_Process` as an interim fix, gated behind `#[cfg(target_os = "windows")]` next to the existing `#[cfg(target_os = "linux")]`/`#[cfg(target_os = "macos")]` split already used in the same file's `process_cwd`.
- **Status**: OPEN

### `process_cwd` returns `None` unconditionally on Windows
- **ID**: L7-DAEMON-IPC-6
- **Severity**: LOW
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/ipc/terminal.rs:977` — `#[cfg(not(any(target_os = "linux", target_os = "macos")))]`
- **Why it breaks**: `process_cwd(pid)` has real implementations for Linux (`/proc/{pid}/cwd`) and macOS (`proc_pidinfo`/`lsof` fallback), and an explicit `None` stub for every other target, including Windows. This feeds `cmd_terminal_get_cwd`'s cache-miss path and the agent-provider `cwd`-keyed session lookups in `src-tauri/src/ipc/agents.rs` (`antigravity_session_id`, `opencode_session_id`, both call `crate::ipc::terminal::process_cwd`). Those lookups always fail on Windows, so any agent-resume path that keys off the shell's live working directory (rather than the daemon's own tracked `cwd`) cannot resolve on Windows.
- **Fix**: Add a `#[cfg(target_os = "windows")]` branch to `process_cwd` using `NtQuerySystemInformation`/`QueryFullProcessImageName`-adjacent APIs is not sufficient for CWD; instead use the Windows `GetProcessImageFileName` combined with `NtQueryInformationProcess(ProcessBasicInformation)` to read the PEB `ProcessParameters->CurrentDirectory`, or, as a pragmatic first step, have callers in `agents.rs` prefer the daemon-tracked session `cwd` (already available via `DaemonSessionDetails::cwd`) over `process_cwd` on Windows instead of returning `None`.
- **Status**: OPEN

### Handover (rolling daemon upgrade without dropping sessions) is entirely unsupported on Windows
- **ID**: L7-DAEMON-IPC-7
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/handover.rs:131` — `Err("Handover unsupported on Windows".to_string())`
- **Why it breaks**: `prepare_handover` is `#[cfg(unix)]` for the real implementation and has a `#[cfg(not(unix))]` stub that always errors (`handover.rs:126-131`); `server.rs`'s `PrepareHandover` request arm additionally returns `DaemonResponse::HandoverRejected` on `#[cfg(not(unix))]` (`server.rs:1613-1617`), and `handle_upgrade_binary` on `#[cfg(not(unix))]` always returns `DaemonResponse::UpgradeUnsupported` (`server.rs:1766-1772`) without ever attempting a session-preserving restart. This is a graceful degradation, not a crash, but it means on Windows every daemon binary upgrade (auto-update) either leaves the old daemon binary running until the user fully quits the app, or (if forced) drops every live PTY session — the rolling-handover UX that Unix users get is a hard feature gap on Windows.
- **Fix**: Implement a Windows-native handover using a second named pipe (or ephemeral TCP listener, matching L7-DAEMON-IPC-3's fix) for the legacy peer, since `windows-sys`'s `LockFileEx`/`UnlockFileEx` already provide the byte-range lock primitive needed to hand off `DaemonLockFiles`; wire `HandoverManager::prepare_handover`'s `#[cfg(not(unix))]` arm to bind that listener and return it the same way the Unix arm returns a `UnixListener`, instead of an unconditional `Err`.
- **Status**: OPEN

### Windows loopback TCP port is guessable/enumerable without needing the port file
- **ID**: L7-DAEMON-IPC-8
- **Severity**: LOW
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/remote/server.rs:1562` — `_ => "0.0.0.0",`
- **Why it breaks**: This is the separate remote-control gateway (not the primary daemon IPC socket), and binding `0.0.0.0` is required for its cross-device feature, so it is not itself a defect. It is flagged here because it compounds L7-DAEMON-IPC-1: on Windows both the primary daemon (loopback TCP, no token) and the optional remote gateway (all-interfaces TCP, PIN-paired) sit behind Windows Firewall's per-app prompt, and there is no code anywhere in `src-tauri/src/remote/` or `src-tauri/src/daemon/` that provisions an explicit inbound-allow firewall rule during install or first bind — the OS default "Block" action for private/public networks combined with a dismissed or auto-blocked first-run prompt would leave the remote gateway silently unreachable from other devices with no in-app diagnostic distinguishing "blocked by firewall" from "network unreachable."
- **Fix**: On `start_remote_server`'s Windows path, after `TcpListener::bind` succeeds, attempt to detect firewall reachability (e.g. a loopback-external probe) and surface a specific `RemoteConfigureError` variant such as `FirewallBlocked` distinguishable from generic bind failures, or provision the inbound rule via `netsh advfirewall firewall add rule` / the Windows Firewall COM API (`INetFwPolicy2`) during daemon Windows install, gated `#[cfg(target_os = "windows")]`.
- **Status**: OPEN

### `resolve_binary_identity`/hot-upgrade re-exec relies on POSIX `exec()`, has no Windows re-exec equivalent
- **ID**: L7-DAEMON-IPC-9
- **Severity**: LOW
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/server.rs:225` — `let mut cmd = std::process::Command::new(exe);`
- **Why it breaks**: `perform_daemon_exec_with_path` (`server.rs:216-227`) is `#[cfg(unix)]` and uses `CommandExt::exec()` to replace the current process image in place, preserving the PID and any inherited descriptors. There is no `#[cfg(not(unix))]` counterpart function at all in this file — the only Windows path for "run a new daemon binary" is the handover-based `spawn_legacy_handover_daemon` (also `#[cfg(unix)]`, see L7-DAEMON-IPC-7) or `tokio::time::sleep` + `perform_daemon_exec_with_path` in `handle_upgrade_binary`'s empty-sessions branch, which is itself `#[cfg(unix)]` only. Combined with L7-DAEMON-IPC-7, this means the empty-sessions "fast path" self-upgrade (`server.rs`, `active_sessions.is_empty()` branch inside `handle_upgrade_binary`) is unreachable on Windows, and `handle_upgrade_binary`'s `#[cfg(not(unix))]` stub (`server.rs:1766-1772`) is the only code path Windows ever takes for a daemon binary upgrade, regardless of whether sessions are active.
- **Fix**: Add a `#[cfg(windows)]` `perform_daemon_exec_with_path` that spawns the new binary with `Command::new(exe).arg("--daemon").spawn()` and then calls `std::process::exit(0)` in the current process after handing off (accepting the PID change, since Windows has no `execve`-style in-place replace), and use it for the empty-sessions fast path in `handle_upgrade_binary`'s Windows arm instead of unconditionally returning `UpgradeUnsupported`.
- **Status**: OPEN

### `get_persistent_lock_path` Windows branch has no `LOCALAPPDATA`-absent test coverage matching the Unix branch
- **ID**: L7-DAEMON-IPC-10
- **Severity**: LOW
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/server.rs:246` — `#[cfg(windows)]`
- **Why it breaks**: `get_persistent_lock_path` (`server.rs:242-256`) correctly falls back `APPDATA` → `USERPROFILE`/.ferryx on Windows, which is good, but if *both* env vars are absent (e.g. a locked-down service account or sandboxed test runner) the function returns `None` and `acquire_daemon_locks` in `run_server_with_handover_and_readiness` (`server.rs:1087`) is called with `get_persistent_lock_path().as_deref()`, i.e. `None`, silently skipping the persistent (survives-reinstall) lock and relying only on the per-runtime-dir legacy lock. This is the same graceful-degradation behavior as Unix's `HOME`-absent case, so it is not a differential bug, but there is no test in `server.rs`'s `#[cfg(test)]` module exercising the Windows env-var-absent path the way Unix paths are exercised, so a future regression that panics instead of degrading gracefully on Windows would not be caught by CI (which only runs on macOS per the audit's stated baseline).
- **Fix**: Add a unit test under a `#[cfg(windows)]`-gated (or env-var-injectable, matching the existing `resolve_session_path` test pattern at `server.rs:64-113`) test module that clears `APPDATA`/`USERPROFILE`/`FERRYX_DATA_DIR` and asserts `get_persistent_lock_path` returns `None` without panicking, and that `acquire_daemon_locks(None, &legacy_path)` still succeeds using only the legacy lock.
- **Status**: OPEN

### Windows TCP daemon transport has no localhost-only IPv6/dual-stack consideration
- **ID**: L7-DAEMON-IPC-11
- **Severity**: LOW
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/daemon/client.rs:379` — `DaemonStream::connect(format!("127.0.0.1:{port}")).await`
- **Why it breaks**: Both the server bind (`server.rs:1102`, `"127.0.0.1:0"`) and the client connect (`client.rs:379`) hard-code IPv4 loopback. On a Windows machine where IPv4 loopback is disabled or filtered by endpoint security software (uncommon but seen in locked-down enterprise images that only permit `::1`), the daemon would fail to bind or the client would fail to connect with no automatic IPv6 retry, whereas the Unix Domain Socket path has no such address-family dependency at all.
- **Fix**: If Windows enterprise-image compatibility matters, add an IPv6 loopback (`[::1]:0`) fallback in both `server.rs`'s `#[cfg(not(unix))]` bind and `client.rs`'s `#[cfg(not(unix))]` `connect_socket`, storing the resolved `SocketAddr` (not just a bare port number) in `daemon.port` so the client does not have to guess the address family.
- **Status**: OPEN
