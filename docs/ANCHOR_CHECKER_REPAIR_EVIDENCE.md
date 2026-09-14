# Source anchor checker repair evidence

## Scope and artifact ledger

Worktree: `/Users/indo/code/project/orca-lite-wt/sa-docs`. Node v22.22.3.
Only the verifier, its focused new tests, and this evidence file are owned by this repair.
Existing documentation changes belong to the lead and are not modified.
No Ferryx process, socket, watcher, or daemon is started or touched.
This file is the debug journal within the explicitly authorized file scope; no root journal or shared git exclude is written.

Planned temporary artifacts: tests create unique `scripts/.anchor-test-*` directories, copy the verifier into each, write synthetic documents/source, and remove them in `finally`. No files outside this worktree.

## Investigation

- H1: line-wide token inference attaches unrelated symbols to all citations. Baseline confirms guide:22 requires FERRYX_RUNTIME_DIR at server.rs:135; privacy:79 requires PATH at four unrelated anchors.
- H2: absence/external-mechanic prose is mistaken for literal source assertions. Baseline confirms guide:273 requires tracing_subscriber and guide:40 requires PATH.
- H3: parsing drops source extensions and range tails. Deterministic fixtures will distinguish missing-file/range validation from token inference.

Baseline command: `node scripts/verify-source-anchors.mjs` (exit 1).
Exact summary: `FAIL  49 of 264 anchors did not verify:`.
Representative exact failures:

```text
docs/HEADLESS_LINUX_SERVER_DEPLOYMENT_GUIDE.md:22 -> src-tauri/src/daemon/server.rs:135  DRIFTED: none of ["FERRYX_RUNTIME_DIR"] found within +/-3 lines
docs/HEADLESS_LINUX_SERVER_DEPLOYMENT_GUIDE.md:273 -> src-tauri/src/cli.rs:471  DRIFTED: none of ["tracing_subscriber"] found within +/-3 lines
site/src/content/docs/privacy.md:79 -> src-tauri/src/remote/auth.rs:75  DRIFTED: none of ["PATH"] found within +/-3 lines
```

## Contract decision

Ordinary prose references prove structure only, not English claims. An immediately following parenthesized inline-code expression, written as `` `path:line` (`CODE`) ``, explicitly asserts its identifiers at that citation. This existing notation preserves the original pairing-constant drift mutation without assigning distant prose tokens to citations. SCREAMING_CASE identifiers take precedence over generic identifiers such as Duration; all selected identifiers must match. Explicit ranges search only their complete union; single lines retain the existing +/-3 tolerance. Token presence is not proof of values, execution, absence, transport security, or any other semantic claim.

## Failing-first evidence

Before implementation, `node --test scripts/verify-source-anchors.test.mjs`
exited 1. Exact TAP summary (timing omitted):

```text
1..30
# tests 30
# suites 0
# pass 12
# fail 18
# cancelled 0
# skipped 0
# todo 0
```

The unrelated-token, absence, and PATH fixtures failed with DRIFTED diagnostics.
Invalid range ends, reversed ranges, and shorthand incorrectly returned exit 0.
JSON/YAML/Astro missing-source fixtures reported `OK  0 anchors verified across 3 deliverables`.
The full-range token fixture failed because the old verifier inspected only its start.
The original three mutation classes already passed their negative tests: drift despite
generic Duration, nonexistent file, and out-of-range start. Those tests remain.

Real execution then exposed a resolved-basename context bug in the repair. A new
failing-first fixture established a full path, crossed a heading, used the unique
basename, then cited `:16`. Exact diagnostic:

```text
docs/HEADLESS_LINUX_SERVER_DEPLOYMENT_GUIDE.md:3 -> (shorthand):16  BROKEN: shorthand has no unambiguous established source path
```

That focused run reported 3 tests, 2 pass, 1 fail. Updating context after resolving
the unique basename fixed it. The ambiguous-basename fixture now supplies both real
files, so a missing-file failure cannot disguise incorrect ambiguity handling.

## Final validation

`node --test --test-reporter=dot scripts/verify-source-anchors.test.mjs`
exited 0 with all 32 deterministic CLI fixtures passing:

```text
....................
............
```

`node --check scripts/verify-source-anchors.mjs` and
`node --check scripts/verify-source-anchors.test.mjs` exited 0 with no output.
`git diff --check` emitted no errors. LSP diagnostics were attempted for both files
but unavailable: `Could not find a valid TypeScript installation.` No dependency
was installed to hide that environment limitation. This standalone JavaScript CLI
has no compilation/build step; the website build is outside this repair's scope.

Real command: `node scripts/verify-source-anchors.mjs`, exit 1. Exact output:

```text
FAIL  3 of 354 references did not verify (16 explicit token assertions):

  site/src/content/docs/privacy.md:33 -> ui/package.json:13-58  BROKEN: line or range out of range/reversed (file has 55 lines)
  site/src/content/docs/privacy.md:35 -> ui/src/lib/updater.ts:54-63,154-180  BROKEN: line or range out of range/reversed (file has 178 lines)
  site/src/content/docs/privacy.md:94 -> ui/src/lib/updater.ts:54-63,154-180  BROKEN: line or range out of range/reversed (file has 178 lines)
```

These are genuine endpoint errors, not exemptions: the cited files end at lines 55
and 178, respectively. The lead owns correcting those documentation anchors.
No other structural errors or explicit assertion failures were reported by this
run. This is NOT a factual PASS for the prose or an independent documentation audit.

## Exact supported scope and limitations

- Source extensions: rs, ts, tsx, mjs, toml, json, yml, yaml, astro; dot-prefixed
  directories such as `.github` are preserved. Other extensions are not scanned.
- Full repository paths may be quoted or unquoted. Basename and `:N` shorthand
  must be inline code. A basename resolves only against one unique full path
  previously established in that document, never by a repository filename search.
- `:N` uses the latest explicitly resolved path; headings clear this context.
  Basename resolution reestablishes it. Unbound/ambiguous shorthand fails visibly.
- Every range endpoint and comma-separated segment is validated, including reversed
  ranges and zero. A terminal newline does not create an extra addressable line.
- Only directly attached parenthesized inline code asserts tokens. All identifiers
  in that expression are selected unless SCREAMING_CASE identifiers are present;
  then those distinctive identifiers alone are selected. Matching uses complete
  lexical identifiers, not substrings. Values and source semantics are not parsed.
- Token assertions on explicit ranges search their union, not gaps or a padded
  first line. Single-line assertions retain +/-3 lines. Unasserted in-range line
  drift is not detectable; it still requires a human semantic audit.
- Fenced captured output (backticks or tildes) and existing anchor-check off/on
  regions remain excluded. No new suppression markers or document exemptions were
  introduced. This is a focused citation recognizer, not a general Markdown parser.
- Existing historical statements in SOURCE_ANCHORS_VERIFICATION.md describing
  whole-line inference are superseded by this contract; that lead-owned file was
  not rewritten by this repair.

## Cleanup and self-review

Temporary fixture directories were removed in `finally`; all writes were inside
this worktree. No commit-generating fixtures, daemon launches, process signals,
shared git metadata edits, or persistent debug artifacts were created.
Only scripts/verify-source-anchors.mjs, scripts/verify-source-anchors.test.mjs, and
this note were written by this repair. Other dirty/untracked documentation belongs
to the concurrent lead and was left intact.

Source size: verifier 104 nonblank/noncomment lines; tests 59. Responsibilities
are citation validation and isolated CLI contract tests. Input parsing stays at the
document boundary. No tagged variants, type escape hatches, defensive catches,
parameter bloat, speculative helpers, sleeps, polling, or production logging were
introduced. The shared fixture helper has 32 callers and tests actual subprocess
outcomes. Red-to-green tests distinguish the repaired behavior and retain the
original drift/nonexistent/out-of-range mutation detection.
