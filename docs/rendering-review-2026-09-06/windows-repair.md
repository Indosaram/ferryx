# D6 Windows owner-thread teardown repair

## Result and owned scope

**Native RED -> GREEN captured at the real `WindowsCompositorTarget::drop`.**
The repair is uncommitted in
`/Users/indo/code/project/orca-lite-rendering-20260906/src-tauri/src/native_terminal/platform/windows.rs`.
Only that source file, this report, and D6 QA artifacts were authored by task
`st_01a0773f`. Main-checkout Windows source remains unchanged. No host/IPC,
renderer, Linux, frontend source, Cargo/build manifest, vendor, or dependency
edits were made. Other workers' changes remain untouched; no index operations,
commits, pushes, merges, application launches, or desktop input were performed.

Evidence directory (relative to main checkout):
`.omo/evidence/ulw/rendering-review-20260906/D6/`.

## Mechanism and minimal repair

Read `WINDOWS_PHASE.md`, review D6/Q6, and the Windows runner/manifest recipe.
The production chain is async detach/close IPC -> `detach_session` /
`close_session` -> `hosts.remove` while holding `hosts` -> target Drop. Teardown
also reaches Drop through `hosts.clear`. Creation comes through main-thread
set-bounds/render dispatch. The host declares `surface` before `target`, which
correctly releases the GPU surface first but does not fix HWND thread affinity.

Previously Drop called `ShowWindow(SW_HIDE)` and `DestroyWindow` on its caller.
The worker cannot destroy an owner-thread HWND. The fix posts the private,
pointer-free `WM_DESTROY_CHILD` message. The real child window procedure calls
`DestroyWindow` on its owner. This requires no Tauri handle, dispatcher registry,
host interface change, synchronous send, or wait under host locks. Same-owner
Drop also queues destruction. Destruction hides/removes the child on its owner.
Post failures are reported except `ERROR_INVALID_WINDOW_HANDLE`, the expected
case when parent destruction already removed its children; owner destruction
failures are reported too.

`WM_NCHITTEST` transparency, creation flags, initial hidden state, reveal logic,
and host field order are unchanged. The existing owner message-pump contract
remains required: it must dispatch queued teardown, or parent destruction must
remove the child. Queue exhaustion is diagnosed, not synchronously retried.

## Faithful test and native RED

The module-local fixture creates a hidden native parent and a real child with
the production registered class and creation flags, then constructs the actual
private target around that HWND. It does not mock or replace Drop, and does not
merely call generic cross-thread `DestroyWindow`. It bypasses the Tauri wrapper
constructor because no application/WebView is launched for this native test.

An owner-thread `SetWindowSubclass` observer is installed before worker Drop.
It forwards every message to the real production window procedure, recording
`WM_NCDESTROY` thread IDs. The worker calls actual Drop, then posts a thread-message
barrier. The owner drains its queue in FIFO order through that barrier. Native
message-queue event waits replace sleeps/polling delays; a 30-second deadline
only fails a stuck test. Observations are copied **before** RAII cleanup, and any
surviving child and parent are destroyed on the owner **before** RED assertion.
The fixture also checks hidden/first-reveal style and native hit-test transparency.

Exact native command, from the isolated repository root:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::platform::windows::tests::child_is_destroyed_on_owner_thread_when_detached_by_worker -- --exact --nocapture
```

RED: `red.log` / UTF-8 `red.txt`, completed 2026-09-07 00:32:04 +09:00
(2026-09-06 15:32:04 UTC), before any production behavior edit:

```text
running 1 test
owner=16488 worker=12048
D6 barrier=true child_live=true destruction_threads=[] owner=16488
D6 cleanup surviving child=0x5ad700ca result=1
D6 cleanup parent=0x5c40132 result=1
D6 cleanup child_live=0 parent_live=0 destruction_threads=[16488]
production WindowsCompositorTarget::drop left child HWND alive after worker barrier
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 465 filtered out
exit=101
```

The first build attempt did not execute a test: Tauri's bundle resource check
required generated `ui/dist`. `red-build-prerequisite.log` preserves that failure;
it is **not** the RED. A hashed archive of the existing local repair-worktree
`ui/dist` build output was staged solely as a resource prerequisite. No UI source
or config was edited, no resource was launched, and no manifest workaround was
used. `resources.ps1`, its receipts, and `ui-dist-prerequisite.tgz` preserve this
input. A separate completion-monitor command timed out after matching itself;
it did not time out or kill Cargo. The subsequent direct Cargo invocation
executed the right-reason RED above.

## Native verification

| Gate | Result | Receipt |
| --- | --- | --- |
| Same exact worker test, GREEN | 1 passed, exit 0; owner 18256, worker 1988; destruction `[18256]` before barrier; child and parent live counts 0 after cleanup | `green.log`, `green.txt` |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::platform::windows::tests:: -- --nocapture` | 3 passed, exit 0: worker, same-owner, parent-already-destroyed | `adjacent.log`, `adjacent.txt` |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::surface_host::tests::surface_host_drop_order_guarantees_surface_drops_before_target -- --exact --nocapture` | 1 passed, exit 0 | `related.log`, `related.txt` |
| `cargo check --manifest-path src-tauri/Cargo.toml` | exit 0, Windows native default features | `check.log`, `check.txt` |
| Changed-file LSP before native build | macOS LSP reports unlinked cfg-gated Windows file; not treated as native validation | `diagnostics.txt` |
| Scoped `git diff --check`; rustfmt equality for added module | both passed | `diagnostics.txt` |

Native lib test builds retain the same 19 pre-existing warnings in RED and GREEN;
native check retains 8 warnings in unchanged files. No failures/warnings were
suppressed. The GREEN exact test passed on its first invocation; no timing retry
was needed. Native tests are the affected runnable surface exercised here.

## Source identity and runner isolation

- Native checkout base: `b8f82d707f0cb99907e3d79c0c9cdc75053ef931`.
- The local lead advanced HEAD concurrently to frontend commit `bce59b4` before
  the complete-history bundle was created. The bundle advertises that newer HEAD,
  but `stage.ps1` explicitly checked out **b8f82d7**, confirmed again at cleanup.
  Native checkout tracked dirty state was only the Windows source file.
- Foreign runner checkout remained at `e2a19066fe36f126d62ffecc952f3dc0b5f3258a`
  with its original dirty paths, recorded before staging and at cleanup.
- Unique staging: `C:/Users/sook/ferryx-winbuild/d6-st_01a0773f-20260906`.
  Pinned Ghostty `6a508fd5e34c7e222c052a6d00bb3891ff3feace` was independently cloned
  with `--no-hardlinks`; no foreign working files or build cache were modified.
- Preflight: no active cargo/rustc/zig/Ferryx/Orca builds/apps found, about 396 GB
  free, 12 logical CPUs. Cargo jobs=8. Cargo/rustc 1.97.0, Zig 0.16.0, native MSVC.
- The existing build.rs already embeds Common-Controls v6 for lib test binaries.
  No release-binary workaround, release build, rollout, or installed app launch.
- `native-Cargo.lock` preserves the native resolved dependency graph as QA only.
  All target and Zig build outputs were isolated; normal Cargo registry reuse was
  non-destructive. No foreign cache cleaning or lock-holder termination occurred.

SHA256 identities (full snapshots and patches retained beside these receipts):

```text
original production windows.rs / unchanged main source
de5d0da481f0c0cf4984661d2658275e7ba9fdb6b795285dcfd2bf2b3b46207e
RED test-only windows-red.rs
6b733478761e029d128c161f17848a1d06ccd8fa17643b435c13a512492d86dd
GREEN and final repair source windows-green.rs
5c81a42082be0ac909b780df491ba21939b23d94522c0f4361387c0ea9f47ab2
native-Cargo.lock
e9009087faa862b51beae7d377fd0aac51ae85fb2670a13ae7b7399f040b385b
```

`red-test.patch` retains test-only RED; `repair.patch` retains the final owned
source delta against b8f82d7. Between RED and GREEN, tests changed only by rustfmt.
Source/report edits and locally generated runner scripts used `apply_patch`;
remote source updates copied the hashed local files, not ad hoc remote edits.

## Resource cleanup and evidence boundary

Every test records HWND identities, owner/destruction threads, and final child
and parent live counts of zero. RED explicitly cleaned its surviving child.
The adjacent parent-destroyed case records exactly one owner-thread destruction,
not a second destruction caused by worker Drop. Worker threads are joined after
their posted barrier; test processes exit through Cargo. Per-phase process
receipts show only the transient SSH command wrapper after native execution.

`cleanup.log` records zero owned native processes, unchanged foreign checkout
HEAD/dirty paths, and successful removal of the entire owned staging tree
(source clones, bundle, resource archive, scripts, target caches, and logs after
local retrieval). `cleanup-final-survey.log` checks no owned command process or
staging directory remains. No process was killed. All retained artifacts are
local under D6; the source repair remains uncommitted for lead integration.

This proves native HWND teardown/thread identity through the production private
destructor in **SSH Session 0**. It does not prove visible Windows GUI behavior,
full Tauri attach/detach IPC integration, swapchain pixels, real tab switches,
masking overlays, or the macOS symptom. Those aggregate acceptance claims remain
separate; the hidden fixture parent never presents a user desktop window.
