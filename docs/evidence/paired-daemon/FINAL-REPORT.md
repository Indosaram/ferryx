# FINAL REPORT — herdr cloud multi-host continuation

Session 01a092ed-229c-75a0-83ca-627b958c0b48 · 2026-09-13
Worktree: `orca-lite-wt/herdr-resume-01a097f8` (isolated from canonical and prior DAG worktrees)

## 1. What this session delivered

- Recovered the lost Wave2 A09 child context and ran the recovered 28-item backlog to
  completion as 24 implementation packets (A06-A24 plus baseline and batch repairs),
  preserving the verified 32-source Wave1 composition and the separately verified
  terminal wire codec (A10).
- Prior gate verdict SHIP WITH FIXES: all 4 blockers resolved; final gate re-review
  finds **no unresolved production-code blocker** (details in §3-§4).

## 2. Final aggregate verification (parent-run, exit codes verified)

- Rust full lib: **1034 passed / 0 failed / 1 ignored** (`--test-threads=1`, 266.65s,
  `RUST_EXIT=0`).
- UI vitest: **4485 passed / 3 failed** — the 3 are `push/client.test.ts`; git diff vs
  HEAD is empty for that directory, `ui/package.json`, and the vitest config, and the
  same 3 fail identically on the canonical tree: pre-existing, not caused by this work.
- UI build: `BUILD_EXIT=0`.
- paired_host suite: **36 passed / 0 failed** (trajectory 27 → 28 → 30 → 31 → 36, never
  red).
- `cargo check --lib` exit 0; `cargo check --bin ferryx-cli --bin ferryx-relay` exit 0.

## 3. Gate history — four blockers, all resolved

1. **Secret hygiene** → `project_remote_error()` (`paired_host/client.rs:128`): remote
   errors projected to allowlisted codes with locally-owned prose; a fixture bearer
   token echoed by a remote error cannot cross the native boundary. RED proves the
   token leaked; GREEN proves it does not. Evidence: `GATEFIX-client-*`.
2. **Capability negotiation fail-closed** → `terminalStreamV1` required before socket
   open (`client.rs:327`); a create-only peer receives zero socket attempts.
   Evidence: `GATEFIX-client-*`.
3. **Owner reaping on spontaneous termination** (EOF / remote exit / keepalive failure)
   in `terminal/paired_runtime.rs`, identity-fenced; stale captured sends fail closed.
   Evidence: `GATEFIX-owner-*`.
4. **Evidence integrity** → ledger corrections (single-use-ticket attribution moved to
   production code lines, AC10 downgraded to PARTIAL, `A23-GREEN.log` misnomer
   disclosed verbatim).

## 4. Post-gate regressions found by the parent's full-suite run, then fixed

- **Nested `block_on` panic** in `remote/workspace_api.rs` broke 15 pre-existing daemon
  tests (including the security tests). Fixed by moving the subprocess supervisor to a
  scoped worker with a private runtime; the production `run_blocking` boundary,
  cancellation, deadlines, and output limits preserved. Original pre-fix RED preserved
  as `REGRESSION-blockon-original-RED.log`; fix evidence `REGRESSION-blockon-*`.
- **Implicit Relay activation on pairing creation removed** (restores the "pairing
  persists, network starts Off" contract), hermetic env restoration in the
  runtime-path test, and a real private-daemon `DaemonClient` fixture for the IPC
  identity contract test. First development failure preserved. Evidence: `REGRESSION2-*`.

## 5. Final gate re-review (post-repair)

- Verdict: **no unresolved production-code blocker.** The three code blockers are
  confirmed resolved in code (`client.rs:128-162,324-327,403,442-445,622`;
  `paired_runtime.rs:25-42,60-95`). Report:
  `.omo/evidence/paired-daemon-rereview-gate-review.md`.
- The one REJECT finding (ledger summary still said AC10 "Met" while the body said
  PARTIAL) is fixed in `AC-LEDGER.md`.
- Accepted limitations: the plan file lives in the canonical checkout (reviewer read
  `PARENT-AUDIT-AC01-AC12.md` instead); `App.test.tsx:745` replaced direct update-check
  integration coverage with mocked polling lifecycle coverage (non-blocking; no
  `#[ignore]` anywhere in the session diff).

## 6. Acceptance status (AC01-AC12) — from AC-LEDGER.md, quoted honestly

- Met/partial with real evidence: **AC10 PARTIAL**, **AC02 / AC09 / AC11 in part**.
- Explicitly NOT met, by the packets' own words: **AC04, AC05, AC06, AC08, AC12**.
- Withheld pending human QA: **AC01, AC03, AC07, AC11**.
- Root cause of the ceiling: `pairedDaemonProxyV1` is deliberately **false** until the
  native proxy path is E2E-verified on a real desktop, and the live-PTY/Linux/Windows
  rehearsals are user-forbidden or user-only. **Full acceptance is NOT claimed.**

## 7. What only the user can do (manual QA — OS automation prohibited)

Launch the debug app from THIS worktree with exactly `bun tauri dev`. Then verify:
- **AC04**: native tabs, splits, focus, shortcuts, native menus.
- **AC01/AC03/AC11**: Add Project Local/SSH/Paired, browse without SSH; hosts coexist
  with separate collisions; offline inventory without empty-authority.
- **AC07**: no session loss on host switch / settings / renderer close / relay loss
  (relay loss needs a paired desktop).
- **AC08**: forced-relay functional pass — requires flipping `pairedDaemonProxyV1` to
  true ONLY after the paired desktop E2E passes. Do not flip before desktop proof.
- **AC05/AC06**: remote worktree create/delete owner-side checks; remote agent launch
  with correct host/pane attribution.
- **AC12**: rollback rehearsal; Linux/Windows frozen-backend rehearsals (Q4 items;
  maho-win WSL / a separate machine).
Full steps: `docs/evidence/paired-daemon/MANUAL-QA-CHECKLIST.md`.

## 8. Residual open tracks

Adjudicated closed (post-review, parent):
- **A11 relay PTY streams**: pre-existing duplex-tunnel transport + generation-bound
  stream test (`relay_server.rs:1073,2890`) pass in the final aggregate; preservation
  proven. Desktop-over-relay E2E stays in the AC08 bucket.
- **Q4 Linux readiness framing**: DONE — Linux capture run + newline markers applied
  (`Q4-linux-readiness-capture.log`, `machine_catalog_persistence.rs:47`).
- **Q4 native Windows platform failures**: DONE — RED→final2 arc, `2 passed; 0 failed`
  + `DEADLINE_PASSED` (`A10-input-windows-final2.log`).

Still open (user/machine-gated):
- **Human desktop QA** (§7).
- **Q4 frozen-backend rehearsals (2026-09-13, real hardware) — DONE**: extracted on
  omaki (Linux x86_64) and maho-win (Windows x86_64), ghostty pinned 6a508fd5.
  Windows: RED compile defect (`WebSocketUpgrade::on_upgrade` Send violation in the
  cfg(windows) input pump) fixed in `src-tauri/src/terminal/session.rs`
  (`write_input_slice` sync helper); green rerun `BUILD_EXIT=0`, `paired_host::` 33
  passed / 0 failed (`Q4-windows-*`, RED record `Q4-windows-rehearsal-RED.md`).
  Linux: `BUILD_EXIT=0`, headless boot smoke `SMOKE_READY=yes` (clean kill), full lib
  suite `--test-threads=1` **1009 passed / 3 failed / 1 ignored** — all 3
  environment-limited (no sshd on 127.0.0.1:22, no `omo` binary, no `xdg-open` on the
  headless bench), not product defects (`Q4-linux-rehearsal.log`); details in
  `AC-LEDGER.md` § "Q4 frozen-backend rehearsals". Final macOS aggregate with the
  fix: **1034 passed / 0 failed / 1 ignored** (264s, exit 0).
- **Commit risk**: `git status` is broken in this worktree (ghostty submodule symlink),
  so ~70 untracked files are invisible — including the entire `paired_host/` module,
  `machine_gateway.rs`, `machine_owner.rs`, `machine_peer.rs`,
  `session_metadata_*.rs`, `ipc/paired_host.rs`. Any commit MUST use explicit paths
  (`git add <paths>`, never `git add -u`). Nothing has been committed.

## 9. Cleanup & safety

- Owned QA temp roots removed (`/tmp/blockon-*`, `/tmp/fix3-*`); all monitors completed
  (exit 0); no stray cargo/test processes.
- Canonical daemon (PID 1010) + GUI (PID 680) untouched throughout (>9h45m continuous
  uptime); no PTY, desktop, or real-HOME access by any packet.
- Nothing committed; no release builds; `pairedDaemonProxyV1` stays false, pinned by
  `paired_host/process_tests.rs:121` and `native_operation_tests.rs:162`.
- All work is uncommitted in a shared tree — vulnerable to concurrent sessions. Commit
  on request, explicit paths only.

## 10. Evidence index

`docs/evidence/paired-daemon/`: `AC-LEDGER.md` (authoritative AC status),
`PARENT-AUDIT-AC01-AC12.md`, `MANUAL-QA-CHECKLIST.md`, `GATEFIX-client-*`,
`GATEFIX-owner-*`, `REGRESSION-blockon-original-RED.log`, `REGRESSION-blockon-*`,
`REGRESSION2-*`, and the A01-A24 packet reports/logs. Q4 rehearsal evidence:
`Q4-linux-rehearsal.log`, `Q4-windows-rehearsal.log`, `Q4-windows-build.out`,
`Q4-windows-test.out`, `Q4-windows-rehearsal-RED.md`.
