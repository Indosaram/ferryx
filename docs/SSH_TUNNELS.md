# SSH tunnels

Ferryx can open a **local port forward** for a saved SSH machine: a loopback port on this
computer is forwarded to a TCP service that only the remote machine can reach. The daemon
owns the forward, so it survives a closed window, and a crash is reclaimed by the next
daemon rather than leaving a stray `ssh` holding the port.

## What a tunnel is here

A tunnel is one saved definition plus, while it runs, one supervised `ssh` child:

- **local side** — `127.0.0.1:<localPort>` on this machine. Always loopback: the child is
  spawned with `-L127.0.0.1:<localPort>:...` and nothing else, so a forwarded service is
  never exposed to the network by Ferryx.
- **remote side** — `<remoteHost>:<remotePort>`, resolved *on the remote machine*. A
  database listening on `127.0.0.1:5432` there is reached at `127.0.0.1:<localPort>` here.
- **transport** — `ssh -N -T` with no TTY, no `RemoteCommand`, and a zero-byte `-F` config
  file (`ssh-tunnel-empty.conf`) so the operator's own `~/.ssh/config` cannot silently add
  a forward, a proxy, or a different identity. `ExitOnForwardFailure=yes`,
  `StrictHostKeyChecking=yes`, `ControlMaster=no`, `ServerAliveInterval=15` and
  `ServerAliveCountMax=2` are forced.
- **credentials** — a tunnel has no credential of its own. It uses the host record's
  existing authentication: agent, key file, jump host, or the password already entered for
  that host in machine settings. Nothing new is stored, and no secret enters the argv.

The daemon supervises the child, waits for the port to actually accept a connection
before reporting it as up, keeps exactly one diagnostic channel open, and reconnects with
backoff (`1s`, doubling to `4s`) when the transport dies.

## Where the UI lives

**Settings → Remote → (a machine) → Tunnels.** The section is inside the SSH machine
drawer (`ui/src/components/settings/remote/SshTunnelsEditor.tsx`), and it mounts only when
the connected daemon advertises the `sshTunnelV1` capability. Against an older daemon it
shows "SSH tunnels need a newer background service." and issues no request at all.

Each saved row shows the label, the local port, the remote target, whether it is enabled, the
state, and — when there is one — the failure code. Buttons are **Add tunnel**, **Start**,
**Stop**, **Edit**, and **Remove**. `enabled` is the editor's own arm switch: **Start** is
disabled while a row is off, so a definition can be kept without being one click from
running. The daemon stores the flag and never consults it, so a start by id succeeds
whatever it says.

## Saved definitions versus an ad-hoc start

Both reach the same runtime; they differ in what is written down.

| | Saved definition | Ad-hoc start |
| :--- | :--- | :--- |
| Written to `ssh_tunnels.json` | yes, per host | no, never |
| Identity | the `id` you saved | a uuid the daemon mints at accept time |
| Local port | **must be explicit** | may be omitted |
| Shown after a restart | yes, as `stopped` | no |
| Start request | `specId` + `hostRevision` | `spec` inline |

The **saved definition needs an explicit port** because the store validates with
`validate_saved_spec`, which rejects `local_port: None` with `InvalidArgument` and
`details.field = "localPort"`. That is deliberate: a restart must be able to report the
port the operator can bookmark, and an auto-allocated port that changes on every boot
would be a different endpoint each time. `0` is rejected too, for every definition.

The **ad-hoc start may leave the port unset**. The daemon then allocates one
(`TcpListener` bind on port 0, read the port, close) and reports it in the returned view's
`localPort`; the number you connect to is the one in the response, not one the client
predicted. Allocation is a hint, not a reservation: the probe closes before `ssh` receives
the port, so a lost race surfaces as `port_in_use`, never as a silent wrong listener.

Duplicate local ports are rejected **per host**, disabled definitions included, so two
tunnels of one machine can never fight for the same loopback port. An unchanged start of a
saved definition is idempotent and returns the existing view instead of a second child.

## States

`view.state` is one of four wire strings.

| state | meaning | what to do |
| :--- | :--- | :--- |
| `stopped` | A saved definition with no runtime right now. It survives restarts and is listed by `sshTunnelList` with `runtimeId: null`. | Press **Start**. |
| `listening` | The child is up and the local port accepted the daemon's own readiness probe. Traffic is being forwarded. | Nothing. Connect to `127.0.0.1:<localPort>`. |
| `reconnecting` | The transport is down and the daemon is retrying (1s, 2s, 4s, then a steady 4s). A first attempt that has not established within ten seconds also answers `reconnecting` with `host_unreachable` instead of blocking the request. | Wait; it repairs itself when the host is reachable again. If it never leaves this state, the host is unreachable for a persistent reason (see `host_unreachable`). |
| `failed` | A terminal attempt failure. The runtime keeps its identity and its place in the list, but holds no child and no port. | Read `view.failure`, fix that cause, then **Start** (Restore) again. The retry reuses the same runtime id with a new `since`. |

Every view also carries `since` (epoch milliseconds of the current state, absent when the
definition never started), `attempt` (spawn attempts for this runtime), `failure` (the code
below, cleared on success), and `failure_piece` (a closed-vocabulary qualifier, present
only with `unsupported_config`).

## Failure codes

`view.failure` is a `TunnelFailureCode`. It is not an `IpcErrorCode`: a refusal before a
runtime exists carries the tunnel code in `details.failureCode` instead.

| code | cause | what to do |
| :--- | :--- | :--- |
| `port_in_use` | The local port is already taken — by another program, by a stale child, or by another tunnel. On a retry the daemon reallocates once and keeps going; a first attempt with an explicit port fails instead. | Free the port or edit the definition to another one. A stale `ssh` from a killed daemon is the usual owner; the next daemon's sweep normally reaps it. |
| `host_unreachable` | No TCP path to the machine: refused, timed out, DNS failure, no route, reset, or the readiness probe never succeeded. | Check the address, port, and network. With a jump host, check the bastion too. |
| `auth_failed` | The server rejected the credentials (`Permission denied (...)`, too many attempts). | Re-enter the password in machine settings, or fix the key/agent entry. Changing either bumps the host revision and stops that host's live tunnels. |
| `host_key_rejected` | The remote host key changed or is unknown (`Host key verification failed`). | Verify the change out of band, then update `known_hosts` for that host. Ferryx never disables host-key checking for a tunnel. |
| `channel_failed` | The transport is up but one forwarded connection failed — the destination refused it, or the server administratively prohibits forwarding. A channel error after establishment keeps the row `listening`; only the failed connection is lost. | Fix the service on the remote side (`remoteHost`/`remotePort`), or enable forwarding for that account on the server. |
| `unsupported_platform` | Every tunnel command is refused on Windows. | Run the tunnel from a macOS or Linux machine. See below. |
| `unsupported_config` | The host's configuration cannot be reproduced safely in a tunnel: a `Host` alias that would silently change the destination, a jump host defined only in config, a password-protected jump, an identity agent, a certificate file, a `Match` block, a bad option, or an algorithm mismatch. `failure_piece` names which. | Edit the host record to carry the endpoint explicitly (address, port, identity file, jump host) instead of relying on `~/.ssh/config`. A directive in the **system** file is the case the host record cannot fix: macOS ships `Include /etc/ssh/ssh_config.d/*`, whose `Ciphers`/`MACs` lines are outside the reproduced allowlist, so a start there refuses before any child exists. Remove the directive, or run the tunnel from a host whose system config is stock. |
| `daemon_unavailable` | The background service is gone or has not been reached. No `ssh` child is classified into it; the code exists so a client can name a daemon it cannot reach instead of inventing a state. | Restart Ferryx; the GUI reconnects and the definitions are re-read from disk. |
| `credential_unavailable` | The host authenticates with a password that is not in the daemon's credential store for this session. | Open machine settings and enter the password once; then Start. |
| `host_changed` | The host record's identity (hostname, username, port, identity file, jump host, disabled flag) changed while the tunnel was live, so the running child no longer matches the record. It was torn down on purpose. | Start again; the new runtime uses the record as it is now. |

Refusals that never create a runtime — reported as an error with `details.reason`
instead of a state — include `host_revision_stale` (re-list and retry), `host_disabled`,
`host_not_found`, `tunnel_spec_not_found`, `invalid_request` (send exactly one of `spec`
and `specId`), `local_only`, `corrupt_store`, `unsupported_version`,
`daemon_shutting_down`, `runtime_unavailable`, `empty_config_unwritable`,
`port_allocation_failed`, `child_reap_timeout`, `store_lock_unavailable`,
`tunnel_store_conflict`, and `revision_required`.

A save can also answer `Details.reason = "tunnel_store_conflict"` when its
`expectedRevision` is stale; the editor offers **Overwrite with these definitions**, which
re-sends with `force`.

## When the definitions file cannot be read

The list response carries `storeStatus`, and the editor reads `kind`, never a guess:

| `storeStatus.kind` | what it means | what the UI offers |
| :--- | :--- | :--- |
| `ok` | Every definition decoded. | Nothing to do. |
| `partial` | Some rows of some hosts could not be decoded. They stay in the file verbatim and are not silently dropped. | **Discard unreadable definitions**, which saves with `discardInvalid` and drops only that host's bad rows. |
| `corrupt` | The file as a whole is not a tunnel store. | **Reset tunnel store**, which moves the file to `ssh_tunnels.corrupt-<unix-seconds>.json` and starts empty; the quarantined path is shown. |
| `unsupported_version` | A newer Ferryx wrote the file. It is valid, not corrupt. | Only the explanation; a reset is refused with `store_not_corrupt`. Update Ferryx. |

## Restore

**Start** on a failed row is the restore path. It reuses the definition, reuses the failed
runtime's id, re-arms a generation, clears `failure`/`failurePiece`, and updates `since`.
The originally allocated port keeps its allocation origin, so a `port_in_use` encountered
during the retry can still reallocate once instead of failing outright.

## What stops a tunnel

- **Stop** in the UI — the child is terminated (SIGTERM, then SIGKILL after two seconds)
  and the definition goes back to `stopped`.
- **Disabling the host** — a start is refused with `host_disabled`, and changing that flag
  is an identity change, so that host's live tunnels are torn down.
- **Editing the host record** (address, user, port, identity file, jump host) — every live
  tunnel of that host moves to `failed` with `host_changed`. The daemon watches the host
  store once every five seconds, and the same rule runs inline on each list and start.
- **Deleting the host** — the same teardown, and its definitions are pruned.
- **Quitting the app** — the daemon reaps its children on a clean shutdown; a crash or
  SIGKILL leaves the children alive, and the next daemon reclaims them by reading
  `ssh_tunnel_runtimes.json` and verifying each child's pid *and* start time before
  signalling its process group.
- **A daemon restart** — nothing is auto-restarted. Saved definitions list as `stopped`;
  press Start.

So a tunnel is a session, not a service unit. It does not come back by itself.

## Why Windows is refused

A tunnel needs a **Unix control socket** and process groups. The daemon proves it is the
local owner over a `flock`-guarded sidecar and a Unix socket; the crash-safety half uses
`flock`, `getpgid`/`kill(-pgid)`, and a start-time read from `ps`. Windows has no
equivalent in this codebase, so every tunnel command is refused before the definitions
store or the runtime registry is even opened, answering `UNSUPPORTED` with
`details.failureCode = "unsupported_platform"`, `details.reason = "unsupported_platform"`,
`details.platform = "windows"`, and `details.missingFacility = "control_socket"`. The
capability is also never advertised to a Windows daemon, so the UI does not offer the
surface at all. Ferryx does not ship tunnels on Windows, and no refusal is a bug report.

## Why remote and dynamic forwards are not offered

Ferryx tunnels are **local forwarding only**. Remote forwarding (`-R`), dynamic forwarding
(`-D`), and SOCKS are not supported, are not emitted by the argv builder, and no UI control
offers them: the feature exists to reach a service that the remote machine can already
reach, and the other three change who can reach what. Requests for them are out of scope
rather than queued.

## Where things live on disk

All under the daemon's data directory (`$FERRYX_DATA_DIR` when set):

- `ssh_tunnels.json` — saved definitions, host revisions, and recorded host identities.
  Written only by the daemon, under a `ssh_tunnels.lock` sidecar.
- `ssh_tunnel_runtimes.json` + `ssh_tunnel_runtimes.lock` — one crash-visible record per
  live runtime generation, so a successor daemon can reclaim an orphan.
- `ssh-tunnel-empty.conf` — the zero-byte `-F` trust root, mode 0600 in the runtime
  directory.

There is no tunnel log file. Diagnostics stay in memory: the daemon classifies the child's
stderr into the codes above, and the raw text never reaches a view, a transition record, or
the disk.
