# Same-uid threat model

Ferryx runs every terminal pane as the owner's own OS user. A coding agent in one pane can
therefore do anything the owner's account can do with files. The daemon cannot change that,
but it decides which **daemon requests** a pane process may send. This document says what is
enforced, where, and what is not.

## Who is who

The daemon reads the connecting process from the kernel when it accepts a connection
(`src-tauri/src/daemon/supervisor_peer.rs`) and classifies it:

| Class | Meaning |
| --- | --- |
| Owner | A process outside every Ferryx pane: the desktop app, an ssh shell, a terminal outside Ferryx, the daemon's in-process hub, `ferryx-cli mcp` started outside a pane. |
| Pane `{src}` | A process descended from the pane `src` (its pid, session, controlling terminal or parent chain matches a pane child). |
| Unknown | No peer pid (in-process pairs). |
| TokenAuthenticated | Windows loopback TCP: the peer showed the transport token, but its process is unknown. |

The owner's own shell inside a Ferryx terminal is a pane too. Owner actions are made from
**Settings > Pane access** in the desktop app, or from a terminal outside Ferryx.

## What is enforced (macOS, Linux, Windows named pipes)

`src-tauri/src/daemon/pane_grants.rs` applies one policy to every mutating request, before the
request's handler runs:

- Owner: allowed.
- Pane `src` acting on its own session: allowed. Spawning and creating or deleting worktrees
  in its own workspace: allowed. Reads (list, describe, attach, agents read/events/wait,
  supervisor snapshot) are never gated.
- Pane `src` acting on another session or workspace needs a grant
  `src_session | src_workspace -> dst_session | dst_workspace | *` with the right op
  (`write`, `control`, `spawn`). Without one the reply is `FORBIDDEN` with reason
  `PANE_GRANT_REQUIRED`.
- Owner-wide actions from a pane (upgrade, handover, shutdown, pairing, remote and paired-host
  configuration, desktop sidebar and session save, agent integration, clipboard upload) need
  the `admin` op on target `*`. Without it the reply is `FORBIDDEN` with reason
  `OWNER_ACTION_REQUIRED` (`run from Settings > Pane access or outside a pane`). `ferryx-cli up`,
  `pair` and `remote` check this before they start.
- Only the owner may add or remove grants (`PaneGrant`, capability `paneGrantsV1`). A pane
  that runs `ferryx-cli grants add` gets `FORBIDDEN` with the hint `open Settings > Pane access`.
- Supervisor fence fields never grant pane access. The gate runs before the fence is admitted,
  so a refused request never raises `supervisor-fence.json`.
- The control channel (`control.sock`, control-v1) identifies its peer the same way and
  answers `FORBIDDEN` (an additive control-v1 code) to `Shutdown`, `RetireWhenIdle` and
  `RequestUpgrade` from a pane without `admin`.

Grants live in `<data>/remote/pane-grants.json` (0600, written atomically through a temporary
file and a rename). They travel in the handover decision record, so a successor daemon serves
the same grants. An unreadable file fails closed: no grants apply.

Supervisor mutations keep their own, stricter gate (`supervisor_peer::refusal`): a pane may
never mutate the supervisor, with or without grants.

## Windows transport migration

Windows serves three owner-only named pipes alongside the existing loopback TCP listeners:
`\\.\pipe\ferryx-<user sid>-daemon`, `-agent-state`, and `-control` (each with `-dev`
in debug builds). A protected DACL `D:P(A;;GA;;;<owner sid>)` permits only the owner's
token; remote clients are rejected. The first instance reserves the name exclusively and a
replacement is created before releasing each connected instance. Failure to reserve a name
fails startup rather than silently downgrading security.

Data and control pipe peers are identified with `GetNamedPipeClientProcessId`, with process
creation time binding their pid. Toolhelp parent snapshots support pane-descendant classification.
They use the same grants and control-v1 refusal rules as Unix peers. Missing process identity
closes the pipe connection. Clients prefer pipes and fall back to TCP before sending; a failed
write is not replayed. Panes receive `FERRYX_AGENT_STATE_PIPE` alongside the compatibility port
and token. Agent-state pipes have the same owner boundary as Unix agent-state sockets, not a
new per-pane report authorization scheme.

There is no Windows live handover. Dual-listen starts on the next idle restart under
`FERRYX_DAEMON_IDLE_UPGRADE`; updating the installed binary never replaces a busy daemon.
No new persisted transport state is introduced. Existing grants are restored from their store.

## What is not enforced

- **Windows legacy TCP (one compatibility release).** Loopback TCP carries no peer process, and the transport token is
  a file every pane process can read. A `TokenAuthenticated` connection therefore keeps the
  behaviour from before grants existed for every newly gated request, and only grant changes are
  refused. Supervisor mutations stay refused for token peers, as before. This exemption does
  not apply to pipe peers. TCP is deliberately not removed in this migration, so an explicitly
  selected legacy TCP connection is still a same-uid bypass for the newly gated requests.
- **File access.** A pane process can read and write every file the owner can, including
  `pane-grants.json`, the daemon's data directory and the socket's runtime directory. Grants
  protect the daemon's request surface, not the filesystem. A pane that edits
  `pane-grants.json` affects only the next daemon start or reload; the running daemon keeps
  serving the grants it holds in memory.
- **Deliberate escape.** A pane process that calls `setsid`, drops the pane's terminal and is
  reparented away from the pane is no longer recognised as a pane (see the module docs of
  `supervisor_peer.rs`). This is the limit of what a same-uid socket can tell.
- **Other uids.** Root and other users are outside this model.

## Least privilege

A terminal must spawn arbitrary shells, so Ferryx does not use the macOS App Sandbox.

- **macOS.** The repo ships no custom entitlements file (`src-tauri/tauri.conf.json` names no
  entitlements). No todo in the rex adoption plan may add entitlements. TCC prompts (Files and
  Folders, Full Disk Access, Automation, camera or microphone) that a command in a pane triggers
  are attributed to the Ferryx app bundle, so granting one grants it to every pane.
- **Windows.** The MSIX package declares only `runFullTrust` (the capability block of
  `AppxManifest.xml`); nothing else may be added besides the `ferryx` protocol extension
  (todo 20).
- **Linux.** No setuid or setcap binaries; the daemon and `ferryx-cli` run as the owner, and the
  systemd unit is a user unit.
