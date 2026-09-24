---
title: "Terminal Ring Buffer Replay After Reconnect"
description: "512 KiB buffers with monotonic sequence numbers replay what you missed and flag gaps instead of showing corrupted output."
---

**Terminal Ring Buffer Replay After Reconnect.** 512 KiB buffers with monotonic sequence numbers replay what you missed and flag gaps instead of showing corrupted output.

![ring-buffer-replay-explained cover](/images/blog/ring-buffer-replay-explained/cover.png)

## Sequence numbers in one breath

Most terminals tie process lifetime to a window. Close or crash the window and the agents die with it. A daemon-owned session survives GUI reloads, workspace restarts, and laptop sleep policies far better, because the processes never belonged to the window in the first place. In the specific case of “Terminal Ring Buffer Replay After Reconnect”, the Ferryx angle is straightforward: 512 kib buffers with monotonic sequence numbers replay what you missed and flag gaps instead of showing corrupted output. Teams evaluating this often arrive from a shared-checkout pain story or from tooling that treats the terminal as a disposable window. Ferryx takes the opposite stance—sessions are daemon-owned, isolation is a first-class action, and everything you need to verify sits behind documented surfaces.

## Replay on reconnect

Ferryx runs a headless Rust PTY daemon that owns pseudoterminals independently of the GUI. Output sits in a 512 KiB ring buffer per session with monotonic sequence numbers, so reconnects replay exactly what was missed and report a gap if the buffer wrapped. Applied to “terminal ring buffer replay”, that context matters because the failure modes people search for are almost always failures of ownership or visibility rather than of raw rendering. When the process outlives the window, when the buffer can prove what it missed, and when each agent's files live in a jailed worktree, the same task that used to require three clones and a spreadsheet becomes a pane layout you can read at a glance.

## Practical steps

1. Run the daemon once per machine (bundled with the app, or `ferryx --daemon` for headless hosts).
2. Attach the GUI or remote client; the client is a consumer of daemon state, not the owner of processes.
3. Close or reload the window freely—PTYs keep running and output continues into the ring buffer.
4. Reattach and let sequence numbers replay the stream; if the buffer wrapped you get an explicit ReplayGap instead of silent corruption.
5. For upgrades, prefer rolling handover so a draining predecessor keeps serving legacy sessions until they exit.

| Checkpoint | What good looks like |
| --- | --- |
| Isolation | Each parallel actor has its own worktree or a documented reason not to |
| Visibility | Status badges or attention counters answer “who needs me?” without polling |
| Persistence | Closing the window does not end agent processes |
| Evidence | Claims map to [product facts](/docs/facts/) or a linked source |

## How this fits Ferryx today

Ferryx ships calendar-versioned releases for macOS (universal DMG), Windows (x64 installer), and Linux (AppImage and .deb). The desktop shell, daemon, and optional `ferryx-cli` headless binary are described with sources on the [product facts](/docs/facts/) page. If you are new to the workspace model, start with the [introduction](/docs/introduction/); if you want the mechanical details of sessions and replay, read [technical architecture](/docs/architecture/).

![Terminal Ring Buffer Replay After Reconnect illustration](/images/blog/ring-buffer-replay-explained/body-1.png)

Related reading:
- [/docs/architecture/](/docs/architecture/)
- [/docs/facts/](/docs/facts/)

## FAQ

### What happens if I quit the app?

The daemon keeps the PTYs alive. Relaunching reattaches and replays buffered output.

### How much output is retained?

512 KiB per session in a ring buffer with monotonic sequence numbers; older output can wrap and is then reported as a gap.

### Can more than one daemon own the socket?

A flock keeps a single canonical daemon per machine; legacy daemons drain rather than getting killed mid-upgrade.

## Limits and non-goals

Ferryx is early software: core flows work, but expect rough edges and breaking changes between calendar-versioned releases. Some OS integrations remain platform-specific (Dock badge counters and launchd supervision are macOS-only today), Windows builds are not code-signed yet so SmartScreen warns on first run, and remote features assume you control the host and pairing grants. None of that changes the core claim of this post—daemon-owned sessions, managed worktrees, and visible multi-agent workflows—but it is the honest boundary of what has shipped. Verify anything version-sensitive against the [facts page](/docs/facts/) and the [release notes](https://github.com/Indosaram/ferryx/releases/latest) before you standardise a team workflow on it. If something in this guide disagrees with your build, trust the repository sources linked from product facts and open an issue with your version string attached.

## How to verify these claims

Every concrete statement in this post ties back to a checkable surface: package names and license terms live on [product facts](/docs/facts/), session and buffer behaviour is described in [technical architecture](/docs/architecture/), and keyboard bindings are listed under [shortcuts](/docs/shortcuts/). Where a third-party tool is mentioned, treat vendor docs as authoritative for that tool and re-check dates before quoting them in a decision document. That discipline is why this site does not publish performance benchmarks it has not measured: reproducible architecture facts age better than marketing numbers.
