---
title: "Installing Ferryx on Linux: AppImage or Deb"
description: "AppImage versus deb choice, execute permissions, and desktop entry basics for Linux installs. Claims map to Ferryx docs and cited sources only."
---

**Installing Ferryx on Linux: AppImage or Deb.** AppImage versus deb choice, execute permissions, and desktop entry basics for Linux installs. Claims map to Ferryx docs and cited sources only.

![ferryx-install-linux cover](/images/blog/ferryx-install-linux/cover.png)

## Choosing a package

Ferryx publishes two desktop packages for Linux, both for x86_64 only. Both are built on Ubuntu 22.04 and need glibc 2.35 or newer, WebKitGTK 4.1, and GTK 3; a release that would need a newer glibc is refused before it is published. There are no ARM64 Linux packages. Check your machine first:

```bash
uname -m                # must print x86_64
ldd --version | head -n1  # glibc 2.35 or newer
```

- **`.deb`** for Debian, Ubuntu and their derivatives. The package declares its dependencies (`libwebkit2gtk-4.1-0`, `libgtk-3-0`, an appindicator library and ALSA), so `apt` installs them and adds the desktop entry for you.
- **AppImage** for other glibc distributions. It is a single file you run in place; it does not add a menu entry by itself.

Download from the [latest release](https://github.com/Indosaram/ferryx/releases/latest) and check the file before you install it. `SHA256SUMS` lists every release file and `SHA256SUMS.minisig` signs that list with the app's updater key. [minisign](https://jedisct1.github.io/minisign/) is not packaged for Ubuntu 22.04, so take its release build there:

```bash
curl -fLO https://github.com/Indosaram/ferryx/releases/latest/download/Ferryx_amd64.deb
curl -fLO https://github.com/Indosaram/ferryx/releases/latest/download/SHA256SUMS
curl -fLO https://github.com/Indosaram/ferryx/releases/latest/download/SHA256SUMS.minisig
minisign -Vm SHA256SUMS -P RWTLZ1zCtZ3JEO3XhjZZ6Uq3rQtEz2FbBcfxxl/+aFlNKJepqoQNj2Zm
sha256sum -c SHA256SUMS --ignore-missing
sudo apt install ./Ferryx_amd64.deb
```

## Making AppImage executable

Download `Ferryx_amd64.AppImage` the same way, check it against `SHA256SUMS`, then mark it executable and run it:

```bash
chmod +x Ferryx_amd64.AppImage
./Ferryx_amd64.AppImage
```

An AppImage mounts itself with FUSE. If it refuses to start on a system without FUSE, extract it and run the extracted copy instead: `./Ferryx_amd64.AppImage --appimage-extract && ./squashfs-root/AppRun`.

## Headless servers

A server without a display does not need either package. The headless `ferryx-cli` runs the same PTY daemon, links no GTK or WebKit, and needs glibc 2.17 or newer on x86_64. In the desktop app, open Settings > Remote, choose **Add Machine** > **Install command**, and run the line it shows on the server:

```bash
curl -fsSL https://relay.ferryx.dev/install.sh | sh -s -- --key <install-key>
```

The installer checks the binary against the release `SHA256SUMS`, installs it to `~/.local/bin` (or `/usr/local/bin` as root), then runs `ferryx-cli up`. `up` enrolls the server with your account and keeps its daemon running as a systemd user service with linger enabled, or as a detached daemon where systemd is not available. Without `--key`, `up` prints an approval code that you enter under **Add Machine** > **Approve a machine**. `ferryx-cli service status` shows how the daemon is kept running. The installer stops with exit code 2 on arm64 and on musl-based distributions such as Alpine.

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

![Installing Ferryx on Linux: AppImage or Deb illustration](/images/blog/ferryx-install-linux/body-1.png)

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
