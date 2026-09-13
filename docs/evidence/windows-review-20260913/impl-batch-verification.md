# Sixteen-packet implementation batch adjudication

Verifier: st_01a09a24. Parent/root: 01a0983f-c995-753d-afa9-593f6d118788.
Cutoff: 2026-09-13 09:45 UTC. Darwin arm64 shared working tree; not a frozen release snapshot.

## Decision

**Combined acceptance FAIL / incomplete. No packet-level or original-goal completion is granted.** Nine packets have some executed local positive evidence (P02, P06, P08, P09, P13, P18, P25, P26, P29); P17 has source-contract checks only; P10/P12/P19/P23/P24/P30 are regression staging, not repaired behavior. Every packet retains native obligations.

Read all sixteen impl reports, repair-packets.md and gap-packet-addendum.md, current source diffs, relevant callers and actual retained logs/JSON, including temporary P06/P08 Cargo receipts. No acceptance is derived from node-completed, top-level toolkit ok, report headings, zero tests, or refusal-to-run exits. Source review found a deleted-test regression and a rejected registration misreported as accepted. These independently block acceptance even where narrow tests pass.

Only this report was authored. No production/test fixes, Cargo invocation, dependency/cache provisioning, native launch, daemon contact, branch/worktree/ref creation, commit, push, release, or install occurred. No exclusive Cargo grant names this verifier. A prior owner's released slot is not authorization to acquire its target; no undocumented dependency was launched or awaited.

## Fresh safe combined checks

Executed once on the current shared source at approximately 09:44 UTC:

| Command | Actual outcome |
|---|---|
| `node --experimental-vm-modules --test scripts/qa/ssh-harness-safety.test.mjs scripts/build-remote-helpers.test.mjs scripts/release-workflow.test.mjs scripts/build-msix.test.mjs` | **exit1: 52 tests, 51 pass, 1 fail, zero skipped/cancelled.** P25 27 cases and P17 8 source cases pass; workflow 16/17. |
| `CI=1 bun run --cwd ui test src/components/NativeTerminalPane.test.tsx src/components/settings/TerminalSection.test.tsx src/lib/browserTauri.test.ts` | **exit0: 3 files, 185 tests pass** (168 + 3 + 14), one invocation, 4.92s. Intentional attach-error fixture stderr retained. This does not restore the eight deleted P09 tests. |
| `git diff --check` | exit0 across current tracked diffs. Not compilation or semantic acceptance. |

Exact fresh failure:

```text
not ok 47 - Pages deployment workflow (.github/workflows/deploy-pages.yml) complies with release policy
location: scripts/release-workflow.test.mjs:315:1
Target not found: /Users/indo/code/project/orca-lite/.github/workflows/deploy-pages.yml
1 !== 0
COMBINED_NODE_EXIT=1
```

The identical missing-workflow failure already appears in P18 RED and GREEN logs: pre-existing relative to this batch's workflow repair, not introduced by this verifier. Do not create an unrelated workflow or delete/skip the assertion to green the batch. Full UI/build receipts remain separately qualified below. No whole-crate build or real-surface execution is claimed.

## Defect-to-oracle disposition

R/G below means actual same behavioral assertions observed failing then passing at the stated seam. It never means native Windows execution unless explicitly stated. All paths in this section are relative to this evidence directory unless source paths are spelled out.

### P02 - input and debug

- native-input-02/03/04/06/07/08/10 and BROWSER-IPC-B11 remain one packet, with CONTRACT-RC-02 and NATIVEUI-GAP-01/02/04/05 fixture extensions. Not all IDs are fixed by wheel work.
- Wheel DOM context: `p02-context-red.log` 167 pass/1 intended missing-context fail -> `p02-context-green.log` 168 pass. Same mounted viewport(100,50)/client(125,120) oracle sends local(25,70), all modifiers, exact session/rows. Current source preserves parent's unit/remainder/reset/clamp changes.
- Backend wheel/mouse: `p02-backend-red-input.log` 12 pass/7 fail -> `p02-backend-green-input.log` 16 pass/3 fail, both exit101. Four added production-seam cases genuinely become green: noncentral Ctrl cell, Shift viewport/sibling, nonwrapping rows, tracking buttons/hover/release. Full target **FAIL**, not GREEN.
- Debug: `p02-backend-red-debug.log` 4 pass/2 injected-root failures -> green 6 pass/exit0. Current diff routes all four former absolute `/tmp` writers through portable append/offloaded callback logging. Fifth existing Windows clipboard writer remains synchronous; do not silently count it repaired.
- Remaining local work: explicit P06 preferences isolation before any further input target; register typed SessionDetached/session-ID correction for the three old NoValue assertions; final-motion selection, independent Windows post-IME Space, foreground/dialog hook and target-shell drop roundtrips. Native: real WM_MOUSEWHEEL -> DOM -> IPC -> viewport/PTY, +/-120 over 200 lines, sibling isolation, tracking/Shift/alternate/unfocused/horizontal and 100/150% DPI, capture/clipboard/drop/focus matrix. NATIVEUI extension acceptance is carried from separate receipts, not established by this 168-test run alone.

### P06 - renderer, layout and fixture safety

- RF-01/02/03/04/06/08; GT-05/07/08 and Wayland part of CONTRACT-RC-02 remain open except bounded initial/DPI pixel reply.
- Actual same `--lib native_terminal::surface_host::tests::p06_ -- --nocapture` receipts: temporary roots `ferryx-p06-st_01a09a00-bcDcYC` RED exit101/0 pass2 fail; `Pit2Hb` first GREEN attempt **also exit101/0 pass2 fail**; `eYT8ul` final GREEN exit0/2 pass. All under `/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/`, with receipt.json and phase log. Intended failures are empty replies versus ESC[6;16;8t / ESC[6;32;16t. Final log SHA256 `e46c8ea5921c18a3bcdcec0c4611a220282569f5ef62b8877e7c0bf11118c516`.
- Current source really implements resize on initialized/dimension/metric change and registers Ghostty size callback; terminal.resize stores grid/cell state. Vendor terminal.h confirms option6 and callback output pointer. Source now spans bell.rs, lifecycle.rs, terminal.rs, constants.rs as well as surface_host.rs; not merely two test additions as stale report text says.
- Final receipt hashes match surface/font/lock, but **omit those newly changed callback files**, and report header omits terminal.rs. A hash manifest containing the same surface hash for failed and successful GREEN cannot prove all input bytes were frozen. Retain already-executed receipts durably and add complete provenance; do not rerun merely to manufacture better chronology.
- No flush/selected-face/metrics/cache/variant/stride or safe example/close/GPU-error fixture closure. preferences.rs unchanged; surface creation still calls cached_terminal_preferences. Optional RF-05 remains excluded absent parity authorization. Native known-font raster/GetTextFaceW/GDI flush, warm attach, DPI metrics and visible retained-vs-fresh cache behavior remain required.

### P08 - daemon admission, cwd, auth and local state

- DS-02 upgrade recursion has bounded R/G: actual internally constructed RPC now shares originating admission Arc. DS-01/05/07/09 and complete FSSH-02 integration remain open.
- P08 temporary roots `ferryx-p08-st_01a09a01-St76os` and `FxEHHJ`: actual RED totalExit101, two commands each 0 pass/1 intended fail; GREEN totalExit0, each 1 pass/0 fail. Logs show admission assertion and cwd empty-vs-src failure. Same exact registered test names; both modules are Unix-gated.
- Current client/server hashes match GREEN. Raw RED SHA256 `1a413ba6f51c7fb0a0a0f33f6a482836efb19ff380f5a085ab2cccc0c56c3841`; GREEN `afe6d6946c4a70cf361e44c747b41850f5c073ad414b52a45df198b2c4e27da4`. These supersede dag-batch-verification's older staging-only row and impl-p08's stale body.
- **Cwd fix is narrower than the registered platform contract:** spawn_remote already knows environment.platform and validates through resolve_remote_spawn_root -> validate_cwd_inside_root. New remote_spawn_relative_path discards that platform, guesses Windows from drive/UNC text, unconditionally replaces backslashes, and uses ASCII folding where upstream uses Unicode lowercase. Accepted Windows Unicode-case aliases can still silently become empty/None; legal POSIX backslash components can be changed. Prefix-neighbor traversal is *not* alleged as a new exploit: upstream containment rejects it. Register actual target-platform/Unicode/POSIX controls and fix this existing conversion once, rather than adding redundant downstream boundary checks. This is source-established missing coverage, not an executed new RED.
- No authenticated control/attach/proxy listener or Windows SID/DACL publication, local authenticated state-listener readiness, portable ConPTY fixtures or full stale-upgrade listener integration. Agree protocol/auth/provider contract with P09/P12/P19/P22/P25 before wire edits. Native second-account denial, actual extension state/provider frames and helper describe-cwd remain.

### P09 - agent detection/install and settings

- DS-04 and USERPROFILE-install part of DS-06: `p09-contract-red.log` 1 pass/2 intended fail -> green 3 pass. Real extracted Rust production functions, not full crate/native APIs. PATHEXT order/case/extensions and bundled install bytes retained.
- HIST-01: UI RED 1 pass/2 fail -> GREEN3 pass and fresh combined3 pass. Actual platform gate is minimal and leaves shell selector.
- **Acceptance FAIL on test preservation:** `git diff -- ui/src/components/settings/TerminalSection.test.tsx` removes all eight original behavior tests (scrollback commit/clamp, font-family buffering, empty/unparseable font-size handling, prop draft refresh, shell selection callback, custom-shell input and custom-shell prop). It replaces 143 lines with a 32-line platform-only suite. Report calls this a new test file; it is tracked and previously populated. No evidence licenses deleting those assertions. Forward-restore the eight tests alongside the three platform cases, preserving current changes and new gate. Expected combined discovery at least11 cases, not3. Do not overwrite the file wholesale from HEAD.
- DS-03 shim interpreter quoting, authoritative provider metadata/discovery and remainder DS-06 are not implemented. Native .cmd/.bat argv recorder through actual ConPTY with spaces/metacharacters/quotes must produce exact sentinel before/after shell repair; native detection and USERPROFILE installation plus real settings remain. P02 needs explicit target-shell metadata, not host-OS inference.

### P10 - SSH/config/worktree

- FSSH-01/03/04/T01, CONTRACT-RC-01/05: **staged only; no executed R/G**. Current diff adds config quoting/import->ssh_plan assertions, native Add-Type argv recorder, pure namespace negatives/positive control; no parser/bridge/manager repair.
- Queue only config p10_ prefix plus exact pure positive namespace test under lead slot; native exact argv and invalid namespace tests require provisioned PowerShell/Windows first. No full worktree target under no-ref-creation restriction; no broad bridge filter with live loopback SSH.
- Remaining: semantic IdentityFile grammar, PowerShell5.1 CRT quoting/trailing slash, target-platform namespace validation before filesystem/Git effects, native absolute key/helper provenance fixtures, request-received cancellation barrier, owned missing helper/zero transport and structured failure sentinel. Native real SSH/helper/cwd and Unicode Git/no-orphan behavior remain separately blocked by runtime/ref allocation.

### P12 - browser authorization

- BROWSER-IPC-B01: **staged only**. Current browser_cli.rs diff is91 test lines; production handle_connection still accepts unauthenticated requests. Actual owned TCP tests have not executed. Require list/snapshot/act and forged-token intended unauthorized RED, then independently scoped capability/authenticated success/rotation/size-bound GREEN.
- Auth API must coordinate with P08/P23; common publication implementation must not share daemon/browser authorization scope. Windows SID/DACL and second-account denial before endpoint advertisement remain. Native actual authorized CLI must list/snapshot/click owned browser; P13 action policy remains distinct. No bare-list pass is authorization evidence.

### P13 - browser OS and engine

- BROWSER-IPC-B02/10: bare-home RED1 fail/1 pass; opener RED1 fail/2 pass -> same resolver/request assertions GREEN3. B05 capability RED1 -> GREEN1 typed Unsupported; current branch checks generation before unsupported. Additional ShellExecute error mapping GREEN1 is **GREEN-only**, not earned RED. B03 manager same-URL generation invalidation RED1 -> GREEN1, not native history completion.
- Raw logs p13-home-red, opener-red-home-green, key-red, history-red and p13-safe-runner-green verify those counts. Production uses shell-free ShellExecuteW, OsStr file path and blocking wrapper; native FFI is not compiled by extracted probes. UI14 pass locally and fresh combined; fixture beforeEach-return failure was not product RED. Metadata harness exit1 is not crate build.
- **B03 still unimplemented:** current Cargo.toml only adds P15 windows-sys features; direct webview2-com dependency/callback integration and engine-native back/forward absent. Serialize manifest edit with P15 owner and preserve its features. Native pushState without load/title, state flags, failed/no-op loading settlement, exact opener query/path/no second command, HOME-absent profile paths and typed CLI keypress remain.

### P17 - MSIX resources

- PKG-03: source layout/hash checks implemented, Node8 source contracts pass (fresh too), native behavioral RED/GREEN unrun. Staged16-case MakeAppx fixture must run against preserved original then repaired script; original must reach missing packaged resource, not missing tool. This cannot retroactively satisfy observed-before-edit chronology.
- **Registration claim is false:** `p17/registration.json` top-level ok=true wraps `result.accepted=false`, `rejectedReasons=["weakened completion"]`. Root ledger09:09:47.687Z records steering_rejected; inspected current C002 has no P17/MSIX/MakeAppx/test-build-msix appended scenario. General packet snapshot incorporation remains, but the claimed accepted additional registration does not exist. Parent must append the full existing criterion plus exact final16-case/original-script command without weakening completion, retain rejection and correct report. Do not label receipt accepted based on transport ok.
- Native parser/16-case archive paths/lengths/hashes/cleanup and real application outside repo serving authenticated UI/resolving helpers remain. Fixture PE is not a runnable app. No signing/install permission implied.

### P18 - build/CI/fixture portability

- COV-TEST-1 (PKG-02 missing source), COV-BLD-1, HIST-02: `p18/prerequisites-red.log`4 pass/3 intended failures -> green7. Missing ignored include is the specified clean-layout compile defect, not incidental build failure. Actual build.rs control-flow probe distinguishes MSVC/GNU/default; stat formatting uses controlled filesystem metadata. Node edge fixture genuinely stages/executes/cleans an owned driver, not daemon protocol integration.
- COV-CI-1: Windows nonzero-contract policy case RED->GREEN but full workflow suite remains16/17 failure, freshly confirmed. Current YAML actually invokes two contract targets on MSVC; it is not hosted execution evidence and does not enable unsafe lib/persistence suites.
- Cargo target build/run, native MSVC load without TaskDialogIndirect and actual CI execution remain. GNU emitted-flag GREEN is not GNU link/runtime (unprovisioned). Keep Windows opacity+edge tests as narrow safe prerequisites, not domain coverage.

### P19 - persistence ownership

- DS-10: source fixture isolation staged, **not R/G**. Shared connect seam still explicitly ignores expected_pid and accepts any HandshakeOk. Do not run full target while staged.
- Safe exact foreign-handshake test must produce one intended fail before guard installation, then same GREEN; version and owned child PID check must protect control and attach. Then11 Unix cases including two owned daemons/panic/sibling Pong, sequence and replay, current Cargo binary only.
- Additional cleanup review: Drop logs kill/reap errors but always prints "reaped" and then TempDir deletes root; child status/reap failure cannot substantiate that receipt. Readiness reader is spawned without retained/joined thread handle. Before full fixture execution, make failure receipt truthful, retain evidence if child termination unproved, and join readiness work. Do not call printed cleanup success proof. Native counterpart is missing, not a cfg-removal task; use owned native endpoints/ConPTY and preserve sibling/user daemon.

### P23 - relay enrollment/integration

- GB-01/03/04: **staged only**, production transaction lock absent. Exact cross-process lock-probe parent requires child WouldBlock at reload/publication, restart retains both keys and conflicts rejected. No Cargo execution; runner refusal64 is not RED.
- Probe uses synchronous `.output()` without its own child deadline; lead runner's outer deadline alone does not prove child reap. Bound/join owned probe on failure and retain cleanup receipts before broader integration. Portable tx.lock guard must span read->publish; keep low-level writer lock distinct and propagate errors.
- GB-03 native /bin/sh failure and GB-04 admission/clock/lease/revocation/all-frame zero payload/computed-output/cleanup validators remain wholly unfinished. Native two relay binaries/ports sharing only owned store and real gateway/ConPTY execution remain; lock seam is not binary enrollment proof.

### P24 - watcher

- GB-02/05: **staged only**; polling remains conditional on initial arm failure and scans/arming remain on async worker. New tests observe exact armed/scan/loss events and blocked-scan sentinel, but have no executed intended RED.
- Recovery test awaits abort only after its outcome block; setup/assertion panic can bypass it. Complete unconditional cancellation/await for new and existing tasks. Then offload actual scan/arming and make periodic reconciliation authoritative after silent watch loss, same tests GREEN. No recursive project watch.
- Native owned NTFS DAG deletion/recreation must resume Running/Completed visible updates without restart; slow scan cannot block async sentinel. This is separate from foreign worktree rescan/PX.

### P25 - helper staging and survival

- GT-01/02/03/04: bounded partial R/G, not functioning full harness acceptance. Raw initial import/PID/fragment tests8 fail; stager10 fail; incremental detached/identity/helper/cleanup/CRLF RED logs fail their added assertions. `p25-logs/final-green.log` **27 pass,0 fail,0 skipped**, independently27 pass in fresh combined Node invocation. Same source-boundary assertions retained; VM mocks cannot prove framed/socket/native survival.
- Source deliberately closes detached setup/bridge entry points with QA_OWNERSHIP_PREREQUISITE before effects. This is a safety interlock, not repaired detached startup. Direct helper/process/resume now verify identities but real transport EOF/late-reply/timeout poisoning, positive ownership and failure cleanup matrices remain.
- Stager rejects stale/fallback assets, verifies explicit source/lock/artifact hashes and machine bytes, privately stages then renames. Current success test proves only one Windows-target fixture; complete three-target publication, independent source/compiler closure and injected copy failure remain. PE machine is not MSVC provenance/loadability. Existing destination rejection and receipt-only CLI are intentional new prerequisites; consumers must supply an owned new destination, not delete shared resources to bypass them.
- PowerShell wrappers remain unchanged with unsafe caller-archive deletion/target-dir/artifact-path/zero-test behavior. Native wrapper original RED then same GREEN is outstanding, including apostrophe quoting. Direct process-survival main still builds its example before an owned fixture and requires a separately coordinated Cargo slot/explicit artifact policy; never run it as safe self-test. Helper protocol1 remains distinct from daemon3.

### P26 - capabilities/contracts

- HIST-03/GT-06: injected actual permission policy Windows8 pass2 fail and Linux8 pass2 fail -> same10 pass each, final-green confirms20 passing total with zero ignored and cleanup. Request=false and canRequest=false off Mac are real production fixes. Launcher seam tests target/argv/result without opening Settings; removed assertions were prose-only, unlike P09's behavioral deletions.
- Pure rustc harness rewrites cfg/serde annotations: not whole crate, native ABI, registered IPC or provider status proof. Native expected4 integration+6 unit cases, unsupported/granted=false/canRequest=false/canOpenSettings=false and request=false through registered IPC remain. macOS Cargo3 integration+6 units needs lead slot; no OS Settings action as test prerequisite.

### P29 - tooling paths

- TOOLING-GAP-03/04: signing RED3 pass1 fail (duplicate debug) -> GREEN4; SEO RED22 pass1 fail (backslash route) -> GREEN24/1020 assertions. Raw p29 logs contain EXIT=1/0; real two Astro builds and owned removed-required-page check, fake-signing exit0/7 retained. Same projection assertions preserved; native execution still pending.
- Windows Bash namespace/interception and actual native path walker need same original-expression RED and repaired GREEN with existing tools, no installs. Missing Bash/dependencies is blocked, not RED. Original-root fixture reaping/removal and two fresh builds must be retained. Do not rerun site builds merely because the producer completed.

### P30 - RGB expansion

- IMAGES-GAP-01: actual Ghostty/WGPU regression staged; **no RED/GREEN or repair**. Accepted4097x4097 RGB=50,356,227 stored bytes would expand beyond64MiB; images.rs remains unchanged and aborts snapshot on expansion error.
- Require exact protocol OK first, then intended text-snapshot failure; GPU/compiler/protocol setup failure is not RED. After observed RED, omit only overbudget expansion/remove cached entry without allocation, preserve malformed ABI errors/cap/vendor and supported image. Same two successive text+supported-red-image GPU readbacks GREEN.
- Native current debug pane must visibly retain red image/text while blue oversize placement stays absent, with presentation signal/readback and own-pane cleanup; offscreen GREEN alone cannot certify HWND.

## Actual conflicts, missing coverage and registration priority

1. **P09 deleted eight tests** is a new batch regression, not a pre-existing failure. First repair allocation: TerminalSection.test.tsx only, forward preservation of both suites. Narrow3pass must not be used as regression clearance.
2. **P17 accepted-registration assertion is disproved** by raw nested response and ledger. Registration/chronology repair is evidence work before additional production work, not a request for more broad auditing.
3. **P08 cwd contract mismatch** survives the five green examples: preserve explicit target platform and upstream path semantics; add Unicode/POSIX controls before repair. Do not claim complete FSSH-02.
4. **P06 callback source closure/ownership must be reconciled:** terminal.rs is changed beyond terse report list; constants.rs also contains P03 backslash/bracket additions. Both sets remain present. No overwrite/conflict marker found, but do not assign a second writer or omit files from transfer/hash/diagnostics. P06 preference seam is a real source dependency of P02/Wayland; Cargo contention is not.
5. **P19/P24/P23 cleanup gaps** above are concrete fixture acceptance gaps; complete before broad integration, not after a panic leaves resources.
6. **P13 manifest dependency vs P15 ownership:** current manifest has only P15's three feature additions, no direct webview2-com. Serialized additive edit is required, not rollback or hand-written COM bypass.
7. **P08/P09/P12/P19/P22/P25 transport metadata/auth** must be published once with caller matrix. Current packet changes did not alter protocol declarations, so no demonstrated wire compile conflict is alleged. Authentication remains unimplemented, not an undocumented wait reason.
8. **Packaging chain P25 -> P17/P18/P10:** matching manifest keys/protocol1 do not provide compiler provenance, full source closure or native import/load proof. Rejected/unsafe wrappers cannot be used to create those receipts.

Accepted registration JSONs were inspected for P06/P09/P10/P18/P23/P24/P25/P26/P29/P30 and current C002 confirms P02/P08/P12/P13/P19 additions. P13's final keypress test name is behavior-registered but not literally enumerated in its original addition; reconcile exact command list before queued execution. P17 remains the actual rejected supplemental registration, not a blanket claim that all registration is missing. No official goal state was changed here.

## Separate DAG/P15/runtime receipts integrated, not relaunched

- `dag-batch-verification.md` remains the PR/runtime/aggregate adjudication. Its P08/P25/P30 rows are superseded here by the actual later implementation receipts; its full-UI failure remains:219 files,217 pass2 fail;2449 tests,2445 pass4 fail, exit1. SettingsDialog stale prose and three push-client stub expectations are separately owned, not fixed by this report. Its build completed historically, before later UI edits; not current combined build proof.
- `dag-p15-review.md`: accept supplemental source/local R/G adjudication only. Actual primary RED0pass1fail101 -> GREEN1pass0, companion invariant GREEN1 in both phases. Current Cargo.toml/lock hashes match that review. Windows adapter four ABI fixtures, actual GetAdaptersAddresses/full resolver and authenticated offline-LAN peer remain; no rerun or route/firewall manipulation.
- Native runtime `runtime/dag-input-compile.receipt.json` explicitly has exit0/DAG_INPUT_COMPILE_OK: layout compilation only, not input dispatch or GUI acceptance. Existing DAG reports inspected no eligible owned checkout in their bounded locations. Creation approval remains unresolved; installed/shared processes are not an allocation. Do not relaunch runtime DAG or wait for an undocumented host dependency.
- PR2 native constructor/hit test and PR3 real keyboard/native-menu close with SAME sibling backend/PID/start identity and fresh output remain external runtime obligations. P14 toast identity/audio remains separate; P15 LAN remains separate; PX remains foreign-owned.

## Precise next repair/execution batch

Keep existing owners, one writer per file and one lead-monitored shared Cargo queue. Resume these disjoint lanes, not sixteen new verify-only tasks:

1. **UI preservation:** P09 restore eight behavioral tests forward, retain three platform cases; register reconciliation and require all11 once. Separately resolve SettingsDialog prose-only oracle and push prototype/spec disposition without inventing unrelated product features. Then freeze source and run one full UI suite/build, retain all failures and source manifest.
2. **Input/render:** P06 explicit owned preferences fixture first; P02 typed detached-error fixtures and final-motion/IME/focus/drop coverage; P30 exact safe RGB RED then minimal images repair; P06 font/GDI/cache/example/close work. Coordinate P03 constants and callback files. P01 native hit test can proceed when native allocation exists, not after every portable repair.
3. **Daemon/SSH:** P08 platform-preserving cwd increment and auth/provider agreement; P09 shim/provider implementation; P10 config/native argv/namespace oracles; P19 guard RED/GREEN then cleanup-complete11-case owned suite. Keep unowned bridge/worktree/SSH filters forbidden. P22/P25 consume published auth without competing writers.
4. **Browser:** P12 exact owned socket RED then independently scoped auth; P13 serialized webview2-com/native callbacks, preserve P05 focused targeting and unsupported-input fallback until trusted input evidence exists.
5. **State:** P23 cross-process lock and deterministic integration validator repair; P24 watcher offload/reconciliation and unconditional task cleanup. Independent source lanes sharing Cargo scheduling only.
6. **Tooling/contracts:** correct P17 rejected registration/report first; owner-controlled scratch native16-case MakeAppx original/repaired; P25 wrapper archive/provenance/transport/ownership repairs; P18 safe Cargo edge+opacity and missing-Pages expectation disposition; P26 Cargo/native IPC; P29 native fake-tool/SEO receipts. No packaging script execution implies installation permission.

For the queued Rust work, lead must explicitly assign its already-provisioned target/cache, child-only owned profile/runtime/temp and exact filtered commands from packet runners; no ambient unwrapped broad `--lib`, cache farms or self-granted guard flags. Require nonzero discovery/intended RED before repair, identical GREEN, changed-file diagnostics, affected runnable entry and combined build only after staged intentional failures are resolved. Raw compiler failure and cancelled diagnostics remain prerequisites, not passing.

Shared Windows acceptance belongs only to st_01a099f8: eligible authorized owned allocation, complete immutable dirty-source/test/lock/generated-input/Ghostty transfer manifest, native compile/load, interactive debug **bun tauri dev** only. Subscribe exact DOM/backend/PTY/state/presentation signals before input; retain physical wheel/selection, shell/key/IME/drop, fonts/DPI, browser history/auth, gateway/watch, permissions, packaging and sibling-survival obligations above. Record actual binary/source/target/tool identities and positively owned cleanup, preserve installed user daemon identities. Scratch packaging/wrapper fixtures may use their smaller separately authorized boundary; GUI checks still need actual app.

## Provenance and verification limits

Selected current SHA256 values checked during this review:

| Source | SHA256 |
|---|---|
| TerminalSection.test.tsx | `9143c23d7807f4bcb7c32ea7f848e2f450f5012b1f608b881edd2977fec97232` |
| daemon/client.rs | `4f0070be20c96d70e5a044a7f703b0de640666e262cf69d7e18b4eb244a01c99` |
| daemon/server.rs | `b2c16c52e9ff5e7ff06453efe83a8af5ff0d1e7581d37e9d8a3a330ef087ced8` |
| native_terminal/surface_host.rs | `133637d3d2d31b0150d17f4bf18a3e644e8adb1cff7a774e88fa626a84e10043` |
| native_terminal/bell.rs | `3f0b88ceabe62bd016a9e9f37dcae0e0de208512b59566ae62fdcdc1615549c6` |
| native_terminal/lifecycle.rs | `a280c6a55f3408669064302f153f7530dff42ba52e27169833b9ca1db0c77d21` |
| native_terminal/terminal.rs | `bc433391893130c183a6615a956f99d5b813c9cbe8322eb41d1929f9e2a6d0bc` |
| native_terminal/sys/constants.rs | `8a810fa8d3046f98a867f00696b8e99d9393b079dd0562113539f5cdb316c8b8` |
| Cargo.toml | `1fdf8ec46e62043b6d75a541bc60682e60f701dcdfe6551a8c03a4492ec6efdc` |
| ignored Cargo.lock | `1b2d01f2d268c2183c5be10cb1d0db464c349afffe9999b79cb93077f11169a5` |

Fresh command output is retained in this child session's tool transcript; this report records exact commands/exits/counts rather than claiming standalone log files were created. Prior packet log paths and temporary receipt paths above were actually read. Hashes bind only named files at inspection, not a whole-tree freeze or historical binary reconstruction. Report-only diagnostics do not substitute for production diagnostics/builds. Temporary fixture cleanup from producer receipts is not a new independent process-absence census; this verifier launched only short-lived safe Node/UI tests, all returned.

Original G001 remains blocked with C001/C002/C003 pending. Final native/full-build/review/PR disposition/verified-push gates are **not complete**. This uncommitted report and all foreign changes remain vulnerable to concurrent drift; preserve them and reconcile forward.
