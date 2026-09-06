# Project location selection: implementation and verification

Status: implementation and independent code review are complete and committed.
Native desktop acceptance remains unverified; the overall goal is not marked complete.

## Requested outcome

The sidebar Add Project action must offer Local and Remote before opening a
folder picker. Only explicit Local selection may open the local picker. Remote
selection must use the SSH machines managed in Settings and register a real
remote project without interpreting its path locally. The user also requested an
implementation plan, implementation and completed code review.

Plan: `PROJECT_LOCATION_PLAN_2026-09-06.md`.
Evidence directory: `evidence/project-location/`.

## Requirement-to-artifact checklist

- **C1: Local/Remote choice before local filesystem access.**
  - Implementation: `ui/src/components/ProjectDialogs.tsx`.
  - Initial RED observed the old eager picker: zero calls expected, one received.
  - Current tests cover explicit Local invocation, StrictMode, duplicate in-flight
    clicks, picker cancellation, local confirmation and manual fallback.
  - A discovered retry regression was fixed: picker rejection -> Back -> Local
    now invokes a second picker and completes registration.
  - Evidence: `chooser.md`; `ui-verification.md`, section 7.
  - Native OS picker and compositor visibility: **not verified**.

- **C2: Remote uses Settings inventory and actual remote registration.**
  - Settings: `SettingsDialog.tsx`, `settings/SshSection.tsx` and
    `lib/sshHosts.ts` share the existing backend `ssh_hosts.json` inventory.
  - Adapter: `lib/remoteProject.ts` invokes `cmd_project_register_remote`.
  - Backend: `ipc/project_remote.rs`, `ssh/direct.rs`, `ssh/projects.rs` and the
    existing daemon/PTY service establish and execute host-qualified projects.
  - The actual implementation uses opaque `ssh:<hash>` IDs and durable remote
    records, not local placeholder directories or a mandatory remote helper.
  - Real OpenSSH/PTY evidence: the lead executed the production registration and
    daemon spawn integration test with a generated, isolated loopback sshd
    fixture. Both new and split/restored terminals produced the exact remote
    working directory.
  - Evidence: `backend.md`, `backend-real-ssh.log`, `qa-environment.md`,
    `runtime.md` and `ui-verification.md`.
  - Actual desktop remote-add screenshot and user interaction: **not verified**.

- **C3: Empty/stale/disabled/deleted hosts and adjacent behavior.**
  - Empty inventory provides the SSH Machines Settings action.
  - Failed authoritative refresh does not fall back to stale inventory.
  - Removing/disabling a selected host clears selection rather than silently
    choosing another machine.
  - Dialog dismissal/unmount suppresses late UI callbacks. This is not a claim
    of IPC transaction cancellation or rollback after a backend request commits.
  - App persistence, active/inactive project handling, new tabs, split panes and
    restored sessions retain host and backend workspace identity.
  - Remote sidebar worktree/reveal actions are disabled rather than sent to local
    filesystem code. Direct SSH projects currently provide root terminals, not
    remote Git worktree mutation or remote agent-session resume.
  - Evidence: `settings-foundation-verification.md`, `chooser.md`, `runtime.md`,
    `runtime-full-suite.md`, `ui-verification.md`.
  - Native empty-state/Settings-navigation interaction: **not verified**.

- **C4: Checks, independent review and cleanup.**
  - Focused SSH settings verification: **32 tests passed**; independently
    reproduced by the lead, with fresh production-file LSP diagnostics clean.
  - Final chooser delta: **55 tests passed** in independent verification,
    including the actual shared-hook retry supplement.
  - Runtime producer/independent verification: **170 tests passed**.
  - Lead notification follow-up verification: **47 tests passed**, no unhandled
    errors. This repaired an unstable mock callback exposed by the new reactive
    restoration subscription, while preserving closed-target assertions.
  - Lead real OpenSSH registration/PTY test: **1 passed**, exit 0.
  - Lead frontend build: **exit 0**, TypeScript plus Vite, 1,868 modules
    transformed; Vite completed in 2.33 seconds.
  - Final whole frontend suite: **1,724 passed, 7 failed**, exit 1, 172 files,
    97.77 seconds. Remaining failures are listed below; the suite is not described
    as green.
  - Final backend verification: **33 SSH tests and 4 project-remote tests passed**;
    Cargo check exited 0. The lead's final Cargo check and 4-test command also
    exited 0.
  - Final independent code review: **APPROVE code**, with no actionable current
    feature-code finding. Native acceptance is explicitly not approved.
    Evidence: `code-review.md` and `backend-verification.md`.
  - One earlier configurable SSH QA invocation hit the production 8-second
    deadline. The later required single execution passed; the earlier timeout is
    retained as an unexplained reliability observation, not erased.
  - Lead-created VM, browser/server and private QA files: cleaned up as recorded
    in `qa-environment.md`.

## Whole-suite failures retained explicitly

Final command:

```sh
CI=1 bun run --cwd ui test --reporter=dot
```

Actual summary:

```text
Test Files  3 failed | 169 passed (172)
     Tests  7 failed | 1724 passed (1731)
Duration  97.77s
Exit 1
```

The seven failures match the previously recorded baseline:

- `src/App.test.tsx`: signed-update-on-start test.
- `src/lib/tauri.test.ts`: two focused-terminal payload expectations omit the
  existing `attentionInventory` field; one notification probe expectation omits
  the existing `sound` argument.
- `src/features/ferryx/push/client.test.ts`: same-origin task-link parsing,
  denied-permission subscription, and failed server-unsubscribe expectations.

The initial whole-suite run also exposed an App notification test timeout and a
missing permissions mock export. Those were fixed without changing production
navigation or suppressing errors. The final full run no longer reports that
eighth failure or the unhandled rejection. Details and HEAD comparison are in
`evidence/project-location/runtime-full-suite.md`.

## Native verification limitation

The selected Orca computer-use runtime exited before it could drive the app.
System Events reported UI scripting disabled; Peekaboo reported Accessibility
and Screen Recording permissions ungranted. A supplemental Bun.WebView attempt
failed to spawn its host process. No native screenshots or browser chooser
scenario passes were captured.

These are environment limitations, not passing product evidence. Actual
verification still requires the debug app launched with exactly:

```sh
bun tauri dev
```

Required observations:

1. Click sidebar Add project: Local and Remote appear and no OS picker opens.
   Choose Local: the OS folder picker opens; cancel and retry still work.
2. In Settings -> SSH Machines, configure an enabled, trusted key/agent-authenticated
   POSIX SSH host. Add a remote project from the sidebar using that machine and a
   remote absolute directory. Confirm the project identifies the correct host
   and its terminal executes `pwd` on that host.
3. With no enabled SSH machines, Remote shows an actionable Settings link and
   cannot submit. Canceling the flow does not add a visible project.

## Review and commit record

Final independent code review: **APPROVE code**.
See `evidence/project-location/code-review.md` for the complete assessment and
the explicitly unverified native gate.

Source commits:

- `8efc26e feat(ssh): register host-qualified projects and route remote terminals`
- `39602ef feat(settings): connect SSH machine management to shared inventory`
- `b708194 feat(projects): choose local or SSH location before adding projects`

Only session-owned files and functional backend hunks were staged. Foreign
native-terminal, permissions, notifications, browser/link-routing, Cargo/vendor
and unrelated formatting/documentation changes remain outside these commits.
No push, release build, deployment or live daemon shutdown was performed.
