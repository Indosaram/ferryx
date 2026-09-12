# Installed failure: read-only session probe

The installed GUI PID 17288 and WebView PID 19396 still have the same start
times recorded during screenshot attribution. They were not restarted.

Reading `AppData/Roaming/com.ferryx.app/session_state.json` found the active
Strawberry workspace at `\\?\C:\Strawberry`, with two terminal tabs and no
browser tabs. Their backend session IDs are:

- `aeb0bde0-188d-4ee4-b024-d4e9f7ddc7f9`.
- `28ff42bb-b9cf-4a51-8902-dd2c35967565`.

Read-only NDJSON requests to the existing daemon at 127.0.0.1:53986:

```json
{"type":"handshake","version":2}
{"type":"listSessions"}
```

Actual responses, exit 0:

```json
{"type":"handshakeOk","version":2,"pid":20196,"epoch":1789212002050}
{"type":"listSessionsOk","epoch":1789212002050,"sessions":["aeb0bde0-188d-4ee4-b024-d4e9f7ddc7f9","28ff42bb-b9cf-4a51-8902-dd2c35967565"]}
```

This rules out missing daemon sessions for these active tabs; it does not
prove the native compositor's raw failure. No attach, write, resize, signal,
close, restart, or session-state mutation was sent.

The application-specific log directories contained no useful native error
log. WebView cache log inventory and Temp Ferryx log inventory did not expose
the raw bounds console error. The persisted session snapshot was copied to
`/tmp/ferryx-installed-session-state.json` for this read only.

## New historical discriminator

`3fa25a1942ebb365515f8f8854a765c356bdf154` (2026-08-29) replaces a Windows
compositor that wraps the root HWND and reports `layer_backed: false` with a
real isolated child HWND. Its commit description identifies rejection by
`validate_desktop_composition` on every first bounds call.

This matches the symptom but remains a hypothesis for the installed binary.
The installed executable was copied without execution to
`/tmp/ferryx-installed-51ab67ee.exe` for static implementation identification.
mtime alone is not accepted as identity. A separate bounded binary inspection
is running; it must supply stronger evidence before this cause is assigned.

## Binary inspection result

`installed-compositor-binary.md` now supplies positive disassembly evidence:
the constructor retains the parent Win32 handle pair, its caller passes
descriptor bytes `00 00 02`, and the validator rejects the zero layer-backed
byte before renderer creation. The lead independently disassembled
`0x140472b71..0x140472bdf` and confirmed the constant descriptor and actual
validator call. This identifies a deterministic startup-blocking defect in
the installed binary, without claiming a recovered runtime console event.

The existing source fix is `3fa25a19`, not a new speculative patch. A bounded
same-assertion source-seam RED/GREEN verification is in progress. Its report
must distinguish descriptor validation from actual child creation/visibility,
which is separately covered by the captured current Windows GUI evidence.
