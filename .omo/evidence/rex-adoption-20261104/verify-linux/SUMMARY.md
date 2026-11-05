# Independent Linux verification - 35c984684

Verdict: FAIL, with incomplete live coverage explicitly BLOCKED in verdict.json.

Frozen candidate run: https://github.com/Indosaram/ferryx/actions/runs/41917889082
Supplemental baseline run: https://github.com/Indosaram/ferryx/actions/runs/41929465914

- Default library: 5752 passed, 30 failed, 7 ignored. Baseline default: 5583 passed, 20 failed, 7 ignored. Exactly 20 matching named failures are baseline; 10 candidate failures are new.
- Headless library: 4731 passed, 26 failed, 6 ignored. Main cannot compile this configuration; do not classify its candidate failures as baseline by inference.
- UI: 9096 passed, 314 failed, 15 failed files. Baseline 9315 passed, zero failures.
- Lint: 7 errors, 79 warnings vs baseline 6 errors, 79 warnings. New invalid useScripting hook call in App.tsx:682.
- Both candidate clippy configurations clean; same-runner-image default baseline clean, headless baseline fails compilation.
- Headless three-bin build, default bins/examples with local-split-qa, iroh feature check, UI build, test typecheck, llms catalog check PASS.
- Size-ownership real gateway/WebSocket QA PASS including viewer denial.
- Live isolated daemon: integrations skill ownership/unknown-agent checks, layout split/move/close plus conflict, Luau startup/error interruption responsiveness observed. No claim for unexecuted happy timer behavior or GUI bindings.
- Screen hash preserved across actual handover, but 9963 retained != 10000; QA FAIL. Detach QA fails at Bun watch iterator API on successor launch.
- Release, release proptest, nightly fuzz builds FAIL at helper freshness gate; fuzz did not run 1800 seconds. Baseline release timed out, so the freshness failure is not classified baseline.
- Candidate debug benchmark p95 128us (10000 requests). This is not the required release A/B delta.

Per-failure exact commands, exits, raw paths, first assertions/file:line are in rust-failures.json and ui-failures.json; workflow summary and raw logs retain all failures. Failed suite does not erase individual passing tests, nor does an individual passing test substitute for a live QA scenario.

No product edits; evidence only. MacOS manual QA SKIPPED BY USER, not PASS. Windows remains the separate leg. CI refs deleted; no persistent host claimed; baseline worktree EXIT cleanup removed/pruned it; ephemeral targets destroyed with runners.
