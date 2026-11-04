# Independent mac verification of 35c984684

Verdict: NOT APPROVED. Exact-head mac full library tests fail (5765 passed, 35 failed, 7 ignored); handover integration fails (11 passed, 3 failed); release build fails helper freshness. No product code changed. See verdicts.json and failures.json for machine-readable details; raw/ contains exact command, exit ledger, and full logs.

## Proven regression
`cargo build --release --manifest-path src-tauri/Cargo.toml --locked` exits 101. src-tauri/build.rs:84 rejects bundled helper manifest fingerprint 3abee5f4... against source 424c02c8.... On main d59508fa9, the exact gate `bun scripts/build-remote-helpers.mjs --check-bundled "$PWD"` exits 0 (baseline-helper-gate.log). Repair shape: rebuild/stage helpers and manifest; do not bypass the gate. Baseline full release was started but stopped after this discriminating gate comparison; no baseline full-release success claimed.

## Acceptance and observables
- UI frozen install/build, FFmpeg fetch, locked metadata, debug CLI build, headless hook integration, and generated llms check succeed.
- Isolated layout real-socket QA succeeds: LAYOUT_QA_OK revision=4 and LAYOUT_CONFLICT tested by shipped helper.
- Same-build isolated `scripts/verify-daemon-handover.mjs <cli> <cli> --v5` succeeds. This is NOT the 4-way matrix: old/new GUI parts remain unverified.
- Short TMPDIR hash handover produces equal grid/text hashes before/after, but retainedLines=9912 rather than 10000; exit 1. Unit continuation test reports 9910. Failure at scripts/qa/screen-hash-handover.mjs:105. Default mac TMPDIR also exceeds SUN_LEN at handover. Repair shapes: short isolated socket path; reconcile exact retention contract with page-granular pruning without weakening requested fidelity.
- Detach QA with TMPDIR=/tmp reaches handover then fails scripts/qa/detach-attach.mjs:23: `events?.next is not a function` under Bun 1.4.2. Repair shape: obtain async iterator explicitly. Initial default-TMPDIR attempt exited daemon 0 before readiness.
- Actual debug cmd_updater_check via local signed-manifest server reports UPDATE_HALTED, UPDATE_BELOW_FLOOR, UPDATE_MANIFEST_UNVERIFIED, downgrade offer=None. PASS.
- Real foreground PTY test passes; no live CLI foreground transcript claimed.
- FFmpeg source/resource diff against d59508fa9 is empty.

## Important failed contracts
See failures.json for all exact first errors, lines, commands and raw log. New layout tests reject fixture window label "second"; migration's machine identity turns restored state into a foreign offer; predecessor-alive assertions fail for layout/plugins; OSC cwd expected /tmp but got None; resize-on-close restore call count is 0 not 1. Integration lock assertion reports identity.lock free while successor serves. Missing remote-helper binary causes many SSH test failures and is a verifier prerequisite deficiency, not proof of product regressions. The remaining mac suite failures have NOT been compared to same-host main and are deliberately unclassified, not called baseline.

## Explicit blockers / fidelity limits
System Events says UI elements enabled=false and `screencapture -x <isolated evidence path>` exits 1: could not create image from display (progress-3.log). No production app was launched or targeted. Screenshots, split/drag/restart layout, deep-link confirmation, onboarding file/no-daemon restart, native Luau binding, Import preview/persistence, and plugin iframe runtime isolation + drag masking cannot receive a PASS. T30 iframe source inspection is not real runtime isolation evidence. Do not accept its provisional deviation based on this leg.

Full test execution is not manual QA. T2, T5, T23 have only test coverage here, not the required live scenarios. T4 latency baseline comparison, interrupted layout transfer/swap recovery, paired/draining size ownership, and full GUI old/new matrix remain unverified. Headless Windows managed_externally=false is out of this leg; default-feature GUI harnesses were not compiled by headless builds and were not treated as covered. Layout sync's null active pane path was inspected (ui/src/lib/layoutSync.ts:233,239 throws) but not classified without real-surface reproduction.

## Host and cleanup
All builds/tests ran on CQFQ4P2LXK via local UDS Ferryx relay, serialized under /tmp/rex-mahomac.lock. Local Bun was used only as relay transport; no local builds/tests. Exact candidate and baseline remote worktrees removed and pruned, pushed ci/rex-verify-mac-20261104 deleted, relay lock released. Existing shared host target cache was used (no per-worktree targets created) and retained. Recorded own baseline cargo and descendants were stopped, never production processes. The fallback remote relay shell was borrowed, not closed. Cleanup logs prove removal.
