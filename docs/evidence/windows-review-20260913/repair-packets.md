# Windows implementation packet map - 2026-09-13

Status: proposed implementation map, not authorization to edit source or launch anything now.
All confirmed IDs in findings.md have exactly one packet owner below. Grouping follows disjoint file ownership, not arbitrary bug counts. Each owner may implement several small fixes sequentially within its packet.
Source prefixes: `R/` = src-tauri/src/, `N/` = R/native_terminal/, `U/` = ui/src/. Braces enumerate files; test trees named below are exclusively owned by that packet.
No packet may expand into another packet's file: request its owner to implement that seam and serialize the dependency. Foreign-owned files require forward coordination first, never restore/reset/cherry-pick over live changes.

## Universal RED/GREEN and runtime contract

- Commands below are exact existing package scripts/Cargo targets or module filters derived from current files. Added regression functions go inside those existing test modules/targets. A command being listed does NOT mean its proposed regression already exists or was run.
- RED: register behavioral regression before product edit, run listed command once against unfixed source, retain exit/output and prove the intended assertion fails. GREEN: after minimal repair, run the IDENTICAL command/assertion, retain output/count. Incidental missing prerequisite, zero selected tests or unrelated compiler failure is not valid behavioral RED.
- Test repair packets first preserve actual original failure output, then replace invalid/nondeterministic fixtures without skipping platform behavior. No fixed sleeps, waitFor/polling success, or prose assertions. Subscribe to actual request/ready/exit/present/state event before triggering; bounded timeout is only a failure deadline. Use controlled scheduler/clock when time itself is tested.
- Future Rust commands must run in an authorized isolated checkout/profile: distinct FERRYX_RUNTIME_DIR, FERRYX_DATA_DIR, FERRYX_SESSION_DIR, HOME/USERPROFILE/APPDATA/LOCALAPPDATA, owned temp roots/ports/children and verified binary paths. Build/test dependencies and helper executable provenance must be recorded. This document creates no checkout, environment or process.
- P19 isolation is mandatory before daemon_persistence_contract execution. No unwrapped `cargo test`, `--all-targets`, daemon reset, global PATH/env mutation in concurrent tests or user-profile extension installation. Some lib fixtures also spawn children: audit exact selected tests before execution.
- Future desktop QA must use exactly `bun tauri dev` from the isolated owned checkout, never the stale installed executable. Preserve user daemon and installed PID identities/start times. Record source tree, binary hash, platform/DPI and output receipts; clean only positively owned resources after signals/exit.
- After each packet: diagnostics on all changed files, listed tests, affected real surface. Combined multi-file work additionally requires authorized `bun run --cwd ui build` and `cargo build --manifest-path src-tauri/Cargo.toml` on target host, plus Windows native runtime matrix. None executed during synthesis.

## P01 - PR #2 child hit testing

- Category: deep (Win32/input). IDs: native-input-01. Files: `N/platform/windows.rs`, new PR file `N/platform/windows_pointer_tests.rs`; no frontend/focus-hook ownership.
- Exact RED/GREEN after staging PR regression alone: `cargo test --manifest-path src-tauri/Cargo.toml --lib presented_terminal_yields_cross_thread_pointer_hit_testing_to_input -- --nocapture` on native Windows; then existing lifetime filter `cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::platform::windows:: -- --nocapture`.
- Scenario: isolated desktop production constructor visible after reveal, underlying input HWND remains target. Then real WebView2 pointer down/move/up selects known sentinel while native pixels stay visible. WindowFromPoint is not a wheel-message test.
- Dependency: P18 Windows linker/test preflight; no need to wait for P02 source changes. Feed native delivery evidence to P02 before deciding whether focus workaround can be retired.

## P02 - Native pane input adapter and portable debug sink

- Category: deep (frontend/input/backend adapter). IDs: native-input-02/03/04/06/07/08/10, BROWSER-IPC-B11. Files: `U/components/NativeTerminalPane.tsx`, its `.test.tsx`; `N/platform/windows_focus.rs`; `R/ipc/native_terminal.rs`, `R/ipc/debug.rs`, `R/lib.rs`; `src-tauri/tests/native_terminal_input_boundary_contract.rs`. Foreign lib.rs additions must be preserved.
- Exact RED/GREEN: `bun run --cwd ui test src/components/NativeTerminalPane.test.tsx`; `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_input_boundary_contract -- --nocapture`; `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::debug:: -- --nocapture`. Put focus-hook regression in existing windows_focus module; run `cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::platform::windows_focus:: -- --nocapture` and require nonzero count after adding it.
- Register separate cases: actual scroll command preserves cell(2,3)/Ctrl; Shift+tracking scrolls viewport with zero PTY bytes; horizontal/zero no-op, line=3, page=visible rows, fractional pixels accumulate and rows never wrap i16. Plain left/right/middle/1003 hover report exact bytes; Shift selects; held-button release correct.
- Controlled-rAF down/move/up before frame must select release cell via real gesture engine; never just assert Release called. Independent Windows Space after compositionEnd must be delivered; retain WebKit replay fixture distinctly. Owned overlay HWND click must emit no native-focus; dialog input keeps activeElement through exact callback/scheduler completion.
- Drop fixture round-trips cmd spaces, PowerShell apostrophe, WSL translated path and remote POSIX quoting through actual target shells. Use existing session metadata; if needed metadata contract change belongs to P09 and blocks this subfix, not a host-OS quoting shortcut.
- Debug regression injects sink root, invokes parsed log event, reads JSONL; fix all four existing /tmp switch-log writers via same portable sink, not only IPC debug.rs.
- Runtime: instrument owned native WM_MOUSEWHEEL -> DOM wheel -> IPC receipt -> viewport/PTY bytes BEFORE real wheel up/down over each split; primary history, alternate screen, 1000+1006, Shift, unfocused pane, horizontal touchpad, 100%/150% DPI. Also fast selection/window exit, IME, clipboard, drop and foreground isolation. Record intended pane offset and untouched sibling state.
- Dependencies: P01 native hit transparency; P03 punctuation; P09 shell metadata if absent. Preserve established Ctrl+W close, Ctrl+V paste and Ctrl+click links; neither P02 nor P04 authorizes default-policy changes. Do not add a second native wheel route without proving DOM missing and preventing duplicate delivery.

## P03 - Legacy punctuation encoding

- Category: deep. ID: native-input-05. Files: `N/key_encoder.rs`, `src-tauri/tests/native_terminal_engine_contract/key_encoding.rs` (no vendor edits).
- RED/GREEN: `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_engine_contract key_encoding -- --nocapture`.
- Regression: fresh real Ghostty terminal, Character backslash/`]`, Ctrl, utf8=None -> [28]/[29]; Ctrl-[ contract for legacy/fixterms and negotiated Kitty cases separately. Runtime P02's byte-capture child receives exact controls from physical Windows key input, not only direct encoder calls.
- Dependency: P18 link prerequisites; otherwise independent of P01/P02.

## P04 - Duplicate workspace/tab bindings; existing policies preserved

- Category: deep (frontend logic). Repair ID: WIN-UI-03 only. WIN-UI-01 and native-input-09/WIN-UI-04 are policy constraints, not default-remapping work. Files: `U/lib/shortcuts.ts`, `U/lib/shortcuts.test.tsx`.
- RED/GREEN: `bun run --cwd ui test src/lib/shortcuts.test.tsx`.
- Cases: under isMac=false, demonstrate duplicate Ctrl+digits currently shadows workspace selection; corrected workspace and tab shortcuts must independently reach their intended handlers. Retain existing non-digit bindings, IME/AltGr gates and Mac behavior. No regression assertion may require bare Ctrl+W/V or other established app chords to become PTY control bytes.
- Runtime: Ctrl+digits still selects tabs; distinct workspace shortcut selects workspaces. Recommend Alt+digits for workspaces, consistent with existing guest bridge, limited to this duplicate mapping. Preserve Ctrl+W close, Ctrl+V paste and Ctrl+click links unchanged; no wholesale Ctrl+Shift remap or new shortcut preference is authorized.
- Dependencies: P02/P05 preserve these policies, not wait for a new policy decision. PR #3 continues to require the existing Windows Ctrl+W path and native close action, with same-sibling-PTY survival evidence.

## P05 - PR #3 optional tab kind and focused browser actions

- Category: deep (frontend lifecycle). IDs: WIN-UI-02/13. Files: `U/App.tsx`, `U/App.test.tsx`; verified existing `U/state/workspaceStore.test.tsx` if integration coverage needed. No production store/types ownership unless separately coordinated.
- RED/GREEN: `bun run --cwd ui test src/App.test.tsx`; register Windows platform fixture (not only metaKey) and native-menu action. Require tagged/untagged, pinned/unpinned, split/unsplit, browser/nonexistent tab cases. Existing PR mocks are wiring evidence only.
- Preserve foreign handleClosePane/active-agent confirmation logic. Apply optional-kind discriminator forward; do not restore old App or overwrite new confirmation tests. Focused browser leaf inside terminal tab must enable address/find/history action and target that leaf, not merely flip a boolean.
- Runtime: two owned split PTYs publish unique PIDs/readiness/output; close active leaf via app action/native menu, honor active-agent confirmation, await only selected exit, then request fresh sibling echo and verify SAME sibling PID/session. Browser leaf focus -> actual address/find/reload behavior. Pinned whole-tab guard remains effective.
- Dependencies: owner approval for overlapping foreign edits; preserve existing Ctrl+W policy independently of P04's digit-only repair; P08 safe daemon fixture if required. PR #3 head 99c7086b remains unmerged.

## P06 - Font pipeline, cache, DPI and renderer tests

- Category: deep (GPU/FFI). IDs: RF-01/02/03/04/06/08. Optional capability RF-05 requires explicit parity approval, not assumed. Files: `N/renderer/{directwrite_raster,font_manager,color_glyph,types,renderer,atlas,row_cache}.rs`, `N/surface_host.rs`; `src-tauri/tests/native_terminal_renderer_contract/` tree, `src-tauri/tests/native_terminal_surface_host_contract.rs`. Foreign renderer/image changes require owner-safe adaptation.
- RED/GREEN: `cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::font_manager::tests:: -- --nocapture`; `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_renderer_contract -- --nocapture`; `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_surface_host_contract -- --nocapture` on native Windows GPU host.
- First fix variant/stride fixtures, preserving known-font assertions. GDI seam queues drawing until Flush; real raster compare against explicitly flushed reference. Face-stack case records GetTextFaceW, missing-first-family fallback, quote/generic handling; metrics follow measured font advance/height/baseline at 13/17px and scales 1/1.25/1.5/2.
- Cache regression: render A, preferences switch B same size, await render completion, retained output equals fresh B and known A differs. DPI regression: 80x24 unchanged grid, 8x16 ->16x32 cells through normal and warm attach; actual Ghostty pixel reply matches receipt. Add config fields only when necessary and account for external literals before implementation; any additional file must be allocated before parallel work.
- Runtime: settings font change affects existing/new glyphs uniformly; ASCII/CJK/PUA/combining/emoji/style/box lines and DPI switch show unclipped correct pixels. Record GPU/format/font inventory. Optional color emoji must produce real Color buffers/ZWJ colors, not disable tests or assume tofu from GDI absence.
- Dependencies: P18 target linking; foreign renderer owner. Never relabel old installed startup failure as current renderer defect.

## P07 - Native toast visibility

- Category: deep (frontend/native overlay). ID: RF-07. Files: `U/lib/nativeTerminalVisibility.tsx`, `U/lib/nativeTerminalVisibility.test.tsx`.
- RED/GREEN: `bun run --cwd ui test src/lib/nativeTerminalVisibility.test.tsx`.
- Mount real Toaster with non-Mac visibility provider, subscribe to mutation/visibility transition, issue persistent toast, assert visible-toast policy yields child while retaining session; dismissed toast restores presentation. Empty always-mounted toaster must not hide terminal indefinitely.
- Runtime: pane error toast fully visible/clickable over bottom-right terminal; dismiss restores same terminal PID/session and viewport. Dependencies P01/P02 runtime observation, no source overlap.

## P08 - Daemon security, upgrade recursion, local state and remote cwd

- Category: deep (backend/protocol). IDs: DS-01/02/05/07/09, FSSH-02. Files: `R/daemon/{server,client,protocol,proxy,handover,manifest}.rs`, `R/terminal/{pty,tests}.rs`, `src-tauri/resources/agent-extensions/ferryx-agent-state.ts` only if existing protocol needs alignment. No agent discovery/installer files. Preserve foreign PTY image env changes.
- RED/GREEN: `cargo test --manifest-path src-tauri/Cargo.toml --lib daemon:: -- --nocapture`; `cargo test --manifest-path src-tauri/Cargo.toml --lib terminal::tests:: -- --nocapture`. Select/audit child-launching fixtures for owned env before execution; existing Unix-only tests give no Windows evidence.
- Before auth fix: ephemeral actual listener accepts unauthenticated ListSessions/fixture Write; regression requires rejection and no fixture input. Authenticated control AND attach/proxy reconnect must succeed; invalid/foreign-writable DACL fixture rejected, normal private ACL accepted. Capability protected at publication, not just obscured port.
- Stale fake daemon: schedule/admission barrier captures internal UpgradeBinary; Unsupported/Deferred replies cannot schedule recursively. Local extension state listener readiness precedes env advertisement; actual bundled extension sends state/provider ID and subscribed stream receives exact frame.
- Remote config regression: `C:\Repo` + `c:\repo\src` and slash/UNC aliases yields correct relative subpath or explicit error, never silent None. Real remote helper describe reports requested worktree/cwd. ConPTY tests use own shell/output sentinel and actual exit signal; replace /bin/sh and polling fixtures.
- Runtime: owned second-account unauthorized control denied; new GUI/old isolated daemon has bounded upgrade attempt; agent working/blocked/idle and provider ID arrive; reopen remote selected checkout stays there. Preserve user daemon; active-session handover remains separately scoped feature gap.
- Dependencies: P18/P19 harness gates, P09 extension installation and execution discovery. Coordinate protocol change with all connection clients before publishing new daemon; no blind restart fallback.

## P09 - Agent availability, shim launch and authoritative discovery

- Category: deep. IDs: DS-03/04/06 (B06/B07). Files: `R/ipc/agents.rs`, `R/terminal/shell.rs`, `R/daemon/agent_extension.rs`; `U/lib/types.ts` only if target-shell metadata required by P02 (allocate explicit additive contract before editing).
- RED/GREEN: `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::agents:: -- --nocapture`; `cargo test --manifest-path src-tauri/Cargo.toml --lib terminal::shell:: -- --nocapture`; `cargo test --manifest-path src-tauri/Cargo.toml --lib daemon::agent_extension:: -- --nocapture`.
- Inject PATH/PATHEXT/home, no parallel global env changes: .exe/.com/.cmd/.bat order/case, explicit extension and rejection of data file. Real ConPTY .cmd fixture prints argv/session ID including spaces/metacharacters; native .exe remains direct. USERPROFILE-only installation yields existing three agent extension dirs, no real user writes.
- Runtime: Agents recognizes fixture CLI; resumed shim preserves provider session; known child metadata discovery returns exact ID without ps/lsof. Prefer P08 authoritative state rather than invasive CWD process-memory scraping. Dependencies P08 state protocol; P02 target shell metadata needs agreement.

## P10 - SSH argv/config, Windows path validation and native fixtures

- Category: deep. IDs: FSSH-01/03/04/T01. Files: `R/ssh/{direct,direct_tests,config,bridge_tests,worktree}.rs`, `R/worktree/manager.rs`, `src-tauri/tests/worktree_safety.rs`. All manager and remote worktree validation stays one owner.
- RED/GREEN: `cargo test --manifest-path src-tauri/Cargo.toml --lib ssh::direct:: -- --nocapture`; `cargo test --manifest-path src-tauri/Cargo.toml --lib ssh::config:: -- --nocapture`; `cargo test --manifest-path src-tauri/Cargo.toml --lib ssh::worktree:: -- --nocapture`; `cargo test --manifest-path src-tauri/Cargo.toml --test worktree_safety -- --nocapture`.
- Build owned helper prerequisite later: `cargo build --manifest-path remote-helper/Cargo.toml`; bridge command `cargo test --manifest-path src-tauri/Cargo.toml --lib ssh::bridge:: -- --nocapture`. Must replace fixed local helper path with verified native .exe fixture and inspect loopback-SSH optional tests before execution.
- Cases: actual ProcessStartInfo argv recorder + framed helper handshake with spaced/non-ASCII root; OpenSSH quoted IdentityFile import -> semantic path -> one -i argv; invalid pipe/angle/quote and genuine DOS device path components rejected before Git/filesystem mutation on Windows target; legal nested Unicode accepted.
- Cancel/read test subscribes to exact request-received barrier, deliberately blocks responder then cancels; no startup-output race/timer success. POSIX execution fixture must run on actual POSIX target, Windows counterpart on native PowerShell/Git, not require WSL bash accidentally.
- Runtime: SSH helper spaced profile opens terminal; imported spaced key connects; worktree create handles valid Unicode and rejects invalid names without orphan artifacts. Dependencies P08 remote cwd, P18 packaging/provenance. In-use deletion stays explicit gap, not a kill/retry fix.

## P11 - SSH store data preservation

- Category: deep. ID: FSSH-06. Files: `R/ipc/ssh.rs` and its embedded tests only.
- RED/GREEN: `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::ssh:: -- --nocapture`.
- Seed corrupt JSON, invoke actual import/update mutation core, require error and identical old bytes; missing-file case creates new store. Windows controlled deny-read handle with explicit barrier must also preserve old store. Distinguish resilient per-entry parsing from whole-store failure.
- Runtime: editing host cannot replace inaccessible inventory with one-host default. Fixed-temp concurrent writers remain a separate deterministic race probe unless confirmed; no speculative blanket rename workaround. Independent of P10 source.

## P12 - Browser automation authentication

- Category: deep. ID: BROWSER-IPC-B01. Files: `R/ipc/browser_cli.rs` (request/server/send client and embedded tests); `R/cli.rs` only if sender signature needs change.
- RED/GREEN: `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::browser_cli:: -- --nocapture` on Windows with actual ephemeral listener.
- Unauthenticated list rejected before execute; authenticated list/snapshot/act work, oversized request still bounded; second account cannot read fixture page title/click marker. Store capability under verified user-private ACL or use owner-restricted pipe, not port secrecy.
- Runtime: actual Ferryx CLI drives owned fixture page with credential; independent unauthorized client cannot. Dependencies P13 actual action semantics; coordinate common auth approach with P08 without shared-file edits.

## P13 - Browser OS opener, engine history, keypress and home links

- Category: deep (browser/backend). IDs: BROWSER-IPC-B02/03/05/10. Files: `R/ipc/browser.rs`, `R/browser/{manager,tests}.rs`, `U/lib/browserTauri.test.ts` if IPC integration needs updates; no permission or browser CLI files.
- RED/GREEN: `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::browser:: -- --nocapture`; `cargo test --manifest-path src-tauri/Cargo.toml --lib browser:: -- --nocapture`; `bun run --cwd ui test src/lib/browserTauri.test.ts`.
- OS seam asserts shell-free exact URL/query/path; actual Windows default opener preserves `?a=1&b=2`, benign second-command marker never executes. HOME absent/USERPROFILE fixture resolves ~/file and bare ~.
- Real WebView2 fixture pushState twice without title/load events, subscribe completion, toolbar Back/Forward traverses correct engine entries; failed/no-op navigation settles loading. CLI Backspace at abc caret yields ab and Tab moves actual focus, not just synthetic event counters; unsupported input must return typed error.
- Runtime above must use actual browser child, not jsdom; P12 auth enables owned CLI route, P05 targets focused leaf. Native harness additions stay embedded in assigned existing modules unless new fixture files explicitly allocated.

## P14/P15/P16 - Independent OS/state boundaries

- P14 category deep, BROWSER-IPC-B04: files `R/notification/{notify_rust_adapter,tests}.rs`, `R/ipc/notifications.rs`. RED/GREEN `cargo test --manifest-path src-tauri/Cargo.toml --lib notification -- --nocapture`. Actual adapter XML System not silent, Silent silent, custom sound once; real toast audible/inaudible with OS sound enabled and click routing intact. Packaged AUMID issue remains probe, not assumed fix.
- P15 category deep, BROWSER-IPC-B09: files `R/remote/state.rs` and embedded tests. RED/GREEN `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::state:: -- --nocapture`. Inject adapter inventory/on-link IPv4 with both route probes failing; resolve correct LAN, preserve Tailscale filtering. Real owned offline LAN peer reaches authenticated gateway; do not change user network/firewall.
- P16 category deep, PKG-01: files `U/lib/{updater,updater.test,windowsStoreMigration,windowsStoreMigration.test}.ts`. RED/GREEN `bun run --cwd ui test src/lib/updater.test.ts src/lib/windowsStoreMigration.test.ts`. Mock only strict Rust command registry, reject unknown identifiers; Store never calls plugin updater, installer notice and NSIS update route work. Real fresh Tauri invoke names verified; actual MSIX/NSIS install QA requires later separate authorization, not replacing installed user app.

## P17/P18/P19 - Packaging and verification prerequisites

- P17 category deep, PKG-03: files `scripts/build-msix.ps1`, `scripts/test-build-msix.ps1`. RED/GREEN `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-build-msix.ps1` on owned native Windows packaging host. Extend existing real MakeAppx archive test to require ui/dist/index.html, helper manifest and target binaries, validate bytes/hashes and fail if absent. Clean package launched outside repo serves authenticated remote UI and resolves helper assets. No signing/installation/release permission implied.
- P18 category deep, COV-TEST-1/COV-CI-1/COV-BLD-1: files `src-tauri/build.rs`, `src-tauri/tests/windows_edge_probe_contract.rs`, restored tracked fixture under `scripts/fixtures/`; `.github/workflows/build-test.yml`, `scripts/release-workflow.test.mjs`. Source recovered locally at `.omo/ulw-loop/01a04fcf-f90f-7878-bd5d-3881f49c4297/evidence/windows-edges/{run-edge-probes.ps1,probe-daemon-edges.mjs}`; both are ignored/untracked, so reproduce missing dependency in a clean authorized checkout, not by removing the local files. Historical fixed-root cleanup, inherited runtime overrides, polling and protocol-2 driver are unsafe/stale; do not execute or blindly copy them. Preserve the test and replace its ephemeral dependency with meaningful portable owned-fixture behavior.
- P18 RED/GREEN: `cargo test --manifest-path src-tauri/Cargo.toml --test windows_edge_probe_contract --no-run`; `node --test scripts/release-workflow.test.mjs`. Conditional GNU link probe ONLY in an already provisioned, separately authorized environment: `cargo test --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-gnu --test windows_window_opacity_contract --no-run`. This is not a GNU support commitment or authorization to install toolchains/add release or CI targets. Otherwise record GNU runtime verification unavailable, not an executed flag RED. Native MSVC existing target must still link and load without TaskDialogIndirect error.
- P18 CI policy test parses actual workflow matrix and requires nonzero Windows tests; enable isolated platform-relevant tests after P06/P08/P10/P19 repairs. Existing Linux-only persistence target on Windows runs zero tests, never a substitute. Archive/runtime evidence still separate from CI source test.
- P19 category deep, DS-10: exclusive file `src-tauri/tests/daemon_persistence_contract.rs`. Before executing target, implement per-test TempDir child env/endpoints, handshake PID==child ID, owned cleanup, exact event barriers. Historical unisolated harness must NOT be executed for RED; validate ownership failure via injected owned fake endpoint/child seam first.
- P19 same final RED/GREEN target once structurally isolated: `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_persistence_contract -- --test-threads=1` on authorized isolated Unix checkout. Windows counterpart remains missing coverage; don't remove cfg gate and expect UnixStream to work. Runtime must prove concurrent owned harnesses cannot touch each other or live user daemon, including panic cleanup. Dependencies P08 protocol adaptation.

## P21/PX - Small UI warning and foreign rescan owner

- P21 category deep (frontend conditional), BROWSER-IPC-B12: `U/components/settings/PermissionsSection.tsx`, its `.test.tsx`. RED/GREEN `bun run --cwd ui test src/components/settings/PermissionsSection.test.tsx`; assert platform/capability alert presence, never warning wording. Real Windows Permissions page has no macOS-only grant action/advice; no new prose-only test.
- PX category deep, FSSH-X01/X02: foreign `R/worktree/rescan.rs` and only its owner's adjacent tests. RED/GREEN `cargo test --manifest-path src-tauri/Cargo.toml --lib worktree::rescan:: -- --nocapture` after confirming module test discovery. Virtual time constant fingerprint scans at 300s despite 30s sweeps; Windows `refs/..\..\outside` never reads outside sentinel. Owner applies/reviews; synthesis grants no foreign edit permission.

## Scheduling and completion gate

- P22 category deep, COV-QA-01/02/03: exclusive existing file `script/qa/win-daemon-e2e.mjs`, including its existing self-test seam. Extend --self-test to await owned ephemeral TCP peer scenarios; keep parsing cases. Exact future RED/GREEN command: `bun script/qa/win-daemon-e2e.mjs --self-test`. Current command lacks transport assertions; register those before helper fixes and require nonzero scenario receipts. No new guessed test filename or real daemon connection is needed.
- P22 request regression: peer signals receipt of A, inject controlled timeout callback (time behavior under test), require A rejection and connection poisoning/settlement before B can be misassigned; send late A response and prove it cannot resolve B. Separate peer emits unsolicited output while request awaits ack: listener receives output, request resolves only on correct response. No request IDs exist to safely continue ambiguous FIFO traffic; removing expired entry alone is insufficient. Isolate listener exceptions from protocol response dispatch.
- P22 lifetime regressions: subscribe peer close and promise settlement before bad handshake/attach reply, explicit close and EOF; assert acquired socket closed and every request/PTY-signal waiter rejects exactly once with no residual timer. Real socket and parser required, not a fake request method. Controlled scheduler handles deadlines; exact peer request/close events coordinate actions, bounded timeout only fails the test.
- P22 protocol regression: fake peer enforces actual current protocol version 3 and rejects stale 2; check the machine-consumed Rust protocol constant against helper handshake value so future drift fails the same command. Both control and attach handshakes covered. Do not negotiate away mismatch or connect to installed stale daemon to make QA pass.
- P22 future actual Windows invocation form already exists: `bun script/qa/win-daemon-e2e.mjs --port <owned-port> <owned-repo-root>`; angle-bracket arguments are required runtime-owned values, not a runnable command here. Capture readiness/PID ownership before supplying them, use unique workspace/request IDs and owned checkout/profile, then verify marker/CWD/sequence and close only created session. Do not run default port-file path: it targets the user's daemon. Coordinate P08 auth/protocol before actual runtime acceptance; self-test uses no desktop/daemon and may run independently after implementation authorization.
- Safe independent wave after ownership approval: P01, P03, P04, P07, P11, P12, P14, P15, P16, P17, P18 and P21. P06/P05/P08/PX depend on active foreign owner coordination; P19 is prerequisite for persistence execution, not excuse to run unsafe RED.
- P02 consumes P01/P03/P09 contracts; P05 requires safe process receipts. P04 changes only duplicate digit bindings and is not a prerequisite to redefine close/paste/link policies. P08/P09 coordinate state protocol; P10 consumes P08 remote semantics; P13 consumes P12 authenticated automation. Keep one writer for each listed file even where multiple IDs share it.
- P20 intentionally unused: worktree validation combined into P10 to keep `ssh/worktree.rs` and manager caller validation in one disjoint scope.
- Before final parent gate, reconcile any required extra struct-literal/caller files into this ownership map, freeze exact tree, rerun all affected checks once reliably, complete coverage.md G items or obtain explicit exclusion, and attach PR #2/#3 dispositions plus real wheel/sibling-survival receipts. No source repair, tests, runtime QA, commit, merge or push is claimed here.
