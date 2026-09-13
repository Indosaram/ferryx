# Windows gap audit: lead checkpoint

Date: 2026-09-13. This is an incomplete-work checkpoint, not a completion verdict.

## Objective and evidence boundary

Resolve PRs #2 and #3, repair every confirmed Windows defect including wheel
input, verify the actual Windows debug application and owned PTYs, obtain the
required gate approval, and push verified atomic commits to origin/main.
The remote main SHA, PR dispositions, and cleanup receipts must agree with the
reviewed final commit. None of those delivery conditions is satisfied yet.

The active loop is `.omo/ulw-loop/01a0983f-c995-753d-afa9-593f6d118788/`.
Its `ledger.jsonl` remains the notepad. No criterion is passed by this report.
The complete prompt-to-artifact acceptance map remains in `lead-baseline.md`;
all its implementation, runtime, gate, and delivery boxes remain unchecked.

## Accepted missing-source audits

- `gap-history.md`: all 146 original `- **ID**:` fields from
  `docs/CROSS_PLATFORM_ISSUE_AUDIT_2026-09-07.md` were compared with the report's
  individual rows. Both sets contain 146 unique IDs, with zero missing or extra
  IDs. Status counts independently recomputed: C 42, K 10, P 4, R 36, N 21,
  U 33. These are historical-ID dispositions, not 42 independent fixes.
- `gap-backend.md`: source coverage accepted. Lead directly reopened
  `src-tauri/src/dag/watcher.rs:87-192` and
  `src-tauri/src/remote/relay_server.rs:395-447,3120-3254`.
  Watch recovery, cross-process enrollment transaction boundaries, and
  misleading integration-test assertions remain repair candidates without
  RED/GREEN or runtime proof.
- `gap-tooling.md`: source coverage accepted. Lead directly reopened
  `src-tauri/tests/permissions_contract.rs:29-48`,
  `src-tauri/src/permissions/mod.rs:304-332`,
  `src-tauri/tests/native_terminal_renderer_contract/standalone_poc.rs:1-96`,
  and `src-tauri/tests/native_terminal_surface_host_contract.rs:585-626`.
  Actual command execution, artifact discovery, and deterministic lifecycle
  assertions remain unverified. Future helper build commands must be debug,
  not release.
- `gap-ui.md`: final classification accepted for source coverage. Stable
  GAP-UI-01 through GAP-UI-12 identifiers restored. Only GAP-UI-06 is a
  confirmed repair candidate: remote wheel unit normalization. The existing
  command is `bun run --cwd ui test src/remote/RemoteTerminal.contract.test.tsx`.
  It has not been run. Exact numerical assertions must be registered before
  editing. Proposed cell metrics and accumulator names are design suggestions,
  not confirmed existing APIs or frozen implementation choices.

The original native wheel direction and repetition code is already present.
Do not repeat a sign fix: `wheel-regression-seam.md` identifies the actual
production IPC position/modifier seam.

## Inventory rejection still open

The latest `gap-inventory.md` claims 1,845 tracked entries, 848 exclusions,
997 first-party files, a 91-file union, and 314 sites. These are not accepted
as comprehensive coverage by this lead.

- Sections 2.3 and 5 retain a 122-item historical accounting and treat ordinary
  Rust rename sharing and synchronous cookie deadlock allegations as confirmed,
  contradicting the individually traced historical report.
- Section 4 calls 38 modules free of platform cfg despite including modules
  with explicit platform branches. Lexical absence, semantic reachability,
  and completed code review are distinct facts.
- Listing imports, comment lines, script entry lines, or string literals as
  sites does not make them conditional branches or prove exhaustive coverage.
- The predicates, exact deduplicated sets, and every uncovered caller still
  require the running verifier's source-backed adjudication.

The running DAG is `dag_f6568880-26de-4341-848f-d36a13e64f78`.
At the last observed snapshot five producers were completed and
`gap-verification` (`st_01a0986d`) was running. Lead corrections were delivered
to that node. Its result must be inspected before synthesis is frozen.

## Remaining prompt-to-artifact links

- PR diffs and initial review-thread evidence: `lead-baseline.md` and the
  first audit reports. Missing: production-seam RED/GREEN, tagged/untagged and
  pinned/unpinned close matrix, keyboard/native-menu actions, sibling PTY
  survival, Win32 pointer delivery, current PR dispositions.
- Comprehensive source coverage: eight `audit-*.md`, five `gap-*.md`, and
  `coverage.md`. Missing: accepted exact inventory and final reconciliation.
- Per-defect repair: `findings.md`, `repair-packets.md`, and gap reports.
  Missing: reconciled ownership, latest packet digest registered in criteria,
  exact test/binary conditions before each fix, production changes, RED/GREEN.
- Real Windows acceptance: missing current-tree `bun tauri dev` launch in an
  isolated interactive session; native SendInput wheel up/down and tracking;
  screenshots plus input/state/PTY logs; keyboard, selection, shell cmd echo,
  resize/DPI, browser/native isolation, cwd/output, and session survival.
- Aggregate verification: missing targeted suites and build exit 0 on the
  repaired combined tree, without disabled tests or suppressed failures.
- Delivery: missing frozen final audit, loop gate approval, atomic commits,
  `git push origin <verified-sha>:main`, matching
  `git ls-remote origin refs/heads/main`, final `gh pr view 2` and
  `gh pr view 3`, applicable checks, and all owned-resource cleanup receipts.

## Authorization and shared-tree preservation

Branch/worktree creation and deletion still require the requested explicit
approval. Repeated continuation messages retain that pending condition.
No production edits, test execution, builds, desktop actions, releases,
installation, commits, merge, or push have been performed in this run.

Latest `git status --short` and `git diff --stat` still show 30 foreign tracked
modified files, totaling 391 insertions and 40 deletions. Foreign untracked
artifacts also remain. No user daemon lifecycle action was taken.
This checkpoint and the audit documents are uncommitted and therefore remain
vulnerable to concurrent edits in the shared tree.
