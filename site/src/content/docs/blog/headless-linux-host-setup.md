---
title: "Headless Linux Host Setup for Remote Access"
description: "Prepare a Linux box: install, daemon service, pairing, and firewall notes for remote Ferryx access. Claims map to Ferryx docs and cited sources only."
---

**Headless Linux Host Setup for Remote Access.** Prepare a Linux box: install, daemon service, pairing, and firewall notes for remote Ferryx access. Claims map to Ferryx docs and cited sources only.

![headless-linux-host-setup cover](/images/blog/headless-linux-host-setup/cover.png)

## Service install steps

A headless Linux host runs `ferryx-cli`, a single binary for x86_64 that needs glibc 2.17 or newer and no GTK or WebKit. In the desktop app, open Settings > Remote, choose **Add Machine** > **Install command**, and run the line it shows on the host:

```bash
curl -fsSL https://relay.ferryx.dev/install.sh | sh -s -- --key <install-key>
```

The installer checks the binary against the release `SHA256SUMS`, installs it, and runs `ferryx-cli up`. `up` enrolls the host with your account and keeps its daemon running as a systemd user service, enabling linger so the daemon outlives your SSH session; where systemd is not available it starts a detached daemon instead. You do not start `ferryx-cli --daemon` yourself. Without a key, run `curl -fsSL https://relay.ferryx.dev/install.sh | bash` in an interactive terminal: `up` prints an approval code that you enter under **Add Machine** > **Approve a machine**, and it waits for you. `ferryx-cli service status` reports the result, and `ferryx-cli service uninstall` removes the service without stopping a running daemon. The relay tunnel is outbound only, so the host needs no open inbound port.

## Pairing on headless hosts

Ferryx pairs a mobile or browser client to your daemon through an authenticated gateway: sign in to your Ferryx account with an email magic link. The remote client renders terminals in a custom DOM grid rather than xterm.js, and an outbound relay covers machines behind NAT. Applied to “headless linux ferryx host”, that context matters because the failure modes people search for are almost always failures of ownership or visibility rather than of raw rendering. When the process outlives the window, when the buffer can prove what it missed, and when each agent's files live in a jailed worktree, the same task that used to require three clones and a spreadsheet becomes a pane layout you can read at a glance.

## Practical steps

1. Enable the gateway on the host and sign in to your Ferryx account there.
2. Open the Ferryx remote client on the phone or browser and complete pairing; the session binds to the host identity.
3. Browse sessions, tap into a pane, and type as if you were local; the DOM grid keeps layout stable on small screens.
4. For off-LAN access, point the client at your own outbound relay instead of depending on a third-party tunnel.
5. Revoke a lost device from the host so its pairing grant stops working.

| Checkpoint | What good looks like |
| --- | --- |
| Isolation | Each parallel actor has its own worktree or a documented reason not to |
| Visibility | Status badges or attention counters answer “who needs me?” without polling |
| Persistence | Closing the window does not end agent processes |
| Evidence | Claims map to [product facts](/docs/facts/) or a linked source |

## How this fits Ferryx today

Ferryx ships calendar-versioned releases for macOS (universal DMG), Windows (x64, through the Microsoft Store; some releases also attach a direct `.exe`), and Linux (x86_64 AppImage and .deb). The desktop shell, daemon, and optional `ferryx-cli` headless binary are described with sources on the [product facts](/docs/facts/) page. If you are new to the workspace model, start with the [introduction](/docs/introduction/); if you want the mechanical details of sessions and replay, read [technical architecture](/docs/architecture/).

![Headless Linux Host Setup for Remote Access illustration](/images/blog/headless-linux-host-setup/body-1.png)

Related reading:
- [/use-cases/remote-terminal-access/](/use-cases/remote-terminal-access/)
- [/docs/architecture/](/docs/architecture/)

## FAQ

### Is this just SSH with extra steps?

No. SSH assumes inbound reachability and hands you a raw shell. Ferryx pairs an authenticated client to a gateway and renders app sessions, with relay support when inbound connections are impossible.

### Do I need to open firewall ports?

Typically no inbound ports: the client reaches the host through the gateway or your own outbound relay.

### What does pairing grant?

An account sign-in lets the client complete an authenticated pairing handshake with your host; revoke the device from host settings if the phone is lost.

## Limits and non-goals

Ferryx is early software: core flows work, but expect rough edges and breaking changes between calendar-versioned releases. Some OS integrations remain platform-specific (Dock badge counters and launchd supervision are macOS-only today), the direct Windows `.exe` that some releases attach is not code-signed, so SmartScreen warns on it (the Microsoft Store build is signed), and remote features assume you control the host and pairing grants. None of that changes the core claim of this post—daemon-owned sessions, managed worktrees, and visible multi-agent workflows—but it is the honest boundary of what has shipped. Verify anything version-sensitive against the [facts page](/docs/facts/) and the [release notes](https://github.com/Indosaram/ferryx/releases/latest) before you standardise a team workflow on it. If something in this guide disagrees with your build, trust the repository sources linked from product facts and open an issue with your version string attached.

## How to verify these claims

Every concrete statement in this post ties back to a checkable surface: package names and license terms live on [product facts](/docs/facts/), session and buffer behaviour is described in [technical architecture](/docs/architecture/), and keyboard bindings are listed under [shortcuts](/docs/shortcuts/). Where a third-party tool is mentioned, treat vendor docs as authoritative for that tool and re-check dates before quoting them in a decision document. That discipline is why this site does not publish performance benchmarks it has not measured: reproducible architecture facts age better than marketing numbers.
