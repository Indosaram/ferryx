# Live lifecycle coordination

Metadata protocol/client/server blocks stable. st_01a09922 owns controller/input blocks; no edits there. A12 follow-through owns machine_events publication/snapshot/forwarder lifecycle, session_metadata_forward.rs, metadata tests and one cfg(test) eventInventoryObserved probe in workspace_api.rs.

Full integration aggregate passed: event9.75s metadata9.50s lifecycle26.39s. Explicit forwarders=0 and owner_streams=0 plus retry exhaustion partial snapshot verified. Deterministic race unit compile currently blocked by A14 declaring missing paired_host/client.rs and projects.rs (deterministic-race.log), not a test failure; waiting for source composition rather than rebuilding partial source.

Reset root traced with tracing enabled: machine_events snapshot fails STALE_REVISION during concurrent worktree commit, then returns dropping watcher/socket. Evidence A12-session-metadata-reset-trace.log. Focused fix preserves watcher and emits partial stale boundary then fresh snapshot; unrecoverable snapshot sends explicit WS Close. reset-GREEN passed real Git/worktree event integration. No relay tunnel code changed.

Owner IPC disconnect test added using abort+join of actual predecessor client tasks. First attempt blocked by sibling signature composition: validate_machine_target changed async while remote/server.rs lines1257/1260/1346/1384 still called synchronously (ipc-recovery-RED.log). Not behavioral RED. Please notify when A11 signature composition ready; child will not patch sibling validation callsites.
