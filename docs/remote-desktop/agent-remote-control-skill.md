---
name: mahord-remote-control
description: Use MahoRD MCP tools to inspect and control a paired remote desktop through screenshots, mouse input, keyboard input, and explicit input release.
---

# MahoRD remote control

## Prerequisite

The agent environment must expose the MahoRD MCP server, launched as:

```sh
/absolute/path/to/maho-client --host HOST --pairing-id PAIRING-ID \
  --pairing-store /absolute/path/to/pairings.json --mcp
```

Pair once with the host's current PIN and approval before registering this
command. The client saves the pairing automatically. Never invent pairing JSON
or disclose the store's keys. This skill supplies instructions; it does not
install the executable, establish pairing, or register MCP by itself.

## Observe, act, verify

1. Call `remote_get_screen_info` and parse JSON from its text content. Fields
   are `width`, `height`, `scale`, and `connected_host`.
2. Call `remote_take_screenshot`. Consume the image, not merely its existence.
   MCP returns image content with `data` (base64) and `mimeType`.
3. Choose a target from the observed screen. Use pixel coordinates or set
   `normalized: true` for the range 0 to 1. Origin is top-left.
4. Execute one relevant action. Check both JSON-RPC `error` and tool
   `result.isError`; dispatch success is not application success.
5. Inspect a subsequent screenshot or application state to verify the result.
   A screenshot may still contain the previous frame. Do not repeat a click
   or destructive action merely because its visual update is delayed.
6. Call `remote_release_all` after an error and when finished. End the MCP
   session by closing its stdin; there is no MCP disconnect tool.

Do not type into a field until its focus is established. Use only the remote
computer and actions requested by the user.

## Tool reference

Pass these argument objects to the corresponding MCP tools. Mouse button
values are `left`, `right`, and `middle`.

| Tool | Arguments |
| --- | --- |
| `remote_get_screen_info` | `{}` |
| `remote_take_screenshot` | `{"format":"png"}`; `jpeg` also supported |
| `remote_mouse_move` | `{"x":0.45,"y":0.45,"normalized":true}` |
| `remote_mouse_click` | Required `x`, `y`; optional `button` (left), `count` (1), `normalized` (false) |
| `remote_mouse_drag` | Required `start_x`, `start_y`, `end_x`, `end_y`; optional `button` (left), `steps` (10), `normalized` (false) |
| `remote_mouse_scroll` | Required `dx`, `dy`; optional `x`, `y`, `normalized` (false) |
| `remote_key_press` | Required `key`, such as `Enter`, `Escape`, `Tab`, `Space`, `F5`; optional `hold_ms` (50) |
| `remote_hotkey` | Required `keys`, e.g. `["Control","Shift","t"]` |
| `remote_type_text` | Required `text`; optional `delay_ms` (20) |
| `remote_release_all` | `{}` |

Example raw MCP request:

```json
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"remote_mouse_move","arguments":{"x":0.45,"y":0.45,"normalized":true}}}
```

MCP is newline-delimited JSON-RPC over stdin/stdout, not HTTP. The client
negotiates protocol version `2024-11-05`; logs go to stderr. An MCP client
normally owns initialization, tool discovery, and response IDs.

## HTTP alternative

When the user supplies shell/API access instead of MCP, launch the same paired
client with `--agent-server 19735`. Use loopback endpoints:

- `GET /api/v1/health`
- `GET /api/v1/screen/info`
- `GET /api/v1/screen/screenshot?format=png` (image in `base64`, not `data`)
- `POST /api/v1/input/action` with `{"action":"mouse_move","x":0.45,"y":0.45,"normalized":true}`
- `POST /api/v1/input/action` with `{"action":"release_all"}`
- `POST /api/v1/session/disconnect`

The instruction file alone does not grant an HTTP client or a running session.

## Limits

- Agent modes have no default overall timeout; `--timeout-secs` is explicit.
- Screenshots require a decoded frame and usable host capture permissions.
- Text typing uses key mappings, not arbitrary Unicode insertion.
- Timing fields are accepted but do not guarantee paced input; do not infer
  a held-key duration from `hold_ms` or typing cadence from `delay_ms`.
- A pointer may be separate from video pixels. Use native cursor observation
  when the task requires exact pointer-position proof.
- Decoder time and receiver gap statistics are not end-to-end display latency
  or direct measurements of network loss.
