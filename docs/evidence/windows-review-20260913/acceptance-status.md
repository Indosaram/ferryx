# Windows remediation acceptance status

2026-09-13. INCOMPLETE. This is a requirement-to-evidence checklist, not a
final gate or a passing runtime report. The loop ledger remains the notepad.
Latest observed HEAD: `da6eec06d65551f67bbc43f09910cde470c3478d`;
changed by another session, not a commit from this task. Earlier source
snapshots remain historical and need final changed-source qualification.

## C1: PR dispositions and real input/session behavior

- PR #2 diff and review threads: inspected; initial evidence in
  `lead-baseline.md`, source findings in `audit-native-input.md`.
  Head `79b02ab6ea4753bf87080dd567368a870c018057`.
- PR #3 diff and review threads: inspected; initial evidence in
  `lead-baseline.md`, source findings in `audit-ui-interactions.md`.
  Head `99c7086b61d7a590530fb8df924e34ce9e91e90b`.
- Both remain OPEN at the last external-change observation. Empty review
  threads/check rollups are not approval or passing checks.
- [ ] Production-seam mutation reproduces each old behavior before repairs.
  P05 optional-kind close has lead original-guard mutation (14 intended
  failures), 128 App GREEN and eight real-store survivor cases (58 total
  GREEN, eight intended whole-tab mutation failures). P01 remains missing.
  See wheel-close-lead-verification.md; no Windows PTY claim is implied.
- [ ] Win32 hit testing reaches WebView2 while native pixels remain visible.
  Direct WindowFromPoint assertions alone do not prove DOM drag selection.
- [ ] Real drag selects a known sentinel through pointer down/move/up.
- [ ] Tagged/untagged, pinned/unpinned, split/unsplit close matrix covers
  Windows keyboard and native-menu actions, preserving confirmation policy.
  Mounted App Ctrl+W/native callback matrix passes locally; actual Windows
  keyboard/menu and PTY lifetime matrix is still pending.
- [ ] Selected PTY exits; sibling responds again with the SAME PID/session.
- [ ] Final `gh pr view 2 --json state,mergeCommit` and
  `gh pr view 3 --json state,mergeCommit` match audited dispositions.
- [ ] Consolidated `pr-review.md` links the actual regression, GUI action,
  screenshot and PTY receipts. The file now contains the directly reread
  full patch/source assessment and 2026-09-13T02:30:53Z GitHub observation:
  both OPEN, unchanged heads, zero review threads and no check rollups.
  Execution and final disposition receipts remain missing.

## C2: exhaustive review and wheel behavior

- Historical review: `gap-history.md` accounts for all 146 original IDs
  from `docs/CROSS_PLATFORM_ISSUE_AUDIT_2026-09-07.md`, independently counted.
  Dispositions C42/K10/P4/R36/N21/U33 are not unique fix counts.
- Source census: `inventory-reconciled.json` contains 1,854 index paths;
  lead recomputed exact index SHA256
  `b5c0ab6db7f9e928b5b5133a4adb066b5d7baa88f2745ffec3e80bd963e926c8`,
  no missing/extra paths, 362 candidates and 342 unique file-ownership sites.
  These sites are NOT AST branch counts.
- [x] Finish the 59 explicit pending source receipts: image additions 22,
  native UI fixtures 11, shared UI paths 18, tooling candidates 8.
  All four closure reports were read, all 59 source hashes checked and new
  mechanisms directly reopened. No missing or duplicate assigned paths.
  See coverage.md for snapshot and changed-source limits.
- [x] Finish the additional 25 bounded-source receipts:
  `bounded-infrastructure.md` (14) and `bounded-ui.md` (11).
  Lead verification and pre-fix registration are recorded in ledger entries
  at 2026-09-13T02:41:53.860Z and 2026-09-13T02:42:14.703Z.
  P31 sortable identity, P28 Opera identification and P32 prototype history
  remain source-confirmed repair obligations, not executed regressions.
- [x] Reconcile the seven explicitly assigned shared native source/test
  paths in `shared-native-callers.md`; accepted lead verification is linked
  from `coverage.md` and C002's 2026-09-13T02:59:53.728Z registration.
  P33 menu ownership remains pending. P34 now has source repair, actual
  local RED/GREEN and real coordinator library integration with a rejected
  stale-snapshot mutation; see red-green.md. Its App wiring and real
  Windows scenarios remain unverified.
  This bounded receipt does not certify other shared noncandidate paths.
- [ ] Reconcile relevant shared callers, ancestor/negative platform
  branches, exclusions and the final changed-source delta into `coverage.md`.
  A matching index or completed inventory worker cannot satisfy this.
- [x] Consolidate verified source findings and aliases in `findings.md`.
  Its follow-up register links every accepted finding family, packet and
  explicit exclusion. This does not complete their repair/evidence register.
- [ ] Before EACH fix, register literal invocation and binary acceptance
  in loop criteria. C002 now includes the current repair-packets/addendum
  and eight supporting report SHA256 snapshots, accepted by the loop CLI
  with exit 0. The original scenario is preserved verbatim as a prefix.
  Every new invocation, fixture name or changed assertion must still be
  registered before its repair. No criterion has passed.
  Subsequent C002 additions also bind the 25-path and seven-path findings
  to their exact report/addendum snapshots; earlier hashes remain historical
  registrations and must not be silently replaced with newer file hashes.
- [ ] Each confirmed defect has identical-assertion RED/GREEN, minimal
  production repair and faithful affected-surface proof.
- [ ] Launch current-source Windows debug GUI only with `bun tauri dev`
  in an owned isolated interactive session on `maho-win`.
- [ ] Generate 200 numbered scrollback lines; native SendInput +120 and
  -120 over terminal content move viewport in opposite directions.
- [ ] Alternate-screen tracking produces exact intended application bytes;
  Shift and primary-history behavior are distinct; sibling pane is unchanged.
- [ ] Capture screenshots, native/DOM/IPC input logs, viewport/PTY state,
  source-tree and binary identity. Existing source analysis of wheel.rs
  sign handling is not this runtime evidence.

## C3: regressions, combined checks and preservation

- [ ] Real Windows keyboard input, fast drag selection and focused/split
  pane close pass through the actual debug application's surface.
- [ ] Shell menu selects Command Prompt and real PTY output includes
  `FERRYX_WIN_CMD_OK`, with recorded cwd and shell identity.
- [ ] Resize/DPI and browser/native pane isolation pass visually and through
  recorded geometry, focus and session state.
- [ ] Real daemon PTY cwd/output, reconnect and session survival pass.
- [ ] Targeted tests, `bun run --cwd ui test`, `bun run --cwd ui build`,
  native Windows backend build and required library/integration checks
  have captured exit 0 on the combined repaired tree.
  Local TRANSPORT-TEST-01 has RED, corrected-oracle GREEN (8 passed) and
  two rejected mapper mutations. P34 has same-assertion RED/GREEN, expanded
  integration GREEN (50 passed) and a rejected stale-snapshot mutation
  (3 failed, 47 passed). See `red-green.md`; neither proves aggregate or
  native Windows acceptance.
  P04/P28 were then independently source-reviewed and old-source mutated.
  The combined six-file run passed all 156 tests; tsc/Vite build exited 0
  with an unsuppressed 500 kB chunk warning. Exact receipts are in
  combined-local-verification.md. This is not the full suite or native build.
- [ ] Unsafe fixture targets are isolated before execution; zero selected
  tests, skipped prerequisites, unrelated compile errors and stale binaries
  never count as intended RED or GREEN.
- [ ] No tests disabled/weakened, errors suppressed or timing-luck passes.
- [ ] Existing user daemons and foreign uncommitted files remain preserved;
  compare current identities and ownership receipts at runtime boundaries.
- [ ] All QA-owned processes, tasks, ports and files are cleaned with
  explicit receipts. Do not remove borrowed resources.

## C4: gate, atomic commits and remote delivery

- [ ] Freeze the final source tree and independently verify all requirements
  against actual artifacts, not worker summaries or manifest counts.
- [ ] Required loop gate approves current evidence, including cleanup.
- [ ] Verified increments are atomic commits; no source commit has been
  created by this session.
- [ ] `git push origin <verified-sha>:main` succeeds only after acceptance.
- [ ] `git ls-remote origin refs/heads/main` equals that exact reviewed SHA.
- [ ] Both PR dispositions and applicable CI checks are captured. Current
  workflow is PR-triggered; absence of a push run is not a green CI result.
- [ ] `final-audit.md` and delivery receipts bind all results to that tree.
  It is not yet present. `red-green.md` contains local transport and P34
  RED/GREEN and mutation receipts. `cleanup.md` now records the completed
  local tests and four deleted temporary configs, not Windows cleanup.
  Proposed `runtime.md`, `gate-review.md` and `push-receipt.md` are still absent;
  those names are targets, not evidence of executed work.
- [ ] Only after every requirement is proved: complete loop/native goal and
  report actual final elapsed time and token accounting.

## Immediate dependency

The bounded source audits and follow-up criterion registration are done.
Local transport validator, P34 focus, P04 shortcuts/hints and P28 Opera
repairs have the independently verified bounded evidence above.
P21 permissions advice, P07 toast visibility and P16 command registration
also have independent original-behavior failures, combined ten-file GREEN
(210 tests) and successful tsc/Vite build; see combined-local-verification.md.
P22 has direct lead 13-case TCP GREEN, independently rejected original
boundaries and an additional cleanup RED-to-GREEN repair (16 final TCP cases).
See p22-local-repair.md for actual output counts, cleanup and LSP timeout.
None of these local checks proves current Windows GUI or ConPTY behavior.
P31 qualified sortable identity and P28's deterministic pairing validator
have now passed direct lead inspection, mutation rejection and combined
12-file verification (222 tests), followed by successful tsc/Vite build.
See combined-local-verification.md for exact commands and cleanup.
P02 frontend normalization and P05 PR3 optional-kind close now have independent
lead original-behavior rejection and combined 15-file GREEN (575 cases) plus
successful tsc/Vite build. Eight real-store survivor cases are separate from
the 28 App wiring cases, not actual PTY survival proof. Lead repaired the
existing invalid App updater-startup test at the real plugin/runtime boundary.
See wheel-close-lead-verification.md. Native HWND delivery, backend cell/
modifier propagation and Windows close/runtime remain unverified.
P27 remote wheel normalization has 58 child GREEN cases and independent
lead original-handler mutation (17 intended failures / 41 passes).
See p27-wheel-normalization.md. P28 request-lifetime and P05 browser-targeting
have returned. Lead verified their diffs, failure evidence and downstream
fixture adaptations; 24-file combined run passed 722 cases and tsc/Vite
build exited 0. See remote-browser-combined-verification.md.
P28 RC-01/02 also has independent seven-failure historical mutation.
P05 WIN-UI-13 has actual mounted sibling-target isolation, not live WebView2.
Cross-worktree target construction now has three intended original failures,
15 child passes and lead five-file/130-case combined selection GREEN, with
clean diagnostics. See p28-target-worktree.md. Current Windows target/socket
acceptance remains pending; subsequent combined build exited 0.
P27 preferences, P32 scoped-history ancestry and P33 native menu ownership
have returned. Lead read their diffs and original/final failure evidence.
Remote/selection/settings/IPC combined passed 201 cases; menu and all
row/tab caller suites passed 120 after strict stale-fixture corrections.
TypeScript/Vite build exited 0. See preferences-menu-combined-verification.md.
P32 has unchanged-oracle local RED/GREEN, not adjacent/native Windows proof;
exact branch inventory (st_01a0997f) remains active and read-only.
P03 key encoding passed local RED in bash_57 (exit 101, 2 intended failures)
and GREEN in bash_59 (exit 0, 7 passes) with Ghostty ABI constants added;
see p03-key-encoding.md. Physical key input remains unverified.
P11 SSH store preservation passed local RED in bash_56 (exit 101, 1 intended failure)
and GREEN in bash_58 (exit 0, 13 passes) with a fallible JSON loader;
see p11-store-preservation.md. Native deny-read handle remains unverified.
P15 offline LAN production adapter repair completed under st_01a099e2 in
state.rs and Cargo.toml; runner GREEN passed primary and companion tests
(exit 0, full receipts in p15-red/ and p15-green/). Four native adapter
tests and real offline LAN reachability remain pending; the runtime owner
is notified and native acceptance is not complete.
Supplemental P15 independent review st_01a099fb runs concurrently targeting
dag-p15-review.md; the DAG amend was refused with details.kind=error
(hasError=false) and the original six-node graph remains unchanged.
Review output is tracked as pending for combined verification.
P14 staged an executable evidence-only native harness under st_01a099e3 in
p14-native/{observe.ps1,submit.js} with JS syntax passed, while PowerShell and
native execution are unrun and production sound omission remains unchanged;
the runtime owner is notified and repair is not complete.
The full frontend run mon_6TQT1KKGVB9PQ546 / bash_60 finished 219 files in
114.46s (212 passed, 7 failed, 2,439 passed tests) following nwsapi 2.2.24 pinning;
see full-ui-post-selector.md.
ExitAttach callback arity assertions passed in bash_62 (exit 0, 55 passes).
App remote tests passed in bash_61 (exit 0, 23 passes; app-remote-suite-repair.md).
SshSection Added icon theme was repaired in ssh-added-theme.md (exit 0, 33 passes,
4/4 render cases).
TypeScript and Vite build exited 0 in bash_63.
Bounded remaining-work register is delivered in dag-remaining-register.md (77 lines).
The latest read-only Windows preflight confirms the original three installed
process identities still exist and console session 1 is active. See
windows-preflight-current.md. This is not GUI/input/PTY verification.

Final changed-source/shared-caller qualification, remaining repairs,
native runtime and delivery work remain. Branch/worktree creation still
needs explicit approval, requested and unanswered. That restriction does
not prohibit mocked local tests or scoped clean-file repairs. Generic
continuation is not ref-creation approval. Main must never be switched.
No release, installation or user daemon restart is authorized.
