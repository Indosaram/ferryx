---
title: Settings and background behavior
description: How the Ferryx terminal follows the system appearance, how the background service restarts after a crash on Linux and Windows, how to remove agent-state integration, and how to cancel or retry an update download.
---

## Terminal appearance

The terminal follows the app appearance: light, dark, or system. When the appearance is set to system, the terminal switches when your OS switches between light and dark.

Terminal programs that opt in to color-scheme reports (DEC private mode 2031) are told when the scheme changes, so TUIs that support it can switch their own colors. Programs that do not enable mode 2031 receive nothing.

## Background service restart

The Ferryx background service (the daemon) owns your terminal sessions. On Linux and Windows, Ferryx can restart it automatically if it crashes.

- **Linux**: Ferryx installs a systemd user unit, `ferryx-daemon.service`, under `~/.config/systemd/user/` (or `$XDG_CONFIG_HOME/systemd/user/`). It runs as your user and does not enable lingering, so it runs only while you are logged in. If `systemctl --user` is not available, Ferryx starts the daemon without the unit.
- **Windows**: Ferryx registers a per-user scheduled task, `Ferryx\Daemon`, that runs a small watchdog. The watchdog restarts the daemon after a crash and stops once the daemon exits normally. The task runs with your user account and is never elevated.
- **Microsoft Store installs** are excluded: the Store manages their lifecycle, so no task is registered.

### Turning it off

Turn off **Restart background service automatically** in **Settings > General**.

To remove the unit or task by hand:

```bash
# Linux
systemctl --user disable --now ferryx-daemon
rm ~/.config/systemd/user/ferryx-daemon.service
systemctl --user daemon-reload
```

```powershell
# Windows
schtasks /Delete /TN "Ferryx\Daemon" /F
```

## Agent state integration

Ferryx can install hooks into supported coding agents so it can show their state. To opt out, turn off **Agent state integration** in **Settings > Agents**.

- **Remove from this computer** removes the integration Ferryx installed locally.
- **Remove integration** on a remote host removes it from that host.

If you edited a file Ferryx installed, Ferryx keeps that file and reports it as kept modified instead of deleting your changes.

## Update downloads

While an update downloads in **Settings > General**, a **Cancel** button stops the download. Cancel is not offered once the downloaded update is installing.

If a download stalls, or you cancel it, the status line says so and **Retry Install and Relaunch** starts the download again.
