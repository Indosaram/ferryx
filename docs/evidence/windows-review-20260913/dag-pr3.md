# DAG PR3 review and focused-close runtime handoff

Task st_01a099f7; original session 01a0983f-c995-753d-afa9-593f6d118788.
Observed 2026-09-13, final GitHub refresh 08:54:08Z. Product read-only.

## Disposition recommendation

Keep PR #3 OPEN pending Windows runtime and final integration gate. The narrow
optional-kind correction is appropriate, but do not merge its old patch blindly:
current App code preserves newer confirmation-aware routing, including leaf roots.
Integrate the already adapted discriminator with that behavior intact after the
lead's approval. After reviewed integration lands, audit whether PR3 is merged
or explicitly superseded and closed; neither disposition has happened here.

- URL: https://github.com/Indosaram/ferryx/pull/3
- OPEN, head `99c7086b61d7a590530fb8df924e34ce9e91e90b`, base main.
- `mergeStateStatus: DIRTY`: conflict resolution remains required, not merge-ready.
- `statusCheckRollup: []`; no passing or pending CI is reported.
- Review decision empty, reviews/comments empty; current GraphQL review threads
  totalCount 0, hasNextPage false. Nothing to resolve or respond to remotely.
- Local HEAD `da6eec06d65551f67bbc43f09910cde470c3478d`; foreign dirty
  App/NativeTerminalPane and all other product bytes were not edited by this task.

## Reviewed mechanism and retained evidence

Read complete `gh pr diff 3`: single production discriminator correction plus
eight parameterized mock wiring cases, tagged/untagged x pinned/unpinned x
web/native callback. PR web tests use metaKey, not Windows Ctrl input.
Read current App.tsx:1767-1834: handleCloseActiveSurface selects the active leaf,
calls handleClosePane, and preserves selected-agent confirmation; fallback calls
handleCloseTab, which guards pinned tabs. Current discriminator already accepts
existing non-browser tabs. Do not restore the PR's direct closePane/split-only path.

Source route: Windows File > Close Tab (`tab.close`, CmdOrCtrl+W in
src-tauri/src/lib.rs:59) -> `menu_close_tab` -> tauri.ts:637 listener ->
App native callback -> handleCloseActiveSurface. App keyboard binding also uses
handleCloseActiveSurface. A real Ctrl+W may be consumed by the native accelerator;
record the observed route, do not label it an independent DOM delivery without
the event trace. Native menu cases require an actual menu click, not callback injection.

Store closePane (workspaceStore.ts:1042) dispatches CLOSE_PANE, then closes only
the selected unreferenced backend; a single leaf delegates to closeTab. Split
close uses closeBackendSession, not the last-tab closeBackendSessionAndWait
helper. Thus GUI disappearance/fulfilled action alone is not an exit receipt.

Retain `wheel-close-lead-verification.md` without rerunning its completed work:
original App guard gives 14 intended failures, App 128 GREEN; real mounted store
eight selected/sibling ownership cases plus wrong-whole-tab mutation; 575-case
combined batch and UI build GREEN. These are existing local evidence, not this
task's executions, and not real Windows PTY proof.

## Executable missing matrix for runtime owner

New `dag-pr3/close-matrix.ps1` is an operator-assisted verifier, not another app
runner. It enumerates all **16** combinations: tagged/untagged x pinned/unpinned
x keyboard/native-menu x first/second focused leaf. It launches nothing and
never stops a daemon. It uses actual shell-generated random-run sentinel files,
retains the selected process handle before action, checks selected process exit,
and requires the survivor's exact same PID **and process start time**, plus new
post-close shell output. An actual HWND must belong to the supplied debug app
PID in the same Windows session. Different backend IDs before close and exact
same sibling backend ID after close are mandatory operator inputs from layout.

Run in the existing owned interactive Windows allocation, launched **only with
`bun tauri dev`**, after the runtime owner records current source/binary hashes.
Set variables to the allocation's actual values, then execute from repository root:

```powershell
& ./docs/evidence/windows-review-20260913/dag-pr3/close-matrix.ps1 `
  -AppProcessId $ownedAppPid -DebugExecutable $ownedDebugExecutable `
  -OutputDirectory $ownedEvidenceDirectory -OwnedInteractiveAllocation
```

Use two actual PowerShell PTYs per case. The script prints exact per-pane shell
commands, including unique output paths; paste into the requested real pane and
execute there. Do not run those commands in the observer console. It prompts for
the real layout snapshots, backend exit-event trace and HWND action captures.
These linked artifacts must be reviewed for correct selected/sibling mappings,
unchanged tab/pin/sibling leaf and lack of respawn; file existence and operator
ID entry are NOT machine verification of their contents. The receipt deliberately
says PROCESS_SENTINELS_VERIFIED, never full acceptance PASS.

Reuse the existing `windows-terminal-20260912/runtime/qa` launch/capture/action
infrastructure under runtime-owner control. Its capture-fresh.ps1 shows the
PID-owned HWND capture pattern. Those historical helpers contain old fixed paths
and PIDs: rebind them to the current owned allocation before use, never execute
historical cleanup scripts against user processes. Existing cdp-echo.mjs uses a
fixed old endpoint, focus emulation and a 400ms sleep, so it is NOT suitable as
real HWND keyboard proof and is not invoked by the matrix. Existing
script/qa/win-daemon-e2e.mjs's normal main spawns its own session: it is useful
separate ConPTY evidence but cannot establish survival of these GUI siblings.
No parallel daemon runner or copy of its private transport was introduced.

Before each close, subscribe the runtime owner's existing terminal exit recorder
to the exact selected backend, then trigger the real action and await that event
with a bounded failure deadline. The script itself has no polling/sleeps; user
prompts gate the manual GUI steps. Capture before/after screenshots and selected
exit trace. After each case close only the owned survivor through the GUI and
retain cleanup. User daemons/installed applications must remain unchanged.

Additional runtime controls remain: selected busy-agent cancel leaves both PTYs
alive, confirm closes only selected; busy sibling alone must not prompt; unsplit
pinned tab remains; unpinned final leaf closes correctly; browser/remote guards
do not close an unrelated terminal. These are not claimed by the 16 split cases.

## Commands, exits and limitations

- `gh pr view 3 --json state,headRefOid,mergeStateStatus,statusCheckRollup`:
  exit 0 initially and final refresh, unchanged values above.
- `gh pr diff 3`: exit 0, complete two-file patch read.
- `gh repo view --json nameWithOwner` and extended `gh pr view 3`:
  exit 0, canonical repository and review metadata read.
- Initial GraphQL query incorrectly used directory-derived indo/orca-lite:
  NOT_FOUND, exit 1. Corrected canonical Indosaram/ferryx query: exit 0,
  zero threads with pagination explicitly exhausted.
- `gh pr checks 3`: exit 1, `no checks reported on the 'fix/close-cli-panes'
  branch`, including final refresh. No pending checks exist to watch; no polling
  or sleep loop was started. Future checks after integration need lead monitoring.
- `omo-agent-toolkit ulw-loop --help` and `omo-agent-toolkit --help`: exit 127,
  command not found. Official registration bound to the original session is
  **blocked and handed to lead**. No toolkit tool is exposed in this child.
  goals.json was read; neither goals nor ledger was edited. No product fix was
  attempted before registration. Register matrix invocation/oracles through the
  original-session toolkit before any runtime regression/fix extension.
- `git diff --check`: exit 0. Only this report and its harness were written,
  using apply_patch. No branch/worktree operations, commits, push or GitHub writes.
- Harness diagnostics attempted: no LSP configured for .ps1. `command -v pwsh`
  found no executable. **PowerShell parsing/execution and Windows GUI execution
  are unverified**, not passing. Runtime owner must parse/run on native Windows;
  no mock PTY, local test rerun, build or desktop launch was performed here.

This handoff closes the current review-state audit and supplies the executable
manual matrix. Runtime results, official registration, conflict resolution,
approval, PR disposition and CI/final delivery remain unresolved requirements.

## Explicit owner handoff

Runtime owner: **st_01a099f8**. PR2 owner: **st_01a099f6**. This task retains
report/harness ownership only; App product files remain foreign.
The exact invocation and 16-case contract above are addressed to st_01a099f8.
The report and harness paths were already verified locally; no verification was
rerun for this handoff. Direct delivery is blocked in this child: no task-message
tool is exposed, `omo-agent-toolkit` is absent, and the environment exposes no
task RPC/socket interface. No internal task-state files were modified to simulate
a message. Parent must relay this report path and invocation to st_01a099f8;
delivery acknowledgment has not been observed.
