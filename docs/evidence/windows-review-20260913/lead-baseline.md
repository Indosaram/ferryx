# Windows review: lead baseline and acceptance map

Date: 2026-09-13. Status: audit in progress; no production changes or push.

## Binding state

- Loop: `.omo/ulw-loop/01a0983f-c995-753d-afa9-593f6d118788/`.
- Goal: `G001-review-and-resolve-every-currently-o`.
- Audit run: `dag_948b2c08-1b90-42dc-abca-48c6b0136c78`.
- Baseline HEAD: `b7ad45163e6d90f2c6ae4410a821e57f7198f5e0`.
- Baseline tree: `1fd9789e3f66ce6b612fe5df5adc05e4f9b0541d`.
- `git rev-list --left-right --count origin/main...HEAD`: `0 0`.
- Main contains foreign changes, including native terminal image support and
  worktree rescanning. They are not part of this session's changes.
- Worktree/branch creation and deletion permission was requested and remains
  pending. Main must not be switched, reset, restored, stashed, or cleaned.

## PR evidence

`gh pr list --state open --limit 100` found:

- PR #2, `fix/windows-drag-interactions`, head
  `79b02ab6ea4753bf87080dd567368a870c018057`.
  Files: `src-tauri/src/native_terminal/platform/windows.rs` and new
  `windows_pointer_tests.rs`. Proposed fix makes the rendering child disabled
  for hit testing. The claim and tests are being independently reviewed.
- PR #3, `fix/close-cli-panes`, head
  `99c7086b61d7a590530fb8df924e34ce9e91e90b`.
  Files: `ui/src/App.tsx` and `ui/src/App.test.tsx`.
  Proposed fix treats an existing non-browser tab with omitted `kind` as a
  terminal for focused-pane close. The claim and tests are being reviewed.

Both were OPEN and MERGEABLE, with empty status-check rollups. GraphQL
`reviewThreads(first:100)` returned zero threads for both PRs. GitHub API
`repos/Indosaram/ferryx/branches/main/protection` returned HTTP 404 with
`Branch not protected`. Merge, squash, and rebase are enabled; automatic
branch deletion is disabled. This is metadata, not approval of either PR.

## Windows host baseline

Read-only SSH to `maho-win` returned hostname `DESKTOP-1LAPJMP`.
An encoded PowerShell `Get-CimInstance Win32_LogicalDisk` query returned:

```json
{"DeviceID":"C:","Size":999127248896,"FreeSpace":195121086464}
```

The earlier `Get-PSDrive C` probe produced null properties and was rejected
as disk evidence; the CIM query above replaced that probe.

Existing installed processes must survive QA. All paths below were
`C:\Users\sook\AppData\Local\Ferryx\ferryx.exe`, in interactive session 1:

- PID 1756, started `2026-09-12T20:20:01.8377047+09:00`.
- PID 17288, started `2026-09-12T20:19:58.8959221+09:00`.
- PID 20196, started `2026-09-12T20:20:02.0376403+09:00`.

Available commands:

- Bun: `C:\Users\sook\.bun\bin\bun.exe`.
- Cargo: `C:\Users\sook\.cargo\bin\cargo.exe`.
- Git: `C:\Program Files\Git\cmd\git.exe`.

Prior `runtime/FRESH-RUN.md` under `windows-terminal-20260912` documents an
isolated interactive scheduled-task launch using exactly `bun tauri dev`.
It is a setup recipe, not current-tree QA evidence. The existing installed
application is an older build and is not this task's test target.

## Prompt-to-artifact acceptance map

Every unchecked item is unproven, not implicitly passed.

- [ ] PR #2: current diff review, same-assertion RED/GREEN, native pointer
  delivery and real drag-selection proof. Target: `pr-review.md`,
  native-input regression logs, GUI action log and screenshots.
- [ ] PR #3: tagged/untagged and pinned/unpinned split-tab close, keyboard
  and native-menu paths, sibling PTY survival. Target: `pr-review.md`,
  UI regression logs, GUI actions and PTY receipts.
- [ ] Windows wheel: native wheel up/down changes the intended viewport;
  alternate-screen reporting and split-pane isolation stay correct.
  Target: `runtime/` screenshots, input/state logs, RED/GREEN captures.
- [ ] Exhaustive Windows review: every Windows branch and caller assigned,
  prior applicable audit findings rechecked, unsupported historical claims
  refuted explicitly. Target: eight `audit-*.md` reports and `coverage.md`.
- [ ] Confirmed findings: each has a reproducer registered before its fix,
  minimal correction, passing regression and faithful surface evidence.
  Target: `findings.md`, `repair-packets.md`, per-finding logs.
- [ ] Adjacent behavior: keyboard, shell menu to real cmd echo, resize/DPI,
  browser/native isolation, reconnect, daemon output/cwd/session survival.
  Target: `runtime/` native GUI and PTY receipts.
- [ ] Combined checks: relevant frontend tests/build, backend tests and
  platform integration checks pass without suppression.
  Target: captured command logs with exit codes.
- [ ] Cleanup: all owned QA resources removed, user process identities
  preserved, foreign changes preserved. Target: cleanup receipts.
- [ ] Final gate: frozen tree and commit tied to complete current evidence
  and explicit gate verdict. Target: gate report and `final-audit.md`.
- [ ] Delivery: verified atomic commits pushed only after completion;
  `git ls-remote origin refs/heads/main` equals reviewed SHA; both PR
  dispositions and subsequent checks verified. Target: push/PR/CI receipts.

The eight audit producers may only write their individual reports. The
synthesis node owns `coverage.md`, `findings.md`, and `repair-packets.md`.
This lead document does not authorize production changes, installation,
release builds, or replacement/restart of the installed user application.
