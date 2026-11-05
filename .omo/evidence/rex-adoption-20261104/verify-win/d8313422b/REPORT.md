# Windows re-verification, revision-separated

## d8313422bde72275f08a1db6bf789597edc46d34

- Local verification branch merged integration; remote worktree initialized at 35c984684, imported Ghostty source bundle from owned rex-repair-terminal-build-20261104 submodule, then checked out exact native 216426b657b93d8fadd11d7506615f744581221e. No local builds.
- Fresh native Windows SSH helper debug/release builds and measured helper receipt passed. Exact artifact copied to /tmp/rex-d831-win-helper/ferryx-remote-helper.exe; receipt windows-helper-receipt.json. Linux verifier artifact handoff written to its evidence directory. This proves Windows artifact only, not five-target staging.
- UI frozen install/build and default debug binary build exit 0; exits.log and raw UTF-16 plus readable logs retained.
- Full locked library run: TIMEOUT, not PASS. Compiled in 6m42s; candidate test runner PID 33368 produced no output after 11:24:55 and remained CPU-active through 11:52:07. Stopped only that recorded runner after >27 minutes without output. Original run not duplicated. full-lib-timeout.log records observations. Thirteen tests had already reported FAILED (see lib-test log); no suite-total claim.
- Headless CLI/relay/account build exit 0, with a dead-code warning (not suppressed). headless.log.
- Exact owner/pane regression fails: supervisor_peer.rs:648 classify_with_cutoff(descendant,...Some(200)).is_err() false, exit 101. owner-regression-focused.utf8.log. Smallest repair shape: do not let cutoff truncate a chain proving a live pane descendant; preserve recovered-owner behavior.
- Real SSH owner registration and two panes spawn successfully in isolated debug daemon (pipe-live.log). Previous exited-ancestor owner refusal no longer reproduced. Per-SID release/debug names compared before launch and live process environment inspected. However PTY-driven cross-pane CLI command did not produce completion artifact within 15s: pipe-pane-denial.log. This is NOT pane-denial proof. Do not infer policy pass from owner success.
- Clock sample compiled remotely with Zig cc wasm32-freestanding, explicit -fno-sanitize=all (Zig's default UBSan otherwise adds unresolved imports). Actual Wasm module in trusted plugin runner emitted pluginRender clock HTML, then EOF teardown exited 0 (clock-live.log). This is actual runner execution, not GUI rendering proof.
- MahoRD timeout diagnosed non-destructively: direct Tailscale TCP timed out while host loopback listener existed. TCP+UDP forwarding via SSH permits stored pairing handshake and frame decode (mahord-full-tunnel.log). Bootstrap PIN was rejected; stored pairing succeeded. Both H264 and requested HEVC screenshots remain solid green (desktop.png, desktop-hevc.png), so GUI observability remains BLOCKED. No production daemon/app changes, no blind GUI input.
- Frozen source audit REJECT was reported by parent for consent race/render document bounds; never called runtime PASS. Those checks were subsequently focused at R2 below.

## 708e0cfb767a02e3d93db1661d6bf5035480f1f9

Advanced only after frozen pending library run was explicitly timed out and logs preserved. No full-suite rerun.

- `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib -- daemon::plugins`: 24 passed, exit 0. New expected-manifest consent identity and global/per-plugin render view quotas, replacement accounting, cleanup/reclaimed capacity, actual bridge budget rejection passed. Forbidden imports, fuel/memory/pointer limits, grants and disable reap tests also passed. ../708e0cfb/plugin-tests.utf8.log.
- `cargo check --manifest-path src-tauri/Cargo.toml --locked --all-targets`: exit 0. ../708e0cfb/compile.utf8.log. Another foreign local-split-qa Cargo build held the shared target lock; did not kill it or start duplicate checks.
- Original candidate failures focused using the exact R2-built test binary: 68 passed, 2 failed, exit 101. Layout/migration/handover, hook opt-outs and diagnostics count now pass in this invocation. Remaining failures: supervisor_peer.rs:648 cutoff assertion, and osc_effects.rs:214 expected Windows cwd C:\Temp but got None. ../708e0cfb/original-failures.utf8.log. Repair shape for OSC: inspect Windows file URI conversion/native callback fixture and drive-letter path preservation, without weakening expectation.
- These are R2 receipts, not a d831 same-head pass. Source approval b66d70f19 is not substituted for these runtime checks.

## Still NOT verified

No complete same-head full suite. No default release build in a fresh output directory after verified five-target helper staging: only Windows artifact was supplied and staging completion was unavailable. Previous release pass/failure is not carried over. No MSIX install, installed externally-managed updater, deep-link confirmation/single instance, GUI plugin iframe/CSP isolation or drag masking: MahoRD frames are green, not a usable desktop. No real enabled-plugin daemon-death/restart lifecycle or Windows idle state recovery proof. Existing focused tests are not claimed as those manual scenarios. Windows QA is not waived. macOS manual QA waiver was not extended here.

No product changes by verifier. Evidence-only scripts corrected for PowerShell quoting/runner synchronization; unsuccessful attempts retained where captured. Cleanup log records owned worktree/target/bundles/tunnels removal and claim release. Shared targets and foreign workloads retained.
