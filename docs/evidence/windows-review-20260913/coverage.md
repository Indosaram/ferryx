# Windows coverage reconciliation - 2026-09-13

Status: synthesis COMPLETE, comprehensive correctness verdict INCONCLUSIVE.
Lead continuation: the initial inventory and gap descriptions below are
historical, not the latest coverage state. `inventory-reconciled.json`
supersedes their counts: 1,854 index entries, 362 candidates, 342 included
file-ownership sites and 20 excluded candidates. Lead independently
recomputed index SHA256
`b5c0ab6db7f9e928b5b5133a4adb066b5d7baa88f2745ffec3e80bd963e926c8`,
path-set equality and classification counts. Sites are not AST branches.

The subsequent parser-backed `platform-branch-ledger.json` and companion
Markdown enumerate 23,316 unique range-and-kind entries across 968
source/config files. Lead independently checked all 968 hashes, unique
IDs, in-bounds byte ranges and disjoint kind counts with zero mismatches.
See continuation-20260913-1705.md. This completes the enumeration task,
not semantic closure: 86 exact parser/format gaps and global alias/Cargo/
macro qualifications remain explicit. Existing bounded source receipts
must be reconciled to these locations, not discarded or blindly repeated.
P03/P11 test edits after this check make their earlier hashes historical.

The exact 59 pending-bounded paths now map to 59 unique verified source
receipts, with zero omissions or extras:
`closure-images.md` (22), `closure-nativeUi.md` (11),
`closure-otherUi.md` (18), `closure-tooling.md` (8).
Lead read all four reports, checked every reported file hash and directly
reopened their new defect mechanisms/callers. RemoteSessionList is bound
to the newer reported dirty hash rather than relabeled as the census hash.
The JSON remains an immutable census; these reports are its follow-up.

The additional 25 bounded-source dispositions now have explicit receipts:
`bounded-infrastructure.md` (14) and `bounded-ui.md` (11). Lead read both
reports and independently matched all 25 file hashes, with no omissions or
duplicates. Full shared-file reads and explicit Mac-body exclusions are
distinguished in the reports. New sorting/device-name and prototype-history
mechanisms were directly reopened; none has executed RED/GREEN evidence.

Seven previously not-selected common native modules/tests have full source
receipts in `shared-native-callers.md`. Lead read the report, independently
matched all seven hashes and reopened the two new mechanisms through actual
consumers/backend or installed dependency. These are source dispositions,
not runtime success or proof that all other noncandidates were reviewed.

Five previously not-selected terminal transport files now have full-file
receipts in `transport-source-closure.md` (408 lines). The lead followed the
actual workspace-restore fallback and security test, correcting incomplete
LSP reference results with explicit TS/TSX structural searches. No shipped
WebSocket adapter consumer was established. A session-list test expectation
mismatch is recorded separately; it is not a Windows wheel root cause.

Historical 146-ID reconciliation is in `gap-history.md`. Backend/UI/tooling
gaps are dispositioned in `gap-backend.md`, `gap-ui.md`, `gap-tooling.md`,
`gap-verification.md`, `remaining-remote-callers.md`,
`remaining-contracts.md`, and `remaining-powershell-wrappers.md`.
Read their explicit exclusions and refutations; generic lane labels are
not additional full-file review claims.

Remaining coverage obligation: qualify the final foreign/concurrent diff
against the implementation base and preserve semantic caller/ancestor
coverage of shared noncandidates. No regex census establishes whole-program
correctness, and the 25 receipts do not certify every shared module.
Local transport validator and P34 focus repair now have RED/GREEN and
mutation evidence in red-green.md; cleanup.md covers their completed local
runs. P04 and P28 Opera repairs are in progress. Native Windows behavior,
aggregate cleanup, gate and delivery remain unverified; acceptance-status.md
maps those obligations. The source receipts below retain their historical
snapshot meaning and are not silently promoted to current runtime evidence.

All eight upstream reports are present and were read in full: native-input, ui-interactions, renderer-fonts, daemon-shell, filesystem-ssh, browser-ipc, packaging, coverage.
No report is ack-only/blocked. Presence is not approval: packaging execution claims lack receipts, and coverage's exhaustive-count claims do not survive cross-checking.
Only coverage.md, findings.md and repair-packets.md are this synthesis child's writes. No source/test/config/ref/runtime changes; no build/test suite/desktop launch.
HEAD baseline b7ad4516. Foreign work expanded from 23 tracked source-related dirty files in upstream receipts to 26 total tracked modified paths during synthesis, including App.test.tsx and App.tsx close flow. No frozen-tree certification.

## Method and limits

- Read root/backend/native/daemon/terminal/worktree/IPC/UI/components/lib AGENTS, programming/debugging/ast-grep skills under `/Users/indo/code/oh-my-openagent/packages/shared-skills/skills/`, and installed frontend-design skill. No code edits requiring implementation language gates.
- Used daemon client LSP symbols, ast-grep call-shape search, read-based source/callee inspection, Bun in-memory tracked-file inventory, read-only GitHub PR list/diffs, and locked Rust dependency source. Every retained finding mechanism was reopened; upstream claims not independently supported were downgraded or rejected.
- Read September 12 FINAL-AUDIT in full and September 7 audit selectively. Historical installed PE/debug receipts are NOT current-tree runtime QA; installed application must remain untouched.
- Inventory predicate used on `git ls-files`: filename Windows/win32 (case-insensitive), or content `target_os = "windows"`, cfg(windows)/cfg(not(windows)), win32, or quoted windows. Excluded docs, .omo, vendor, generated schemas, ui/dist and lockfiles from executable-source counts.
- Result: 85 matched tracked paths, explicitly dispositioned below. This is a conservative lexical inventory, NOT all reachable Windows code. Non-Unix/default implementations and shared code are reconciled separately. Unmatched shared code not named in lane coverage remains an explicit gap rather than silently clean.
- Coverage producer says 43 files/125 hits, but its own lane file counts total 60 (3+9+6+10+9+11+12), with overlaps and unnamed entries; its branch counts total 136 (15+8+8+32+31+27+15), not 125. Its native build path omits that `build_ghostty.rs` lives under `src-tauri/native_terminal/`, not `src/`.
- Its historical lane subtotals total 122, but individual enumerations/ranges disagree with their headings and are mostly RECHECK, not dispositions. Therefore 100% partitioned/zero-orphan/all-122-rechecked claims are rejected. No exact historical completion percentage is certified.

## Disposition key

R = reviewed source by named lane and cross-checked at its finding/refutation boundary; NOT runtime-green.
T = test/probe source reviewed, actual Windows execution pending; known invalid or nondeterministic seams identified.
G = explicit missing/incomplete source coverage; owner assigned below, not clean.
A = artifact/config contract only; no executable binary validation.
Prefixes: R/ = src-tauri/src/, N/ = R/native_terminal/, U/ = ui/src/. Braces enumerate each listed path individually.

## All 85 lexical-inventory paths

| Paths | Owner / disposition |
|---|---|
| `.github/workflows/build-test.yml` | packaging + coverage R: no Windows test step, COV-CI-1; unsafe persistence harness cannot simply be added. |
| `remote-helper/Cargo.toml` | filesystem-ssh G: helper process source reviewed but manifest/features/toolchain not independently accounted by coverage producer. |
| `scripts/build-latest-json.test.mjs` | packaging T: updater matrix contract, runtime artifacts unverified. |
| `scripts/lib/{release-hosts,release-platforms}.mjs` | packaging R: host/artifact selection; MSIX resource gap PKG-03. |
| `scripts/qa/{ssh-bridge-survival,ssh-helper-setup,ssh-helper-survival,verify-ferryx-resume-cwd}.mjs` | filesystem-ssh/daemon G: named runtime harnesses exist; complete Windows safety/fixture audit absent. Do not execute by name alone. |
| `scripts/release-hosts.example.json` | packaging A: host examples, not a verified Windows host. |
| `scripts/{release-hosts.test,release-local,release-local.test,release-workflow.test}.mjs` | packaging R/T: local release policy source; no execution receipts accepted. |
| `site/src/components/{FeatureVisuals.tsx,ui/PlatformIcons.tsx}` | packaging G: public Windows imagery/copy outside seven reports' actual review. |
| `site/src/lib/{downloads.ts,downloads.test.ts}` | packaging G: Windows download routing/user-visible artifact selection unreviewed. |
| `src-tauri/{Cargo.toml,build.rs}` | packaging R: dependencies/features, manifest ABI issue COV-BLD-1. |
| `src-tauri/examples/web_remote_security_qa.rs` | browser-ipc G: executable QA example not covered by lane's command-wrapper inventory. |
| `src-tauri/native_terminal/build_ghostty.rs` | packaging R: Zig target/link policy; no current Windows link run. |
| `src-tauri/resources/helpers/x86_64-pc-windows-msvc/ferryx-remote-helper.exe` | filesystem-ssh/packaging A: shipped opaque PE, source/manifest selection reviewed; binary provenance/runtime not independently validated. |
| `R/browser/{security,tests}.rs` | browser-ipc R/T: URL/platform UA and tests; cmd-boundary B02 not solved by scheme validation. |
| `R/cli.rs` | browser-ipc/packaging R: browser automation dispatch, launcher boundary. |
| `R/clipboard_image.rs` | browser-ipc R: Windows PNG/DIB reader; actual clipboard formats, ownership and permissions G. |
| `R/daemon/server.rs` | daemon/filesystem R: DS-01/05/07, FSSH-02; endpoint security, agent-state and remote cwd. |
| `R/ferryx_scope/design/native.rs` | filesystem-ssh G: Windows design/open integration not reviewed in seven reports. |
| `R/ferryx_scope/ssh/{helper,mod,process}.rs` | filesystem-ssh R: canonical jail, ACL/endpoint trust and lifecycle; live alias/ACL/AV behavior G. |
| `R/ferryx_scope/ssh/{helper_core_tests,helper_service_tests}.rs` | filesystem-ssh T: Windows helper seams reviewed, native runs pending. |
| `R/ferryx_scope/ssh/process/process_windows.rs` | filesystem-ssh R: Job Object, CIM/argv and helper ownership; no runtime proof today. |
| `R/ipc/browser.rs` | browser-ipc R: B02/03/05/10; profile/cookies/download wrappers; actual native WebView2 scenarios G. |
| `R/ipc/cli_install.rs` | browser-ipc/packaging R: explicit unsupported before HOME; claimed crash refuted. |
| `R/ipc/native_terminal.rs` | native-input/browser R: wheel context, tracking, clipboard; debug sink siblings included. |
| `R/ipc/notifications.rs` | browser-ipc R: sound B04; picker callback/oneshot, no deadlock established. |
| `R/ipc/project.rs` | filesystem/browser R: canonical registration/Explorer reveal, not a missing Windows branch. |
| `R/ipc/updater.rs` | packaging R: exact cmd_ names; frontend mismatch PKG-01. |
| `R/{lib,main}.rs` | daemon/native/browser/packaging R: actual setup/dispatch inspected; foreign image/rescan additions and close integration require frozen-tree recheck. |
| `N/composition.rs` | renderer R: geometry/descriptor; fallback descriptor is not actual child constructor descriptor. |
| `N/platform/{mod,windows,windows_focus}.rs` | native-input/renderer R: PR #2 hit routing, native-input-03 focus, owner-thread destruction; actual wheel delivery G. |
| `N/renderer/{font_manager,mod}.rs` | renderer R: RF-02/03/04/08, GDI cfg; no actual installed-font/GPU certification. |
| `R/notification/{mod,model,notify_rust_adapter,tests}.rs` | browser-ipc R/T: sound B04; native toast attribution/click/Focus Assist G. |
| `R/permissions/mod.rs` | browser-ipc R: capability status; Windows FDA warning caller B12. |
| `R/remote/{auth,state}.rs` | browser-ipc R: portable transactions and explicit network selection; B09; overridden-dir ACL and network/firewall G. |
| `R/ssh/bridge_tests.rs` | filesystem-ssh T: missing .exe fixture + timing-luck cancellation FSSH-T01. |
| `R/ssh/{helper_assets,runtime}.rs` | filesystem-ssh R: target/schema/hash assets and remote OS semantics; binary/source pairing G. |
| `R/terminal/{preferences,resume_cwd,shell}.rs` | daemon/filesystem R: existing Windows profiles/CLI lookup; DS-03; optional Ghostty config-location contract G. |
| `R/util/mod.rs` | daemon R: no-window spawn helpers; does not supply command-language escaping. |
| `R/worktree/mod.rs` | filesystem-ssh R: canonical jail and foreign rescan exports; owner-only findings PX. |
| `src-tauri/{tauri.conf,tauri.linux.conf,tauri.windows.conf}.json` | packaging R/A: resources/opacity; titleBarStyle Windows overlap rationale refuted by locked runtime cfg. |
| `src-tauri/tests/{native_terminal_surface_host_contract,permissions_contract,rorca_native_contract}.rs` | renderer/browser T: contracts reviewed selectively, full Windows behavior/fixture pass G. |
| `src-tauri/tests/ssh_windows_live.rs` | filesystem-ssh T: opt-in live seam, no native SSH run; opt-out is not pass evidence. |
| `src-tauri/tests/{updater_config_contract,updater_endpoint_contract}.rs` | packaging T: policy/config contracts, no actual updater/package runtime. |
| `src-tauri/tests/windows_edge_probe_contract.rs` | coverage/packaging T: COV-TEST-1 depends on ignored/untracked ephemeral include. Wrapper and driver recovered locally; fresh tracked checkout lacks them. Compilation not run. |
| `src-tauri/tests/windows_window_opacity_contract.rs` | packaging T: source opacity contract, not caption input or GPU proof. |
| `src-tauri/windows/msix/{AppxManifest.xml,priconfig.xml}` | packaging A: Store identity contract; certification/provisioning/runtime assets G. |
| `U/components/settings/{PermissionsSection.tsx,PermissionsSection.test.tsx}` | browser-ipc R/T: B12 rendering; existing tests not run. |
| `U/components/settings/{SshSection.tsx,SshSection.test.tsx}` | filesystem-ssh G: reports cover config/parser/IPC but do not fully audit this settings surface and its Windows fixtures. |
| `U/lib/{sshHosts.ts,types.ts}` | filesystem/UI R: host identity/DTO and optional terminal kind; PR #3 contract independently reopened. |
| `U/lib/{windowsStoreMigration.ts,windowsStoreMigration.test.ts}` | packaging R/T: PKG-01; mocks must reject unknown command identifiers. |

## Upstream inventory paths omitted by literal Windows predicate

These are Windows-reachable through shared/default/non-Unix code; lexical absence is not exclusion.

| Paths / expansion of upstream grouping | Disposition |
|---|---|
| `U/{App.tsx,App.test.tsx}`, `U/components/{TabBar,TerminalSplitView,NativeTerminalPane,BrowserPane,BrowserToolbar}.tsx` | UI/native/browser R/T: close, split, shell menu, keyboard, mouse/drop/wheel, browser dispatch. False UI claims rejected in findings.md; foreign App overlap explicit. |
| `U/lib/{shortcuts,updater,contextMenuGuard}.ts`, `U/lib/nativeTerminalVisibility.tsx`, `U/remote/RemoteTerminal.tsx` | UI/native/renderer R: shortcut conflicts, updater identifier, uncalled guard, toast yielding. Remote wheel path only selectively reviewed; full remote input parity G. |
| `N/{surface_host,terminal,input,key_encoder,mouse,mouse_encoder,wheel,selection,scroll}.rs` | native/renderer R: all confirmed input/GPU geometry mechanisms reopened. Shared encoding supports alt-screen wheel; native delivery remains G. |
| `N/renderer/{directwrite_raster,color_glyph,atlas,row_cache,instances,gpu_context,pipeline,shaders,pass,renderer,render_target,scenario,types,rasterizer}.rs` | renderer R: RF-01..08 source; font coverage, hybrid GPU/driver/format and pixel screenshots G. |
| `N/{child_surface,surface_error,cell_extractor,render_pass,snapshot,surface_snapshot,lifecycle}.rs` | renderer R: lifecycle/resize/recovery source reviewed by lane; no runtime guarantee. |
| `R/daemon/{client,proxy,handover,manifest,protocol,launchd,agent_extension,agent_state}.rs`, `R/terminal/{pty,session,service,remote,output_hub,metrics,mod}.rs` | daemon R: security/upgrade/PTY/remote lifecycle; DS-01..10 and explicit feature gaps. |
| `R/ipc/{agents,terminal,worktree,ssh,project_remote,session,preferences,dag,remote,browser_cli,permissions,native_menu,debug,diagnostics,error,mod,native_terminal_disabled}.rs` | applicable daemon/filesystem/browser R: caller/command boundaries; synchronous wrapper presence is not Windows execution. |
| `R/ssh/{mod,config,exec,direct,operations,browse,worktree,helper_setup,bridge,state_bridge,projects}.rs` | filesystem R: FSSH-01..06 and qualified rename/canonicalization refutations. |
| `R/ferryx_scope/ssh/{config,standalone}.rs`, `R/worktree/{manager,registry,git,model}.rs`, `R/session/mod.rs` | filesystem R: jail/persistence/delete boundaries; concurrent fixed-temp publication and busy checkout remain G. |
| `R/browser/{manager,model,guest,cookies,download,find}.rs`, `R/notification/{service,permission,audio,activation,badge}.rs`, `R/remote/server.rs`, `R/remote/discovery/tailscale.rs` | browser R: history/auth/listeners/OS integration; provider OAuth, authenticated downloads, package toast and clipboard native QA G. |
| `scripts/{build-msix.ps1,test-build-msix.ps1,release-workflow-policy.mjs,build-latest-json.mjs,lib/release-contract.mjs}`, `src-tauri/tauri.macos.conf.json`, `src-tauri/capabilities/` | packaging R/A: MSIX staging and local-only release policy; no release execution. |
| `src-tauri/tests/daemon_persistence_contract.rs`, `R/terminal/tests.rs`, `R/ssh/direct_tests.rs`, `R/ssh/worktree.rs` tests | T: unsafe shared daemon endpoint, Unix-only/invalid Windows fixtures and nondeterminism explicitly recorded; no blanket cargo test command approved. |
| Native renderer/engine/child/input test contract trees and SSH/session/worktree integration targets named in domain reports | T: reviewed selectively; Windows actual test counts, GPU fixture isolation and live opt-in execution remain explicit G. |

## Outside-current-review gaps, not clean exclusions

- Supplemental named-path inventory: `script/qa/win-daemon-e2e.mjs` (singular script/) independently read in full and mapped to coverage/daemon P22; COV-QA-01/02 queue/lifetime defects and COV-QA-03 version-2/current-version-3 mismatch confirmed in source. This adds a reviewed path outside the earlier 85-path snapshot; that snapshot is not an exhaustive current inventory. Pure --self-test currently supplies no transport or ConPTY evidence.
- `R/remote/relay_server.rs`, `R/remote/tests.rs`, `R/dag/` Windows mentions/fixtures, `scripts/qa/ssh-process-survival.mjs`, `scripts/build-remote-helpers.mjs`, `U/components/RemoteDirectoryPicker.tsx`, remote UI/input and every shared source module not explicitly covered above: additional broad Windows text scan exposed these paths; complete branch/caller reconciliation absent. Assign relay/DAG to browser-ipc, helper build to packaging, picker to filesystem-ssh, remote UI to ui-interactions.
- All vendor/generated files excluded from inventory counts, not from behavior: locked portable-pty, Tauri/Wry, notify-rust/winrt and Ghostty key/resize callees were read selectively. Entire transitive dependency audit is not claimed. Generated windows-schema and shipped helper PE need artifact provenance gates.
- Foreign `N/images.rs`, png_decoder.rs, sys/kitty.rs, renderer/images.rs and renderer integration are moving-owner code; renderer lane reviewed source selectively, no Windows image acceptance. Foreign `R/worktree/rescan.rs` has PX findings. Re-run frozen-tree review after owners finish.
- September 7 historical IDs not individually dispositioned in the seven domain tables are unresolved historical coverage, including partially grouped L1 cfg and L10 tooling items. Coverage producer's RECHECK rows are assignments, never clean dispositions; seven domain reports' explicit refutations take precedence over those stale assignments.
- Required runtime matrix: real wheel (primary/alt/tracking/Shift/split/unfocused/horizontal/DPI); pointer selection/capture/window exit; keyboard/IME/AltGr/clipboard/drop; cmd/pwsh/powershell/WSL echo; resize/font/DPI; browser history/editing/isolation; reconnect/provider IDs/sibling PTY survival; remote LAN/auth; signed/package asset/update/toast behavior. None was run here.

## All-open-PR reconciliation and acceptance state

- Persisted lead adjudication: WIN-UI-01 is a policy constraint, not approved default-remapping work; P04 repairs only duplicate workspace/tab Ctrl+digits. Existing Ctrl+W close, Ctrl+V paste and Ctrl+click links remain unchanged. P18's GNU ABI finding is conditional and neither establishes GNU support nor permits toolchain installation.
- P22 QA-helper packet update is complete in findings.md/repair-packets.md (queue dispatch, lifetime settlement and protocol drift). Separate missing-coverage audits are being dispatched by the lead; this synthesis does not expand into remaining G gaps or wait for those reports. All acceptance criteria remain pending.
- GitHub list returned only #2 (79b02ab6) and #3 (99c7086b); both diffs read completely. P01/P05 own their repairs. No PR has been merged/closed by this review.
- #2 correct input-disabled child scope, but its WindowFromPoint test is not real wheel, capture or WebView2 delivery proof.
- #3 optional-kind discriminator correct against HEAD; Windows ctrlKey and actual sibling process survival remain missing, and foreign close-confirmation edits require owner-safe forward adaptation.
- Foreign close report `docs/CMD_W_TAB_CLOSE_ROOT_CAUSE_2026-09-12.md` is contextual only. Current tagged leaf-root close/busy-agent confirmation remains the other session's work; optional-kind bug remains distinct. No macOS live-store receipt is counted as current Windows QA. Lead reports C001-C003 pending; no criteria or loop state changed by synthesis.
- Historical FINAL-AUDIT verifies earlier cold debug shell/resize and source child startup repair, not current wheel or stale installed-binary replacement.
- Acceptance remains INCONCLUSIVE until explicit gaps are resolved or scoped out by the parent/user and confirmed defects obtain same-assertion RED/GREEN plus faithful binary receipts. No unsupported lane report is silently treated as clean.
- Report validation: all three reports below 220 lines; whitespace check completed. Markdown LSP diagnostics requested for all three, but no .md server is configured; no configuration changed. Foreign tracked modifications reached 30 paths during final validation, so observed line citations can drift and must be reopened before implementation.
