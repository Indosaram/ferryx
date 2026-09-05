# Ferryx Memory Footprint Audit & Root Cause Analysis

**Date:** 2026-09-05  
**Target:** Ferryx Desktop App (`/Applications/Ferryx.app`, PID 63564) & Daemon on macOS Sonoma/Sequoia  
**Issue:** Activity Monitor reports Ferryx consuming > 2.3 GiB of memory total, with the main `Ferryx` process alone taking 1.66 GB.

---

## 1. Executive Summary & Verdict

Ferryx was architected to be significantly lighter than Orca (Rust backend + WebKit + libghostty native terminal vs. Electron + Node.js + Chromium + xterm.js DOM). In a clean state:
- Baseline Ferryx App: ~40–60 MB
- WebKit Viewport (`tauri://localhost` + GPU process): ~150–250 MB
- Background Daemon: ~15–30 MB
- **Normal Total Baseline:** ~250–350 MB (compared to Orca's baseline of 500–900 MB+).

However, live forensic inspection of the running process (`PID 63564`) revealed **a severe GPU memory leak in the native terminal engine**:
- **1.54 GB out of the 1.66 GB in the main process is leaked Metal / IOSurface swapchain drawables.**
- Actual Rust / C heap memory is only **46 MB**.
- The leak is caused by **Metal swapchain / WgpuObserverLayer accumulation during tab switching and pane re-attachment**. Each attach cycle spawns a new Metal layer with double/triple-buffered Retina drawables (~24 MB per pane), which are retained by CoreAnimation / FramePacing and never released. Over 45 minutes of usage, 159 layer instances and 324 high-resolution drawables accumulated.
- In addition, **two background daemon processes** were running concurrently (one orphan from an earlier launch), adding unnecessary overhead.

---

## 2. Activity Monitor Breakdown (from User Evidence)

- **Ferryx (Main Process, PID 63564):** 1.66 GB
  - Real App Logic (MALLOC_SMALL / Tiny / Nanomalloc): **46.6 MB**
  - Text & Shared Libraries (__TEXT): **393 MB** (shared code pages)
  - **IOSurface GPU Textures:** **1,143 MB (1.14 GB)**
  - **Owned Physical Footprint (Graphics):** **394 MB**
- **tauri://localhost (WebKit Web Process, PID 63747):** 270.9 MB
  - React UI, Zustand store, DOM nodes, JavaScript engine heap.
- **Ferryx Graphics and Media (WebKit GPU Process, PID 63745):** 263.7 MB
  - Safari/WebKit compositor layer backing surfaces and UI rendering buffers.
- **ferryx --daemon (PID 64274, active):** 94.4 MB
  - Headless PTY manager, ring buffers (512 KiB per session), 25 active sessions.
- **ferryx --daemon (PID 45506, duplicate orphan):** 46.5 MB
  - Stale daemon instance from 8:15 AM that did not exit after upgrade/restart.
- **AutoFill & Networking (WebKit helper processes):** ~19 MB

---

## 3. Forensic Analysis of Main Process (PID 63564)

### 3.1 macOS Footprint Analysis (`footprint 63564`)

```
ferryx [63564]: 64-bit    Footprint: 1699 MB (16384 bytes per page)
  Dirty      Clean  Reclaimable    Regions    Category
1143 MB        0 B        93 MB        416    IOSurface
 394 MB        0 B          0 B         81    Owned physical footprint (unmapped) (graphics)
  81 MB        0 B          0 B         68    MALLOC_SMALL
  27 MB        0 B          0 B         74    app-specific tag 1
  26 MB        0 B          0 B        420    Owned physical footprint (unmapped)
```

### 3.2 IOSurface Breakdown (`vmmap 63564`)

- **CAMetalLayer Display Drawable:** 324 surfaces, **1,143.2 MB resident / swap**
  - Dimensions: `2150x1427` (BGRA 32-bit Retina framebuffer = ~12.1 MB each)
  - Shared with: `WindowServer [394]`
- **WebKit LayerBacking:** 44 surfaces, 102.7 MB virtual, 0 MB dirty
- **WebKit ImageBuffer:** 6 surfaces, 0.1 MB

### 3.3 Heap Object Count (`heap 63564`)

- `FerryxNativeTerminalView` (Active NSViews): **2** (matches the 2 visible panes on screen)
- `WgpuObserverLayer` (Metal Layers): **159**
- `CAImageQueue` / `CA::Render::ImageQueue`: **159**
- `FPCAMetalLayerState` (macOS FramePacing): **159**
- `IOSurfaceSharedEventListener`: **159**

---

## 4. Root Cause: Metal Swapchain Leak on Pane Re-Attach

1. **Attach/Detach Lifecycle:**
   When the user switches tabs or navigates worktrees, `NativeTerminalPane.tsx` unmounts and calls `cmd_native_terminal_detach`. When switching back, it calls `cmd_native_terminal_attach`.

2. **WGPU Surface Construction:**
   On attach, `NativeTerminalSurfaceHost::new()` creates a new `PlatformCompositorTarget`, which creates a `FerryxNativeTerminalView` and invokes `wgpu::Instance::create_surface()`.

3. **wgpu-hal Sublayer Injection:**
   In `wgpu-hal 24.0.4` (`src/metal/surface.rs` / `layer_observer.rs`), because the NSView backing layer is not a raw `CAMetalLayer`, it instantiates `WgpuObserverLayer` (a dynamic `CAMetalLayer` subclass) and calls:
   ```objc
   [root_layer addSublayer: observer_layer];
   [root_layer addObserver: observer_layer forKeyPath:@"contentsScale" ...];
   [root_layer addObserver: observer_layer forKeyPath:@"bounds" ...];
   ```

4. **Missing Teardown & External Retention:**
   When `detach_session` drops `NativeTerminalSurfaceHost`:
   - `wgpu-hal`'s `Surface` on Metal has **no Drop implementation** and never removes the sublayer or unregisters KVO.
   - CoreAnimation's `CAImageQueue` and Apple's `FramePacing.framework` (`FPCAMetalLayerState`) maintain strong handles to the layer and its allocated display drawables.
   - As a result, every time a pane is re-attached, **two new 12 MB Retina drawables (~24 MB)** are allocated and permanently retained in VRAM/IOSurface memory. Over 150 tab switches, this leaked over **1.54 GB**.

---

## 5. Orca vs. Ferryx Architecture Comparison

- **Orca Architecture:**
  - Runtime: Electron (Chromium + Node.js)
  - Terminal Engine: xterm.js (DOM / HTML Canvas)
  - Baseline Footprint: 500 MB – 900 MB on startup. Because Chromium manages all web contents and canvases in a single shared GPU process, memory is high but predictable unless long scrollback buffers build up.
- **Ferryx Architecture:**
  - Runtime: Tauri v2 (Rust desktop binary + OS WebKit)
  - Terminal Engine: `libghostty-vt` + native Metal/WGPU hardware surface
  - Target Baseline: 200 MB – 350 MB total.
  - **Why it appeared heavier (>2.3 GiB):**
    - 1.54 GB was an unintended Metal drawable leak caused by repeated surface creation without drawable purging.
    - An orphaned duplicate daemon process was running (PID 45506).
    - WebKit's multi-process model splits memory across `Ferryx`, `tauri://localhost`, and `Ferryx Graphics and Media`, whereas Electron combines several of these under Chrome Helper processes.

---

## 6. Recommended Action Plan

1. **Fix Metal Surface Leak in Rust Backend:**
   - Instead of destroying and recreating `PlatformCompositorTarget` and `wgpu::Surface` on every tab switch/detach, pool or retain the existing target and surface, or explicitly invoke `[sublayer removeFromSuperlayer]` and clear drawable queues before releasing.
   - Alternatively, configure `FerryxNativeTerminalView` to have `CAMetalLayer` as its native root layer (`-[NSView makeBackingLayer]` returning `CAMetalLayer`), avoiding `wgpu-hal`'s `new_observer_layer` sublayer wrapper entirely.
2. **Daemon Duplicate Cleanup:**
   - Terminate PID 45506 and ensure daemon startup strictly enforces single-instance takeover with process killing.
