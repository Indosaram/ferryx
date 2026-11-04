# Out-of-process block plugin SDK

Plugins are executables, never native libraries. No project-local discovery is supported.
Nothing is automatically enabled. Out-of-process execution is not an OS sandbox:
an enabled executable runs as the owner and must be trusted like any installed app.

## Manifest and consent

Install a manifest at `~/.config/ferryx/plugins/<name>/plugin.toml` (or beneath
`XDG_CONFIG_HOME/ferryx/plugins`). On Windows HOME falls back to USERPROFILE.
`examples/plugins/clock/plugin.toml` shows the schema: name, version, command argv,
and requested capabilities. Set the command entries to the absolute executable
and script paths on the destination machine. Commands are never shell-expanded.
Manifest names contain only ASCII letters, digits, underscores and hyphens.

Settings > Terminal > Block plugins shows the full argv and capabilities. Enabling
explicitly persists the approved full manifest using `plugins::Approvals`.
A changed version, argv or capability set invalidates
approval. Missing approval means no spawn; malformed approval fails closed with
an error. Settings disable revokes persisted approval and signals the runtime,
then awaits it; runtime shutdown kills and reaps the process before returning.

## JSON-lines

stdin/stdout carry one JSON value per line, sequentially, with at most 64 KiB per
line. stderr is diagnostic output. No banners or HTML belong on stdout.
The initial request subset uses the existing daemon schema:

```json
{"type":"getCapabilities"}
{"type":"listSessions"}
{"type":"write","sessionId":"target","data":"aGVsbG8="}
{"type":"pluginRender","view":"main","html":"<h1>Clock</h1><time>12:00</time>"}
```

Supported manifest capabilities are `getCapabilities`, `listSessions`, `write` and
`render`. `ferryx-cli api describe panes --json` publishes `pluginRequests`, the
stdio request types, properties, capabilities and framing limits. New daemon requests are
denied by default, even if the pane policy treats them as reads.

A manifest capability is necessary but not a pane grant. Write additionally needs
an owner-created grant from `plugin:<name>` to the target with the `write` op.
An ungranted Write returns:

```json
{"code":"FORBIDDEN","reason":"PANE_GRANT_REQUIRED"}
```

The runtime invokes the daemon adapter only after both policies pass. It never
connects as an Owner peer. A plugin has no own-session/workspace exemption.
Unknown capabilities are rejected at manifest load. Unsupported requests return
`FORBIDDEN` / `PLUGIN_CAPABILITY_REQUIRED`.

## Lifetime and recovery

`plugins::runtime::supervise` requires an approved manifest and a disable watch
receiver. It reports each started PID to the adapter and reaps children on disable.
After an exit or launch failure it retries at 1, 2, and 4 seconds, then stops.
Oversized, malformed framing also ends the process and consumes the restart budget.
JSON parse errors return a structured `INVALID_REQUEST` without dispatching.

Approvals persist beside the session data for Windows idle restart. The committed
handover decision record carries the approval snapshot. The predecessor stops and
awaits supervisors before committing; the successor restores approvals before
starting supervisors. The predecessor stays alive to serve retained terminals.

## Block content

Daemon layouts can serialize `Plugin { plugin, view }` content, so identity rides
the existing layout persistence and handover. `plugins::block_document` supplies
HTML with `default-src 'none'`, plugin-origin images/styles, no scripts, no forms
and no base URL. The desktop renderer uses a separate opaque-origin sandboxed
iframe browsing context with no scripts, same-origin access, Tauri IPC, forms,
top navigation or popups. It never injects plugin HTML into the app document.
Blocks are masked throughout pane/tab drag gestures without destroying them.

After enabling, `ferryx-cli panes attach --plugin clock [--window <label>]` adds a
Plugin leaf to the daemon-owned layout. The daemon advertises `blockPluginsV1`.
`pluginRender` updates only the originating plugin's view map. `PluginViewChanged`
events refresh block documents; rendering does not poll the daemon.

## Clock example

Copy `examples/plugins/clock/plugin.toml` into the owner configuration directory's
`clock/` folder and replace both absolute paths in `command`. Enable clock in
Settings, then run `ferryx-cli panes attach --plugin clock`. The Bun process sends
a fresh scriptless clock document every second. Lifecycle logs record the PID;
disable clock in Settings to revoke approval and kill/reap the process.

No builds, tests or QA were executed during implementation. Acceptance is deferred
to the combined integration verification required by the work plan.
