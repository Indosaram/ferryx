---
title: "How libghostty-vt Parses Terminal Output"
description: "Escape sequences to grid cells: where Ferryx parses VT streams before WGPU draws them. Claims map to Ferryx docs and cited sources only."
---

**How libghostty-vt Parses Terminal Output.** Escape sequences to grid cells: where Ferryx parses VT streams before WGPU draws them. Claims map to Ferryx docs and cited sources only.

![libghostty-vt-parsing cover](/images/blog/libghostty-vt-parsing/cover.png)

## VT parsing pipeline

Architecture choices explain the product behaviours users feel: why sessions survive window reloads, why replay is honest about gaps, why there is no Electron tax, and why cross-platform input paths look the way they do. In the specific case of “How libghostty-vt Parses Terminal Output”, the Ferryx angle is straightforward: escape sequences to grid cells: where ferryx parses vt streams before wgpu draws them. claims map to ferryx docs and cited sources only. Teams evaluating this often arrive from a shared-checkout pain story or from tooling that treats the terminal as a disposable window. Ferryx takes the opposite stance—sessions are daemon-owned, isolation is a first-class action, and everything you need to verify sits behind documented surfaces.

## Why Ghostty parser

The desktop shell is Rust on Tauri v2. Terminal output is parsed with libghostty-vt, rendered through WGPU on native child surfaces, and owned by a headless PTY daemon with a sequenced ring buffer. UI and daemon talk over a versioned local protocol with 20-byte stream framing. Applied to “libghostty vt parsing”, that context matters because the failure modes people search for are almost always failures of ownership or visibility rather than of raw rendering. When the process outlives the window, when the buffer can prove what it missed, and when each agent's files live in a jailed worktree, the same task that used to require three clones and a spreadsheet becomes a pane layout you can read at a glance.

## Practical steps

1. Start from the daemon: it owns PTYs and buffers, so client crashes are survivable.
2. Follow a keystroke through IME handling to the PTY write to see where latency hides.
3. Follow output from PTY read through VT parse to WGPU present to see the render path.
4. Note the framing rules (fixed headers, a sub-frame tick/32KB coalescing) that keep streams predictable.
5. Map platform-specific surfaces (NSView, HWND, X11, Wayland) behind the shared abstraction when debugging display issues.

| Checkpoint | What good looks like |
| --- | --- |
| Isolation | Each parallel actor has its own worktree or a documented reason not to |
| Visibility | Status badges or attention counters answer “who needs me?” without polling |
| Persistence | Closing the window does not end agent processes |
| Evidence | Claims map to [product facts](/docs/facts/) or a linked source |

## How this fits Ferryx today

Ferryx ships calendar-versioned releases for macOS (universal DMG), Windows (x64 installer), and Linux (AppImage and .deb). The desktop shell, daemon, and optional `ferryx-cli` headless binary are described with sources on the [product facts](/docs/facts/) page. If you are new to the workspace model, start with the [introduction](/docs/introduction/); if you want the mechanical details of sessions and replay, read [technical architecture](/docs/architecture/).

![How libghostty-vt Parses Terminal Output illustration](/images/blog/libghostty-vt-parsing/body-1.png)

Related reading:
- [/docs/architecture/](/docs/architecture/)
- [/docs/facts/](/docs/facts/)

## FAQ

### Why not Electron?

A native stack with libghostty-vt and WGPU avoids the memory and input costs of a web shell while keeping the UI in Rust/Tauri.

### How does replay stay correct?

Monotonic sequence numbers let the client request exactly what it missed and detect ring-buffer wrap as an explicit gap.

### Where does cross-platform risk live?

In the surface and input layers; Ferryx isolates platform code behind explicit modules with fallbacks on other targets.

## Limits and non-goals

Ferryx is early software: core flows work, but expect rough edges and breaking changes between calendar-versioned releases. Some OS integrations remain platform-specific (Dock badge counters and launchd supervision are macOS-only today), Windows builds are not code-signed yet so SmartScreen warns on first run, and remote features assume you control the host and pairing grants. None of that changes the core claim of this post—daemon-owned sessions, managed worktrees, and visible multi-agent workflows—but it is the honest boundary of what has shipped. Verify anything version-sensitive against the [facts page](/docs/facts/) and the [release notes](https://github.com/Indosaram/ferryx/releases/latest) before you standardise a team workflow on it. If something in this guide disagrees with your build, trust the repository sources linked from product facts and open an issue with your version string attached.

## How to verify these claims

Every concrete statement in this post ties back to a checkable surface: package names and license terms live on [product facts](/docs/facts/), session and buffer behaviour is described in [technical architecture](/docs/architecture/), and keyboard bindings are listed under [shortcuts](/docs/shortcuts/). Where a third-party tool is mentioned, treat vendor docs as authoritative for that tool and re-check dates before quoting them in a decision document. That discipline is why this site does not publish performance benchmarks it has not measured: reproducible architecture facts age better than marketing numbers.


## Checklist before you ship this habit

Confirm three things after a week of running this loop: isolation is still one worktree (or documented shared-checkout) per actor, attention reaches you through status badges rather than manual pane polling, and every external claim in your notes maps to a dated source you would still trust next quarter. If any check fails, fix that layer before adding more agents to the workflow; parallelism amplifies both good structure and debt.
