# Owner Luau scripting (luauApiV1)

Open **Settings > Scripting**, enable scripting, then use **Reload script** after
editing your owner configuration. The same section shows the daemon log and a
Refresh log button. Disabled is the default. Reload errors are shown in Settings;
initialization, timer and event failures are recorded in `<data>/script.log`.
`ferryx-cli script log` also reads that file.

Only `$XDG_CONFIG_HOME/ferryx/init.luau` (default
`$HOME/.config/ferryx/init.luau`) on Unix or `%APPDATA%\ferryx\init.luau` on
Windows is loaded. Neither repositories nor the current directory supply scripts.
The enable flag is persisted in `<data>/settings.json`; unrelated settings are
preserved. No new entitlement or MSIX capability is needed.

```lua
assert(ferryx.version == 'luauApiV1')
ferryx.bind('cmd+shift+a', function(session)
    local pane = ferryx.split(session, 'right')
    ferryx.send(pane, 'echo luau\n')
end)
ferryx.on('agent.done', function(event)
    ferryx.notify('Agent finished', event.sessionId)
end)
```

## Functions

- `ferryx.bind(keys, callback, options?)`: registers a chord such as `cmd+shift+a`
  or `ctrl+alt+a`. Modifiers are cmd, ctrl, alt, shift; order and case do not
  matter. Re-registering a chord replaces it. The callback receives the focused
  backend session ID. A script only claims an otherwise unhandled app chord,
  unless `{override=true}` explicitly grants precedence over default shortcuts.
  IME composition, repeated keydown and editable controls do not invoke bindings.
- `ferryx.on(event, callback)`: subscribes to `agent.state`, `agent.done`,
  `session.exit`, or `layout.changed`. Agent events contain `sessionId`, `state`,
  `agent`, and `outcome`. Exit contains `sessionId`. Layout contains `window` and
  `revision`. Handlers run serially in the daemon worker, not the desktop.
- `ferryx.split(session, direction)`: creates a shell inheriting the source
  session's workspace, worktree, cwd and dimensions, applies a daemon layout
  intent, and returns its backend session ID. Direction is right, left, up or
  down. The source must be attached. Layout conflicts reject the split and close
  its newly created shell; existing shells are not closed.
- `ferryx.send(session, text)`: writes literal UTF-8 bytes to the session through
  the ordinary owner pane gate. Include a newline when submitting a command.
- `ferryx.read(session, lines)`: returns up to 1..10,000 retained plain-text rows,
  including available scrollback. It does not change the scrollback budget.
- `ferryx.notify(title, body)`: forwards a notification to attached desktops using
  the existing notification delivery service and permission policy.
- `ferryx.exec(argv)`: runs a nonempty dense array of strings as one executable
  and arguments, never through a shell. For example `{'git', 'status', '--short'}`.
  The cwd is the current key/agent/session callback's session cwd. A callback
  without session context raises `NO_SESSION_CONTEXT`. The host operation runs
  through `run_blocking`, with a 30-second timeout and kill-on-drop. Returns
  `{code, stdout, stderr}`. A string such as `ferryx.exec('sh -c rm')` raises
  `BAD_ARGUMENT` before anything executes. Explicitly naming a shell executable
  is an owner decision, not automatic shell interpolation.
- `ferryx.after(milliseconds, callback)`: schedules one invocation and returns
  its timer identity. Delay is a positive integer.
- `ferryx.every(milliseconds, callback)`: schedules recurring invocations with
  the same durable identity. Missed periods are not replayed in a burst.
  Both timer functions share a limit of 64 pending timers.
- `ferryx.log(message)`: appends a string to the daemon script log.
- `ferryx.layout(window, baseRevision, intent)`: applies the actual todo-9 layout
  intent schema through the same owner request as the desktop. For example
  `{type='ACTIVATE_TAB', tabId='tab-id'}`. A stale revision raises
  `LAYOUT_CONFLICT`; fetch a fresh revision via a layout.changed callback or
  desktop tooling instead of overwriting another client's layout.

## Execution and recovery

Scripts are owner-authored: session writes, spawns and layout changes use ordinary
owner daemon requests and the existing pane gate. Pane peers cannot enable,
reload or invoke scripts. Functions reject malformed input before host effects.
The sandbox has no io, os, package, debug, dynamic loading, require or catchable
interrupt escape; it retains the 64 MiB allocator and 50 ms thread-CPU budget.
Waiting for host work does not consume thread CPU but does serialize callbacks.

Bindings and subscriptions are rebuilt from owner source after handover and
Windows idle restart. The enable preference and existing script ledger remain
on disk. Initialization effects and timer claims are durable before execution;
this is at-most-once, not a transaction: a crash after a claim can lose an effect.
The predecessor's execution fence covers the full callback and is paused before
the handover ledger snapshot. Live key and event callbacks are not replayed.
No script callback state or Lua closure is serialized.
