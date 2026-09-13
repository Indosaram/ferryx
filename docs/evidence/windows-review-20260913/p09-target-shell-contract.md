# P09 -> P02 target-shell metadata proposal

Current-source check, 2026-09-13; proposal only, no shared types or protocol edits.

## Confirmed missing contract

- `DaemonSessionDetails` in daemon/protocol.rs:92 carries identity, cwd, size,
  running and sequence bounds, but no resolved shell or path dialect.
- IPC SpawnTerminalRequest accepts optional `shell`; SpawnTerminalResponse wraps
  DaemonSessionDetails. TerminalSessionSummary also lacks shell metadata.
- UI TerminalSession and TerminalTransport attachment/list shapes lack target shell.
- server StoredSessionMeta retains a spawn fingerprint with requested shell/startup;
  this is not the resolved executable and is not exposed to consumers.
- NativeTerminalPane quoteShellPath currently applies POSIX single-quote escaping
  regardless of the session target. Host OS cannot choose correct quoting for cmd,
  PowerShell, WSL, SSH, or a resumed agent process.

## Recommended exact additive shape, pending ownership allocation

Add an optional `inputTarget` to DaemonSessionDetails and mirror it on UI
TerminalSession. Use the existing SpawnOk/AttachOk session envelope (both carry
DaemonSessionDetails), not a new request variant, handshake version, or stream kind.

```ts
type TerminalInputTarget = {
  shell: "cmd" | "powershell" | "posix" | "unknown";
  pathDomain: "windows" | "posix" | "wsl" | "unknown";
};
// Existing DaemonSessionDetails / UI TerminalSession:
inputTarget?: TerminalInputTarget | null;
```

Rust field: `#[serde(default, skip_serializing_if = "Option::is_none")]`
`pub input_target: Option<TerminalInputTarget>`; contained enums serialize lowercase.
Absent/null and unknown values are not permission to assume the GUI host shell.
For compatibility with a future enum value, UI boundary parser must map unsupported
values to unknown rather than crash or silently choose POSIX.

A two-axis contract is needed: POSIX quoting does not imply a local POSIX path.
WSL needs translation, remote SSH needs upload-returned remote path, and Windows
POSIX environments cannot automatically be labeled WSL.

## Producer and lifecycle semantics

Producer is the daemon that owns the PTY, after resolving actual startup command,
not the GUI preference and not a remote proxy's own host OS. Derive known shells
from resolved executable identity, not substrings in arbitrary command lines:
cmd.exe => cmd/windows; powershell.exe or pwsh.exe on Windows => powershell/windows;
known POSIX shell on Unix => posix/posix. Remote peer supplies its own metadata;
proxy forwards unchanged. A custom executable or AgentResume is unknown unless an
explicit input contract exists. wsl.exe alone does not prove the user's default
WSL shell is POSIX; advertise posix/wsl only after that shell is established.

Store the resolved value with session metadata, return it on both spawn and attach,
and copy it to the UI binding only for matching backendSessionId + daemonEpoch.
Reattach must replace cached values; old-daemon omission clears stale metadata.
No reliance on current preferences or restored UI state. Manual `exec`/nested shell
changes are not detectable from launch metadata: mark this launch-time information,
and do not claim it authoritative for arbitrary foreground applications. Live shell
integration would be a separate contract, not part of this additive proposal.

## P02 consumer requirements

Quote according to the advertised shell, translate according to pathDomain, and
use remote-upload-returned path for SSH. WSL conversion must run in the owning WSL
context (distribution identity must be known there); do not hardcode /mnt/c. For
unknown shell/path domain, report unavailable automatic shell escaping rather than
silently paste host-OS-quoted paths. Existing explicit raw paste can stay separate.
Do not treat cmd double quotes alone as protection against percent expansion.

## Exact allocation needed before implementation

One serialized backend owner: daemon/protocol.rs DTO, daemon/server.rs stored metadata
and SpawnOk/AttachOk constructors, and shell-resolution-to-spawn metadata plumbing.
One frontend owner (P02): ui/src/lib/types.ts DTO, actual spawn/attach mapping and
workspace session binding, NativeTerminalPane drop consumer. Discover exact mapper
files before requesting allocation; they are not acquired by this proposal.
P08's current client/server auth work remains independent and its protocol unchanged.
P09 has not modified any of these shared files.

Register tests before that future change: old payload missing field deserializes;
known inputTarget roundtrips spawn and attach; proxy preserves target rather than
local OS; rebind clears stale metadata; real drop dispatch uses distinct cmd,
PowerShell apostrophe, WSL translated path and SSH upload path fixtures. Native
shell execution must return identical intended path arguments, including spaces,
apostrophes, percent and metacharacters. Unknown target negative control emits no
silently guessed shell command.

## Independent P09 status

PATHEXT and USERPROFILE repairs are already implemented, with production-function
RED/GREEN and owned fixture cleanup in impl-p09.md. They did not wait for P08.
HIST-01 mounted UI RED/GREEN and UI build also passed. No Cargo invocation is made
while P02 owns its slot. DS03 native ConPTY shim oracle is still required; this
metadata proposal neither blocks those tests nor constitutes a shim launch fix.
