# bd4d3459 Windows verification

Exact remote HEAD bd4d3459d70e5de999c44fe2eaef63fadd87bdee; native 216426b657b93d8fadd11d7506615f744581221e. Integration merged into local evidence branch. All compilation/tests executed on maho-win; no product edits, local builds or paid resources.

Commands are preserved verbatim in gates.ps1 and runtime-build.ps1. Raw PowerShell logs are UTF-16; .utf8.log files are readable copies. Original gate script PID 16068 survived tool interruption; commands were not duplicated. Completion-session PID/log records a process-exit waiter. Foreign shared-target Cargo jobs, including PID 20896, were not terminated. Added-test exits.log entry was absent despite added.log containing EXIT=0; trust raw command receipt, not inferred ledger entry.

Results:
- FFmpeg prerequisite exit 0.
- Windows helper debug/release/actual measured receipt exit 0. Exact executable retained /tmp/rex-bd4-win-helper/ferryx-remote-helper.exe; windows-helper-receipt.json. Linux verifier coordination note windows-helper-handoff-bd4d3459.txt written in sibling evidence worktree. No full verified five-target staged helper tree received; fresh release blocked, not pass.
- UI frozen install/build exit 0.
- Original unchanged 70-case failure selection: 70 passed, 0 failed, exit 0. Includes descendant-denial cutoff and OSC expectation table. original.log.
- Added supervisor_peer and OSC module selection: 21 passed, 0 failed, exit 0. added.log.
- cargo check --manifest-path src-tauri/Cargo.toml --locked --all-targets exit 0.
- cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --locked --all-targets exit 0. Warning: NATIVE_DEBUG_SEQUENCE unused; not suppressed.
- Full unfiltered library: cargo test --manifest-path src-tauri/Cargo.toml --locked --lib -- --test-threads=1 --nocapture. Completed naturally, 5114 passed, 29 failed, 10 ignored, exit 101, test time 1055.56s. No blind timeout/termination; no baseline classification. Raw output identifies test names and panic locations; full failure inventory remains in log. stack.ps1 captures exact process and current output with optional non-invasive cdb stack on a matching runner; no stalled runner needed termination.
- Fresh runtime CLI: cargo build --manifest-path src-tauri/Cargo.toml --locked --bin ferryx-cli failed exit 101 at linker LNK1140, program database size limit exceeded. runtime-build.log. Smallest repair direction: investigate Windows link debug/PDB output limits in build lane; no verifier product/config workaround made. This blocks same-head live owner/pane, idle restart and enabled-plugin daemon-death QA; older executable was not substituted.
- MahoRD connection restored via owned TCP+UDP tunnels; fresh default display screenshot still uniformly green. Read-only diagnostics prove active console session 1, explorer and maho-host both session 1: not simply a service/session-0 capture. Prior peer evidence mentions native client display 1; local display-1 attempt returned no image. Reusing peer probe was blocked by missing native-configured-adapters.mjs in its referenced worktree. mahord-native-probe.log records exact missing-module error. No host replacement, production input or blind GUI operations. Windows GUI remains BLOCKED, not waived/pass.

Cleanup: remote worktree and its per-worktree helper target removed only after logs and helper executable/receipt were copied; owned bundles/scripts and TCP/UDP tunnels removed; production/foreign processes and shared target retained. See cleanup.log. Evidence was subsequently classified and committed as requested; no pushes made. See failure-classification.json (27 exact same-host baseline failures, one baseline-passed candidate failure, one unclassified failure), AUTHOR-PACKET.md and HELPER-HANDOFF.txt.
