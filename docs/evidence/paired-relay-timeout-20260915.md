# Paired relay terminal spawn timeouts — diagnosis & fix (2026-09-15)

## Symptoms
- Opening terminals on the paired daas host intermittently failed with TIMEOUT.
- Repeated retries created orphaned remote PTY sessions (18 idle shells on the machine).

## Root causes (layered)
1. Budgets too tight for the relay path: attach GET session-detail 10s + WS connect 10s.
2. Failed spawns leaked the already-created remote session (no cleanup) -> pileup worsened latency.
3. Disk full (2.4GB free) broke the tauri dev link; stale handover_routes.json blocked fresh dev daemon startup.

## Fixes
- src-tauri/src/paired_host/client.rs: GET budget 10s -> 30s, WS connect 10s -> 30s.
- src-tauri/src/ipc/terminal.rs: cmd_terminal_spawn closes the remote session best-effort when reattach fails after CreateSession (no more orphans).
- Cleanup: 17 orphan remote sessions closed via relay closeSession (omo agent session 1c487ed2 preserved); removed target/debug/incremental (18GB freed); removed stale handover_routes.json + legacy socket.

## Verification (live, through relay.checka.cc -> daas host)
- createSession: 1799ms OK
- pairedTerminalReattach (ticket + WS): 1841ms OK
- write "echo PAIRED_OK": writeOk; remote shell exited cleanly (no orphan)
- cargo test --lib paired_host: 42/42 pass; cargo check clean

## Notes
- paired_host sessions list served from local machine-catalog cache can lag (stale rows) after closes; session detail is authoritative.
