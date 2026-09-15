# Windows startup native terminal bounds failure — executable diagnosis

**Date:** 2026-09-12
**Task:** st_01a09574 (bounds-diagnosis lane; read-only — no production edits made)
**Tree state:** HEAD `99e0450d`, working tree clean at start; evidence dir shares files from concurrent lanes
(st_01a09573 shell-diagnosis, st_01a09575 windows-environment). This report writes only this file.
**Symptom under diagnosis:** `bun tauri dev` on Windows shows no usable native terminal at startup with the
banner `Failed to update native terminal bounds` (NativeTerminalPane).
**Method:** source trace of UI NativeTerminalPane -> native bridge -> Rust native_terminal -> IPC with LSP
document symbols; git regression-window archaeology; one existing cheap test run. No Windows reproduction was
claimed without execution; runtime confirmation items are listed as such.

---

## 1. Exact failure source (observed fact chain)

The banner exists in exactly one place. `ui/src/components/NativeTerminalPane.tsx:1824-1829`:

```ts
setError(
  isStructuredIpcError(error)
    ? `Failed to update native terminal bounds: ${error.code}: ${error.message}`
    : "Failed to update native terminal bounds",
);
```

It is reachable only from the `.catch` of the `cmd_native_terminal_set_bounds` invoke inside
`dispatchBounds` (`NativeTerminalPane.tsx:1760`, catch at 1804-1837). Every other terminal IPC failure
shows a different banner (`Failed to attach native terminal`, `NativeTerminalPane.tsx:1989-1992) or is
console-only. Therefore the observed failure is precisely: **attach succeeded, then
`cmd_native_terminal_set_bounds` rejected.**

Startup ordering that makes that combination possible (all verified on disk):

1. Surface effect gates on `surfaceVisible && element && isTauri() && targetSessionId`
   (`NativeTerminalPane.tsx:1725-1737`). The ResizeObserver only reports bounds after attach
   (`NativeTerminalPane.tsx:2023-2032`); the first `dispatchBounds` always follows a successful attach.
2. `attemptAttach` -> `cmd_native_terminal_attach` (`ipc/native_terminal.rs:482-517`).
   Attach **never fails on layout**: `attach_daemon_attachment_with_bounds_and_client`
   (`surface_host.rs:1099-1125`) swallows any `prepare_session_layout` error with
   `tracing::warn!(... "initial bounds layout failed during attach; falling back to default dimensions")`
   (`surface_host.rs:1112-1118`), and `attach_daemon_attachment_with_client`
   (`surface_host.rs:1264-1372`) always inserts the session (`layout: None`, `surface_attached: true`)
   if it was missing. Attach also **never creates the GPU surface host**.
3. Successful attach -> `reportBounds()` -> `dispatchBounds` ->
   `cmd_native_terminal_set_bounds` (`ipc/native_terminal.rs:548-619`), which requires *everything*
   to succeed and maps any error to the banner.

So the failing seam is, narrowed to what set_bounds does that attach does not:

```
cmd_native_terminal_set_bounds                     ipc/native_terminal.rs:548
  app.get_window("main")                           ipc/native_terminal.rs:566-568   (A)
  subscribe_session_update / _detach               surface_host.rs:1874-1893        (B)
  window.run_on_main_thread -> state.render(...)   ipc/native_terminal.rs:584-599
    lock_attached_hosts -> ensure_surface_attached surface_host.rs:929-934, 920-927 (C)
    hosts.entry -> NativeTerminalSurfaceHost::new  surface_host.rs:2038-2046 -> 2252
      NativeSurfaceFrameTarget::new               surface_host.rs:2398-2426
        PlatformCompositorTarget::new (child HWND) windows.rs:218-270                (D1)
        descriptor.validate_desktop_composition   surface_host.rs:2404 (passes: windows.rs:279-286)
        NativeTerminalRenderer::new -> GpuContext::new  gpu_context.rs:49-104       (D2)
        renderer.create_surface(child HWND)       gpu_context.rs:106-114            (D3)
        renderer.configure_surface(1x1)           gpu_context.rs:116-141            (D4)
    prepare_session_layout -> layout compute      surface_host.rs:959, composition.rs:217-320 (E)
    session_render_snapshot / update_config       surface_host.rs:2048-2084         (F)
    host.render_snapshot (full-size configure,    surface_host.rs:2337-2510
      acquire frame, render passes, present,                                        (G)
      reveal_after_present)
  receiver await / deferred loop                   ipc/native_terminal.rs:601-619    (H)
```

Attach shares (E) only through the swallowed call, so a deterministic failure at (E) reproduces exactly
"attach OK, bounds banner, persistent across retries". A failure at (D1)-(D4)/(G) is exclusive to
set_bounds/scheduled renders.

Error fingerprint mapping (observed fact, `ipc/error.rs:216-228`): `NativeTerminalError::SessionDetached`
-> code `SESSION_NOT_FOUND`; **everything else -> `INTERNAL_ERROR`**. The frontend silently swallows
`SESSION_NOT_FOUND` (`NativeTerminalPane.tsx:1809-1814`), so the observed banner is NOT a detached
session; it is an `InternalError` with a Rust message suffix (or an unstructured raw error, which shows
the bare generic banner).

---

## 2. Recalled prior cause checked and DISPROVEN for recurrence at HEAD

Prior incident (2026-09-09, memory `ferryx-browser-terminal-window-lookup.md`): the same banner text with
suffix `INTERNAL_ERROR: Main Ferryx window is unavailable` plus frozen terminals, caused by
`Manager::get_webview_window("main")` returning `None` whenever an embedded browser child webview
(`Window::add_child`) existed. Fix migrated native-terminal infrastructure to
`Manager::get_window("main")` / `tauri::Window<R>` (commit `eeb9e1dc`, 09-09).

Current-tree audit (observed facts):

- `ipc/native_terminal.rs` uses `get_window("main")` at every site: lines 566, 618, 652, 882, 960, 1022,
  1072, 1185, 1378, 1447. The bounds handler itself: `ipc/native_terminal.rs:566`.
- Output-pump dispatch uses `app.get_window("main")` (`surface_host.rs:1410-1415`); a `None` still
  silently consumes the pending render (`render_coordinator.consume_render()`), but `get_window` is
  label-based on the window map and is unaffected by child-webview labels.
- Platform seams take `Window<R>`: `windows.rs:218` (`WindowsCompositorTarget::new`),
  `windows_focus.rs:186,216` (`get_window("main")`).
- The two MockRuntime regressions from the fix are present:
  `bounds_ipc_presents_when_browser_child_is_open` (`surface_host.rs:2861`) and
  `output_presents_when_browser_child_is_open` (`surface_host.rs:2890`). Both add a real child webview,
  assert `get_webview_window("main").is_none()`, and require the bounds IPC / pump to still present.
- **Executed here (macOS host, cross-platform MockRuntime):**
  `cd src-tauri && cargo test --lib browser_child -- --test-threads=1`
  -> `2 passed; 0 failed; ... finished in 0.99s`, exit 0.

Conclusion: at HEAD `99e0450d` the recalled mechanism is fixed and regression-covered. It remains on the
list only as a discriminator: if the QA box's banner suffix literally reads
`INTERNAL_ERROR: Main Ferryx window is unavailable`, the tested build predates `eeb9e1dc` (i.e. is stale,
v2026.09.08.1 or earlier for a released build) or a *new* window-lifecycle regression exists. Startup at
HEAD cannot produce it via the old lookup, and even a browser pane restored before the terminal's first
set_bounds cannot (proven by the regression test). `tauri.conf.json` labels the window `main`
(`src-tauri/tauri.conf.json` `app.windows[0].label = "main"`).

Off-chain remnants of the same lookup pattern (observed, NOT the bounds chain, listed for a later
cleanup lane): `lib.rs:111` (native menu action emits), `lib.rs:471-521` (macOS key monitor, mac-only),
`lib.rs:598,623` (macOS file-drop install/release), `lib.rs:868` (notification wake/show),
`lib.rs:990,1012` (setup focus; the non-macOS branch at 1012 silently skips `set_focus` if the lookup
ever failed). None of these can emit the bounds banner.

---

## 3. Ranked hypotheses for the startup failure at HEAD, with discriminators

Ranked by fit to "Windows startup, attach OK, bounds IPC rejects persistently". H1 is the working
hypothesis; every hypothesis is falsified or confirmed by the runtime probe in section 4.

### H1 — GPU/surface pipeline failure on the hidden child HWND (wgpu v30 window) — leading

**Mechanism.** The first `set_bounds` after attach is the only point that builds the host
(`surface_host.rs:2038-2046`): child HWND creation (`windows.rs:233-253`), wgpu adapter/device
(`gpu_context.rs:49-104`, `Backends::all()`, HighPerformance then fallback-adapter retry),
pipeline/atlas construction, surface creation from the child HWND (`gpu_context.rs:106-114`), configure
at 1x1 then at full pane size (`surface_host.rs:2457-2465`), frame acquire, render passes, present
(`surface_host.rs:2467-2510`). Any of these failing yields a persistent banner on every retry, while
attach stays green.

**Why plausible.** Regression window fits exactly: the last verified-good Windows GUI was v2026.09.06.1
(memory `ferryx-windows-verification.md`, 2026-09-07). Commit `29ea50be` (09-07 10:16) upgraded wgpu
24 -> 30.0.1 and removed the local wgpu-hal vendor patch; `7535fbdd`, `11a2755b` followed adapting the
frame-retry harness. The user's failing QA runs a debug build of HEAD, which includes this.

**Honest boundaries of this hypothesis.** The removed vendor patch touched only
`wgpu-hal/src/metal/layer_observer.rs` (macOS Metal teardown;
`docs/FERRYX_METAL_OWNERSHIP_INVESTIGATION_2026-09-05.md:191-207`),
so its removal is not itself a Windows factor. v30 backend/dxgi behavior on the user's GPU stack remains
unverified from source alone. macOS works, so the failure must be Windows-platform-specific (DX12/Vulkan
adapter, DXGI swapchain on a hidden `WS_CHILD` window, or WebView2 sibling interaction).

**Discriminator.** Banner suffix / tracing text contains one of: `GPU adapter request failed:`
(`gpu_context.rs:70-72`), `GPU device request failed:` (`gpu_context.rs:84-93`), `Surface create error:`
(`gpu_context.rs:108-113`), `GPU render pipeline error:` (renderer/pipelines), or a `configure`/acquire
failure string. The identical error must recur on every retry (deterministic) and must also appear as
`tracing::warn!("Failed to lazily create native terminal surface host during scheduled render")`
(`surface_host.rs:384-391`) or `"Failed to render native terminal snapshot"` (`surface_host.rs:404-410`)
from the output pump.

**Minimal falsifiable repair scope.** A Windows-native RED test (QA lane's maho-win environment) that
mirrors the chain on a real hidden child HWND: `WindowsCompositorTarget::new` ->
`GpuContext::new` -> `create_surface` -> `configure_surface` -> one acquire/render/present, asserting
each step; the step that fails RED is the seam. Fix only that step (e.g. adapter/device fallback order,
or defer D3-D4 until the child is shown) — no UI changes.

### H2 — Startup geometry rejected by layout validation

**Mechanism.** `SurfaceCompositionLayout::compute` (`composition.rs:217-320`) rejects non-finite values,
negative x/y, non-positive width/height/scale, and — importantly — `cols == 0 || rows == 0`
(`composition.rs:302-310`). The frontend's only guard is "at least 1 physical pixel"
(`NativeTerminalPane.tsx:758-766`); a pane smaller than one cell (~10x20 px physical at default font)
passes the frontend check and fails `compute` with `Invalid terminal dimensions: cols=0, rows=0`.
Attach swallows that exact error (`surface_host.rs:1112-1118`); set_bounds does not.

**Why possible.** It produces the exact symptom deterministically and self-heals only if a later
ResizeObserver tick measures a valid pane. Requires the startup pane to be degenerate at the first
report — plausible if the restored layout/animation leaves the pane tiny on first layout on Windows.

**Discriminator.** Banner suffix `INTERNAL_ERROR: Invalid terminal dimensions: cols=0, rows=0 (both must
be > 0)` or `INTERNAL_ERROR: Invalid value or parameter: ...`; the webview
`[ferryx:switch] terminal.surface.bounds.error` entry carries the exact dispatched bounds, showing a
degenerate rect; and the warn `initial bounds layout failed during attach` appears in stdout.

**Minimal falsifiable repair scope.** Once the suffix confirms it: raise the frontend deferral
threshold (defer until the pane spans at least one cell) or clamp in `compute` — one seam, one test
(`panic_free_layout` unit RED at `surface_host.rs:129`).

### H3 — Main-thread dispatch failure or panic in the render closure

**Mechanism.** `run_on_main_thread` rejects -> `INTERNAL_ERROR: Could not dispatch native terminal
render: ...` (`ipc/native_terminal.rs:599-604`); closure dropped before `sender.send` ->
`INTERNAL_ERROR: Main thread stopped before native terminal render completed`
(`ipc/native_terminal.rs:605-608`); a panic in the closure produces the latter plus likely process
death. Fits the known clean-machine hang family (memory
`ferryx-windows-daemon-tcp-and-clean-machine-hang.md`) only if the event loop is wedged.

**Discriminator.** Exact suffix above; `panicked at` in `bun tauri dev` stdout; or the app dying instead
of showing a persistent banner (app alive with banner in the user report argues against a hard panic).

**Repair scope.** If panic: capture and surface the panic source, fix that step; keep the Sep 5
constraint that follow-up renders enqueue off-thread (Wry runs main-thread dispatch inline —
`surface_host.rs:413-419`).

### H4 — Session missing at subscribe (`NoValue`)

`subscribe_session_update/_detach` (`surface_host.rs:1874-1893`) fail with `NoValue` only if the session
does not exist. Attach guarantees insertion before the first set_bounds (section 1). A detach racing
set_bounds returns `SessionDetached` -> `SESSION_NOT_FOUND` -> silently swallowed, never a banner.
Ruled out except a still-unknown exotic order; confirmed only if the suffix reads
`INTERNAL_ERROR: Requested value does not exist (NoValue)`.

### H5 — Recurrence of the recalled window-lookup failure — disproven at HEAD

Covered in section 2. Discriminator: suffix `INTERNAL_ERROR: Main Ferryx window is unavailable`. On a
HEAD debug build that reading indicates either a stale build or a brand-new regression in
`get_window("main")` reachability at startup — neither supported by current source; verify build provenance
first if this suffix appears.

---

## 4. Exact discriminating runtime probe (Windows lane owns execution)

No hand-rolled daemon clients: for any daemon-level check use the known verifier
`script/qa/win-daemon-e2e.mjs` against Windows TCP `%LOCALAPPDATA%\Ferryx\runtime\daemon.port`
(memory `ferryx-windows-verification.md`). The probe below is log/console capture, not a new client.

1. Reproduce once: `bun tauri dev 2>&1 | tee C:\Users\sook\ferryx-winbuild\bounds-stdout.log`
   (QA-lane launch recipe per PLAN.md; interactive session only).
2. Capture the pane banner suffix verbatim (QA-lane screenshot technique) and the webview devtools
   console entries: `console.error "Native terminal IPC command failed" {command,
   "cmd_native_terminal_set_bounds", error}` (`NativeTerminalPane.tsx:343-345`) and
   `[ferryx:switch]` events `terminal.surface.attach.start/complete/error`,
   `terminal.surface.bounds.start/error/detached/deferred` (`NativeTerminalPane.tsx:1766-1782,
   1816-1820, 1856-1879`). Note: webview console does not forward to stdout.
3. Grep the stdout log for the decision strings (each maps to exactly one seam):

| Log/banner string (grep, case-sensitive) | Seam | Hypothesis |
|---|---|---|
| `initial bounds layout failed during attach` | layout rejected at attach, then again at set_bounds | H2 (with suffix) |
| `Invalid terminal dimensions: cols=` or `Invalid value or parameter:` | `composition.rs:217-320` | H2 |
| `GPU adapter request failed:` / `GPU device request failed:` / `Surface create error:` | `gpu_context.rs:49-141` | H1 |
| `Failed to lazily create native terminal surface host during scheduled render` | host build on pump path (`surface_host.rs:384-391`) | H1 (same root as banner) |
| `Failed to render native terminal snapshot` (`surface_host.rs:404-410`) | render/present | H1 (late step) |
| `Could not dispatch native terminal render` / `Main thread stopped before native terminal render completed` | main-thread dispatch (`ipc/native_terminal.rs:599-608`) | H3 |
| `Main Ferryx window is unavailable` | window lookup | H5 (stale build / new regression) |
| `Requested value does not exist (NoValue)` | subscribe on missing session | H4 |
| `panicked at` | panic in render closure | H3 |

4. Decision: suffix names the seam -> implement the matching minimal repair from section 3, with the RED
   test first. If the banner shows the generic (suffix-less) variant, the error was unstructured
   (`isStructuredIpcError` false): recover the structured detail from the devtools console entry, which
   always carries `{code, message}` for Tauri IPC rejects.

Evidence-gap defect observed (not fixed here): `cmd_switch_debug_log` appends switch-debug JSONL to a
hardcoded POSIX path `/tmp/ferryx-switch-debug.jsonl` (`ipc/debug.rs:40-52`); `OpenOptions::open` fails
on Windows unless `C:\tmp` exists, so on Windows this evidence channel is silently lost (webview-side
`console.warn "[ferryx:switch] log sink failed"` only). Debug-builds tracing remains on stdout, which is
why step 1 captures stdout.

---

## 5. Retained-frame baseline defect — separately owned, not Windows startup

Independent baseline layer (validated against source on 09-12): on macOS exit-with-retained-frame,
`surfaceSessionId` retains the backend id only when `isMacShortcutPlatform()` is true
(`NativeTerminalPane.tsx:536`), while `bindingKey` becomes `null` because `targetSessionId` is null
(`NativeTerminalPane.tsx:537-540`). `bindingKey` sits in the main surface effect's dependency array
(`NativeTerminalPane.tsx:2088`), so the cleanup runs and schedules `cmd_native_terminal_detach`
(`NativeTerminalPane.tsx:2063-2069`); the re-created effect's `attemptAttach` returns immediately because
`attachmentOwnerRef.current?.live` no longer matches (`NativeTerminalPane.tsx:1907`), so nothing cancels
the pending detach and `isAttached` is never re-set — retained bounds are never re-reported. This chain
plausibly explains all three pre-existing `NativeTerminalPane.presentation.test.tsx` failures recorded in
`baseline.md`; it is macOS-gated code and does not produce the Windows startup banner.

Source-based discriminator for the lead's fixture suspicion ("tests fail to pin MacIntel") — verified, not
assumed: `detectMacPlatform()` (`ui/src/lib/shortcuts.ts:542-548`) reads bare global `navigator` at call
time (`navigator.platform` / `navigator.userAgent`, lines 543-544) with no module-level caching and falls
back to `process.platform === "darwin"` (546-547). No `window.navigator`/`globalThis.navigator` read
exists in `shortcuts.ts`, `NativeTerminalPane.tsx`, or the presentation test, so the suite's
`vi.stubGlobal("navigator", { platform: "MacIntel", userAgent: "Macintosh" })`
(`NativeTerminalPane.presentation.test.tsx:55`) is exactly the object production resolves through the
global environment record in the vitest jsdom environment (globalThis === window); the stub reaches the
read, and on the darwin host the fallback makes `isMacShortcutPlatform()` true regardless of stub
fidelity. The retention branch at `NativeTerminalPane.tsx:536` was therefore active in the failing run,
and the three failures follow the production chain above (detach scheduled on `bindingKey`-null cleanup,
attach early-return, `presentation` cleared on detach completion), not a missing platform fixture.
Caveat: only on a non-darwin host could stub fidelity change outcomes; these results are from lead
capture `bash_1` on macOS.

Ownership (per lead plan): the shell-selection regression is implemented in `TabBar.tsx` +
`TerminalSplitView.tsx` and their tests (scope of st_01a09573 shell-diagnosis; no native-file overlap);
the retained-frame presentation defect is owned by the repair batch
`dag_0af60f86-ac23-45f9-93dd-2b918f67c2e0` (UI NativeTerminalPane/lifecycle files). The startup-bounds
implementation must NOT write those UI files concurrently; coordinate startup fixes through the Rust/IPC
seams or wait until those batches settle. Tests must not be weakened to mask this chain.

---

## 6. Prior-fix constraints to preserve in the startup repair (from Sep 5/8 repairs)

From memory `ferryx-native-attach-readiness-probe.md` and `ferryx-obsolete-deferred-bounds-replay.md`
(prior verified repairs, not fresh root proof — keep as invariants):
explicit `presented` receipts; retry of dropped bounds frames; serialized host presentation vs
detach/close; Lagged/Gap render scheduling; unconditional warm-return PTY dimension reassertion;
`initial_request.take()` single-application of requested geometry (no obsolete-bounds replay after
DEC2026 readiness retries); follow-up render dispatch enqueued off-thread (Wry inline main-thread
dispatch, `surface_host.rs:413-419`).

---

## 7. Verification status and remaining runtime unknowns

Executed in this lane (macOS host):
- `cd src-tauri && cargo test --lib browser_child -- --test-threads=1` -> 2 passed / 0 failed, exit 0
  (proves the migrated window-lookup chain on the current tree).
- LSP document symbols on `surface_host.rs`, plus full source reads cited above; error-fingerprint table
  derived from `ipc/error.rs:216-228` and `native_terminal/error.rs:20-44`.

Not reproduced (requires the Windows box; QA lane st_01a09575 owns): the actual startup banner suffix,
the stdout tracing warns, and which of H1/H2/H3 fires. The source-level causal discriminator is the
banner-suffix decision table in section 4; which row fires remains UNCERTAIN until the Windows runtime
probe (section 4 steps 1-4) returns — do not treat H1 as confirmed. No long cargo run is pending from
this lane; the one test run above completed. Until then, H1 is the recommended working hypothesis (only
hypothesis whose regression window matches the last verified-good Windows GUI), H2 the first fallback to
check because its discriminator is cheapest to read.

Unexpected defects found during diagnosis (reported only; no writes): Windows `/tmp` switch-debug path
loss (section 4); off-chain `get_webview_window` remnants in `lib.rs` (section 2), of which the menu
emission at `lib.rs:111` silently drops menu actions while a browser child is open — same lookup class
as the Sep 9 incident, candidate for the post-batch cleanup, unrelated to the startup banner.
