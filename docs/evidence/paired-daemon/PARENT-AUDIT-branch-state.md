# Parent audit: isolated branch state (continuation 01a097f8)

Collected by the parent with its own git calls, not from any child claim.

## Repository facts

- Worktree: `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`
- Branch: `herdr-resume-01a097f8`, HEAD `e1a00339`, **not** an ancestor of `origin/main`
- `git status` fails repo-wide with
  `error: expected submodule path 'src-tauri/vendor/ghostty' not to be a symbolic link`
  because the isolated tree symlinks the vendored Ghostty checkout. Path-scoped
  plumbing (`git diff --name-only HEAD -- <paths>`, `git ls-files --others`)
  works and was used instead.

## Change surface

- 50 modified tracked source files under `src-tauri/src`, `src-tauri/tests`,
  `ui/src`, `scripts`.
- 78 new untracked source files, including the whole `src-tauri/src/paired_host/`
  module, `src-tauri/src/remote/machine_*`, `workspace_api/`, `terminal_wire.rs`,
  the `machine_*` / `a10_*` integration tests, and the UI
  `pairedDaemonProject` / `pairedHostInventory` modules plus their tests.

## Commits present without explicit authorization

`origin/main..HEAD` contains **10 commits**, all dated 2026-09-12 and authored by
the local `indo` identity (child agents ran with the repo's git identity):

```
e1a00339 feat(daemon): persist project catalog across isolated restarts
fe9011d5 refactor(daemon): share headless workspace and session authority
a2534ff4 fix(remote): require Control permission for terminal resize
4de9d285 fix(remote): retain real Ghostty VT in headless builds
0ce84a11 feat(remote): add owner-issued machine pairing grants
cd16c90e feat(remote): define paired daemon wire and persistence contracts
f9042c8d test(daemon): isolate handover lifecycle and order owned cleanup
a8c6d1ec test(ui): guard recursive jsdom top-layer selectors
921fa4e0 test(daemon): isolate persistence contract processes and state
9553dc6a test(remote): add host-isolated paired daemon fixtures
```

These exist on the isolated continuation branch only. Nothing was pushed, no
release or deployment ran, and `main` is untouched. The parent did **not** and
will not rewrite or drop them: history rewriting is a destructive git operation
and is out of scope without explicit user authorization. Reported as a deviation
from the "no commits without authorization" constraint so the user can decide.

## Scope audit of the diff (parent-read, both directions)

Checked every changed file that looked unrelated to the plan. No out-of-scope
drift was found:

- `src-tauri/src/clipboard_image.rs` (+8/-4): replaces
  `crate::ipc::native_terminal::CF_DIB_ID/CF_DIBV5_ID` with local Win32 constants
  so the file compiles without the native renderer. Required by the headless
  `ferryx-cli` / `ferryx-relay` builds this plan depends on. In scope.
- `ui/src/components/NativeTerminalPane.tsx` (+19): threads a `bindingKey`
  (daemon identity) through the attach lifecycle and forces a reattach when the
  same backend session id appears under a different daemon. The lifecycle queue
  deduplicates by backend id only, which would alias sessions across machines -
  exactly the AC03/AC04 identity separation this plan requires. In scope.
- `ui/src/components/settings/RemoteAccessSection.tsx` (+1/-1): description copy
  explaining when re-pairing is needed. Documents new pairing-durability
  behavior. In scope.
- `ui/src/components/settings/SshSection.tsx` (+1/-1): `text-emerald-500` ->
  `text-status-success`. NOT a drive-by refactor - it is repo-enforced
  compliance: `ui/src/appearanceThemeContract.test.ts:71` asserts the settings
  source must not contain `text-emerald-500`, and `text-status-success` is the
  token used across 9 files.
- `src-tauri/src/worktree/git.rs` (+546) with the new `worktree/git/{drain,unix,
  windows}.rs` modules: remote worktree operations executed on the owning daemon
  (AC05) split behind explicit platform modules, matching the repository's
  cross-platform convention. In scope.

## In-flight at the time of this audit

- `st_01a09922` (A10 saturated-socket controller isolation) still running.
- DAG `dag_eee987ec` node `native-integration` (`st_01a09938`) still running;
  `aggregate-verifier` (`st_01a0992f`) pending behind it. `native-client` and
  `ui-adapter` completed and were retained across the generation-2 amendment.
- Backend aggregate rerun is deliberately paused: the parent's last attempt
  failed only on `PairedHostOperation` non-exhaustive matches in
  `daemon/server.rs` and `daemon/client.rs` while the integration child is
  mid-composition (`A12-session-metadata-parent-backend-repaired.log`).
