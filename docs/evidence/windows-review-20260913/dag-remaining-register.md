# DAG remaining-work register

Date: 2026-09-13. Scope: reports only. Status: INCOMPLETE. Active objective: G001.
This register lists every confirmed defect packet, its current status, exact remaining proof, coverage inventory gaps, and foreign diff boundaries. Loop ledger remains the audit trail.

## Active workers and recent receipts

P15 worker st_01a099e2 completed production adapter repair on `src-tauri/src/remote/state.rs` and `src-tauri/Cargo.toml`. Runner GREEN verified primary test (1 pass, exit 0) and companion test (1 pass, exit 0); full receipts in `p15-red/` and `p15-green/`. Native four adapter tests and real offline LAN reachability remain pending; runtime owner is notified.
Supplemental P15 independent review st_01a099fb runs concurrently targeting `docs/evidence/windows-review-20260913/dag-p15-review.md`. Active DAG amend was refused with details.kind=error (despite hasError=false); the original six-node graph remains unchanged. This review is tracked as pending for the combined verifier.
P14 staged an executable evidence-only native harness under st_01a099e3 (`p14-native/{submit.js,observe.ps1}`). JS syntax check passed; PowerShell and native execution remain unrun. Production sound omission remains unchanged and runtime owner is notified; repair is not complete.
Recent local batches completed: P03 legacy punctuation encoding passed local RED/GREEN in bash_57/59 (exit 0, 7 pass; p03-key-encoding.md). P11 SSH store preservation passed local RED/GREEN in bash_56/58 (exit 0, 13 pass; p11-store-preservation.md).
The full frontend test run (mon_6TQT1KKGVB9PQ546 / bash_60) finished 219 files in 114.46s: 212 passed, 7 failed (full-ui-post-selector.md). Three exitAttach assertions were corrected in bash_62 (exit 0, 55 pass). App remote tests passed in bash_61 (exit 0, 23 pass; app-remote-suite-repair.md). SshSection Added icon theme was repaired in ssh-added-theme.md (exit 0, 33 pass, 4/4 render cases).

## Confirmed packet status and remaining proof

| Packet | Owner | Confirmed IDs | Current verified status | Exact remaining proof (artifact refs) |
|---|---|---|---|---|
| P01 | P01 | native-input-01 | Open. No production repair or native Windows test run yet. | Native Windows `cargo test --lib presented_terminal_yields_cross_thread_pointer_hit_testing_to_input` and WebView2 sentinel drag selection (`lead-baseline.md`, `audit-native-input.md`). |
| P02 | P02 | native-input-02/03/04/06/07/08/10, BROWSER-IPC-B11, CONTRACT-RC-02 (input), NATIVEUI-GAP-01/02/04/05 (pane), TRANSPORT-TEST-01 | Partial local GREEN (wheel 167 pass, exitAttach 55 pass, transport 8 pass). | Native Windows HWND delivery, backend cell/modifiers, portable debug sink in all 4 switch-log sites, SendInput +120/-120 over 200 lines, drop quoting (`wheel-close-lead-verification.md`, `full-ui-post-selector.md`, `red-green.md`). |
| P03 | P03 | native-input-05, CONTRACT-RC-03 | Local RED/GREEN complete (bash_57 exit 101, bash_59 exit 0, 7 pass). | Native Windows physical key capture for Ctrl+\ and Ctrl+], CONTRACT-RC-03 mouse encoding oracle test on Windows (`p03-key-encoding.md`, `continuation-20260913-1705.md`). |
| P04 | P04 | WIN-UI-03 (NATIVEUI-GAP-03) | Local RED/GREEN and lead mutation complete (156 pass, exit 0). | Native Windows interactive Ctrl+digits vs Alt+digits validation (`combined-local-verification.md`). |
| P05 | P05 | WIN-UI-02, WIN-UI-13 | Local RED/GREEN complete (App close 128 pass, store 58 pass, browser 24 files 722 pass). | Native Windows PTY sibling survival on pane close (Ctrl+W and native menu with confirmation), live WebView2 address/find/history isolation (`wheel-close-lead-verification.md`, `remote-browser-combined-verification.md`). |
| P06 | P06 | RF-01/02/03/04/06/08, GT-05/07/08, CONTRACT-RC-02 (Wayland), RF-05 optional | Open. No native GPU runs yet. | Native Windows GPU DirectWrite/GDI raster tests (`GdiFlush`, fallback stack, metrics, font identity cache clear, DPI pixel scaling), GT-05/07/08 renderer contract tests (`audit-renderer-fonts.md`, `gap-tooling.md`). |
| P07 | P07 | RF-07 | Local RED/GREEN and lead mutation complete (210 pass, exit 0). | Native Windows GUI Sonner toast rendering yielding child HWND, dismissal restoring terminal session/viewport (`combined-local-verification.md`). |
| P08 | P08 | DS-01, DS-02, DS-05, DS-07, DS-09, FSSH-02 | Open. No implementation or test run yet. | Authenticated loopback control/attach, Windows SID/DACL endpoint check, non-recursive upgrade, extension state listener before advertisement, remote cwd relative conversion, ConPTY test fixture (`audit-daemon-shell.md`, `gap-backend.md`). |
| P09 | P09 | DS-03, DS-04, DS-06, HIST-01 | Open. No implementation or test run yet. | PATHEXT resolution for .cmd/.bat shims without CreateProcessW failure, USERPROFILE agent extension discovery, authoritative provider metadata, HIST-01 TerminalSection platform gate (`audit-daemon-shell.md`, `gap-history.md`). |
| P10 | P10 | FSSH-01, FSSH-03, FSSH-04, FSSH-T01, CONTRACT-RC-01, CONTRACT-RC-05 | Open. No implementation or test run yet. | PowerShell StartInfo argv quoting for spaces, OpenSSH IdentityFile quoted path parsing, Windows reserved path/device validation in worktree manager, native test fixtures replacing /keys and /bin/sh (`audit-filesystem-ssh.md`, `remaining-contracts.md`). |
| P11 | P11 | FSSH-06 | Local RED/GREEN complete (bash_56 exit 101, bash_58 exit 0, 13 pass). | Native Windows deny-read handle store preservation test (`p11-store-preservation.md`, `continuation-20260913-1705.md`). |
| P12 | P12 | BROWSER-IPC-B01 | Open. No implementation or test run yet. | Authenticated loopback browser CLI listener, user-private ACL/pipe, rejection of unauthenticated requests on Windows (`audit-browser-ipc.md`). |
| P13 | P13 | BROWSER-IPC-B02, BROWSER-IPC-B03, BROWSER-IPC-B05, BROWSER-IPC-B10 | Open. No implementation or test run yet. | Shell-free URL opening without cmd.exe start boundary, WebView2 HistoryChanged synchronization, trusted CLI keypress/Backspace/Tab, profile/USERPROFILE home resolution (`audit-browser-ipc.md`). |
| P14 | P14 | BROWSER-IPC-B04 | Evidence-only harness staged (`p14-native/`), JS syntax passed; native run pending, production sound unchanged. | Native Windows XML `<audio>` generation and audible/silent playback verification for System and Silent variants (`p14-notification-sound.md`, `p14-native/`). |
| P15 | P15 | BROWSER-IPC-B09 | Local RED/GREEN complete (primary exit 0, companion exit 0; p15-green/green.done.json). | Native four Windows adapter tests and real authenticated offline-LAN peer reachability (`p15-offline-lan.md`, `p15-green/`). |
| P16 | P16 | PKG-01 | Local RED/GREEN and lead mutation complete (210 pass, exit 0). | Native Windows Store bypass vs installer update probe behavior (`combined-local-verification.md`). |
| P17 | P17 | PKG-03 | Open. No packaging run yet. | `test-build-msix.ps1` requiring ui/dist/index.html, helper manifest and binaries on native Windows packaging host (`audit-packaging.md`). |
| P18 | P18 | COV-TEST-1 (PKG-02), COV-CI-1 (PKG-08), COV-BLD-1 (PKG-10), HIST-02 | Open. No test run yet. | Tracked safe fixture for `windows_edge_probe_contract.rs`, GitHub Actions workflow isolated Windows test step, `build.rs` MSVC manifest gate (`target_env = "msvc"`), `check-tree-quiescent.sh` portable metadata (`audit-packaging.md`, `gap-tooling.md`). |
| P19 | P19 | DS-10 | Open. No test run yet. | Structurally isolated harness with per-test temp endpoints, handshake PID validation, panic cleanup before execution on Unix; Windows counterpart coverage (`audit-daemon-shell.md`). |
| P20 | Unused | None | Merged into P10 to keep worktree validation in one owner. | None. Reserved identifier (`repair-packets.md`). |
| P21 | P21 | BROWSER-IPC-B12 | Local RED/GREEN and lead mutation complete (210 pass, exit 0). | Native Windows settings display confirming unsupported-capability alert absence (`combined-local-verification.md`). |
| P22 | P22 | COV-QA-01, COV-QA-02, COV-QA-03 | Local TCP transport self-test RED/GREEN and cleanup repair complete (16 pass, exit 0). | Real Windows daemon ConPTY session execution via `bun script/qa/win-daemon-e2e.mjs --port <owned-port> <owned-repo-root>`, verified PID ownership and cleanup (`p22-local-repair.md`). |
| P23 | P23 | GB-01, GB-03, GB-04 | Open. No implementation or test run yet. | `relay_server.rs` enrollment transaction, lease assertions, admission barriers (`gap-backend.md`, `gap-verification.md`). |
| P24 | P24 | GB-02, GB-05 | Open. No implementation or test run yet. | `src-tauri/src/dag/watcher.rs` watch recovery state and offloaded scan (`gap-backend.md`, `gap-verification.md`). |
| P25 | P25 | GT-01, GT-02, GT-03, GT-04 | Open. No test run yet. | Import safety for `scripts/qa/ssh-*.mjs`, wrapper contracts in PowerShell, protocol 1 vs 3 preservation (`gap-tooling.md`, `remaining-powershell-wrappers.md`). |
| P26 | P26 | HIST-03, GT-06 | Open. No test run yet. | Machine-consumed capability/result contract in `permissions/mod.rs` and `permissions_contract.rs` (`gap-history.md`, `gap-verification.md`). |
| P27 | P27 | GAP-UI-06 (WIN-UI-09), REMOTE-RC-04 | Local wheel (58 pass) and host preferences (201 pass) complete. | Real Windows browser runtime wheel event delivery and host-scoped preference loading over live socket (`p27-wheel-normalization.md`, `preferences-menu-combined-verification.md`). |
| P28 | P28 | REMOTE-RC-01/02/03, BOUNDED-UI-02, OTHERUI-GAP-01 | Local selection lifetime (50 pass), target worktree (130 pass), Opera UA (156 pass), pairing determinism complete. | Real Windows browser runtime with live relay response gate and cross-worktree selection (`p28-selection-lifetime.md`, `p28-target-worktree.md`, `combined-local-verification.md`). |
| P29 | P29 | TOOLING-GAP-03, TOOLING-GAP-04 | Open. No test run yet. | Windows signing fixture paths in `scripts/macos-dev-runner.test.mjs`, SEO route key slash handling on Windows in `site/src/seo.test.ts` (`closure-tooling.md`). |
| P30 | P30 | IMAGES-GAP-01 | Open. No test run yet. | `native_terminal_images.rs` 4097x4097 RGB expansion limit test preserving text snapshots, verified on native Windows debug terminal (`closure-images.md`). |
| P31 | P31 | BOUNDED-UI-01 | Local RED/GREEN and lead sortable-mutation complete (222 pass, exit 0). | Native Windows debug GUI grouped row displacement, drag and reload persistence (`combined-local-verification.md`). |
| P32 | P32 | BI-01 | Local sandbox run with Rust 1.98.1 (5 tests, exit 101; ancestry and paging passed, symlink test failed). | Native Windows test executable execution for `scoped_history` active ancestry (`p32-history-ancestry.md`, `p32-adjacent-verification.md`, `continuation-20260913-1705.md`). |
| P33 | P33 | SHARED-NATIVE-01 | Local RED/GREEN and lead caller verification complete (120 pass, exit 0). | Native Windows debug GUI menu dismissal, same-ID action invocation and clipboard copy proof (`preferences-menu-combined-verification.md`). |
| P34 | P34 | SHARED-NATIVE-02, NATIVEUI-GAP-05 (focus/coordinator) | Local RED/GREEN and stale-snapshot mutation complete (50 pass, exit 0). | Native Windows startup foreground transition, terminal bell and unread decision verification (`red-green.md`). |
| PX | PX | FSSH-X01, FSSH-X02 | Foreign owner only. No fix applied. | Virtual time forced scan at 300s, path traversal rejection for `refs/..\..\outside` in `worktree/rescan.rs` (`repair-packets.md`, `findings.md`). |

Excluded items: CONTRACT-RC-04 (macOS-only observation, out of Windows scope); TOOLING-GAP-01/02 (legacy Homebrew-bound mock, not Windows acceptance harness).
Downgraded or refuted claims: WIN-UI-01 and native-input-09 (policy constraints); WIN-UI-05/07/10/11/12 (refuted mechanisms/styling); RF-05 (optional color emoji); DS-08 and FSSH-05 (feature gaps); PKG-05/06/07/09/11 and COV-TOOL-1 (refuted or missing package probes); COV-BLD-1 (conditional GNU link probe, GNU toolchain unprovisioned).

## Coverage inventory gaps needing final qualification

The lexical census (`inventory-reconciled.json`, 1,854 paths, 362 candidates) and parser ledger (`platform-branch-ledger.json`, 23,316 range/kind entries across 968 files) verified structural counts.
Specific gaps remain to qualify:
1. Reconcile the 86 explicit parser/format gaps and global alias/Cargo/macro qualifications from `platform-branch-ledger.md`.
2. Qualify semantic caller and ancestor coverage of shared noncandidate files outside the 59 closure, 25 bounded, 7 shared-native, and 5 transport receipts.
3. Validate final changed-source diffs against the implementation base once active repair workers return.

## Foreign diff boundaries and active ownership

Active child worker st_01a099e2 exclusively owns `src-tauri/src/remote/state.rs` and `src-tauri/Cargo.toml`. Other sessions must not touch those files.
Child worker st_01a099e3 exclusively owns `docs/evidence/windows-review-20260913/p14-native/` and notification evidence.
Preserve all foreign uncommitted modifications across 30+ working-tree files in `ui/` and `src-tauri/`.
Worktree and branch creation approval remains requested and unanswered. Main branch must not switch.
Desktop testing is restricted to `bun tauri dev` debug mode only. Running user daemons and installed application identities must remain preserved.

## Unmet requirements and aggregate delivery gate

PR resolutions remain open: PR #2 (WS_DISABLED drag hit test) and PR #3 (optional-kind pane close) remain OPEN on GitHub with unchanged heads (`pr-review.md`).
Unresolved frontend test failures: `SettingsDialog.test.tsx:614` Remote Access prose mismatch; `features/ferryx/push/client.test.ts` (3 failures in unconnected push client stub).
Native Windows runtime verification on `maho-win` is unexecuted for all packets (C001, C002, C003).
Delivery gate: atomic commits, approved final gate, and remote main verification remain blocked until all repairs and Windows runtime verifications finish. Goal G001 remains incomplete.
