# A18 - Add Project and generic paired directory picker

## Delivered

- Add Project now offers Local, SSH, and Paired Daemon. A separate `PairedDaemonProjectForm.tsx` keeps the new add-only flow away from concurrent worktree/removal changes in ProjectDialogs.
- Paired selection reads the native remote-host inventory, captures host ID/generation, negotiates directoryBrowseV1 and machineWorkspaceV1, and uses the existing validated paired adapter exclusively. No SSH credentials, local directory picker, desktop mirror, or terminal proxy is involved.
- The picker accepts a generic generation-keyed directory source. Cache keys include owner, path and hidden mode; stale results cannot update selection after source changes/unmount. Hidden folders are requested remotely. SSH retains its existing request shape and immediate client-side hidden toggle.
- A host/generation/availability change remounts the paired selection, clearing its canonical path and negotiated adapter. Registration uses a UUID request ID and the daemon-returned project identity without deriving a local workspace ID. The adapter revalidates native context/generation and rejects malformed or cross-host results.
- Empty, unpaired/mirror-only, offline, incompatible, native-unavailable and feature-disabled states have explicit pairing/upgrade/access guidance. The feature flag is honored when explicitly false; an absent flag retains existing availability semantics. Pairing itself stays in Settings > Remote Access.

## Evidence

- `A18-RED.log`: written before implementation, seven failing tests for the missing Paired Daemon choice.
- `A18-GREEN.log`: latest targeted run passes 86 tests across ProjectDialogs.pairedDaemon, existing ProjectDialogs, RemoteDirectoryPicker and pairedDaemonProject adapter suites, in one run under Node v22.22.3 with the required PATH.
- Tests exercise paired home and non-Latin/spaced folders, deferred stale host responses, hidden requests, canonical registration, malformed/cross-host rejection, generation invalidation, unavailable states, return to location choices, generic-source keyboard/IME behavior, and existing SSH host revalidation. Async work uses explicit promise releases and React act, with no sleeps or polling added.
- `A18-SSH-regression.log` preserves an intermediate regression: reloading on the SSH hidden toggle hid its immediate results. The implementation was corrected; existing tests were not weakened.
- Initial LSP diagnostics were clean on all five then-changed implementation/test files; the added generic picker test also has clean diagnostics. Later refreshes for the picker and paired test timed out; full TypeScript build remains the authoritative final validator.
- UI build attempts are preserved in GREEN, including concurrent-packet failures (first missing PairedMachinesSection, then Sidebar's missing pairedConnectionStatus, then pairedProjectWorktrees.ts:17 string | undefined). No changes were made to those other packets.

## Not proved / human QA

No running desktop, user daemon, PTY, relay machine, or real HOME was accessed. Native command responses were mocked at the Tauri boundary while using the real paired adapter and contract decoders. Thus these tests do not establish live relay connectivity, actual remote directory permissions, or remote persistence. They establish the renderer-side AC01 workflow on the existing backend, not live end-to-end AC01.

Human QA must open Add Project in the desktop, select an online paired daemon, browse home and another permitted directory without SSH, toggle hidden folders, and register the returned canonical project. Confirm keyboard-only navigation and actual OS IME composition for a folder containing spaces/non-Latin characters; verify focus, visual layout, and return to Local/SSH including the native Local folder chooser. Confirm empty/unpaired/offline/upgrade messages with real inventory. No remote terminal operation was attempted or enabled.

No commits, release builds, deployments, desktop automation, daemon/PTY operations, or edits to forbidden files. Evidence and source files are intentional deliverables; no test server or fixture process was created.
