# Q4 Windows candidate review corrections

Both review concerns are corrected only in candidate-owned test code.
Production candidate bytes are unchanged. Prior patch, manifests and failure
logs remain retained; use `Q4-windows-review-candidate.patch` as the revised
complete candidate, not the earlier patch plus an assumed textual correction.
`Q4-windows-review-source-hashes.json` and `Q4-windows-review-manifest.json`
record exact updated identity.

The relay browser now records an actual received Close frame and asserts it
was observed after the receive loop. EOF without Close fails; frame errors,
including reset, still fail through unwrap. Native runtime output explicitly
records `Q4 browser close acknowledgement observed=true`: 1 passed, exit 0.

The natural-exit ConPTY fixture accumulates bytes across receive chunks. If no
complete ESC[6n query is present it retains the final three bytes, enough to
recognize every possible split of the four-byte query. On recognition it answers
and clears the buffer. It therefore neither requires one-chunk delivery nor
retains an unbounded terminal history. The full native worktree_safety target
passed 9 tests, exit 0, and logged the real ConPTY query response. This run does
not claim the OS happened to fragment the query; split recognition follows
from preserving the unmatched three-byte suffix across every chunk.

## Retained private snapshot and artifacts

- Local complete source: `/tmp/ferryx-herdr-q4-windows-01a097f8/source`
  (Darwin physical path `/private/tmp/ferryx-herdr-q4-windows-01a097f8/source`).
- Original pre-candidate source: `/tmp/ferryx-herdr-q4-windows-01a097f8/candidate-baseline`.
- Native source: `maho-win:C:\Users\sook\ferryx-herdr-q4-windows-01a097f8\source`.
- Local scripts/manifests/full logs: `/tmp/ferryx-herdr-q4-windows-01a097f8`,
  including `review-logs/`; evidence copies have Q4-windows prefixes here.

## Parent native rerun

The retained script sets private HOME/USERPROFILE, APPDATA/LOCALAPPDATA, FERRYX,
XDG and TEMP paths before test initialization; retains toolchain/target; sets
jobs=3, debug=0, incremental=0 and empty wrapper through the private override;
uses explicit private source cwd and bounded process waits. It creates only
previously owned runtime directories removed by cleanup.

```sh
ssh maho-win 'powershell -NoProfile -File C:\Users\sook\ferryx-herdr-q4-windows-01a097f8\review-run.ps1'
ssh maho-win 'powershell -NoProfile -File C:\Users\sook\ferryx-herdr-q4-windows-01a097f8\review-cleanup.ps1'
```

This script runs the following native commands with the private Cargo override:

```text
cargo --config C:\Users\sook\ferryx-herdr-q4-windows-01a097f8\cargo-qa.toml test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib test_relay_browser_ws_terminal_bridge_success -- --nocapture
cargo --config C:\Users\sook\ferryx-herdr-q4-windows-01a097f8\cargo-qa.toml test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test worktree_safety -- --nocapture
```

Before rerunning, preserve/rename the script's `review-*.log` outputs if retaining
another execution under the same remote root; this execution's immutable copies
are already in the resumed evidence directory. For the broader original selectors,
the retained `candidate-aggregate.ps1` runs remote, the six integration targets,
worktree, the native path repro and headless build under the same environment.

## Scope reference and verification

Original task: st_01a0984d, parent/root
01a097f8-4568-7573-897e-d61f0fe6d692, assigned native Windows maho-win Q4 platform
evidence expansion against frozen Wave1 plus inherited dirty implementation.
Requirements are in the full approved
`/Users/indo/code/project/orca-lite/.omo/plans/ferryx-herdr-cloud-multi-host-plan.md`
and resumed `WAVE1-resume-acceptance-gaps.md` (Q4) / `PLATFORM-resume-preflight.md`.
The subsequent explicit user repair authorization confined candidate changes to
this private snapshot and Q4-prefixed artifacts. This review turn authorizes only
the two test corrections above. No fresh production scope or composition occurred.

Local LSP found no errors for worktree_safety; relay fresh diagnostics timed out
and are not claimed clean for this turn. Native compilation and both test runs
completed successfully, with warnings retained. Cleanup exit 0 and native cleanup
log record 970 matching inputs, zero owned runtime processes (excluding the exact
active cleanup wrapper), both children waited, and removal/absence of all nine
owned runtime directories. Source/build roots remain retained. No user clipboard,
desktop, canonical daemon, credential, live-worktree edit or commit was involved.
