# Disk management: current manual QA

Status: NOT RUN. These steps are handed to the user, not automated or claimed
as desktop verification. They supersede the disk deletion expectations in the
older main-repository `.omo/FERRYX_MANUAL_GUI_QA.md`.

## Launch and scope

Use this track's current worktree, not an older integration build:

```sh
cd /Users/indo/code/project/orca-lite-wt/sa-worktree-disk
bun tauri dev
```

Run only the debug app. Do not kill, restart, or signal a live daemon. Record
the current daemon PID and start time before and after the manual session.
This is a new observation, not replacement of the original PID 36170 baseline.
If launch requires a daemon replacement, stop and report that requirement.

Use an existing disposable worktree with no active agents for deletion checks.
Do not create commits for this QA. Actual destructive deletion is optional:
inspect and cancel the preview if no disposable target is available, and mark
the deletion step NOT RUN.

## User actions and expected observations

1. Right-click a project header and open Manage Worktree Disk. Let a scan
   complete. Inspect apparent-size ordering, last commit information, dirty
   state, per-row errors, and the approximate-size caption. An advisory cleanup
   badge must not authorize or perform deletion.
2. On a sufficiently large repository, cancel a scan, then separately close
   the dialog while another scan is running. Reopen and check responsiveness
   and absence of a partial completed result. A scan that finishes before the
   action does not exercise cancellation; report that case as NOT EXERCISED.
   Backend cancellation itself is covered by automated evidence, not inferred
   solely from a responsive window.
3. After scanning a disposable worktree, add an untracked file there. Open
   its deletion preview. The new path must appear even though it was absent
   from the scan snapshot. The count comes from the fresh preview, not the
   earlier disk row. Cancel the dialog without deleting anything.
4. While deletion preview is loading, destructive confirmation must be
   unavailable. If a preview error occurs naturally, record the structured
   error and verify deletion remains unavailable. Do not disable or restart
   the daemon to manufacture failure; controlled failure is already tested.
5. With more than eight changed files, verify eight paths and the remaining
   count are shown. Check that the confirmation control remains reachable.
6. If a safe deletion attempt reports newly dirty or unmerged state, inspect
   the refreshed preview before any destructive confirmation. The old preview
   must not remain actionable during refresh. This race is deterministic in
   the automated test; inability to trigger it manually is NOT EXERCISED.
7. Only on the disposable target, optionally confirm deletion. Its row must
   disappear and must not reappear because a queued scan result arrives.
   Report the target path and whether deletion was actually performed.

## Source and automated evidence

Read for this handoff: `ui/src/components/WorktreeDeleteDialog.tsx:79-111`
derives dirty files from the preview and requests fresh metadata;
`:140-148` invalidates and refreshes after safe-delete refusal;
`:157-158` guards destructive handling; `:239` disables the control while
busy or without a preview. Line numbers describe this snapshot.

`docs/DISK_REPAIR_LEAD_VERIFICATION.md` records lifecycle/deletion regression
checks. `docs/DISK_BUN_COMPATIBILITY_EVIDENCE.md` records independent Bun and
Vitest passes for 31 tests. Neither proves the desktop observations above.

## Return evidence

For each numbered action, report PASS, FAIL, NOT RUN, or NOT EXERCISED, with
the visible behavior and any error. Include the worktree/build used, daemon
PID/start-time observations, and any disposable files left for cleanup.
Close the GUI normally; do not signal the daemon as cleanup. A manual pass
cannot repair the original historical acceptance failures.
