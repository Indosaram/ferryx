# SSH password authentication backend and verification

## Settings UI

- Removed the three top destination buttons. The machine list is the default; inbound access and diagnostics are disclosures below it.
- Added basic SSH authentication selection with Password, SSH Agent, and Key File. Password is passed separately before the connection probe and never enters the saved host DTO.
- Existing machines support password entry through Edit for reauthentication. Modal cancellation and successful addition clear the frontend password field.
- Fixed success-modal focus escape and competing row focus on dismissal.
- Remote settings regression suite: 17 passed. SettingsDialog navigation suites: 31 passed. Final combined tests and frontend build exited 0.
- Browser boundary-fixture checks: 34/34 passed at 1280 and 390 widths in the implementation session. Local-only screenshots and harness output are not committed. Image input was unavailable, so screenshot capture is not claimed as visual approval. Real SSH evidence is separate below.

Settings UI owns no persisted password field. `SshAuthMethod` adds `"password"`.

- `setSshPassword(host: SshHost, password: string): Promise<void>` -> `cmd_ssh_set_password { host, password }`
- `clearSshPassword(host: SshHost): Promise<void>` -> `cmd_ssh_clear_password { host }`

Call set before testing/saving password hosts. Existing frontend test/browse/project/helper/terminal/retry APIs remain unchanged. The Rust test command additionally injects managed DaemonClient state. Passwords are transient GUI/daemon memory, separate from serialized hosts. Missing credentials return `IO_ERROR` with `details.stage = "authentication"`. Restarting the credential-owning processes requires entry again.

## Implementation

- Password-mode OpenSSH disables public-key and keyboard-interactive authentication, uses one password prompt, and keeps strict host-key checking and connection-reuse restrictions.
- A loopback askpass broker hands the password to OpenSSH over a pipe through the same executable's early askpass entry point. Only a random capability and broker port enter the SSH child environment; password bytes do not enter argv, environment, host JSON, or log formatting. The daemon request uses a Debug-redacted password wrapper.
- GUI and daemon receive separate transient copies over existing local IPC (`sshPassword { host, password }`, null clears). Credential writes/probes are serialized at the GUI boundary. Rejected authentication clears the failed generation; stale failures cannot clear a replacement credential in the backend store.
- Probe, browse, upload, helper bridge, Windows state bridge, direct PTY spawn, and newly established reconnects share the broker setup.
- Updating host inventory does not clear just-set endpoint credentials.
- The existing daemon was not restarted or killed. An old daemon rejects the additive credential command; setter rolls back its GUI credential rather than claiming support. A compatible daemon binary is required for this feature. No automatic daemon migration was added.

## Observed verification (macOS arm64, 2026-09-15)

- `cargo check --manifest-path src-tauri/Cargo.toml --lib`: passed (19 existing warnings).
- `cargo test --manifest-path src-tauri/Cargo.toml --lib ssh:: -- --test-threads=4`: 153 passed at the full-suite checkpoint; one additional endpoint-lookup test subsequently passed in the four-test password module run.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib ssh::password:: -- --test-threads=4`: 4 passed, including redaction, transient broker exchange, endpoint isolation, stale-generation clear, default-user/alias/IPv6/jump argument lookup, and invalid input.
- `bun run --cwd ui test src/lib/sshHosts.test.tsx src/lib/sshHosts.identity.test.ts`: 17 passed.
- `cargo build --manifest-path src-tauri/Cargo.toml --example ssh_password_fixture`: passed.
- `/tmp/ferryx-password-fixture-venv/bin/python scripts/qa/ssh-password-fixture.py`: passed after isolated-fixture path corrections. It executes real `/usr/bin/ssh` against a password-only AsyncSSH server with a generated host key and pinned private known_hosts. The secret travels to the Rust fixture over stdin only.
- Live fixture exercised missing/wrong password, environment detection, directory browse, project path probe, streamed upload, real helper installation, foreground owned helper startup, two real helper bridge handshakes/project registrations, portable-pty SSH command output (sentinel not present verbatim in input), fresh-connection authentication, and clear. It also spawned a separate process running real `DaemonServer::handle_client`, and exchanged handshake/set/clear over its private stdio stream. No existing daemon socket was opened by the fixture.
- `git diff --check`: passed.

## Limits and remaining release verification

- Windows/Linux execution and Windows GUI-subsystem askpass stdout are not verified on this macOS host. The implementation uses portable TCP/stdin/stdout and the executable early-entry path, but this is not equivalent to platform runtime evidence.
- Actual packaged GUI invocation and safe migration of a running old daemon were not exercised. The isolated protocol fixture verifies the new server, not an old daemon's upgrade lifecycle.
- Full daemon-owned helper-backed terminal retry state-machine testing was not performed by this fixture; it verifies real bridge reconnect and direct PTY paths independently.
- Password-protected jump hosts and SSH alias configuration overrides require further end-to-end verification; argument endpoint lookup alone is not network evidence.
- Rust LSP requests timed out for all changed Rust paths. TypeScript diagnostics returned no errors. Compiler/test results above are available instead.
- `--no-default-features` check fails in existing `ipc/agents.rs` and `ipc/terminal.rs` references to feature-gated `native_terminal::surface_host`; those unrelated files were not repaired here.
- The implementation was not delivered with a strict failing-test-first sequence; new tests and the live fixture were added during implementation. No TDD claim is made.

All verification above was recorded before commit preparation. Raw local screenshots, process captures, and private-session data are intentionally not shipped.

## Follow-up: running app compatibility and actual daemon spawn

Read-only live inspection found a debug daemon on protocol 3, version `2026.915.1`, with two active sessions. Its capability response was `["machinePairingV1","pairedHostInventoryV1"]`. The GUI and development runner were active. No live upgrade request was sent during inspection.

Current source already supports non-destructive Unix handover:

1. `DaemonClient::upgrade_binary` supplies the new GUI executable path via `UpgradeBinary`.
2. With active sessions, `handle_upgrade_binary` prepares a private legacy listener and starts the successor with `--daemon --handover-from <legacy socket>`; it does not terminate existing PTYs.
3. Successor `run_server_with_handover_and_readiness` obtains the predecessor session list, persists routing, commits handover, acquires canonical ownership, and adopts the legacy peer.
4. The old daemon remains the owner of existing terminals while the new daemon serves the canonical socket and proxies legacy sessions. GUI stale-binary detection already schedules this path based on version/mtime.

Recommendation: build/package the new debug executable and let the existing GUI stale-binary mechanism perform this handover (or explicitly invoke the existing upgrade command), then verify the same two session IDs remain accessible and the canonical daemon advertises the new capability. Do not kill the predecessor daemon. This operation has not been executed in this worker, so current live GUI compatibility remains unverified rather than complete. On Windows the current upgrade handler returns `UpgradeUnsupported`; no session-preserving Windows upgrade exists in the inspected source.

Narrow change: new daemon capability responses now include `sshPasswordV1`, providing an observable distinction even when package version and protocol remain unchanged.

The password fixture was extended and rerun successfully: actual `register_remote_project`, `DaemonRequest::Spawn` through real `DaemonServer::handle_client`, helper-backed remote state reaching Connected via a watch subscription, generation-aware terminal input producing a non-echo sentinel, and `DaemonRequest::Close`. Helper and SSH children are privately owned. This closes the actual daemon-spawn coverage gap.

## Live debug handover and forced-disconnect retry observed

The debug bundle signature verified successfully (`codesign --verify --strict`), and its executable contained `sshPasswordV1`.

Between preflight and the guarded explicit upgrade request, the existing GUI automatic upgrade path completed the handover. The worker's predecessor-PID assertion stopped before sending a redundant UpgradeBinary request. Subsequent live evidence shows:

- The canonical successor daemon advertised protocol 3 and `sshPasswordV1`.
- The predecessor remained alive; the successor used `--daemon --handover-from` with a private legacy socket and the predecessor as its parent.
- Both original session IDs remained available and `describeSession` reported `running: true` through the canonical successor.
- The handover manifest routed both IDs to the predecessor legacy socket, and the GUI was running the debug bundle.
- Raw local process captures, session IDs, paths, and signature identity are omitted from the committed report.

The isolated fixture now forcibly aborts all fixture SSH connections while retaining the privately owned helper and its PTY. The Rust exercise subscribes before triggering the disconnect, awaits a higher-generation Connected state with a bounded timeout, then successfully executes a second non-echo terminal sentinel in the same daemon session. Output: `daemon forced-disconnect automatic retry preserved session and accepted input`. Example build and complete live fixture passed. An initial fixture trigger used bytes instead of AsyncSSH's string command value and timed out; correcting that fixture type mismatch produced the observed pass. No fixed sleeps or polling were added.

This supersedes the earlier live handover and forced-disconnect coverage limitations. Windows/Linux and packaged GUI password-form interaction remain separate unverified surfaces.
