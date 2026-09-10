# macOS TCC permissions: why a granted permission stops working, and how to restore it without killing sessions

Verified on macOS 26.6 (25G72), 2026-09-10. Every claim below is backed by `log show`
(`subsystem == "com.apple.TCC"`), `codesign`, `lsof`, and live filesystem probes from a shell
whose ancestry is `ferryx --daemon`.

Prior incident in the same family:
[`MACOS_TCC_DOCUMENTS_ACCESS_ROOT_CAUSE_AND_FIX_2026-09-04.md`](./MACOS_TCC_DOCUMENTS_ACCESS_ROOT_CAUSE_AND_FIX_2026-09-04.md)
— an agent inside a Ferryx terminal ran `find /` for 90 minutes and tripped
`kTCCServiceSystemPolicyDocumentsFolder`, attributed to `com.ferryx.app`.

## Observation (NOT the root cause): daemons run from deleted bundle inodes

**Corrected after resolution.** This is real and worth knowing, but it did NOT block
authorization. Once the Full Disk Access switch was turned on, a shell hosted by the *orphaned*
daemon 65513 (executing from a deleted `.bak` path) read `~/Library/Safari`, `~/Library/Mail`,
`~/Library/Containers/com.apple.Notes` and `~/Library/Application Support/com.apple.TCC`
successfully. tccd still resolves the responsible identity to `com.ferryx.app` even when the
executable path is gone.

`lsof -p <pid> -a -d txt` on the live daemon chain:

```
pid 827   -> /private/var/folders/.../T/tauri_current_appFbvkLF/current_app/Contents/MacOS/ferryx
pid 26697 -> /Applications/Ferryx.app.bak-20260909225357/Contents/MacOS/ferryx
pid 65513 -> /Applications/Ferryx.app.bak-20260910065116/Contents/MacOS/ferryx
pid 28594 -> /Applications/Ferryx.app/Contents/MacOS/ferryx        (GUI, correct)
```

`ls -d /Applications/Ferryx.app*` lists only `/Applications/Ferryx.app`. Every `.bak-*` bundle
has been deleted; the daemons keep executing from unlinked inodes. The current on-disk binary
is inode `2319142292`, while the daemons hold `2316450152`, `2317641875`, `2318484050`.

TCC keys the Full Disk Access row to the app at `/Applications/Ferryx.app`. When a daemon
child touches a protected path, tccd resolves the *running process's* code identity from its
executable vnode — a path that no longer exists — and therefore never maps it to the granted
client. The symptom in the log is a repeated

```
spooky magic for /Applications/Ferryx.app/Contents/MacOS/ferryx (0xb4000593) at text offset: 81920
```

emitted on every attribution attempt. FDA is denied silently and
`kTCCServiceSystemPolicyAppData` prompts fire instead.

This is *not* a stale-cache problem: tccd is queried live at the moment of access
(`AUTHREQ_CTX ... service=kTCCServiceSystemPolicyAllFiles, preflight=no` logged at the exact
probe timestamp). It is an identity-resolution failure.

### The real damage: a daemon that cannot upgrade itself

This costs nothing in TCC terms, but it breaks self-upgrade and can strand every session.
`handle_upgrade_binary` defaulted `target_exe` to `std::env::current_exe()`, which for an
orphaned daemon is the deleted `.bak` path. Two failure modes followed:

- With no live sessions, `perform_daemon_exec_with_path` execs a path that does not exist. The
  upgrade silently fails and the daemon keeps running the old image.
- With live sessions, `prepare_handover` has *already* moved the canonical listener to a legacy
  socket by the time `Command::new(deleted).spawn()` fails. The old code then returned from the
  task, abandoning the legacy accept loop too, so neither socket had a listener and no client
  could reach any live session.

**Fixed in `src-tauri/src/daemon/server.rs`:**

- New `resolve_upgrade_target_exe` resolves the explicit `newBinaryPath` first, then
  `current_exe()`, and accepts either only when the file exists right now. When nothing
  resolves, `handle_upgrade_binary` returns a structured `Error` *before* touching the
  listener, so the socket topology is never dismantled for an upgrade that cannot happen.
- `spawn_legacy_handover_daemon` no longer returns when the successor fails to spawn; it keeps
  serving the legacy peer so live sessions stay reachable.
- Unit test `resolve_upgrade_target_exe_only_resolves_paths_that_exist` covers all three cases,
  including the orphaned-daemon case that must resolve to `None`.

Callers should still pass an explicit `newBinaryPath` pointing at the canonical bundle; the
resolver is the safety net, not a substitute.

## Three distinct failure modes, confirmed separately

1. **Screen Recording row was csreq-poisoned.** 34 log lines of
   `Failed to match existing code requirement for subject com.ferryx.app and service
   kTCCServiceScreenCapture`. The toggle read ON in System Settings and authorized nothing.
   This is the residue of past ad-hoc signing, whose designated requirement pins an ephemeral
   cdhash that no Developer ID rebuild can satisfy.
2. **Full Disk Access was simply switched off.** RESOLVED by reading the row directly. FDA rows
   live in the SYSTEM database `/Library/Application Support/com.apple.TCC/TCC.db`, not the
   per-user one, and reading it requires the *reading* process to hold FDA (sudo is not enough,
   because TCC evaluates the responsible process). From a Terminal.app that was granted FDA:

   ```
   service=kTCCServiceSystemPolicyAllFiles  client=com.ferryx.app  client_type=0
   auth_value=0  auth_reason=5  last_modified=1789052137  length(csreq)=156
   ```

   The stored requirement decodes to a healthy Developer ID requirement with no cdhash clause:

   ```
   identifier "com.ferryx.app" and anchor apple generic
     and certificate 1[field.1.2.840.113635.100.6.2.6]
     and certificate leaf[field.1.2.840.113635.100.6.1.13]
     and certificate leaf[subject.OU] = "5DUM8WPB4C"
   ```

   and `codesign --verify --strict -R="$REQ" /Applications/Ferryx.app` reports
   **explicit requirement satisfied**. Nothing was poisoned or mismatched: the row existed with a
   valid requirement and `auth_value=0`, i.e. the switch was off. The Screen Recording pane, which
   *was* enabled, is a different service and was mistaken for the FDA pane.

   Useful invariants learned here: a Settings toggle writes `auth_reason=3` (USER_SET) and a
   prompt answer writes `auth_reason=2` (USER_CONSENT); `auth_reason=5` (SERVICE_POLICY) with
   `auth_value=0` is the default-deny path and does NOT prove the row is missing. The enum is
   verifiable locally in `$(xcrun --sdk macosx --show-sdk-path)/usr/include/EndpointSecurity/ESTypes.h`.
3. **Attribution noise for orphaned daemons - cosmetic, not blocking.** tccd emits
   `Failed to fetch responsible file descriptor: [2: No such file or directory]` when resolving
   a responsible process whose executable path no longer exists. Verified after the fix: sessions
   hosted by the orphaned daemons 827 / 26697 / 65513 are authorized normally anyway. Neither a
   cold restart nor the rolling handover was required to restore access.

Incidental finding from the same trace: a recurring `kTCCServiceDeveloperTool` request every
~40s, `responsible=com.ferryx.app`, `accessing=magick-555549445ba3dd8685753c0184bc7a034af825ad`
(`/opt/homebrew/.../bin/magick`), answered with `Service kTCCServiceDeveloperTool does not allow
prompting; returning denied`. An agent in a Ferryx terminal is invoking an ad-hoc signed
Homebrew ImageMagick, and the denial is attributed to Ferryx.

## Clearing the dead rows

```
tccutil reset ScreenCapture        com.ferryx.app
tccutil reset SystemPolicyAllFiles com.ferryx.app
tccutil reset SystemPolicyAppData  com.ferryx.app
```

All three reported success on 2026-09-10. Note that `tccutil reset` does not necessarily delete
the row: afterwards the AllFiles row still existed with `auth_value=0` and `last_modified` set to
the reset time, so "reset" left an explicit deny behind. Verify with SQL rather than trusting the
"Successfully reset" string.

Re-granting mints the requirement against the current Developer ID signature
(`identifier com.ferryx.app` + cert leaf), which survives rebuilds and bundle replacement. Never
let an ad-hoc signed build run again, or cdhash pinning returns.

## Restoring the grant with zero session loss

Never `kill`/`SIGTERM` the daemon: it is the sole owner of every PTY master fd, so its death is
unconditionally fatal to all sessions. Use the daemon's own rolling handover over the UDS
control socket (newline-delimited JSON, `DAEMON_PROTOCOL_VERSION = 3`):

```
{"type":"handshake","version":3}
{"type":"upgradeBinary","newBinaryPath":"/Applications/Ferryx.app/Contents/MacOS/ferryx"}
```

An explicit `newBinaryPath` skips the version/mtime guard. With live sessions present the
handler takes the `prepare_handover` + `spawn_legacy_handover_daemon` branch: the current
daemon rebinds to `/tmp/rorca-<uid>/legacy-<pid>-<epoch>.sock` and keeps serving its existing
PTYs, while a **new** daemon process binds the canonical `daemon.sock`.

Executed 2026-09-10 23:19 with these results:

```
before: 827(15 children) -> 26697(7) -> 65513(8, canonical)
after : 36170 --handover-from /tmp/rorca-501/legacy-65513-1789049955335.sock
        pid 36170 -> /Applications/Ferryx.app/Contents/MacOS/ferryx   (correct inode)
        65513 still alive, 9 children, serving legacy-65513-...sock
        handshake on daemon.sock -> {"pid":36170,"binaryMtimeMs":1789023076847}
```

Zero processes were killed. Sessions still hosted by legacy peers keep the old, unresolvable
identity and age out naturally; only sessions created after the handover gain the grant.

Verification from a **new** terminal (one created after the handover):

```
ls ~/Library/Safari >/dev/null 2>&1 && echo FDA_OK || echo FDA_DENIED
```

Optional cleanup once FDA is effective:
`tccutil reset SystemPolicyAppData com.ferryx.app`.

## Verified end state (2026-09-11 07:40)

System database `/Library/Application Support/com.apple.TCC/TCC.db`:

```
service                          ct  av  ar  modified
kTCCServiceAccessibility         0   2   4   2026-09-07 13:11:10
kTCCServiceSystemPolicyAllFiles  0   2   4   2026-09-11 07:40:39
```

`av=2` is allowed. Note `ar=4` (SYSTEM_SET) rather than 3 (USER_SET) — on 26.6 the Privacy pane
recorded this grant as system-set. All four FDA probes pass, including the canonical
`~/Library/Application Support/com.apple.TCC` read, from a shell under an orphaned daemon.

All four daemons (827, 26697, 65513, 36170) stayed alive through the entire investigation and no
PTY session was lost.

## Why the dialog says "Ferryx"

The recurring prompt is `kTCCServiceSystemPolicyAppData`. In 8 hours of logs it fired 12 times;
the *accessing* binaries were `/usr/bin/find` (x8), `katok` (x3), `omo-gateway` (x1), each with
`responsible_path=/Applications/Ferryx.app/Contents/MacOS/ferryx`. Ferryx has no `find` call
site; macOS attributes a child's protected-resource access to the responsible process, which
for anything started in a Ferryx terminal is Ferryx. Prompts do not converge because each
distinct target app container creates its own record.

## Signing: stable vs churning identities

Developer ID signed, identity stable across rebuilds (requirement anchors to
`identifier + certificate leaf`):

- `/Applications/Ferryx.app` and `src-tauri/target/debug/Ferryx.app` —
  `com.ferryx.app`, `Developer ID Application: Indo Yoon (5DUM8WPB4C)`, hardened runtime.

Ad-hoc / linker-signed, identity bound to the cdhash so every rebuild looks like a new app
(new prompt, new row, old toggle becomes a dead entry):

- `omo-gateway` -> `omo_gateway-bc057576fec7be80`
- `katok` -> `katok-3abb8e60ba4b8519`
- `opencode` -> `a.out`
- `python3.11` -> `-`

This is the mechanism behind the cluttered Screen & System Audio Recording list, and it has its
own log signature:

```
Failed to match existing code requirement for subject
  /Users/indo/code/project/omo-bridge/target/debug/gpt2omo
  and service kTCCServiceSystemPolicyAllFiles
```

Fix for first-party helpers — sign with Developer ID and pin a stable identifier:

```
codesign -f -s "Developer ID Application: Indo Yoon (5DUM8WPB4C)" \
  -i com.ferryx.omo-gateway --options runtime --timestamp <binary>
```

Entitlements are irrelevant to FDA. Both bundles ship hardened runtime with **no entitlements
plist**, which only blocks services that require a declaring entitlement — tccd refuses
`kTCCServicePhotos`, `kTCCServiceAddressBook`, and `kTCCServiceAppleEvents` for
`com.ferryx.app` for exactly this reason.

## Rejected: moving responsibility to the child

`responsibility_spawnattrs_setdisclaim` (used by LLDB and Chromium, proposed for Ghostty in
ghostty#9263) can break the attribution chain at spawn so a child becomes its own responsible
process. Rejected here:

- Ferryx only spawns the login shell, so blame would move to `/bin/zsh`, and granting a
  permission to `/bin/zsh` grants it to every shell on the system.
- Disclaiming discards inheritance, so each tool needs its own grant; most are ad-hoc signed
  and would re-prompt per rebuild — one honest dialog becomes N.
- `portable-pty` 0.9 spawns via `std::process::Command` + `pre_exec` (needed for `setsid` and
  `TIOCSCTTY`), which forces the fork/exec path and leaves no `posix_spawnattr` to carry the
  flag. It would require a raw `posix_spawn` PTY path or a signed launch helper.
