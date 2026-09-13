# P02 native input/debug implementation - st_01a099ff

Status: frontend wheel context and portable debug sink repaired with actual RED/GREEN; four new backend wheel/mouse assertions changed from RED to passing. Full input target remains FAILED on three pre-existing detached-error expectations. No complete P02/native repair claimed. **Exclusive shared Cargo slot RELEASED after the four-run batch; P08 may acquire it.**

Resource state: **RELEASED**. Reconfirmed 2026-09-13T09:21:33Z: exact owned runner PIDs49652,50912,51270,52218 absent (ps exit1, no rows). P02 holds no shared-target reservation and will not start another Cargo process without a new grant. Lead may schedule P06/P08 immediately; this release does not assert packet acceptance.

Lead independent RED reconciliation: supplied GLXgU4/receipt.json confirms original debug source SHA c76dad51591ac715f7c9512d8c7566dc3b8105b6130fda7a7b2e3c1b690b9a93, exit101, finished09:10:30Z. Subsequent child debug GREEN finished09:17:38Z, 6pass/exit0, current debug source SHA ca3093aeb879d9353e11b059085271f691af1715456b178dbaa546937ad19103 matches recorded GREEN. Re-read all four writer call sites: repair already present, enabled/opt-in logic unchanged, lib.rs diff only assigned writer. No duplicate source edit or Cargo execution needed. Latest instruction now prohibits further input execution until preferences isolation; no additional input run started. The earlier input runs remain transparently recorded below, not erased or qualified as fixture-isolation acceptance. Slot remains RELEASED.

## Exclusive Cargo batch - latest evidence supersedes staging notes below

Lead explicitly granted `/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/{target,cargo}` and the clean lib.rs single writer. Reused P15 direct Rust1.98.1 sandbox/environment pattern with separately owned roots and Homebrew excluded from PATH. No user dev process touched. Four runner processes completed; ps on exact owned PIDs49652,50912,51270,52218 returned none.

| Exact Cargo command suffix after `cargo test --manifest-path src-tauri/Cargo.toml` | Before | After |
| --- | --- | --- |
| `--lib ipc::debug:: -- --nocapture` | exit101, 4pass/2 intended injected-root failures | exit0, 6pass |
| `--test native_terminal_input_boundary_contract -- --nocapture` | exit101, 12pass/7fail: four new intended assertions plus three existing errors | exit101, 16pass/3 existing failures; all four new assertions pass |

Logs and full command/env/source-hash/PID/exit receipts copied as `p02-backend-{red,green}-{debug,input}.{log,json}`. Build/link succeeded for both actual targets on Darwin; 16 lib/17 lib-test existing warnings retained, not suppressed. These are actual test executions, not inferred passes.

Minimal backend fixes after observed RED:
- Wheel command policy consumes provided pane-local point and modifiers, converts point to global logical coordinates for existing engine, validates finite input, and saturates rows before i16 conversion.
- Tracking mode reports all ordinary button/hover/release events; Shift selects instead. New real encoder matrix checks left/middle/right/1003 hover/right-release bytes.
- Sink uses injected portable root; all FOUR packet hardcoded-/tmp writers use same append implementation, native callbacks offloaded via spawn_blocking and errors traced. lib.rs diff is exactly single writer replacement. A fifth pre-existing Windows-only clipboard writer already uses temp_dir but still writes synchronously; this was discovered after slot release and is not silently claimed unified or fixed.

Pre-existing full-target failures: production_paste_boundary_rejects_detached_and_unattached_session at200; production_mouse_boundary_rejects_detached_and_unattached_session at243; selection_and_search_boundaries_reject_detached_and_unattached_sessions at545 expect NoValue. Current surface_host.rs946-952 returns SessionDetached(session_id), forwarded by unchanged require_attached_surface. Same failures existed in RED before behavioral changes. No test was deleted, ignored, filtered out or weakened. Parent may separately register exact variant/session-ID fixture correction; full target is NOT GREEN.

Fresh post-batch diagnostics: debug.rs no diagnostics; native_terminal.rs/lib.rs only inactive cfg hints. Initial diagnostic requests during compilation timed out/cancelled and are superseded by these fresh responses. Scoped diff check exit0. Owned fixture temp directories empty on all four exits; evidence roots retained intentionally, shared Cargo/cache untouched. Slot released immediately after test exits, before documentation work.

Remaining native focus/drop/IME/final-motion and explicit P06 fixture acceptance remain below; no Windows compiler/runtime or manual real-surface verification occurred. Earlier statements below about backend staging/unallocated lib.rs are historical and superseded by this batch.

## Authorized pane context increment

Latest user explicitly authorized forward layering over parent-owned GREEN. Read current pane diff: parent remainder/reset/deltaMode/clamp additions preserved unchanged. Official executeAgentToolkit revise_criterion appended exact context-only command/condition to full C002; accepted:true.

- Added mounted context test at real onWheel/invoke boundary: viewport rect(100,50), client(125,120) yields pane-local(25,70), all six modifiers true, exact backend ID/rows-1.
- RED `bun run --cwd ui test src/components/NativeTerminalPane.test.tsx`: exit1, new missing-wheel assertion failed; 167 existing cases passed. `p02-context-red.log`.
- Minimal product addition: wheel context mirrors existing mouse geometry/modifier contract. Existing strict payload expectations now include explicit default context while keeping all rows/session/generation assertions.
- Identical GREEN command: exit0, all168 tests pass. `p02-context-green.log` retains intentional error-fixture stderr.
- LSP both pane source/test: no diagnostics found.
- `bun run --cwd ui build`: exit0, tsc/Vite success; chunk-size warning retained in `p02-context-build.log`. No real native surface exercised.

Backend helper currently receives but ignores this context until registered Cargo RED. UI GREEN must not be reported as corrected backend/native delivery.

## Changes delivered

- `src-tauri/src/ipc/native_terminal.rs`: extracted `native_scroll_outcome`, called by the actual scroll command, with optional typed `wheel: {position:{x,y},modifiers}` parameter. Position contract is pane-local logical pixels, identical to mouse IPC. Extraction still deliberately chooses pane center/default modifiers and casts rows to i16 for baseline. Extracted `native_mouse_routes`, called by actual mouse command, retaining old plain-left bypass for baseline.
- `src-tauri/tests/native_terminal_input_boundary_contract.rs`: four added tests exercise these production policy seams and real Ghostty: Ctrl wheel at noncentral cell(2,3), Shift primary-history viewport dispatch with unchanged sibling, 65536/-65536 non-wrapping rows plus zero, and exact plain left/middle/right/1003-hover/right-release output plus Shift policy. No socket, PTY, GUI or daemon added. The tests use real terminal objects; the policy tests are not a full Tauri invocation/runtime proof.
- `src-tauri/src/ipc/debug.rs`: extracted actual append/path seam; production command uses it. Two added tests require injected root before any disk write, then deserialize event and compare two parsed JSONL records, or assert invalid-root error and preserved sentinel. Old absolute path retained deliberately for safe RED: path-parent assertion fails before touching shared `/tmp`.
- `p02-runner.mjs`: syntax-checked owned runner, requires phase/target/provisioned shared Cargo root/exclusive-slot receipt. Runs only the registered debug or input target, with isolated child environment/profile/temp, offline Cargo, jobs8, Homebrew excluded from PATH, explicit Zig, network/ambient-write sandbox, exact command/environment/source-hash/exit receipts. No runner execution yet. This runner is not a substitute for P06's explicit preferences fixture seam.

## Registration

Official `executeAgentToolkit` loaded from `/Users/indo/.bun/install/global/node_modules/omo-ai/plugin/extensions/omo-agent-toolkit.js`, with resolveCwd bound to this repository and resolveSessionId bound to `01a0983f-c995-753d-afa9-593f6d118788`.

First steer was rejected (`accepted:false`), so no registration inferred from it. Second `steer/revise_criterion` retained complete existing C002 scenario and appended P02 exact commands and binary requirements: tool result `{"ok":true,"accepted":true,"reasons":[]}`. Durable goals/ledger own the official receipt; no direct JSON editing.

## Required parent relay/action

1. Grant st_01a099ff exclusive Darwin Cargo slot and concrete provisioned shared root plus slot receipt. Runner interface is `bun docs/evidence/windows-review-20260913/p02-runner.mjs red debug <shared-root> <slot-receipt>`; angle-bracket values describe lead-supplied inputs, not runnable placeholders. Internal exact command: `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::debug:: -- --nocapture`. Require two intended path-parent failures and four existing enablement passes, not compilation errors/zero cases. Return RED receipt to child so path can change to `root.join("ferryx-switch-debug.jsonl")`, shared offloaded logging can replace all four writers, and identical GREEN can execute.
2. Coordinate P06 `terminal/preferences.rs` explicit owned fixture configuration before input target. P06's current `impl-p06.md` confirms that seam is still outstanding. Existing input attachment tests consult cached preferences/renderer metrics; do not certify ambient profile isolation merely from new direct-engine tests. Then execute runner target `input`: exact `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_input_boundary_contract -- --nocapture`. Require intended cell/Ctrl, Shift outcome, row-wrap, and plain-left assertions, then return logs for smallest fixes and same GREEN.
3. Coordinate `lib.rs` one debug writer (still untouched). Pane layering is now explicitly authorized and context repair above is GREEN; parent normalization remains intact. Backend helper accepts but intentionally ignores context pending Cargo RED.
4. P09 must supply authoritative target-shell/session contract: current drop handler still uses `paths.map(quoteShellPath)` where quoteShellPath is POSIX-only. Required native shell oracles: cmd spaces; PowerShell apostrophe; WSL translated path; remote POSIX quoting. Host-OS quoting is not an acceptable replacement. Pane layering is authorized, but authoritative shell metadata remains the actual prerequisite.
5. Sole Windows runtime owner st_01a099f8 must stage/run `cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::platform::windows_focus:: -- --nocapture` with nonzero tests and actual owned overlay HWND, not fabricated IDs. `windows_focus.rs` remains unchanged: desktop-wide low-level mouse-up uses only rectangle membership. The focus subfix needs real HWND/foreground filtering plus frontend dialog acceptance or demonstrated safe retirement after P01 hit-delivery proof. No native focus test or repair claimed here.

## Verification actually run

- Startup and pre-write status/diff: owned backend/debug/input-test files clean; foreign NativeTerminalPane dirty, lib clean. Foreign changes preserved.
- LSP `debug.rs`: no diagnostics found.
- LSP `native_terminal.rs`: only inactive-code hints for Windows and macOS cfg regions, no errors/warnings returned.
- LSP input boundary contract: no diagnostics found.
- `node --check docs/evidence/windows-review-20260913/p02-runner.mjs`: exit0.
- `git diff --check -- src-tauri/src/ipc/debug.rs src-tauri/src/ipc/native_terminal.rs src-tauri/tests/native_terminal_input_boundary_contract.rs`: exit0.
- Scoped diff: debug +54/-17; native_terminal +45/-31; input contract +86/-0.
- Cargo RED/GREEN/build: **not run**, no exclusive slot issued to this child. Windows compilation/native/manual/drop/focus: **not run**. Diagnostics are not test evidence.

## Remaining acceptance

All packet runtime matrix remains: owned real WM_MOUSEWHEEL -> DOM -> IPC -> viewport/PTY receipts; SendInput +120/-120 over 200 lines; split sibling isolation; primary/alternate/1000+1006/Shift/unfocused/horizontal/100%-150% DPI; fast selection release and window exit; independent Windows Space after compositionEnd distinct from WebKit replay; clipboard; foreground/dialog/drop-shell roundtrips. No criteria or whole-domain closure inferred.

## Cleanup

No Cargo build, runner child, daemon, GUI, OS settings, audio, native remote mutation, dependency cache provisioning, branch/worktree, commit/push or install was performed. Initial broad instruction-file discovery timed out and created no files/process owners requiring cleanup. The runner has not created fixture roots. Only owned source/test staging and this report/runner remain uncommitted; shared tree remains vulnerable to concurrent edits. Preserve it and continue after concrete slot/ownership relay rather than claiming the packet implemented.
