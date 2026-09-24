---
title: "Managing Remote Access to Multiple Hosts"
description: "Patterns for several paired machines: host labels, which daemon answers, and avoiding cross-wiring sessions. Claims map to Ferryx docs and cited sources only."
---

**Managing Remote Access to Multiple Hosts.** Patterns for several paired machines: host labels, which daemon answers, and avoiding cross-wiring sessions. Claims map to Ferryx docs and cited sources only.

![multi-host-remote cover](/images/blog/multi-host-remote/cover.png)

## Host identity model

Long agent runs do not wait for you to be at the desk. Remote access that does not require inbound SSH or a vendor-hosted jump box keeps you checking progress, sending input, and watching CI from a phone without exposing a raw shell to the internet. In the specific case of “Managing Remote Access to Multiple Hosts”, the Ferryx angle is straightforward: patterns for several paired machines: host labels, which daemon answers, and avoiding cross-wiring sessions. claims map to ferryx docs and cited sources only. Teams evaluating this often arrive from a shared-checkout pain story or from tooling that treats the terminal as a disposable window. Ferryx takes the opposite stance—sessions are daemon-owned, isolation is a first-class action, and everything you need to verify sits behind documented surfaces.

## Selection UI

Ferryx pairs a mobile or browser client to your daemon through an authenticated gateway using a 6-digit PIN or QR. The remote client renders terminals in a custom DOM grid rather than xterm.js, and an outbound relay covers machines behind NAT. Applied to “multi host remote terminal”, that context matters because the failure modes people search for are almost always failures of ownership or visibility rather than of raw rendering. When the process outlives the window, when the buffer can prove what it missed, and when each agent's files live in a jailed worktree, the same task that used to require three clones and a spreadsheet becomes a pane layout you can read at a glance.

## Practical steps

1. Enable the gateway on the host and generate a pairing PIN or QR.
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

Ferryx ships calendar-versioned releases for macOS (universal DMG), Windows (x64 installer), and Linux (AppImage and .deb). The desktop shell, daemon, and optional `ferryx-cli` headless binary are described with sources on the [product facts](/docs/facts/) page. If you are new to the workspace model, start with the [introduction](/docs/introduction/); if you want the mechanical details of sessions and replay, read [technical architecture](/docs/architecture/).

![Managing Remote Access to Multiple Hosts illustration](/images/blog/multi-host-remote/body-1.png)

Related reading:
- [/use-cases/remote-terminal-access/](/use-cases/remote-terminal-access/)
- [/docs/architecture/](/docs/architecture/)

## FAQ

### Is this just SSH with extra steps?

No. SSH assumes inbound reachability and hands you a raw shell. Ferryx pairs an authenticated client to a gateway and renders app sessions, with relay support when inbound connections are impossible.

### Do I need to open firewall ports?

Typically no inbound ports: the client reaches the host through the gateway or your own outbound relay.

### What does a pairing QR contain?

Enough information for the client to complete an authenticated pairing handshake with your host; revoke the device from host settings if the phone is lost.

## Limits and non-goals

Ferryx is early software: core flows work, but expect rough edges and breaking changes between calendar-versioned releases. Some OS integrations remain platform-specific (Dock badge counters and launchd supervision are macOS-only today), Windows builds are not code-signed yet so SmartScreen warns on first run, and remote features assume you control the host and pairing grants. None of that changes the core claim of this post—daemon-owned sessions, managed worktrees, and visible multi-agent workflows—but it is the honest boundary of what has shipped. Verify anything version-sensitive against the [facts page](/docs/facts/) and the [release notes](https://github.com/Indosaram/ferryx/releases/latest) before you standardise a team workflow on it. If something in this guide disagrees with your build, trust the repository sources linked from product facts and open an issue with your version string attached.

## How to verify these claims

Every concrete statement in this post ties back to a checkable surface: package names and license terms live on [product facts](/docs/facts/), session and buffer behaviour is described in [technical architecture](/docs/architecture/), and keyboard bindings are listed under [shortcuts](/docs/shortcuts/). Where a third-party tool is mentioned, treat vendor docs as authoritative for that tool and re-check dates before quoting them in a decision document. That discipline is why this site does not publish performance benchmarks it has not measured: reproducible architecture facts age better than marketing numbers.
