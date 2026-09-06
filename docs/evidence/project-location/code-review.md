# Final Local/Remote project-add code review

Reviewer task: `st_01a077ff`. Date: 2026-09-06 UTC.
Working tree: `/Users/indo/code/project/orca-lite`.

## Verdict

**APPROVE code for the scoped Local/Remote project-add implementation.** No
current substantive feature-code defect requiring changes was identified in the
reviewed implementation. This is **not** an unconditional release/acceptance pass:
**the native desktop gate remains UNVERIFIED and is NOT APPROVED**, and the full
frontend suite is not green.

This is a completed, executable review, not a request for another implementation
or review lane. Only this report and `backend-verification.md` were written.
No source fixes, test modifications, commits, new harness, VM, research, desktop
automation, or review panel were used.

## Severity-ordered findings

### Current feature-code findings

- **P0 / P1:** None identified.
- **P2 / P3:** No actionable current feature-code finding identified. The already
  corrected picker retry and Reveal provenance defects are not reopened as if
  their old snapshots were current. No polish or foreign-tree changes are
  proposed as feature blockers.

### Retained verification failures and resolved findings

These are explicitly separated from current code findings; they do not receive a
manufactured product-defect severity without evidence of a product cause.

1. **Unresolved verification reliability observation** -
   `src-tauri/src/daemon/remote_ssh_qa.rs:95`, with the parent assertion at `:67`.
   The prior optional configurable QA phase failed in production registration:
   `actual backend registration/probe against QA host: IpcError { code: InvalidPath, message: "SSH directory probe timed out after its bounded deadline", details: None }`.
   Parent: `SSH QA child failed: exit status: 101`. The child finished in 8.01s;
   parent in 14.14s; exit 101 (`backend-public-core.log:1499-1518`).
   `ssh/direct.rs:125-127` supplies the actual 8-second deadline. Reading the
   test confirms that this phase invokes the same production core as the Tauri
   wrapper, not a substitute implementation. The capture does not identify why
   that invocation stalled. This review's **first and only required SSH command
   passed all 33 tests**, including the configurable seam against the existing
   generated loopback fixture. The earlier failure remains evidence, not a pass,
   and repeated-run reliability is not claimed. Minimal recommendation if the
   failure is investigated: identify the stalled connection/probe stage in the
   existing fixture before changing its deadline; no retries, skipped assertions,
   or weakened trust. No such follow-up work was executed here.

2. **Pre-existing frontend regression failure** - `ui/src/App.test.tsx:662:56`:
   `await waitFor(() => expect(updater.checkForUpdate).toHaveBeenCalledOnce());`
   fails with `AssertionError: expected "spy" to be called once, but got 0 times`.
   Both `runtime-app-related.log` and the supplied
   `runtime-updater-baseline.log` contain this same assertion failure. The
   baseline classification is supported by those historical captures and the
   scoped diff, not a baseline reconstruction in this review. Minimal
   recommendation: align the updater-start fixture with the actual polling
   startup contract in its owning scope; do not suppress the expectation or
   describe the broad suite as green. No updater fix or rerun was made here.

3. **Resolved prior feature defect** -
   `ui/src/components/ProjectDialogs.tsx:178-180` releases the picker guard in
   `finally`. Local -> rejection -> Back -> Local is no longer locked in pending.
   The deterministic deferred-promise regression at
   `ui/src/components/ProjectDialogs.test.tsx:375-414` and the independently
   executed real-hook supplement in `ui-verification.md` section 7 establish
   the correction. All six chooser/adapter/hook source and test hashes captured
   in this review match that section's final verified snapshot exactly.

4. **Resolved prior boundary defects** - `src-tauri/src/ssh/projects.rs:80-85`
   hashes an eight-byte little-endian UTF-8 host-ID length; `:178-184` guards
   Reveal only from explicit workspace context. The formerly overbroad
   path-prefix inference is absent. The three lead-regression identity/Reveal
   tests passed in this review's SSH command. No remote `/`, `/Users/admin`, or
   identical remote path can make a legacy local Reveal remote.

## Review basis and ownership

Read the current implementation report, the plan's **lead execution corrections**
and acceptance checklist, `backend.md`, `ui-verification.md` including its
superseding section 7, `runtime-full-suite.md`, and `qa-environment.md`. Examined
actual tracked diffs and explicitly read the new untracked SSH/registration,
chooser adapter, identity, Settings and regression-test files; `git diff` alone
would have omitted them.

The implemented design correctly replaces obsolete plan sketches: no placeholder
local directory or remote WorktreeManager is needed; the durable remote record is
the registration. The daemon payload is typed `RemoteSsh { host_store_path }`,
not arbitrary frontend-selected program/argv. Canonical opaque `ssh:<hash>` IDs
are adopted by the frontend, not replaced with advisory slugs. These departures
satisfy the lead corrections rather than constituting missing functionality.

Foreign native-terminal, permissions, notifications, browser/linkRouting, Cargo,
vendor and unrelated documentation changes are **not this feature**. Broad Rust
formatting changes were separated from behavioral hunks. The notification fixture
follow-up was read as integrated evidence, not expanded into a notification
review. Existing foreign warnings remain visible. Shared dirty-tree authorship
cannot be reconstructed merely from `git status`; no blanket historical
foreign-write guarantee is claimed. This review did not alter those files.

## Backend boundary assessment

| Boundary | Inspected mechanism and evidence | Assessment |
| --- | --- | --- |
| Host authority and actual registration | `ipc/project_remote.rs:37-86` resolves the persisted host via the same inventory path as Settings, validates the remote path, probes it over SSH, rechecks the host snapshot, and only then persists. The Tauri wrapper pings the daemon before registration. The public core is the production function, not a QA duplicate. | PASS: no fabricated local registration or frontend host DTO authority. |
| Canonical identity and collision | `ssh/direct.rs:103-121` uses remote `cd`, `pwd -P`, optional remote Git root and strict NUL-framed parsing. `ssh/projects.rs:80-109,147-164` qualifies SHA-256 identity by host ID and canonical remote root, checks stored identity and rejects conflicting locations. `ipc/project.rs:79-81` and `worktree/registry.rs:18-39` reject the remote namespace before local canonical-root deduplication. | PASS: same path on different hosts, or a local workspace with that path, cannot collide through path-only identity. |
| Persistence and restart routing | `ssh/projects.rs:41-47,111-174` reloads JSON, fails on corruption, serializes remote mutations, uses unique create-new 0600 temporary files, syncs and renames, and propagates failed cleanup. `daemon/server.rs:1909-1935` resolves disk state without requiring a local registry entry. | PASS for ordinary persistence/reload and a fresh daemon instance, exercised by tests. Not a native app restart or power-loss durability observation. |
| PTY startup and attach | `ipc/terminal.rs:581-604` routes reserved IDs before local CWD inheritance/canonicalization and rejects frontend startup/worktree overrides. Batch spawn calls the same entry. `daemon/server.rs:1909-1935,1988-2002` resolves the saved host before idempotency and calls typed `TerminalService::spawn_ssh`; `terminal/service.rs:53-70` never assigns the remote root to local process CWD. `ipc/terminal.rs:814-835` attaches through the daemon, whose `server.rs:1411` checks `validate_session_ssh_target` (`:2117-2132`). | PASS: real SSH PTY output for new and split/replacement sessions; attach validation is source-traced and its helper is exercised in the real test. No claim of a separately executed native attach UI. |
| Disabled/deleted host and no fallback | `ssh/projects.rs:50-77` rejects missing/disabled hosts. Spawn reloads the host; attach reloads the stored route. `terminal/shell.rs:394-396` rejects unresolved remote startup. Unregister revokes daemon workspace sessions before removing the remote record (`ipc/project.rs:263-275`). | PASS: disabled/deleted host tests reject subsequent spawn; disabled attach validation rejects. Already attached sessions are not retroactively terminated by host disable, and no such guarantee is required or claimed. |
| Quoting, options and lifetime | `ssh/direct.rs:13-99,130-194` validates connection tokens and explicit absolute POSIX paths, quotes apostrophes, keeps saved port/key/jump as argv values, forces batch/strict trust and bounded connection options, limits each probe output stream to 16 KiB, kills/reaps on timeout/overflow, and uses kill-on-drop for cancellation. | PASS for the inspected direct-child boundary; deadline/reaping and overflow tests pass. Trusted personal OpenSSH configuration is not treated as attacker input; no arbitrary executable or trust override exists in the registration request. |
| Local Git and Reveal | `worktree/registry.rs:18-39,157-166` makes all manager-based SSH operations unsupported, including unloaded records. `ipc/project_remote_tests.rs:21-126` actually invokes list/create/delete/destructive-delete/status/preview/branches/Reveal boundaries. `ipc/project.rs:281-285` applies `guard_reveal` before local filesystem access. | PASS: structured `UNSUPPORTED`; explicit remote workspace context blocks Reveal, while local/absent context preserves legacy local behavior. `ui/src/lib/tauri.ts:151-152`'s path-only reveal remains local-only and Sidebar never calls it for remote projects. No path/store inference is reintroduced. |

The real transport regression uses generated keys, an already-bound loopback
listener and real sshd in inetd mode. It registers a symlinked directory containing
spaces/apostrophes, verifies plain/Git results and stable re-registration, creates
a fresh daemon, and exercises two PTYs. It subscribes before input, uses an
escaped output marker rather than accepting terminal input echo, closes its
sessions, and aborts/awaits its listener. Deadline tests test time itself; no
sleep/polling-based synchronization was introduced in these inspected new tests.
This surface proves backend behavior, not desktop behavior.

## Frontend and runtime assessment

| Criterion | Current mechanism | Assessment / executable provenance |
| --- | --- | --- |
| Choice before picker | `ProjectDialogs.tsx:87,153-184` starts at `choose-location`; only explicit Local calls `open`. The pending guard spans promise settlement; mounted/dismissed checks suppress late callbacks. | PASS at component/native-transport-fixture boundary. Independent final delta: 55 tests; native OS picker not observed. |
| Remote inventory and empty state | `ProjectDialogs.tsx:95-96,126-146,500-527` uses `useSshHosts`, clears removed/disabled selection instead of choosing a different host, and offers SSH Machines Settings when none are enabled. `SettingsDialog.tsx:149,183` mounts the real SSH section. `SshSection.tsx:65,167-225` uses the same shared CRUD wrappers. | PASS based on current code and retained Settings/actual-hook UI verification, not a second inventory. |
| Authoritative refresh and cancel/back | `ProjectDialogs.tsx:233-284` refreshes at submit, fails closed on refresh rejection or unavailable selected ID, calls the typed remote adapter, and checks dismissal/unmount before callbacks. Remote Back is disabled while submitting; Cancel remains available. | PASS for UI lifetime. Cancel is **not** IPC transaction cancellation or rollback after persistence; no visible project is added by a late callback. |
| Canonical identity and target | `remoteProject.ts:18-36` preserves the backend response ID and host. `App.tsx:781-873,2195-2217` selects remote registration, adopts canonical identity, gates readiness, persists target and rejects invalid stored targets. `projectIdentity.ts:4-10` never treats reserved SSH IDs with missing/invalid targets as local. | PASS. Current App/runtime hashes match the independently verified 170-test snapshot. No locally registered fallback after failed remote registration. |
| Active/inactive roots and restore | `inactiveProjectWorktrees.ts:76-79,121-122` skips local registration and event relisting for SSH. `workspaceRuntime.ts:164` uses root-only mode. `sessionPersistence.ts:243-248,348-351` preserves target and explicit remote root ownership; `worktreeOwnership.ts:27-47` excludes remote projects from local path inference. App gates queued selection on registration and restore status (`:1235-1237`), plus new tabs/splits (`:1442,:1773`). | PASS at App/store/transport surface, including same local/remote path, native-startup session fixture, canonical adoption and deferred restore. Real backend PTY routing is independently verified separately. |
| Unsupported UX and friendly labels | `Sidebar.tsx:378-386,504-509,524-545,611` renders an SSH root rather than local Git rows, shows host-aware labels, disables local Git/reveal menu entries, and guards stale callbacks. `App.tsx:1658-1665` rejects remote worktree creation. `ProjectDialogs.tsx:882-893` uses friendly removal text without changing backend identity. | PASS for the scoped unsupported actions. Remote Git mutations, remote agent resume and live remote CWD inheritance are not added requirements. No foreign browser/link routing behavior is approved by this review. |

The UI evidence is inherited explicitly: no new frontend test or harness was run
here. Hash comparison matches all six files in `ui-verification.md` section 7 and
the App, App remote test, inactive-root, runtime, restore and store hashes in its
historical runtime snapshot. Source inspection is not represented as desktop QA.

## Actual checks and non-green evidence

Fresh review checks, each executed once:

- Pre-Cargo Rust LSP directory request: 50 files scanned (tool cap), zero
  diagnostics/errors. Not all-file LSP coverage.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib ssh -- --nocapture`:
  **33 passed, exit 0**, including real loopback SSH/PTY and its existing QA seam.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib project_remote`:
  **4 passed, exit 0**.
- `cargo check --manifest-path src-tauri/Cargo.toml`: **exit 0**. Vendor and
  unrelated existing warnings were retained, not suppressed.
- Scoped tracked `git diff --check`: exit 0. SHA-256 comparison as described above.
- Exact timestamps, captured exits/tails and historical backend failure output
  are in `backend-verification.md`.
- Both reports were read back and checked for verdict/native-gate text and
  trailing whitespace. Markdown LSP was requested for both; no `.md` server is
  configured, so no Markdown diagnostic pass is claimed.

Retained lead/independent evidence, **not rerun in this review**:

- Final chooser delta: **55 passed**; independent runtime: **170 passed**.
- Lead UI build: **exit 0**, TypeScript + Vite, 1,868 modules, 2.33s, as recorded
  in `docs/PROJECT_LOCATION_REPORT_2026-09-06.md`.
- Final full Vitest run: **1,724 passed, 7 failed**, 3 failed / 169 passed files,
  97.77s, exit 1, as recorded in that implementation report. Failures: App signed
  updater (1); tauri focused-terminal `attentionInventory` payload expectations
  (2) and notification-probe `sound` argument (1); push same-origin task-link,
  denied-permission subscription and failed server-unsubscribe expectations (3).
  These are reported as the retained baseline, not newly established by this
  reviewer for every failing test. They prevent an all-suite-green claim, not a
  demonstrated new Local/Remote criterion failure.
- The additional notification timeout/missing permissions mock export was **not**
  dismissed as pre-existing. `runtime-full-suite.md` traces it to an unstable
  fixture callback exposed by reactive restoration. Its stable mock/deferred
  signal correction retains closed-target assertions. The related capture
  `runtime-notifications-related.log` records **47 passed**, with no unhandled
  errors reported in the follow-up receipt. The final whole-suite report no
  longer includes that extra failure.

## Separate native-QA limitation

**UNVERIFIED / NOT APPROVED.** The retained environment receipt records Orca
exiting before interaction, UI scripting disabled, Accessibility and Screen
Recording ungranted, and a supplemental WebView host spawn failure. No successful
native scenario or screenshots were captured. None were attempted here.

Still unobserved through the real debug app launched with exactly `bun tauri dev`:
initial Local/Remote chooser without an OS picker, explicit Local picker/cancel/
retry, native Settings -> SSH Machines navigation and remote addition with remote
terminal `pwd`, empty-host guidance, and native terminal surface occlusion. This
is an acceptance-evidence limitation separate from the **APPROVE code** verdict;
component mocks, source reads, builds and real backend SSH do not approve it.
