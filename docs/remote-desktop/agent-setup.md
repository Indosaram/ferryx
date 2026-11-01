# Connect an agent to MahoRD

MahoRD provides a stdio MCP server and a separate loopback HTTP API. Both
operate a real remote desktop through `maho-client`; neither replaces host
pairing or installs an agent on the remote computer.

## Pair once

Build the host and client using the [README prerequisites](../README.md#build-from-source).
Run the host inside the desktop session you intend to share:

```sh
maho-host --pin generate
```

If the host runs under launchd, systemd, or the Windows service manager, it
has no console: read the bootstrap PIN from the redirected log instead — see
[connect instructions](../README.md#connect-to-a-computer).

On the agent's computer, replace `HOST` and `PIN` with the actual values:

```sh
mkdir -p "$HOME/.config/mahord"
maho-client --host HOST --pin PIN \
  --pairing-store "$HOME/.config/mahord/pairings.json" \
  --frames 10 --timeout-secs 60
```

Approve the pairing at the host console. The client saves the pairing itself;
do not manufacture a JSON record or copy a key from logs. A frame timeout after
successful pairing may still leave the saved record, so inspect the failure
before repeating enrollment. The bootstrap window lasts five minutes.

Read only the saved IDs and names (requires `jq`):

```sh
jq -r '.[] | [.id, .name] | @tsv' "$HOME/.config/mahord/pairings.json"
```

Use that ID and the same store for reconnects. The store contains secret keys;
keep it private and out of version control. MCP `initialize` returns protocol
metadata, not a pairing ID. Custom host ports require `--tcp-port` and, when
different from TCP port plus one, `--udp-port` in every client invocation.

## Register the MCP server

Use absolute binary and store paths. Ensure the client can load its native
FFmpeg libraries in the agent process environment.

### Claude Code

From the project where the tools should be available:

```sh
claude mcp add --transport stdio --scope project mahord \
  -- /absolute/path/to/maho-client --host HOST \
  --pairing-id PAIRING-ID \
  --pairing-store /absolute/path/to/pairings.json --mcp
claude mcp get mahord
```

### Codex

```sh
codex mcp add mahord \
  -- /absolute/path/to/maho-client --host HOST \
  --pairing-id PAIRING-ID \
  --pairing-store /absolute/path/to/pairings.json --mcp
codex mcp get mahord
```

The equivalent table in `~/.codex/config.toml` is:

```toml
[mcp_servers.mahord]
command = "/absolute/path/to/maho-client"
args = ["--host", "HOST", "--pairing-id", "PAIRING-ID", "--pairing-store", "/absolute/path/to/pairings.json", "--mcp"]
```

### Claude Desktop

Open **Settings > Developer > Edit Config** and merge this entry into the
existing `mcpServers` object. On macOS the file is
`~/Library/Application Support/Claude/claude_desktop_config.json`; on Windows it
is `%APPDATA%\Claude\claude_desktop_config.json`.

```json
{
  "mcpServers": {
    "mahord": {
      "command": "/absolute/path/to/maho-client",
      "args": [
        "--host", "HOST",
        "--pairing-id", "PAIRING-ID",
        "--pairing-store", "/absolute/path/to/pairings.json",
        "--mcp"
      ]
    }
  }
}
```

Restart the agent client after changing its configuration. Registration stores
the command; a successful `get` alone does not prove remote screen access.

## Install or inject the reusable skill

The canonical instruction file is
[`skills/mahord-remote-control/SKILL.md`](../skills/mahord-remote-control/SKILL.md).
It has YAML frontmatter and explains tool arguments, screenshot interpretation,
input verification, and cleanup.

For a project-local installation, run from the MahoRD checkout:

```sh
# Claude Code project skill
mkdir -p /path/to/your-project/.claude/skills/mahord-remote-control
cp skills/mahord-remote-control/SKILL.md \
  /path/to/your-project/.claude/skills/mahord-remote-control/SKILL.md

# Codex project skill
mkdir -p /path/to/your-project/.agents/skills/mahord-remote-control
cp skills/mahord-remote-control/SKILL.md \
  /path/to/your-project/.agents/skills/mahord-remote-control/SKILL.md
```

For clients without a skill loader, attach that file as task context or paste
its instructions into the agent's context. Skill injection supplies operating
instructions, not executable tools: register MCP separately, or explicitly
provide access to the HTTP API. MahoRD does not edit global agent settings.

## Verify with an agent

Ask the connected agent:

> Use MahoRD to read the screen dimensions, take and inspect a screenshot,
> move the pointer without clicking, take another screenshot, and release all
> inputs. Report actual observations and any tool errors.

The MCP client initializes the connection and discovers ten tools. Screenshots
are MCP image content: `result.content[]` contains `type: "image"`, `mimeType`,
and base64 `data`. Screen information is JSON inside a text content item with
`width`, `height`, `scale`, and `connected_host`.

For a protocol-only smoke check, pipe newline-delimited requests directly into
the process:

```sh
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"1"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
  | maho-client --host HOST --pairing-id PAIRING-ID \
      --pairing-store /absolute/path/to/pairings.json --mcp
```

MCP does not listen on a TCP port. Stdout carries JSON-RPC; diagnostics use
stderr. This finite pipe checks negotiation, not video readiness. MCP and HTTP
agent sessions have no default overall timeout; `--timeout-secs` sets an
explicit one. Closing MCP stdin releases tracked inputs and ends the session.

## HTTP alternative

```sh
maho-client --host HOST --pairing-id PAIRING-ID \
  --pairing-store /absolute/path/to/pairings.json --agent-server 19735
```

After the listener starts and the first frame arrives:

```sh
curl -fsS http://127.0.0.1:19735/api/v1/health
curl -fsS http://127.0.0.1:19735/api/v1/screen/info
curl -fsS 'http://127.0.0.1:19735/api/v1/screen/screenshot?format=png'
curl -fsS -X POST http://127.0.0.1:19735/api/v1/input/action \
  -H 'Content-Type: application/json' \
  -d '{"action":"mouse_move","x":0.45,"y":0.45,"normalized":true}'
curl -fsS -X POST http://127.0.0.1:19735/api/v1/input/action \
  -H 'Content-Type: application/json' -d '{"action":"release_all"}'
curl -fsS -X POST http://127.0.0.1:19735/api/v1/session/disconnect
```

HTTP screenshots use the `base64` field, unlike MCP's image `data`. The API
binds loopback. A successful dispatch acknowledges sending input, not the
remote application's reaction; inspect a subsequent frame or native state.

## Limits and troubleshooting

- No frame yet: check capture permissions and an active desktop. A successful
  handshake does not imply video is available.
- Pairing failure: use the saved ID with its original store, or enroll with a
  current PIN and host approval.
- Input: use screen pixels or explicitly set `normalized: true`; `(0, 0)` is
  top-left. Confirm focus and host injection privileges before typing.
- Text uses the existing key mapping, not general Unicode text injection.
  `hold_ms`, `delay_ms`, and drag duration do not guarantee paced delivery in
  the current action conversion path.
- There is no screenshot or end-to-end latency guarantee. Receiver sequence
  gaps, stale content age, decoder time, and frame queue recovery measure
  different stages; see the evidence-linked performance section in the README.
