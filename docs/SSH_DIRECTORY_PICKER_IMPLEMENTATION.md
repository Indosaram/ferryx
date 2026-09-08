# SSH project directory picker: implementation and verification

Date: 2026-09-08

## Post-implementation review

The independent gate reviewer approved the corrected code delta. See
`docs/SSH_DIRECTORY_PICKER_CODE_REVIEW.md` for the findings, fixes and final receipts.

The review corrected whole-response UTF-8 decoding so one unsupported POSIX
filename does not hide valid sibling folders. It also corrected failed navigation
to display the attempted path and strengthened cache-refresh, host-replacement,
and exact parent-navigation regressions.

After those corrections: 78 related UI tests and the UI build passed; 58 SSH Rust
tests, `cargo check`, and real Windows/Linux browse-registration tests passed.
The counts below describe the earlier implementation baseline, not the final
reviewed delta.

The user authorized the prerequisite scope. SSH runtime/platform changes were
independently reviewed, fixed and verified without the picker, then committed as
`c85f17e` (`feat(ssh): support Windows and POSIX remote runtimes`). The picker can
now be committed separately on that verified foundation.

Final isolated picker candidate verification: 100 UI tests, 60 SSH Rust tests,
4 remote-project registration tests, UI build and cargo check passed. Actual
Windows and Linux browse-registration tests passed in 8.14 and 1.04 seconds.
The UI and Rust/check/live command chains both exited 0. Unrelated working-tree
changes were absent from this verification tree.

## Result

Add Project > Remote (SSH) now opens a directory picker on the selected saved host.
Users can navigate from the remote home, enter an absolute path or `~`/`~/...`,
move up, refresh, filter the loaded directory names, show hidden directories, and
register the current folder with Add this folder.

Settings > SSH Machines exposes Open Project on enabled hosts. It opens the same
picker with the host preselected. Add or manage SSH machines in the picker opens
settings; after saving a new machine, its Open Project action continues the flow.
Internal remote workspace IDs are derived rather than exposed as an editable field.

There is no file editor, file-content viewer, SSHFS mount, clone operation, or new
remote agent. File editors are excluded from future product scope as well.

## Implementation

- `src-tauri/src/ssh/browse.rs` adds the host-scoped read-only listing implementation
  and typed request/response structures. The existing saved host inventory is
  checked before connecting and again after the response; disabled, deleted, or
  changed hosts fail rather than silently targeting another machine.
- `src-tauri/src/ipc/project_remote.rs` exposes `cmd_ssh_list_directories`; the
  command is registered in `src-tauri/src/lib.rs`. It does not require an existing
  project or restart the PTY daemon.
- POSIX and Windows PowerShell use the existing SSH execution layer. Paths are
  quoted as data and interpreted by the remote platform, not the local OS.
  Immediate directory entries include hidden directories and directory symlinks.
  There are no recursive filesystem scans or remote writes.
- Nonce-prefixed NUL frames preserve spaces, quotes and UTF-8 names. Each listing
  limits entry data to 6,000 bytes and at most 1,000 entries, fitting the existing
  16 KiB SSH response cap with two maximum-length paths. Truncation or omitted
  unsupported path names is reported explicitly. A full path can still be entered.
- Detected environments are cached for 60 seconds, keyed by the complete saved
  host configuration, with a maximum of 32 entries. Directory content is not cached
  globally.
- `ui/src/lib/remoteDirectories.ts` provides the typed IPC bridge.
- `ui/src/components/RemoteDirectoryPicker.tsx` owns navigation, local filtering,
  hidden-directory display, error/retry/loading states and a picker-lifetime cache.
  Request generations ignore older responses and responses after unmount.
- `ui/src/components/ProjectDialogs.tsx` remounts the picker on host configuration
  changes, separates validated selection from path input, and reuses the existing
  authoritative host refresh and remote registration. Path edits, pending reads,
  failures and host changes invalidate the selectable path. Registration preserves
  the exact canonical path rather than trimming valid path characters.
- `SshSection.tsx`, `SettingsDialog.tsx` and `App.tsx` connect the settings action
  through typed callbacks. Local project registration and project grouping retain
  their existing contracts.

The surface contract is in `docs/SSH_DIRECTORY_PICKER_DESIGN.md`.

## Automated evidence

Observed RED before implementation:

- Backend tests failed because the directory script/parser/path functions did not
  exist.
- The existing AddProjectDialog seam could not find a Remote home action.
- SSH settings could not find Open project on the selected host.
- A later regression test showed that an initial host selection was lost while
  the host inventory was loading. The host synchronization effect now waits for
  that load to finish.

Final UI command, exit code 0:

```sh
cd ui
node node_modules/vitest/vitest.mjs run \
  src/components/ProjectDialogs.test.tsx \
  src/components/settings/SshSection.test.tsx \
  src/components/SettingsDialog.ssh.test.tsx \
  src/lib/remoteProject.test.ts \
  src/lib/projectGrouping.test.ts \
  --maxWorkers=1 --reporter=dot
bun run build
```

All 76 tests passed in the single final combined run. TypeScript and Vite build
passed. Tests cover nested browsing to canonical registration, Enter navigation
without submission, stale path/host responses, hidden/filter/cache/refresh,
failure/retry, initial host loading, authoritative host removal, late registration
responses and the existing local flow.

Final Rust commands, combined exit code 0:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib ssh -- --nocapture
cargo check --manifest-path src-tauri/Cargo.toml
git diff --check
```

All 57 SSH library tests passed. Seven directory-picker tests include actual
POSIX script execution in isolated temporary fixtures, literal names, directory
symlinks, file exclusion, empty/missing/unreadable paths, bounded listings,
Windows drive/UNC response decoding and disabled/deleted host rejection.

## Real SSH verification

`src-tauri/tests/ssh_browse_live.rs` uses the actual non-Tauri listing and
registration cores, OpenSSH transport and a temporary local host/project store.
It does not modify the remote filesystem or operate on the live PTY daemon.

```sh
FERRYX_SSH_BROWSE_HOST=maho-win cargo test \
  --manifest-path src-tauri/Cargo.toml --test ssh_browse_live \
  -- --ignored --nocapture
FERRYX_SSH_BROWSE_HOST=omarchy cargo test \
  --manifest-path src-tauri/Cargo.toml --test ssh_browse_live \
  -- --ignored --nocapture
```

- Windows `maho-win`: one test passed in 14.39 seconds.
- Linux `omarchy`: one test passed in 6.50 seconds on the final invocation.
- Both observed home listing, a real child directory, parent navigation, `~`
  resolution, project registration and resolution of the persisted identity.
- The first Linux invocation failed at the existing environment detection's
  12-second deadline, before directory listing. Separate measurements then observed
  successful POSIX shell startup in 0.74 seconds and missing Windows executors
  returning in 0.57 and 1.08 seconds. The subsequent full Linux invocation passed.
  No timeout increase or transport retry was added to production code.

The opt-in live test is excluded from ordinary test runs because it needs an
explicit trusted SSH endpoint. UNC shares and Windows-local clients were not
tested against real hosts; UNC parsing is covered by a library test.

## Browser evidence

An isolated Vite harness rendered the actual AddProjectDialog, RemoteDirectoryPicker
and SshSection components with fixture responses only at the Tauri IPC boundary.
It was not the running desktop application and did not access real remote files.
The temporary harness files and server were removed after verification.

Observed behavior:

- Clicked through `/home/developer/projects/My App` and registered exactly that
  path with advisory ID `My-App`; the returned `ssh:qa-registered` ID was adopted.
- Opened SSH settings and used the Windows host's Open Project action, which
  selected that host and displayed `C:\Users\developer`.
- Loaded, empty, hidden, truncated and permission-error states were rendered.
  Loading and errors disabled project registration.
- At 1,100px and 390px widths, document scroll width matched viewport width.
  A 390x480 short window retained its footer within the viewport: the action
  button's bottom was 447px, below the dialog's 456px bottom.

Geometry is stored in `docs/evidence/ssh-picker-geometry.json`.
Screenshots are stored under `docs/evidence/ssh-picker-*.png`, covering desktop,
mobile, hidden, error, loading, empty, truncated, settings, Windows and short-window
states. These screenshots use a 2x capture scale.

The current model could not visually read the image attachments, so this is DOM
and geometry verification, not a completed aesthetic review. The headless
WebView's key helper did not emit an observable keydown event in the final keyboard
probe. Enter behavior is covered by React event tests but requires desktop
confirmation rather than a claim of physical-keyboard E2E success.

LSP diagnostics were attempted but the local LSP daemon was unreachable.
Compiler, type checker, tests and build results provide the automated checks.

## Manual desktop confirmation

Use the debug application through exactly:

```sh
bun tauri dev
```

1. Open Add Project > Remote (SSH), choose a saved host, and confirm its home
   directories appear without entering a path.
2. Navigate into a project, move up and return, and toggle hidden folders.
3. Paste an absolute remote path, press Enter, and confirm it only navigates.
   Add this folder should register the canonical selected folder.
4. Open a terminal in the added project and confirm its working directory.
5. In Settings > SSH Machines, save a new host or select an existing enabled host,
   then use Open Project and confirm the same picker opens on that host.
6. Confirm the dialog's visual appearance, focus behavior and footer at a narrow
   window size.

No desktop UI automation, release build/installation, app replacement, or daemon
restart was performed. Changes are uncommitted in the shared working tree; other
sessions' existing SSH platform and release-pipeline changes were preserved.
