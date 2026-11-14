---
title: "Installing Ferryx on macOS: DMG, Gatekeeper, Notarization"
description: "Download the universal DMG, clear Gatekeeper warnings, and understand notarization prompts on first launch. Claims map to Ferryx docs and cited sources only."
---

**Installing Ferryx on macOS: DMG, Gatekeeper, Notarization.** Download the universal DMG, clear Gatekeeper warnings, and understand notarization prompts on first launch. Claims map to Ferryx docs and cited sources only.

![ferryx-install-macos cover](/images/blog/ferryx-install-macos/cover.png)

## Download and verify

Ferryx ships one universal DMG for Apple Silicon and Intel Macs, macOS 10.15 or later. It is signed with a Developer ID and notarized by Apple, so it installs without disabling Gatekeeper.

1. Download [Ferryx_universal.dmg](https://github.com/Indosaram/ferryx/releases/latest/download/Ferryx_universal.dmg) with `SHA256SUMS` and `SHA256SUMS.minisig` from the [latest release](https://github.com/Indosaram/ferryx/releases/latest).
2. Check the download before you open it. In the folder holding the files, run `shasum -a 256 -c SHA256SUMS --ignore-missing`. The DMG line must read `OK`. To confirm the checksum list itself came from the release, run `minisign -Vm SHA256SUMS -P RWTLZ1zCtZ3JEO3XhjZZ6Uq3rQtEz2FbBcfxxl/+aFlNKJepqoQNj2Zm` (`brew install minisign`); the key is the app's updater key.
3. Optional: confirm Apple's verdict yourself with `spctl -a -vvv -t install Ferryx_universal.dmg`. A good download reports `accepted` and `source=Notarized Developer ID`. The release process runs this same check on every published DMG and refuses to publish one that fails it.

## Gatekeeper prompts

Gatekeeper steps for the notarized DMG:

1. Double-click `Ferryx_universal.dmg`. macOS mounts it and Gatekeeper checks the notarization. On a slow or offline network, the first open can pause for a few seconds while that check runs.
2. Drag **Ferryx** onto the **Applications** shortcut in the DMG window, then eject the DMG.
3. Open Ferryx from Applications or Launchpad. The first launch shows the standard prompt that Ferryx is "an app downloaded from the Internet." Click **Open**. This prompt appears once per installed copy.
4. If macOS instead says Apple cannot check the app for malicious software, or that it "is damaged and can't be opened," the copy did not come intact from the release page. Delete it, download again, and repeat the checksum step. Do not run `xattr -cr` or turn off Gatekeeper. A notarized build never needs either.
5. If you copied the app in some other way and macOS blocks it, open **System Settings > Privacy & Security** and click **Open Anyway** next to the Ferryx message, then confirm with your password. Prefer a fresh download over this step.

Updates installed from inside Ferryx are signed and notarized the same way, so they do not show the first-launch prompt again. Running sessions survive the update because the daemon hands them to the new version.

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

Ferryx ships calendar-versioned releases for macOS (universal DMG), Windows (x64, through the Microsoft Store; some releases also attach a direct `.exe`), and Linux (x86_64 AppImage and .deb). The desktop shell, daemon, and optional `ferryx-cli` headless binary are described with sources on the [product facts](/docs/facts/) page. If you are new to the workspace model, start with the [introduction](/docs/introduction/); if you want the mechanical details of sessions and replay, read [technical architecture](/docs/architecture/).

![Installing Ferryx on macOS: DMG, Gatekeeper, Notarization illustration](/images/blog/ferryx-install-macos/body-1.png)

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

Ferryx is early software: core flows work, but expect rough edges and breaking changes between calendar-versioned releases. Some OS integrations remain platform-specific (Dock badge counters and launchd supervision are macOS-only today), the direct Windows `.exe` that some releases attach is not code-signed, so SmartScreen warns on it (the Microsoft Store build is signed), and remote features assume you control the host and pairing grants. None of that changes the core claim of this post—daemon-owned sessions, managed worktrees, and visible multi-agent workflows—but it is the honest boundary of what has shipped. Verify anything version-sensitive against the [facts page](/docs/facts/) and the [release notes](https://github.com/Indosaram/ferryx/releases/latest) before you standardise a team workflow on it. If something in this guide disagrees with your build, trust the repository sources linked from product facts and open an issue with your version string attached.

## How to verify these claims

Every concrete statement in this post ties back to a checkable surface: package names and license terms live on [product facts](/docs/facts/), session and buffer behaviour is described in [technical architecture](/docs/architecture/), and keyboard bindings are listed under [shortcuts](/docs/shortcuts/). Where a third-party tool is mentioned, treat vendor docs as authoritative for that tool and re-check dates before quoting them in a decision document. That discipline is why this site does not publish performance benchmarks it has not measured: reproducible architecture facts age better than marketing numbers.


## Checklist before you ship this habit

Confirm three things after a week of running this loop: isolation is still one worktree (or documented shared-checkout) per actor, attention reaches you through status badges rather than manual pane polling, and every external claim in your notes maps to a dated source you would still trust next quarter. If any check fails, fix that layer before adding more agents to the workflow; parallelism amplifies both good structure and debt.
