# Capability-only out-of-process block plugins

Plugins are binary WebAssembly modules interpreted by Wasmi in a separate Ferryx
runner process. Native executable manifests are rejected with
`PLUGIN_NATIVE_UNSUPPORTED`; there is no native fallback. This is an intentional
SDK compatibility break. No project-local discovery or automatic approval exists.

## Install and approve

Place `plugin.toml` under the owner's configuration directory:
`~/.config/ferryx/plugins/<name>/` (XDG_CONFIG_HOME overrides .config; Windows
uses HOME or USERPROFILE). The manifest has exactly `name`, `version`, `module`
(absolute binary .wasm path), `sha256` (64 lowercase hex characters), and
`capabilities`. Supported capabilities are `render`, `write`, `listSessions`,
and `getCapabilities`.

Settings displays module path, digest and capabilities before enabling. Approval
binds all those fields. The daemon validates the digest when approving and on
every start, then sends that exact byte snapshot to the runner over stdin.
Changing a path, digest, version or capabilities requires explicit approval again.
Changing bytes without updating the digest fails closed. Old native approvals are
discarded when read, including from handover snapshots; they never approve a Wasm
replacement. Remove native manifests and port them before enabling again.

## Complete guest import surface

The sole linked import is:

```
ferryx.request(pointer: i32, length: i32, output: i32, capacity: i32) -> i32
```

The module exports `memory` and `tick(unix_seconds: i64)`. The trusted runner
calls tick once a second. request reads one UTF-8 JSON daemon request from guest
memory and returns the response JSON into the supplied range, returning its byte
length. Both ranges are validated before sending. Each message is at most 64 KiB;
embedded newlines are rejected. Requests are synchronous and sequential.

Only GetCapabilities, ListSessions, Write and PluginRender are allowed. Every
request is checked in the daemon against the approved manifest. Write also needs
a current owner-created pane grant from `plugin:<name>` to the target; revocation
takes effect on the next operation. Plugins have no own-session exemption.
PluginRender is bound to the originating plugin's view map. Shutdown, grant
creation and every other request are denied, even with an admin pane grant.

No WASI is linked. There are no filesystem, environment, socket, named-pipe,
process, fork, exec, setsid, handle, dynamic-linking or arbitrary clock imports.
Unknown imports fail instantiation before a guest start function runs. The runner
receives an empty environment and piped stdio; the guest has no API for enumerating
or using native handles. The trusted runner itself is not an OS-sandboxed process;
the interpreter and explicit host bridge form the guest boundary, not process
ancestry or PID classification. No plugin process tombstones are retained.

## Budgets and lifetime

Module input is capped at 1 MiB. Guest linear memory is capped at 8 MiB, tables at
1024 elements, and each store at one memory/table/instance. Start and each tick
have 5 million fuel; each tick has at most 32 host requests. Each daemon dispatch
has a 5-second deadline. A runner has a 3605-second outer deadline and at most
3600 ticks. Compilation/start is included in that outer deadline. Wasmi's default
bounded value/call stack applies. These are execution limits, not a claim that
total native interpreter allocation equals the guest memory cap.

Disable, restart and handover stop and reap the runner. EOF closes the guest's
only host bridge if the daemon dies; a computing guest exhausts fuel. No guest
can create descendants. Supervisors must finish teardown before successor start;
only approvals cross handover, never a running guest or ambient authority. Failed
processes retry after 1, 2 and 4 seconds, then stop. Enable/disable and block attach
remain owner actions; protocol version remains 5.

## Rendering

PluginRender emits HTML only into the plugin's own block document. Existing CSP
uses default-src none, no scripts/forms/base URL, and plugin-only image/style
origins. The UI uses an opaque-origin iframe with empty sandbox and no-referrer;
no same-origin, script, Tauri IPC, popup or navigation privilege is granted.
Wasm execution does not establish this rendering boundary: native WebView
cross-origin/CSP checks remain part of consolidated verification.

## Clock migration

`examples/plugins/clock/clock.c` is freestanding and uses only ferryx.request.
Compile with a WebAssembly-capable clang, for example:

```
clang --target=wasm32 -nostdlib -Wl,--no-entry -Wl,--export-memory -Wl,--allow-undefined -o clock.wasm clock.c
shasum -a 256 clock.wasm
```

Set module to the absolute output path and sha256 to that digest in the example
manifest. The shipped manifest deliberately has no valid approval digest. Copy it
to the owner config, approve in Settings, and attach with
`ferryx-cli panes attach --plugin clock`. UTC time is supplied by tick, not WASI.

No builds or tests were run during this source-only repair. Cargo.lock records
Wasmi 0.46.0 and its resolved dependencies; no WASI runtime dependency is added.
All-platform compilation, adversarial execution, teardown and native rendering
checks are deferred to consolidated verification.
