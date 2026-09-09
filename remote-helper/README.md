# ferryx-remote-helper

Standalone remote PTY owner for Ferryx, built without Tauri or GPU dependencies.
The helper owns the PTYs; a short-lived SSH bridge transports framed requests.
Desktop integration and automatic reattachment are tracked in
[`docs/SSH_PROCESS_SURVIVAL_PLAN.md`](../docs/SSH_PROCESS_SURVIVAL_PLAN.md).

## Building

```bash
cargo build --manifest-path remote-helper/Cargo.toml
cargo test --manifest-path remote-helper/Cargo.toml
```

Debug binary: `remote-helper/target/debug/ferryx-remote-helper`
(`ferryx-remote-helper.exe` on Windows).

## Usage

### Start the daemon

```bash
ferryx-remote-helper daemon --root <private-root> --host-id <id>
```

- `--root`: Private directory for PTY state, socket/endpoint (must exist or daemon creates it)
- `--host-id`: Unique identifier for this remote host

Existing state directories must already be private. Missing directories are
created privately. An OS-held lock prevents replacing a live runtime; stale
endpoints may be replaced only after exclusive ownership is established.

The daemon emits:
```json
{"event":"ready","protocol":1}
```

### Bridge mode (SSH tunnel)

```bash
ferryx-remote-helper bridge --stdio --root <private-root>
```

Reads framed JSON requests from stdin, forwards to the daemon socket, writes responses to stdout.

## Protocol

All communication uses framed JSON:
- Frame format: 4-byte big-endian length + JSON bytes
- Max frame size: 1 MiB

Request structure:
```json
{
  "protocol": 1,
  "token": "<daemon-auth-token>",
  "op": "<operation>",
  "params": {}
}
```

Operations:
- `handshake`: Identify daemon and capabilities
- `project.register`: Register a project repository root
- `project.list`: List registered projects
- `worktree.create`: Create a git worktree
- `pty.spawn`: Start a new PTY session
- `pty.list`: List active PTY sessions
- `pty.describe`: Describe a scoped session without creating another process
- `pty.read`: Read buffered PTY output (with optional wait)
- `pty.write`: Write to PTY stdin
- `pty.resize`: Resize PTY
- `pty.stop`: Terminate PTY session

See `src-tauri/src/ferryx_scope/ssh/helper.rs` tests for protocol examples.

Closing the bridge must not terminate remote PTYs. Explicit `pty.stop` is the
session termination operation. A remote helper restart invalidates its previous
epoch; it does not restore processes from that lost helper.
