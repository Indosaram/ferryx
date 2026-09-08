# macOS local app replacement

## Scope and current status

The user explicitly requested a macOS release build and replacement of the
installed application after the terminal repair commit. This authorizes this
local release installation; it does not make the earlier debug desktop
acceptance scenarios pass.

Status: local build, signed app replacement, and process launch completed.
All 28 pre-existing session IDs remain present on the surviving legacy daemons.

## Build source and command

- Repository: `/Users/indo/code/project/orca-lite`, branch `main`.
- Terminal error recovery commit: `a8db9b3`.
- Working-tree app and Cargo version: `2026.908.1`.
- Build uses the current main working tree, including concurrent uncommitted
  work; it is not a reproducible clean-commit release.
- HEAD was `a8db9b3` during initial inspection and
  `27b160c01e6976689eefee9590cb8e90177935c4` at the later baseline capture.
  Neither hash alone identifies all working-tree build inputs.
- Scope: this Mac's app bundle, not Windows/Linux builds or public publication.

```sh
APPLE_SIGNING_IDENTITY="Developer ID Application: Indo Yoon (5DUM8WPB4C)" \
  bun tauri build --bundles app
```

Build monitor: `mon_Z4WHAWHVCBFZ9JT4`, session `bash_20`.
Frontend build completed successfully in 2.03 seconds. The optimized native
build finished in 3 minutes 46 seconds; app bundling, Developer ID signing and
updater archive signing completed with exit code zero. The executable is arm64.
No Apple notarization or public release publication was performed.

## Pre-install baseline

- Installed app: `/Applications/Ferryx.app`, version `2026.902.2`.
- GUI PID: `669`.
- Existing daemon PIDs: `845`, `57449`.
- Both daemon handshakes returned protocol version `3` and daemon version
  `2026.902.2`.
- Read-only `listSessions` returned 9 sessions from
  `/tmp/rorca-501/daemon.sock` and 19 from
  `/tmp/rorca-501/legacy-845-1788877044782.sock`.
- The complete 28-session inventory is retained in
  `evidence/macos-replacement-before-sessions.json`.
- Existing binary SHA-256:
  `aafde0d57d54a426790bd1507e7810067ac6cdbe395141b0696252826ad96dfe`.
- Existing signature: Developer ID Application, Team ID `5DUM8WPB4C`,
  identifier `com.ferryx.app`.

The designated requirement includes the Apple generic anchor, Developer ID
certificate constraints, and leaf organizational unit `5DUM8WPB4C`.
The replacement must preserve this identity.

## Installation gates

1. Require build exit code zero and strict signature verification.
2. Back up the installed app by moving it, preserving the executable inode
   still used by live daemons.
3. Never terminate either daemon or any PTY child process.
4. Install the complete signed app bundle, not only its executable.
5. Verify the installed version, signature, binary hash and preserved session
   inventory.
6. Record the backup path and exact installation outcome here.

## Completed installation and preservation checks

- Prepared a complete bundle with `ditto` at
  `/Applications/Ferryx-install-20260909-0523.app` and verified its signature.
- Sent `SIGTERM` only to GUI PID `669`. Monitor `bash_21` reported
  `GUI_STOPPED_DAEMONS_PRESERVED`, exit zero, after confirming PIDs `845` and
  `57449` survived.
- Moved the old app to `/Applications/Ferryx.app.bak-20260909-0523`.
  This backup remains intentionally preserved for the live old executables.
- Moved the verified staging bundle to `/Applications/Ferryx.app`. No staging
  bundle remains.
- Installed app version: `2026.908.1`.
- Installed binary SHA-256, identical to the verified build:
  `8967dd9d17ce053a3ecd1ebdbcf54586d4101064c11165f6da5211a7d3c99950`.
- `codesign --verify --deep --strict` passed on both staged and installed apps.
  Developer ID identity, Team ID and designated requirement match the old app.
- `open /Applications/Ferryx.app` launched new GUI PID `35996`;
  monitor `bash_22` reported `NEW_GUI_PROCESS_STARTED`, exit zero.
  This proves process launch, not visual terminal behavior.
- The GUI's version mismatch triggered its built-in rolling upgrade. The new
  canonical daemon is PID `36119`, version `2026.908.1`, epoch `1788899080133`.
  No daemon was manually signalled or terminated.
- Old PID `845` remains at
  `/tmp/rorca-501/legacy-845-1788877044782.sock`, epoch `1788875291502`,
  with all 19 original session IDs.
- Old PID `57449` moved to
  `/tmp/rorca-501/legacy-57449-1788899079459.sock`, epoch `1788877224715`,
  with all 9 original session IDs.
- Exact set comparison across the new canonical and both legacy inventories:
  28 original IDs preserved, zero missing. The new canonical daemon also
  reported 3 new IDs. Session survival does not prove the GUI reattached every
  session or preserved the complete visible layout.
- Build, GUI shutdown and launch monitors have exited. The backup is retained,
  not a disposable temporary directory. The earlier permission subscription
  remains separate from this completed installation.

## Separate unresolved desktop acceptance

Screen Recording and Accessibility were both denied in the permission query
at `2026-09-08T20:17:07Z`. No actual terminal input, resize, or overlay screenshot
has been obtained. Installation checks must not be reported as those missing
desktop behavior checks.
