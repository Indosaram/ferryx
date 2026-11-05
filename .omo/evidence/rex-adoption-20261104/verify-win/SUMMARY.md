# Windows independent verification: 35c984684

Verdict: NOT APPROVED. Product code was not edited or committed. All executed builds/tests ran over SSH on maho-win, never on the Mac workstation.

## Proven results

- `cargo metadata --manifest-path src-tauri/Cargo.toml --locked --no-deps --format-version 1`: exit 0.
- Full `cargo test --manifest-path src-tauri/Cargo.toml --lib --locked`: exit 101; 5068 passed, 52 failed, 10 ignored. See `lib-test-corrected.utf8.log`, `failures.json`, `baseline-classification.json`.
- Same-host d59508fa9 baseline reproduced 43 failing test identities. Full baseline stalled; it was stopped by recorded runner PID, then matching failures were exercised directly with the baseline test executable. Do NOT call all 52 baseline. Two failures passed on baseline (`baseline-last-two.utf8.log`); seven additional failures are new tests or unclassified (including the blocked route test).
- Headless CLI/relay/account build: exit 0 (`headless-corrected.utf8.log`). This covers Luau C++ headless compilation, not the GUI-only harnesses.
- Default debug binary build initially failed LNK2019/LNK1120 in shared incremental output; `CARGO_INCREMENTAL=0` rebuilt successfully (`debug-nonincremental.utf8.log`). This is a diagnosed harness/cache retry, not a passing full-suite retry.
- Default release build: exit 101; ffmpeg-sys-next build script exited STATUS_STACK_OVERFLOW (`release.utf8.log`). Baseline release was not run; do not classify regression or baseline. Smallest repair investigation: reproduce FFmpeg bindgen/build-script stack behavior with valid SDK and clean release output.
- Named-pipe focused tests: 4 passed, exit 0 (`pipe-focused.utf8.log`), including restricted-token rejection, real pipe peer refusal, control refusal, and TCP transport compatibility.
- Real isolated debug daemon: all three per-SID `-dev` pipes present; live process environment read via ReadProcessMemory and saved before requests. Release/debug pipe names differ and no same-flavor pipe existed before launch. With daemon.port removed, `agents list --json` returned `{"ok":true,"sessions":[]}`. See `pipe-final-qa.log`, safety/live-env/pipes artifacts.
- T12 real cross-pane QA could not spawn panes: owner `registerWorkspace` returns FORBIDDEN because SSH ancestor PID 932 has exited. Repeated in independent SSH runs. `supervisor_peer.rs:399-425,513-516` walks historic Windows parent IDs and treats an already-dead ancestor as a transient change. Smallest repair shape: distinguish stable orphan/reused Windows ancestors from a race without admitting pane descendants. No repair applied. Earlier harness attempts also had a malformed Write frame after spawn failure; final harness stops at the actual owner refusal.
- Updater command-path local signed HTTP fixture QA: 22 focused tests passed, with observable UPDATE_HALTED, UPDATE_BELOW_FLOOR, UPDATE_MANIFEST_UNVERIFIED, and no downgrade (`updater-focused.utf8.log`). This is the plan's command integration-test QA, NOT installed-MSIX proof.
- T2 real CLI with isolated USERPROFILE: skill install/status, second install CHANGED_FILES=0, uninstall preserved marker-removed owner takeover. Unknown-agent emitted UNKNOWN_AGENT but PowerShell Stop mode interrupted exit recording; no claim of measured CLI exit for that one. Hook opt-out new test failed in full suite.
- MSIX template suite: 18 passed, 1 failed; localized missing-parameter error did not match English regexp (`msix-template-tests.utf8.log`). This is NOT an install test.

## Definite acceptance shortfalls

- T9/T10/T11: layout tests fail on invalid app window label `second` (`layout.rs:964,1070`); restart migration treats the persisted layout as foreign (`layout_migration.rs:234`). Focused rerun: 36 passed, 5 failed, exit 101.
- T30/T9: predecessor-alive assertions fail on Windows (`handover.rs:884,911`), where live handover is unsupported. Review platform scope of these assertions, not just compilation.
- T2: per-agent opt-outs boot-install test fails at `agent_extension.rs:1271`; inherited disabled-integration failure is independently baseline.
- Candidate-only failures with baseline pass: inventory event test sees agentControlChanged before expected sessionExited (`wip_desktop_inventory_tests.rs:404`); diagnostics entry count 2 instead of 1 (`ipc/debug.rs:1006`). Their global/parallel state sensitivity is not proven; do not mark flaky.

## Explicit blockers and uncovered risk

- MahoRD stored pairing reconnect timed out (`mahord-stderr.log`). No SSH GUI automation substituted. MSIX install, externally-managed updater in an installed package, ferryx:// activation/confirmation/single-instance, plugin-frame isolation and drag masking have NO passing real-surface evidence. No package was installed.
- T26 Windows two-host/UDP-blocked measurement not run: no isolated iroh relay or second-host probe was established; default binaries deliberately omit iroh-direct. NO-GO result from earlier Linux work is not Windows proof.
- Windows idle-restart recovery for ownership/layout/detached/onboarding and plugin repeated approval writes remains unverified. Owner pipe refusal prevents the planned real daemon setup; unit tests do not replace these QA scenarios.
- Headless supervisor explicitly uses managed_externally=false at `daemon/supervisor.rs:623-625`: build success does not prove MSIX behavior.
- Empty-active-pane layout rejection remains an integration semantic risk; no passing Windows GUI recovery evidence.
- No waived macOS scenario was taken on; macOS manual QA is SKIPPED BY USER, never PASS.

## Harness/setup provenance

Initial fixture errors (not product failures): local-file submodule transport disabled; FFMPEG_DIR set before fetch caused the fetch script to skip an absent SDK. Repaired explicitly using per-command `protocol.file.allow=always` and fetching with FFMPEG_DIR unset. Original raw logs retained. Initial detached Start-Process driver did not survive SSH teardown; subsequent scripts executed directly, with exit ledgers. Raw PowerShell UTF-16 logs and UTF-8 readable copies are retained.

## Cleanup

`cleanup.log`: candidate and baseline Windows worktrees removed and pruned, existence false; no per-worktree target created; shared target retained. No ci refs pushed, so none to delete. Host claim released exit 0. QA daemon recorded PIDs exited with SSH job teardown, final recorded PID already gone; precleanup lists remaining foreign processes untouched. No production daemon killed/restarted. Product build-generated files existed only in disposable remote worktrees and were removed with them. Local commit is evidence only.
