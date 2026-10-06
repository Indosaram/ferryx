# GitHub issue intake in Add Worktree — UI evidence (2026-10-06)

Scope: optional GitHub issue intake in `AddWorktreeDialog` (`ui/src/components/ProjectDialogs.tsx`),
backed by `cmd_github_issue_preview` (`src-tauri/src/ipc/github_issue.rs`).

## How the captures were produced

The dialog was rendered by the real React component in a real browser engine against a stubbed
Tauri IPC bridge — no hand-built mock markup:

```bash
bunx vite --port 5199            # from ui/
# harness entry: ui/issue-intake-qa.html -> ui/src/qa/issueIntakeHarness.tsx
```

The harness stubs only `window.__TAURI_INTERNALS__.invoke` (`cmd_project_branches`,
`cmd_github_issue_preview`, `cmd_ssh_list_hosts`). Screenshots were taken with the headless
browser at 900px width.

## Captures

| File | Scenario | Asserted |
|---|---|---|
| `issue-preview-desktop.png` | Issue `#12` loaded | Title, URL and body render as inert text (zero `<a>` elements), the slug is pre-filled with `issue-12-native-pane-drag-leaves`, the base-branch select is retained at `main (current)`, and the confirm button reads `Create Worktree from Issue #12` |
| `clipboard-failure-desktop.png` | Clipboard write denied | `Could not copy the issue context. Select the preview text and copy it manually.` is shown, the preview is retained, and the create flow is untouched |
| `intake-error-desktop.png` | `gh` missing (`CLI_EXECUTABLE_NOT_FOUND`) | The structured message is shown, no preview is rendered, and the confirm button falls back to the plain `Create Worktree` label |

Observed values at capture time:

```
preview   : {"preview":true,"slug":"issue-12-native-pane-drag-leaves","links":0,
             "buttons":["","Load issue","Copy issue context","Cancel","Create Worktree from Issue #12"]}
clipboard : {"alerts":["Could not copy the issue context. Select the preview text and copy it manually."],
             "createLabel":"Create Worktree from Issue #12","preview":true}
error     : {"alerts":["The GitHub CLI (gh) was not found on PATH. Install it and run `gh auth login`."],
             "createLabel":"Create Worktree","preview":false}
```

## Behavioural coverage

- Backend fixtures: `cargo test --manifest-path src-tauri/Cargo.toml --lib github_issue` — origin and
  URL validation, wrong-repository input, missing `gh`, auth failure, timeout, malformed and
  oversized responses, and argument-injection-shaped references.
- UI: `bun run --cwd ui test` — `ui/src/components/ProjectDialogs.test.tsx` covers preview, slug
  editing, cancellation, duplicate submission, creation failure, successful existing-pane handoff
  and clipboard failure; `ui/src/lib/tauri.test.ts` covers the bridge request shape.

## Pre-existing backend breakage (not introduced here)

The tree this slice lands on does not compile: `af95b9a7` records two accepted errors from
`a15fdcf9`, where `daemon/machine_owner.rs` calls `DaemonSessionService::project_desktop_gui_session`
and sets `SpawnRequestFingerprint::requested_session_id`, neither of which exists on any branch.
Nothing in this slice touches those files or the daemon. The backend fixtures above were therefore
executed against a copy of this tree in which only those two references are neutralized by
`preexisting-daemon-breakage.patch`, so the module under test is exactly the one committed here:

```bash
# The scratch tree only needs src-tauri/src to be current; its target/ and ui/dist are reused so
# the fixtures do not force a full dependency rebuild.
rsync -a --delete src-tauri/src/ /tmp/ferryx-verify/src-tauri/src/
patch -p1 -d /tmp/ferryx-verify --forward \
  < docs/evidence/github-issue-intake-20261006/preexisting-daemon-breakage.patch
cargo test --manifest-path /tmp/ferryx-verify/src-tauri/Cargo.toml --lib github_issue
```

Run it on a fresh scratch tree first (copy `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`,
`src-tauri/build.rs`, `ui/dist` and a `ui/node_modules` link alongside `src-tauri/src/`); the
`rsync --delete` above restores the unstubbed file, so the patch re-applies cleanly on every run.

The patch applies cleanly to this tree with both `git apply --check` and `patch -p1 --forward`; it
touches no file the slice modifies.

## Pre-existing UI failures (not introduced here)

The full UI suite is not green on this tree either, and the slice adds no failure to it. A pristine
`git archive HEAD` copy of the tree (no uncommitted changes) was compared against this working tree
over the failing files:

| Tree | Files failing | Tests failing |
|---|---|---|
| pristine `HEAD` (`git archive HEAD`) | 12 | 214 |
| this working tree | 12 | 215 |

The same 12 files fail in both (`NativeTerminalPane`, `App`, `App.remote`, `App.pairedDaemon`,
`sshActivity.integration`, `TerminalSearchOverlay`, `activityRenderChain`, `workspaceNativeActivity`,
`TerminalSplitView.paneHandleReach`, `workspaceActivity`, `updater`, `pairedDaemonRollout`), and
`src/state/workspaceNativeActivity.test.tsx` is flaky in both trees (3-5 of its 14 tests fail per run,
with a different fourth test each time), which accounts for the single-test difference. Every file
this slice touches passes: `bun run --cwd ui test src/components/ProjectDialogs.test.tsx
src/lib/tauri.test.ts` is 98/98, and `bun run --cwd ui build` (the CI gate) succeeds.
