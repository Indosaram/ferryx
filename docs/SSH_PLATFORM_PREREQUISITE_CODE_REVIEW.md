# SSH platform prerequisite code review

Date: 2026-09-08
Reviewer: omo-senpi-gate-reviewer / st_01a080cd

recommendation: APPROVE (corrected prerequisite code delta only)
conclusion: APPROVE
blockers: []
validationStatus: Passed; final parent-owned isolated-tree receipts are recorded below

## Final parent verification

The user authorized the prerequisite scope. A private Git index separated the
prerequisite hunks from the picker and excluded release/package/scripts and the
unrelated agent-exit review. Its complete source tree was exported to an isolated
detached worktree; none of the picker modules were present during these checks.

- UI: 42 tests passed across SshSection, sshHosts and sshAgentState. UI build passed.
- Rust: 52 SSH library tests and 4 remote-project registration tests passed.
- `cargo check` passed.
- Actual Windows endpoint `maho-win`: 3 opt-in integration tests passed in 27.07
  seconds, including real PTY operation and the isolated integration path.
- Actual Linux endpoint `omarchy`: 1 opt-in real PTY test passed in 0.87 seconds.
- The Rust test/check/live command chain exited 0. UI test/build chain exited 0.
- A headless browser rendered the isolated tree's actual SshSection. After a
  successful fixture connection test, opening Edit disabled both Test and Prepare.
  Screenshot: `docs/evidence/ssh-prereq-edit-guards.png`.
- The deterministic production POSIX installer test first failed on false success;
  the corrected failure and success tests passed. Deferred UI tests first exposed
  all four stale connection/integration completion paths and then passed.

Two initial isolated-build attempts failed because checkout-index recreated the
empty Ghostty submodule directory over a test symlink. Export now excludes
gitlinks, and the corrected fixture passed all checks above. This was verification
setup, not a production source failure.

The independent reviewer owns the code approval; these execution receipts are
parent-owned. No desktop automation, daemon restart, release build or deployment
was performed. The documented manual desktop and platform coverage limits remain.

## originalIntent

Fix the Windows PowerShell SSH prerequisite failure rather than merely replacing `true`: retain system OpenSSH, detect the actual remote executor, register remote paths, launch usable terminals, transfer attachments, and optionally report agent state. Preserve existing POSIX identities and require explicit consent for extension installation. The user authorized separate prerequisite and directory-picker commits, excluding unrelated release/MSIX/scripts/package.json changes.

## desiredOutcome

The user can Test a Windows or POSIX host without remote installation, register its real directory, open a correctly located remote terminal, and explicitly prepare agent integration with truthful success/failure feedback. Host changes must not leave runtime results describing the previous connection.

## userOutcomeReview

The corrected prerequisite code resolves both concrete defects identified in the initial review: stale host completion state and masked POSIX installation failure. No remaining code blocker was found in this focused delta. This is code approval, not certification of the parent's pending isolated-tree, build, live, or manual validation. No picker-specific rejection is introduced.

Criterion identifiers below name requirements explicitly present in the supplied design/implementation documents; they are review labels, not identifiers supplied by a loop plan.

## Historical findings (resolved by focused re-review)

The observations below describe the rejected implementation, not the current corrected code.

### P2: stale environment is restored after editing an in-flight tested host

- violatedCriterion: C-HOST — invalidate detection on connection configuration changes and do not retain results obtained during a host change (`docs/SSH_REMOTE_PLATFORM_DESIGN.md`, environment detection section).
- evidencePointer: `ui/src/components/settings/SshSection.tsx:223-258,339-360,999-1001`.
- Observation: Edit first opens the form, but the row's Test button is still enabled while that form is open. Start Test against the old host, save the edited hostname while Test is pending, then resolve the old Test. Save deletes the cached result, but `handleTestHost` unconditionally writes the old response back under the unchanged host ID. The replacement hostname is now accompanied by the old platform/executor/Git capabilities and a Connection verified state. The Prepare button is also available while the edit form is open, allowing its old-host completion to restore an installed indicator after the edit cleared it.
- Minimal fix: prevent Test and Prepare from starting while the edit form is open, and reject stale completions against the current host configuration/generation (including inventory updates from other consumers). Apply the same guard to preparation success. Add one deterministic deferred-response regression exercising an actual edit/save between request and response; assert the replacement host never acquires the old runtime/prepared status.
- Evidence strength: direct control-flow/UI-enable-state inspection; not a claimed executed UI regression. Existing newly added tests await an immediately resolved Test and never overlap an edit.

### P2: POSIX integration can report success after a failed installation copy

- violatedCriterion: C-CONSENT-RESULT — explicit extension preparation must expose its result rather than hide failures (`docs/SSH_REMOTE_PLATFORM_DESIGN.md`, extension preparation and user-visible state sections; implementation promises explicit preparation).
- evidencePointer: `src-tauri/src/ssh/operations.rs:140-147`; `ui/src/components/settings/SshSection.tsx:372-379`.
- Observation: `cp "$tmp" "$stage" && mv ...` places `cp` on the left side of `&&`, where POSIX `set -e` does not terminate execution. If the first target's copy fails but a later target succeeds, the script exits zero. The IPC resolves successfully and Settings says the extension is installed, although the primary `.omo` extension was not updated. A failed copy can also leave the per-target stage file behind.
- Reproduction performed: extracted the exact new POSIX installer string from `operations.rs` and executed it with `/bin/sh`; shell-function boundary substitutes discarded writes via `/dev/null`, failed only the first `cp`, and allowed subsequent copies. Observed exit **0**, stderr **`FIRST EXTENSION COPY FAILED\n`**. No project files or remote endpoints were modified. An earlier all-copies-fail run exited 1, confirming the defect specifically requires a later successful target rather than every error being ignored.
- Minimal fix: make copy failure terminate explicitly (or execute `cp` and `mv` as separate `set -e` commands), with stage cleanup on failure. Test the production installer path with a deterministic first-target failure and assert an error instead of installation success.

## Scope and adversarial review

| Area | Direct inspection result |
| --- | --- |
| Executor detection | Windows executors precede sh; Windows OS is checked rather than inferring from pwsh's presence; supported POSIX uname values are explicit. Detection uses a shared deadline, a request nonce, strict field count/UTF-8/capability checks, and validates returned home/temp paths. Exit 255 is not classified by localized prose. |
| Quoting/path validation | POSIX scripts enter explicit sh and use single-quote escaping; PowerShell data is UTF-8 Base64 inside UTF-16LE encoded bootstrap commands. Windows Git arguments use native Windows argument quoting. Drive-relative and device paths are rejected; no lowercasing or local canonicalization of remote paths is introduced. Real UNC behavior remains unverified. |
| Transport/auth | Existing saved port/key/jump options and strict host-key/noninteractive policy are retained. Automated commands now explicitly use `-T`; child output is bounded and timeout termination reaps the child. Host-store mutation serialization and narrow registration check/persist races already exist in HEAD, rather than being newly fixed by this delta. |
| Host/config races | Registration rechecks the host after network probing. The corrected Settings state invalidates changed connection configurations and rejects stale completions; see focused re-review below. |
| Consent | No extension installation remains in Test, project registration, or terminal spawn. The new explicit IPC must be included in the prerequisite `lib.rs` hunk. The corrected POSIX installer propagates copy failure and cleans the pending stage; see focused re-review below. Starting an ephemeral state relay is not installation. |
| Identity | Optional platform serialization preserves legacy records and the existing host/path hash. Terminal startup detects again and rejects a stored/detected platform mismatch. Windows reports require both token and session ID; token is removed before local forwarding, and daemon parsing validates supported state/provider identity. |
| Process lifetime | Windows bridge owns its SSH child with kill-on-drop; remote relay observes the sshd ancestor's exit and bounds TCP report reads. Follow observes actual terminal output-channel closure. Relay failure logs a warning and does not reject terminal spawn. Live lifetime test checks process exit, not a fixed sleep. |
| Windows launch | Explicit selected PowerShell starts a PTY, sets session/endpoint variables and literal CWD; bootstrap preferences are scoped in an invoked script block. Profiles are not disabled for interactive startup. Actual profile/CWD behavior is only parent-reported, not executed here. |
| Errors/fallback | Nonzero command status preserves exit code, optional UTF-8 stderr, Base64 bytes, and operation stages. Parse errors fail closed. Some inherited spawn/output-limit errors still lack detailed stage fields (NOTE, not a new blocker). POSIX on Windows-local clients gracefully lacks Unix state forwarding; no local filesystem fallback is introduced. |
| Remote/worktree dependencies | `ipc/ssh.rs` worktree commands already return Unsupported. `ipc/project_remote_tests.rs` checks local Git/reveal boundaries. `ssh/worktree.rs` command generators are not newly wired to execution. `operations::git` is used by the live fixture, while actual product Git root/origin queries are implemented in probe. No unrelated worktree migration is required for this prerequisite commit. |

## remove-ai-slops / programming perspective

The named skill files were not found in the searched local skill/config directories (`~/.agents`, `~/.omo`, `~/.config`, and conventional skill paths). Their prompt-documented criteria were applied directly to the diff, production files, and tests.

- No newly added deletion-only, requested-removal-only, tautological, or prose-pinning test found. The explicit-consent test also verifies runtime data and the actual preparation IPC, not merely an absent call.
- `-T` argv assertions test machine-consumed transport policy; parser and authentication tests exercise untrusted data boundaries. They are not excessive abstractions or normalization.
- The TCP extension test uses a real loopback socket and subscribes before triggering session_start. New live PTY tests use event/output signals with bounded timeouts rather than sleeps. The bridge exit wait tests lifetime itself.
- Maintenance/false-confidence NOTE: `direct_tests.rs:146-193,279-303` still exercise the old `probe_command`/`parse_probe` and `install_remote_extension_script`, whose only callers found are tests. Production now executes different scripts in `operations.rs`. Existing suite counts therefore overstate regression coverage of the new POSIX implementation. Port relevant behavior assertions to the production path; do not add tests that merely prove the old helpers were removed. This contributed to the untested failure mode in C-CONSENT-RESULT but is not independently a blocker.
- New operation/runtime separation is justified by the actual two-platform boundary. No unnecessary generic RPC layer, parsing framework, or path normalization was introduced.
- No demand for unrelated architecture cleanup, release changes, or currently unsupported worktree features.

The supplied `docs/SSH_DIRECTORY_PICKER_CODE_REVIEW.md` explicitly covers both named skill perspectives, including excessive/useless tests, deletion-only/requested-removal tests, tautology, source-text/implementation mirroring, timing, and unnecessary extraction/normalization. Its coverage is picker-only and does not certify these prerequisites. This report supplies the direct prerequisite skill-perspective review; no independent prerequisite code review was supplied.

## Checked artifact paths

- `docs/SSH_REMOTE_PLATFORM_DESIGN.md`
- `docs/SSH_REMOTE_PLATFORM_IMPLEMENTATION.md`
- `docs/SSH_DIRECTORY_PICKER_CODE_REVIEW.md`
- `docs/evidence/ssh-platform-desktop.png`
- `docs/evidence/ssh-platform-mobile.png`
- `src-tauri/src/ssh/runtime.rs`
- `src-tauri/src/ssh/operations.rs`
- `src-tauri/src/ssh/state_bridge.rs`
- `src-tauri/src/ssh/direct.rs`, `direct_tests.rs`
- `src-tauri/src/ssh/projects.rs`, `projects_tests.rs`
- `src-tauri/src/ssh/exec.rs`, `worktree.rs`, `mod.rs`
- `src-tauri/src/ipc/ssh.rs`
- `src-tauri/src/ipc/project_remote.rs`, `project_remote_tests.rs`
- `src-tauri/src/daemon/server.rs` (delta, state parser/listener, lifecycle integration)
- `src-tauri/src/terminal/service.rs`
- `src-tauri/src/lib.rs` (mixed IPC registration diff)
- `src-tauri/resources/agent-extensions/ferryx-agent-state.ts`
- `src-tauri/tests/ssh_windows_live.rs`, `ssh_posix_live.rs`
- `ui/src/lib/sshHosts.ts`, `sshHosts.test.tsx` (relevant inventory/wrapper tests)
- `ui/src/lib/sshAgentState.test.ts`
- `ui/src/components/settings/SshSection.tsx`, `SshSection.test.tsx` (platform tests/delta)

Picker-specific implementations and unrelated release changes were excluded; status and diff scope were inspected against HEAD. The prerequisite must include the `cmd_ssh_prepare_integration` registration in `lib.rs`, but not the adjacent list-directories registration. Mixed `ssh/mod.rs`, `ipc/project_remote.rs`, and SshSection hunks require splitting rather than whole-file staging.

## Verification and exact evidence gaps

Directly performed: scoped source/diff inspection, referenced document/test inspection, both screenshot inspections (runtime and consent control visibly present), local deterministic POSIX installer failure reproduction, and `git diff --check` (passed).

Not performed or claimed: broad builds, live SSH endpoints, GUI launch/automation, daemon restart, release build, isolated commit-tree verification, staging, or commits. Parent explicitly owns those checks. No implementation file was edited.

The implementation report's claimed Rust/UI/live passes are prose, not independently reproduced validation receipts. Its screenshot artifacts exist and were inspected; they do not establish live IPC or real terminal behavior. The provided picker review includes parent-reported counts and live browse receipts, not prerequisite execution logs. No separate prerequisite manual QA matrix or notepad path was supplied. The implementation document explicitly leaves user desktop testing, UNC, cmd-default SSH, WSL endpoint, and Windows-local client coverage incomplete. macOS-remote execution, resize/new-tab/split/reconnect and adversarial failure-stage receipts were not supplied either. These are recorded as evidence gaps, not additional code blockers for this expressly read-only prerequisite review; approval after focused fixes is still not a claim of completed parent/manual validation.

`omo-agent-toolkit ulw-loop status --json` returned `ULW_LOOP_PLAN_MISSING` for this child. Per the task's more specific only-allowed-write instruction, this report is written only to `docs/SSH_PLATFORM_PREREQUISITE_CODE_REVIEW.md`, not to an additional fallback artifact.


## Focused re-review of completed fixes

Both original criteria are satisfied by the inspected corrected code. No implementation file was edited and no broad suite was run.

### C-HOST resolved

Evidence: `ui/src/components/settings/SshSection.tsx:82-137,367-413,994,1029`; `ui/src/components/settings/SshSection.test.tsx:116-187`.

- `sameConnection` compares the actual primitive connection configuration, not object identity, label, or import source. Inventory changes clear stale test/prepared state, including host removal and replacement under the same ID.
- Both Test and Prepare check current configuration before success and error state writes. Buttons and handlers prohibit starting these operations while the edit/add form is open.
- Deferred regressions drive the real shared `updateSshHost` API between request and completion, rather than manipulating React internals. They cover late success and failure for both operations, plus disabled controls during editing. The integration-success case tests again on the replacement host so that a wrongly retained prepared flag would become visible and fail the assertion.
- The originally described Edit-first interleaving is closed by disabling Test/Prepare during editing. Opening Edit after starting Test was already prohibited, so the corrected shared-inventory tests exercise a legitimate separate update path rather than relying on that impossible sequence.

### C-CONSENT-RESULT resolved

Evidence: `src-tauri/src/ssh/operations.rs:133-141,187-243`.

- `cp` and `mv` are standalone commands under `set -e`; copy failure terminates before replacement or later targets.
- The EXIT trap removes both the input temporary file and any current target staging file. Successful moves clear the stage variable.
- The two new shell tests execute the exact constant used by production preparation. The failing-copy fixture leaves a later `.pi` target available to reproduce the original masked-error case, asserts nonzero status, preserves original target bytes, and checks no staging residue or later installation. The success case checks actual bytes and clean directories for all present agent locations.
- Directly reran the earlier no-filesystem-write shell-boundary reproduction against the corrected production constant: first-copy failure now returns **17**, with stderr **`FIRST EXTENSION COPY FAILED\n`**, rather than the previously reproduced zero. `git diff --check` also passed during this focused re-review.

### Direct remove-ai-slops / programming pass on corrections

The documented criteria were applied directly again; the previously supplied picker report explicitly covers the same skill perspectives but is not substituted for this prerequisite check.

The private installer constant is necessary to test the actual production script, not unnecessary extraction. Its tests cover filesystem results and failure propagation, not copied script text or implementation shape. The deferred UI cases exercise different observable success/error channels and use the actual inventory subscription path. No fixed sleeps, polling, deletion-only tests, requested-removal-only tests, tautological assertions, prose-pinning tests, unnecessary normalization, or generic production abstraction was added. No new scope drift or maintenance-burden blocker was found. The historical note about old direct helper tests remains nonblocking; the corrected installer now has production-path coverage.

### Focused verification limits

Parent reports both shell regressions GREEN (2/2) after a RED reproduction, and reports RED results for the four stale UI completion cases. Those test-command receipts were not independently loaded or rerun here. The UI GREEN run was still in progress in the supplied update. This review directly verified the code and test artifacts plus the corrected shell-control-flow reproduction; it does not claim pending UI tests, broad Rust/UI suites, builds, exact private-index tree validation, or live endpoints passed. Parent owns those checks. Existing manual/platform evidence gaps remain recorded above.
