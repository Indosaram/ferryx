# DAG Windows runtime coordinator receipt

Task st_01a099f8; original session 01a0983f-c995-753d-afa9-593f6d118788.
Preparation delivered; **current-source Windows acceptance NOT EXECUTED**.

**External blocker: no eligible existing owned Windows checkout was found; creation approval is required for C:\Users\sook\ferryx-qa-dag-st01a099f8\orca-lite on new branch qa/windows-review-st01a099f8.**

Authorization correction: reversible synchronization into a proven existing
QA-owned isolated checkout is permitted without separate approval. Only branch/
worktree creation/deletion remains reserved; main must not switch and foreign
files must not be overwritten. The earlier separate synchronization-approval
wording was overly restrictive and is superseded by this correction.

## Concurrent source freeze and execution priority

Lead started implementation DAG dag_536f9a7e-9459-408f-b07f-5e524f8ea10b for
P02/06/08/09/10/12/13/17/18/19/23/24/25/26/29/30. Those workers stage native
cases only; st_01a099f8 retains exclusive maho-win mutation ownership. Before
ANY sync, agree a frozen file/hash manifest with lead including dirty source,
test fixtures, dependency manifests/locks, generated inputs and submodule SHA.
Create transfer bytes from that snapshot, not a live directory while writers
continue; verify transferred hashes before build and retain the approved manifest
with each binary receipt. Subsequent packet changes require a new explicit
snapshot; never silently HMR a moving aggregate during acceptance.

Priority remains PR2 same-assertion native RED/GREEN, then owned debug startup
and actual pointer/wheel/close acceptance; base preparation does not await all
sixteen workers. PR2's ledger annotation does NOT register its new command in
C002: direct criterion inspection found the exact test name absent. Lead must
register the exact command/oracle from dag-pr2.md before staging the regression.
No PR2 repair has been applied by this coordinator.

Lead reports P15 independent local review PASS; native fixtures/API/peer remain
open. Cargo.lock is ignored/absent from HEAD: prove its frozen-byte stability by
hash receipts, not an empty git diff. This is a handoff constraint, not an
independently repeated local review or native test result.

## Fresh execution, not repeated historical acceptance

Read windows-preflight-current.md and prior runtime pinned commands/FRESH-RUN,
launch isolation and native input recipes. Their cleanup explains why both
historical owned roots are now absent. Do not reuse their PIDs/tasks/screenshots.

`runtime/dag-preflight.receipt.json` stores exact SSH argv including the complete
UTF-16LE encoded `runtime/dag-preflight.ps1`; stdout and stderr are separate.
One bounded SSH execution exited 0 at **2026-09-13T08:53:00.3533413Z**:

- DESKTOP-1LAPJMP, sook; SSH is Session0, console sook Session1 Active.
  This is session readiness only, never GUI success.
- Installed GUI17288, daemon1756, daemon20196 retain exact executable path,
  parents and creation milliseconds1789211998895/1789212001837/1789212002037
  from prior preflight. All are Session1. No installed process was touched.
- C: free186810707968 of999127248896 bytes.
- Fail-closed listener enumeration succeeded: queried5173/1420/9223/9224
  absent;127.0.0.1:53986 belongs to installed daemon20196. No port reservation.
- Historical ferryx-qa-fresh-0912 and ferryx-qa-rt-st01a0958a checkouts absent;
  matching historical QA scheduled tasks absent. Git/Bun/Cargo/Rustc/Zig/Node
  executable paths resolve; version compatibility/build tools not certified.

One result-dependent read-only provenance query exited0 at08:55:16Z, exact
PowerShell/argv/output in `runtime/dag-existing-checkout.receipt.json`:
shared `C:\Users\sook\ferryx-winbuild\orca-lite` is e2a19066fe36f126d62ffecc952f3dc0b5f3258a,
with six dirty tracked entries (listed in receipt). Its Ghostty source is
6a508fd5e34c7e222c052a6d00bb3891ff3feace. It is NOT an owned current-source
allocation; no modification/build there is authorized. Home also contains
`ferryx-qa`, initially of unknown ownership. The authorization clarification
prompted one bounded eligibility inspection, not another process audit:
`runtime/dag-remaining-root.receipt.json`, SSH exit0 at08:59:43Z, lists only
linux-task.sh and two task-4-daemon log files. No subdirectory or Git checkout
exists there (neither root nor orca-lite has .git). It cannot supply an existing
eligible checkout. Those historical files were not read, changed or acquired.
Local HEAD is da6eec06d65551f67bbc43f09910cde470c3478d with foreign dirty fixes;
HEAD alone cannot identify the eventual combined source snapshot.

## Owned base harness ready for approved allocation

Only new evidence/harness files under this evidence directory were authored
with apply_patch. No source synchronization, branch/worktree/ref operation,
remote file creation, scheduled task, build, GUI launch/input, daemon connection,
commit/push/PR write occurred. No remote resource was acquired for cleanup.
P15 source/Cargo and P14 notification evidence were untouched.

Allocated future root: `C:\Users\sook\ferryx-qa-dag-st01a099f8`.
Future CDP9224, frontend5173/1420 require fresh owner checks before launch.
`runtime/dag-launch.cmd` is a gated recipe, not executed: root command is exactly
`bun tauri dev`, debug only. It requires `approved-source.json`, repo and evidence
directory; this marker is a coordinator receipt, not self-validating approval.
Coordinator must first validate approval and full source-file hashes, dependency
locks and Ghostty SHA, provision owned runtime/session/appdata/localappdata/webview2
directories and dependency caches, then use an InteractiveToken task for sook.
Never run the launcher through SSH Session0. Record debug binary hash and actual
GUI PID/creation time/HWND plus WebView child profile after real frontend readiness.
Daemon/session overrides follow current server.rs:44,142; APPDATA/LOCALAPPDATA and
WebView overrides keep settings/profile state in the allocation. Actual endpoint
and profile ownership must still be observed, not inferred from environment text.

`runtime/dag-input.ps1` supplies PID+creation+debug-path+session+foreground guarded
native SendInput wheel/key/drag and owned HWND capture. It never focuses an ambient
window. Caller coordinates are physical pixels; capture includes GetDpiForWindow.
It refuses Session0 and points outside the QA GUI process. Example interfaces
(variables must come from actual observed allocation, not historical constants):

```powershell
& ./docs/evidence/windows-review-20260913/runtime/dag-input.ps1 -GuiPid $ownedAppPid -CreatedUnixMs $ownedCreatedUnixMs -Hwnd $ownedHwnd -Action wheel -X $terminalX -Y $terminalY -Delta 120 -Receipt "$ownedEvidenceDirectory\wheel-up.json"
& ./docs/evidence/windows-review-20260913/runtime/dag-input.ps1 -GuiPid $ownedAppPid -CreatedUnixMs $ownedCreatedUnixMs -Hwnd $ownedHwnd -Action wheel -X $terminalX -Y $terminalY -Delta -120 -Receipt "$ownedEvidenceDirectory\wheel-down.json"
```

These only prove dispatch when executed. Subscribe exact render/PTY/selection
signals BEFORE triggering, then use Action capture with a fresh Receipt path
after completion. Immediate screenshot is NOT a render-completion assertion.
No sleeps/polling or historical focus-unlock helpers are reused.

`runtime/dag-mouse-fixture.mjs` runs only INSIDE the owned visible Windows PTY:
`node <owned-script-path> <owned-evidence-path>`. It subscribes stdin before
emitting DAG_MOUSE_READY, enables alternate1049+mouse1000+SGR1006, records exact
hex input and cwd/PID, resets modes on q/Ctrl+C/normal exit. Use an external
pre-subscribed readiness/output observer with bounded deadline; no timeout here
pretends to be input completion. Unexpected process death cannot guarantee reset.

## PR2/PR3 shared interfaces and complete remaining matrix

Read `dag-pr2.md` and `dag-pr3.md` when they arrived; no waiting blocked base work.
PR2 exact native HWND regression command and same old/new style oracle are in
dag-pr2.md. Its actual native drag observer is not yet supplied. Base Action drag
takes X/Y/EndX/EndY; it dispatches native button down, cursor move, button up.
Runtime owner must add the actual pre-subscribed DOM pointer/backend selection
observer and retain exact selected sentinel and sibling state before calling
this PR2 acceptance. Release-outside/cancel needs its own registered action;
the guard intentionally does not inject input into unowned windows.

PR3 shared operator interface is:

```powershell
& ./docs/evidence/windows-review-20260913/dag-pr3/close-matrix.ps1 -AppProcessId $ownedAppPid -DebugExecutable $ownedDebugExecutable -OutputDirectory $ownedEvidenceDirectory -OwnedInteractiveAllocation
```

Run only in verified interactive allocation, not Session0. Sixteen cases cover
kind/pin/route/selected leaf. Base Action key -VirtualKey87 -Control can deliver
Ctrl+W; native menu case still requires real File/Close Tab click. Matrix output
only asserts process sentinels; independently inspect layout/backend exit and
same sibling PID/start/backend plus fresh output. Busy-agent cancel/confirm,
busy sibling, unsplit pinned/unpinned, browser and remote controls remain required.
PR3 script belongs to its author and was not changed or executed here.

Additional actual-surface obligations, all unexecuted:

1. In PowerShell PTY: `1..200 | ForEach-Object { 'DAG_LINE_{0:D3}' -f $_ }`.
   Capture bottom then SendInput+120/-120 over terminal content; visible numbered
   range moves backward/forward, sibling range/session unchanged. Same old/new
   observable required; no dispatch-only or parser-only PASS.
2. Alternate fixture receives exact SGR wheel sequences at observed terminal
   cell/modifiers (up64/down65), while ordinary viewport/sibling remain correct;
   Shift tracking/normal screen restoration separately observed.
3. Native drag selected sentinel plus release/cancel/reentry, keyboard sentinel
   and Ctrl-backslash/right-bracket byte28/29 with adjacent key controls.
4. Actual + menu -> Command Prompt -> `echo FERRYX_WIN_CMD_OK`, separate PTY output
   and cwd, not echo text merely appearing in the input line.
5. Subscribe resize event, resize owned HWND, capture geometry and actual DPI;
   another monitor/DPI transition is required to claim DPI behavior, not resizing
   at one DPI. Do not change system scaling globally for a fixture.
6. Focused browser/native split: address/find/reload/back/forward target only
   selected browser; browser sibling and terminal retain state/input ownership.
7. Actual ConPTY cwd via `bun script/qa/win-daemon-e2e.mjs --port-file "$ownedRoot\runtime\daemon.port" "$ownedRoot\orca-lite"` only after verify port owner is the
   isolated debug daemon. NEVER omit port-file (default reaches installed daemon).
   This spawns a separate session and does not prove GUI sibling survival/reconnect.
   Main's owned workspace cleanup needs lead qualification before execution.
8. Close only owned GUI/PTY/task tree with exact process identity; retain installed
   identities and listener53986 afterward. Historical cleanup scripts are forbidden.

## P15 native fixture execution handoff

Read current state.rs:190-474, including every concrete Windows adapter fixture.
All four exact names below are already present in C002 of the original-session
goals.json (confirmed after the lead's successful official registration). No
duplicate registration or criterion mutation was needed. P15 source was not edited.

Run each separately in the approved existing or newly authorized owned Windows
checkout, through the lead's bounded execution monitor, with its isolated Cargo
target/cache and preserved source manifest:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib remote::state::windows_adapters::tests::native_inventory_walk_filters_records_and_preserves_cgnat -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib remote::state::windows_adapters::tests::native_buffer_overflow_retries_with_aligned_larger_storage -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib remote::state::windows_adapters::tests::native_buffer_overflow_is_bounded -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib remote::state::windows_adapters::tests::native_api_no_data_is_empty_and_failure_is_not_success -- --exact --nocapture
```

Each must discover exactly one test and exit0; no broad filter is authorized.
Safety read: closures passed to production enumerate_with populate only its
owned aligned buffer or return Win32 status codes. Inventory links remain in
that allocation; fixture filters down/loopback adapters, invalid family/length/
pointer/DAD records and retains CGNAT until actual selection rejects it. Overflow
fixture expands to32KiB and requires two calls; bounded failure requires three;
no-data yields empty while status5 yields error. None of these four closures
calls GetAdaptersAddresses, sockets, daemon, settings or audio. Cargo compilation
still writes build outputs and must not use the foreign historical checkout.

Native compile/link/load and these ABI fixtures are unexecuted here. Actual
GetAdaptersAddresses production enumeration and authenticated offline-LAN peer
reachability remain DISTINCT additional acceptance; Darwin primary/companion
GREEN and the released local Cargo target do not satisfy them. The absence of an
eligible Windows checkout remains the execution dependency, not registration.

## P14 notification readiness and identity dependency

Read P14's concrete `p14-native/submit.js`, `observe.ps1` parameter/native XML
boundary and `p14-notification-sound.md` invocation sequence. P14 exclusively
owns those files and notification source; this coordinator changed none of them.
The four cases are targeted-system, targeted-silent, idless-system, idless-silent.
submit.js installs an observer in the actual Ferryx DevTools and submits only on
explicit p14.submit calls through real cmd_notification_dispatch IPC. observe.ps1
reads Toast History Content.GetXml for the exact run/case marker. Missing or
duplicate history is INCONCLUSIVE, not behavioral RED. Observe the unique toast
in Action Center before the one-shot XML query; do not click/dismiss it first.

Required allocation input is `$ownedAumid`, proven exclusive to this debug QA
allocation and actually used by BOTH builders, plus same interactive account,
owned existing workspace/frontend session target and evidence directory. Never
use installed Ferryx identity or the shared PowerShell fallback. Provisioning
an AUMID/registration or permission to emit four toasts is not implied by source
checkout approval; no such identity has been allocated or modified here.

Exact observer interface, with allocation-derived variables:

```powershell
powershell.exe -NoProfile -File docs/evidence/windows-review-20260913/p14-native/observe.ps1 -AppUserModelId $ownedAumid -RunId $runId -Case targeted-system -OutputDirectory $ownedEvidenceDirectory -OwnedInteractiveAllocation
```

Repeat each remaining case once with matching p14.submit case (omit target for
idless). First capture actual unchanged System XML with silent=true and observer
soundPass=false/exit1 BEFORE either builder repair. Capture both System routes
and both Silent controls independently; use identical cases for GREEN afterward.
For targeted toasts arm p14.armClick BEFORE physical click, await exact real
activation drain, verify subsequent empty drain and actual selected target UI.
Save XML/JSON, p14.receipts and screenshots, then dispose the observer. XML proves
configured policy, not audible playback. Audible System/Silent checks remain
separate and must respect explicit notification authorization.

There is a concrete launch compatibility issue for the lead/P14 owner: P14's
report currently requires running outside target/debug or target/release so both
builders use the configured identity. This assignment permits desktop launch
ONLY via bun tauri dev, debug only. Do not copy/direct-launch an executable to
satisfy that prerequisite. P14 must qualify an owned identity under the permitted
dev launch before notification execution; otherwise these cases stay blocked
independently of wheel/drag/close QA. No identity workaround or production fix
is authorized by this coordinator note.

P14 reports JS syntax-only verification; PowerShell/native XML, submission,
activation, audio and RED/GREEN have not executed. This readiness integration
adds no notification execution claim.

## Verification receipts and toolkit handoff

- Read-only preflight execution exit0 and provenance query exit0 as above.
- Native PowerShell compile-only execution of full dag-input.ps1 with CompileOnly
  exit0, `DAG_INPUT_COMPILE_OK`; actual x64 INPUT struct size40 checked. Exact argv
  and output in runtime/dag-input-compile.receipt.json. CLIXML module preparation
  progress retained, not suppressed product errors. No SendInput call occurred.
- `node --check .../runtime/dag-mouse-fixture.mjs` exit0; LSP no diagnostics.
- LSP for .ps1/.cmd unavailable, explicitly not clean. Preflight exercised actual
  PS parser/runtime; input only compiled. Launcher, mouse PTY and GUI behaviors
  are unverified. No build/tests claimed for a nonexistent eligible allocation.
- Official toolkit was initially not exposed; PR2 handoff supplied the installed
  executeAgentToolkit module. Imported it via Bun with resolveSessionId explicitly
  bound to the original session and resolveCwd to this repo. steer/annotate_ledger
  accepted=true, ok=true, exit0; runtime/dag-toolkit.receipt.json retains response.
  Registers wheel/drag/PR3 interfaces before any associated product fix. No direct
  goals/ledger edit and no criterion PASS. Parent long-run monitor remains unavailable
  to this child; build/GUI acceptance must use parent's monitor after approval.

Preparation is not runtime completion. The next eligible action requires approval
and monitored allocation, not another preservation audit of the same stale tree.
