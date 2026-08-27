# Native Terminal Wheel Scrolling

## Root cause

The macOS compositor view correctly passes wheel events to the underlying React pane. The React
handler did run, but it sent an externally tagged enum object:

```json
{ "Delta": { "rows": 3 } }
```

`NativeTerminalScrollBehavior` is an internally tagged Rust enum, so Tauri expects this wire
format instead:

```json
{ "type": "delta", "rows": 3 }
```

Serde rejected the original payload before `cmd_native_terminal_scroll` ran. No scrollback
mutation or native surface render occurred.

## Fixed behavior

`NativeTerminalPane` now sends `{ type: "delta", rows }` for positive and negative wheel
deltas. The IPC command maps it to Ghostty's viewport scroll API and re-renders the native
surface. Negative row deltas move into older scrollback; positive row deltas move toward the live
prompt.

## Regression coverage

- The UI component test asserts the correct internally tagged payload for both wheel directions.
- The Rust input-boundary contract deserializes all public scroll payload variants and rejects the
  old externally tagged format.
- The native terminal contract verifies scrolling from the bottom by `-10` rows renders older
  lines, then that absolute row scrolling renders the requested history line.

## Required desktop QA

Desktop input is not automated in this workspace. Run the app, execute `seq 1 200`, scroll up
over the terminal, and confirm early lines become visible; scroll down and confirm the prompt is
reached again. Verify a mouse-aware terminal program such as `vim` separately.
