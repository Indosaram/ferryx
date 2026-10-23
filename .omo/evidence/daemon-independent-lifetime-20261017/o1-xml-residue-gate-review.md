# Gate review: F1 O1 (Windows QA task-definition XML residue), commit 4a3c6856

recommendation: APPROVE
blockers: []

## originalIntent
Close F1-compliance-r4 section 5 open item O1. Each Windows QA run left its own
`Ferryx-ferryx-qa-<run>-<key>.xml` in the real `%LOCALAPPDATA%\Ferryx\supervisor`, and the
harness neither removed nor reported it. The fix is scoped to the QA harness only.

## desiredOutcome
- Teardown deletes the definition record of every QA task of the run, using the product's own path.
- It never touches a non-QA or production record (`Ferryx-DaemonQa.xml`, `Ferryx\Daemon*`).
- A remaining record of THIS run fails the run, as a teardown leftover. A foreign run's record and a Linux or mac run do not fail it.
- The existing residue on maho-win is cleaned up safely.

## userOutcomeReview
The harness delivers that outcome:
- The deletion path now matches the product's path byte for byte.
- remove() is name-guarded before any side effect.
- The new leftover check only looks at this run's prefix, and only on Windows.
- The scoped live Q00 run at 4a3c6856 ended with RUN_EXIT=0 and leftovers [].
- On maho-win the supervisor dir is back to only Ferryx-DaemonQa.xml, with that file's hash unchanged.

## Checks

### 1. The XML path matches the product: PASS
- SOURCE supervisor_windows.rs@4a3c6856:99-107. The product's path is `PathBuf::from(LOCALAPPDATA).join("Ferryx").join("supervisor").join(format!("{}.xml", name.replace('\\', "-")))`. Rust `str::replace` replaces every occurrence.
- SOURCE supervisor_windows.rs:86-96. Product names are `Ferryx\<SUPERVISOR_NAME>` and `Ferryx\<SUPERVISOR_NAME>-<key>`.
- SOURCE supervisor_windows.rs:352-366 and :1068-1076. import_definition writes this file before `/Create`, and uninstall removes the same task_xml_path.
- SOURCE jobs.mjs@4a3c6856:16-18. `taskXmlFileName` replaces every `\` with `-` after stripping an optional leading `\`. Product names never carry a leading `\`, so the result is identical.
- SOURCE jobs.mjs:233. It emits `Join-Path $env:LOCALAPPDATA 'Ferryx\\supervisor\\<file>'`, which is `Ferryx\supervisor\<file>` after JS template escaping. That is the correct fix for the old bug: old line `'Ferryx\supervisor\…'` produced `Ferryxsupervisor`, because `\s` is a no-op escape in a JS template.
- The LOCALAPPDATA base is the same, because the QA env keeps the real LOCALAPPDATA (SOURCE guards.mjs:164-168).
- MEASURED test-red.txt / test-green.txt and my own rerun. The asserted command string is `...'Ferryx\\supervisor\\Ferryx-ferryx-qa-<RUN>-2026.1018.1-516169bd.xml'...`, which is `Ferryx\supervisor\...` at runtime.
- MEASURED cleanup.json. For all 40 deleted entries, `task.replace('\\','-') + '.xml' == file` (python check: True). That is real on-disk confirmation of the derivation.

### 2. remove() cannot hit a non-QA task: PASS
- SOURCE jobs.mjs:227. On Windows, `isQaJobName(name, runId, 'windows')` runs before `/End`, `/Delete` and Remove-Item.
- SOURCE guards.mjs:62-75. That check accepts only `Ferryx\ferryx-qa-<runId>` or `Ferryx\ferryx-qa-<runId>-*`, and assertRunId requires `^\d{8}t\d{6}z$`.
  - `Ferryx\DaemonQa` (record `Ferryx-DaemonQa.xml`) and `Ferryx\Daemon*` are refused.
  - So is any other run's name.
  - Because the guard prefix ends at the fixed-length runId plus `-`, no prefix collision is possible.
- The file name is built only from the accepted name, so the deletion path has no other source.
- MEASURED by my own rerun of the test. `remove('Ferryx\\Daemon-2026.1018.1-516169bd')` rejects with /non-QA task/, and zero exec calls are made.
- All remove() callers (run.mjs:366 and q07-q09.mjs:103,609,625) pass this run's job names. Each still goes through the guard.
- NOTE: the schtasks mutations are also guarded independently by the host-agent guard (SOURCE guards.mjs:246-252).

### 3. The leftover check is this run only and Windows only: PASS
- SOURCE jobs.mjs:80-86. `listQaTaskXml` returns `[]` when `platform !== 'windows'`, before any agent call, so Linux and mac runs are unaffected.
- Its filter is `Ferryx-${base}*.xml`, with `base = ferryx-qa-<runId>`, so a foreign run's records are not matched.
- A listing failure throws. The throw becomes a teardown error, which is pushed as a leftover (run.mjs:416), so it fails loudly rather than silently passing.
- SOURCE run.mjs:393-396. The records are added as `{kind:'task-xml'}` leftovers.
  - A non-empty leftover list keeps the run dirs and makes the run not clean (run.mjs:399, :620).
  - The leftover step runs after both remove-jobs passes (run.mjs:378, :384).
- MEASURED run/summary.json (runId 20261022t193632z, head 4a3c6856..., dirty []):
  - jobsRemoved is [the run's generation task]; leftovers, errors and refused are all []; exitCode is 0.
  - MEASURED run/RUN_EXIT is `RUN_EXIT=0`.
  - MEASURED run/supervisor-after.txt shows QA=0 and RUNQA=0, NAMES=Ferryx-DaemonQa.xml, and an empty TASKS list.

### 4. The test can fail: PASS (reproduced)
I ran these myself on a `git archive 4a3c6856` scratch copy under /tmp, deleted afterwards. The Mac worktree named in the brief no longer exists.
- MEASURED: `node --test scripts/qa/daemon-lifetime/windows-all-fix.test.mjs` gives 13/13 pass.
- MEASURED: reverting only lib/jobs.mjs to cfe34b8e gives EXIT=1. `not ok 13` fails on the first deepEqual: there is no Remove-Item for the generation task. This matches test-red.txt.
- MEASURED: reverting only run.mjs to cfe34b8e, with the fixed jobs.mjs, gives EXIT=1, `not ok 13`. So the leftover half of the test also bites.
- MEASURED: `node --test scripts/qa/daemon-lifetime/*.test.mjs` gives 212/212 pass, EXIT=0. This matches test-full-suite.txt and the command recorded in PROGRESS.md.
- NOTE: the bare directory form `node --test scripts/qa/daemon-lifetime/` fails with MODULE_NOT_FOUND. That is node 22 CLI behavior with a directory argument, not a test failure. The glob form is the one the evidence used.

### 5. No weakened assertions, sleeps or pattern kills: PASS
- SOURCE `git diff cfe34b8e 4a3c6856`: +59/-6 over three files.
  - No assertion was removed or loosened; the diff only adds lines to the test file.
  - No sleep or setTimeout was added, and no taskkill, Stop-Process, pkill or wildcard kill.
- The only removed production lines are the old buggy legacy-only Remove-Item block.
- Remove-Item uses `-LiteralPath` with an exact file name, never a wildcard.

### 6. The one-time cleanup was safe: PASS
- MEASURED cleanup.json.
  - before: 41 entries, 40 QA XMLs. after: 1 entry, 0 QA XMLs, remaining [Ferryx-DaemonQa.xml]. keptBecauseTaskExists is [].
  - My python check over all 40 deleted entries: each matches `Ferryx-ferryx-qa-[0-9a-z.-]+\.xml`, and each has queryExit == 1 (no live task).
  - daemonQaSha256 is DC91241F...96882 both before and after, and `daemonQaUnchanged: true`.
- MEASURED run/supervisor-after.txt, taken after the proof run, shows the same DAEMONQA_SHA.

## remove-ai-slops / programming pass (direct)
- The test checks machine-consumed values: the exact emitted command, its order after /Delete, the refusal with no side effects, and the leftover entry with run dirs kept. It is not tautological; two independent reverts each turn it RED.
- It is one new test at 29 lines, sized like its neighbors in windows-all-fix.test.mjs. It is not a deletion-only test.
- Production code adds one small exported helper (`taskXmlFileName`), used by remove() and the test. That is justified: it mirrors the product derivation and documents it. No extraneous parsing or normalization was added.
- NOTE (non-blocking):
  - The `jobs.listQaTaskXml ?` existence check in run.mjs:395 is a mild defensive branch for older mock objects in tests. It is harmless.
  - installLegacy still builds `recordName` with a single-occurrence `.replace('\\','-')` (jobs.mjs:162). This is correct for `task(null)`, which has exactly one `\`, but it is inconsistent with the new helper. This is a follow-up at most.
- Code review report: none was supplied in the input, so I cannot confirm that its skill-perspective coverage exists. My direct pass above stands in for it. This is not tied to a stated success criterion, so it is a NOTE.

## Checked artifacts
- git objects 4a3c6856 (parent cfe34b8e; branch fix/qa-xml-residue-20261023):
  - scripts/qa/daemon-lifetime/lib/jobs.mjs
  - run.mjs
  - windows-all-fix.test.mjs
  - lib/guards.mjs
  - scenarios/q07-q09.mjs (callers)
  - src-tauri/src/daemon/supervisor_windows.rs
- Evidence dir:
  - PROGRESS.md
  - test-red.txt
  - test-green.txt
  - test-full-suite.txt
  - cleanup.json
  - prep.log
  - run/summary.json
  - run/RUN_EXIT
  - run/supervisor-after.txt

## Evidence gaps
- The Mac worktree /Volumes/T9-Mac/project/ferryx-wt/qa-xml-residue-20261023 does not exist any more, so I verified against the git objects of 4a3c6856 via a scratch archive instead.
- The orchestrator's post-run ssh check (PROGRESS.md: no ferryx-qa task, no processes, maho-win worktree removed) is only partly backed by an artifact, in run/supervisor-after.txt. I did not re-check it remotely, because remote hosts were out of bounds.
- I did not run the live run myself; I took it from its artifacts.
- No code review report or manual QA matrix was supplied.

VERDICT: confirmed
