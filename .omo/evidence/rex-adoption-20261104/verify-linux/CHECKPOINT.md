# Durable checkpoint - 2026-11-05

No product edits or local builds. Earlier fd85b422c evidence preserved.

## Completed handles
- d8313422 / GHA 41952594628: full default 5786 pass/13 fail; headless 4761 pass/13 fail. UI 9396 pass/17 fail. All raw exits in d8313422/gha/rex-verify-linux-gate/summary.txt. Exact-10000 live hash and detached handover QA pass. Size ownership QA times out. Clippy fails runner.rs items_after_test_module; headless also unused NATIVE_DEBUG_SEQUENCE. Release/proptest/fuzz fail helper freshness; never bypassed.
- 708e0cfb / GHA 41960022396: plugin 24 pass in each mode; plugin UI 3 pass; both all-target checks and typecheck pass. Both clippy gates fail. Raw logs 708e0cfb/gha-recovered/.
- bd4d3459 / GHA 41964894364: OSC 7 pass, supervisor 17 pass in each mode, both all-target checks pass, measured Linux x64 helper built. Raw logs bd4d3459/gha-recovered/.
- bd4 live corrected / 41968711562: real account unauthenticated PUT 401 observed, login request returned legitimate 202 but verifier incorrectly expected 200 (harness failure, not product). Extra daemon harness failed startup with ENOENT; all scenarios there remain unproven. Existing integrations/layout/script live harness passed. Raw logs bd4d3459/live-corrected-recovered/. First script-path failed attempt 41965762257 retained separately.

## Pending
03e0d6b5 live/targeted job launched from 03e0d6b5/commands.txt and driver.log. It runs exact SSH remote-membership refusal test in both feature modes, both compile checks, bins, corrected account login expectation, and extra timer/foreign-layout/compiled-clock harness. It does not duplicate a surviving job.

## Helpers
Linux x64 receipt sourceFingerprint 8f02c2b160b5cb7369ab5c1c4a4e78d37b79ebcaa099d6c1621fbb37df23c295; Windows bd4 receipt 400dca582fba8fe412ca1158f9a6adcc87470f72271ac3826827112cfb26d56d. Many source-file hashes differ (possible checkout representation issue, not diagnosed); cannot stage together. Exact binaries and receipts retained under revision dirs; Windows binary externally at /tmp/rex-bd4-win-helper/ferryx-remote-helper.exe. Still absent measured native target pairs: aarch64-unknown-linux-gnu, aarch64-apple-darwin, x86_64-apple-darwin. Both x64 receipts need a common canonical fingerprint before valid all-five staging. No paid host authorized. macOS manual QA waived, not PASS.

## Coverage still owed
Live grants, foreground secret suppression, OSC stream checks, account authenticated round-trip/machine-step rejection, foreign snapshot import, exact timer-once, Wasm clock approval/disable/daemon-death and adversarial boundaries, browser rendering isolation, real logged-in agents, Linux AppImage/dbus links, crash recovery and required release A/B benchmark. Tests are not substituted for those scenarios. Several are GHA runnable and remain OPEN, not blanket BLOCKED. Release long proptest/fuzz cannot execute until valid full helper staging exists. No matching baseline proof for d831 new control/OSC/ancestry failures; not classified baseline.
