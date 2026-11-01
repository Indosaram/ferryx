# Windows login-screen streaming (MahoRDHost service)

Streaming the Windows logon screen, lock screen and UAC consent prompt requires a
host that can follow the console session onto the **secure desktop**
(`WinSta0\Winlogon`). A process running inside a logged-on user session cannot:
DXGI loses access the moment Windows switches away from `WinSta0\Default`.

`maho-host` solves this the way Parsec does — a LocalSystem service supervises the
active console session and keeps one SYSTEM worker alive on whichever desktop
currently owns input.

## Install

```powershell
maho-host.exe --install-service      # registers MahoRDHost (LocalSystem, auto-start) and starts it
maho-host.exe --uninstall-service    # stops, removes, and releases 19730/19731
sc query MahoRDHost                  # expect: STATE : 4  RUNNING
```

The SCM runs `maho-host.exe --service-run`. The supervisor spawns
`maho-host.exe --session-worker` itself — **do not launch the worker by hand while
the service is running**, or it will hold TCP 19730 and the service's own worker
will fail to bind and respawn in a loop.

## How it works

| Component | Session | Role |
|---|---|---|
| `--service-run` | 0 | SCM dispatcher; supervises the console session |
| `--session-worker` | 1 (console) | Captures, encodes, streams, injects input |

Each second the supervisor reads the console session id
(`WTSGetActiveConsoleSessionId`) plus the current input desktop, and reconciles
them against the running worker: spawn when none exists, respawn when the session
or desktop changed, stop when no console session is attached. Workers are created
with `OpenProcessToken` → `DuplicateTokenEx(TokenPrimary)` →
`SetTokenInformation(TokenSessionId)` → `CreateProcessAsUserW`, with `lpDesktop`
set to `WinSta0\Winlogon` or `WinSta0\Default`.

### Why the worker reports the desktop

`OpenInputDesktop` is **session-local**: called from the service in session 0 it
always fails, so it cannot distinguish a locked console from an active one. WTS
lock flags (`WTSQuerySessionInformationW`) were measured and never change on a
console session under autologon, so they are useless here too.

The worker runs *inside* the console session and can see the input desktop, so it
publishes the name to `%ProgramData%\MahoRD\input-desktop.txt` every 500 ms. When
the console switches to the secure desktop the worker's own `OpenInputDesktop`
starts failing — that failure **is** the signal, and it reports `Winlogon` so the
supervisor respawns a worker that has access.

## Pairing store

Service mode uses `%ProgramData%\MahoRD\host-authorizations.json`, not the per-user
store. A LocalSystem process resolves the per-user data directory to
`C:\Windows\System32\config\systemprofile`, which would force already-paired
clients back through PIN bootstrap. Migrate existing records once:

```powershell
Copy-Item "$env:APPDATA\MahoRD\host-authorizations.json" "$env:ProgramData\MahoRD\" -Force
Copy-Item "$env:APPDATA\MahoRD\pairing-keys.json"        "$env:ProgramData\MahoRD\" -Force
```

That file holds the pre-shared keys a paired client authenticates with, and
anything under `%ProgramData%` inherits a read grant for `BUILTIN\Users`. The
service therefore re-applies an ACL of SYSTEM and Administrators only, with
inheritance severed, to both the directory and the file on every start. Audit it
with:

```powershell
(Get-Acl $env:ProgramData\MahoRD\host-authorizations.json).Access
# expect NT AUTHORITY\SYSTEM and BUILTIN\Administrators only
```

## Migrating from the scheduled-task host

Earlier deployments ran `maho-host.exe` from a Windows scheduled task with a logon
trigger (`erd-host-run`). That task and the service both bind TCP 19730, and the
`AUTO_START` service wins the race on boot, leaving the task's host to fail. Disable
the task once the service is installed:

```powershell
Disable-ScheduledTask -TaskName erd-host-run
```

The service replaces it entirely: it starts before logon, survives logoff, and
follows the console session on its own.

## Crash recovery

`--install-service` registers an SCM restart policy: three attempts at 1s, 5s and
15s, with the failure count clearing after a quiet day. Without it the SCM default
is *take no action*, and a crash leaves the host stopped until someone logs in and
starts it by hand — the one state this service exists to avoid. Confirm it with:

```powershell
sc qfailure MahoRDHost   # expect RESET_PERIOD 86400 and three RESTART actions
```

## Ctrl+Alt+Del (SAS) policy side effect

An owner's Ctrl+Alt+Del request is sent with `SendSAS` from the Session 0
service, which Windows allows only when the machine policy
`HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System\SoftwareSASGeneration`
is `1` (services) or `3` (services and Ease of Access). If the value does not
exist, the first SAS request **creates it as `1`** (services only) and logs
`SAS: created absent SoftwareSASGeneration=1 (services only)` to `service.log`.
The value stays after the session ends and after `--uninstall-service`. An
existing value is never changed: `0` or `2` makes SAS requests fail instead.
If you manage this policy with Group Policy ("Disable or enable software Secure
Attention Sequence"), set it explicitly; to undo the host's write, delete the
value:

```powershell
Reg Delete HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System /v SoftwareSASGeneration /f
```

## Troubleshooting

An SCM-hosted process has no console, so diagnostics go to
`%ProgramData%\MahoRD\service.log`:

```
tick console=ConsoleState { session_id: Some(1), desktop: Default } worker=Some(...) action=Idle
spawned worker pid=9996
```

| Symptom | Cause |
|---|---|
| Worker respawns every second | Worker is dying at startup. Check whether something else already holds 19730, or whether it was spawned onto a desktop DXGI cannot capture. |
| `action=Spawn` repeating with a changing pid | Same as above; the supervisor is healthy, the worker is not. |
| Client falls back to PIN | Service is reading the per-user store; migrate the pairing files above. |
| No listener on 19730 | No worker is running; read `service.log` for the last `action=`. |

## Verified

On `DESKTOP-1LAPJMP` (Windows 11, rustc 1.97):

- `cargo build --release -p maho-host` exit 0; `sc query MahoRDHost` → `STATE : 4 RUNNING`.
- Service in session 0, exactly one worker in session 1, stable across 10+ supervision ticks.
- Uninstall removes the service, terminates the worker and releases both ports with no orphans; reinstall restores the full pipeline.
- `maho-client` decodes 5/5 frames at 3840×1600 H.264, `packet_loss_ratio 0.0`, reconnecting PIN-lessly through the machine-wide store.
- Desktop transition: writing `Winlogon` into the hint file produces
  `action=Respawn { session_id: 1, desktop: Winlogon }`, the worker pid changes
  (confirming `CreateProcessAsUserW` onto the secure desktop ran), and the loop
  self-heals back to `Default` when the worker republishes.
- **Secure desktop, end to end:** with UAC set to prompt on the secure desktop
  (`EnableLUA=1`, `PromptOnSecureDesktop=1`, `ConsentPromptBehaviorAdmin=2`), an
  elevation prompt moved the console to `WinSta0\Winlogon`. The worker published
  `HINT=Winlogon`, the supervisor held a worker bound to that desktop
  (`desktop: Winlogon ... action=Idle`), and `maho-client` decoded 5/5 frames with
  `packet_loss_ratio 0.0` while the consent desktop was on screen.

A host configured to elevate without prompting (`PromptOnSecureDesktop=0`) never
switches to the secure desktop for elevation, so exercise this path there with the
lock screen or a logon screen instead.

### Triggering a secure desktop remotely

On a console running under autologon with no physically attached display, remote
attempts to reach the lock screen fail: `LockWorkStation` invoked from session 0
returns `False` with `GetLastError` 5 (`ERROR_ACCESS_DENIED`), and a session-1
`schtasks /it` task reports `LastTaskResult=0` while its payload never runs. The
reliable remote channel is a UAC elevation prompt with `PromptOnSecureDesktop=1`
and `ConsentPromptBehaviorAdmin=2`; restore the original values afterwards.

When a probe claims an API succeeded yet nothing happened, log both the return
value and `GetLastError` before believing it — a PowerShell parse error in the
probe (double quotes inside a double-quoted `Add-Type` signature) can stop the
call from ever being made.

## Recovering from a worker respawn

When the console switches desktop the supervisor replaces the session worker,
which tears down the control channel and media session underneath a connected
client. The client handles this: `should_reconnect` treats worker-loss failures as
retryable and the agent backend re-handshakes with the stored pairing, retrying the
event once.

Proven on the host by killing the session-1 worker outright (PID 1404 -> 1920, new
listener on 19730). Before the fix that returned `{"error":"session is not ready"}`;
now input returns `{events_sent:1, ok:true}` and stays working.

**Not yet verified:** the pre-logon logonUI screen specifically, which requires a
logoff or reboot on the host.
