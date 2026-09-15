# Windows terminal regression repair

Tier: HEAVY, because native terminal attachment and bounds may cross concurrent session lifecycle boundaries.

Ideal end state: opening a new Windows terminal offers shell choices, and application startup plus window resize display a usable native terminal without `Failed to update native terminal bounds`.

## Execution

1. Diagnose shell UI, native bounds and Windows QA environment in parallel; verify the combined diagnosis.
2. Capture failing-first evidence and implement the two fixes in disjoint scopes in parallel; verify the combined batch.
3. Run actual Windows debug GUI checks and relevant local tests/build. Correct failures in scoped parallel batches.
4. Clean QA resources, save evidence, self-review and commit verified increments.

## Active runs and revised dependency boundary

- Diagnosis: `dag_dece750f-aa1a-49c0-963c-1ba6be838716`. Shell diagnosis is complete; bounds and environment reporting encountered provider connection errors. Reports remain required, but do not gate independently established fixes.
- Independent repair batch: `dag_0af60f86-ac23-45f9-93dd-2b918f67c2e0`. Shell-selection and retained-presentation producers run in parallel, followed by one combined test/diagnostics/build verification. The presentation repair addresses three observed baseline failures; it is not proof of the Windows startup cause.
- Windows runtime RED: `dag_0180c8f2-ce1f-4aef-b114-1483c74f995f`. A separate committed-source Windows checkout captures startup behavior through exactly `bun tauri dev`, followed by artifact verification. It must not include ongoing uncommitted fixes, alter the installed app, or restart the user's daemon.
- A startup production correction follows decisive runtime evidence. Its write scope must not overlap the running presentation producer. Final Windows shell/startup/resize GREEN and cleanup still gate completion.

## Scenarios

- Shell choice: click tab-bar `+`, choose Command Prompt, enter `echo FERRYX_WIN_SHELL_OK`. Pass requires real selectable shell entries and observed command output, plus RED/GREEN regression proving exact selected shell reaches spawn.
- Startup: launch exactly `bun tauri dev` in an interactive Windows session; enter `echo FERRYX_WIN_START_OK`. Pass requires a visible native terminal, observed output and no bounds failure, plus RED/GREEN at the confirmed seam.
- Resize: resize the same debug window and enter `echo FERRYX_WIN_RESIZE_OK`. Pass requires valid viewport geometry and unobscured terminal output. Targeted lifecycle tests cover the confirmed edge and retain non-Windows default behavior.

Exact commands and test IDs are recorded in lane evidence before execution. Screenshots and action logs, not process existence, establish GUI success.

## Constraints

Preserve foreign edits and the running user's daemon. Debug only; no release build or publishing. One workflow run per phase, independent implementation first, combined verification afterward. No ulw-plan reviewer gate applies. Notepad is append-only at `/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ulw-20260912-202404.XXXXXX.md.9tqDfhYLOf`.
