# Agent-state watchdog: current manual QA

Status: NOT RUN. These steps are for user execution, not desktop automation.
They supplement the older main-repository `.omo/FERRYX_MANUAL_GUI_QA.md` and
replace its unconditional manual-reset success expectation.

## Launch

```sh
cd /Users/indo/code/project/orca-lite-wt/sa-watchdog
bun tauri dev
```

Use the debug app from this worktree. Record the current live daemon PID and
start time before and after. Never kill, restart, or signal a daemon to run
these checks or manufacture a reset failure. If a daemon replacement is
required, stop and report it. An older running daemon may not contain this
track's backend changes; in that case backend behavior is NOT VERIFIED for
this source snapshot, even if the frontend opens.

## User actions and expected observations

1. In a disposable session, run a real supported agent and confirm its activity
   indicator appears. Observe process exit back to the shell: the indicator
   should release on that process evidence. An agent finishing a response but
   remaining alive in its prompt is not an agent-to-shell transition.
2. Observe an agent that remains alive while quiet. Quietness alone must not
   clear its activity. Record the observed interval without treating a chosen
   duration as an implementation timeout or proof of indefinite behavior.
3. On a disposable session with activity, invoke Reset Agent State from the
   tab menu. On backend success, its activity clears and success is reported.
   Start another agent afterward and confirm detection still works.
4. Repeat using Reset Agent State on the worktree row with multiple disposable
   sessions. The operation should attempt the matching sessions. For all
   successful responses, activity clears and one success notification appears.
5. If a reset fails naturally, record the error and whether the corresponding
   activity remains. Failure must not produce an unconditional success toast.
   For a partial group failure, successful sessions may clear while failed
   sessions retain activity, and the outcome should be an error notification.
   Do not break the live daemon to exercise this case. If no failure occurs,
   mark it NOT EXERCISED; controlled promise tests already cover it.
6. If available without changing the runtime, inspect release-reason logs
   for manual reset versus foreground agent-to-shell transition. Report the
   actual reason and session, rather than inferring a log entry from the UI.

## Code and evidence

The current production diff was read for this handoff:
`ui/src/state/workspaceStore.ts` awaits reset IPC before local dispatch and
uses `Promise.allSettled` for worktree groups; `ui/src/App.tsx` routes failed
tab/worktree outcomes to error notifications.

`docs/WATCHDOG_RESET_OUTCOME_REPAIR_EVIDENCE.md` contains the repair's
pre-production RED, GREEN and independent 145-test pass, including controlled
single/grouped failure scenarios. `docs/AGENT_STATE_WATCHDOG_EVIDENCE.md`
records backend HOLD/transition tests. Those results do not prove the running
desktop or daemon uses the changed source.

## Return evidence

Report PASS, FAIL, NOT RUN, or NOT EXERCISED for each action, with the actual
indicator/notification behavior, source worktree, and daemon PID/start time.
Close the GUI normally; do not signal the daemon. These observations cannot
replace the original PID baseline or recover missing historical RED evidence.
