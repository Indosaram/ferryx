# Task8 pass2 completion audit

Objective: verify/report frozen172baa87, preserve pass1, run ordered gates remotely, classify all observed failures against base, archive raw provenance, reopen owners, remove owned resources. Product acceptance is not achieved: candidate rejected. Evidence delivery is complete subject to final invariant inspection.

| Explicit requirement | Evidence | Result |
| --- | --- | --- |
| Exact clean commit/tree/base chain |manifest.json,current-frozen-source-audit.json |172baa874f5e320ef08f4ed1dc5f11b391898477/a0ac605669047d15f184b395018924f38aaf71b0 clean |
| Locks/Ghostty/fresh3hosts |manifest.json,host-provenance.txt |Hash/pin verified; shared clones preserved |
| UI build first3 |commands.jsonl,completed-command-order-audit.json |All3 native0 before dependent gates |
| Scoped split/lifecycle3 |commands.jsonl,platform-verdicts.md |19/19 and112/112 each |
| Exact runner nonzero |runner logs,scripts-findings.md |Windows/Linux26selected5F21P native1; Mac NOT_RUN GUI_BOUNDARY explicitly |
| Four list/run selectors3 |rust-attempt-completion-audit.json,rawlogs |24 attempts101; list before run; selection UNKNOWN_COMPILE_FAILED |
| all-targets/full-lib3 |same ledger/rawlogs |6 attempts101 |
| Linux suspension/journal list/run |same ledger/rawlogs |4 attempts101 |
| Full UI3 once |49candidate commands and logs |Windows/Linux completed native1; Mac attempted1200s/SIGKILL TIMED_OUT, native/finalcount unknown |
| Every observed failed file A/B |platform-ab-classification.jsonl,baseline-classification.md |99Linux/11Windows/10observedMac covered; Mac full inventory remains unproven due timeout |
| Mandatory two/protected two |mandatory-ab-classification.md,Macrawlogs,pass1AB |Exact assertions reproduced; protected files untouched/out-of-scope |
| Exact argv/host/rawnative/assertion/count |platform-verdicts.md,baseline-command-verdicts.md,candidate-recovery-verdicts.md |49original+120AB+10supplemental; no skip as pass |
| Windows7/0 disk transferredPS1 |host-provenance.txt,stage/gatePS1 |Immediate7then0;186.12GB free |
| Bounded monitors/owned termination |runners,mac-full-ui-timeout.json |Timeout captured; no foreign termination |
| Both passes preserved |rootcommands.jsonl |49pass1+49candidate+120AB+10recovery=228 |
| Verbatim lane routing |lane-findings.md,compilerJSONL,all-platformrouting |Backend/native/frontend/scripts reopened, no repairs |
| Untracked omission named modules |tracked-closure-audit.json |All6 required script/test modules tracked |
| Every spawned resource cleanup |cleanup.json |All3staging/ownedtargets/archives removed; fixtures removed; sharedGhostty/foreignroots preserved |
| No product edits/commits/GUI/production/daemon mutation |clean source/current audit,boundaryrecords |Kept; foreign separate-target command disclosed, untouched |
| Final report |report.md |Candidate rejected, failures/unproven explicitly reported; Task9/10 not dispatched |

No claim of green acceptance or complete Mac full-suite coverage. Supplemental file runs recover observed assertions only. Mac timeout cause is unknown. Every required gate has a concrete passed/failed/not-run/timed-out disposition and evidence; no verifier product fix deferred.

