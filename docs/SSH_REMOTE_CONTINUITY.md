# SSH Remote Continuity

SSH remote continuity keeps a remote terminal alive when the Ferryx desktop app,
its local daemon, or the SSH relay disconnects. The remote PTY belongs to a detached
Ferryx resident daemon on the remote host. Each connection uses a short-lived
`--relay-bridge` process to carry daemon protocol v3 newline-delimited JSON over SSH
stdio. Losing the SSH channel terminates the bridge, not the resident daemon or its
PTYs; reconnect creates a new bridge, reattaches the existing session, and replays
buffered output.

This applies only to PTYs owned by the remote resident daemon. It does not make an
ordinary remote shell survive independently and does not require tmux.

## Continuity modes

Continuity is configured per SSH host:

| Mode | Behavior |
| --- | --- |
| `auto` | Default. Use the resident daemon when a compatible deployment is already alive. If none is available, keep using the ordinary local `ssh -tt` PTY path and offer **Enable continuity**. `auto` never installs software by itself. |
| `on` | Continuity is explicitly enabled. Ferryx may deploy and start the versioned resident daemon for this host, then create terminals through the relay. This is the user-consent boundary for deployment. |
| `off` | Never deploy or use the resident daemon for this host. Terminals use the existing local `ssh -tt` PTY path and therefore end when that local PTY/SSH process ends. |

To opt in without automatic installation, leave a host on `auto` until you explicitly
choose **Enable continuity**. To return to the legacy behavior, set the host to `off`.
Changing to `off` prevents future relay use; remove the remote files separately if
you also want to uninstall the deployed daemon.

## Remote deployment and removal

Deployments are versioned under the remote user's home directory:

```text
~/.ferryx-remote/daemon-<version>/
```

On macOS and Linux, the directory contains the `ferryx` executable plus runtime
artifacts such as `daemon.pid` and `daemon.log`. Ferryx garbage collection retains
the current and immediately previous deployment and removes older version
directories. It does not remove worktrees or other user data.

Before manual removal, close remote panes for the host and set continuity to `off`.
Then stop the resident daemon and remove only the Ferryx remote deployment root for
the same remote user. For macOS/Linux, after confirming the PID belongs to Ferryx:

```sh
kill "$(cat ~/.ferryx-remote/daemon-<version>/daemon.pid)"
rm -rf -- ~/.ferryx-remote
```

If the PID file is absent or stale, identify the resident `ferryx --daemon` process
for that user before stopping it. Do not run the removal command as root unless the
deployment was intentionally created for root.

## Windows remote hosts

Native Windows hosts use the same architecture: a hidden, detached
`ferryx.exe --daemon` owns PTYs and a per-SSH-channel `ferryx.exe --relay-bridge`
connects to its loopback daemon endpoint. Deployments are stored at:

```text
%USERPROFILE%\.ferryx-remote\daemon-<version>\ferryx.exe
```

Ferryx uses PowerShell for platform detection, deployment, process launch, readiness,
liveness checks, and version garbage collection. SSH ControlMaster reuse is a POSIX
optimization and is not required on Windows. For manual removal, set continuity to
`off`, close the host's panes, stop the matching `ferryx.exe --daemon` process, and
remove `%USERPROFILE%\.ferryx-remote` as that user.

## Requirements and agent rules

- SSH authentication remains the system SSH client's responsibility. Ferryx does not
  store remote credentials.
- D5 applies: Ferryx does not install coding agents, Git, shells, runtimes, or other
  host dependencies. The only deployment authorized by continuity=`on` or an explicit
  **Enable continuity** action is Ferryx's own versioned remote daemon binary.
- Agent resume uses provider session IDs captured from the agent's own output/state.
  Ferryx never invents, mints, substitutes, or injects an agent session ID.
- A successful reattach keeps the existing remote PTY and agent process. Ferryx only
  starts an agent resume command when that PTY is known to be terminated or the
  resident daemon is gone and reattachment is impossible.
- Users are responsible for installing and authenticating agent CLIs on the remote
  host. If an agent or its resume command is unavailable, Ferryx surfaces that remote
  error rather than installing or replacing it.

## Security and scope

The relay is SSH stdio carrying daemon protocol v3 frames. It does not open a public
remote TCP listener, create an SSH dynamic forwarding (`ssh -D`) proxy, or provide a
remote file explorer/editor. Remote worktree discovery and terminal startup remain
the SSH-host features documented in [SSH_REMOTE_HOSTS.md](SSH_REMOTE_HOSTS.md).
