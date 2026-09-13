# Final documentation lead check

Worktree: `/Users/indo/code/project/orca-lite-wt/sa-docs`.
Date: 2026-09-13.

## Current result

The two requested documents exist. The final correction ledgers contain all 51
guide audit identifiers and all 36 privacy audit identifiers from
`docs/DOCUMENTATION_CLAIM_COVERAGE.md`; identifier comparison found no missing
privacy items, and the guide identifiers matched in order. This checks ledger
coverage, not the truth of every sentence.

The guide ledger's summary originally claimed 39 corrected, 2 removed, and 10
supported recommendations. Counting its actual disposition entries produced
37 corrected, 2 removed, and 12 supported recommendations. The lead corrected
the summary and clarified that the retained category includes source-supported
identifiers and schemas, not only external operational recommendations.

## Captured command results

From this worktree, monitor `mon_54RC9JSP3S48EX5P`, session `bash_308`, ran:

```sh
node scripts/verify-source-anchors.mjs
bun run --cwd site build
```

Verbatim decisive output:

```text
FAIL  49 of 264 anchors did not verify:
FINAL_ANCHOR_EXIT=1
22:08:25 [WARN] [glob-loader] Duplicate id "privacy" found in /Users/indo/code/project/orca-lite-wt/sa-docs/site/src/content/docs/privacy.md. Later items with the same id will overwrite earlier ones.
22:08:29   ├─ /privacy/index.html (+8ms)
22:08:29 [build] 16 page(s) built in 4.33s
22:08:29 [build] Complete!
FINAL_SITE_BUILD_EXIT=0
watcher exited_1 (exit code 1)
```

The lead read the full command output. All 49 reported errors were `DRIFTED`
token checks, not missing-file or line-out-of-range errors. The verifier applies
tokens from an entire Markdown line to each reference on that line, and inspects
only the starting line of a cited range. Examples from the captured failure:

```text
docs/HEADLESS_LINUX_SERVER_DEPLOYMENT_GUIDE.md:22 -> src-tauri/src/daemon/server.rs:135  DRIFTED: none of ["FERRYX_RUNTIME_DIR"] found within +/-3 lines
docs/HEADLESS_LINUX_SERVER_DEPLOYMENT_GUIDE.md:273 -> src-tauri/src/cli.rs:471  DRIFTED: none of ["tracing_subscriber"] found within +/-3 lines
site/src/content/docs/privacy.md:79 -> src-tauri/src/remote/auth.rs:75  DRIFTED: none of ["PATH"] found within +/-3 lines
```

The first associates an override token with the default-path reference instead
of the later override reference. The second requires the very token whose
absence the prose describes. The third confuses the external PATH lookup
mechanism with a required literal in the identity-loading code. The completed
`anchor-verifier-repair` separates structural references from directly attached
token assertions. Its deterministic fixtures retain the original drift,
missing-file, and invalid-line failures and cover ranges and shorthand.

Markdown LSP diagnostics were attempted and unavailable:

```text
No LSP server configured for extension: .md
```

No global language-server configuration was changed for a prose-only edit.
`git diff --check` returned exit 0 after the lead's document corrections.

## Additional source-backed corrections after this build

The lead read `src-tauri/src/remote/auth.rs:374-473,624-716,735-806`.
`begin_transaction` replaces memory only when loading the persisted file
succeeds. Deleting the file from a running daemon therefore leaves its current
tokens in memory; a later best-effort save can recreate the file. The guide now
limits file-deletion reset to a stopped daemon and directs running-daemon
revocation to the API.

The guide and privacy page now distinguish PIN expiry from immediate disk
erasure. Pairing operations prune or consume codes; persistence is best-effort,
so failed saves can leave older records. The privacy page also names the
persisted relay pairing capabilities. These two prose edits occurred after the
captured build above. The subsequent lead-run site build covered those final
edits; monitor `mon_S2WRTXGV20FS1T50`, session `bash_309`, captured:

```text
22:13:58 [WARN] [glob-loader] Duplicate id "privacy" found in /Users/indo/code/project/orca-lite-wt/sa-docs/site/src/content/docs/privacy.md. Later items with the same id will overwrite earlier ones.
22:14:02 [build] Complete!
CREDENTIAL_DOCS_BUILD_EXIT=0
watcher completed (exit code 0)
```

No process or server was left running by the completed build command.

## Final independent verification

The repaired verifier found three genuine out-of-range citations in the privacy
page. The lead read the cited files and corrected `ui/package.json:13-58` to
`:13-55` and both updater range endpoints from `180` to `178`.

Monitor `mon_TM1DVDAJA2E4NZ9E`, session `bash_310`, independently executed:

```sh
node --test scripts/verify-source-anchors.test.mjs
node scripts/verify-source-anchors.mjs
bun run --cwd site build
git diff --check
```

All 32 named regression tests passed, with zero failures, cancellations, or
skips. Decisive command output:

```text
LEAD_ANCHOR_TEST_EXIT=0
OK  354 structural references verified across 3 deliverables; 16 explicit token assertions verified (prose claims not checked)
LEAD_ANCHOR_VERIFY_EXIT=0
22:16:02 [WARN] [glob-loader] Duplicate id "privacy" found in /Users/indo/code/project/orca-lite-wt/sa-docs/site/src/content/docs/privacy.md. Later items with the same id will overwrite earlier ones.
22:16:05 [build] Complete!
LEAD_FINAL_SITE_EXIT=0
LEAD_DIFF_CHECK_EXIT=0
watcher completed (exit code 0)
```

The lead reviewed both changed scripts. Fresh JavaScript LSP diagnostics failed
to initialize because the server could not resolve a valid TypeScript
installation. This is an environment limitation, not a clean LSP result.
The child separately captured successful JavaScript syntax checks.

A post-run scan found no `scripts/.anchor-test-*` fixture directories. The
monitor exited, all fixture CLI processes were awaited, and this check opened
no listening server or tmux session. Read-only inspection still found daemon
PID 21591. This proves only stability during these final checks, not C5.

The claim coverage ledger and two correction ledgers carry semantic source
review; the automated checker proves only its stated structural and explicit
token contract. Historical broader claims in earlier verification notes are
not the current verifier contract.

## Acceptance boundary and daemon observation

This continuation does not repair or waive the original historical acceptance
failures. Earlier evidence records original live daemon PID 36170 and later PID
1010. The current read-only process observation was:

```text
21591 Sun Sep 13 19:27:52 2026     /Users/indo/code/project/orca-lite-wt/dag-viewport-wave3/src-tauri/target/debug/Ferryx.app/Contents/MacOS/ferryx --daemon
```

This is not an unchanged-PID result or a replacement baseline. No daemon was
launched or signalled by this continuation. The missing historical
pre-production RED and previously created commit objects remain acceptance
failures. No commit was created in this continuation.

The site build is not a Linux deployment test, a live relay reachability test,
or desktop GUI evidence. The duplicate privacy identifier warning remains
visible in the captured build. All new corrections remain uncommitted in this
worktree.
