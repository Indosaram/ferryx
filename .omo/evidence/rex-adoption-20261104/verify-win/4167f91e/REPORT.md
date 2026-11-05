# Windows 4167f91e continuation

Remote source: 4167f91e1c2ed9c29e6959274f6f497e163d0269. Native 216426b657b93d8fadd11d7506615f744581221e imported using documented source bundle from owned repair submodule. Local evidence branch merged integration as bf1b7610f5f703e896001f51f637e90206b37fbd. Product code unchanged.

## Build

Host picker initially rejected busy Windows; omarchy claims released. Subsequent picker selected maho-win while omarchy was claimed. No surviving Rex cargo/rustc/zig build before launch. Sequential jobs used Start-Process -PassThru and WaitForExit (completion monitor), no polling.

Exact command in rex-4167-build.ps1:
`cargo build --manifest-path src-tauri/Cargo.toml --locked --profile windows-dev --bin ferryx-cli -vv`
Documented target, incremental=0, CMake policy and LIBCLANG_PATH, VS18 x64 developer shell. First PowerShell harness aborted on a native stderr warning (not a Cargo outcome); preserved harness-first files. Corrected harness did not suppress diagnostics and captured native exit. Initial actual Cargo exit101: missing ui/dist build resource. Repaired by frozen Bun UI install/build (both exit0), no source changes. Final CLI exit0, 2026-11-05T20:50:41.9453154+09:00 through 20:54:45.1960179+09:00. cli-final.log raw UTF16 and cli-final.utf8.log readable; binary.txt measured hash. Output .target/windows-dev/ferryx-cli.exe only. External debuginfo1 recorded in cli-retry.utf8.log (e.g. tauri line63903); local debuginfo2 and PDBPAGESIZE8192 recorded in cli-final.utf8.log. Actual final DLL /DEBUG link command not emitted on successful link; do not claim direct linker-command observation. Ordinary dev LNK1140 remains unresolved, not fixed by this opt-in success.

`cargo test --manifest-path src-tauri/Cargo.toml --locked --profile windows-dev --test platform_build_contract`: exit0, 6 passed, 0 failed, 0 ignored. Remote platform-contract.log and platform-contract-exit.txt (not downloaded before host loss); output captured in tool transcript. Prior 5116 pass/27 exact baseline failure set and 70/21 focused receipts remain historical; no full-suite rerun.

## Runtime observations and failure packet

Before launch source windows_pipe.rs confirmed cfg(debug_assertions) selects -dev and server.rs honors FERRYX_RUNTIME_DIR; build log records debug assertions and inherited dev. No occupied Ferryx daemon-dev pipe was found. HOME/USERPROFILE/LOCALAPPDATA/APPDATA, runtime/data/session paths isolated under C:\Users\sook\ferryx-wt\.evidence\rex-verify-win-4167\qa. Copied only fresh windows-dev CLI and SDK DLLs. Initial detached daemon PID28448 disappeared with SSH session; direct launch persisted as PID20508, emitted FERRYX_DAEMON_READY. Version invocation exit0, ferryx-cli 2026.1101.1. No production/foreign process was stopped.

Named pipe handshake PID20508 epoch1793879857234 binaryPath exact QA CLI; registerWorkspace qa-win -> registerWorkspaceOk; two spawnOk ids bd25efa2-7838-4e1b-aed5-0b119ffa72d9 and cb774a62-5eca-4ed0-bc31-bf0b25ad2c70. Cross-pane agents send PTY write -> writeOk but no pane-cross-write.exit artifact within subscribed 15s completion timeout. Bun exit1, pane command completion timeout. This is NOT a denial pass. Same symptom as prior d831 runtime harness; no root cause asserted.

State harness Bun exit0: onboardingSet/Get returned exact version1 completedSteps intro/features, dismissed=false updatedAt4167. Initial layout qa-win rejected non-app window label. Corrected main layout read returned layoutOk revision0 empty tabs, machineId bba76647-3e52-40c8-84db-530f8402570c. Minimal valid binary Wasm tick(i64) module SHA dfc917357979752430184394470acba3df62a7b4355f8a5846ce0d1825e34ea0 enabled with manifest consent; pluginsList confirms enabled=true. Actual --plugin-wasm-runner PID10332 parent20508, same QA executable observed. No daemon-death result yet.

Idle harness got handshake and main layoutOk, then no further log after panes list request. Remote SSH execution timed out40s. Subsequent read timed out20s; hostname probes failed banner exchange/connect timeout. Harness was intended to close both owned panes before stopping recorded daemon20508 and awaiting daemon and runner10332 exits; whether it reached stop is UNKNOWN. No exit or plugin death/restart/idle persistence claim. Host outage cause UNKNOWN, not attributed to product. Remote logs remain under C:\Users\sook\ferryx-wt\.evidence\rex-verify-win-4167: pane.log, state.log, idle-before.log, platform-contract.log, daemon-direct.log. Local scripts permit exact reproduction; all runtime observations above are tool-transcript receipts, not downloaded raw logs.

Windows GUI remains open, no pass/waiver. macOS GUI waived. No local Mac builds, paid resources, pushes, production restart, foreign process deletion or GUI manipulation.

## Cleanup blocked by unreachable host

Windows host claim released. Remote owned worktree C:\Users\sook\ferryx-wt\rex-verify-win-20261104 and QA artifacts remain; do NOT delete before recovering raw logs. Recorded owned daemon20508/runner10332 may remain. Shared .target untouched. Remote final worktree-list/cleanup cannot be run while SSH unreachable; earlier full list retained in tool transcript. Local evidence worktree retained intentionally for parent integration and evidence preservation. Next recovery must inspect these exact PIDs/path/creation identity before any termination, download logs, then remove owned worktree with submodule-aware procedure and prune. Do not stop unrelated candidate daemon15404/supervisor26580 or GUI20516.
