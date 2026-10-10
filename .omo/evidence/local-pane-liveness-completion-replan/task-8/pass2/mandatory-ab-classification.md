# Mandatory failure attribution
Linux and Windows: both named failures are A/B-confirmed pre-existing against immutable base d82b35e43f208b53adf6310f4e3c89cbde8814f4. Candidate full-suite assertion blocks and corresponding base raw logs were inspected, not inferred from native exits.

TerminalSplitView.paneHandleReach.test.tsx:83: base and candidate expect h-3 but receive the same h-5 handle classes; base1selected1failed native1 on both hosts.
pairedDaemonRollout.test.ts:35: base and candidate resolve the same capabilities response with directoryBrowseV1/futureV9 instead of rejecting; base5selected1failed4passed native1 on both hosts.

Evidence: mandatory-ab-candidate-assertions.md (Linux), windows-ui-failures.jsonl; linux/windows-base-paneHandleReach.log and linux/windows-base-pairedDaemonRollout.log, mandatory-ab-baseline-proof.json. Mac base and supplemental candidate runs independently reproduce both exact named assertions, selected1 and5 respectively, native1; full blocks in mac-ab-classification.jsonl and raw logs in mac-base-ab/logs and mac-candidate-assertions/logs. These are out-of-scope baseline findings; no test/product edit. All three platforms covered.
