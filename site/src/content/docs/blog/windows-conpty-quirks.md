---
title: "Windows ConPTY Quirks With Agent TUIs"
description: "Alternate screen buffers, resize events, and known Windows differences when running agent interfaces. Claims map to Ferryx docs and cited sources only."
---

**Windows ConPTY Quirks With Agent TUIs.** Alternate screen buffers, resize events, and known Windows differences when running agent interfaces. Claims map to Ferryx docs and cited sources only.

![windows-conpty-quirks cover](/images/blog/windows-conpty-quirks/cover.png)

## ConPTY differences

Terminal tooling fails in confusing ways because several layers (OS, WebView, PTY, daemon, renderer) can each produce similar symptoms. A structured triage path saves you from restarting blindly and killing sessions that were fine. In the specific case of “Windows ConPTY Quirks With Agent TUIs”, the Ferryx angle is straightforward: alternate screen buffers, resize events, and known windows differences when running agent interfaces. claims map to ferryx docs and cited sources only. Teams evaluating this often arrive from a shared-checkout pain story or from tooling that treats the terminal as a disposable window. Ferryx takes the opposite stance—sessions are daemon-owned, isolation is a first-class action, and everything you need to verify sits behind documented surfaces.

## Alternate screen issues

Most Ferryx issues cluster into install/platform quirks, IME and rendering oddities, daemon socket permissions, and overlay states like Shell exited. The product facts page and architecture doc are the first places to confirm expected behavior before changing anything. Applied to “windows conpty agent tui”, that context matters because the failure modes people search for are almost always failures of ownership or visibility rather than of raw rendering. When the process outlives the window, when the buffer can prove what it missed, and when each agent's files live in a jailed worktree, the same task that used to require three clones and a spreadsheet becomes a pane layout you can read at a glance.

## Practical steps

1. Reproduce once and note the exact surface: desktop GUI, remote client, one platform, all platforms.
2. Confirm expected behavior against docs/facts and the architecture page before assuming a bug.
3. Check the cheapest layer first: permissions, socket paths, package choice, display scaling.
4. Capture evidence (error text, overlay copy, screenshot) before restarting the daemon.
5. If a session must be recovered, respawn the pane rather than killing the daemon that owns other live PTYs.

| Checkpoint | What good looks like |
| --- | --- |
| Isolation | Each parallel actor has its own worktree or a documented reason not to |
| Visibility | Status badges or attention counters answer “who needs me?” without polling |
| Persistence | Closing the window does not end agent processes |
| Evidence | Claims map to [product facts](/docs/facts/) or a linked source |

## How this fits Ferryx today

Ferryx ships calendar-versioned releases for macOS (universal DMG), Windows (x64 installer), and Linux (AppImage and .deb). The desktop shell, daemon, and optional `ferryx-cli` headless binary are described with sources on the [product facts](/docs/facts/) page. If you are new to the workspace model, start with the [introduction](/docs/introduction/); if you want the mechanical details of sessions and replay, read [technical architecture](/docs/architecture/).

![Windows ConPTY Quirks With Agent TUIs illustration](/images/blog/windows-conpty-quirks/body-1.png)

Related reading:
- [/docs/architecture/](/docs/architecture/)
- [/docs/facts/](/docs/facts/)

## FAQ

### Should I restart the daemon first?

Usually not. Daemon-owned sessions are precious; inspect logs and pane state before killing processes other work depends on.

### Where do I report a bug?

GitHub issues on Indosaram/ferryx with platform, release version, and reproduction steps get the fastest useful replies.

### Why does documentation differ from what I see?

Ferryx ships calendar-versioned releases quickly; check that your version matches the facts page date before digging deeper.

## Limits and non-goals

Ferryx is early software: core flows work, but expect rough edges and breaking changes between calendar-versioned releases. Some OS integrations remain platform-specific (Dock badge counters and launchd supervision are macOS-only today), Windows builds are not code-signed yet so SmartScreen warns on first run, and remote features assume you control the host and pairing grants. None of that changes the core claim of this post—daemon-owned sessions, managed worktrees, and visible multi-agent workflows—but it is the honest boundary of what has shipped. Verify anything version-sensitive against the [facts page](/docs/facts/) and the [release notes](https://github.com/Indosaram/ferryx/releases/latest) before you standardise a team workflow on it. If something in this guide disagrees with your build, trust the repository sources linked from product facts and open an issue with your version string attached.

## How to verify these claims

Every concrete statement in this post ties back to a checkable surface: package names and license terms live on [product facts](/docs/facts/), session and buffer behaviour is described in [technical architecture](/docs/architecture/), and keyboard bindings are listed under [shortcuts](/docs/shortcuts/). Where a third-party tool is mentioned, treat vendor docs as authoritative for that tool and re-check dates before quoting them in a decision document. That discipline is why this site does not publish performance benchmarks it has not measured: reproducible architecture facts age better than marketing numbers.

## Further reading in this workspace

Open the [comparison hub](/compare/) if you are still choosing tools, the [use cases](/use-cases/parallel-ai-agents/) if you want workflow narrative, or [privacy](/privacy/) if you are checking what telemetry exists before rolling Ferryx out on a team machine. Each page is maintained with the same verification habit: claims map to sources, dates are explicit, and unmeasured numbers stay out of the text.
