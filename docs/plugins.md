# Out-of-process block plugin SDK (integration pending)

This branch provides a headless SDK, not a usable Settings feature yet. Plugins
are executables, never native libraries. No project-local discovery is supported.
Nothing is automatically enabled. Out-of-process execution is not an OS sandbox:
an enabled executable runs as the owner and must be trusted like any installed app.

## Manifest and consent

Install a manifest at `~/.config/ferryx/plugins/<name>/plugin.toml` (or beneath
`XDG_CONFIG_HOME/ferryx/plugins`). On Windows HOME falls back to USERPROFILE.
`examples/plugins/clock/plugin.toml` shows the schema: name, version, command argv,
and requested capabilities. Set the command entries to the absolute executable
and script paths on the destination machine. Commands are never shell-expanded.
Manifest names contain only ASCII letters, digits, underscores and hyphens.

The owner adapter must load manifests on a blocking worker, show the full argv
and capabilities in Settings, and explicitly persist the approved full manifest
using `plugins::Approvals`. A changed version, argv or capability set invalidates
approval. Missing approval means no spawn; malformed approval fails closed with
an error. Settings disable must revoke persisted approval and signal the runtime,
then await it; runtime shutdown kills and reaps the process before returning.

## JSON-lines

stdin/stdout carry one JSON value per line, sequentially, with at most 64 KiB per
line. stderr is diagnostic output. No banners or HTML belong on stdout.
The initial request subset uses the existing daemon schema:

```json
{"type":"getCapabilities"}
{"type":"listSessions"}
{"type":"write","sessionId":"target","data":"aGVsbG8="}
```

Supported manifest capabilities are `getCapabilities`, `listSessions`, and `write`.
The CLI command catalog (`ferryx-cli api describe agents`) documents corresponding
agent operations; it is not itself a JSON request schema. This SDK does not claim
that the catalog currently publishes the stdio schema. New daemon requests are
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

Persist approvals in the daemon's data directory for Windows idle restart. Carry
the approvals snapshot with the handover decision record. The owner adapter must
stop and await predecessor supervisors before starting successor supervisors to
avoid duplicate processes. These lifecycle hooks are not yet wired on this branch.

## Block content and remaining integration

Daemon layouts can serialize `Plugin { plugin, view }` content, so identity rides
the existing layout persistence and handover. `plugins::block_document` supplies
HTML with `default-src 'none'`, plugin-origin images/styles, no scripts, no forms
and no base URL. The renderer must put that document in a separate sandboxed
webview with no Tauri IPC, no navigation to other origins and no popup access.
Do not inject plugin HTML into the app document. A CSP helper alone is not webview
isolation.

Still blocked on daemon-core integration: Settings enable/disable transport,
daemon-owned supervisor registry, handover/idle hooks, capabilities advertisement,
`ferryx-cli panes attach --plugin clock`, the render/view protocol, isolated webview
creation and masking during drag, and CLI API schema publication. `server.rs` and
`protocol.rs` are explicitly excluded from this non-lane task while todo 14 owns
the daemon lane. Do not advertise the feature or mark todo 30 complete until these
paths and end-to-end acceptance scenarios are implemented.

The clock example currently exercises the stdio handshake and reports a timestamp
on stderr; it is not yet a renderable clock block.
