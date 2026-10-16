# Ferryx Unreleased: complaint gap fixes

- **Status**: unreleased; version and tag are assigned at release time.

## What ships in this release

- **Terminal follows the app appearance**: the native terminal rethemes when the app switches between light, dark, and system appearance. Programs that enable DEC mode 2031 receive an unsolicited color-scheme report on each change; programs that do not enable it receive nothing.
- **Daemon auto-restart on Linux**: a systemd user unit (`ferryx-daemon.service`, no linger) restarts a crashed daemon. Only allowlisted `FERRYX_*` variables are forwarded into the unit. Falls back to an unsupervised daemon when `systemctl --user` is unavailable.
- **Daemon auto-restart on Windows**: a per-user, non-elevated scheduled task (`Ferryx\Daemon`) runs a watchdog that respawns a crashed daemon and follows a handover successor after a clean exit. Microsoft Store installs are excluded. Both platforms can be turned off with **Restart background service automatically** in Settings > General.
- **Agent state integration removal**: **Agent state integration** in Settings > Agents opts out; **Remove from this computer** and per-host **Remove integration** remove installed files, keeping and reporting any file you modified. Remote helper forwarding of agent state follows the same flag.
- **Keyboard copy mode**: <kbd>Cmd+Shift+X</kbd> (macOS) / <kbd>Ctrl+Shift+X</kbd> (Windows, Linux) enters a vi-style copy mode (`hjkl`, `w b e`, `0 ^ $`, `gg G`, `Ctrl-u Ctrl-d`, `H M L`, `v V Ctrl-v`, `y`, `Esc`/`q`). The viewport stays frozen and paste is disabled while it is active.
- **Update download Cancel and Retry**: a **Cancel** button stops an in-progress update download; a stalled or cancelled download explains what happened and offers **Retry Install and Relaunch**.
- **Remappable shortcuts**: Settings > Shortcuts offers Edit, Unbind, Reset, and Reset all per action. Conflicting chords are refused with the owning action named; terminal clipboard chords and macOS app-menu chords stay fixed. Native key interception follows the user's bindings.
