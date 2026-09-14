# Review: the headless UDS daemon (Rust)

Scope: `src-tauri/src/daemon/server.rs` (agent-state listener, main RPC client loop, socket
setup), `src-tauri/src/daemon/logging.rs` (bounds).
Reviewed-at: 2026-09-14

> Provenance note: the `rust-daemon` dag node completed and wrote its report here; the lead
> session then accidentally overwrote the file with its own parallel findings. The node's six
> findings are restored below from its recovered completion output (task `st_01a0a06d`,
> model `mahoquot/nekos-claude-opus-5`). Every line reference below is the node's; the lead
> independently re-read and confirmed the two `read_line` sites and the logging bounds. The
> remaining four were reported by the node and are recorded here as its claims, marked
> `[node-reported]` where the lead did not independently re-verify them.

## Findings

### [P1] Unbounded `read_line` on the agent-state socket

- Location: `src-tauri/src/daemon/server.rs:1365`
- Observed (lead-confirmed): `while reader.read_line(&mut line).await.unwrap_or(0) > 0` with
  `let mut line = String::new();` and no size limit. `read_line` appends until it sees a newline
  or EOF.
- Why it is wrong: any process able to open the socket can write bytes containing no newline and
  grow that `String` without bound until the daemon is OOM-killed. The daemon owns **every PTY
  master fd**, so losing it terminates all live terminal sessions and running agent workflows.
- Minimal fix: bound each line the way the browser CLI already does —
  `MAX_REQUEST_BYTES = 1024 * 1024` with a `TooLarge` arm and a `read_limited_line` helper
  (`src/ipc/browser_cli.rs:242-246`). The less critical surface was hardened; this one was not.

### [P1] Agent-state accept loop busy-spins on sticky accept errors [node-reported]

- Location: `src-tauri/src/daemon/server.rs:1358`
- Observed: the accept loop uses `let Ok((stream, _)) = listener.accept().await else { continue; }`.
- Why it is wrong: `continue` on a *persistent* accept error (fd exhaustion, socket teardown)
  retries immediately with no backoff and no error inspection, spinning the async worker at
  100% CPU instead of surfacing the failure.
- Minimal fix: match the error, log it, and either break on unrecoverable kinds or apply a short
  backoff before retrying.

### [P2] Agent-state socket unlinked and rebound with no ownership check or lock [node-reported]

- Location: `src-tauri/src/daemon/server.rs:1343`
- Why it is wrong: removing and rebinding the path without verifying ownership (or holding the
  daemon's `flock`) lets a second process or a stale instance steal the endpoint.

### [P2] Agent-state reports are unauthenticated — any connection can publish for any session id [node-reported]

- Location: `src-tauri/src/daemon/server.rs:1366`
- Why it is wrong: `parse_agent_state_report` feeds `states.publish_canonical(...)` directly, so
  any same-uid process can forge agent state for an arbitrary `session_id`, corrupting the
  activity/attention UI.

### [P2] Main RPC client loop reads unbounded lines

- Location: `src-tauri/src/daemon/server.rs:1587`
- Observed (lead-confirmed): `handle_client` calls `reader.read_line(&mut line)` inside its
  `select!` loop with no cap — the same defect as the P1 above, on the primary RPC surface.
- Minimal fix: same bounded reader.

### [P3] Non-fatal `set_permissions` failure on the agent-state socket [node-reported]

- Location: `src-tauri/src/daemon/server.rs:1351`
- Why it is wrong: if `set_permissions` fails, the socket can remain world-accessible while the
  daemon proceeds as though it were restricted.

### Verified negative — the logging sink is bounded (lead)

- Location: `src-tauri/src/daemon/logging.rs:6-7`, `:19`, `:139`
- Observed: `MAX_BYTES = 1024 * 1024`, `MAX_RECORD = 8192`; oversize records are dropped rather
  than queued (`:19`), and the file is truncated with `set_len(0)` once appending would exceed
  `MAX_BYTES` (`:139`). Log growth cannot fill the disk.

### Verified negative — the stream writer reuses one buffer (lead)

- Location: `src-tauri/src/daemon/server.rs:3060`
- Observed: `Vec::with_capacity(8 * 1024)` allocated once outside the stream loop and `clear()`ed
  per frame, not reallocated per frame.

## Repair status

All findings above are **DEFERRED, not fixed** in this pass. Every one of them edits the request
or accept path of the process that currently owns the user's live PTY sessions; the project
forbids disrupting that daemon, and these changes deserve their own verified pass rather than a
tail-end edit during a review run. The falsification for the unbounded `read_line` (write ~100 MB
with no `\n` and watch daemon RSS) was deliberately **not executed** for the same reason.

## Summary

- P0: 0
- P1: 2
- P2: 3
- P3: 1
