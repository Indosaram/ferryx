# Durable checkpoint - 2026-11-05

No product edits or local builds. Earlier fd85b422c evidence preserved.

## Completed handles
- d8313422 / GHA 41952594628: full default 5786 pass/13 fail; headless 4761 pass/13 fail. UI 9396 pass/17 fail. All raw exits in d8313422/gha/rex-verify-linux-gate/summary.txt. Exact-10000 live hash and detached handover QA pass. Size ownership QA times out. Clippy fails runner.rs items_after_test_module; headless also unused NATIVE_DEBUG_SEQUENCE. Release/proptest/fuzz fail helper freshness; never bypassed.
- 708e0cfb / GHA 41960022396: plugin 24 pass in each mode; plugin UI 3 pass; both all-target checks and typecheck pass. Both clippy gates fail. Raw logs 708e0cfb/gha-recovered/.
- bd4d3459 / GHA 41964894364: OSC 7 pass, supervisor 17 pass in each mode, both all-target checks pass, measured Linux x64 helper built. Raw logs bd4d3459/gha-recovered/.
- bd4 live corrected / 41968711562: real account unauthenticated PUT 401 observed, login request returned legitimate 202 but verifier incorrectly expected 200 (harness failure, not product). Extra daemon harness failed startup with ENOENT; all scenarios there remain unproven. Existing integrations/layout/script live harness passed. Raw logs bd4d3459/live-corrected-recovered/. First script-path failed attempt 41965762257 retained separately.

## Recovery st_01a1ab9b
See RECOVERY-st_01a1ab9b.md and 4167f91e/per-todo-receipts.json. Candidate 4167f91e remains unverified: GitHub DNS/direct HTTPS failed and picker found no idle host. No new runs, refs or leases created. Initial successful remote lookup showed inherited ci/rex-verify-linux* refs already absent. Historical SUMMARY.md/verdict.json are deliberately not relabeled.

## Completed since the prior checkpoint
03e0d6b5 targeted run 41970713685 completed: SSH refusal tests in both feature modes, both checks and bins exit 0; account/extra harnesses exit 1. Revision-specific commands/exits remain in revision-results.json.

03e0d6b5 run 41974624352: corrected account live gate exit 0, including unauthenticated 401, login 202/200, authenticated PUT/GET 200 and machine-only step rejection 400. Raw log 03e0d6b5/live-final-recovered/rex-verify-linux-live-gate/2.log.

03e0d6b5 run 41976072348: bins exit 0 (861s); live gate exit 1. Foreign layout import, exact timer once, compiled clock approval mismatch/disable PASS. Cross-pane grants FAIL at grants.rs:83 reactor panic, under separate repair. Artifacts 03e0d6b5/grant-gha/. These results are not evidence for 4167f91e.

## Helpers
Linux x64 receipt sourceFingerprint 8f02c2b160b5cb7369ab5c1c4a4e78d37b79ebcaa099d6c1621fbb37df23c295; Windows bd4 receipt 400dca582fba8fe412ca1158f9a6adcc87470f72271ac3826827112cfb26d56d. Source divergence diagnosed: all 19 Linux hashes match bd4 Git blobs; all 19 Windows hashes match the same blobs expanded to CRLF (4167f91e/helper-divergence.json). Cannot stage together; Windows needs an LF checkout plus a measured rebuild/receipt, not rewritten hashes. Exact binaries and receipts retained under revision dirs; Windows binary externally at /tmp/rex-bd4-win-helper/ferryx-remote-helper.exe. Still absent measured native target pairs: aarch64-unknown-linux-gnu, aarch64-apple-darwin, x86_64-apple-darwin. Both x64 receipts need a common canonical fingerprint before valid all-five staging. No paid host authorized. macOS manual QA waived, not PASS.

## Coverage still owed
At 03e0d6b5 the account round-trip/machine rejection, foreign import, timer once and clock approval/disable now have passing live receipts above; grants has a genuine failing receipt. All still need candidate-head coverage. Foreground secret suppression, OSC stream checks, Wasm daemon-death/adversarial boundaries, browser rendering isolation, real logged-in agents, Linux AppImage/dbus links, crash recovery and required release A/B benchmark remain owed. Tests are not substituted for those scenarios. Several are GHA runnable and remain OPEN, not blanket BLOCKED. Release long proptest/fuzz cannot execute until valid full helper staging exists. No matching baseline proof for d831 new control/OSC/ancestry failures; not classified baseline.
