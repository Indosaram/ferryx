# PR 2 and PR 3 review and disposition evidence

Observed 2026-09-13T02:30:53Z. INCOMPLETE: both PRs remain OPEN.
This is a source review receipt, not merge approval or Windows runtime proof.
Working base last verified as ab08b94fdeda5039982fc8a37e8bc36885426667;
foreign App changes remain uncommitted and were read without modification.

## Current GitHub observations

Commands read directly in this session:

- `gh pr diff 2`: complete two-file patch, including the 198-line new test.
- `gh pr diff 3`: complete App implementation and parameterized test patch.
- `gh pr view 2 --json number,title,state,headRefOid,baseRefName,mergeCommit,url,reviewDecision,statusCheckRollup`
- Same `gh pr view` command for PR 3.
- `gh api graphql` queried both pull requests' `reviewThreads(first:100)`,
  totalCount, pageInfo.hasNextPage, resolution and comment fields; exit 0.

PR 2: https://github.com/Indosaram/ferryx/pull/2

- Head: `79b02ab6ea4753bf87080dd567368a870c018057`.
- Base main; state OPEN; mergeCommit null; reviewDecision empty.
- statusCheckRollup empty, not a passing CI run.
- Review thread count 0; hasNextPage false.

PR 3: https://github.com/Indosaram/ferryx/pull/3

- Head: `99c7086b61d7a590530fb8df924e34ce9e91e90b`.
- Base main; state OPEN; mergeCommit null; reviewDecision empty.
- statusCheckRollup empty, not a passing CI run.
- Review thread count 0; hasNextPage false.

No comments, reviews, merges, closures or other GitHub writes were sent.

## PR 2: valid narrow source correction, incomplete acceptance

The draw-only compositor HWND is currently created with
`WS_CHILD | WS_CLIPSIBLINGS` in
`src-tauri/src/native_terminal/platform/windows.rs:245`.
The proposed `WS_DISABLED` addition prevents it from becoming an input
target. The extracted `from_parent_hwnd` retains the production constructor
path, allowing the test to exercise actual compositor creation/reveal.

The test creates an owned nonvisible test desktop, an input child and
production compositor, then queries WindowFromPoint from another thread
before and after reveal. It checks the compositor remains visible and the
input child remains the target. It does not create WebView2, dispatch wheel
messages, drag a selection or prove capture/drop behavior. Those omissions
are unfulfilled acceptance evidence, not reasons to reject the narrow fix.

P01 in repair-packets.md owns this patch. Before product changes, stage its
regression independently or mutate the production style in the isolated
test checkout, and capture the same assertion failing for the intended
reason. Then verify GREEN with the corrected style and native lifetime
checks. P02 additionally requires the actual WM_MOUSEWHEEL/DOM/IPC path,
selection sentinel, visible terminal pixels and screenshot/action logs.
Do not infer wheel correctness from WindowFromPoint alone.

## PR 3: adapt discriminator forward, preserve current close semantics

The PR changes optional tab-kind handling from explicit terminal equality
to an existing-tab/non-browser guard. Its eight parameterized wiring cases
cover tagged/untagged and pinned/unpinned tabs through native callback and
web keyboard routes. The web event still uses metaKey, not a Windows Ctrl
fixture. Mocked closePane calls do not establish sibling PTY survival.

Current `ui/src/App.tsx:1806-1823` differs from the PR base: the caller
handles both split and leaf-root layouts through `handleClosePane`, which
honors active-agent confirmation. It still requires explicit terminal kind,
so the optional-kind problem persists in the inspected working bytes.
Preserve this newer confirmation/routing behavior; apply only the necessary
discriminator correction forward. A blind patch that restores split-only
routing or direct closePane would discard the other session's behavior.

P05 owns the adapted repair. Acceptance requires production-seam RED/GREEN,
Windows ctrlKey and native-menu coverage, tagged/untagged and
pinned/unpinned cases, split/unsplit and browser guards, and confirmation
semantics. In the real debug GUI, close only the selected owned PTY, await
its exit, then obtain new output from the same sibling PID/backend session.

## Unfulfilled acceptance and final disposition

- PR3 now has independently verified local App wiring and real-store
  regression evidence: original App guard rejects 14 cases; all 128 App
  cases pass, all 58 store cases pass, eight wrong-whole-tab mutations
  reject sibling/selected ownership. Combined 575 tests and UI build pass.
  See wheel-close-lead-verification.md for exact commands and limitations.
  PR2 native regression and runtime remain unexecuted.
- No current Windows debug wheel, drag, close or sibling-survival receipt.
- No reviewed atomic integration commit, gate approval or remote push.
- Both PRs must receive audited completed dispositions only after those
  checks. Refresh state/mergeCommit and record exact commits at delivery;
  these OPEN observations are not final dispositions.

See acceptance-status.md, wheel-regression-seam.md, findings.md and the
registered repair packets for the remaining requirement-to-evidence map.
The report is uncommitted; it does not claim ownership of foreign changes.

## Current external-state refresh

2026-09-13T06:13:32.225Z. Both exact commands exited 0:

```sh
gh pr view 2 --json number,state,headRefOid,mergeCommit,statusCheckRollup,url
gh pr view 3 --json number,state,headRefOid,mergeCommit,statusCheckRollup,url
```

- PR2 OPEN, head 79b02ab6ea4753bf87080dd567368a870c018057,
  mergeCommit null, statusCheckRollup [].
- PR3 OPEN, head 99c7086b61d7a590530fb8df924e34ce9e91e90b,
  mergeCommit null, statusCheckRollup [].

Unchanged heads retain the earlier read diff assessment. This refresh does
not reassert review-thread freshness or imply CI success. No GitHub write.
The current local HEAD remains da6eec06d65551f67bbc43f09910cde470c3478d;
the source repairs remain uncommitted and are not PR dispositions.
