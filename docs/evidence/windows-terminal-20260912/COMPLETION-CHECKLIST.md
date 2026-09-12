# Completion audit

Status: NOT COMPLETE. Authoritative final audit: `FINAL-AUDIT.md`.
Shell and retained-frame changes committed as `1313d387` and `40c2ae5c`.
Final four suites: 62 passed; build exit0. Actual PTY startup/resize/cmd echo
outputs are saved. Cold-loaded `7f7ecd8e` now proves full menu-to-cmd and
unobscured startup/resize output. The original installed app's bounds banner
is positively identified, but its raw cause and same-seam RED/GREEN remain
unknown. Both QA runs are cleaned with the installed app and daemon preserved.

## Prompt-to-artifact checklist

- [x] Windows terminal type selection restored.
  - Diagnosis and historical regression: `shell-diagnosis.md`.
  - [x] Before/after behavior evidence: exact test IDs and RED/GREEN logs in `shell-implementation.md`; lead rerun confirms 23 shell tests pass.
  - Actual menu screenshot and action log: `+` exposes shell choices.
  - Actual Command Prompt execution: `echo FERRYX_WIN_SHELL_OK` output is visible.
  - Exact selected shell reaches spawn; default and non-Windows paths retain behavior.
  - Fresh evidence: `runtime/FRESH-RUN.md`, `runtime/fresh-visual-review.md`,
    `runtime/artifacts/fresh-menu-cmd-receipt.json`.
- [ ] Startup terminal appears instead of native bounds failure.
  - Confirmed call-chain and runtime cause: `bounds-diagnosis.md`.
  - RED before production edit and identical GREEN proof.
  - Windows interactive launch command is exactly `bun tauri dev`.
  - Real GUI screenshot shows terminal plus `echo FERRYX_WIN_START_OK` output.
  - Runtime log captures actual HWND geometry and no bounds failure.
- [ ] Resize and lifecycle edge behavior is preserved.
  - Pin relevant early-attach, zero-size and resize tests against the diagnosed seam.
  - Resize real Windows debug window and show `echo FERRYX_WIN_RESIZE_OK` output.
  - Screenshot verifies native surface does not obscure app chrome.
  - Genuine failure remains actionable, not silently ignored.
- [ ] Combined verification covers the actual change.
  - [x] LSP diagnostics on current edited files have no errors; existing `keyCode` deprecation hint recorded.
  - [x] Targeted test invocation and exit code are captured in `lead-verification.md`; two failures remain explicitly reported, not hidden by a green status.
  - [x] Frontend build uses `bun run --cwd ui build` (`tsc && vite build`), exit 0.
  - Backend/Windows compilation covers any changed platform-specific code.
  - No passing suite, process count or HTTP response is accepted as GUI proof.
- [ ] mass-ulw execution is evidenced.
  - Diagnosis run: `dag_dece750f-aa1a-49c0-963c-1ba6be838716`.
  - [x] Implementation run `dag_0af60f86-ac23-45f9-93dd-2b918f67c2e0` executes disjoint lanes in parallel before combined verification.
  - Corrections, if needed, use separate phase runs and preserve conflict boundaries.
- [ ] Safety and cleanup are evidenced.
  - Foreign edits are preserved; source hashes/status captured before writes.
  - User daemon is not killed or restarted.
  - No release builds, publishing or pushes.
  - Every QA process, browser context, scheduled task, port and temporary remote artifact has a cleanup receipt.
- [ ] Delivery is complete.
  - Final repository report links actual evidence, limitations and self-review.
  - Durable confirmed facts are recorded in memory.
  - Verified atomic commits follow repository history and include only this task.
  - Append-only notepad has current status and evidence.
  - Goal completion is called only after every criterion has concrete passing evidence.
