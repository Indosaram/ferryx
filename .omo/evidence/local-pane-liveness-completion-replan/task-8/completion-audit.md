# Task 8 completion audit

Objective: verify immutable candidate on three remote hosts, preserve exact failed/passed/not-run evidence, route findings without repairs, and remove owned staging. This is verifier completion, NOT product acceptance.

| Explicit requirement | Inspected evidence | Result |
| --- | --- | --- |
| Both authoritative briefs read fully | Session/notepad, full initial reads | COMPLETE |
| maho-mac/maho-win SSH PROBE_OK and identity | host-provenance.txt and manifest.json; Task1 hosts matched | COMPLETE |
| Frozen commit/tree, locks, Ghostty roots/SHA before gate | manifest.json; remote hashes matched; final HEAD 5464da0dd65a7a6312d096229d2067ec02735c78 tree 3c91df668bd2fac62a4fc37a0aa2df4cdbd3cc9d | COMPLETE |
| Exact git archive, no target/cache copy, one target/serial | runner.mjs, per-platform timestamps/PIDs; final audit serial=true each | COMPLETE |
| Windows disk Get-PSDrive C and immediate LASTEXITCODE 7 then 0 | stage-windows.ps1 / capture validation / 217.50GiB free | COMPLETE |
| UI build before Rust | commands.jsonl, all hosts UI native2 before first Rust | RAN_FAILED recorded |
| Scoped split tests | ui-split.log each, native0 19/19 | RAN_PASSED |
| Scoped persistence/lifecycle/native tests | ui-lifecycle.log each, native1; Mac/Win9failed102passed Linux12failed99passed | RAN_FAILED |
| Exact runner tests | runner.log each native1, zero selected | FAILED_COVERAGE, no relaxation |
| Four Rust --list before exact affected runs | 8 records each host, ordering audit true | native101, selector UNKNOWN_COMPILE_FAILED |
| all-targets check/full lib | logs each and commands.jsonl | native101, test bodies NOT RUN due missing ui/dist |
| Full UI suite | final summary in full-ui.log each | Mac243failed, Win244failed/5skipped, Linux1271failed; all native1 |
| Linux changed Unix suspension/journal | four extra list/run logs | native101, behavior UNPROVEN |
| Exact argv/host/raw native exit/asserted output/count/bounded monitor | 49 unique commands.jsonl records, original platform JSONL, runner/PS1, monitor-ledger.json | COMPLETE, skipped not passed |
| Every failure verbatim and owning lane | report.md, baseline-findings.md, complete raw logs | frontend/scripts REOPEN, backend/native UNPROVEN |
| Out-of-scope pre-existing claims require base A/B | logs/base-ab.log, two identical named Mac cases native1 2failed32passed | ONLY these two baseline-confirmed |
| Source closure/new authored files | 20 new tracked files in frozen commit, no product untracked files | COMPLETE |
| All owned teardown, shared Ghostty preserved | cleanup.json; Mac/Linux receipts, Windows sourceAbsent=True; final ownedProcesses=0; local source.tar absent | COMPLETE |
| No product edit/commit/GUI/production/daemon mutation | final git status clean, execution confined to owned source | COMPLETE |
| Task9/10 exclusion | no native scenarios, packaging/signing launched | NOT RUN, outside dispatch |
| Final report and per-platform table | report.md, commands.jsonl, baseline-findings.md | COMPLETE |

Final audit: Mac15/Windows15/Linux19 unique gates, raw logs all present, serial=true/listBeforeRun=true, candidate HEAD/tree unchanged. No missing Task8 verifier deliverable remains. Product cannot be accepted: compile prerequisite blocks Rust and runner selects no tests. Backend/native claims remain reopened/unproven rather than silently closed.

