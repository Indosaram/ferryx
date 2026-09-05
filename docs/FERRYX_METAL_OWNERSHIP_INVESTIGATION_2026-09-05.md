# Metal layer ownership investigation

Date: 2026-09-05. Read-only runtime inspection; no implementation changes.

## Runtime evidence

Installed app PID 84403, version 2026.902.2; macOS 26.6 (25G72). The installed executable is stripped: nm exposes imported symbols but no named WgpuObserverLayer dealloc implementation. Its exact dependency source-to-binary match and live dealloc execution have not been established.

Earlier same-PID samples: footprint 1027 -> 3303 MB; IOSurface 447 -> 2607 MB; observer layers 35 -> 198; native views 6 -> 4. At approximately 13:55, exact-class heap address enumeration found 204 observer layer objects. Address order is not creation order.

## Reference graph samples

Two object addresses were inspected with leaks --trace (both exit 0). One had two roots; the other had 115 roots. These are memory reference paths, not proof that every pointer is an owning reference. In particular FramePacing _layerAddress is not marked __strong by this output.

The small sample has paths from QuartzCore CA::Render::notifications and FramePacing globals, without a native view in the reported chains. This narrows investigation to incomplete compositor cleanup but does not demonstrate whether dealloc was called.

### Verbatim small trace

```text
Tracing: <WgpuObserverLayer@0x102758b60 0xc8bc30060> [48]

Found 2 roots referencing: <WgpuObserverLayer@0x102758b60 0xc8bc30060> [48]

VM: __DATA_DIRTY  0x1ed7b88e0-0x1ed7c0c90 [V=33K] rw-/rw-  /System/Library/Frameworks/QuartzCore.framework/Versions/A/QuartzCore
__DATA_DIRTY __bss: 'CA::Render::notifications' + 744 0x1ed7bcd10 --> 0xc8c1dc050 [80]
    +8:                           0xc8c1dc058 --> 0xc8c1dc000 [80]
    +8:                           0xc8c1dc008 --> 0xc8bcac000 [80]
    +8:                           0xc8bcac008 --> 0xc8c1d0000 [80]
    +8:                           0xc8c1d0008 --> 0xc989a7f70 [80]
    +8:                           0xc989a7f78 --> 0xc989a7f20 [80]
    +8:                           0xc989a7f28 --> 0xc989a7ed0 [80]
    +8:                           0xc989a7ed8 --> 0xc989a7e80 [80]
    +8:                           0xc989a7e88 --> 0xc989a7e30 [80]
    +8:                           0xc989a7e38 --> 0xc989a7de0 [80]
    +8:                           0xc989a7de8 --> 0xc989a7d90 [80]
    +8:                           0xc989a7d98 --> 0xc989a7d40 [80]
    +8:                           0xc989a7d48 --> 0xc989a7cf0 [80]
    +8:                           0xc989a7cf8 --> 0xc989a7ca0 [80]
    +8:                           0xc989a7ca8 --> 0xc989a7c50 [80]
    +8:                           0xc989a7c58 --> 0xc989a7c00 [80]
    +8:                           0xc989a7c08 --> 0xc989a7bb0 [80]
    +8:                           0xc989a7bb8 --> 0xc989a7b60 [80]
    +8:                           0xc989a7b68 --> 0xc989a7b10 [80]
    +8:                           0xc989a7b18 --> 0xc989a7ac0 [80]
    +8:                           0xc989a7ac8 --> 0xc989a7a70 [80]
    +8:                           0xc989a7a78 --> 0xc989a7a20 [80]
   +56:                           0xc989a7a58 --> <WgpuObserverLayer@0x102758b60._priv (malloc) 0xc8c0a6e40> [320]
   +24:                           0xc8c0a6e58 --> <WgpuObserverLayer@0x102758b60 0xc8bc30060> [48]

VM: __DATA_DIRTY  0x29974c210-0x29974c548 [V=824] rw-/rw-  /System/Library/PrivateFrameworks/FramePacing.framework/Versions/A/FramePacing
__DATA_DIRTY __bss: '_MergedGlobals' + 24 0x29974c420 --> <NSMutableDictionary 0xc98d49ce0> [32]
    +8: __strong storage          0xc98d49ce8 --> <NSMutableDictionary (Storage) 0xc99519800> [6144]
 +3400: __strong                  0xc9951a548 --> <FPCAMetalLayerState 0xc8c075180> [224]
   +24: _layerAddress             0xc8c075198 --> <WgpuObserverLayer@0x102758b60 0xc8bc30060> [48]

```

## Source finding

Local locked wgpu-hal 24.0.4 src/metal/layer_observer.rs:172 implements dealloc, removes KVO observers if superlayer exists, and returns without calling CAMetalLayer's superclass dealloc. This is a concrete source-level teardown defect; runtime attribution still needs a source-matched debug experiment. Removing sublayers alone does not correct this missing superclass call and changes the superlayer lookup used for observer removal.

The application drops fields in surface -> platform target -> renderer order. MacosCompositorTarget::drop removes NSView from its superview and releases its owned reference on the main thread. A missing explicit Surface Drop implementation alone does not prove a leak: Rust field destructors and MetalLayer RAII still run.

## Independent review reconciliation

The independent source review confirms the surface -> target -> renderer field order and missing superclass dealloc call. Correct wording: the observer override does not invoke its superclass CAMetalLayer's dealloc (not CAMetalLayer's superclass).

The review also proposed that QuartzCore reachability proves an active retain and therefore dealloc has not run. That inference is not accepted: pointer reachability does not establish a retain count or whether a custom dealloc previously returned without freeing the object. Neither missing explicit Surface Drop nor absence of removeFromSuperlayer alone proves a leak, since parent-layer destruction can release child layers normally. The source-level superclass-call defect is established; the active runtime mechanism remains unresolved.

A direct CAMetalLayer backing layer is a plausible alternative experiment that bypasses WgpuObserverLayer, not a verified fix. It must preserve scale, geometry, filtering, opacity and presentation behavior. Pooling is an optimization and cannot establish correct destruction.

## Remaining discriminating experiment

### Reconstructed observer experiment (Gemini executor)

Executed by mahoquot/gemini-3.8-flash-high, child st_01a07092. Existing /tmp/wgpu_diagnostic reconstruction was run for 50 cycles per mode with readiness/stdin pipe handshake (no FIFO). Both diagnostic processes and both heap commands exited 0.

- Original PID 84609: dealloc entered 50 times; heap still observed 50 OriginalWgpuObserverLayer, 50 private layer allocations, 50 CAImageQueue, 50 FPCAMetalLayerState.
- Corrected PID 84635: dealloc entered and superclass call completed 50 times; no corrected layer/private/FPCAMetalLayerState rows observed; one CAImageQueue remained, cause unattributed.
- This positively demonstrates missing superclass dealloc can leave these objects allocated even after dealloc entry. It refutes the assumption that graph reachability necessarily means dealloc never ran.
- Limit: reconstructed observer with stubbed observation callback, not actual crate source; no drawables allocated. Actual dependency-source reproduction and GPU attribution remain outstanding.
- Prior multi-hour experiment hang was FIFO-open deadlock plus shell busy polling, from 05:18:57Z to cancellation at 07:47:24Z, not a long build. Existing diagnostic processes were absent in recovery process scan.

### Actual dependency-source comparison

Gemini executor built diagnostics/metal-observer-proof offline with exit 0 using CARGO_TARGET_DIR=/tmp/ferryx-metal-observer-proof-target. Original module imports the actual registry source; corrected copy preserves its functions and differs only by superclass dealloc call (CRLF normalized).

- Original source SHA-256: 280d533027bff23ee5c6caf5913b6f82f2afa4c933135b840d130100ca9abc7b.
- Corrected source SHA-256: 399f888f55e462b0673aae9279bc99f9f045010a16f886b78a7ee1651cf88fd0.
- Original PID 92714, 50 cycles: 50 observer objects, 50 private allocations, 50 FPCAMetalLayerState, 650 FPDurationStatistics, 50 CAImageQueue.
- Corrected PID 92748, 50 cycles: no observer/private/FPCAMetalLayerState/FPDurationStatistics rows observed; 1 unattributed CAImageQueue.
- Both child and heap commands exited 0, diagnostic stderr empty. Build emitted 91 unsuppressed legacy objc macro check-cfg warnings (cargo-clippy); no build errors. LSP daemon unavailable.
- Verdict: missing superclass dealloc causes actual dependency observer/backing-object accumulation in this controlled lifecycle. No explicit removeFromSuperlayer or additional KVO cleanup was needed for the corrected branch to eliminate observed observer instances.
- Still untested: drawable allocation/IOSurface growth and equivalence to installed app binary. No user app was modified or launched by the experiment.

## Final causal verdict: actual observer with real drawables

Experiments executed exclusively by mahoquot/gemini-3.8-flash-high (st_01a07092); parent applied experimental artifacts and reviewed raw tool output. Original registry source and corrected copy differ only by superclass dealloc invocation. In each branch 20 of 20 offscreen 512x512 drawables were acquired, autorelease pools drained, observer handle and parent view released before heap inspection.

Original PID 99417 retained 20 observer objects, 20 private allocations, 20 FPCAMetalLayerState, 20 IOSurface objects and 20 IOSurfaceSharedEventListener. Corrected PID 99461 showed none of these rows, with one unattributed CAImageQueue remaining. All six diagnostic/heap/footprint exit codes were 0 and diagnostic stderr empty. Build exit0; 98 unsuppressed objc cargo-clippy check-cfg warnings.

This establishes missing superclass dealloc as a causal defect in the actual wgpu-hal24.0.4 observer path, including retention of real drawable-associated IOSurface objects. Explicit sublayer removal, pooling and frontend changes were not required to eliminate this observed retention.

IMPORTANT LIMIT: raw footprint shows IOSurface dirty bytes equal ZERO in BOTH branches. Drawables were acquired but not rendered/presented. The experiment proves resource-object retention, not reproduction of the installed app's 2GB resident/dirty GPU increase. Installed binary equivalence and a full app fix verification remain untested. Never claim this experiment measured 20MB of leaked resident GPU memory. Total dirty was 7953KB vs7793KB; CoreAnimation dirty320KB vs0; IOSurface regions64 vs43.

No product fixes, shared dependency edits, UI automation or app restarts were performed. Diagnostic files are macOS-only temporary investigation artifacts, with an absolute local registry import; not portable shipped code.

### Verified raw tool output

```text
=== Starting mode: original ===
[original] PAUSED_FOR_HEAP_INSPECTION received. Child PID: 99417
[original] Invoking /usr/bin/heap and /usr/bin/footprint on PID 99417...
[original] heap exited with 0, footprint exited with 0
[original] Sending newline to stdin and ending pipe...
[original] Child exited with code 0
=== Starting mode: corrected ===
[corrected] PAUSED_FOR_HEAP_INSPECTION received. Child PID: 99461
[corrected] Invoking /usr/bin/heap and /usr/bin/footprint on PID 99461...
[corrected] heap exited with 0, footprint exited with 0
[corrected] Sending newline to stdin and ending pipe...
[corrected] Child exited with code 0

==================== RAW EXPERIMENT RESULTS ====================

### MODE: ORIGINAL ###
Exit Codes -> Child: 0, Heap: 0, Footprint: 0
Diagnostic Full Stdout:
=== WGPU Metal Layer Observer Proof Harness ===
Process PID: 99417
Mode: original, Target cycles: 20, With drawables: true
Metal default device: 0x102fb28f0
Drawables requested: 20
Drawables acquired: 20
PAUSED_FOR_HEAP_INSPECTION
Experiment run finished cleanly.
Diagnostic Full Stderr: (empty)
Filtered Heap Rows (9 matches):
       260      12480      48.0   FPDurationStatistics                              ObjC    FramePacing
        20       8960     448.0   IOSurface._impl (malloc)                          C       IOSurface
        20       7680     384.0   CAImageQueue                                      CFType  QuartzCore
        20       6400     320.0   WgpuObserverLayer@0x102940290._priv (malloc)      C       QuartzCore
        20       4480     224.0   FPCAMetalLayerState                               ObjC    FramePacing
        20        960      48.0   IOSurfaceSharedEventListener._notificationPort (struct IONotificationPort)  C       IOSurface
        20        960      48.0   WgpuObserverLayer@0x102940290                     ObjC    QuartzCore
        20        640      32.0   IOSurfaceSharedEventListener                      ObjC    IOSurface
        20        320      16.0   IOSurface                                         ObjC    IOSurface
Footprint Output Summary:
    Dirty      Clean  Reclaimable    Regions    Category
  3744 KB        0 B          0 B          8    MALLOC_SMALL
   933 KB        0 B          0 B        508    __DATA_DIRTY
   640 KB        0 B          0 B          9    MALLOC metadata
   320 KB        0 B          0 B         25    CoreAnimation
   256 KB        0 B          0 B          1    MALLOC_TINY
    48 KB        0 B          0 B          4    IOKit
    16 KB        0 B          0 B          1    MALLOC_NANO
      0 B        0 B          0 B         64    IOSurface
  7953 KB    3088 KB          0 B       4772    TOTAL

### MODE: CORRECTED ###
Exit Codes -> Child: 0, Heap: 0, Footprint: 0
Diagnostic Full Stdout:
=== WGPU Metal Layer Observer Proof Harness ===
Process PID: 99461
Mode: corrected, Target cycles: 20, With drawables: true
Metal default device: 0x1027d6840
Drawables requested: 20
Drawables acquired: 20
PAUSED_FOR_HEAP_INSPECTION
Experiment run finished cleanly.
Diagnostic Full Stderr: (empty)
Filtered Heap Rows (1 matches):
         1        384     384.0   CAImageQueue                                      CFType  QuartzCore
Footprint Output Summary:
    Dirty      Clean  Reclaimable    Regions    Category
  3760 KB        0 B          0 B          8    MALLOC_SMALL
   947 KB        0 B          0 B        509    __DATA_DIRTY
   640 KB        0 B          0 B          9    MALLOC metadata
   256 KB        0 B          0 B          1    MALLOC_TINY
    48 KB        0 B          0 B          4    IOKit
    16 KB        0 B          0 B          1    MALLOC_NANO
      0 B        0 B          0 B          5    CoreAnimation
      0 B        0 B          0 B         43    IOSurface
  7793 KB    3088 KB          0 B       4743    TOTAL

```

