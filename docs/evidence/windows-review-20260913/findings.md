# Windows defect ledger - 2026-09-13

Status: initial synthesis plus verified follow-up source findings; implementation and runtime acceptance INCOMPLETE. No fixes or RED/GREEN runs performed.
Baseline HEAD b7ad4516; current foreign-dirty source also inspected. Line numbers are observed working-tree lines, not immutable commit coordinates.
All eight audit reports and both open PR diffs were read. The ledger below supersedes unsupported severity/verification claims in those reports.
Paths: `R/` = `src-tauri/src/`; `N/` = `R/native_terminal/`; `U/` = `ui/src/`. Packet IDs refer to repair-packets.md and supply executable test commands plus runtime scenarios.
Confirmed means reachable source/API-contract defect, not an observed Windows failure. Conditional manifestations are identified. Proposals are not recorded as RED results.

## High-priority confirmed defects

## Follow-up register and authoritative detail receipts

The original tables below retain their initial source coordinates. The
following qualified IDs extend them; exact mechanisms, proposed commands,
binary conditions and limits are in the linked reports. Every entry is
source-reviewed only, with RED/GREEN and runtime evidence still absent.
`gap-packet-addendum.md` extends the original ownership map. Neither a
report hash nor a packet assignment closes a defect.

- GB-01, GB-03, GB-04: relay enrollment transaction and integration
  validators; `gap-backend.md`, adjudicated by `gap-verification.md`; P23.
- GB-02, GB-05: DAG watch recovery and blocking scan; same receipts; P24.
- GT-01 through GT-04: helper/survival/staging harness boundaries;
  `gap-tooling.md`, `gap-verification.md`; P25.
  `remaining-powershell-wrappers.md` extends archive/cleanup ownership,
  artifact lookup and zero-test acceptance in the same packet.
- GT-05, GT-07, GT-08: renderer example lookup, false-green prerequisite
  and lifecycle assertion boundaries; same tooling/verifier receipts; P06.
- HIST-01: platform applicability of Option-as-Alt UI; `gap-history.md`;
  P09. HIST-02: diagnostic metadata portability; same receipt; P18.
  HIST-03 and GT-06: permission capability and contract; P26.
- GAP-UI-06 is the remote wheel unit defect, alias WIN-UI-09 and historical
  L5-UI-FRONTEND-8, not three defects; `gap-ui.md`; P27.
- REMOTE-RC-01/02/03 (local RC-01/02/03 in
  `remaining-remote-callers.md`): selection POST hangs before its deadline,
  obsolete completion mutates the next request, and swipe/waiting targets
  use the current rather than published target worktree; P28.
- REMOTE-RC-04 (local RC-04 in that report): scoped remote credentials are
  absent from the legacy preference caller/cache; P27. Desktop-embedded
  appearance policy remains a separate decision, not an assumed defect.
- CONTRACT-RC-01/05: owned missing-helper fixture and structured error
  oracle; `remaining-contracts.md`; P10. CONTRACT-RC-02: ambient preference
  import in input/Wayland fixtures; P02/P06. CONTRACT-RC-03: weak exact
  mouse-byte oracle; P03. These are distinct from the remote RC identifiers.
- NATIVEUI-GAP-01/02/04/05: callback arity, host-dependent Mac fixtures,
  absent in-flight-detach transition and completion synchronization;
  `closure-nativeUi.md`; P02. NATIVEUI-GAP-03 aliases WIN-UI-03's
  effective-platform chord validator, P04; no extra product defect.
- OTHERUI-GAP-01: pairing completion synchronization;
  `closure-otherUi.md`; P28. No pairing product failure established.
- TOOLING-GAP-03/04: Windows signing-test fixture paths and filesystem-to-URL
  SEO route keys; `closure-tooling.md`; P29.
- IMAGES-GAP-01: accepted 4097x4097 RGB input expands beyond native RGBA
  snapshot capacity and aborts the whole text frame; `closure-images.md`;
  P30. Pinned parser/storage acceptance and the fallible caller chain were
  directly reopened by the lead. No runtime reproduction has run.
- BOUNDED-UI-01: workspace-qualified SortableContext IDs differ from row
  registrations, and path-only persisted ordering collapses distinct member
  rows; `bounded-ui.md`; P31. Lead reopened grouping, both ID constructors,
  drag data, persistence and the installed DnD index contract.
- BOUNDED-UI-02: Chromium Opera matches Chrome before OPR detection in
  submitted pairing device-name metadata; `bounded-ui.md`; P28.
- BI-01: scoped_history's included prototype returns all Claude siblings
  while its active-branch test requires only a,c; `bounded-infrastructure.md`;
  P32. Lead read the test include, full parser and assertion. This is a
  Windows-reachable test-target defect, not a proven shipped history failure.

- SHARED-NATIVE-01: app-wide menu IDs reach stale per-row listeners during
  dismissal overlap; `shared-native-callers.md`; P33. Lead reopened actual
  row callbacks, helper and backend forwarder. Windows TrackPopupMenu
  blocks until dismissal; an open-popup 200ms timeout is refuted.
- SHARED-NATIVE-02: startup snapshot precedes native focus subscription,
  leaving stale cached focus for real unread/notification consumers;
  `shared-native-callers.md`; P34. Lead reopened helper, installed listener
  API, App wiring and coordinator decision. Runtime manifestation remains
  unverified. Its test synchronization issue extends NATIVEUI-GAP-05.

- TRANSPORT-TEST-01: exact session-list test expectation omits the real
  mapper's `running: true` default; `transport-source-closure.md`.
  This validator mismatch was reproduced on macOS (1 failed, 7 passed,
  exit 1); `red-green.md`. Not a shipped Windows terminal defect.
  Required under C3 frontend checks;
  C002 registered the exact invocation, default/false mutation conditions
  and test-only ownership at 2026-09-13T03:30:46.402Z. Test-only correction
  now passes all 8 cases on macOS in one run (exit 0); both actual loader
  mutations fail only the expected case. Native Windows execution remains
  pending. Not a fully closed finding.

CONTRACT-RC-04 is a macOS-only validator observation. TOOLING-GAP-01/02
are defects in the old Homebrew-bound image mock, not a Windows acceptance
harness used in this task. These observations remain in their reports but
are excluded from Windows repair packets, not silently marked fixed.
Unsupported capabilities and rejected historical/UI claims retain the
dispositions below and in `gap-verification.md`.

The historical 146-ID map is complete in `gap-history.md`; its C42 count
does not mean 42 unique repairs. Follow-up counts must deduplicate the
aliases above. Final changed-source qualification and per-fix criterion
registration remain outstanding before implementation.

## Initial high-priority source findings

| ID (upstream aliases) | Independently reopened mechanism and reachable chain | Smallest correction / packet |
|---|---|---|
| native-input-01 | `N/platform/windows.rs:243-250,311-333`: constructor creates enabled draw-only HWND; first present raises it above WebView2; HTTRANSPARENT is not cross-thread input transparency. | PR #2 WS_DISABLED + constructor test seam; P01. Wheel success still unproved. |
| native-input-02 | `U/components/NativeTerminalPane.tsx:2219-2229` -> `R/ipc/native_terminal.rs:922-939` -> `N/wheel.rs:28-97`: actual DOM wheel drops modifiers/position; IPC invents pane center/default modifiers. Shift override and hovered TUI coordinates cannot work through this adapter. | Preserve wheel context with authoritative geometry; P02. |
| native-input-03 | `N/platform/windows_focus.rs:152-175` -> `N/surface_host.rs:877-889` -> pane `:1594-1617`: global mouse-up tests only pane rectangles, emits terminal focus for unrelated occluding HWND clicks, repeatedly focuses sink. | Ownership/foreground filtering or retire workaround after real pointer proof; P02. Modal manifestation also depends on visibility yielding. |
| native-input-04 (WIN-UI-06 functional portion) | Pane `:2190-2216,1197-1216` sends only left drag; `R/ipc/native_terminal.rs:1276-1282` always selects for left/null motion/release even with tracking. Plain TUI clicks, right/middle and hover are unreachable. | Respect tracking versus Shift-selection, held buttons and explicit link gestures; P02. Cursor styling alone is not a defect. |
| native-input-05 | `N/key_encoder.rs:29-39,148-160` maps punctuation Unidentified and omits UTF-8 under Ctrl; pane `:2369-2383` sends utf8:null -> input -> Ghostty `src/input/key_encode.zig:705-731` requires UTF-8 or logical codepoint. Legacy Ctrl-backslash/`]` emits nothing. | Correct logical punctuation/UTF-8 construction; P03. |

| WIN-UI-02 | `U/lib/types.ts:138` optional kind; `U/state/workspaceStore.ts:576-581` omits kind; current `U/App.tsx:1806` still requires terminal tag and falls to whole-tab close. Foreign close-confirmation work has changed the PR's original seam. | Adapt PR #3 discriminator forward onto owner's confirmation flow; P05, owner coordination required. |
| DS-01 + DS-07 | `R/daemon/server.rs:1466-1478,1508-1569` -> direct dispatch accepts unauthenticated loopback control. `:403-430,488-532` checks type/symlink but no Windows SID/DACL; client trusts production endpoint. A second local account can enumerate ports; permissive endpoint planting is an additional conditional risk. | One daemon security packet: authenticate every control/attach/proxy path and protect/validate capability publication; P08. Default profile ACL weakness is NOT asserted. |
| DS-02 | `R/daemon/client.rs:352-359,438-457,620-635`: stale handshake spawns a new temporary client with fresh upgrade flag; its handshake recursively schedules another. Windows server `:2291-2296` always Unsupported, so stale identity persists. | Disable auto-upgrade for internal upgrade RPC or share guard; P08. |
| DS-03 | `R/terminal/shell.rs:410-434` -> daemon spawn -> portable-pty 0.9.0 `win/psuedocon.rs:132-153`: resolved .cmd/.bat shim passed directly to CreateProcessW, no command interpreter. PATHEXT lookup already exists in dependency. | Interpreter-aware Windows resume planning, exact argv quoting; P09. |
| FSSH-01 | `R/ssh/direct.rs:249-263` -> bridge_plan -> BridgeConnection::spawn -> helper bridge: raw StartInfo.Arguments concatenates decoded root; profile spaces split --root argument. | Quote child argv for PowerShell 5.1 ProcessStartInfo boundary; P10. |
| FSSH-02 | `R/ssh/worktree.rs:169-204` accepts Windows case/separator aliases; daemon remote spawn `R/daemon/server.rs:1230-1245` uses case-sensitive strip_prefix, defaults failure to empty -> helper worktree=None -> repository root. | Remote-platform-aware relative conversion, reject failure instead of silently changing checkout; P08. |
| FSSH-06 | `R/ipc/ssh.rs:179-183` converts read/parse errors to empty store; update/import `:231-274` saves replacement. Corrupt inventory plus an edit can erase saved hosts. | Fallible load; only NotFound means new store; preserve bytes on read/parse failure; P11. |
| BROWSER-IPC-B01 | `R/lib.rs:1033-1036` starts CLI listener -> `R/ipc/browser_cli.rs:194-240,299-365` accepts any loopback peer and executes list/snapshot/act. Remote gateway Off does not gate it. | Authenticate server and sender with user-private capability or restricted pipe; P12. |
| BROWSER-IPC-B02 | Browser Open External -> `R/ipc/browser.rs:1673-1693` -> `cmd /C start URL`; scheme validation permits query ampersands and cmd syntax. URL crosses command-language boundary, spawn errors discarded. File opener `:1743-1746` shares boundary. | Shell-free URI/file opening and error propagation; P13. Exact Windows serialization/exploit effect remains a runtime probe, not claimed execution. |
| RF-01 | Host render -> atlas miss -> FontManager -> `N/renderer/directwrite_raster.rs:165-178`: TextOutW immediately followed by DIB bits read without GdiFlush. Violates DIB synchronization contract; first-glyph corruption is conditional on batching. | Flush drawing before CPU read and handle GDI failure; P06. |
| RF-02 | Preferences -> FontManager `:56-72,269-281` -> `directwrite_raster.rs:115,144-159`: entire CSS-style family list passed as one CreateFontW face. | Resolve stack to installed face, including quotes/generic fallback; P06. No guaranteed proportional-font claim. |
| RF-03 | Bounds -> `N/renderer/font_manager.rs:119-136`: synthetic size ratios ignore selected face; GDI selects real font independently. Layout, atlas clipping and PTY grid use guessed advance/height. | Shared resolved font metrics/baseline; P06. Actual clipping depends on chosen face. |
| RF-04 | Apply preferences -> host config -> `N/renderer/renderer.rs:710-716`: clear only when RendererConfig changes; `types.rs:194-203` has no font identity; atlas key omits family. Equal-metric family changes retain old glyphs. | Font identity/revision participates in cache invalidation; P06. Preserve foreign image additions. |
| PKG-01 | `U/lib/windowsStoreMigration.ts:35` and `updater.ts:59` invoke names without cmd_; `R/ipc/updater.rs:22,42` and `R/lib.rs:1069-1070` register cmd_ names. Store bypass guard fails open; installer notice is silently absent. | Correct two identifiers and strict dispatch-contract tests; P16. Updater plugin error IS caught into status:error (`updater.ts:103-106`), not unhandled. |
| PKG-03 | Local release -> `scripts/build-msix.ps1:268-341` stages only exe/icons/manifest; no declared ui/dist or helpers resources. `R/remote/server.rs:1957-2008` and `R/ssh/helper_assets.rs:107-122` require filesystem assets on clean installed host. | Stage resources and verify actual archive entries; P17. Embedded desktop HTML alone does not supply remote gateway/helper files. |
| COV-TEST-1 (PKG-02) | `src-tauri/tests/windows_edge_probe_contract.rs:1-3` include_str references an ignored, untracked .omo run artifact. Lead recovered both wrapper and driver locally; the earlier current-file-absence claim was false. `git ls-files --stage` returns no entry and `git check-ignore -v` names `.gitignore:20:.omo/`; a fresh tracked checkout cannot supply the dependency. | Restore an owned tracked, safe behavioral fixture; P18. Do not delete the test or blindly copy the historical launcher. No compiler run claimed. |
| DS-10 | `src-tauri/tests/daemon_persistence_contract.rs:34-43,93-115` uses shared endpoint/env and sends Shutdown/unlinks paths without per-harness root/PID equality. Unix-only suite, but dangerous for this macOS repair host. | Isolate every child/client/cleanup before any execution of target; P19. Serial execution is insufficient. |

## Medium/low confirmed defects

| ID (aliases) | Independently reopened mechanism / reachability | Smallest correction / packet |
|---|---|---|
| native-input-06 | Pane `:1197-1245`: move queued on rAF, pointer-up cancels it and sends Release only; `N/selection.rs:605-646` release yields no extended selection. | Ordered final motion before release; controlled-rAF regression; P02. |
| native-input-07 (WIN-UI-09) | Pane wheel `:2223`: deltaY=0 becomes -1, line/page units divided as pixels, tiny deltas become full rows. IPC casts rows to i16. | Unit-aware accumulated bounded conversion, horizontal-only no-op; P02. Windows does not necessarily emit line-mode events. |
| native-input-08 | Pane compositionEnd `:2395-2406` arms tail suppression; later noncomposing identical key `:2330-2345` discarded indefinitely, without WebKit provenance/platform gate. | Distinguish replay from independent Windows key; P02. Real IME ordering unverified. |
| native-input-10 (WIN-UI-08/B08) | Tauri drop -> pane `:1640-1680` -> POSIX quoteShellPath `:376-381` -> paste; cmd single quotes literal, PowerShell apostrophe escaping differs. | Target shell/platform-aware quoting and WSL path translation; P02. Never branch solely on desktop OS. |
| WIN-UI-03 | `U/lib/shortcuts.ts:149-275,472-473`: Ctrl+digits tab bindings precede mod+digits workspace bindings, identical on Windows; first match returns. | Distinct workspace binding (guest bridge already uses Alt+digits); P04. |
| WIN-UI-13 | `U/App.tsx:2278,2372-2376`: browser actions enabled only by tab.kind; `TerminalSplitView.tsx:1226-1241` renders browser leaf within terminal tab. | Derive active action target from focused leaf; P05. Guest-native bridge behavior still requires integration proof. |
| DS-04 (B06) | cmd_agents_detect -> `R/ipc/agents.rs:510-525,570-595`: exact filename only, no extension resolution. | PATHEXT order/case/runnability policy; P09. |
| DS-05 | Daemon startup `server.rs:1497-1499` gates listener Unix-only; local PTY `pty.rs:134-138` advertises socket on Windows; extension TCP port/token supported but not supplied. | Host authenticated endpoint before advertising env; P08. |
| DS-06 (B07, PKG-12 partly) | Discovery -> `R/ipc/agents.rs:71-80,172-204,313-353` uses ps/lsof/Windows-None CWD; `daemon/agent_extension.rs:14-25` HOME-only installation. | Authoritative provider metadata / Windows discovery and USERPROFILE fallback; P09. Persisted IDs remain usable. |
| RF-06 | Bounds and warm attach -> `N/surface_host.rs:1067-1102,1231-1255` skip terminal.resize when grid unchanged, then replace metrics. Ghostty pixel cell geometry remains stale after DPI-only metric change. | Compare pixel geometry too; P06. |
| RF-07 | Native child top z-order + `U/lib/nativeTerminalVisibility.tsx:12-27` recognizes only dialog/search; pane `:1019-1041` emits real Sonner toast (section/ol). | Explicit visible-toast yielding policy; P07. Not all dialogs are occluded. |
| RF-08 | `N/renderer/font_manager.rs:349-355,449-455` and renderer contract `font_rasterization.rs:29-47` assume RGBA stride, Windows returns Alpha. | Variant-aware meaningful assertions and controlled fonts; P06. No app symptom; validation defect. |
| FSSH-03 | Import parser `R/ssh/config.rs:72-76,94-100,124` keeps IdentityFile quote delimiters -> direct host validation rejects quoted absolute path. | Parse OpenSSH quoting into semantic path; P10. |
| FSSH-04 | Worktree create -> `R/worktree/manager.rs:385-420,442-451` accepts pipe/angle/quote characters and reserved device components before Windows path creation. | Validate Windows path components at local/remote target boundary; P10. Device-name examples must exercise actual prefixed path components, not assume wt-CON itself is reserved. |
| FSSH-T01 | `R/ssh/direct_tests.rs:4-37` local /keys fixture invalid on Windows; bridge `:23-31` omits helper.exe; `ssh/worktree.rs:531-590` executes local bash/POSIX paths unconditionally. | Native fixtures and executable tests; remove timing-luck cancellation through exact request barrier; P10. |
| DS-09 narrowed | `R/terminal/tests.rs:18-21` included on Windows but spawns /bin/sh; removal helper `:7-14` polls. | Real ConPTY fixture plus event-driven exit/ready signals; P08. IPC tests cited upstream are Unix-gated, so that subclaim is rejected. |
| BROWSER-IPC-B03 | Browser toolbar flags -> manager `:237-282` shadow URL history -> browser IPC `:802-834,1196-1267`; Windows never installs native history flags. Same-document entries absent from observed load/title updates strand Back. | WebView2 HistoryChanged/engine flags and completion reconciliation; P13. |
| BROWSER-IPC-B04 | Coordinator System sound -> `R/ipc/notifications.rs:50-67` / notify adapter `:29-59` omit sound; notify-rust 4.18.0 windows.rs:50-86 sends None -> winrt 0.7.3 lib.rs:474-483 emits silent=true. | Map System/Silent on both paths; P14. |
| BROWSER-IPC-B05 | CLI keypress -> `R/ipc/browser.rs:1632-1641,342-352` synthetic untrusted KeyboardEvent then success; no native Backspace/Tab default action. | Trusted input or explicit unsupported, never false success; P13. |
| BROWSER-IPC-B09 | Enable local network -> `R/remote/state.rs:149-190,217-227`: enumeration is two off-link UDP route probes. Usable isolated LAN with neither route fails resolver. | Actual Windows adapter enumeration with deterministic route-failure fixture; P15. |
| BROWSER-IPC-B10 | Terminal link -> `R/ipc/browser.rs:1709-1725`: HOME-only ~/ expansion; bare ~ joins literal ~ even with HOME. | Framework/profile home resolution and correct bare-tilde case; P13. |
| BROWSER-IPC-B11 LOW | Debug UI -> `R/ipc/debug.rs:33-54`: opens Unix-root /tmp path, no mkdir; standard Windows without root tmp loses trace. | Portable sink; P02 owns all sibling sites (`R/lib.rs:983`, native IPC `:1370,1484`) to avoid partial repair. |
| BROWSER-IPC-B12 LOW | Permissions fetch -> `U/components/settings/PermissionsSection.tsx:175-189`: ungranted/non-authoritative Windows state renders macOS FDA warning. | Capability/platform gate; P21. No prose-pinning test. |
| COV-CI-1 (PKG-08) | `.github/workflows/build-test.yml:123-150`: Windows cargo check/build only; cargo test Linux-only. | Real isolated Windows test gate after validator repairs; P18. Zero tests is not a pass. |
| COV-BLD-1 (PKG-10) LOW | `src-tauri/build.rs:30-35,44-63` emits MSVC /MANIFEST flags for any Windows ABI, including GNU executable links. | Gate both branches by target_env=msvc and preserve GNU resource path; P18. cargo check is not a linker reproducer. |

## Additional QA helper findings (source-confirmed, no runtime run)

| ID | Independently reopened mechanism and reachable chain | Packet |
|---|---|---|
| COV-QA-01 HIGH | `script/qa/win-daemon-e2e.mjs:46-84`: main/attachStream -> connectClient.request stores wrapper resolve (`:77`), but timeout compares against original res (`:72`), leaving expired queue entry. Every parsed frame unconditionally shifts pending (`:62-64`), so unsolicited output or a late response can consume/misassign a request. Listener exceptions can also reject an unrelated pending request. | P22: explicit response/stream dispatch and timeout-poisoned uncorrelated connection; do not merely splice and misroute the late reply. |
| COV-QA-02 MEDIUM | Same helper `:89-136`: handshake/attach error rejects without closing acquired client; main assigns streamClient only after successful await (`:194`), so its finally cannot close failed attachment. Returned close (`:134`) clears waiter timers without resolving/rejecting their promises; connectClient has no end/close rejection path. | P22: ownership-safe failure cleanup and exactly-once settlement on close/error/EOF for requests and signal waiters. CLI process.exit may mask leaked resources; reuse/failure paths are still defective. |
| COV-QA-03 HIGH | Helper main handshake `:177` and attach handshake `:116` hardcode version 2; current `src-tauri/src/daemon/protocol.rs:13` is 3. Actual current daemon rejects handshake before spawn/ConPTY probe. Existing --self-test (`:140-154`) tests only parsing and cannot detect this incompatibility. | P22: current-version contract shared/checked at helper boundary with explicit mismatch rejection; no silent fallback to stale daemon. |

## Foreign-owner confirmed findings (do not silently incorporate into main repair)

| ID | Evidence and corrected bound | Packet |
|---|---|---|
| FSSH-X01 | Foreign `R/worktree/rescan.rs:412-418,439-446` resets last_scan for unchanged fingerprint on every sweep; repeated 30s sweeps prevent 300s forced scan. | PX, owner only. Inject time; require real scan at 300s. |
| FSSH-X02 | Same file `:267-278`: splits only `/` before joining native Windows path. Correct reachable malicious input is `refs/..\..\outside`; upstream `refs\..` example fails starts_with and is not a reproducer. | PX, owner only. Assert outside sentinel never read. |

## Rejected, downgraded and deduplicated claims

- WIN-UI-05: async IPC before navigator.clipboard.writeText does not prove activation expires on Chromium. Real permission/focus clipboard probe required; no native clipboard rewrite authorized by this allegation.
- WIN-UI-07 / PKG-04: REFUTED mechanism. Locked tauri-runtime-wry 2.11.4 `src/lib.rs:864-878,1200` applies hiddenTitle/titleBarStyle only under macOS cfg. Windows config Overlay does not establish top-right caption overlap. Do not add 140px padding or change titlebar to fix an unproved condition.
- WIN-UI-10: REFUTED reachability. No App caller of installContextMenuGuard; only module/tests found. DOM contextmenu cancellation also would not establish native caption-menu failure.
- WIN-UI-11: preventDefault alone does not prove native double-click maximize failure; real drag/double-click probe needed. WIN-UI-12 36px spacer is design spacing, not a correctness defect.
- WIN-UI-01: source confirms capture of configured Ctrl chords (`U/lib/shortcuts.ts:429-446,472-473`), but terminal-control overlap is a policy constraint, not authorization to replace defaults. Removed from confirmed repair defects. Preserve user-established Ctrl+W close, Ctrl+V paste and Ctrl+click links; no wholesale Ctrl remapping in P04 or another packet. Any policy change needs separate explicit approval. WIN-UI-03 duplicate workspace/tab Ctrl+digits remains actionable.
- native-input-09 / WIN-UI-04: bare Ctrl+V paste versus SYN is a capability/policy conflict, not an automatically approved defect. Preserve current paste default; P04 does not own a new paste policy. P02 must also retain established Ctrl+click link behavior while repairing ordinary TUI tracking.
- RF-05: Windows color glyph function explicitly returns None. Confirmed missing color-emoji capability, not a new regression requiring unconditional redesign. Product parity decision and installed-font/color probe required; optional P06 extension.
- DS-08: UpgradeUnsupported is explicit Windows lifecycle capability gap, not session loss. P08 must stop recursive attempts without implementing unsafe active-session replacement. Zero-session upgrade and active handover are separately scoped future work.
- FSSH-05: missing PTY coordination is real, but Git refusal under deny-delete handles may be correct safe behavior. Record typed in-use UX/PTY-cwd scenario as gap, not unconditional corruption. Never kill sessions or add retries on this report alone.
- PKG-05 HOME crash refuted: cli_install.rs:77-89 returns unsupported before home use. Windows launcher UI unsupported is a feature gap; installer/PATH/alias behavior needs separate contract.
- PKG-06 autostart caller not established; launchd-only source is not a demonstrated Windows autostart regression. Reboot does not preserve arbitrary PTY processes on any OS.
- PKG-07: MSI remains in base targets but missing WiX-on-PATH does not prove Tauri cannot provision its tooling. Release contract chooses NSIS/MSIX; default-build policy mismatch is a gap, not a verified build failure.
- PKG-09: macOS-private-api metadata on Windows has no demonstrated adverse effect. No config/feature cleanup packet for symmetry alone.
- PKG-11: packaged AUMID attribution needs actual package identity/toast probe; omitting app_id falls back to PowerShell identity in notify-rust, so the proposed fix is not established.
- COV-TOOL-1: macos-dev-runner's non-Darwin cargo fallback is deliberate; root package scripts route cargo tauri, not directly this script. No demonstrated Windows asset-staging defect there.
- COV-BLD-1 is a conditional source issue: GNU linking would receive MSVC flags; cargo check is not a linker reproducer. P18 does not establish Windows GNU support, add it to release/CI support, or authorize installing a new toolchain. Native GNU verification remains unavailable unless an already provisioned environment is separately authorized.
- Prior cookie-set deadlock, universal CJK tofu, all-open-handles rename failure, raw/canonical jail mismatch and missing drop bridge are NOT accepted blockers. Domain reports contain specific refutations; current shell menu forwarding and child-HWND startup repair already exist. Historical installed executable remains stale.
- Packaging report claims cargo/bun tests ran without logs/exit receipts and despite audit-only scope. Those claims are unverified and provide no acceptance evidence here.

## PR disposition

- Fresh `gh pr list --state open --limit 100 --json number,headRefOid,title` returned exactly #2 and #3; both full diffs reread. No merge, close, commit or ref change performed.
- #2 head 79b02ab6: sound narrow WS_DISABLED fix; test uses production constructor and isolated desktop. Cross-thread WindowFromPoint query is not real WebView2 wheel/capture/DnD proof. P01 must obtain same-assertion RED/GREEN and P02 runtime wheel receipts.
- #3 head 99c7086b: sound optional-kind fix against HEAD; parameterized tagged/untagged + pinned/unpinned tests use mocked store and metaKey only. Add Windows Ctrl-path and real sibling PTY survival. Foreign App.tsx/App.test.tsx changed during review: preserve handleClosePane confirmation semantics, rebase/adapt by owner, no blind cherry-pick.
- Foreign `docs/CMD_W_TAB_CLOSE_ROOT_CAUSE_2026-09-12.md` read as context only: its macOS live-store and historical test receipts are not this review's Windows QA. Reopened current App.tsx:1783-1817 routes tagged terminal leaf-root layouts through busy-agent-aware handleClosePane but retains kind===terminal. That owner change and PR #3's optional-kind correction are distinct; neither is adopted or edited here.
- Source ledger complete; application correctness, all-open-PR resolution and installed runtime repair remain pending later authorized implementation and QA.
