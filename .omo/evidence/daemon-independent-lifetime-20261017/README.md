# daemon-independent-lifetime-20261017: final records

- Plan: `.omo/plans/daemon-independent-lifetime-20261017.md`. All 15 todos are [x]. F1 and F2 are [x]. F3 and F4 are [~] because macOS was verified from source only, by the user's waiver.
- The work ends at main 8fdebe2b. The final QA ran on cfe34b8e; the only later commit is the harness O1 fix in 8fdebe2b, which was proven by a scoped Windows run.
- Final verdicts:
  - F1-compliance-r4.md: APPROVE-SOURCE-ONLY-MACOS.
  - F2-review-r4.md: APPROVE-SOURCE-ONLY-MACOS.
  - F4-fidelity.md: PASS-PARTIAL (macOS source-only).
- F3:
  - F3-linux-summary.json: runId 20261022t175629z, RUN_EXIT=0.
  - F3-windows-summary.json: runId 20261022t184142z, RUN_EXIT=0.
- Gates:
  - t12-regate-review.md (T12)
  - hubfix-gate-review.md (22a8ecea, Windows hub build)
  - o1-xml-residue-gate-review.md (4a3c6856, QA task XML residue)
- ledger.jsonl: this work's rows, extracted from the shared .omo/ulw-execute/ledger.jsonl.
- User approval of the final wave: "ㅇㅋ" (2026-10-23).
- The full evidence tree (about 296 MB of logs, screenshots and run dirs) stays outside the repo at ferryx-wt/.evidence/daemon-independent-lifetime-20261017/.
