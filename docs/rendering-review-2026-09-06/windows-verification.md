# D6 independent Windows teardown verification

Verifier: `st_01a07772`. Review date: 2026-09-06 (native receipts use +09:00,
where the executions occurred on 2026-09-07).

## Disposition

**PASS for the bounded D6 native HWND teardown repair contract. Interactive
Windows acceptance remains PENDING.** The retained Windows runs exercise the
actual private `WindowsCompositorTarget::drop`, reproduce the original surviving
child, and pass the same regression after the production change. This is not a
generic Win32 imitation, a zero-test success, or visible Session 0 GUI proof.

This verification independently inspected the current repair-worktree source,
its Windows-only diff, callers, tests, runner scripts, raw logs, and hashes. It
executed local read-only hash, source, patch-reconstruction, formatting-equivalence,
and log-consistency checks. It did **not** rerun Windows tests, contact or write
the remote runner, launch an application, or manipulate a desktop. Native run
results below are verified retained evidence, not newly executed native tests.
The only file authored by this verifier is this report; no production edits,
index operations, or commits were made. Other workers' UI/Linux/host diffs are
not attributed to the Windows repair or approved by this report.

Evidence root, relative to the main checkout:
`.omo/evidence/ulw/rendering-review-20260906/D6/` (abbreviated `E/` below).
Source references below are relative to
`/Users/indo/code/project/orca-lite-rendering-20260906/src-tauri/src/`.

## Criterion dispositions

| Criterion | Disposition | Checked evidence and boundary |
| --- | --- | --- |
| Actual production destructor on native Windows | PASS | `platform/windows.rs:459-624` creates the production registered child class with the real creation flags, constructs the actual private target, and calls `drop(target)`. Its subclass observer forwards to the real production window procedure. Native logs name the Windows `.exe`, source path, test, and Session 0; no mock destructor. The Tauri constructor and WebView are intentionally bypassed. |
| Right-reason RED before production replacement | PASS | RED production prefix equals immutable base. `red.log` runs exactly one test, reaches the worker barrier with a live child and no destruction event, then fails the surviving-HWND assertion, exit 101. `green-stage.ps1` requires this RED before copying GREEN; its receipt records old and new hashes after RED completed. |
| Same regression GREEN | PASS | The exact Cargo command is identical for RED/GREEN in `run.ps1`; the test modules are identical after rustfmt normalization. `green.log` runs one test, passes, exit 0, with one owner-thread destruction before the barrier. No changed assertion or success-by-filtering. |
| Deterministic async observation | PASS | `SetWindowSubclass` subscribes before destruction/drop. Worker posts `DROP_COMPLETE` only after actual Drop. Unfiltered owner message dispatch reaches that FIFO marker after the teardown post. `MsgWaitForMultipleObjectsEx` waits for queue events; the 30-second deadline only fails a stuck test. No fixed sleep or elapsed-time success condition. |
| Correct thread and exactly one destruction | PASS | `WM_NCDESTROY` observer records `GetCurrentThreadId`; pre-cleanup observations equal exactly `[owner]` in GREEN. Joined worker identity differs from owner. Exact identities are tabulated below. |
| Same-owner edge | PASS | Adjacent case `Owner`: actual Drop queues teardown, pumping reaches one owner destruction, child absent. This does not promise synchronous destruction at Drop return. |
| Parent already destroyed edge | PASS | Adjacent case destroys the parent on owner before worker Drop; observes exactly one owner destruction and no second event through the worker barrier. Covers that ordered edge, not arbitrary concurrent parent teardown/recreation. |
| Nonblocking transfer under host mutex | PASS, source + native seam | Production Drop no longer calls `ShowWindow`, `DestroyWindow`, or a synchronous dispatcher. It posts pointer-free `WM_DESTROY_CHILD`; only the child procedure calls `DestroyWindow`. The test proves worker Drop returns while owner pumps. Full Tauri lock/event-loop stress is not exercised. |
| Async lifetime | PASS within existing owner-pump contract | Queued message contains no Rust pointer, closure, Arc, or visibility reference. Production procedure uses only its HWND and static code. No target state must survive message dispatch. HWND reclamation is deferred until owner dispatch, or parent destruction; posting does not prove immediate removal. |
| Failure handling | PASS by inspection; failure injection not run | Failed `PostMessageW` is warned except error 1400, expected after parent destruction. Failed owner `DestroyWindow` is warned. Queue exhaustion is not silently called successful and has no synchronous retry. A stopped pump or failed post can defer removal until parent destruction; no unconditional eventual-destruction guarantee is claimed. |
| Surface-before-target order | PASS by production-source inspection; limited supporting test | `surface_host.rs:1873-1918` declares `surface` before `target` before `renderer`; no host Drop override reverses it. Constructor error unwinding also drops any created surface before the earlier target local. `related.log` passes the existing mirror-struct order test, not a real wgpu/Win32 destruction trace. |
| All relevant removal callers use repaired boundary | PASS by source tracing | Async detach/close IPC calls host removal directly; `detach_session`/`close_session` remove the host under `hosts`, and `teardown` clears the same map (`surface_host.rs:1516-1564`). The platform wrapper owns the actual Windows target. No independent HWND-destruction path was substituted at those callers. This is not a runtime shutdown/IPC integration test. |
| Hidden creation, transparency, reveal | PASS at native API + source level | Production class/creation/reveal code is unchanged except teardown handling. Fixture asserts initial `WS_VISIBLE` absent, native `WM_NCHITTEST == HTTRANSPARENT`, and visible style after actual `reveal()`. Host still calls reveal after `frame.present()` (`surface_host.rs:2009-2010`). Hidden-parent fixture does not prove pixels, real pointer routing through WebView2, or swapchain first-frame appearance. |
| Native compilation and runnable entry point | PASS from receipts | Exact GREEN 1 pass; adjacent suite 3 passes; related order test 1 pass; native default-feature `cargo check` exit 0. Cargo test compiled/linked and ran the affected native entry point. No release/package build or full application launch is claimed. |
| Source identity and isolation | PASS for identified D6 snapshot | All 60 entries in `artifacts.sha256` recomputed successfully. RED/final patches reconstruct their saved source snapshots in memory. Current Windows repair bytes equal native GREEN. Native checkout is base `b8f82d7` plus Windows only, not the entire concurrently modified repair worktree. |
| HWND/process/staging cleanup | PASS from retained receipts | All recorded cases finish child/parent live counts zero; RED cleans its survivor before asserting. Per-phase process surveys contain only transient command wrappers. Cleanup records no owned native processes, removes owned staging, and final survey reports zero remaining owned command processes and staging absent. |
| Interactive Windows desktop behavior | PENDING; blocks aggregate D6 pixel acceptance | No real Tauri tab/overlay/attach-detach run, live swapchain screenshot, or interactive event-loop/resource-count trace is supplied. Session 0 and hidden HWNDs cannot satisfy this item. |

## Exact native sequence and events

RED and GREEN both use this command from the isolated repository root:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::platform::windows::tests::child_is_destroyed_on_owner_thread_when_detached_by_worker -- --exact --nocapture
```

1. Initial build prerequisite failure ended at `00:15:09 +09:00`, exit 101:
   `tauri build failed: resource path '..\ui\dist' doesn't exist`.
   `E/red-build-prerequisite.log` contains no executed regression. It is **not**
   the accepted RED. The separately reported completion-monitor timeout is also
   not test evidence; `red-completion.log` is empty.
2. Hashed existing `ui/dist` prerequisite was extracted at `00:31:34`. Archive
   inspection confirms all 30 entries are confined to `ui/dist`. No resource
   launch or manifest workaround is needed to interpret the native test.
3. Accepted RED ran `00:31:35.4846001` to `00:32:04.9206740 +09:00`, one failed
   test, exit 101. Assertion: `production WindowsCompositorTarget::drop left
   child HWND alive after worker barrier`.
4. GREEN staging at `00:33:44.1740540 +09:00` records RED source hash before
   replacement and GREEN hash afterward, guarded by the RED exit/assertion.
5. Identical exact test ran `00:34:04.7193830` to `00:34:16.9030834 +09:00`, one
   pass, exit 0. The retained sequence contains one right-reason RED and one
   exact GREEN, not timing retries; `red-native.log` is a byte-identical duplicate
   of `red.log`, not an additional regression execution.

| Run/case | Parent HWND | Child HWND | Owner | Drop thread | Observation before fixture cleanup |
| --- | --- | --- | --- | --- | --- |
| RED worker | `0x5c40132` | `0x5ad700ca` | 16488 | 12048 | barrier true, child live, destruction `[]` |
| GREEN worker | `0x5c60132` | `0x5ad900ca` | 18256 | 1988 | barrier true, child absent, destruction `[18256]` |
| Adjacent owner | `0x10a0012e` | `0x5ada00ca` | 424 | 424 | barrier true, child absent, destruction `[424]` |
| Adjacent parent destroyed | `0xbbc0136` | `0x57a0120` | 2968 | 19008 | barrier true, child absent, destruction `[2968]` |
| Adjacent worker | `0x5c70132` | `0x552a00a8` | 4408 | 17604 | barrier true, child absent, destruction `[4408]` |

RED cleanup explicitly destroys surviving child with result 1, destroys parent
with result 1, then logs `child_live=0 parent_live=0 destruction_threads=[16488]`
**before** the failing assertion. GREEN/adjacent cleanup logs both live counts
zero without adding a second destruction. Observations are copied before cleanup,
so fixture cleanup cannot manufacture GREEN. The boxed observer lives through
owner-thread window cleanup; only that owner accesses its vector.

`adjacent.log` ends at `00:34:18.2003428`, 3 passed; `related.log` ends at
`00:34:19.5721484`, 1 passed; `check.log` ends at `00:36:14.7704008`, exit 0.
All five raw UTF-16 logs decode exactly to their UTF-8 `.txt` counterparts;
their separate exit files agree. RED/GREEN retain identical 19 warning headings
plus summary. Native check retains 8 warnings in unchanged files. PowerShell's
`NativeCommandError` formatting of Cargo stderr is not itself a Cargo failure;
the test results and captured native exit codes establish the dispositions.

## Lifetime and caller analysis

Creation is `cmd_native_terminal_set_bounds` main-thread dispatch -> host
`render` -> `NativeTerminalSurfaceHost::new` -> platform wrapper -> actual
Windows constructor. Detach and close use async Tauri command dispatch instead.
Read dependency sources corroborate `respond_async_serialized` in
`tauri-macros-2.6.3/src/command/wrapper.rs:378-392` and runtime spawning in
`tauri-2.11.5/src/ipc/mod.rs:371-388`. Thus host field order alone never fixed
the old cross-thread `DestroyWindow` call.

The only production `surface_target()` consumer found in this path is host
construction, forwarding the cloned handle through renderer/GPU context to
`wgpu::Instance::create_surface`. Normal host destruction releases its surface
before target Drop posts teardown; constructor failure after surface creation
unwinds that surface before target too. The posted procedure does not touch
the target's handle Arc or mutex. It can execute concurrently with the tail of
worker Drop: the source comment saying the Rust target has "already been
dropped" is stronger than the scheduling guarantee, but no code relies on that
claim. The necessary invariant is no Rust-state access, which is satisfied.

The repair removes immediate worker-side hiding as well as invalid destruction.
Consequently the child may remain mapped between the detach reply and owner
message dispatch. Native tests establish eventual removal through their queue
barrier, not visual absence at IPC return. Actual tab replacement/overlay timing
must therefore remain in interactive acceptance rather than be inferred here.

## Build identity and cleanup audit

Independently recomputed SHA256 identities:

```text
base/main Windows production:
de5d0da481f0c0cf4984661d2658275e7ba9fdb6b795285dcfd2bf2b3b46207e
RED source (unchanged production plus tests):
6b733478761e029d128c161f17848a1d06ccd8fa17643b435c13a512492d86dd
GREEN source/current Windows repair:
5c81a42082be0ac909b780df491ba21939b23d94522c0f4361387c0ea9f47ab2
native resolved Cargo.lock:
e9009087faa862b51beae7d377fd0aac51ae85fb2670a13ae7b7399f040b385b
source bundle:
947f8ac84ef9f65001a565c77ed84fdedb4c480bbf400069779900e622d2695a
UI build prerequisite archive:
abdccd4457fda5e9bde04a5257ed3536bd20d28276d7966d4614b08f084998c0
```

`git bundle list-heads` advertises `bce59b45d6d2d43f46cc63e35a5fb9b1161ca145`,
but `stage.ps1` explicitly checks out
`b8f82d707f0cb99907e3d79c0c9cdc75053ef931`. `stage-ssh.log` (not the incomplete
PowerShell transcript alone) confirms that checkout and only the Windows dirty
path. Cleanup confirms the same base and final source hash. The newer advertised
bundle HEAD must not be called the tested source base. The existing build.rs
already embeds Common-Controls v6 for test executables; base/current build.rs
and IPC file equality were independently checked. The native dependency lock
is retained as evidence, not presented as a repository dependency edit.

Runner receipts identify Microsoft Windows NT `10.0.26200.0`, native
`x86_64-pc-windows-msvc`, Cargo/rustc 1.97.0, Zig 0.16.0, 12 logical CPUs,
and Cargo jobs 8. Preflight records about 396 GB free and no matching native
build/application process. Owned staging was
`C:/Users/sook/ferryx-winbuild/d6-st_01a0773f-20260906`; source, target output,
and separately cloned pinned Ghostty
`6a508fd5e34c7e222c052a6d00bb3891ff3feace` were isolated there.

Foreign checkout HEAD remains `e2a19066fe36f126d62ffecc952f3dc0b5f3258a` with
the same recorded dirty paths before/after: `scripts/build-msix.ps1`,
`src-tauri/target`, `src-tauri/vendor/ghostty`,
`src-tauri/windows/msix/AppxManifest.xml`, `ui/dist`, and `ui/node_modules`.
These receipts establish unchanged HEAD/status, not a bytewise forensic hash
of every foreign dirty file. Scripts do not clean foreign caches or kill
processes. Cleanup at `00:41:42-00:41:47 +09:00` records
`OWNED_NATIVE_PROCESSES=0` and `OWNED_STAGING_EXISTS=False`; final survey records
`OWNED_REMAINING_COMMAND_PROCESSES=0` and staging absent. Local artifacts remain.

Local verification also passed scoped `git diff --check` for Windows source.
The producer's macOS LSP receipt reports the Windows file unlinked/cfg-gated;
it is not treated as native typechecking. No native result is assigned to other
workers' current host/Linux/frontend changes or to a combined integration build.

## Remaining interactive proof (not approved)

The precise aggregate blocker is absence of an identified repaired application
running in an interactive Windows desktop session through its real Tauri surface.
Required evidence remains:

- Repeated real tab switches and masking-overlay open/close cycles, exercising
  attach, detach, close, and replacement through production IPC.
- Correlated child creation and exactly one owner-thread `WM_NCDESTROY` per
  retired child; after owner-queue settlement, one live child per attached pane
  and no accumulating hidden children across repeated cycles.
- Responsive UI/message pump while teardown occurs under host ownership, with
  real screenshots or capture showing no old pane above its replacement/chrome,
  correct overlay masking, and initial-hidden/first-present behavior.
- Identified application source/build and interactive session, plus final window
  and process cleanup. The current native logs do not contain GPU pixels or a
  real swapchain destruction trace.

No decisive evidence is missing for the bounded native API phase. These desktop
items remain pending and block any stronger claim that D6's visible rendering
acceptance, full IPC integration, or the separate macOS symptom is fixed.
