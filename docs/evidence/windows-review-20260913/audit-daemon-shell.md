# Windows audit: daemon-shell

Status: COMPLETE (source review); lane daemon-shell; task st_01a09847; 2026-09-13.
Baseline: HEAD b7ad4516; inspected current working tree, including foreign PTY changes.
Only this report was written. No builds, tests, desktop launches, daemon probes, or process manipulation.
"Confirmed" below means a source-confirmed defect, NOT reproduced Windows runtime behavior.
All failing-first tests and binary observables below are proposals for the later fix phase.
Source spans and their callers were re-opened. LSP server symbols and ast-grep command-call search used.
Read root/backend/daemon/terminal AGENTS, programming/debugging/ast-grep skills, prior cross-platform audit,
and windows-terminal-20260912/FINAL-AUDIT.md. Installed 2026-08-28 binary is stale, not this source.

## Domain coverage and disposition

| Path / boundary reviewed | Disposition |
| --- | --- |
| daemon/server.rs runtime directory, port publication, LockFileEx, request dispatch | DS-01/07; actual Windows locking exists; IPv4 loopback explicitly selected |
| daemon/client.rs connect/spawn/readiness, control retry, attach and epoch | DS-02; bounded readiness; ambiguous writes not replayed; attach timeout unknown below |
| daemon/protocol.rs | Protocol 3, camelCase/base64, explicit mismatch; handshake has version only (DS-01) |
| daemon/handover.rs, manifest.rs, proxy.rs | DS-08; Unix route preservation exists; Windows proxy TCP exists but normal Windows prepare/upgrade rejected |
| daemon/launchd.rs; cli.rs; main.rs; lib.rs daemon wiring | On-demand spawn cross-platform; launchd install has no external production caller; login autostart not shipped |
| daemon/agent_extension.rs, agent_state.rs; resources/agent-extensions/ferryx-agent-state.ts | DS-05/06; retained state subscription itself platform-neutral |
| terminal/shell.rs all Windows explicit/default/custom and all provider-resume plans | Four Windows shell choices implemented; DS-03/04 affect agent executable resolution |
| terminal/pty.rs, session.rs, service.rs; portable-pty 0.9.0 Windows backend | Close, interrupt, reaping, I/O ownership reviewed; descendant-lifetime qualification below |
| terminal/output_hub.rs, metrics.rs, mod.rs | Shared sequenced bytes and resize ledger; snapshot/subscription lock; no Windows-specific transport fork |
| terminal/remote.rs; server durable remote restore and proxy routing | Reconnect describes same target, generation-fenced control, bounded attempts; SSH process implementation delegated |
| terminal/resume_cwd.rs; worktree manager/registry callers | HOME/USERPROFILE fallback exists; transcript header bounds and identity checked; canonical containment refutes old blanket claim |
| ipc/agents.rs; ipc/terminal.rs process_cwd and terminal commands | DS-03/04/06; live CWD syscall unavailable on Windows; cmd_terminal_get_cwd now uses daemon metadata |
| session/mod.rs; ui/state/workspaceRestore.ts; ui/lib/sessionPersistence.ts | Portable save/load; local missing/epoch-mismatched sessions become exited; SSH identity retained across epoch |
| util/mod.rs; ipc/cli_install.rs | CREATE_NO_WINDOW helpers exist; Unix-only launcher installation explicitly reports unsupported on Windows |
| terminal/preferences.rs | Windows ghostty.exe lookup exists; HOME-only optional import fallback noted, not a shell-start defect |
| agent_detect modules/manifests | Screen/title matcher path, not process enumeration; no platform process API; does not repair provider discovery |
| terminal/tests.rs, PTY/client test gates and ipc/tests.rs CWD case | DS-09; Unix-only fixtures and nondeterministic waits are existing verification debt |

SSH-specific process modules, browser CLI authentication, filesystem deletion, native rendering/input/wheel,
and packaging/PR disposition belong to sibling lanes. No inference that this audit explains the non-working wheel.

## Confirmed findings, severity ordered

### DS-01 - P1: Windows loopback daemon exposes full control without authenticating a peer
- Evidence: src-tauri/src/daemon/server.rs:1466-1478 binds TCP and writes a bare port; :1508-1513
  accepts any peer; :1526-1569 parses and dispatches before any handshake state/auth check.
  src-tauri/src/daemon/protocol.rs:124-128 handshake contains only version.
- Reachability: run_daemon_headless -> run_server_with_handover_and_readiness -> accept -> handle_client;
  RegisterWorkspace/Spawn/Write and Shutdown are reachable request arms, not merely informational IPC.
- Local users able to reach the loopback port need not read daemon.port: the port is enumerable.
  Same-user access alone is not the Unix differential; lack of an owner boundary for other local users is.
- Failing-first: isolated Windows daemon, subscribe to readiness; second-account client sends ListSessions
  and a fixture Write without authentication; assert rejection and unchanged fixture output. Current dispatch accepts.
- Binary observable: unauthenticated ListSessionsOk or fixture sentinel proves unauthorized control.
- Smallest scope: server/client/protocol plus every attach/proxy connection path; per-boot capability protected
  by user ACL, authenticate before dispatch (including streams), or owner-restricted named pipes. Pair with DS-07.

### DS-02 - P1: stale-daemon auto-upgrade creates recursively auto-upgrading temporary clients
- Evidence: src-tauri/src/daemon/client.rs:352-359 initializes upgrade_requested=false for every new client;
  :438-457 creates a new temporary client and sends UpgradeBinary; :620-635 its handshake calls
  maybe_trigger_upgrade_if_stale again. :466-470 only logs UpgradeUnsupported on that particular request.
- Reachability: desktop first request -> connect_and_handshake -> stale version -> spawned temp_client
  -> send_request -> connect_and_handshake -> another temp_client, before handling the upgrade reply.
  Windows server src-tauri/src/daemon/server.rs:2291-2296 always returns UpgradeUnsupported, so it never
  changes the stale version that stops this recursion. Each client has an independent suppression flag.
- Failing-first: fake TCP daemon with an explicitly different daemonVersion; signal receipt of first
  UpgradeBinary, reply UpgradeUnsupported, and capture connection/request admission deterministically.
  Assert an internally spawned upgrade client's handshake cannot schedule another upgrade (inject task scheduling).
- Binary observable: isolated old-version daemon/new GUI pair produces repeated handshakes/UpgradeBinary
  requests despite "Suppressing further upgrade requests"; monitor request events, not a fixed sleep.
- Smallest scope: client.rs; use an upgrade RPC path with automatic upgrade disabled, or share the
  upgrade guard with the temporary client. Test Windows Unsupported and Unix Deferred/failure responses too.

### DS-03 - P1: direct agent resume cannot execute Windows command-script shims
- Evidence: src-tauri/src/terminal/shell.rs:410-434 constructs CommandBuilder directly from the resume
  program; daemon/server.rs:2580-2605 resolves that command then spawns it via TerminalService/PtyManager.
- Dependency evidence: portable-pty-0.9.0/src/cmdbuilder.rs:579-603 DOES search PATHEXT;
  src/win/psuedocon.rs:132-153 passes the resolved module name directly to CreateProcessW.
  No cmd.exe /c or PowerShell -File interpreter is selected for .cmd/.bat/.ps1.
- Reachability: AgentResume startup -> resolve_agent_resume_plan -> resolve_startup_command ->
  spawn_in_worktree -> slave.spawn_command -> CreateProcessW(script path). Native .exe agents are not this defect.
- Failing-first: Windows fixture PATH with only claude.cmd emitting argv as JSON; real resume Spawn must
  emit supplied session ID. Repeat path containing spaces and arguments containing shell metacharacters.
- Binary observable: shim-backed resume currently returns Spawn error (expected Windows invalid executable),
  rather than the fixture output sentinel. Exact Win32 error remains runtime-unverified.
- Smallest scope: shell.rs Windows interpreter-aware command planning, coordinated with DS-04;
  quote for the selected interpreter, not just CRT argv. Do not claim adding PATHEXT alone fixes execution.

### DS-04 - P2: availability detection misses .exe/.cmd agents requested without an extension
- Evidence: src-tauri/src/ipc/agents.rs:570-578 only joins the exact name; :592-595 only tests is_file.
  Caller :510-525 detect_agents -> resolve_binary, reached by cmd_agents_detect at :17-19.
- The PATH directory may contain claude.exe or claude.cmd while detection reports unavailable for claude.
  This is separate from portable-pty's existing PATHEXT implementation and DS-03 execution failure.
- Failing-first: temp directory containing only fixture.exe, resolve_binary("fixture", &[dir]) must return
  that file on Windows; matrix .COM/.EXE/.BAT/.CMD, case-insensitive extension and explicit-extension input.
- Binary observable: cmd_agents_detect returns available=false despite the fixture being on PATH.
- Smallest scope: ipc/agents.rs resolution helper and deterministic Windows tests; respect PATHEXT order,
  keep rejecting path-shaped detection names, and do not mark arbitrary files runnable without a launch policy.

### DS-05 - P2: local Windows agent-state endpoint is advertised but never hosted
- Evidence: src-tauri/src/terminal/pty.rs:134-138 exports agent-state.sock from daemon/server.rs:830-835;
  listener definition :1344-1382 and startup call :1497-1499 are Unix-gated.
  resources/agent-extensions/ferryx-agent-state.ts:11-15,44-64 supports TCP variables, but local PTY/daemon
  code never supplies FERRYX_AGENT_STATE_PORT/TOKEN. Existing SSH TCP bridge support is not a local listener.
- Reachability: daemon startup -> local PTY env -> installed extension send -> nonexistent socket path;
  AgentStateHub never receives those local authoritative state/provider-session reports.
- Failing-first: Windows isolated daemon, attach state stream before fixture agent lifecycle event;
  extension must deliver exact working/blocked/idle and provider ID through advertised endpoint, bounded await.
- Binary observable: PTY outputs normally but attached stream receives no matching AgentState frame.
- Smallest scope: server.rs listener + PTY endpoint env; reuse bundled TCP protocol with authentication,
  or named pipe; ensure readiness only advertises a hosted endpoint. Fix DS-06 installation independently.

### DS-06 - P2: provider discovery and automatic extension installation retain Unix-only dependencies
- Evidence: src-tauri/src/ipc/agents.rs:71-80 /bin/ps, :172-180 /bin/ps environment, :192-204
  /usr/sbin/lsof; :313-353 CWD-keyed fallbacks call ipc/terminal.rs:993-997, which returns None on Windows.
  daemon/agent_extension.rs:14-25 uses HOME only; :44-55 silently iterates zero directories without HOME.
- Reachability: cmd_agent_session_discover -> daemon/server.rs:1685-1707 -> discover_agent_session_id;
  daemon startup :1499 -> install_agent_state_extension -> extension_dirs. Native Windows lacks these Unix tools.
- Antigravity/OpenCode/Pi HOME-only caches remain additional blockers after process CWD is repaired;
  explicit persisted provider IDs/resume transcript paths do not require discovery and are not all disabled.
- Failing-first: native Windows fixture provider process with known session transcript; daemon discovery must
  return exact ID. Separate env-injected HOME-unset/USERPROFILE-set install test must produce three paths.
- Binary observable: DiscoverAgentSessionOk providerSessionId=null and absent extension in existing agent
  extension directories despite valid USERPROFILE. Neither output implies a dead PTY.
- Smallest scope: ipc/agents.rs Windows discovery adapter and profile resolution, agent_extension.rs home
  fallback; prefer authoritative provider reporting over invasive process-memory CWD scraping where possible.

### DS-07 - P2: Windows runtime trust validator never checks owner SID or DACL
- Evidence: src-tauri/src/daemon/server.rs:403-430 ignores expected UID, checks only symlink/type;
  :488-498 delegates with 0; ensure_runtime_directory :500-532 secures permissions only on Unix.
  Caller daemon/client.rs:377-385 trusts this result for the production endpoint before connecting.
- Confirmed missing validation; exploit precondition is a permissive/foreign-owned runtime directory or
  port file. Default LOCALAPPDATA ACL weakness is NOT asserted. Manifest default inherited ACL is the same risk.
- Failing-first: Windows fixture directory with foreign owner or an untrusted writable ACE; validator must
  reject it before reading a planted port file. Also test normal per-user ACL is accepted.
- Binary observable: client connects to planted loopback responder under permissive fixture runtime ACL.
- Smallest scope: server runtime creation/validation, SID/DACL APIs and tests; secure ancestor/leaf trust.
  Merely checking owner is insufficient if DACL grants another user write access.

### DS-08 - P2: Windows upgrades remain explicitly unsupported even with no sessions
- Evidence: daemon/server.rs:2099-2122 routes upgrade/prepare; :2291-2296 returns UpgradeUnsupported;
  daemon/handover.rs:242-248 rejects preparation. client.rs:466-470 only logs unsupported.
- Reachability: new GUI -> stale daemon handshake -> UpgradeBinary. No Windows re-exec/empty-session
  replacement path exists; leaving the GUI does not itself stop the persistent daemon.
- Failing-first: isolated Windows old/new fixture binaries, zero active sessions, request UpgradeBinary;
  await successor readiness and assert new handshake identity. Active-session variant must retain session IDs.
- Binary observable: UpgradeUnsupported and unchanged daemon binary identity. This is a feature gap,
  not evidence that the current client kills sessions. Protocol mismatch returns typed error rather than restart.
- Smallest scope: staged Windows lifecycle policy: safe zero-session replacement first; active-session
  handoff requires a TCP/named-pipe legacy route and lock ownership transfer, not blindly kill-and-spawn.

### DS-09 - P2 (test-only): existing terminal tests are not a reliable Windows safety net
- Evidence: terminal/mod.rs:49-50 includes tests on Windows; terminal/tests.rs:16-21 spawns /bin/sh;
  ipc/tests.rs:611-625 unconditionally requires process_cwd to return Some despite Windows None stub.
  terminal/tests.rs:7-14 polls; terminal/pty.rs Unix close tests use fixed 100ms readiness sleeps.
- Reachability: cargo test --lib on native Windows; these are fixture/contract defects, not a terminal UI symptom.
- Failing-first proposal: run the named test_spawn_write_echo_and_read and terminal_process_cwd_resolves_accurately
  individually in a Windows isolated checkout; preserve failure output, then replace fixtures with Windows peers
  and subscribe-before-trigger readiness/exit events. No fixed sleeps or skip-to-green.
- Binary observable for replacement tests: real ConPTY command emits sentinel and process handle signals exit.
- Smallest scope: terminal/tests.rs, PTY close tests, ipc/tests.rs plus targeted Windows integration tests.

## Prior findings: retained, narrowed, refuted

- Prior daemon auth/ACL findings retained as DS-01/07; port secrecy and remote-gateway firewall rules do not fix IPC auth.
- Prior provider ps/lsof/CWD/HOME and extension findings consolidated DS-05/06; no duplicate severity inflation.
- Prior PATHEXT finding split DS-03/04: "portable-pty has no PATHEXT lookup" is REFUTED by dependency source.
- Prior handover and missing POSIX exec counterpart consolidated DS-08; no assertion that an unsupported reply drops PTYs.
- Prior unconditional orphan-grandchild claim NARROWED: session.rs kills only direct child, but close_io drops
  ConPTY master, and portable-pty src/win/psuedocon.rs:73-76 calls ClosePseudoConsole. Attached console children
  may consequently terminate. Detached/new-console descendants lack Job Object ownership; probe required below.
- Prior verbatim-prefix containment defect REFUTED for reviewed spawn path: manager.try_new canonicalizes root,
  canonical_allowed_path canonicalizes candidate, registry returns canonical paths; daemon normalizes only at cmd.cwd.
  Filesystem lane owns other remove/new-path boundaries, not covered by this refutation.
- Prior process_cwd claim about cmd_terminal_get_cwd is outdated: ipc/terminal.rs:859-873 uses describe metadata.
- Prior missing persistent-lock environment test is test debt, not a Windows locking defect: APPDATA/USERPROFILE
  fallback and LockFileEx/UnlockFileEx exist; missing all homes degrades to runtime lock just as Unix does.
- Prior IPv6-only enterprise/firewall scenario is an environment compatibility unknown, not proven ordinary failure.
- Prior launchd legacy label/log path and login-shell -l/Linux bash/PATH concerns are not Windows reachable defects;
  no production install_launchd_agent caller; no claimed Windows login autostart feature to regress.
- LANG=C.UTF-8 injection is inert for native Windows shells, not independently a demonstrated bug.
- Prior /tmp session test uses serialized fixture paths, not actual required /tmp filesystem access; do not infer failure.
- FINAL-AUDIT shell-choice and installed-compositor conclusions retained as historical evidence only; current
  shell plans implement pwsh/powershell/cmd/wsl. No current desktop runtime or wheel repair claimed here.

## Remaining unknowns / required later probes

- PTY tree lifetime: fixture parent launches detached child and signals readiness over owned pipe; retain both
  process handles, close pane, await parent/child termination with bounded OS waits. If detached child survives,
  fix ownership using a per-session Job Object (assignment must be race-free), not taskkill by guessed PID.
- portable-pty WinChild::kill discards do_kill errors; do_kill's success branch is inverted (win/mod.rs:41-56).
  Caller session.rs:199-207 therefore cannot surface termination failures; current close later polls/reaps.
  Quantify actual reachable failure and affected dependency version before selecting adapter/upstream fix.
- client.attach handshake/Attach reads (:1051,:1124) lack the control path's timeout. Fault-injected listener
  should withhold an exact response and verify bounded attach failure; cross-platform issue, not Windows-exclusive.
- Windows ConPTY ClosePseudoConsole blocking/output drain, Ctrl+C behavior, detached daemon survival after GUI
  exit, WSL distro/CWD behavior, console code pages, AV file locks, custom shell paths and Unicode require real OS QA.
- Session saves share a fixed .json.tmp path; concurrent save collision deserves deterministic barrier testing,
  not a claim that Rust rename cannot replace Windows files. Optional Ghostty HOME-only import needs product policy.
- No PR inventory was assigned to this child; parent owns all-open-PR reconciliation. No runtime tests executed.

## DS-10 - P1 QA safety: persistence harness still not isolated by default
- tests/daemon_persistence_contract.rs:1 is Unix-only (zero Windows coverage); :34-43 uses shared
  get_socket_path/get_lock_path and inherits environment; :93-115 sends Shutdown then unlinks shared paths.
  Caller :531-534 starts this harness; :452-476 independently starts a daemon on the same endpoint.
- Exact readiness now prevents casually attaching after a lock-rejected child, but handshake PID (:73-76)
  is not compared to child.id(); shutdown resolves the endpoint again. No per-harness temp runtime exists.
  Debug/release separation (server.rs:124-137) is NOT isolation from a live same-profile dev daemon or tests.
- Do not run cargo test --all-targets or this unwrapped target. Serial execution alone is insufficient.
  Supported external isolation: server.rs:44,128,271 honors SESSION/RUNTIME/DATA overrides; remote/state.rs:672
  also puts gateway state under DATA. Empty HOME prevents agent-extension writes to the user's profile.
- Later Unix-only invocation below requires QA_TEST_BIN to name a provenance-verified prebuilt integration
  executable from an isolated checkout, whose embedded CARGO_BIN_EXE_ferryx is likewise QA-owned. Not executed.
  It runs no cargo/build/devrunner; preserve its printed temp root for owned-process cleanup if a test panics.
```sh
q=$(mktemp -d /tmp/fxqa.XXXXXX) && mkdir -p "$q/home" "$q/runtime" "$q/data" "$q/session" && printf '%s\n' "$q" && env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin HOME="$q/home" SHELL=/bin/bash TMPDIR="$q" FERRYX_RUNTIME_DIR="$q/runtime" FERRYX_DATA_DIR="$q/data" FERRYX_SESSION_DIR="$q/session" "${QA_TEST_BIN:?Set verified isolated integration executable}" --test-threads=1
```
- Smallest fix: per-test TempDir/env on child, injected endpoint on every client, PID ownership assertion,
  and cleanup only owned artifacts. Existing polling at :750-755 remains test debt; no pass claimed.

## Verification receipt
Re-opened cited defect spans and callers, including cached portable-pty dependency implementations.
Bun win32 path lookup cross-check distinguished extensionless names from .exe/.cmd paths; not a Windows spawn test.
git diff --stat remained the foreign baseline: 23 tracked files, 178 insertions, 3 deletions before report creation.
Report is the sole authorized addition and remains uncommitted. No production/test/config/ref writes performed.
