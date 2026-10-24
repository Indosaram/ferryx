---
title: "Installing Ferryx on Windows and Reading SmartScreen"
description: "x64 installer walkthrough: SmartScreen warnings, SHA-256 verification, and what the MSIX package is not. Claims map to Ferryx docs and cited sources only."
---

**Installing Ferryx on Windows and Reading SmartScreen.** x64 installer walkthrough: SmartScreen warnings, SHA-256 verification, and what the MSIX package is not. Claims map to Ferryx docs and cited sources only.

![ferryx-install-windows cover](/images/blog/ferryx-install-windows/cover.png)

## Installer download

The Microsoft Store is Ferryx's signed Windows channel. The Store signs the MSIX package it publishes, so a Store install shows no SmartScreen warning, and Store installs are updated by the Store rather than by Ferryx's own updater. Ferryx does not buy a separate code-signing certificate. The Store listing (product ID `9NLHQL5JNLM4`) is still pending certification, so the Store is not offered as a download yet. Until it is, the installer to use is the GitHub release `Ferryx_x64-setup.exe`, which is not code-signed.

## SmartScreen warning

Install in this order:

1. **Microsoft Store (signed, once certified).** When the download page lists the Microsoft Store as the first Windows option, install from there, or run `winget install --source msstore 9NLHQL5JNLM4` in a terminal. SmartScreen does not warn on a Store install. Until the listing is certified, the download page does not link the Store.
2. **GitHub installer (unsigned).** Download `Ferryx_x64-setup.exe` and `SHA256SUMS.txt` from the [latest release](https://github.com/Indosaram/ferryx/releases/latest). Before running the installer, check its hash in PowerShell: `Get-FileHash .\Ferryx_x64-setup.exe -Algorithm SHA256` must match the `Ferryx_x64-setup.exe` line in `SHA256SUMS.txt`.
3. **Expect the SmartScreen prompt for the unsigned installer.** Because the `.exe` has no Authenticode signature, Windows shows "Windows protected your PC". Run it only if the hash matched: choose **More info**, then **Run anyway**. The installer installs for the current user and updates itself in place.

The `Ferryx_x64.msix` attached to GitHub releases is the unsigned package that is submitted to the Store. It is not a sideloading installer, so install from the Store or the `.exe` instead.

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

![Installing Ferryx on Windows and Reading SmartScreen illustration](/images/blog/ferryx-install-windows/body-1.png)

Related reading:
- [/docs/introduction/](/docs/introduction/)
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


## Checklist before you ship this habit

Confirm three things after a week of running this loop: isolation is still one worktree (or documented shared-checkout) per actor, attention reaches you through status badges rather than manual pane polling, and every external claim in your notes maps to a dated source you would still trust next quarter. If any check fails, fix that layer before adding more agents to the workflow; parallelism amplifies both good structure and debt.
