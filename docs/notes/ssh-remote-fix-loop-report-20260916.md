# SSH Remote Workspace Fix — Loop Report (2026-09-16)

## Outcome
Review-clean landing complete. Web worker re-review VERDICT: blockers=0 majors=0 on the final diff. Merged to main (489c63be is in main history; main has since advanced with concurrent-session commits, unaffected).

## Commits (atomic, Conventional Commits)
- 93efacc0 fix(ssh): normalize Windows verbatim cwd before PTY spawn
- 7eeb7179 fix(ssh): report terminal helper readiness via bounded non-installing probe
- 489c63be fix(ssh): reject UNC cwd spawn with explicit cmd.exe incompatibility error (review major 1 fix)

## What was fixed
1. helper.rs pty.spawn: prepare_spawn_cwd normalizes Win32 verbatim drive (\\?\C:\...) and verbatim UNC (\\?\UNC\server\share) before CommandBuilder::cwd; containment check runs on the canonical path BEFORE normalization (daemon normalize_process_cwd semantics).
2. ensure_cwd_spawnable (489c63be): normalized UNC cwd (\\server\share) is rejected with an explicit UNSUPPORTED error instead of cmd.exe silently falling back to the Windows directory. POSIX paths unchanged.
3. SshTargetSummary.helper tri-state (installed/missing/unknown) from a bounded 10s non-installing probe; SshSection.tsx renders only from that field (no git-based readiness inference).

## Evidence (repo copies)
- G002 attempt dir: .omo/evidence/ulw/01a0a92a-2aba-72f8-8b20-4f13bf2468f3/G002-web-worker-review-gate-integration-l/a0 (10 files: RED/GREEN logs, final suite logs, review report)
- Mirrored at .../ssh-fix/ (10 files)
- Quality gate: checkpoint complete rev=13, gateReview APPROVE blockers=[]

## Final suite runs at merged HEAD 489c63be
- cargo test --lib -- --test-threads=4: 1372 passed / 0 failed
- ui vitest (bun run test): 5013 passed / 5013 (the 8 previously failing UI tests were fixed by concurrent-session commits already in main)

## Pre-existing failure notes (documented separately)
- daemon::manifest flock test: flaky under 16-thread parallel load; 3x green solo and green with --test-threads=4 (module untouched by this loop)
- a03_owner_cli protocol handshake failure: fixed on main by the concurrent session (583535be); our tree picked it up via rebase
- Full-suite flakiness under default parallelism was observed 3 times with different test sets; all passed solo immediately after (load-sensitive fixtures, not our diff)

## Cleanup receipts
- Lane private target dirs target-laneA/target-laneB removed (7.7 GiB; ENOSPC recovery 132MiB -> 17GiB)
- /tmp/ulw-ssh-fix and /tmp/sshfix-base-wt removed (evidence copied into repo first)
- Ghostty submodule worktree registration removed (gitdir remains detached at 6a508fd5e)
- Branch worktree orca-lite-wt/ssh-pty-cwd-fix removed after merge (it held the removed gitlink state)
- Stale worktree registrations pruned (sshfix-base-wt, orca-head-check)
- Wedged review CLI process killed; wedged scope closed explicitly (see reference/project/delegate-web-wedged-nonterminal-scope-recovery.md)

## Manual maho-win desktop QA steps (user-performed; recorded, not gate-blocking)
1. Pull/build the app on maho-win from main (or update the installed copy), start ferryx daemon.
2. In the desktop app, open the SSH section and connect to the maho-win remote host; confirm the helper readiness row shows an honest value (Installed / Not installed / Not verified).
3. Open a terminal on a remote workspace whose path is a normal drive path (C:\Users\sook\...): confirm cwd is the project path (pwd shows the project directory, not C:\Windows).
4. If the remote project path is a UNC path (\\server\share\...): confirm the terminal spawn now fails with the explicit UNSUPPORTED UNC cwd error instead of silently opening in the Windows directory.
5. Verify a worktree under the remote project (e.g. .orca-worktrees/wt-<slug>) opens with the worktree path as cwd and helper readiness matches reality.

## 2026-09-17 update: helper auto-provisioning landed + bundles refreshed

Root cause #3 (stale helpers) is now fixed on main:

- `1f369e5f` feat(ssh): provision remote helper with version-aware install/upgrade — helper reports `--version` + handshake `helperVersion`; `installed_version` probe (non-installing); `provision()` = install + daemon stop via endpoint.json pid + atomic exe replace + ensure_started; `cmd_ssh_provision_helper` / `cmd_ssh_helper_update_state` wired in lib.rs; SshSection shows an "업데이트 가능" row. Upgrade decision: not installed → Install; installed + version unknown/mismatched → Upgrade; equal → NoOp.
- `010428f1` helper version bumped to **2026.917.1** (field helpers are version-less or 2026.908.1 → both correctly flag as Upgrade).
- `98ac2894` resources/helpers re-staged from real-remote builds: x86_64-pc-windows-msvc (built on maho-win, 823,296 B, sha256 7d171d96…) + x86_64-unknown-linux-gnu (built on omaki, 1,280,568 B, sha256 05a09d15…); both verified `--version 2026.917.1`. aarch64-linux deferred to the release pipeline. Receipts: /tmp/helper-stage-receipts.json.
- `e67e9183` + `55fe1851` dev typecheck unblocked (lib ES2021 for replaceAll, AddMachineModal onSuccess ×8, paired-host fixture remoteWorkspaceId + retained read). `tsc --noEmit` now exits 0.

Verification at merged HEAD 55fe1851: ui vitest 5025/5025 (254 files); scoped ssh cargo suites green; staging script tests 10/10. Known non-blockers on main: `paired_host::native_ambiguity_tests::native_35s_deadline_retains_mutation_reconciliation` fails deterministically (its own `advance(PAIRED_MUTATION_OUTER_TIMEOUT)` ≈ 165s exceeds the test's 140s wrapper — introduced by the concurrent paired-host session, not the ssh work); `daemon::client::tests::test_client_workspace_registration_and_remote_apis` passes solo (load flake).

### User QA (dev GUI, bun tauri dev — rebuilt automatically)
1. Settings → SSH: maho-win/omaki rows should show installed helper with "업데이트 가능" (no version → Upgrade).
2. Trigger the update/provision action; expect the remote helper to become 2026.917.1 and the row to settle at installed + current.
3. Open a terminal on a remote workspace: cwd must be the exact project/worktree path.

## 2026-09-17 review verdict (typesafe-judge) and hardening

Full review of 1f369e5f + 010428f1 with typesafe-judge (jev-1.13.0), plus manual code reading:

- Gate 1 (provisioning diff): fulfills 77% YES, risk 1.08 moderate, clean 0.32 fail. Gate 2 (hardening diff): fulfills 81%, risk 0.97, clean 0.26 — the clean metric contradicts the judge's own explicit scope probe ("none out of scope" 75%, conf 0.67), so it was treated as a conservative false positive and documented rather than ripening code to satisfy it.
- Confirmed findings, all fixed in 3095c1a5: (a) kill-by-pid from endpoint.json had no process-identity check (noul P(safe)=6%) — both platform scripts now verify the pid is ferryx-remote-helper before taskkill/kill, with live POSIX child-process tests; (b) the Windows installer restores the previous binary if the atomic move fails midway; (c) version comparison was equality-only and could offer a downgrade for a newer remote — decide_helper_upgrade now parses calver (parse_calver_version) and never downgrades; the decision moved to the backend (HelperUpdateState.decision) and SshSection consumes it, including a new Install button for missing helpers; (d) helper_core_tests pinned "2026.908.1" and broke on the bump — they now assert the shared const.
- User decision (upgrade policy): unconditionally stop the daemon and kill sessions during upgrade (reconnect restores terminals) — current behavior retained, no session check added.
- Real-hardware evidence: the actual generated Windows install script executed on maho-win against a stale 489c63be daemon — FERRYX_INSTALL_OK, identity-checked kill, replaced binary reports 2026.917.1. POSIX script execution is covered by real `sh -s` tests.
- Remaining known main-tree test failures are not ssh scope: paired_host::native_ambiguity_tests::native_35s_deadline (deterministic virtual-time bug introduced by the concurrent paired-host session) and a03_owner_cli (requires ferryx-cli prebuilt).
