# SSH host multiplexing: one connection per host

Status: design, not implemented. 2026-11-23.

## Problem

Every remote SSH pane opens its own `SshBridgeClient`, and each client spawns two
`ssh ... ferryx-remote-helper bridge --stdio` processes: `control` for RPCs and `reader` for the
`pty.read` long poll (`src-tauri/src/ssh/bridge.rs`, `SshBridgeClient::connect_with_target`).
Every spawn also runs one-shot SSH probes first (environment detection, `helper_setup::ensure_started`).
The DAG watcher opens another client per project (`src-tauri/src/ipc/dag.rs`).

Measured on 2026-11-23 (MacBook Air -> MacBook Pro):

- About 22 panes produced 44 `ssh` processes on the client and 44 `sshd-session` processes on the
  host.
- macOS ships sshd with `inetdCompatibility.Instances = 42`. Past that, new panes failed with
  `Could not create new instance of inetd service: 67: Too many processes`.
- OpenSSH `PerSourcePenalties` (default on) read the connection bursts as abuse and blocked the
  client for about 15 s at a time: `Connection closed by <host> port 22` during
  `environment_detection_probe`.
- Each new pane paid a full SSH handshake (about 0.3 s on Tailscale) plus the probes.

Orca, T3 Code and OmO Desktop all keep one SSH connection per host and carry every pane inside it
(source references in the memory note `ssh-multiplexing-orca-t3code-omo-vs-ferryx-2026-11-23`).

## What stays the same

- The remote helper daemon. It already runs once per host and root, owns every PTY, and keeps a
  ring buffer per session. PTYs are not connection-scoped (`ferryx_scope/ssh/process.rs` `serve`),
  so losing a connection never kills a PTY.
- The helper RPC protocol (`handshake`, `pty.*`, `project.*`, `worktree.create`, `dag.*`) and its
  frame format.
- `RemoteRuntime`, `Session`, the `Transport` trait, the reconnect state machine, cursors and
  replay. Only the object behind `Transport` changes.

## Design

### Remote side: `bridge --mux`

A new helper subcommand, `ferryx-remote-helper bridge --mux --root <root>`, speaks a channel
protocol on stdio:

```
frame = u32 BE length | u32 BE channel id | u8 kind | payload
kind  = OPEN | DATA | CLOSE | PING | PONG
```

- `OPEN(ch)` makes the bridge open a new local connection to the helper daemon's `helper.sock`
  (or loopback TCP on Windows), exactly as `bridge --stdio` does today, including the token.
- `DATA(ch)` carries one existing helper request or response frame for that channel.
- `CLOSE(ch)` closes that local connection. The daemon releases that connection's DAG
  subscriptions on EOF, as today.
- `PING` / `PONG` replace the per-connection 60 s keepalive and the 180 s
  `BRIDGE_IDLE_TIMEOUT` reaping: the mux exits when no frame arrives for the idle bound, and the
  client sends `PING` every 15 s.

Each channel is its own helper connection, so a long `pty.read` on one channel never blocks
another channel. The helper daemon needs no change for concurrency.

Fairness: a single helper response can be up to `MAX_FRAME` (1 MiB). The mux splits payloads into
segments of at most 64 KiB and interleaves channels round-robin, so one pane dumping output
cannot stall input on another pane for more than one segment.

### Client side: `HostMux`

A process-wide registry in the daemon, keyed like `master::ConnectionKey`
(host id, hostname, user, port, identity), holds at most one `HostMux` per host:

- It owns one supervised `ssh` child running `bridge --mux`, using the existing supervision
  (`transport_unix` / `transport_windows`), so the transport still dies with the daemon.
- `open_channel()` returns a `MuxChannel` that implements the same `request()` contract as
  `BridgeConnection` today (one request in flight, poison on I/O error).
- `SshBridgeClient` keeps its shape: `control` and `reader` become two `MuxChannel`s on the shared
  `HostMux`. Callers (`terminal/remote.rs`, `ipc/dag.rs`) do not change.
- The remote environment and `ensure_started` result are cached on the `HostMux` while it is
  connected, so a new pane on a connected host runs no extra SSH probes.
- The `HostMux` is closed after the last channel closes plus a 5 minute grace period, so tab
  churn does not reconnect.

### Failure and reconnect

- When the mux transport dies, every channel on it fails with `ConnectionClosed`. Each `Session`
  already treats that as a transport loss and enters its existing reconnect loop
  (`reconnect_backoff`, 250 ms doubling, cap 32 s).
- Reconnects for one host coalesce: the first caller re-dials, and the others wait on the same
  connect future instead of each spawning `ssh`. One reconnect therefore costs one SSH handshake
  no matter how many panes the host has.
- PTYs survive on the remote side, and each pane resumes from its own cursor, so output is replayed
  from the helper ring buffer as today.

### Compatibility

- Older helpers lack `bridge --mux`. The client starts the mux, and if the first frame is not a
  valid mux hello (for example a usage error or exit 2), it marks the host `legacy` and falls back
  to the current per-connection `bridge --stdio` path. Nothing breaks during a mixed rollout.
- A new helper still supports `bridge --stdio`, so an old client works against it.
- The helper version tag bumps, so hosts get the new binary through the existing install path.

### Daemon handover

`SshBridgeTransferState` can pass the two `ssh` file descriptors per session to a successor, but
only tests call it: `RemoteRuntime::export_for_transfer` and `live_import` have no production
caller (checked 2026-11-23 with a workspace-wide search). A restart reconnects from the persisted
descriptor. The mux follows that reconnect path, so a successor daemon dials its own `HostMux`, and
the transfer-state types can stay for the legacy path.

## Expected effect

| | Today (22 panes) | With `HostMux` |
|---|---|---|
| `ssh` processes on the client | 44 + DAG watchers | 1 per host |
| `sshd` sessions on the host | 44+ | 1 per host |
| New pane on a connected host | handshake + probes (about 0.3 s or more) | one `OPEN` frame, no handshake |
| macOS 42-instance limit | hit at about 20 panes | not reachable from one client |
| `PerSourcePenalties` | triggered by bursts | one connection, nothing to penalise |
| One pane's large output | isolated | segmented, round-robin |
| Transport loss | panes fail one at a time | all panes on that host reconnect together, one handshake |

## Platforms

- Client: Unix uses `transport_unix` supervision, and Windows uses `transport_windows`. `ssh.exe`
  stdio works for both. No OpenSSH ControlMaster is required, which Windows OpenSSH does not
  support.
- Remote: the mux is in the Rust helper and runs on POSIX and Windows hosts. Windows hosts use
  loopback TCP to the daemon, as `bridge --stdio` does today.

## Work plan

1. Helper: `bridge --mux` with frame codec, per-channel helper connections, segmentation,
   round-robin writer, PING/PONG idle bound. Unit tests over in-memory streams: open/close,
   concurrent long poll on one channel while another channel writes, segmentation fairness, idle
   exit.
2. Client: `HostMux` registry, `MuxChannel` with the `BridgeConnection` request contract,
   environment/`ensure_started` cache, grace close, coalesced reconnect, legacy fallback. Tests
   with a fake mux peer: N channels share one transport, transport death fails all channels,
   reconnect dials once, legacy hello falls back.
3. Wire `SshBridgeClient::connect_with_target` and the DAG watcher to `HostMux`, keeping
   `BridgeConnection` for the legacy path.
4. Handover: keep descriptor reconnect as the only production path; add a test that a successor
   daemon reconnects every SSH pane of a host through one new `HostMux`.
5. Verification on real hosts: macOS host with default sshd, 40 panes from one client, assert one
   `ssh` client process and one `sshd-session`; kill the transport and assert all panes recover
   with output replay; repeat against a Linux host and a Windows host; mixed old/new helper.
6. Build and gate on remote hosts only (picker), never on the developer MacBook.

## Out of scope

- The relay path (paired hosts). It has its own E2E session model.
- Changing the helper RPC protocol itself.
