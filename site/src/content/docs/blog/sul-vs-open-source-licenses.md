---
title: "SUL, MIT, AGPL: Terminal Licenses Explained"
description: "What Ferryx SUL-1.0, Warp AGPL client, and classic OSS licenses mean for commercial use and contributions. Claims map to Ferryx docs and cited sources only."
---

**SUL, MIT, AGPL: Terminal Licenses Explained.** What Ferryx SUL-1.0, Warp AGPL client, and classic OSS licenses mean for commercial use and contributions. Claims map to Ferryx docs and cited sources only.

![sul-vs-open-source-licenses cover](/images/blog/sul-vs-open-source-licenses/cover.png)

## License types in plain language

Choosing a terminal for agent-heavy work is less about ANSI rendering and more about process lifetime, isolation, licensing, and remote access. Different tools optimise for different constraints; an honest comparison states what each does well and where the other wins. In the specific case of “SUL, MIT, AGPL: Terminal Licenses Explained”, the Ferryx angle is straightforward: what ferryx sul-1.0, warp agpl client, and classic oss licenses mean for commercial use and contributions. claims map to ferryx docs and cited sources only. Teams evaluating this often arrive from a shared-checkout pain story or from tooling that treats the terminal as a disposable window. Ferryx takes the opposite stance—sessions are daemon-owned, isolation is a first-class action, and everything you need to verify sits behind documented surfaces.

## Ferryx SUL terms

Ferryx is a source-available desktop terminal under the Sustainable Use License (SUL-1.0), free for personal and non-commercial use, written in Rust on Tauri v2 with a headless daemon. Comparisons on this site stick to licensing, architecture, and workflow facts rather than unmeasured performance claims. Applied to “sul vs agpl terminal license”, that context matters because the failure modes people search for are almost always failures of ownership or visibility rather than of raw rendering. When the process outlives the window, when the buffer can prove what it missed, and when each agent's files live in a jailed worktree, the same task that used to require three clones and a spreadsheet becomes a pane layout you can read at a glance.

## Practical steps

1. List your hard requirements: license constraints, platforms, isolation model, remote access, persistence.
2. Map each candidate tool against those requirements using primary sources (docs, LICENSE, pricing pages) with today's date.
3. Ignore benchmark numbers you cannot reproduce; prefer architectural facts you can verify from repositories.
4. Try the top two on a real multi-agent task for a day before committing the team.
5. Record the decision with verification dates so the next re-evaluation is cheap.

| Checkpoint | What good looks like |
| --- | --- |
| Isolation | Each parallel actor has its own worktree or a documented reason not to |
| Visibility | Status badges or attention counters answer “who needs me?” without polling |
| Persistence | Closing the window does not end agent processes |
| Evidence | Claims map to [product facts](/docs/facts/) or a linked source |

## How this fits Ferryx today

Ferryx ships calendar-versioned releases for macOS (universal DMG), Windows (x64 installer), and Linux (AppImage and .deb). The desktop shell, daemon, and optional `ferryx-cli` headless binary are described with sources on the [product facts](/docs/facts/) page. If you are new to the workspace model, start with the [introduction](/docs/introduction/); if you want the mechanical details of sessions and replay, read [technical architecture](/docs/architecture/).

![SUL, MIT, AGPL: Terminal Licenses Explained illustration](/images/blog/sul-vs-open-source-licenses/body-1.png)

Related reading:
- [/compare/warp/](/compare/warp/)
- [/docs/facts/](/docs/facts/)

## FAQ

### Why does Ferryx publish no benchmarks?

Editorial policy: unmeasured numbers go stale and mislead. Claims here map to sources on the product facts page instead.

### Is Ferryx open source?

Source-available under SUL-1.0, which is not an OSI license. It permits your own internal business purposes, non-commercial, or personal use.

### Which tool should I pick?

Write down your constraints first. If you need OSI copyleft, a tool with AGPL/MIT client code may fit better; if you need daemon-owned sessions and worktree isolation today, evaluate Ferryx against that checklist.

## Limits and non-goals

Ferryx is early software: core flows work, but expect rough edges and breaking changes between calendar-versioned releases. Some OS integrations remain platform-specific (Dock badge counters and launchd supervision are macOS-only today), Windows builds are not code-signed yet so SmartScreen warns on first run, and remote features assume you control the host and pairing grants. None of that changes the core claim of this post—daemon-owned sessions, managed worktrees, and visible multi-agent workflows—but it is the honest boundary of what has shipped. Verify anything version-sensitive against the [facts page](/docs/facts/) and the [release notes](https://github.com/Indosaram/ferryx/releases/latest) before you standardise a team workflow on it. If something in this guide disagrees with your build, trust the repository sources linked from product facts and open an issue with your version string attached.

## How to verify these claims

Every concrete statement in this post ties back to a checkable surface: package names and license terms live on [product facts](/docs/facts/), session and buffer behaviour is described in [technical architecture](/docs/architecture/), and keyboard bindings are listed under [shortcuts](/docs/shortcuts/). Where a third-party tool is mentioned, treat vendor docs as authoritative for that tool and re-check dates before quoting them in a decision document. That discipline is why this site does not publish performance benchmarks it has not measured: reproducible architecture facts age better than marketing numbers.
