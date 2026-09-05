# Ferryx memory baseline — 2026-09-05

Collection started: 2026-09-05T03:23:00.498Z

## Scope and interpretation

Read-only observation of the already-running installed app. No app launch, restart, UI automation, debugger injection, source edits, or process termination. This is not the controlled debug reproduction baseline. No repeated-action growth rate or root cause has been established. Native view count is an allocation count, not independently verified visible pane count.

- App PID: 70940; started Sep 5 12:18:20 local time.
- Executable: /Applications/Ferryx.app/Contents/MacOS/ferryx.
- Footprint: 922 MB in footprint; 919.0M in vmmap and heap (separate snapshots).
- IOSurface: 397 MB dirty in footprint, 159 regions.
- Unmapped graphics footprint: 411 MB.
- FerryxNativeTerminalView: 6 objects.
- WgpuObserverLayer: 34 objects (excluding separate _priv allocation row).
- CAImageQueue / CA::Render::ImageQueue / FPCAMetalLayerState: 34 each.
- Two daemon processes observed: 45506 and 64274. Their roles were not established; neither was terminated.

## Next checkpoint

User launches the debug app with exactly bun tauri dev and confirms it is ready. Collect a fresh baseline tied to its PID and start time before requesting any repeated UI action. Do not compare absolute memory across different app PIDs/builds as a leak slope. User performs desktop actions manually. Keep pane/session count, window size, and display scale stable for tab-switch experiments.

## Same-process follow-up

Measured at 2026-09-05T04:25:08.680Z; PID 84403, same start time Sep 5 12:33:01. No UI automation or controlled ten-round-trip experiment was performed. Intervening user actions and session count are unverified.

- Footprint: 1027 -> 3303 MB (+2276 MB).
- IOSurface dirty: 447 -> 2607 MB (+2160 MB); regions 171 -> 662.
- Unmapped graphics footprint: 412 -> 402 MB.
- WgpuObserverLayer, CAImageQueue, FPCAMetalLayerState: 35 -> 198 each (+163).
- Native view objects: 6 -> 4.
- This establishes same-process growth beyond the startup snapshot, concentrated in IOSurface, with observer layers increasing despite fewer native views. It does not establish which operation causes growth or prove the exact ownership defect.
- A tauri dev runner was also observed, but this measurement targets only the existing installed app PID 84403.

### Follow-up: footprint 84403

Exit: 0

```text
ferryx [84403]: 64-bit    Footprint: 3303 MB (16384 bytes per page)
2607 MB        0 B        10 MB        662    IOSurface
 402 MB        0 B          0 B        114    Owned physical footprint (unmapped) (graphics)
  24 MB        0 B          0 B        208    Owned physical footprint (unmapped)
    phys_footprint: 3303 MB
    phys_footprint_peak: 3348 MB
```

### Follow-up: vmmap -summary 84403

Exit: 0

```text
Physical footprint:         3.2G
Physical footprint (peak):  3.3G
IOSurface                          2.6G     2.5G   294.0M     2.3G       0K     2.5G    10.2M      662 
```

### Follow-up: heap 84403

Exit: 0

```text
Physical footprint:         3.2G
Physical footprint (peak):  3.3G
     629     281792     448.0   IOSurface._impl (malloc)                          C       IOSurface
     629      10064      16.0   IOSurface                                         ObjC    IOSurface
     198      76032     384.0   CAImageQueue                                      CFType  QuartzCore
     198      63360     320.0   WgpuObserverLayer@0x102758b60._priv (malloc)      C       QuartzCore
     198      44352     224.0   FPCAMetalLayerState                               ObjC    FramePacing
     198       9504      48.0   IOSurfaceSharedEventListener._notificationPort (struct IONotificationPort)  C       IOSurface
     198       9504      48.0   WgpuObserverLayer@0x102758b60                     ObjC    QuartzCore
     198       6336      32.0   IOSurfaceSharedEventListener                      ObjC    IOSurface
      48       1536      32.0   CAIOSurface                                       CFType  QuartzCore
       8        384      48.0   IOSurfaceSharedEvent                              ObjC    IOSurface
       4       2560     640.0   FerryxNativeTerminalView                          ObjC    ferryx
       1         48      48.0   WTF::Detail::CallableWrapper<WTF::RunLoop::Timer::Timer<WebCore::IOSurfacePool>(WTF::Ref<WTF::RunLoop, WTF::RawPtrTraits<WTF::RunLoop>, WTF::DefaultRefDerefTraits<WTF::RunLoop>>&&, WTF::ASCIILiteral, WebCore::IOSurfacePool*, void (WebCore::IOSurfacePool::*)())::'lambda'(), void>  C++     WebCore
```

## Restart checkpoint

Collection started: 2026-09-05T03:35:46.627Z. Installed app PID 84403, started Sep 5 12:33:01 local time; executable /Applications/Ferryx.app/Contents/MacOS/ferryx. This is not a debug launch. Daemon PID 84429 (parent 84403); previous app and daemon PIDs absent from the process scan.

- Footprint: 1027 MB; peak 1131 MB.
- IOSurface dirty: 447 MB across 171 regions.
- Unmapped graphics footprint: 412 MB.
- Native views: 6; WgpuObserverLayer: 35; CAImageQueue and FPCAMetalLayerState: 35 each.
- Compared with prior PID: footprint 922 to 1027 MB, IOSurface 397 to 447 MB, views 6 to 6, observer layers 34 to 35. These are different process lifetimes and uncontrolled action histories, not a measured leak slope.
- No reproduction actions were performed by the agent. Startup restoration versus subsequent UI actions remains unresolved.

### Restart: footprint 84403

Exit code: 0

```text
ferryx [84403]: 64-bit    Footprint: 1027 MB (16384 bytes per page)
 447 MB        0 B          0 B        171    IOSurface
 412 MB        0 B          0 B        166    Owned physical footprint (unmapped) (graphics)
  40 MB        0 B          0 B        425    Owned physical footprint (unmapped)
    phys_footprint: 1027 MB
    phys_footprint_peak: 1131 MB
```

### Restart: vmmap -summary 84403

Exit code: 0

```text
Physical footprint:         1.0G
Physical footprint (peak):  1.1G
IOSurface                        550.8M   446.8M    58.5M   388.3M       0K   446.8M       0K      171 
```

### Restart: heap 84403

Exit code: 0

```text
Physical footprint:         1.0G
Physical footprint (peak):  1.1G
     152      68096     448.0   IOSurface._impl (malloc)                          C       IOSurface
     152       2432      16.0   IOSurface                                         ObjC    IOSurface
      51       1632      32.0   CAIOSurface                                       CFType  QuartzCore
      35      22400     640.0   CA::Render::ImageQueue                            C++     QuartzCore
      35      13440     384.0   CAImageQueue                                      CFType  QuartzCore
      35      11200     320.0   WgpuObserverLayer@0x102758b60._priv (malloc)      C       QuartzCore
      35       7840     224.0   FPCAMetalLayerState                               ObjC    FramePacing
      35       1680      48.0   IOSurfaceSharedEventListener._notificationPort (struct IONotificationPort)  C       IOSurface
      35       1680      48.0   WgpuObserverLayer@0x102758b60                     ObjC    QuartzCore
      35       1120      32.0   IOSurfaceSharedEventListener                      ObjC    IOSurface
      10        480      48.0   IOSurfaceSharedEvent                              ObjC    IOSurface
       6       3840     640.0   FerryxNativeTerminalView                          ObjC    ferryx
       1         48      48.0   WTF::Detail::CallableWrapper<WTF::RunLoop::Timer::Timer<WebCore::IOSurfacePool>(WTF::Ref<WTF::RunLoop, WTF::RawPtrTraits<WTF::RunLoop>, WTF::DefaultRefDerefTraits<WTF::RunLoop>>&&, WTF::ASCIILiteral, WebCore::IOSurfacePool*, void (WebCore::IOSurfacePool::*)())::'lambda'(), void>  C++     WebCore
```

## Raw command evidence

### footprint 70940

Exit code: 0

```text
======================================================================
ferryx [70940]: 64-bit    Footprint: 922 MB (16384 bytes per page)
======================================================================

  Dirty      Clean  Reclaimable    Regions    Category
    ---        ---          ---        ---    ---
 411 MB        0 B          0 B        155    Owned physical footprint (unmapped) (graphics)
 397 MB        0 B      1040 KB        159    IOSurface
  46 MB        0 B      9840 KB         22    MALLOC_SMALL
  26 MB        0 B          0 B        237    Owned physical footprint (unmapped)
  19 MB        0 B       208 KB         43    app-specific tag 1
6848 KB        0 B          0 B        226    IOAccelerator (graphics)
3600 KB        0 B       752 KB          6    WebKit malloc
2332 KB        0 B          0 B        882    __DATA_DIRTY
1984 KB        0 B        32 KB         73    stack
1408 KB        0 B          0 B         12    MALLOC metadata
1353 KB        0 B          0 B        978    __DATA
1025 KB        0 B          0 B          1    page table
 992 KB        0 B          0 B         67    CoreAnimation
 880 KB        0 B          0 B       1034    __DATA_CONST
 451 KB        0 B          0 B       2530    unused dyld shared cache area
 432 KB        0 B          0 B         82    untagged (VM_ALLOCATE)
 425 KB        0 B          0 B        639    __AUTH
 352 KB        0 B          0 B          8    IOAccelerator
 352 KB        0 B          0 B          1    MALLOC_TINY
 176 KB        0 B          0 B          3    CoreUI image data
 144 KB        0 B          0 B         10    IOKit
 112 KB        0 B          0 B          4    MALLOC_NANO
 112 KB        0 B          0 B       1026    __AUTH_CONST
 112 KB        0 B          0 B          2    __TPRO_CONST
  64 KB        0 B          0 B          5    Foundation
  48 KB        0 B          0 B          1    Activity Tracing
  48 KB        0 B          0 B          3    CoreGraphics
  16 KB        0 B          0 B          1    os_alloc_once
  16 KB        0 B          0 B          1    ColorSync
    0 B        0 B        32 KB          4    ImageIO
    0 B      29 MB          0 B         71    mapped file
    0 B      16 MB          0 B       1057    __TEXT
    0 B     112 KB          0 B          6    __LINKEDIT
    0 B        0 B          0 B          1    __GLSLBUILTINS
    0 B        0 B          0 B          1    __FONT_DATA
    0 B        0 B          0 B         32    JS VM Gigacage
    0 B        0 B          0 B          1    __CTF
    ---        ---          ---        ---    ---
 922 MB      45 MB        12 MB       9386    TOTAL

Auxiliary data:
    phys_footprint: 922 MB
    phys_footprint_peak: 941 MB


```

### vmmap -summary 70940

Exit code: 0

```text
Process:         ferryx [70940]
Path:            /Applications/Ferryx.app/Contents/MacOS/ferryx
Load Address:    0x10019c000
Identifier:      com.ferryx.app
Version:         2026.902.2 (2026.902.2)
Code Type:       ARM64
Platform:        macOS
Parent Process:  launchd [1]
Target Type:     live task

Date/Time:       2026-09-05 12:23:00.508 +0900
Launch Time:     2026-09-05 12:18:20.180 +0900
OS Version:      macOS 26.6 (25G72)
Report Version:  7
Analysis Tool:   /usr/bin/vmmap

Physical footprint:         919.0M
Physical footprint (peak):  941.4M
Idle exit:                  untracked
----

ReadOnly portion of Libraries: Total=1.8G resident=532.9M(29%) swapped_out_or_unallocated=1.3G(71%)
Writable regions: Total=5.3G written=521.1M(10%) resident=884.5M(16%) swapped_out=397.5M(7%) unallocated=4.1G(77%)

                                VIRTUAL RESIDENT    DIRTY  SWAPPED VOLATILE   NONVOL    EMPTY   REGION 
REGION TYPE                        SIZE     SIZE     SIZE     SIZE     SIZE     SIZE     SIZE    COUNT (non-coalesced) 
===========                     ======= ========    =====  ======= ========   ======    =====  ======= 
Activity Tracing                   256K      48K      48K       0K       0K      48K       0K        1 
ColorSync                           16K       0K       0K      16K       0K       0K       0K        1 
CoreAnimation                     1072K     544K     224K     768K       0K     544K       0K       67 
CoreGraphics                        48K      32K      32K      16K       0K       0K       0K        3 
CoreUI image data                  176K       0K       0K     176K       0K       0K       0K        3 
Foundation                         144K      64K      16K      48K       0K      48K      32K        5 
IOAccelerator                      352K     352K     352K       0K       0K       0K       0K        8 
IOAccelerator (graphics)          19.1M    6848K    5648K    1200K       0K    6848K    12.3M      226 
IOKit                              160K     144K     144K       0K       0K      16K       0K       10 
IOSurface                        433.2M   397.2M    42.3M   354.8M       0K   397.2M    1040K      159 
Image IO                            64K       0K       0K       0K       0K       0K      64K        4 
JS VM Gigacage (reserved)          4.0G       0K       0K       0K       0K       0K       0K        1         reserved VM address space (unallocated)
Kernel Alloc Once                   32K      16K      16K       0K       0K       0K       0K        1 
MALLOC                            1024K      32K      32K       0K       0K       0K       0K        1 
MALLOC guard page                 3760K       0K       0K       0K       0K       0K       0K        4 
MALLOC metadata                   1872K     544K     544K     864K       0K       0K       0K        8 
MALLOC_NANO metadata               128K     112K     112K       0K       0K       0K       0K        4         see MALLOC ZONE table below
MALLOC_SMALL                      83.5M    38.4M    28.8M    17.0M       0K       0K       0K       93         see MALLOC ZONE table below
MALLOC_SMALL (empty)              4608K      16K       0K     192K       0K       0K       0K       21         see MALLOC ZONE table below
MALLOC_TINY                       4096K     240K     240K     112K       0K       0K       0K        1         see MALLOC ZONE table below
Memory Tag 240                    38.2M    2160K    1952K    17.5M       0K       0K       0K       43 
STACK GUARD                        544K       0K       0K       0K       0K       0K       0K       34 
Stack                             63.6M    1632K    1600K     384K       0K       0K       0K       36 
Stack (reserved)                   544K       0K       0K       0K       0K       0K       0K        1         reserved VM address space (unallocated)
Stack Guard                       56.0M       0K       0K       0K       0K       0K       0K        2 
VM_ALLOCATE                        624K      32K      32K      16K       0K       0K       0K       31 
VM_ALLOCATE (reserved)            3328K       0K       0K       0K       0K       0K       0K       26         reserved VM address space (unallocated)
WebKit Malloc                     32.0M    3664K    2928K     512K       0K       0K       0K        2 
WebKit Malloc (reserved)          64.0M       0K       0K       0K       0K       0K       0K        1         reserved VM address space (unallocated)
WebKit Malloc metadata           160.0M     304K     288K       0K       0K       0K       0K        3 
__AUTH                            6011K    2455K     232K     192K       0K       0K       0K      639 
__AUTH_CONST                      89.1M    38.0M      32K      80K       0K       0K       0K     1026 
__CTF                               824      824       0K       0K       0K       0K       0K        1 
__DATA                            34.9M    9714K     933K     420K       0K       0K       0K      978 
__DATA_CONST                      35.3M    20.1M     544K     336K       0K       0K       0K     1034 
__DATA_DIRTY                      8388K    3501K    1553K     779K       0K       0K       0K      882 
__FONT_DATA                        2352     2352       0K       0K       0K       0K       0K        1 
__GLSLBUILTINS                    5174K       0K       0K       0K       0K       0K       0K        1 
__LINKEDIT                       575.4M    23.1M       0K       0K       0K       0K       0K        5 
__OBJC_RO                         79.2M    52.5M       0K       0K       0K       0K       0K        1 
__OBJC_RW                         2599K    2231K      39K      16K       0K       0K       0K        1 
__TEXT                             1.2G   509.8M       0K       0K       0K       0K       0K     1057 
__TPRO_CONST                       128K      32K      32K      80K       0K       0K       0K        2 
mapped file                      677.2M    29.3M       0K       0K       0K       0K       0K       69 
owned unmapped                    28.5M    25.4M    22.5M     240K       0K       0K       0K      233 
owned unmapped (graphics)        424.9M   406.5M    22.5M    4448K       0K       0K       0K      155 
page table in kernel              1025K    1025K    1025K       0K       0K       0K       0K        1 
shared memory                      976K     208K     208K     144K       0K       0K       0K       20 
unused but dirty shlib __DATA      459K     218K     218K     241K       0K       0K       0K      342 
===========                     ======= ========    =====  ======= ========   ======    =====  ======= 
TOTAL                              8.1G     1.5G   134.7M   400.3M       0K   404.5M    13.4M     7248 
TOTAL, minus reserved VM space     4.0G     1.5G   134.7M   400.3M       0K   404.5M    13.4M     7248 

                                          VIRTUAL   RESIDENT      DIRTY    SWAPPED ALLOCATION      BYTES DIRTY+SWAP          REGION
MALLOC ZONE                                  SIZE       SIZE       SIZE       SIZE      COUNT  ALLOCATED  FRAG SIZE  % FRAG   COUNT
===========                               =======  =========  =========  =========  =========  =========  =========  ======  ======
WebKit Malloc_0x1144ee5f8                  192.0M      3968K      3216K       496K       2228       329K      3383K     92%       4
DefaultMallocZone_0x101e9c000               91.6M      38.6M      29.0M      17.2M     145414      45.9M       255K      1%      75
QuartzCore_0x104784000                      1104K       656K       592K        32K       2361       234K       390K     63%      37
AttributeGraph_0xb21168fc0                  1024K        32K        32K         0K        720        25K         7K     22%       1
LSBindingEvaluator_0x105488000               256K         0K         0K       176K          0         0K       176K    100%       7
AttributeGraph graph data_0x110d34000         32K        32K        32K         0K          9       2496        30K     93%       2
DefaultPurgeableMallocZone_0x127394000         0K         0K         0K         0K          0         0K         0K      0%       0
WebKit Using System Malloc_0x111294000         0K         0K         0K         0K          0         0K         0K      0%       0
===========                               =======  =========  =========  =========  =========  =========  =========  ======  ======
TOTAL                                      286.0M      43.1M      32.7M      17.9M     150732      46.5M      4240K      9%     126


```

### heap 70940

Exit code: 0

```text
Process:         ferryx [70940]
Path:            /Applications/Ferryx.app/Contents/MacOS/ferryx
Load Address:    0x10019c000
Identifier:      com.ferryx.app
Version:         2026.902.2 (2026.902.2)
Code Type:       ARM64
Platform:        macOS
Parent Process:  launchd [1]
Target Type:     live task

Date/Time:       2026-09-05 12:23:00.510 +0900
Launch Time:     2026-09-05 12:18:20.180 +0900
OS Version:      macOS 26.6 (25G72)
Report Version:  7
Analysis Tool:   /usr/bin/heap

Physical footprint:         919.0M
Physical footprint (peak):  941.4M
Idle exit:                  untracked
----

Process 70940: 8 zones

All zones: 150732 nodes malloced - Sizes: 2048KB[5] 528KB[1] 128KB[3] 96KB[2] 80KB[2] 64KB[6] 48KB[42] 40KB[1] 32KB[2] 28KB[2] 24KB[41] 20KB[58] 16.5KB[1] 16KB[81] 14KB[7] 13KB[1] 12.5KB[1] 12KB[180] 10.5KB[2] 10KB[44] 8KB[168] 7KB[83] 6KB[30] 5KB[409] 4.5KB[6] 4KB[367] 3.5KB[217] 3KB[435] 2.5KB[227] 2KB[1718] 1.75KB[779] 1.5KB[805] 1.4375KB[1] 1.25KB[254] 1.04688KB[2] 1KB[742] 1008[6] 896[672] 848[1] 768[763] 640[2250] 592[1] 576[3] 544[2] 528[26] 512[737] 496[1] 448[1687] 432[1] 416[9] 384[928] 352[13] 320[958] 304[2] 288[7] 256[1480] 228[3] 224[2216] 208[123] 192[4025] 160[3249] 144[3] 132[3] 128[3581] 112[3796] 110[3] 108[6] 104[3] 96[3488] 91[3] 88[3] 85[18] 84[6] 80[5142] 76[3] 74[3] 72[18] 67[15] 65[3] 64[17006] 63[3] 60[15] 58[3] 56[9] 53[3] 52[15] 50[3] 48[23945] 47[3] 44[6] 43[3] 40[9] 36[12] 34[3] 33[6] 32[43195] 31[3] 30[3] 28[63] 24[51] 20[6] 17[9] 16[24235] 12[6] 10[3] 8[93] 4[27] 1[9] 

Found:  1614 ObjC classes  447 Swift classes  333 C++ classes  266 CFTypes
Type names for non-objects could be derived from allocation backtraces if the process used MallocStackLogging

-----------------------------------------------------------------------
All zones: 150732 nodes (48774664 bytes)

   COUNT      BYTES       AVG   CLASS_NAME                                        TYPE    BINARY
   =====      =====       ===   ==========                                        ====    ======
   93066   38277243     411.3   non-object                                                
   13048     683568      52.4   CFString                                          ObjC    CoreFoundation
    3810     121920      32.0   Class.data (class_rw_t)                           C       libobjc.A.dylib
    2192      70144      32.0   NSMutableDictionary                               ObjC    CoreFoundation
    2028     521904     257.3   NSMutableDictionary (Storage)                     C       CoreFoundation
    1541     526656     341.8   Class.methodCache._buckets (bucket_t)             C       libobjc.A.dylib
    1373     219584     159.9   CGPath                                            CFType  CoreGraphics
    1159      59584      51.4   __NSMallocBlock__                                 ObjC    libsystem_blocks.dylib
    1157     740480     640.0   NSCTFont                                          ObjC    UIFoundation
    1132      54336      48.0   NSMutableArray                                    ObjC    CoreFoundation
     986      85216      86.4   NSMutableArray (Storage)                          C       CoreFoundation
     739      32448      43.9   NSArray                                           ObjC    CoreFoundation
     689     221968     322.2   NSDictionary                                      ObjC    CoreFoundation
     549      35136      64.0   pthread_mutex_t                                   C       libpthread.dylib
     541      17312      32.0   NSKeyValueDependency                              ObjC    AppKit
     528      33792      64.0   CFDictionary                                      ObjC    CoreFoundation
     503       8048      16.0   NSArray                                           ObjC    CoreFoundation
     491      31424      64.0   OS_os_log                                         ObjC    libsystem_trace.dylib
     475      15200      32.0   NSDictionary                                      ObjC    CoreFoundation
     474     165584     349.3   NSDictionary (Storage)                            C       CoreFoundation
     474       7584      16.0   NSDictionary.cow (struct __cow_state_t)           C       CoreFoundation
     457      51184     112.0   SVGAttribute                                      C++     CoreSVG
     457      14624      32.0   SVGAttribute                                      CFType  <unknown>
     442      21216      48.0   FPDurationStatistics                              ObjC    FramePacing
     429      20592      48.0   CFSet                                             ObjC    CoreFoundation
     425      45408     106.8   NSURL                                             ObjC    CoreFoundation
     411      38288      93.2   NSSet                                             ObjC    CoreFoundation
     404      22784      56.4   Class.data.methods.lists (method_list_t)          C       libobjc.A.dylib
     396      26928      68.0   CFSet (Value Storage)                             C       CoreFoundation
     387      18576      48.0   Class.data.extended (class_rw_ext_t)              C       libobjc.A.dylib
     375     248832     663.6   CFDictionary (Value Storage)                      C       CoreFoundation
     355      14336      40.4   __NSExactBlockVariable__                          ObjC    libsystem_blocks.dylib
     349      44672     128.0   dispatch_queue_t (serial)                         ObjC    libdispatch.dylib
     318      10176      32.0   NSNumber                                          ObjC    CoreFoundation
     308      14784      48.0   NSMutableCharacterSet                             ObjC    CoreFoundation
     303      72496     239.3   CFData                                            ObjC    CoreFoundation
     301     192640     640.0   TBaseFont                                         C++     CoreText
     291      23280      80.0   _WKUserInitiatedAction                            ObjC    WebKit
     237       8032      33.9   NSConcreteValue                                   ObjC    Foundation
     236       7552      32.0   NSKeyBindingAtom                                  ObjC    AppKit
     220     253440    1152.0   AGX::HAL200::Texture                              C++     AGXMetalG16X
     218      17440      80.0   NSAutoresizingMaskLayoutConstraint                ObjC    CoreAutoLayout
     197     147264     747.5   CFDictionary (Key Storage)                        C       CoreFoundation
     191      15280      80.0   Class.data.readonly (class_ro_t)                  C       libobjc.A.dylib
     189      14352      75.9   CFArray                                           ObjC    CoreFoundation
     184       5888      32.0   NSConcreteData                                    ObjC    Foundation
     178     101568     570.6   CFDictionary (Weak Key Storage)                   C       CoreFoundation
     177      11328      64.0   NSAccessibilityAttributeAccessorInfo              ObjC    AppKit
     174       5568      32.0   NSMethodSignature                                 ObjC    CoreFoundation
     174       5568      32.0   NSMethodSignature._frameDescriptor (struct NSMethodFrameDescriptor)  C       CoreFoundation
     163      10944      67.1   CFString (Storage)                                C       CoreFoundation
     153      17136     112.0   CUIRenditionKey                                   ObjC    CoreUI
     152       9728      64.0   _MTLLibrary                                       ObjC    Metal
     152       7296      48.0   NSKeyValueObservance                              ObjC    Foundation
     152       2432      16.0   NSSet                                             ObjC    CoreFoundation
     151      57984     384.0   IOGPUMetalPooledResource                          ObjC    IOGPU
     145       4640      32.0   __NSObserver                                      ObjC    Foundation
     144       4608      32.0   NSKeyValueDependencyContext                       ObjC    AppKit
     141      18048     128.0   dispatch_source_t                                 ObjC    libdispatch.dylib
     138       8736      63.3   Class (objc_class)                                C       libobjc.A.dylib
     137      61376     448.0   CA::Render::Surface                               C++     QuartzCore
     137      61376     448.0   IOSurface._impl (malloc)                          C       IOSurface
     137       2192      16.0   IOSurface                                         ObjC    IOSurface
     135       4320      32.0   NSMutableSet                                      ObjC    CoreFoundation
     132      16896     128.0   NSMapTable                                        ObjC    Foundation
     128      28672     224.0   APComponent_FromBundle_Loadable                   C++     AudioToolboxCore
     128       4096      32.0   std::__shared_ptr_pointer<APComponent_FromBundle_Loadable*, std::shared_ptr<APComponent_FromBundle_Loadable>::__shared_ptr_default_delete<APComponent_FromBundle_Loadable, APComponent_FromBundle_Loadable>>  C++     AudioToolboxCore
     126      10080      80.0   dispatch_semaphore_t                              ObjC    libdispatch.dylib
     126       4032      32.0   __CFPrefsWeakObservers                            ObjC    CoreFoundation
     124       7936      64.0   NSKeyValueDependencyInfo                          ObjC    AppKit
     119       8448      71.0   NSMapTable (Key Storage)                          C       Foundation
     118       9440      80.0   SVGAttributeMap                                   C++     CoreSVG
     118       3776      32.0   SVGAttributeMap                                   CFType  <unknown>
     115      18400     160.0   NSMenuItem                                        ObjC    AppKit
     115       3680      32.0   CGDisplayMode                                     CFType  SkyLight
     112       8000      71.4   CGRegion                                          CFType  CoreGraphics
     111      12432     112.0   NSKeyValueSlowMutableCollectionGetter             ObjC    Foundation
     111       3552      32.0   NSArray                                           ObjC    CoreFoundation
     111       2544      22.9   NSArray._list (id[])                              C       CoreFoundation
     110      70400     640.0   AGXG16XFamilyTexture                              ObjC    AGXMetalG16X
     106       6784      64.0   NSLayoutDimension                                 ObjC    CoreAutoLayout
     105       1680      16.0   NSMutableDictionary.cow (struct __cow_state_t)    C       CoreFoundation
     104       7136      68.6   NSMapTable (Value Storage)                        C       Foundation
      99      19008     192.0   CFRunLoopSource                                   CFType  CoreFoundation
      97      43456     448.0   _NSViewLayoutAux                                  ObjC    AppKit
      96       4608      48.0   _NSTrackingAreaAKViewHelper                       ObjC    AppKit
      89      22784     256.0   NSExtraMIData                                     ObjC    AppKit
      89      11392     128.0   CFPrefsPlistSource                                ObjC    CoreFoundation
      85       2720      32.0   NSDictionary                                      ObjC    CoreFoundation
      84       9408     112.0   BSObjCValue                                       ObjC    BaseBoard
      84       6720      80.0   std::__shared_ptr_emplace<AGX::FunctionTableSet<AGXG16XFamilyUserIntersectionFunctionTable>>  C++     AGXMetalG16X
      84       6720      80.0   std::__shared_ptr_emplace<AGX::FunctionTableSet<AGXG16XFamilyVisibleFunctionTable>>  C++     AGXMetalG16X
      82       6096      74.3   NSMutableSet (Storage)                            C       CoreFoundation
      77       2464      32.0   NSKeyValueObservationInfo                         ObjC    Foundation
      76       5136      67.6   xpc_dictionary_t (Storage)                        C       libxpc.dylib
      75      28544     380.6   NSData                                            ObjC    Foundation
      74       5088      68.8   CUINamedRenditionInfo._bitmap (malloc)            C       CoreUI
      74       2368      32.0   CUINamedRenditionInfo                             ObjC    CoreUI
      73      14944     204.7   Class.data.methods (method_array_t)               C       libobjc.A.dylib
      73       3504      48.0   CALayerArray                                      ObjC    QuartzCore
      72     294912    4096.0   AGXG16XFamilyRenderPipeline                       ObjC    AGXMetalG16X
      72       5760      80.0   dispatch_group_t                                  ObjC    libdispatch.dylib
      71       2272      32.0   NSTaggedPointerStringCStringContainer             ObjC    CoreFoundation
      70      13440     192.0   MudaMenuItem                                      ObjC    ferryx
      70       3360      48.0   NSKeyValueUnnestedProperty                        ObjC    Foundation
      68       5248      77.2   Swift.StringStorage                               Swift   libswiftCore.dylib
      67      10720     160.0   dispatch_mach_t                                   ObjC    libdispatch.dylib
      64      20480     320.0   CGImage                                           CFType  CoreGraphics
      64       5120      80.0   NSHashTable                                       ObjC    Foundation
      62       1984      32.0   _NSXPCConnectionClassCache                        ObjC    Foundation
      60      19264     321.1   xpc_connection_t                                  ObjC    libxpc.dylib
      60       4800      80.0   NSKeyValueMethodGetter                            ObjC    Foundation
      60       3840      64.0   _NSAutoresizingMaskXAxisAnchor                    ObjC    CoreAutoLayout
      59      11360     192.5   NSHashTable (Weak Object Storage)                 C       Foundation
      59       3776      64.0   _NSAutoresizingMaskYAxisAnchor                    ObjC    CoreAutoLayout
      58       7424     128.0   CFPrefsManagedSource                              ObjC    CoreFoundation
      57      29184     512.0   TFPFont                                           C++     libFontParser.dylib
      56      25088     448.0   AGXG16XFamilyBuffer                               ObjC    AGXMetalG16X
      56      17920     320.0   NSCache._cache (struct cache_s)                   C       CoreFoundation
      56       2688      48.0   NSCache                                           ObjC    CoreFoundation
      55       4400      80.0   NSPointerArray                                    ObjC    Foundation
      55       2640      48.0   NSViewBackingLayer                                ObjC    AppKit
      54       8640     160.0   IOGPUMetalDeviceShmem                             ObjC    IOGPU
      54       6912     128.0   CFPrefsSuiteSearchListSource                      ObjC    CoreFoundation
      53       6784     128.0   CGFont                                            CFType  CoreGraphics
      52      33280     640.0   NSMenuBarItemView                                 ObjC    AppKit
      52       3328      64.0   NSXPCInterface                                    ObjC    Foundation
      51       2368      46.4   Closure context                                   Swift   <unknown>
      48       1136      23.7   _NSKVODeallocSentinel                             ObjC    Foundation
      46      11776     256.0   _NSServiceEntry                                   ObjC    AppKit
      44       8448     192.0   TGlyphOutlineDictionaryCache<unsigned short, 64ul, 512ul>  C++     libFontParser.dylib
      44       5808     132.0   WKObject                                          ObjC    WebKit
      44       4928     112.0   IOGPUMetalResourcePool                            ObjC    IOGPU
      44       4928     112.0   IOGPUMetalResourcePool._resourceArgs (struct IOGPUNewResourceArgs)  C       IOGPU
      44        704      16.0   NSMutableArray.cow (struct __cow_state_t)         C       CoreFoundation
      43     263968    6138.8   CFData (Bytes Storage)                            C       CoreFoundation
      43       4656     108.3   CGColor                                           CFType  CoreGraphics
      43       1376      32.0   SVGNode                                           CFType  <unknown>
      42      37632     896.0   TOpenTypeCIDDataForkFont                          C++     libFontParser.dylib
      42       2016      48.0   NSPointerArray (Object Storage)                   C       Foundation
      40       3840      96.0   _NSServiceFilter                                  ObjC    AppKit
      39       6976     178.9   CGImageProvider                                   CFType  CoreGraphics
      39       3744      96.0   NSKeyValueMethodSetter                            ObjC    Foundation
      39       3120      80.0   NSKeyValueSlowSetter                              ObjC    Foundation
      39       2496      64.0   CA::Render::Shmem                                 C++     QuartzCore
      39       2496      64.0   _NSUndoBeginMark                                  ObjC    Foundation
      39       1872      48.0   _NSUndoLightInvocation                            ObjC    Foundation
      39       1248      32.0   CAIOSurface                                       CFType  QuartzCore
      39       1248      32.0   _NSUndoEndMark                                    ObjC    Foundation
      39        624      16.0   WKEditCommand                                     ObjC    WebKit
      38       2432      64.0   CFBag                                             CFType  CoreFoundation
      36       1152      32.0   NSUUID                                            Swift   Foundation
      35       6720     192.0   TTrueTypeScaler                                   C++     libFontParser.dylib
      35       1680      48.0   cacheStrike                                       C++     libFontParser.dylib
      35       1120      32.0   std::__shared_ptr_pointer<MCacheData*, std::shared_ptr<MCacheData>::__shared_ptr_default_delete<MCacheData, MCacheData>>  C++     libFontParser.dylib
      34      21760     640.0   CA::Render::ImageQueue                            C++     QuartzCore
      34      17408     512.0   AGXG16XFamilyResidencySet._hashTable (struct)     C       AGXMetalG16X
      34      13056     384.0   CAImageQueue                                      CFType  QuartzCore
      34      10880     320.0   WgpuObserverLayer@0x101954b30._priv (malloc)      C       QuartzCore
      34       8704     256.0   AGXG16XFamilyResidencySet                         ObjC    AGXMetalG16X
      34       7616     224.0   CFRunLoopMode                                     CFType  CoreFoundation
      34       7616     224.0   FPCAMetalLayerState                               ObjC    FramePacing
      34       4352     128.0   OS_dispatch_queue_runloop                         ObjC    libdispatch.dylib
      34       4352     128.0   THVARTable                                        C++     libFontParser.dylib
      34       2720      80.0   FPOnGlassCAMetalLayerDrawableInterval             ObjC    FramePacing
      34       2176      64.0   SVGPathCommand                                    C++     CoreSVG
      34       1632      48.0   IOSurfaceSharedEventListener._notificationPort (struct IONotificationPort)  C       IOSurface
      34       1632      48.0   WgpuObserverLayer@0x101954b30                     ObjC    QuartzCore
      34       1088      32.0   IOSurfaceSharedEventListener                      ObjC    IOSurface
      34       1088      32.0   SVGPathCommand                                    CFType  <unknown>
      34       1088      32.0   WKCompositingLayer                                ObjC    WebKit
      33    1622016   49152.0   MTLResourceList                                   ObjC    Metal
      33       4224     128.0   NSMachPort                                        ObjC    CoreFoundation
      33       1056      32.0   WPLinkFilteringConditionals                       ObjC    WebPrivacy
      32      10240     320.0   CGDataProvider                                    CFType  CoreGraphics
      32       8192     256.0   _NSMenuImpl                                       ObjC    AppKit
      32       2560      80.0   NSTextCheckingKeyEvent                            ObjC    Foundation
      32       2048      64.0   NSMenu                                            ObjC    AppKit
      31       4960     160.0   NSXPCConnection                                   ObjC    Foundation
      31       2480      80.0   _NSXPCConnectionExportedObjectTable               ObjC    Foundation
      31       1488      48.0   _NSXPCConnectionImportInfo                        ObjC    Foundation
      31       1488      48.0   _NSXPCConnectionRequestedReplies                  ObjC    Foundation
      31        992      32.0   _NSXPCConnectionExpectedReplies                   ObjC    Foundation
      30      26880     896.0   AGXG16XFamilyCommandBuffer                        ObjC    AGXMetalG16X
      30      19200     640.0   AGXG16XFamilyCommandBuffer._impl (malloc)         C       AGXMetalG16X
      30       3840     128.0   IIOReader_RawCamera                               C++     ImageIO
      30       3840     128.0   SVGPaint                                          C++     CoreSVG
      30       2416      80.5   NSMutableRLEArray.theList (struct _NSRefCountedRunArray)  C       Foundation
      30        960      32.0   SVGPaint                                          CFType  <unknown>
      30        480      16.0   NSMutableRLEArray                                 ObjC    Foundation
      29      18560     640.0   TInstanceFont                                     C++     CoreText
      29      12992     448.0   _MTLFunctionInternal                              ObjC    Metal
      29       1392      48.0   NSKeyValueContainerClass                          ObjC    Foundation
      29        928      32.0   _NSSwiftLocale                                    Swift   Foundation
      28      17920     640.0   Foundation.LockedState<Foundation._LocaleICU.State>.(_Buffer in $18348d314)<>  Swift   Foundation
      28       6272     224.0   _LocaleICU                                        Swift   Foundation
      28       5376     192.0   CA::Render::Image                                 C++     QuartzCore
      28       2816     100.6   NSMapTable (Weak Value Storage)                   C       Foundation
      28       1776      63.4   Closure context (unknown layout)                  Swift   <unknown>
      28        896      32.0   NSConcreteAttributedString                        ObjC    Foundation
      27       3456     128.0   CFPrefsSearchListSource                           ObjC    CoreFoundation
      26       5824     224.0   CFBundle                                          CFType  CoreFoundation
      26       4160     160.0   NSViewBackingLayerContents                        Swift   AppKit
      26       3312     127.4   NSPathStore2                                      ObjC    Foundation
      26       1664      64.0   BSObjCMethod                                      ObjC    BaseBoard
      26        832      32.0   NSDisplayCycleObserver                            ObjC    AppKit
      26        832      32.0   TSMInputSource                                    CFType  HIToolbox
      25     102400    4096.0   @autoreleasepool content                          C       libobjc.A.dylib
      25      12800     512.0   _CTNativeGlyphStorage                             ObjC    CoreText
      24      24576    1024.0   CGDisplayList                                     CFType  CoreGraphics
      24      10752     448.0   CTRun                                             CFType  CoreText
      24       7680     320.0   CTLine                                            CFType  CoreText
      24       6144     256.0   CG::DisplayListEntryGlyphs                        C++     CoreGraphics
      24       3840     160.0   std::__shared_ptr_emplace<CG::DisplayListShape>   C++     CoreGraphics
      24       3744     156.0   Swift._ContiguousArrayStorage<SwiftUI.DisplayList.Item>  Swift   libswiftCore.dylib
      24       3072     128.0   CG::DisplayListEntryStateDrawing                  C++     CoreGraphics
      24       2688     112.0   NSViewBackingLayerContents.ContentLayer           Swift   AppKit
      24       2688     112.0   Swift._DictionaryStorage<AppKit.DisplayList, AppKit.Region>  Swift   libswiftCore.dylib
      24       1920      80.0   NSKeyValueUndefinedSetter                         ObjC    Foundation
      24       1920      80.0   std::__shared_ptr_emplace<TCharStreamCFAttrString>  C++     CoreText
      24       1536      64.0   CG::DisplayListResourceClip                       C++     CoreGraphics
      24       1152      48.0   CG::DisplayListResourceColor                      C++     CoreGraphics
      24        768      32.0   CATintedImage                                     ObjC    QuartzCore
      24        768      32.0   CG::DisplayListResourceColorSpace                 C++     CoreGraphics
      24        768      32.0   CG::DisplayListResourceFont                       C++     CoreGraphics
      24        768      32.0   std::__shared_ptr_pointer<CG::DisplayListEntry const*, std::shared_ptr<CG::DisplayListEntry const>::__shared_ptr_default_delete<CG::DisplayListEntry const, CG::DisplayListEntry const>>  C++     CoreGraphics
      24        768      32.0   std::__shared_ptr_pointer<CG::DisplayListEntryStateDrawing*, std::shared_ptr<CG::DisplayListEntryStateDrawing const>::__shared_ptr_default_delete<CG::DisplayListEntryStateDrawing const, CG::DisplayListEntryStateDrawing>>  C++     CoreGraphics
      24        768      32.0   std::__shared_ptr_pointer<CG::DisplayListEntryStateFill*, std::shared_ptr<CG::DisplayListEntryStateFill const>::__shared_ptr_default_delete<CG::DisplayListEntryStateFill const, CG::DisplayListEntryStateFill>>  C++     CoreGraphics
      24        768      32.0   std::__shared_ptr_pointer<CG::DisplayListResourceClip*, std::shared_ptr<CG::DisplayListResourceClip>::__shared_ptr_default_delete<CG::DisplayListResourceClip, CG::DisplayListResourceClip>>  C++     CoreGraphics
      24        768      32.0   std::__shared_ptr_pointer<CG::DisplayListResourceColor*, std::shared_ptr<CG::DisplayListResourceColor>::__shared_ptr_default_delete<CG::DisplayListResourceColor, CG::DisplayListResourceColor>>  C++     CoreGraphics
      24        768      32.0   std::__shared_ptr_pointer<CG::DisplayListResourceColorSpace*, std::shared_ptr<CG::DisplayListResourceColorSpace>::__shared_ptr_default_delete<CG::DisplayListResourceColorSpace, CG::DisplayListResourceColorSpace>>  C++     CoreGraphics
      24        768      32.0   std::__shared_ptr_pointer<CG::DisplayListResourceFont*, std::shared_ptr<CG::DisplayListResourceFont>::__shared_ptr_default_delete<CG::DisplayListResourceFont, CG::DisplayListResourceFont>>  C++     CoreGraphics
      24        384      16.0   CG::DisplayListEntryStateFill                     C++     CoreGraphics
      24        384      16.0   MudaMenuDelegate                                  ObjC    ferryx
      24        384      16.0   SwiftUI.DisplayList                               Swift   SwiftUI
      23       8832     384.0   SVGShapeNode                                      C++     CoreSVG
      23       2208      96.0   ColorSyncTRC                                      CFType  ColorSync
      23        736      32.0   CALayer                                           ObjC    QuartzCore
      23        368      16.0   Class.data.protocols.lists (protocol_list_t)      C       libobjc.A.dylib
      22       5632     256.0   CUINamedVectorGlyph                               ObjC    CoreUI
      22       1760      80.0   IFImageSpecification                              ObjC    IconFoundation
      21       4704     224.0   TTrueTypeDataForkFont                             C++     libFontParser.dylib
      21       3360     160.0   Swift._DictionaryStorage<Swift.String, Swift.Optional<Swift.String>>  Swift   libswiftCore.dylib
      21       3360     160.0   _NSAppleMenuItem                                  ObjC    AppKit
      20       4480     224.0   TIKeyboardState                                   ObjC    TextInput
      20       1600      80.0   TIDocumentState                                   ObjC    TextInput
      20       1472      73.6   CALayerArray._ivars (struct)                      C       QuartzCore
      20       1280      64.0   _TUIGeneratorResultAccumulator                    ObjC    TextInputUI
      20        960      48.0   _TUIKeyboardCandidateGenerationContext            ObjC    TextInputUI
      20        640      32.0   TICandidateRequestToken                           ObjC    TextInput
      20        640      32.0   _TUIGeneratorResultAccumulatorPolicy              ObjC    TextInputUI
      20        640      32.0   _TUIKeyboardCandidateContainer                    ObjC    TextInputUI
      19       8224     432.8   NSArray (Storage)                                 C       CoreFoundation
      19       1216      64.0   NSKeyValueDependencyInfo._classInfo (struct NSDPClassInfo)  C       AppKit
      19        912      48.0   NSArray                                           ObjC    CoreFoundation
      19        304      16.0   NSArray.cow (struct __cow_state_t)                C       CoreFoundation
      18      18432    1024.0   _CUIInternalLinkRendition                         ObjC    CoreUI
      18      11328     629.3   Swift._DictionaryStorage<Swift.ObjectIdentifier, SwiftUI.(AnyTrackedValue in $22ef97d14)>  Swift   libswiftCore.dylib
      18       3456     192.0   CUIRenditionMetrics                               ObjC    CoreUI
      18       2880     160.0   CGPDFDictionary                                   CFType  CoreGraphics
      18       1530      85.0   SwiftUI.AnimatableFrameAttribute                  Swift   SwiftUI
      18       1440      80.0   NSBundle                                          ObjC    Foundation
      18        864      48.0   NSDynamicSystemColor                              ObjC    AppKit
      18        864      48.0   SwiftUI.ViewFrame                                 Swift   SwiftUI
      18        288      16.0   SwiftUI.LayoutComputer                            Swift   SwiftUI
      17     215168   12656.9   Swift Metadata                                    C       libswiftCore.dylib
      17      52224    3072.0   CFRunLoop                                         CFType  CoreFoundation
      17       3808     224.0   icu::UnicodeSet                                   C++     libicucore.A.dylib
      17       1792     105.4   xpc_dictionary_t                                  ObjC    libxpc.dylib
      17       1264      74.4   Swift._ContiguousArrayStorage<AppKit.NSMenuKeyCache.CacheEntry>  Swift   libswiftCore.dylib
      17        544      32.0   NSHIObject                                        ObjC    HIToolbox
      16       2560     160.0   NSImage                                           ObjC    AppKit
      16       1792     112.0   CUIRenditionSliceInformation                      ObjC    CoreUI
      16       1536      96.0   NSLock                                            ObjC    Foundation
      16       1280      80.0   _NSBPlistMappedData                               ObjC    Foundation
      16        512      32.0   FSNode                                            ObjC    LaunchServices
      15       3360     224.0   NSRecursiveLock                                   ObjC    Foundation
      15        960      64.0   SwiftUI.(AtomicBuffer in $22efa14ec)<SwiftUI.(TrackerData in $22ef98070)>  Swift   SwiftUI
      15        960      64.0   SwiftUI.ViewGeometry                              Swift   SwiftUI
      15        480      32.0   PropertyList.Tracker                              Swift   SwiftUICore
      15        480      32.0   Swift weak reference storage                      C       libswiftCore.dylib
      15        240      16.0   SwiftUI.EnvironmentValues                         Swift   SwiftUI
      14       6272     448.0   _CUIThemePixelRendition                           ObjC    CoreUI
      14       4480     320.0   _FileCache                                        CFType  CoreServicesInternal
      14       2688     192.0   NSSymbolImageRep                                  ObjC    AppKit
      14       2240     160.0   ColorSyncTransform                                CFType  ColorSync
      14       1792     128.0   CSIHelper                                         ObjC    CoreUI
      14       1792     128.0   TFileDataReference                                C++     libFontParser.dylib
      14       1568     112.0   _CUIThemeFacetCacheKey                            ObjC    CoreUI
      14       1344      96.0   CGColorSpace                                      CFType  CoreGraphics
      14       1120      80.0   CA::Render::Filter                                C++     QuartzCore
      14        672      48.0   CFBasicHash                                       CFType  CoreFoundation
      14        448      32.0   _NSSimpleLRUCacheKeyValuePair                     ObjC    AppKit
      13       5824     448.0   AGXBuffer                                         ObjC    AGXMetalG16X
      13       4352     334.8   ColorSyncProfile                                  CFType  ColorSync
      13       2080     160.0   ColorSyncTransformIterator                        CFType  ColorSync
      13       1504     115.7   NSMapTable (Weak Key Storage)                     C       Foundation
      13       1248      96.0   ColorSyncMatrix                                   CFType  ColorSync
      13       1040      80.0   __NSXPCInterfaceProxy_XTFontRegistryProtocol      ObjC    Foundation
      13        832      64.0   NSKVONotifying_WKUserDefaults                     ObjC    WebKit
      13        624      48.0   NSImageSymbolRepProvider                          ObjC    AppKit
      13        448      34.5   NSPointerArray (Weak Object Storage)              C       Foundation
      13        416      32.0   RBSDomainAttribute                                ObjC    RunningBoardServices
      13        416      32.0   _NSSimpleLRUCache                                 ObjC    AppKit
      12      17136    1428.0   NSConcreteMutableData (Bytes Storage)             C       Foundation
      12      10752     896.0   TTenuousComponentFont                             C++     CoreText
      12       4608     384.0   _CUIThemeSVGRendition                             ObjC    CoreUI
      12       1920     160.0   CGPDFArray                                        CFType  CoreGraphics
      12       1920     160.0   Swift._ContiguousArrayStorage<(key: SwiftUI._ShapeStyle_Pack.Key, style: SwiftUI._ShapeStyle_Pack.Style)>  Swift   libswiftCore.dylib
      12       1920     160.0   Swift._ContiguousArrayStorage<SwiftUI.KeyedAnimatableArray<SwiftUI._ShapeStyle_Pack.Key, SwiftUI.AnimatablePair<SwiftUI._ShapeStyle_Pack.Fill.AnimatableData, SwiftUI.AnimatablePair<Swift.Float, SwiftUI.AnimatableArray<SwiftUI.AnimatablePair<Swift.Float, SwiftUI._ShapeStyle_Pack.Effect.Kind.AnimatableData>>>>>.Element<>>  Swift   libswiftCore.dylib
      12        960      80.0   CUINamedImage                                     ObjC    CoreUI
      12        768      64.0   dispatch_data_t                                   ObjC    libdispatch.dylib
      12        576      48.0   CA::Render::String                                C++     QuartzCore
      12        576      48.0   CAFilter                                          ObjC    QuartzCore
      12        576      48.0   NSConcreteMutableData                             ObjC    Foundation
      12        576      48.0   NSISVariableObservation                           ObjC    CoreAutoLayout
      12        576      48.0   _UTDeclaredTypeRecord._resolvedProperties (malloc)  C       LaunchServices
      12        384      32.0   CA::Render::KeyValue                              C++     QuartzCore
      12        384      32.0   CA::Render::Vector                                C++     QuartzCore
      12        288      24.0   AttributeGraph.Map<SwiftUI.EnvironmentValues, CoreGraphics.CGFloat>  Swift   AttributeGraph
      12        192      16.0   NSRegion                                          Swift   AppKit
      12         96       8.0   CoreGraphics.CGFloat                              Swift   CoreGraphics
      12         96       8.0   SwiftUI._ShapeStyle_Pack                          Swift   SwiftUI
      11       8448     768.0   vImageConverterRef                                CFType  libCGInterfaces.dylib
      11       2112     192.0   NSTimer                                           ObjC    CoreFoundation
      11       1408     128.0   WKUserScript                                      ObjC    WebKit
      11        880      80.0   NSKeyValueUndefinedGetter                         ObjC    Foundation
      11        880      80.0   NSTrackingArea                                    ObjC    AppKit
      11        624      56.7   NSMethodSignature._typeString (char[])            C       CoreFoundation
      11        176      16.0   _LSDictionaryBackedPropertyList                   ObjC    LaunchServices
      10       6400     640.0   NSContextMenuImpl                                 ObjC    AppKit
      10       3200     320.0   SVGRootNode                                       C++     CoreSVG
      10       2240     224.0   SVGNode                                           C++     CoreSVG
      10       2240     224.0   URLParseInfo                                      Swift   Foundation
      10       1600     160.0   CFRunLoopObserver                                 CFType  CoreFoundation
      10       1600     160.0   _CFPasteboardEntry                                ObjC    CoreFoundation
      10        816      81.6   NSSet (Storage)                                   C       CoreFoundation
      10        800      80.0   SVGDocument                                       C++     CoreSVG
      10        480      48.0   RBSTarget                                         ObjC    RunningBoardServices
      10        480      48.0   WTF::WorkQueue                                    C++     JavaScriptCore
      10        480      48.0   xpc_string_t                                      ObjC    libxpc.dylib
      10        320      32.0   BSZeroingWeakReference                            ObjC    BaseBoard
      10        320      32.0   NSSet                                             ObjC    CoreFoundation
      10        320      32.0   SVGDocument                                       CFType  <unknown>
      10        320      32.0   xpc_endpoint_t                                    ObjC    libxpc.dylib
      10        272      27.2   xpc_string_t (Storage)                            C       libxpc.dylib
      10        160      16.0   NSSet.cow (struct __cow_state_t)                  C       CoreFoundation
       9       5760     640.0   CUICommonAssetStorage                             ObjC    CoreUI
       9       4032     448.0   CUICommonAssetStorage._header (struct _carheader)  C       CoreUI
       9       3456     384.0   CUICommonAssetStorage._bitmapKeydb (malloc)       C       CoreUI
       9       3456     384.0   CUICommonAssetStorage._facetKeysdb (malloc)       C       CoreUI
       9       3456     384.0   CUICommonAssetStorage._imagedb (malloc)           C       CoreUI
       9       2304     256.0   CGCMSConverter                                    CFType  CoreGraphics
       9        864      96.0   CUIStructuredThemeStore                           ObjC    CoreUI
       9        720      80.0   NSKeyValueSlowGetter                              ObjC    Foundation
       9        576      64.0   CUICommonAssetStorage._keyfmt (struct _renditionkeyfmt)  C       CoreUI
       9        576      64.0   IIO_Reader_HEIF                                   C++     ImageIO
       9        432      48.0   CAHostingToken                                    ObjC    QuartzCore
       9        432      48.0   RBSAssertionIdentifier                            ObjC    RunningBoardServices
       9        288      32.0   NSKeyboardShortcut                                ObjC    AppKit
       9        288      32.0   WTF::Detail::CallableWrapper<WTF::RunLoop::Timer::Timer<WebKit::ProcessThrottler>(WTF::Ref<WTF::RunLoop, WTF::RawPtrTraits<WTF::RunLoop>, WTF::DefaultRefDerefTraits<WTF::RunLoop>>&&, WTF::ASCIILiteral, WebKit::ProcessThrottler*, void (WebKit::ProcessThrottler::*)())::'lambda'(), void>  C++     WebKit
       9        216      24.0   AttributeGraph.Map<SwiftUI.EnvironmentValues, SwiftUI.LayoutDirection>  Swift   AttributeGraph
       9        144      16.0   __C.CGPoint                                       Swift   SwiftUICore
       9         72       8.0   SwiftUI.(PairPreferenceCombiner in $22ef6a634)<SwiftUI.ViewRespondersKey>  Swift   SwiftUI
       8       5120     640.0   NSView                                            ObjC    AppKit
       8       5120     640.0   _NSMenuBarBackingView                             ObjC    AppKit
       8       4096     512.0   NSOperationQueue                                  ObjC    Foundation
       8       1792     224.0   TTrueTypeFontLongDataHandler                      C++     libFontParser.dylib
       8       1536     192.0   CFXNotificationRegistrar                          CFType  CoreFoundation
       8       1536     192.0   Swift.ManagedBuffer<Foundation.URLComponents, __C.os_unfair_lock_s>  Swift   libswiftCore.dylib
       8       1280     160.0   __NSFontExtraData                                 ObjC    UIFoundation
       8        640      80.0   CUIThemeFacet                                     ObjC    CoreUI
       8        608      76.0   Swift.BridgingHashBuffer                          Swift   libswiftCore.dylib
       8        544      68.0   NSAttributeDictionary                             ObjC    UIFoundation
       8        384      48.0   NSCursor                                          ObjC    AppKit
       8        256      32.0   CFNotificationCenter                              CFType  CoreFoundation
       8        256      32.0   NSDisplayCyclePhase                               ObjC    AppKit
       8        256      32.0   NSMutableOrderedSet                               ObjC    CoreFoundation
       8        256      32.0   WTF::Detail::CallableWrapper<WTF::RunLoop::Timer::Timer<WebKit::WebPageProxy>(WTF::Ref<WTF::RunLoop, WTF::RawPtrTraits<WTF::RunLoop>, WTF::DefaultRefDerefTraits<WTF::RunLoop>>&&, WTF::ASCIILiteral, WebKit::WebPageProxy*, void (WebKit::WebPageProxy::*)())::'lambda'(), void>  C++     WebKit
       8        256      32.0   _CUISubrangeData                                  ObjC    CoreUI
       8        192      24.0   CUIThemeFacet._renditionKeyList (struct _renditionkeytoken)  C       CoreUI
       8        128      16.0   SLSEventAuthenticationMessageVersionedPID         ObjC    SkyLight
       8        128      16.0   _NSSwiftURLComponents                             Swift   Foundation
       7       3584     512.0   MenuElement                                       C++     HIToolbox
       7       2688     384.0   MenuData                                          C++     HIToolbox
       7       2240     320.0   CAContext._impl (malloc)                          C       QuartzCore
       7       1344     192.0   CFPasteboard                                      CFType  CoreFoundation
       7        896     128.0   dispatch_queue_t (concurrent)                     ObjC    libdispatch.dylib
       7        784     112.0   CGColorTransform                                  CFType  CoreGraphics
       7        672      96.0   NSClassicMapTable                                 ObjC    Foundation
       7        672      96.0   _MTLLibrary._cacheEntry (struct MTLLibraryContainer)  C       Metal
       7        560      80.0   BSProtobufSchema                                  ObjC    BaseBoard
       7        448      64.0   BSAuditToken                                      ObjC    BaseBoard
       7        448      64.0   BSObjCProtocol                                    ObjC    BaseBoard
       7        448      64.0   CFBasicHash                                       CFType  Foundation
       7        448      64.0   RBSAssertion                                      ObjC    RunningBoardServices
       7        448      64.0   _CFPasteboardCache                                ObjC    CoreFoundation
       7        336      48.0   RBSAssertionDescriptor                            ObjC    RunningBoardServices
       7        256      36.6   Class.data.protocols (protocol_array_t)           C       libobjc.A.dylib
       7        224      32.0   BSServiceDispatchQueue                            ObjC    BoardServices
       7        224      32.0   CABackdropLayer                                   ObjC    QuartzCore
       7        224      32.0   CFUUID                                            CFType  CoreFoundation
       7        224      32.0   LSASN                                             CFType  LaunchServices
       7        224      32.0   NSWeakObjectValue                                 ObjC    Foundation
       7        224      32.0   OS_xpc_mach_send                                  ObjC    libxpc.dylib
       7        112      16.0   CAContext                                         ObjC    QuartzCore
       7        112      16.0   _CFPasteboardWeakRef                              ObjC    CoreFoundation
       6      30720    5120.0   AGX::HAL200::FragmentProgramVariant               C++     AGXMetalG16X
       6       7680    1280.0   AGXG16XFamilyCommandQueue                         ObjC    AGXMetalG16X
       6       6144    1024.0   NSWindowAuxiliary                                 ObjC    AppKit
       6       5376     896.0   AGXG16XFamilyComputePipeline                      ObjC    AGXMetalG16X
       6       3840     640.0   FerryxNativeTerminalView                          ObjC    ferryx
       6       3072     512.0   CABackingStore                                    CFType  QuartzCore
       6       2688     448.0   NSCGSWindow                                       ObjC    AppKit
       6       1920     320.0   CGSWindow                                         CFType  SkyLight
       6       1920     320.0   IOGPUMetalResource                                ObjC    IOGPU
       6       1344     224.0   BSXPCServiceConnection                            ObjC    BoardServices
       6       1344     224.0   CFAllocator                                       CFType  CoreFoundation
       6       1344     224.0   NSCGSWindow._deviceID (malloc)                    C       AppKit
       6       1344     224.0   dispatch_workloop_t                               ObjC    libdispatch.dylib
       6       1152     192.0   BSXPCServiceConnectionEventHandler                ObjC    BoardServices
       6       1152     192.0   NSDragDestination                                 ObjC    AppKit
       6       1152     192.0   _NSThreadData                                     ObjC    Foundation
       6        960     160.0   NSViewHierarchyLock                               ObjC    AppKit
       6        960     160.0   _NSTrackingAreaAKManager                          ObjC    AppKit
       6        768     128.0   NSCGSCAWindowBackingStore                         ObjC    AppKit
       6        672     112.0   CGPDFString                                       CFType  CoreGraphics
       6        576      96.0   CUICatalog                                        ObjC    CoreUI
       6        576      96.0   NSKeyValueIvarMutableCollectionGetter             ObjC    Foundation
       6        576      96.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(BackgroundMaterialKey in $22ef7a9e0)>>  Swift   SwiftUI
       6        480      80.0   FPInFlightCommandBuffer                           ObjC    FramePacing
       6        480      80.0   NSWindowTitleController                           ObjC    AppKit
       6        384      64.0   CUICoreThemeRenderer                              C++     CoreUI
       6        384      64.0   NSISRestrictedToNonNegativeMarkerVariableToBeMinimized  ObjC    CoreAutoLayout
       6        384      64.0   NSISRestrictedToNonNegativeVariable               ObjC    CoreAutoLayout
       6        384      64.0   NSISRestrictedToNonNegativeVariableToBeMinimized  ObjC    CoreAutoLayout
       6        384      64.0   NSISRestrictedToZeroMarkerVariable                ObjC    CoreAutoLayout
       6        384      64.0   NSISUnrestrictedVariable                          ObjC    CoreAutoLayout
       6        384      64.0   TSMInputMethodInstance                            CFType  HIToolbox
       6        384      64.0   _MTLSharedEvent                                   ObjC    Metal
       6        288      48.0   CA::Render::Array                                 C++     QuartzCore
       6        288      48.0   NSKVONotifying_NSViewBackingLayer                 ObjC    AppKit
       6        288      48.0   SwiftUI._AnyResolvedPaint<SwiftUI.ColorView>      Swift   SwiftUI
       6        288      48.0   _UTDeclaredTypeRecord                             ObjC    LaunchServices
       6        288      48.0   std::__shared_ptr_emplace<Platform::SharedMemory::adopt(void*, unsigned long)::EnableMakeShared>  C++     WebPrivacy
       6        192      32.0   AGSubgraph                                        CFType  AttributeGraph
       6        192      32.0   CUICFType                                         CFType  CoreUI
       6        192      32.0   NSCGSWindowBackingStoreLayer                      ObjC    AppKit
       6        192      32.0   NSOrderedSet                                      ObjC    CoreFoundation
       6        192      32.0   NSXPCListenerEndpoint                             ObjC    Foundation
       6        192      32.0   SwiftUI.(ShapeStyleBox in $22ef776b4)<DesignLibrary.(WindowControlForegroundStyle in $2411b18b4)>  Swift   SwiftUI
       6        192      32.0   SwiftUI.ViewSize                                  Swift   SwiftUI
       6        192      32.0   WKRBSAssertionDelegate                            ObjC    WebKit
       6        192      32.0   _LSLocalizedStringRecord                          ObjC    LaunchServices
       6        192      32.0   _NSLocalEventObserver                             ObjC    AppKit
       6         96      16.0   CAFilter._attr (malloc)                           C       QuartzCore
       6         96      16.0   MTLPrivateDataTable                               ObjC    Metal
       6         96      16.0   NSCGSDockMessageHandlers                          ObjC    AppKit
       6         96      16.0   NSKVOForwarder                                    ObjC    AppKit
       6         96      16.0   NSOrderedSet.cow (struct __cow_state_t)           C       CoreFoundation
       6         96      16.0   _LSCoreTypesRecordProxy                           ObjC    LaunchServices
       6         48       8.0   SwiftUI.LayoutPositionQuery                       Swift   SwiftUI
       6         48       8.0   SwiftUI.PreferenceKeys                            Swift   SwiftUI
       6         48       8.0   SwiftUI.Time                                      Swift   SwiftUI
       5      25600    5120.0   AGX::HAL200::VertexProgramVariant                 C++     AGXMetalG16X
       5       5952    1190.4   Swift._DictionaryStorage<Swift.String, Swift.String>  Swift   libswiftCore.dylib
       5       3840     768.0   CA::Display::IOMFBDisplay                         C++     QuartzCore
       5       3840     768.0   LibraryWithData                                   C++     Metal
       5       3248     649.6   NSHashTable (Object Storage)                      C       Foundation
       5       3200     640.0   TSplicedFont                                      C++     CoreText
       5       1920     384.0   CUICommonAssetStorage._colordb (malloc)           C       CoreUI
       5       1728     345.6   NetworkAgent.NetworkAgentBackingClass.data._position (malloc)          Network
       5        800     160.0   CGSEventAppendix                                  CFType  CoreGraphics
       5        800     160.0   LSNotificationReceiverRef                         CFType  LaunchServices
       5        800     160.0   dispatch_mach_msg_t                               ObjC    libdispatch.dylib
       5        704     140.8   CFMessagePort                                     CFType  CoreFoundation
       5        640     128.0   CGColorTransformCache                             CFType  CoreGraphics
       5        560     112.0   HIDEvent                                          ObjC    IOKit
       5        480      96.0   NetworkAgent.NetworkAgentBackingClass             Swift   Network
       5        400      80.0   __NSFontTypefaceInfo                              ObjC    UIFoundation
       5        352      70.4   NSColorSpaceColor                                 ObjC    AppKit
       5        320      64.0   IOGPUMetalDeviceShmemPool                         ObjC    IOGPU
       5        320      64.0   NSThread                                          ObjC    Foundation
       5        256      51.2   Swift._ContiguousArrayStorage<Swift.UInt8>        Swift   libswiftCore.dylib
       5        240      48.0   CGColorTransformBase                              CFType  CoreGraphics
       5        240      48.0   NSAutoreleasePool                                 ObjC    Foundation
       5        240      48.0   NSColorSpace                                      ObjC    AppKit
       5        240      48.0   std::__shared_ptr_emplace<Platform::SharedMemory>  C++     SafariSafeBrowsing
       5        160      32.0   BKSHIDEventKeyboardDescriptor                     ObjC    BackBoardServices
       5        160      32.0   CGEvent                                           CFType  SkyLight
       5        160      32.0   TParserDictionary                                 C++     libFontParser.dylib
       5        160      32.0   WKWebPrivacyNotificationListener                  ObjC    WebKit
       5        160      32.0   _FPMetadata                                       ObjC    libFontParser.dylib
       5        160      32.0   _NSMenuImpl.sidebandUpdaters (struct NSMenuUpdaterInfo_t)  C       AppKit
       5         80      16.0   CADisplay                                         ObjC    QuartzCore
       5         80      16.0   NSCountedSet                                      ObjC    Foundation
       4      20480    5120.0   AGX::HAL200::ComputeProgramVariant                C++     AGXMetalG16X
       4      14336    3584.0   AGX::HAL200::BlitComputeProgramVariant            C++     AGXMetalG16X
       4       2560     640.0   NSMenuBarReplicantWindow                          ObjC    AppKit
       4       2560     640.0   NSMenuBarReplicantWindowFrame                     ObjC    AppKit
       4        800     200.0   CGPDFResources                                    CFType  CoreGraphics
       4        384      96.0   AGGraphStorage                                    CFType  AttributeGraph
       4        352      88.0   xpc_pipe_t                                        ObjC    libxpc.dylib
       4        320      80.0   NSISInlineStorageVariable                         ObjC    CoreAutoLayout
       4        320      80.0   NSLayoutConstraint                                ObjC    CoreAutoLayout
       4        320      80.0   _CTFontFallbacksArray                             ObjC    CoreText
       4        320      80.0   _NSXPCDistantObject                               ObjC    Foundation
       4        320      80.0   _WMWindowShadowProperties                         ObjC    WindowManagement
       4        256      64.0   NSControlAuxiliary                                ObjC    AppKit
       4        192      48.0   NSPurgeableData._reserved (struct)                C       Foundation
       4        192      48.0   NSVB_ViewAnimationAttributes                      ObjC    ViewBridge
       4        192      48.0   Swift._SwiftDeferredNSDictionary<Swift.String, Swift.String>  Swift   libswiftCore.dylib
       4        192      48.0   _NSStateMarker                                    ObjC    AppKit
       4        176      44.0   NSConcreteValue.typeInfo (malloc)                 C       Foundation
       4        128      32.0   IIODictionary                                     C++     ImageIO
       4        128      32.0   MemoryCookies                                     C++     CFNetwork
       4        128      32.0   NSNotificationCenter                              ObjC    Foundation
       4        128      32.0   NSPurgeableData                                   ObjC    Foundation
       4        128      32.0   RBSProcessIdentifier                              ObjC    RunningBoardServices
       4        128      32.0   SLSEventAuthenticationMessageEventType            ObjC    SkyLight
       4        128      32.0   WFDatabaseObjectDescriptor                        ObjC    VoiceShortcutClient
       4        128      32.0   WTF::Detail::CallableWrapper<WTF::RunLoop::Timer::Timer<WebKit::WebProcessPool>(WTF::Ref<WTF::RunLoop, WTF::RawPtrTraits<WTF::RunLoop>, WTF::DefaultRefDerefTraits<WTF::RunLoop>>&&, WTF::ASCIILiteral, WebKit::WebProcessPool*, void (WebKit::WebProcessPool::*)())::'lambda'(), void>  C++     WebKit
       4        128      32.0   _CGFontCacheKey                                   ObjC    CoreText
       4        128      32.0   _CTFontFallbacksArray._refTraits (struct TTraitsValues)  C       CoreText
       4        128      32.0   _NS1DVelocityFilter                               ObjC    AppKit
       4        128      32.0   _NS1DVelocityFilterIvars                          ObjC    AppKit
       4         64      16.0   NSMutableSet.cow (struct __cow_state_t)           C       CoreFoundation
       4         64      16.0   WTF::Detail::CallableWrapper<PAL::HysteresisActivity::HysteresisActivity(WTF::Function<void (PAL::HysteresisState)>&&, WTF::Seconds)::'lambda'(), void>  C++     WebKit
       3       6160    2053.3   NSOrderedSet                                      ObjC    CoreFoundation
       3       3328    1109.3   AGX::EndOfTileProgramKey                          C++     AGXMetalG16X
       3       2304     768.0   Swift._DictionaryStorage<SwiftUI.DisplayList.ViewUpdater.ViewCache.Key, SwiftUI.DisplayList.ViewUpdater.ViewInfo>  Swift   libswiftCore.dylib
       3       2304     768.0   _NSCoreHostingView<ThemeWidgetView>               Swift   AppKit
       3       1920     640.0   SwiftUI.(LayoutEngineBox in $22ef92010)<SwiftUI.(UnaryLayoutEngine in $22ef7f76c)<SwiftUI._AspectRatioLayout>>  Swift   SwiftUI
       3       1920     640.0   SwiftUI.(LayoutEngineBox in $22ef92010)<SwiftUI.(UnaryLayoutEngine in $22ef7f76c)<SwiftUI._FrameLayout>>  Swift   SwiftUI
       3       1536     512.0   SwiftUI.(BoxVTable in $22ef792c0)<SwiftUI.(EnvironmentBox in $22ef8a8a8)<SwiftUI.ControlSize>>  Swift   SwiftUI
       3       1536     512.0   ViewGraph                                         Swift   SwiftUICore
       3       1536     512.0   WKWebpagePreferences                              ObjC    WebKit
       3       1152     384.0   CGPDFPage                                         CFType  CoreGraphics
       3       1152     384.0   CUICommonAssetStorage._appearancedb (malloc)      C       CoreUI
       3       1152     384.0   SwiftUI.LeafViewResponder<SwiftUI.ShapeStyledResponderData<(extension in SwiftUI):SwiftUI.Image.Resolved>>  Swift   SwiftUI
       3        960     320.0   Swift._ContiguousArrayStorage<SwiftUI._ShapeStyle_InterpolatorGroup.(Layer in $22ef7accc)>  Swift   libswiftCore.dylib
       3        960     320.0   SwiftUI.(InterpolatedDisplayList in $22ef9b31c)<(extension in SwiftUI):SwiftUI.Image.Resolved>  Swift   SwiftUI
       3        960     320.0   SwiftUI.ZStack<SwiftUI.ModifiedContent<SwiftUI.ModifiedContent<SwiftUI.ModifiedContent<SwiftUI.ModifiedContent<SwiftUI._ShapeView<SwiftUI.Capsule, DesignLibrary.(WindowControlFillShapeStyle in $2411b18d8)>, SwiftUI._FrameLayout>, SwiftUI._OverlayModifier<SwiftUI.StrokeShapeView<SwiftUI.Capsule._Inset, SwiftUI._OpacityShapeStyle<SwiftUI._BlendModeShapeStyle<DesignLibrary.(WindowControlFillShapeStyle in $2411b18d8)>>, SwiftUI.EmptyView>>>, SwiftUI._OverlayModifier<SwiftUI.ModifiedContent<SwiftUI.ModifiedContent<SwiftUI.ModifiedContent<SwiftUI.Image, SwiftUI._AspectRatioLayout>, SwiftUI._ForegroundStyleModifier<DesignLibrary.(WindowControlForegroundStyle in $2411b18b4)>>, SwiftUI._EnvironmentKeyWritingModifier<Swift.Optional<SwiftUI.Material>>>>>, SwiftUI._OverlayShapeModifier<SwiftUI._OpacityShapeStyle<SwiftUI.InterpolatedShapeStyle<SwiftUI.InterpolatedShapeStyle<SwiftUI.InterpolatedShapeStyle<SwiftUI.Color, SwiftUI.Color>, SwiftUI.Color>, SwiftUI.Color>>, SwiftUI.Capsule>>>  Swift   SwiftUI
       3        768     256.0   CGPDFDocument                                     CFType  CoreGraphics
       3        768     256.0   SwiftUI.(LayoutEngineBox in $22ef92010)<SwiftUI.LeafLayoutEngine<SwiftUI.AnimatedShape<SwiftUI._StrokedShape<SwiftUI.Capsule._Inset>>>>  Swift   SwiftUI
       3        672     224.0   CGPDFSource                                       CFType  CoreGraphics
       3        672     224.0   DisplayList.ViewUpdater                           Swift   SwiftUICore
       3        672     224.0   HitTestBindingResponder                           Swift   SwiftUICore
       3        672     224.0   NSTextInputContext                                ObjC    AppKit
       3        672     224.0   NSTextInputContext._documentID (malloc)           C       AppKit
       3        672     224.0   SwiftUI.(LayoutEngineBox in $22ef92010)<SwiftUI.(ResolvedImageLayoutEngine in $22ef68140)>  Swift   SwiftUI
       3        672     224.0   SwiftUI.LeafViewResponder<SwiftUI.ShapeStyledResponderData<SwiftUI.AnimatedShape<SwiftUI._StrokedShape<SwiftUI.Capsule._Inset>>>>  Swift   SwiftUI
       3        672     224.0   SwiftUI.LeafViewResponder<SwiftUI.ShapeStyledResponderData<SwiftUI._BackgroundShapeModifier<SwiftUI._OpacityShapeStyle<SwiftUI.InterpolatedShapeStyle<SwiftUI.InterpolatedShapeStyle<SwiftUI.InterpolatedShapeStyle<SwiftUI.Color, SwiftUI.Color>, SwiftUI.Color>, SwiftUI.Color>>, SwiftUI.Capsule>>>  Swift   SwiftUI
       3        672     224.0   _CSIRenditionBlockData                            ObjC    CoreUI
       3        672     224.0   _CUIRawDataRendition                              ObjC    CoreUI
       3        672     224.0   _CUIThemePDFRendition                             ObjC    CoreUI
       3        576     192.0   (extension in SwiftUI):SwiftUI.Image.Resolved     Swift   SwiftUICore
       3        576     192.0   Swift._DictionaryStorage<Swift.OpaquePointer, SwiftUI.DisplayList.ViewUpdater.ViewCache.Key>  Swift   libswiftCore.dylib
       3        576     192.0   SwiftUI.(LayoutEngineBox in $22ef92010)<SwiftUI.LeafLayoutEngine<SwiftUI._ShapeView<SwiftUI.Capsule, DesignLibrary.(WindowControlFillShapeStyle in $2411b18d8)>>>  Swift   SwiftUI
       3        576     192.0   ViewGraphHost                                     Swift   SwiftUICore
       3        480     160.0   CUIVectorGlyphHierarchicalLayer                   ObjC    CoreUI
       3        480     160.0   NSCompositeAppearance                             ObjC    AppKit
       3        480     160.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(ContainerShapeKey in $22ef94af8)>>  Swift   SwiftUI
       3        480     160.0   SwiftUI.LeafViewResponder<SwiftUI.ShapeStyledResponderData<SwiftUI._ShapeView<SwiftUI.Capsule, DesignLibrary.(WindowControlFillShapeStyle in $2411b18d8)>>>  Swift   SwiftUI
       3        480     160.0   _CalendarGregorian                                Swift   Foundation
       3        480     160.0   __NSTextInputContextAuxiliaryStorage              ObjC    AppKit
       3        384     128.0   MatchedGeometryScope                              Swift   SwiftUICore
       3        384     128.0   NSButtonBezelConfiguration                        ObjC    AppKit
       3        384     128.0   Path.PathBox                                      Swift   SwiftUICore
       3        336     112.0   DisplayList.ViewRenderer                          Swift   SwiftUICore
       3        336     112.0   NSHostingViewBase                                 Swift   AppKit
       3        330     110.0   SwiftUI.Image.(ImageViewChild in $22efa3740)<SwiftUI.EmptyImageAccessibilityProvider>  Swift   SwiftUI
       3        288      96.0   BSXPCServiceConnectionMessage                     ObjC    BoardServices
       3        288      96.0   NSMutableParagraphStyle                           ObjC    UIFoundation
       3        288      96.0   Swift._ContiguousArrayStorage<SwiftUI.PreferenceKey.Protocol>  Swift   libswiftCore.dylib
       3        288      96.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.InterfaceIdiomInput>  Swift   SwiftUI
       3        288      96.0   SwiftUI.ImageProviderBox<SwiftUI.Image.NamedImageProvider>  Swift   SwiftUI
       3        288      96.0   SwiftUI.MutableBox<SwiftUI.CachedEnvironment>     Swift   SwiftUI
       3        288      96.0   WebKit::ProcessAndUIAssertion                     C++     WebKit
       3        288      96.0   pdf_document                                      CFType  CoreGraphics
       3        273      91.0   DesignLibrary.MacWindowControlElement             Swift   DesignLibrary
       3        240      80.0   BSServiceInitiatingConnection                     ObjC    BoardServices
       3        240      80.0   LocalizedTextStorage                              Swift   SwiftUICore
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<(extension in SwiftUI):SwiftUI._GraphInputs.(PlatformSystemKey in $22ef78cf0)>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.(MatchedGeometryScope in $22ef6fb1c)>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<(extension in DesignLibrary):SwiftUI.EnvironmentValues.(__Key_controlContext in $2411b3570)>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<(extension in DesignLibrary):SwiftUI.EnvironmentValues.(__Key_designIdiom in $2411b35cc)>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<(extension in DesignLibrary):SwiftUI.EnvironmentValues.(__Key_glassGroupContext in $2411b3700)>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<(extension in DesignLibrary):SwiftUI.EnvironmentValues.(__Key_glassMaterialPocketContainer in $2411b2da0)>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<(extension in DesignLibrary):SwiftUI.EnvironmentValues.(__Key_wantsPreSolariumMetrics in $2411b375c)>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<(extension in SwiftUI):SwiftUI.BackgroundProminence.Key>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<(extension in SwiftUI):SwiftUI.EnvironmentValues.(ExplicitColorSchemeKey in $22efa32d4)>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<(extension in SwiftUI):SwiftUI.EnvironmentValues.(MaterialBackdropProxyKey in $22efa54dc)>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<(extension in SwiftUI):SwiftUI.EnvironmentValues.(__Key_glassFrost in $22ef8ee8c)>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(ColorSchemeContrastKey in $22efa32b0)>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(ControlSizeKey in $22ef7a088)>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(DisplayGamutKey in $22ef9d34c)>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(DisplayScaleKey in $22ef9d274)>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(ForegroundStyleKey in $22ef845c0)>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.AppearsActiveKey>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.EnvironmentValues.LayoutDirectionKey>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.WindowEnvironmentKeys.AppearsActive>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.WindowEnvironmentKeys.AppearsFocused>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.WindowEnvironmentKeys.AppearsMain>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.WindowEnvironmentKeys.BackgroundIsOpaque>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.WindowEnvironmentKeys.IsFocused>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.WindowEnvironmentKeys.IsMain>>  Swift   SwiftUI
       3        240      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.Solarium.AllowedInCompatibilityKey>  Swift   SwiftUI
       3        240      80.0   WebKit::ProcessAssertion                          C++     WebKit
       3        240      80.0   WebKit::ProcessLauncher                           C++     WebKit
       3        228      76.0   SwiftUI.AnimatableAttribute<SwiftUI._StrokedShape<SwiftUI.Capsule._Inset>>  Swift   SwiftUI
       3        216      72.0   SwiftUI.(ShapeStyledDisplayList in $22ef6c164)<(extension in SwiftUI):SwiftUI.Image.Resolved>  Swift   SwiftUI
       3        216      72.0   SwiftUI.ShapeStyleResolver<DesignLibrary.(WindowControlFillShapeStyle in $2411b18d8)>  Swift   SwiftUI
       3        216      72.0   SwiftUI.ShapeStyleResolver<SwiftUI.AnyShapeStyle>  Swift   SwiftUI
       3        216      72.0   SwiftUI.ShapeStyleResolver<SwiftUI._OpacityShapeStyle<SwiftUI.InterpolatedShapeStyle<SwiftUI.InterpolatedShapeStyle<SwiftUI.InterpolatedShapeStyle<SwiftUI.Color, SwiftUI.Color>, SwiftUI.Color>, SwiftUI.Color>>>  Swift   SwiftUI
       3        216      72.0   SwiftUI.ShapeStyleResolver<SwiftUI._OpacityShapeStyle<SwiftUI._BlendModeShapeStyle<DesignLibrary.(WindowControlFillShapeStyle in $2411b18d8)>>>  Swift   SwiftUI
       3        195      65.0   SwiftUI.(ContainerShapeEnvironment in $22ef94ab0)  Swift   SwiftUI
       3        192      64.0   EventLoopTimer                                    CFType  HIToolbox
       3        192      64.0   IIO_Reader_ASTC                                   C++     ImageIO
       3        192      64.0   IIO_Reader_BC                                     C++     ImageIO
       3        192      64.0   IIO_Reader_PVR                                    C++     ImageIO
       3        192      64.0   NSHashTable.slice.internalProps (struct NSSliceInternalProperties)  C       Foundation
       3        192      64.0   NSStringMeasurementCacheKey                       ObjC    AppKit
       3        192      64.0   NSThemeWidgetAquaduckVisualProvider               Swift   AppKit
       3        192      64.0   NSXPCListener                                     ObjC    Foundation
       3        192      64.0   PDFImageContents                                  Swift   SwiftUICore
       3        192      64.0   Swift._ContiguousArrayStorage<SwiftUI.CachedEnvironment.(MapItem in $22ef8dacc)>  Swift   libswiftCore.dylib
       3        192      64.0   SwiftUI.ImageProviderBox<SwiftUI.Image.ResizableProvider>  Swift   SwiftUI
       3        192      64.0   SwiftUI.ViewGraphFeatureBuffer.(_VTable in $22efa5bf4)<SwiftUI.ViewGraphHost.GraphFeature>  Swift   SwiftUI
       3        192      64.0   WFPurgeableImage                                  ObjC    VoiceShortcutClient
       3        192      64.0   WFServicesWorkflow                                ObjC    VoiceShortcutClient
       3        192      64.0   _NSAxisAlignedVolumeColorGamut                    ObjC    AppKit
       3        192      64.0   _ShapeStyle_InterpolatorGroup                     Swift   SwiftUICore
       3        160      53.3   Swift._ContiguousArrayStorage<Swift.String>       Swift   libswiftCore.dylib
       3        156      52.0   SwiftUI.(ShapeStyledDisplayList in $22ef6c164)<SwiftUI.AnimatedShape<SwiftUI._StrokedShape<SwiftUI.Capsule._Inset>>>  Swift   SwiftUI
       3        156      52.0   SwiftUI.(ShapeStyledDisplayList in $22ef6c164)<SwiftUI._BackgroundShapeModifier<SwiftUI._OpacityShapeStyle<SwiftUI.InterpolatedShapeStyle<SwiftUI.InterpolatedShapeStyle<SwiftUI.InterpolatedShapeStyle<SwiftUI.Color, SwiftUI.Color>, SwiftUI.Color>, SwiftUI.Color>>, SwiftUI.Capsule>>  Swift   SwiftUI
       3        156      52.0   SwiftUI.(ShapeStyledDisplayList in $22ef6c164)<SwiftUI._ShapeView<SwiftUI.Capsule, DesignLibrary.(WindowControlFillShapeStyle in $2411b18d8)>>  Swift   SwiftUI
       3        150      50.0   SwiftUI.AnimatedShape<SwiftUI._StrokedShape<SwiftUI.Capsule._Inset>>  Swift   SwiftUI
       3        144      48.0   AXUIElement                                       CFType  HIServices
       3        144      48.0   BSServiceInterface                                ObjC    BoardServices
       3        144      48.0   BSXPCCoder                                        ObjC    BaseBoard
       3        144      48.0   BSXPCServiceConnectionChildContext                ObjC    BoardServices
       3        144      48.0   BSXPCServiceConnectionEndpoint                    ObjC    BoardServices
       3        144      48.0   BSXPCServiceConnectionRootClientContext           ObjC    BoardServices
       3        144      48.0   MTLResourceListPool                               ObjC    Metal
       3        144      48.0   NSAccessibilityRemoteUIElement                    ObjC    AppKit
       3        144      48.0   NSHashTable.slice.personalityProps (struct NSSliceExternalPersonalityProperties)  C       Foundation
       3        144      48.0   NSKeyBindingManager                               ObjC    AppKit
       3        144      48.0   NSSortedArray                                     ObjC    AppKit
       3        144      48.0   NSSymbolEffectOptions                             ObjC    Symbols
       3        144      48.0   Swift._ContiguousArrayStorage<Swift.AnyObject>    Swift   libswiftCore.dylib
       3        144      48.0   SwiftUI._StrokedShape<SwiftUI.Capsule._Inset>     Swift   SwiftUI
       3        144      48.0   WTF::Detail::CallableWrapper<WebKit::ProcessLauncher::finishLaunchingProcess(WTF::ASCIILiteral)::$_0, void, NSObject<OS_xpc_object>*>  C++     WebKit
       3        144      48.0   _NSTrackingAreaCGViewHelper                       ObjC    AppKit
       3        112      37.3   NSKeyValueMethodSetter._selector (SEL)            C       Foundation
       3         99      33.0   SwiftUI.(ChildEnvironment in $22ef70158)<Swift.Optional<SwiftUI.Material>>  Swift   SwiftUI
       3         99      33.0   SwiftUI.RootContainerShape                        Swift   SwiftUI
       3         96      32.0   AGXG16XFamilyComputeOrFragmentOrTileProgram       ObjC    AGXMetalG16X
       3         96      32.0   CAMediaTimingFunctionBuiltin                      ObjC    QuartzCore
       3         96      32.0   CSCustomAttributeKey                              ObjC    CoreSpotlight
       3         96      32.0   NSCompoundPredicateOperator                       ObjC    Foundation
       3         96      32.0   NSConcreteMutableAttributedString                 ObjC    Foundation
       3         96      32.0   NSHashTable.slice.acquisitionProps (struct NSSliceExternalAcquisitionProperties)  C       Foundation
       3         96      32.0   SwiftUI.(ShapeStyledResponderFilter in $22ef6c108)<(extension in SwiftUI):SwiftUI.Image.Resolved>  Swift   SwiftUI
       3         96      32.0   SwiftUI.(ShapeStyledResponderFilter in $22ef6c108)<SwiftUI.AnimatedShape<SwiftUI._StrokedShape<SwiftUI.Capsule._Inset>>>  Swift   SwiftUI
       3         96      32.0   SwiftUI.(ShapeStyledResponderFilter in $22ef6c108)<SwiftUI._BackgroundShapeModifier<SwiftUI._OpacityShapeStyle<SwiftUI.InterpolatedShapeStyle<SwiftUI.InterpolatedShapeStyle<SwiftUI.InterpolatedShapeStyle<SwiftUI.Color, SwiftUI.Color>, SwiftUI.Color>, SwiftUI.Color>>, SwiftUI.Capsule>>  Swift   SwiftUI
       3         96      32.0   SwiftUI.(ShapeStyledResponderFilter in $22ef6c108)<SwiftUI._ShapeView<SwiftUI.Capsule, DesignLibrary.(WindowControlFillShapeStyle in $2411b18d8)>>  Swift   SwiftUI
       3         96      32.0   WFServicesIconDescriptor                          ObjC    VoiceShortcutClient
       3         96      32.0   WTF::Detail::CallableWrapper<WTF::RunLoop::Timer::Timer<WebKit::ResponsivenessTimer>(WTF::Ref<WTF::RunLoop, WTF::RawPtrTraits<WTF::RunLoop>, WTF::DefaultRefDerefTraits<WTF::RunLoop>>&&, WTF::ASCIILiteral, WebKit::ResponsivenessTimer*, void (WebKit::ResponsivenessTimer::*)())::'lambda'(), void>  C++     WebKit
       3         96      32.0   WTF::Detail::CallableWrapper<WebKit::ProcessLauncher::finishLaunchingProcess(WTF::ASCIILiteral)::$_1, void, NSObject<OS_xpc_object>*>  C++     WebKit
       3         96      32.0   WTF::Logger                                       C++     WebKit
       3         96      32.0   WebKit::WebPagePreferencesLockdownModeObserver    C++     WebKit
       3         84      28.0   SwiftUI.(DynamicBody in $22ef7c948)<SwiftUI.ViewBodyAccessor<DesignLibrary.MacWindowControlElement>, SwiftUI.(MainThreadFlags in $22ef7c924)>  Swift   SwiftUI
       3         72      24.0   (SwiftUI.DisplayList, SwiftUI.DisplayList.Version)  Swift   <unknown>
       3         72      24.0   SwiftUI.ContainerShapeTransform                   Swift   SwiftUI
       3         60      20.0   SwiftUI._SafeAreaInsetsModifier.(Transform in $22efa2f70)  Swift   SwiftUI
       3         51      17.0   AppKit.ThemeWidgetView                            Swift   AppKit
       3         51      17.0   SwiftUI.(ChildEnvironment in $22ef70158)<SwiftUI.ControlSize>  Swift   SwiftUI
       3         51      17.0   SwiftUI.ModifiedContent<DesignLibrary.WindowControl, SwiftUI._EnvironmentKeyWritingModifier<SwiftUI.ControlSize>>  Swift   SwiftUI
       3         48      16.0   SwiftUI.(HitTestBindingFilter in $22ef7b0e8)      Swift   SwiftUI
       3         48      16.0   SwiftUI._SafeAreaInsetsModifier                   Swift   SwiftUI
       3         48      16.0   SwiftUI._SafeAreaInsetsModifier.(Insets in $22efa2f4c)  Swift   SwiftUI
       3         48      16.0   WTF::Detail::CallableWrapper<WebKit::AuxiliaryProcessProxy::didFinishLaunching(WebKit::ProcessLauncher*, IPC::Connection::Identifier&&)::$_0, void>  C++     WebKit
       3         48      16.0   WTF::Detail::CallableWrapper<WebKit::ProcessThrottler::setThrottleState(WebKit::ProcessThrottleState)::$_1, void>  C++     WebKit
       3         48      16.0   WTF::Detail::CallableWrapper<WebKit::ProcessThrottler::setThrottleState(WebKit::ProcessThrottleState)::$_3, void>  C++     WebKit
       3         24       8.0   Swift.Dictionary<SwiftUI.EventID, SwiftUI.EventType>  Swift   libswiftCore.dylib
       3         24       8.0   SwiftUI.Transaction                               Swift   SwiftUI
       3         24       8.0   SwiftUI._ForegroundStyleModifier<DesignLibrary.(WindowControlForegroundStyle in $2411b18b4)>.(ForegroundStyleEnvironment in $22ef844e4)<>  Swift   SwiftUI
       3         24       8.0   SwiftUI._GestureInputs.InheritedPhase             Swift   SwiftUI
       2       7168    3584.0   AGX::HAL200::BackgroundObjectProgramVariant       C++     AGXMetalG16X
       2       3584    1792.0   AGXG16XFamilyFragmentProgram                      ObjC    AGXMetalG16X
       2       3072    1536.0   AGXG16XFamilyVertexProgram                        ObjC    AGXMetalG16X
       2       3072    1536.0   CVDisplayLink                                     CFType  CoreVideo
       2       1792     896.0   icu::CollationSettings                            C++     libicucore.A.dylib
       2       1664     832.0   AGX::BackgroundObjectProgramKey                   C++     AGXMetalG16X
       2       1536     768.0   LibraryWithFile                                   C++     Metal
       2       1536     768.0   NSKVONotifying_NSRemoteView                       ObjC    ViewBridge
       2       1280     640.0   NSRemoteViewMarshal                               ObjC    ViewBridge
       2       1280     640.0   NSSeparatorToolbarItemView                        ObjC    AppKit
       2       1280     640.0   PNGReadPlugin                                     C++     ImageIO
       2       1280     640.0   WKWebViewConfiguration                            ObjC    WebKit
       2       1024     512.0   IIOImageRead                                      C++     ImageIO
       2        896     448.0   PKHostPlugIn                                      ObjC    PlugInKit
       2        640     320.0   CGContextDelegate                                 CFType  CoreGraphics
       2        640     320.0   CGImageMetadata                                   CFType  ImageIO
       2        640     320.0   Gestures.GestureNode<()>                          Swift   Gestures
       2        640     320.0   IIOImagePlus                                      C++     ImageIO
       2        640     320.0   IIOImageProviderInfo                              C++     ImageIO
       2        640     320.0   NSKVONotifying__WMWindow                          ObjC    WindowManagement
       2        640     320.0   NSMenuBarImpl                                     ObjC    AppKit
       2        640     320.0   SCClientSession                                   C++     CarbonCore
       2        640     320.0   __NSFastEnumerationEnumerator                     ObjC    CoreFoundation
       2        512     256.0   NSISObjectiveLinearExpression._priorityMap (struct)  C       CoreAutoLayout
       2        512     256.0   NSKVONotifying_NSWindowSectionControllerBoundingDivider  ObjC    AppKit
       2        512     256.0   NSURLRequestInternal                              ObjC    CFNetwork
       2        448     224.0   NSMenuBarRepresentation                           ObjC    AppKit
       2        448     224.0   NWConcrete_nw_protocol_definition                 ObjC    Network
       2        448     224.0   TTrueTypeFontDataHandler                          C++     libFontParser.dylib
       2        448     224.0   WMWindowPropertySnapshot                          ObjC    WindowManagement
       2        384     192.0   CGContext                                         CFType  CoreGraphics
       2        384     192.0   Interface.BackingClass                            Swift   Network
       2        384     192.0   NSEvent                                           ObjC    AppKit
       2        384     192.0   NSEvent._eventRef (malloc)                        C       AppKit
       2        384     192.0   NSRegularLegacyScrollerImp                        ObjC    AppKit
       2        320     160.0   CFHTTPCookieStorage                               CFType  CFNetwork
       2        320     160.0   MTLCompilerFSCache                                C++     Metal
       2        320     160.0   NSParagraphStyleExtraData                         ObjC    UIFoundation
       2        320     160.0   NSVibrantDarkAppearance                           ObjC    AppKit
       2        320     160.0   SVGPath                                           C++     CoreSVG
       2        320     160.0   WKPreferences                                     ObjC    WebKit
       2        256     128.0   NSISObjectiveLinearExpression._constant (struct)  C       CoreAutoLayout
       2        256     128.0   NSMenuSelectionRect                               ObjC    AppKit
       2        256     128.0   SLSTransaction                                    CFType  SkyLight
       2        256     128.0   WTF::RunLoop                                      C++     JavaScriptCore
       2        224     112.0   NSCursorArea                                      ObjC    AppKit
       2        224     112.0   NSRemoteViewControllerParametersForService        ObjC    ViewBridge
       2        224     112.0   NSTextFieldBezelConfiguration                     ObjC    AppKit
       2        224     112.0   SLSSkyLightEventAuthenticationMessage             ObjC    SkyLight
       2        224     112.0   Swift._ContiguousArrayStorage<Gestures.GesturePhase<()>>  Swift   libswiftCore.dylib
       2        224     112.0   Swift._DictionaryStorage<Foundation.Locale.IdentifierType, Swift.String>  Swift   libswiftCore.dylib
       2        224     112.0   WebScrollerImpDelegateMac._scroller (struct)      C       WebCore
       2        224     112.0   _NSViewControllerPrivateData                      ObjC    AppKit
       2        192      96.0   CSSearchableIndex                                 ObjC    CoreSpotlight
       2        192      96.0   CUIWindowFrameLayer                               ObjC    CoreUI
       2        192      96.0   NSContentSizeLayoutConstraint                     ObjC    CoreAutoLayout
       2        192      96.0   NSKeyValueShareableObservationInfoKey             ObjC    Foundation
       2        192      96.0   NSWindowLayout                                    ObjC    AppKit
       2        192      96.0   PKDiscoveryDriver                                 ObjC    PlugInKit
       2        192      96.0   Swift._SetStorage<Gestures.RelationDefinition>    Swift   libswiftCore.dylib
       2        160      80.0   CSRequestQueue                                    ObjC    CoreSpotlight
       2        160      80.0   CUIVectorGlyphPath                                ObjC    CoreUI
       2        160      80.0   NSCGImageRep                                      ObjC    AppKit
       2        160      80.0   NSISObjectiveLinearExpression                     ObjC    CoreAutoLayout
       2        160      80.0   NSImageSymbolConfiguration                        ObjC    AppKit
       2        160      80.0   NSKeyValueIvarSetter                              ObjC    Foundation
       2        160      80.0   NSKeyValueNestedProperty                          ObjC    Foundation
       2        160      80.0   OS_voucher                                        ObjC    libdispatch.dylib
       2        160      80.0   Swift.WritableKeyPath<SwiftUI.EnvironmentValues, Swift.Bool>  Swift   libswiftCore.dylib
       2        160      80.0   Swift.WritableKeyPath<SwiftUI.EnvironmentValues, SwiftUI.ControlSize>  Swift   libswiftCore.dylib
       2        160      80.0   _SwiftURL                                         Swift   Foundation
       2        160      80.0   _WKVisitedLinkStore                               ObjC    WebKit
       2        160      80.0   icu::CollationCacheEntry                          C++     libicucore.A.dylib
       2        128      64.0   CFPrefsSource                                     ObjC    CoreFoundation
       2        128      64.0   Gestures.GestureNodeShim<()>                      Swift   Gestures
       2        128      64.0   IIOImageReadSession                               C++     ImageIO
       2        128      64.0   IIO_Reader_ETC                                    C++     ImageIO
       2        128      64.0   NSKeyValueObservation.Helper                      Swift   Foundation
       2        128      64.0   NSRemoteViewControllerAuxiliary                   ObjC    ViewBridge
       2        128      64.0   NSTextFieldAppearanceBasedVisualProvider          ObjC    AppKit
       2        128      64.0   NSVBCALayerHost                                   ObjC    ViewBridge
       2        128      64.0   NSViewRemoteBridge                                ObjC    ViewBridge
       2        128      64.0   RBSInheritance                                    ObjC    RunningBoardServices
       2        128      64.0   Swift._ContiguousArrayStorage<Gestures.GestureNodeMatcher>  Swift   libswiftCore.dylib
       2        128      64.0   WebKit::WebURLSchemeHandlerCocoa                  C++     WebKit
       2        128      64.0   WebScrollerImpDelegateMac                         ObjC    WebCore
       2        128      64.0   _NSKeyBindingStateActual                          ObjC    AppKit
       2        128      64.0   _NSWMWindowTilingStateController                  Swift   AppKit
       2         96      48.0   BSObjCIvar                                        ObjC    BaseBoard
       2         96      48.0   BSProcessHandle                                   ObjC    BaseBoard
       2         96      48.0   BSXPCServiceConnectionPeer                        ObjC    BoardServices
       2         96      48.0   CUIPSDGradientColorStop                           ObjC    CoreUI
       2         96      48.0   Foundation.LockedState<Foundation._NSSwiftTimeZone.State>.(_Buffer in $18348d314)<>  Swift   Foundation
       2         96      48.0   NSCGSWindowCornerRadiusMask                       ObjC    AppKit
       2         96      48.0   NSKVONotifying_NSUserDefaults                     ObjC    CoreFoundation
       2         96      48.0   NSMenuFixedVisibleIndexDictionary                 Swift   AppKit
       2         96      48.0   NSPersistentUIWindowInfo                          ObjC    AppKit
       2         96      48.0   NSViewCornerRadii                                 ObjC    AppKit
       2         96      48.0   Swift._ContiguousArrayStorage<Swift.Set<Gestures.RelationDefinition>>  Swift   libswiftCore.dylib
       2         96      48.0   SwiftUI.ColorBox<SwiftUI.ResolvedColorProvider>   Swift   SwiftUI
       2         96      48.0   WKNSArray                                         ObjC    WebKit
       2         96      48.0   _NSCGSWindowOrderingProperties                    ObjC    AppKit
       2         96      48.0   _NSUndoStack                                      ObjC    Foundation
       2         96      48.0   _TimeZoneGMTICU                                   Swift   Foundation
       2         96      48.0   __NWProtocolIdentifier                            Swift   Network
       2         96      48.0   std::__shared_ptr_emplace<WS::Shmem>              C++     SkyLight
       2         64      32.0   BSMutableIntegerMap                               ObjC    BaseBoard
       2         64      32.0   BSServiceInitiatingConnectionMultiplexer          ObjC    BoardServices
       2         64      32.0   CGImagePlus                                       CFType  ImageIO
       2         64      32.0   CGImageRead                                       CFType  ImageIO
       2         64      32.0   CGImageReadSession                                CFType  ImageIO
       2         64      32.0   CUIPSDGradientOpacityStop                         ObjC    CoreUI
       2         64      32.0   Foundation.LockedState<Swift.Optional<Swift.AnyObject>>.(_Buffer in $18348d314)<>  Swift   Foundation
       2         64      32.0   Foundation.LockedState<Swift.Optional<__C.NSURL>>.(_Buffer in $18348d314)<>  Swift   Foundation
       2         64      32.0   NSMutableIndexSet                                 ObjC    Foundation
       2         64      32.0   NSTextTab                                         ObjC    UIFoundation
       2         64      32.0   NSThreadSafeClassCache                            ObjC    AppKit
       2         64      32.0   NSVBTestedFault                                   ObjC    ViewBridge
       2         64      32.0   NSXPCSpellServerClient                            ObjC    AppKit
       2         64      32.0   PKDiscoveryLSWatcher                              ObjC    PlugInKit
       2         64      32.0   SVGPath                                           CFType  <unknown>
       2         64      32.0   TFontPathIndex                                    C++     libFontRegistry.dylib
       2         64      32.0   TType1FontType2CIDCharStringHandler               C++     libFontParser.dylib
       2         64      32.0   WTF::Detail::CallableWrapper<WTF::RunLoop::Timer::Timer<WebKit::BackgroundProcessResponsivenessTimer>(WTF::Ref<WTF::RunLoop, WTF::RawPtrTraits<WTF::RunLoop>, WTF::DefaultRefDerefTraits<WTF::RunLoop>>&&, WTF::ASCIILiteral, WebKit::BackgroundProcessResponsivenessTimer*, void (WebKit::BackgroundProcessResponsivenessTimer::*)())::'lambda'(), void>  C++     WebKit
       2         64      32.0   WTF::Detail::CallableWrapper<WTF::RunLoop::Timer::Timer<WebKit::ProcessThrottlerTimedActivity>(WTF::Ref<WTF::RunLoop, WTF::RawPtrTraits<WTF::RunLoop>, WTF::DefaultRefDerefTraits<WTF::RunLoop>>&&, WTF::ASCIILiteral, WebKit::ProcessThrottlerTimedActivity*, void (WebKit::ProcessThrottlerTimedActivity::*)())::'lambda'(), void>  C++     WebKit
       2         64      32.0   WTF::Detail::CallableWrapper<WTF::RunLoop::Timer::Timer<WebKit::SharedStringHashStore>(WTF::Ref<WTF::RunLoop, WTF::RawPtrTraits<WTF::RunLoop>, WTF::DefaultRefDerefTraits<WTF::RunLoop>>&&, WTF::ASCIILiteral, WebKit::SharedStringHashStore*, void (WebKit::SharedStringHashStore::*)())::'lambda'(), void>  C++     WebKit
       2         64      32.0   _NS2DVelocityFilter                               ObjC    AppKit
       2         64      32.0   _NSSwiftTimeZone                                  Swift   Foundation
       2         64      32.0   _SwiftURL.ResourceInfo                            Swift   Foundation
       2         64      32.0   __NWInterface                                     Swift   Network
       2         64      32.0   icu::UVector64                                    C++     libicucore.A.dylib
       2         64      32.0   xpc_pointer_t                                     ObjC    libxpc.dylib
       2         32      16.0   BKSHIDEventDeferringEnvironment                   ObjC    BackBoardServices
       2         32      16.0   CABasicAnimation._attr (malloc)                   C       QuartzCore
       2         32      16.0   NSKeyValueObservation.Helper.lock (malloc)                Foundation
       2         32      16.0   NSObject                                          ObjC    libobjc.A.dylib
       2         32      16.0   Swift unowned reference storage                   C       libswiftCore.dylib
       2         32      16.0   _BKSHIDStringIdentifierEventDeferringToken        ObjC    BackBoardServices
       2         32      16.0   _NSCGSWindowOrderingGroup                         ObjC    AppKit
       2         32      16.0   _NSKeyValueObservation                            Swift   Foundation
       2         32      16.0   __NWInterface.lock (malloc)                               Network
       1     540672  540672.0   GLDDeviceRec                                      C++     AppleMetalOpenGLRenderer
       1      24576   24576.0   __NSFontExtraData._glyphAdvancementCache (double[])  C       UIFoundation
       1      20480   20480.0   AGXG16CDevice._impl (malloc)                      C       AGXMetalG16X
       1       6144    6144.0   _MTLDeviceFeatureQueries                          ObjC    Metal
       1       5120    5120.0   WKNavigation                                      ObjC    WebKit
       1       4608    4608.0   WebKit::WebMouseEvent                             C++     WebKit
       1       4608    4608.0   WebKit::WebPageProxy::Internals                   C++     WebKit
       1       3584    3584.0   AGX::HAL200::ClearVisibilityVertexProgramVariant  C++     AGXMetalG16X
       1       3584    3584.0   AGX::HAL200::DummyFeedbackFragmentProgramVariant  C++     AGXMetalG16X
       1       3584    3584.0   AGX::HAL200::PassthroughVertexProgramVariant      C++     AGXMetalG16X
       1       3072    3072.0   AGXG16CDevice                                     ObjC    AGXMetalG16X
       1       3072    3072.0   AGXG16CDevice._buffer_suballocator (struct IOGPUMetalSuballocator)  C       AGXMetalG16X
       1       3072    3072.0   WebKit::WebKeyboardEvent                          C++     WebKit
       1       2560    2560.0   Swift._DictionaryStorage<Swift.String, Foundation._LocaleProtocol & Swift.AnyObject>  Swift   libswiftCore.dylib
       1       2560    2560.0   TKeyLookUpCache<256u>                             C++     libFontParser.dylib
       1       2048    2048.0   MeCab::Tokenizer<mecab_node_t, mecab_path_t>      C++     libmecab.dylib
       1       1792    1792.0   Swift._ContiguousArrayStorage<SwiftUI.ObjectCache<(extension in SwiftUI):SwiftUI.Color.ResolvedHDR, __C.CGColorRef>.(Item in $22ef99e64)<>>  Swift   libswiftCore.dylib
       1       1792    1792.0   Swift._DictionaryStorage<Swift.String, Foundation._NSSwiftLocale>  Swift   libswiftCore.dylib
       1       1792    1792.0   Swift._DictionaryStorage<Swift.String, Swift.Int>  Swift   libswiftCore.dylib
       1       1792    1792.0   _LSDatabase                                       ObjC    LaunchServices
       1       1536    1536.0   WKProcessPool                                     ObjC    WebKit
       1       1472    1472.0   WebKit::WebProcessProxy                           C++     WebKit
       1       1280    1280.0   ..NSKVONotifying_wry::wkwebview::class::wry_web_view::WryWebView0.55.1  ObjC    ferryx
       1       1280    1280.0   AGXG16XFamilyComputeProgram                       ObjC    AGXMetalG16X
       1       1280    1280.0   Swift._DictionaryStorage<AppKit.NSMenuKeyCache.CacheKey, Swift.Array<AppKit.NSMenuKeyCache.CacheEntry>>  Swift   libswiftCore.dylib
       1       1280    1280.0   WKImmediateActionController                       ObjC    WebKit
       1       1024    1024.0   ..NSKVONotifying_wry::wkwebview::class::wry_web_view::WryWebView0.55.1._impl (unique_ptr<WebKit::WebViewImpl>)  C++     ferryx
       1       1024    1024.0   AGX::BlitComputeProgramKey                        C++     AGXMetalG16X
       1       1024    1024.0   AGX::BlitFragmentProgramKey                       C++     AGXMetalG16X
       1       1024    1024.0   NSKVONotifying_NSThemeFrame                       ObjC    AppKit
       1       1024    1024.0   _WKHitTestResult                                  ObjC    WebKit
       1       1008    1008.0   WebKit::AuthenticatorManager                      C++     WebKit
       1        896     896.0   NSTextField                                       ObjC    AppKit
       1        896     896.0   Swift._DictionaryStorage<SwiftUI.NamedImage.BitmapKey, SwiftUI.NamedImage._BitmapInfo<SwiftUI.NamedImage.WeakOrStrongImageContents>>  Swift   libswiftCore.dylib
       1        896     896.0   Swift._SetStorage<GenerativeModels.GenerativeModelsAvailability.Availability.UnavailableInfo.UnavailableReason>  Swift   libswiftCore.dylib
       1        896     896.0   icu::RuleBasedBreakIterator::BreakCache           C++     libicucore.A.dylib
       1        768     768.0   AGX::BlitFastClearProgramKey                      C++     AGXMetalG16X
       1        768     768.0   AGX::BlitSparseProgramKey                         C++     AGXMetalG16X
       1        768     768.0   AGX::BlitVertexFastClearProgramKey                C++     AGXMetalG16X
       1        768     768.0   AGX::BlitVertexProgramKey                         C++     AGXMetalG16X
       1        768     768.0   AGX::ComputeControlFlowPredicateProgramKey        C++     AGXMetalG16X
       1        768     768.0   AGX::PassthroughObjectProgramKey                  C++     AGXMetalG16X
       1        768     768.0   AGX::TessellationObjectProgramKey                 C++     AGXMetalG16X
       1        768     768.0   AGX::TileDispatchVertexProgramKey                 C++     AGXMetalG16X
       1        768     768.0   CA::Display::LocalDisplay                         C++     QuartzCore
       1        768     768.0   MeCab::Connector                                  C++     libmecab.dylib
       1        768     768.0   MeCab::Dictionary                                 C++     libmecab.dylib
       1        768     768.0   NSTextFieldSimpleLabel                            Swift   AppKit
       1        768     768.0   NSTitlebarContainerView                           ObjC    AppKit
       1        768     768.0   _NSThemeCloseWidget                               ObjC    AppKit
       1        768     768.0   _NSThemeWidget                                    ObjC    AppKit
       1        768     768.0   _NSThemeZoomWidget                                ObjC    AppKit
       1        640     640.0   NSKVONotifying_TUINSWindow                        ObjC    TextInputUIMacHelper
       1        640     640.0   NSKVONotifying_TaoWindow                          ObjC    ferryx
       1        640     640.0   NSKVONotifying__NSTitlebarDecorationView          ObjC    AppKit
       1        640     640.0   NSNextStepFrame                                   ObjC    AppKit
       1        640     640.0   NSTitlebarBackgroundView                          ObjC    AppKit
       1        640     640.0   NSTitlebarView                                    ObjC    AppKit
       1        640     640.0   TaoView                                           ObjC    ferryx
       1        640     640.0   WKFlippedView                                     ObjC    WebKit
       1        640     640.0   WKWebsiteDataStore                                ObjC    WebKit
       1        640     640.0   _WKWebsiteDataStoreConfiguration                  ObjC    WebKit
       1        592     592.0   WebKit::NetworkProcessProxy                       C++     WebKit
       1        576     576.0   WebKit::RemoteLayerTreeDrawingAreaProxyMac        C++     WebKit
       1        544     544.0   WebKit::RemoteLayerTreeEventDispatcher            C++     WebKit
       1        544     544.0   WebKit::ScrollingTreeFrameScrollingNodeRemoteMac  C++     WebKit
       1        512     512.0   CoreNLP::MeCabSubTokenizer                        C++     CoreNLP
       1        496     496.0   WebKit::GPUProcessProxy                           C++     WebKit
       1        448     448.0   AGXG16XFamilyRayTracingAccelerationStructure      ObjC    AGXMetalG16X
       1        448     448.0   MeCab::Writer                                     C++     libmecab.dylib
       1        448     448.0   NSDocumentController                              ObjC    AppKit
       1        448     448.0   NSISEngine                                        ObjC    CoreAutoLayout
       1        448     448.0   NSWorkspaceNotificationCenter                     ObjC    AppKit
       1        448     448.0   icu::RuleBasedBreakIterator                       C++     libicucore.A.dylib
       1        432     432.0   WebKit::RemoteScrollingTreeMac                    C++     WebKit
       1        384     384.0   BinaryCookieStorage                               C++     CFNetwork
       1        384     384.0   CUICommonAssetStorage._localizationdb (malloc)    C       CoreUI
       1        384     384.0   MeCab::Viterbi                                    C++     libmecab.dylib
       1        384     384.0   NSConcreteData (Bytes Storage)                    C       Foundation
       1        384     384.0   NSCoreTypesetter._reserved (malloc)               C       UIFoundation
       1        384     384.0   NSDocumentRevisionsController                     ObjC    AppKit
       1        384     384.0   NSKVONotifying_TaoApp                             ObjC    ferryx
       1        384     384.0   NSMenuBarTrackingSession                          ObjC    AppKit
       1        384     384.0   ScreenProviderCoordinator                         Swift   AppKit
       1        384     384.0   _CSStore                                          ObjC    CoreServicesStore
       1        384     384.0   _NSThemeZoomWidgetCell                            ObjC    AppKit
       1        384     384.0   icu::RegexMatcher                                 C++     libicucore.A.dylib
       1        320     320.0   AppManager                                        Swift   AppIntents
       1        320     320.0   BKSHIDEventDeliveryManager                        ObjC    BackBoardServices
       1        320     320.0   DDScanner                                         CFType  DataDetectorsCore
       1        320     320.0   MTLCompilerScheduler                              C++     Metal
       1        320     320.0   NSPanGestureRecognizer                            ObjC    AppKit
       1        320     320.0   NSScreen                                          ObjC    AppKit
       1        320     320.0   Swift._ContiguousArrayStorage<Swift.Bool>         Swift   libswiftCore.dylib
       1        320     320.0   Swift._ContiguousArrayStorage<SwiftUI.DynamicPropertyCache.Field>  Swift   libswiftCore.dylib
       1        320     320.0   Swift._DictionaryStorage<Network.SystemUUID, Network.NetworkAgent>  Swift   libswiftCore.dylib
       1        320     320.0   Swift._DictionaryStorage<Network.SystemUUID, Swift.UInt64>  Swift   libswiftCore.dylib
       1        320     320.0   Swift._SetStorage<Swift.OpaquePointer>            Swift   libswiftCore.dylib
       1        320     320.0   _NSThemeCloseWidgetCell                           ObjC    AppKit
       1        320     320.0   _NSThemeWidgetCell                                ObjC    AppKit
       1        320     320.0   __NSAppKitThreadSpecificData                      ObjC    AppKit
       1        256     256.0   AGXG16CDevice._supportedGPUFamilies (vector<MTLGPUFamily>)  C++     AGXMetalG16X
       1        256     256.0   CFHTTPMessage                                     CFType  CFNetwork
       1        256     256.0   GestureNodeCoordinator                            Swift   Gestures
       1        256     256.0   NSImmediateActionGestureRecognizer                ObjC    AppKit
       1        256     256.0   NSTextFieldCell                                   ObjC    AppKit
       1        256     256.0   NSTitleTextFieldCell                              ObjC    AppKit
       1        256     256.0   Swift._DictionaryStorage<__C.NSObject, AppKit.(NSWindowPocketAppearanceCoordinator in $18647df60).Pocket>  Swift   libswiftCore.dylib
       1        256     256.0   _CUIThemeGradientRendition                        ObjC    CoreUI
       1        256     256.0   _WKInspector                                      ObjC    WebKit
       1        256     256.0   icu::RuleBasedTokenizer                           C++     libicucore.A.dylib
       1        224     224.0   CAMetalDrawable                                   ObjC    QuartzCore
       1        224     224.0   CoreNLP::WordDispatchTagger                       C++     CoreNLP
       1        224     224.0   HIApplication                                     C++     HIToolbox
       1        224     224.0   MeCab::LatticeImpl                                C++     libmecab.dylib
       1        224     224.0   NSKVONotifying_NSWindowSectionContentController   ObjC    AppKit
       1        224     224.0   NSLayoutGuide                                     ObjC    AppKit
       1        224     224.0   NSScreenManager                                   Swift   AppKit
       1        224     224.0   NSStringDrawingContext                            ObjC    UIFoundation
       1        224     224.0   NWConcrete_nw_context.globals (struct nw_context_globals)  C       Network
       1        224     224.0   SPSafariPlatformSupport                           ObjC    SafariPlatformSupport
       1        224     224.0   Swift._DictionaryStorage<Swift.Int32, AppKit.ScreenMenuBar>  Swift   libswiftCore.dylib
       1        224     224.0   Swift._DictionaryStorage<Swift.ObjectIdentifier, SwiftUI.DynamicPropertyCache.Fields>  Swift   libswiftCore.dylib
       1        224     224.0   TLocalFontRegistryImp                             C++     libFontRegistry.dylib
       1        224     224.0   TUINSCursorUIController                           ObjC    TextInputUIMacHelper
       1        224     224.0   icu::CollationTailoring                           C++     libicucore.A.dylib
       1        224     224.0   icu::OlsonTimeZone                                C++     libicucore.A.dylib
       1        224     224.0   icu::RegexPattern                                 C++     libicucore.A.dylib
       1        224     224.0   std::__shared_ptr_emplace<SafeBrowsing::LookupContext>  C++     SafariSafeBrowsing
       1        192     192.0   AGXG16XFamilySampler                              ObjC    AGXMetalG16X
       1        192     192.0   APComponent_Carbon                                C++     AudioToolboxCore
       1        192     192.0   IPC::StreamServerConnection                       C++     WebKit
       1        192     192.0   NSAutoFillHeuristicController                     ObjC    AppKit
       1        192     192.0   NSCoreTypesetter                                  ObjC    UIFoundation
       1        192     192.0   NSResizeMoveHelper                                ObjC    AppKit
       1        192     192.0   NSTextAttachmentCell                              ObjC    AppKit
       1        192     192.0   WebCore::ScrollingTreeScrollingNodeDelegateMac    C++     WebCore
       1        192     192.0   _NSAutomaticFocusRingState                        ObjC    AppKit
       1        192     192.0   _WKProcessPoolConfiguration                       ObjC    WebKit
       1        160     160.0   DDLookupTable                                     CFType  DataDetectorsCore
       1        160     160.0   MTLPipelineDataCache                              C++     Metal
       1        160     160.0   NSAquaAppearance                                  ObjC    AppKit
       1        160     160.0   NSCGSDisplay                                      ObjC    AppKit
       1        160     160.0   NSDarkAquaAppearance                              ObjC    AppKit
       1        160     160.0   NSPersistentUIManager                             ObjC    AppKit
       1        160     160.0   NSSpellChecker                                    ObjC    AppKit
       1        160     160.0   NSSystemAppearance                                ObjC    AppKit
       1        160     160.0   NSWindowRepresentingMenuItem                      ObjC    AppKit
       1        160     160.0   NWConcrete_nw_context                             ObjC    Network
       1        160     160.0   PLClientLogger                                    ObjC    PowerLog
       1        160     160.0   RBSConnection                                     ObjC    RunningBoardServices
       1        160     160.0   SPCompletionListRemoteViewController              ObjC    SafariPlatformSupport
       1        160     160.0   Swift._ContiguousArrayStorage<AppKit.ScreenDisplay>  Swift   libswiftCore.dylib
       1        160     160.0   Swift._DictionaryStorage<Swift.ObjectIdentifier, Swift.UInt>  Swift   libswiftCore.dylib
       1        160     160.0   Swift._DictionaryStorage<Swift.String, Foundation._TimeZoneProtocol & Swift.AnyObject>  Swift   libswiftCore.dylib
       1        160     160.0   TUINSRemoteViewController                         ObjC    TextInputUIMacHelper
       1        160     160.0   TaoView.taoState (malloc)                         C       ferryx
       1        160     160.0   WKBackForwardListItem                             ObjC    WebKit
       1        160     160.0   WebKit::RemoteScrollingCoordinatorProxyMac        C++     WebKit
       1        160     160.0   WindowTabIndexer                                  Swift   AppKit
       1        160     160.0   std::__shared_ptr_emplace<AGX::HAL200::Sampler>   C++     AGXMetalG16X
       1        160     160.0   std::__shared_ptr_emplace<SafeBrowsing::BrowsingDatabaseCoordinator>  C++     SafariSafeBrowsing
       1        128     128.0   AppViewBridgeAggregator                           Swift   AppIntents
       1        128     128.0   CGSConnection                                     CFType  SkyLight
       1        128     128.0   CoreNLP::DefaultSubWordTagger                     C++     CoreNLP
       1        128     128.0   CoreNLP::ICUTextBreakWithCustomizedRules          C++     CoreNLP
       1        128     128.0   CoreNLP::KoreanSubWordTagger                      C++     CoreNLP
       1        128     128.0   CoreNLP::storage::JapaneseTokenStorage            C++     CoreNLP
       1        128     128.0   Foundation.LockedState<Foundation.TimeZoneCache.State>.(_Buffer in $18348d314)<>  Swift   Foundation
       1        128     128.0   IIO_Reader_WebP                                   C++     ImageIO
       1        128     128.0   IPC::StreamConnectionWorkQueue                    C++     WebKit
       1        128     128.0   MeCab::Allocator<mecab_node_t, mecab_path_t>      C++     libmecab.dylib
       1        128     128.0   NLStringTokenizer                                 CFType  CoreNLP
       1        128     128.0   NSCondition                                       ObjC    Foundation
       1        128     128.0   NSMenuBarLocalDisplayWindow                       ObjC    AppKit
       1        128     128.0   NSMenuBarPresentationInstance                     ObjC    AppKit
       1        128     128.0   NSTextAttachment                                  ObjC    UIFoundation
       1        128     128.0   SLSSkyLightKeyEventAuthenticationMessage          ObjC    SkyLight
       1        128     128.0   SLSSkyLightMouseEventAuthenticationMessage        ObjC    SkyLight
       1        128     128.0   Swift._DictionaryStorage<Swift.Int, Foundation._TimeZoneProtocol & Swift.AnyObject>  Swift   libswiftCore.dylib
       1        128     128.0   Swift._DictionaryStorage<Swift.String, Foundation.Date>  Swift   libswiftCore.dylib
       1        128     128.0   WKPreferenceObserver.m_userDefaults (struct)      C       WebKit
       1        128     128.0   WKUserContentController                           ObjC    WebKit
       1        128     128.0   WebKit::VideoPresentationManagerProxy             C++     WebKit
       1        128     128.0   WebKit::WebPaymentCoordinatorProxy                C++     WebKit
       1        112     112.0   AppManager.TypeCache                              Swift   AppIntents
       1        112     112.0   BSXPCServiceConnectionProxy<BKSHIDEventDeliveryManagerServerInterface>  ObjC    BoardServices
       1        112     112.0   BSXPCServiceConnectionProxy<CPXRemoteViewEventProtocolClientCallsServer>  ObjC    BoardServices
       1        112     112.0   BSXPCServiceConnectionProxy<WMXPCServerInterface>  ObjC    BoardServices
       1        112     112.0   CFURLStorageSession                               CFType  CFNetwork
       1        112     112.0   CGFunction                                        CFType  CoreGraphics
       1        112     112.0   IIOXPCClient                                      C++     ImageIO
       1        112     112.0   NSApplicationFunctionRowController                ObjC    AppKit
       1        112     112.0   NSISLinearExpression                              ObjC    CoreAutoLayout
       1        112     112.0   NSKVONotifying_NSWindowSectionController          ObjC    AppKit
       1        112     112.0   NSToolTipManager                                  ObjC    AppKit
       1        112     112.0   NSUndoManager                                     ObjC    Foundation
       1        112     112.0   RBSProcessHandle                                  ObjC    RunningBoardServices
       1        112     112.0   Swift._DictionaryStorage<Foundation.Calendar.Identifier, Foundation._CalendarProtocol & Swift.AnyObject>  Swift   libswiftCore.dylib
       1        112     112.0   Swift._DictionaryStorage<Swift.Int, Foundation._NSSwiftTimeZone>  Swift   libswiftCore.dylib
       1        112     112.0   Swift._DictionaryStorage<Swift.Int32, AppKit.ScreenEDRValues>  Swift   libswiftCore.dylib
       1        112     112.0   Swift._DictionaryStorage<SwiftUI.Solarium.EnablementIdiom, (SwiftUI.Solarium.EnablementLevel, SwiftUI.Solarium.EnablementCriteria)>  Swift   libswiftCore.dylib
       1        112     112.0   WebKit::NavigationState                           C++     WebKit
       1        112     112.0   WebKit::PageClientImpl                            C++     WebKit
       1        112     112.0   WebKit::WebAuthenticatorCoordinatorProxy          C++     WebKit
       1        112     112.0   WebKit::WebProcessCache                           C++     WebKit
       1        112     112.0   _CFXPreferences                                   ObjC    CoreFoundation
       1        112     112.0   _CTSplicedFontKey                                 ObjC    CoreText
       1        112     112.0   _NSAppleMenu                                      ObjC    AppKit
       1        112     112.0   _NSMenuToolTipManager                             ObjC    AppKit
       1         96      96.0   AccessibilitySupportOverrides                     ObjC    libAccessibility.dylib
       1         96      96.0   CGEventSource                                     CFType  SkyLight
       1         96      96.0   CGNotificationCenter                              CFType  CoreGraphics
       1         96      96.0   CUIPSDGradientEvaluator                           ObjC    CoreUI
       1         96      96.0   CUISharedArtCatalog                               ObjC    CoreUI
       1         96      96.0   CoreNLP::ICUTextBreakWithBuiltInRules             C++     CoreNLP
       1         96      96.0   DDCache                                           CFType  DataDetectorsCore
       1         96      96.0   HIEventDispatcher                                 C++     HIToolbox
       1         96      96.0   HIUserFocus                                       C++     HIToolbox
       1         96      96.0   IIO_ReaderHandler                                 C++     ImageIO
       1         96      96.0   MeCab::TaggerImpl                                 C++     libmecab.dylib
       1         96      96.0   MemoryCookieStorage                               C++     CFNetwork
       1         96      96.0   NSDisplayCyclePhaseCollection                     ObjC    AppKit
       1         96      96.0   NSDockTile                                        ObjC    AppKit
       1         96      96.0   NSISEngine._rowTables (malloc)                    C       CoreAutoLayout
       1         96      96.0   NSParagraphStyle                                  ObjC    UIFoundation
       1         96      96.0   NSPasteboard                                      ObjC    AppKit
       1         96      96.0   NSScrollerImpPair                                 ObjC    AppKit
       1         96      96.0   NSTempAttributeDictionary                         ObjC    UIFoundation
       1         96      96.0   NSTouchDevice                                     ObjC    AppKit
       1         96      96.0   NWConcrete_nw_protocol_definition.common_state (struct nw_protocol_definition_common_state)  C       Network
       1         96      96.0   RBSMacAppProcessIdentity                          ObjC    RunningBoardServices
       1         96      96.0   Swift._SetStorage<Swift.String>                   Swift   libswiftCore.dylib
       1         96      96.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(CalendarKey in $22ef9d2bc)>>  Swift   SwiftUI
       1         96      96.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(LocaleKey in $22ef9d2e0)>>  Swift   SwiftUI
       1         96      96.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(TimeZoneKey in $22ef9d298)>>  Swift   SwiftUI
       1         96      96.0   TUIKeyboardCandidateMultiplexer                   ObjC    TextInputUI
       1         96      96.0   WFObservableArrayResult                           ObjC    VoiceShortcutClient
       1         96      96.0   WKBackForwardList                                 ObjC    WebKit
       1         96      96.0   WebKit::PlaybackSessionManagerProxy               C++     WebKit
       1         96      96.0   WebKit::WebFullScreenManagerProxy                 C++     WebKit
       1         96      96.0   WebScrollerImpPairDelegateMac._scrollerPair (struct)  C       WebCore
       1         96      96.0   _LSDefaults                                       ObjC    LaunchServices
       1         96      96.0   _NSConcreteUserNotificationCenter                 ObjC    Foundation
       1         96      96.0   icu::RuleBasedCollator                            C++     libicucore.A.dylib
       1         96      96.0   wry::wkwebview::class::wry_navigation_delegate::WryNavigationDelegate0.55.1  ObjC    ferryx
       1         80      80.0   AFPreferences                                     ObjC    AssistantServices
       1         80      80.0   AXBBundleManager                                  ObjC    AccessibilityBundles
       1         80      80.0   BSRBSService                                      ObjC    BoardServices
       1         80      80.0   CUIRuntimeStatistics                              ObjC    CoreUI
       1         80      80.0   ColorSyncCMM                                      CFType  ColorSync
       1         80      80.0   CoreLockable                                      C++     CFNetwork
       1         80      80.0   FBSWorkspace                                      ObjC    FrontBoardServices
       1         80      80.0   Foundation.LockedState<Foundation._NSSwiftProcessInfo.State>.(_Buffer in $18348d314)<>  Swift   Foundation
       1         80      80.0   IFIconSpecification                               ObjC    IconFoundation
       1         80      80.0   ISVariantIconSpecification                        ObjC    IconFoundation
       1         80      80.0   Inspector::InspectorTargetAgent                   C++     JavaScriptCore
       1         80      80.0   MTLCompiler                                       ObjC    Metal
       1         80      80.0   NSFontManager                                     ObjC    AppKit
       1         80      80.0   NSIATextInputActionsContext                       ObjC    AppKit
       1         80      80.0   NSISEngine._colTables (malloc)                    C       CoreAutoLayout
       1         80      80.0   NSPersistentUIWindowSnapshotter                   ObjC    AppKit
       1         80      80.0   NWConcrete_nw_context.cache (struct nw_context_cache)  C       Network
       1         80      80.0   ScreenEDRCache                                    Swift   AppKit
       1         80      80.0   ScreenLocalization                                Swift   AppKit
       1         80      80.0   SecTask                                           CFType  Security
       1         80      80.0   Swift.ReferenceWritableKeyPath<AppKit._NSViewMaterialBackdropContext, Swift.Bool>  Swift   libswiftCore.dylib
       1         80      80.0   Swift.ReferenceWritableKeyPath<AppKit._NSViewMaterialBackdropContext, Swift.Optional<DesignLibrary.GlassMaterialProvider.Pocket>>  Swift   libswiftCore.dylib
       1         80      80.0   Swift.ReferenceWritableKeyPath<AppKit._NSViewMaterialBackdropContext, Swift.Optional<SwiftUI._Glass.Frost>>  Swift   libswiftCore.dylib
       1         80      80.0   Swift.ReferenceWritableKeyPath<DesignLibrary.GlassMaterialProvider.Pocket.Storage, DesignLibrary.GlassMaterialProvider.Pocket.Parameters>  Swift   libswiftCore.dylib
       1         80      80.0   Swift.WritableKeyPath<SwiftUI.EnvironmentValues, CoreGraphics.CGFloat>  Swift   libswiftCore.dylib
       1         80      80.0   Swift.WritableKeyPath<SwiftUI.EnvironmentValues, Swift.Optional<SwiftUI.Material>>  Swift   libswiftCore.dylib
       1         80      80.0   Swift.WritableKeyPath<SwiftUI.EnvironmentValues, SwiftUI.ColorScheme>  Swift   libswiftCore.dylib
       1         80      80.0   Swift._ContiguousArrayStorage<AppIntents.UndoManagerProvider>  Swift   libswiftCore.dylib
       1         80      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<(extension in SwiftUI):SwiftUI.EnvironmentValues.(__Key_glassDiffusion in $22ef8ee68)>>  Swift   SwiftUI
       1         80      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(AccessibilityButtonShapesKey in $22efa22e8)>>  Swift   SwiftUI
       1         80      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(AccessibilityDifferentiateWithoutColorKey in $22efa2330)>>  Swift   SwiftUI
       1         80      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(AccessibilityInvertColorsKey in $22efa239c)>>  Swift   SwiftUI
       1         80      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(AccessibilityReduceMotionKey in $22efa230c)>>  Swift   SwiftUI
       1         80      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(AccessibilityReduceTransparencyKey in $22efa2354)>>  Swift   SwiftUI
       1         80      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(DefaultAccentColorKey in $22ef6ad74)>>  Swift   SwiftUI
       1         80      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(IsLowPowerModeEnabledKey in $22ef9d680)>>  Swift   SwiftUI
       1         80      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(ReduceDesktopTintingKey in $22ef9d614)>>  Swift   SwiftUI
       1         80      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.(SystemAccentValueKey in $22ef6ad98)>>  Swift   SwiftUI
       1         80      80.0   SwiftUI.(TypedElement in $22ef97e24)<SwiftUI.EnvironmentPropertyKey<SwiftUI.EnvironmentValues.MaxAllowedDynamicRangeKey>>  Swift   SwiftUI
       1         80      80.0   TGlobalFontRegistryImp                            C++     libFontRegistry.dylib
       1         80      80.0   VCVoiceShortcutClient                             ObjC    VoiceShortcutClient
       1         80      80.0   WKContentWorld                                    ObjC    WebKit
       1         80      80.0   WPResources                                       ObjC    WebPrivacy
       1         80      80.0   WebKit::WebFrameInspectorTargetProxy              C++     WebKit
       1         80      80.0   WebKit::WebPageInspectorTargetProxy               C++     WebKit
       1         80      80.0   _NSGFGestureEnvironment                           ObjC    AppKit
       1         80      80.0   _NSImageTypeData                                  ObjC    AppKit
       1         80      80.0   _NSWidgetContentStyle                             ObjC    AppKit
       1         80      80.0   _NSWorkspacePowerNotifier                         ObjC    AppKit
       1         80      80.0   _NSWorkspacePowerNotifier._userActivityLevelNotificationHandle (malloc)  C       AppKit
       1         80      80.0   icu::Normalizer2Impl                              C++     libicucore.A.dylib
       1         80      80.0   xpc_session_t                                     ObjC    libxpc.dylib
       1         64      64.0   AFPreferencesNotificationCenter                   ObjC    AssistantServices
       1         64      64.0   AFSiriAvailability                                ObjC    AssistantServices
       1         64      64.0   AGXG16CDevice._libraryBuilder (malloc)            C       AGXMetalG16X
       1         64      64.0   BKSHIDEventBaseAttributes                         ObjC    BackBoardServices
       1         64      64.0   BSMachPortTaskNameRight                           ObjC    BaseBoard
       1         64      64.0   BSServiceMainRunLoopQueue                         ObjC    BoardServices
       1         64      64.0   CSIndexConnection                                 ObjC    CoreSpotlight
       1         64      64.0   CSPowerLog                                        ObjC    CoreSpotlight
       1         64      64.0   DDScanQuery                                       CFType  DataDetectorsCore
       1         64      64.0   ExclusionPool                                     Swift   Gestures
       1         64      64.0   FPInFlightDrawableLifetime                        ObjC    FramePacing
       1         64      64.0   Foundation.LockedState<Foundation.LocaleCache.State>.(_Buffer in $18348d314)<>  Swift   Foundation
       1         64      64.0   GestureNodeCoordinatorShim                        Swift   Gestures
       1         64      64.0   IATextInputActionsAnalytics                       ObjC    InputAnalytics
       1         64      64.0   IATextInputActionsInputMode                       ObjC    InputAnalytics
       1         64      64.0   IIO_Reader_AI                                     C++     ImageIO
       1         64      64.0   IIO_Reader_ATX                                    C++     ImageIO
       1         64      64.0   IIO_Reader_AppleJPEG                              C++     ImageIO
       1         64      64.0   IIO_Reader_BMP                                    C++     ImageIO
       1         64      64.0   IIO_Reader_CUR                                    C++     ImageIO
       1         64      64.0   IIO_Reader_GIF                                    C++     ImageIO
       1         64      64.0   IIO_Reader_ICNS                                   C++     ImageIO
       1         64      64.0   IIO_Reader_ICO                                    C++     ImageIO
       1         64      64.0   IIO_Reader_JP2                                    C++     ImageIO
       1         64      64.0   IIO_Reader_KTX                                    C++     ImageIO
       1         64      64.0   IIO_Reader_KTX2                                   C++     ImageIO
       1         64      64.0   IIO_Reader_LibJPEG                                C++     ImageIO
       1         64      64.0   IIO_Reader_MPO                                    C++     ImageIO
       1         64      64.0   IIO_Reader_OpenEXR                                C++     ImageIO
       1         64      64.0   IIO_Reader_PBM                                    C++     ImageIO
       1         64      64.0   IIO_Reader_PDF                                    C++     ImageIO
       1         64      64.0   IIO_Reader_PICT                                   C++     ImageIO
       1         64      64.0   IIO_Reader_PNG                                    C++     ImageIO
       1         64      64.0   IIO_Reader_PSD                                    C++     ImageIO
       1         64      64.0   IIO_Reader_RAD                                    C++     ImageIO
       1         64      64.0   IIO_Reader_SGI                                    C++     ImageIO
       1         64      64.0   IIO_Reader_TGA                                    C++     ImageIO
       1         64      64.0   IIO_Reader_TIFF                                   C++     ImageIO
       1         64      64.0   ISDefaults                                        ObjC    IconServices
       1         64      64.0   ISGenericDocumentIcon                             ObjC    IconServices
       1         64      64.0   ISIconManager                                     ObjC    IconServices
       1         64      64.0   IntelligenceCollectionListener                    Swift   UIIntelligenceSupport
       1         64      64.0   LSSharedMemoryRef                                 CFType  LaunchServices
       1         64      64.0   MTLIOAccelService                                 ObjC    Metal
       1         64      64.0   MTLIOAccelServiceGlobalContext                    ObjC    Metal
       1         64      64.0   MTLLoader                                         ObjC    Metal
       1         64      64.0   MTLLoader._global (malloc)                        C       Metal
       1         64      64.0   MeCab::FreeList<mecab_node_t>                     C++     libmecab.dylib
       1         64      64.0   MeCab::ModelImpl                                  C++     libmecab.dylib
       1         64      64.0   NSAffineTransform                                 ObjC    Foundation
       1         64      64.0   NSDockConnection                                  ObjC    AppKit
       1         64      64.0   NSHelpManager                                     ObjC    AppKit
       1         64      64.0   NSPersistentUIFlushScheduler                      ObjC    AppKit
       1         64      64.0   NSRegularExpression._internal (malloc)            C       Foundation
       1         64      64.0   NSRunLoop                                         ObjC    CoreFoundation
       1         64      64.0   NamedImage.Cache                                  Swift   SwiftUICore
       1         64      64.0   PKExternalProviders                               ObjC    PlugInKit
       1         64      64.0   PKHost                                            ObjC    PlugInKit
       1         64      64.0   RBSProcessBundle                                  ObjC    RunningBoardServices
       1         64      64.0   RemoteCacheable                                   C++     CarbonCore
       1         64      64.0   SCService                                         C++     CarbonCore
       1         64      64.0   Swift.KeyPath<DesignLibrary.GlassMaterialProvider.Pocket.Storage, DesignLibrary.GlassMaterialProvider.Pocket.ResolvedParameters>  Swift   libswiftCore.dylib
       1         64      64.0   Swift.KeyPath<SwiftUI.EnvironmentValues, SwiftUI.ColorSchemeContrast>  Swift   libswiftCore.dylib
       1         64      64.0   Swift.KeyPath<__C._WMWindow, Swift.Optional<__C._WMWindowTilingState>>  Swift   libswiftCore.dylib
       1         64      64.0   Swift.ManagedBuffer<UIIntelligenceSupport.IntelligenceSupportAgentXPCConnection.State, __C.os_unfair_lock_s>  Swift   libswiftCore.dylib
       1         64      64.0   Swift._ContiguousArrayStorage<Foundation.URL>     Swift   libswiftCore.dylib
       1         64      64.0   Swift._ContiguousArrayStorage<GenerativeModels.FoundationModelsCompatibilityVersionsInfo.Version>  Swift   libswiftCore.dylib
       1         64      64.0   Swift._ContiguousArrayStorage<Swift.Array<Swift.UInt8>>  Swift   libswiftCore.dylib
       1         64      64.0   Swift._ContiguousArrayStorage<SwiftUI.AlignmentID.Protocol>  Swift   libswiftCore.dylib
       1         64      64.0   TaoWindowDelegate.taoState (malloc)               C       ferryx
       1         64      64.0   VCAccessSpecifier                                 ObjC    VoiceShortcutClient
       1         64      64.0   WMClientWindowManager                             ObjC    WindowManagement
       1         64      64.0   WebKit::AboutSchemeHandler                        C++     WebKit
       1         64      64.0   WebKit::WebPageDebuggable                         C++     WebKit
       1         64      64.0   WebKit::WebPageProxyFrameLoadStateObserver        C++     WebKit
       1         64      64.0   WebKit::WebScreenOrientationManagerProxy          C++     WebKit
       1         64      64.0   _CFXPreferencesHandle                             ObjC    CoreFoundation
       1         64      64.0   _NSDocumentRecentItemsMenuController              ObjC    AppKit
       1         64      64.0   _NSMainThread                                     ObjC    Foundation
       1         64      64.0   _NSSwiftProcessInfo                               Swift   Foundation
       1         64      64.0   _NSTextCompletionContext                          ObjC    AppKit
       1         64      64.0   __NSBundleTables                                  ObjC    Foundation
       1         64      64.0   dd_icu_3_6__5_0_2::RuleBasedClassifier            C++     DataDetectorsCore
       1         64      64.0   icu::UnifiedCache                                 C++     libicucore.A.dylib
       1         64      64.0   std::__function::__func<bool (*)(mecab_node_t const*), bool (mecab_node_t const*)>  C++     CoreNLP
       1         64      64.0   std::__shared_ptr_emplace<FrameworkConfiguration>  C++     CoreAnalytics
       1         64      64.0   wry::wkwebview::class::wry_download_delegate::WryDownloadDelegate0.55.1  ObjC    ferryx
       1         64      64.0   wry::wkwebview::class::wry_navigation_delegate::WryNavigationDelegate0.55.1.ivars (malloc)  C       ferryx
       1         48      48.0   ..NSKVONotifying_wry::wkwebview::class::wry_web_view::WryWebView0.55.1._cachedActiveNSURL (struct)  C       ferryx
       1         48      48.0   ..NSKVONotifying_wry::wkwebview::class::wry_web_view::WryWebView0.55.1._uiDelegate (unique_ptr<WebKit::UIDelegate>)  C++     ferryx
       1         48      48.0   AGX::HAL200::EncoderComputeServiceCDMSubstreamProcessor  C++     AGXMetalG16X
       1         48      48.0   AGXG16CDevice._commandBufferStoragePool (struct IOGPUMetalCommandBufferStoragePool)  C       AGXMetalG16X
       1         48      48.0   AKAuthorizationDaemonConnection                   ObjC    AuthKit
       1         48      48.0   BKSHIDEventDeferringRule                          ObjC    BackBoardServices
       1         48      48.0   BSMutableServiceInterface                         ObjC    BoardServices
       1         48      48.0   BSSimpleAssertion                                 ObjC    BaseBoard
       1         48      48.0   CFError                                           ObjC    CoreFoundation
       1         48      48.0   CUIThemeGradient                                  ObjC    CoreUI
       1         48      48.0   CollectionsInternal._DequeBuffer<()>              Swift   CollectionsInternal
       1         48      48.0   CompactCookieArray                                CFType  CFNetwork
       1         48      48.0   CoreNLP::storage::OpenerImpl                      C++     CoreNLP
       1         48      48.0   DDActionsManager                                  ObjC    DataDetectors
       1         48      48.0   FailureDependencyGraph                            Swift   Gestures
       1         48      48.0   Foundation.LockedState<Foundation._NSSwiftProcessInfo.GlobalState>.(_Buffer in $18348d314)<>  Swift   Foundation
       1         48      48.0   Foundation.LockedState<Foundation._ProcessInfo.State>.(_Buffer in $18348d314)<>  Swift   Foundation
       1         48      48.0   Foundation.LockedState<Swift.Optional<Foundation._CalendarProtocol & Swift.AnyObject>>.(_Buffer in $18348d314)<>  Swift   Foundation
       1         48      48.0   Foundation.LockedState<Swift.Optional<Foundation._LocaleProtocol & Swift.AnyObject>>.(_Buffer in $18348d314)<>  Swift   Foundation
       1         48      48.0   Foundation.LockedState<Swift.Optional<Swift.String>>.(_Buffer in $18348d314)<>  Swift   Foundation
       1         48      48.0   GLRResourceListPool                               C++     AppleMetalOpenGLRenderer
       1         48      48.0   GlobalEnvironment                                 Swift   AppKit
       1         48      48.0   IFImage                                           ObjC    IconFoundation
       1         48      48.0   IOSurfaceSharedEvent                              ObjC    IOSurface
       1         48      48.0   ISIconCache                                       ObjC    IconServices
       1         48      48.0   ISStore                                           ObjC    IconServices
       1         48      48.0   ISStoreIndex                                      ObjC    IconServices
       1         48      48.0   Inspector::TargetBackendDispatcher                C++     JavaScriptCore
       1         48      48.0   IntelligenceSupportAgentXPCConnection             Swift   UIIntelligenceSupport
       1         48      48.0   LNAppConnectionListener                           ObjC    AppIntents
       1         48      48.0   LSApplicationWorkspaceRemoteObserver              ObjC    LaunchServices
       1         48      48.0   MTLIOAccelService._notifyPort (struct IONotificationPort)  C       Metal
       1         48      48.0   MTLIOAccelServiceGlobalContext._deviceNotifyPort (struct IONotificationPort)  C       Metal
       1         48      48.0   NSApplicationSceneWorkspace                       ObjC    AppKit
       1         48      48.0   NSCGSDisplayConfiguration                         ObjC    AppKit
       1         48      48.0   NSCGSScreenDisplaySetProvider                     Swift   AppKit
       1         48      48.0   NSCGSScreenEDRProvider                            Swift   AppKit
       1         48      48.0   NSConcretePointerFunctions                        ObjC    Foundation
       1         48      48.0   NSConditionLock                                   ObjC    Foundation
       1         48      48.0   NSCoreDragManager                                 ObjC    AppKit
       1         48      48.0   NSCustomDynamicColor                              ObjC    AppKit
       1         48      48.0   NSDictationManager                                ObjC    AppKit
       1         48      48.0   NSHTTPCookieStorageInternal                       ObjC    CFNetwork
       1         48      48.0   NSKeyValueShareableObservanceKey                  ObjC    Foundation
       1         48      48.0   NSMenuKeyCache                                    Swift   AppKit
       1         48      48.0   NSPersistentUIRemoteStorageClient                 ObjC    AppKit
       1         48      48.0   NSRegularExpression                               ObjC    Foundation
       1         48      48.0   NSTextGenerationReceiver                          ObjC    AppKit
       1         48      48.0   NSUserDefaults                                    ObjC    CoreFoundation
       1         48      48.0   NSUserDefaults.Global                             Swift   GenerativeModelsFoundation
       1         48      48.0   NSWMStageInfo                                     ObjC    AppKit
       1         48      48.0   NSWMWindowCoordinator                             ObjC    AppKit
       1         48      48.0   NSWindowPocketAppearanceCoordinator               Swift   AppKit
       1         48      48.0   NetworkAgentCache                                 Swift   Network
       1         48      48.0   ObservationCenter                                 Swift   SwiftUICore
       1         48      48.0   PAL::SystemSleepListenerMac                       C++     WebCore
       1         48      48.0   RBSService                                        ObjC    RunningBoardServices
       1         48      48.0   RIPData                                           CFType  CoreGraphics
       1         48      48.0   SLSRemoteViewEventClient                          ObjC    SkyLight
       1         48      48.0   ScriptMessageHandlerDelegate                      C++     WebKit
       1         48      48.0   Swift.BridgingBufferStorage                       Swift   libswiftCore.dylib
       1         48      48.0   Swift.ManagedBuffer<AppKit.NSWMDeferredTransactionQueue.(State in $18647bb28), __C.os_unfair_lock_s>  Swift   libswiftCore.dylib
       1         48      48.0   Swift.ManagedBuffer<Foundation.Calendar, __C.os_unfair_lock_s>  Swift   libswiftCore.dylib
       1         48      48.0   Swift.ManagedBuffer<UIIntelligenceSupport.IntelligenceCollectionCoordinator.(State in $277f14fb4), __C.os_unfair_lock_s>  Swift   libswiftCore.dylib
       1         48      48.0   Swift.WritableKeyPath<AppKit.(NSWindowPocketAppearanceCoordinator in $18647df60).Pocket, Swift.Int>  Swift   libswiftCore.dylib
       1         48      48.0   Swift._ContiguousArrayStorage<AppIntents.AppViewBridge & Swift.AnyObject>  Swift   libswiftCore.dylib
       1         48      48.0   Swift._ContiguousArrayStorage<__C.NSScreen>       Swift   libswiftCore.dylib
       1         48      48.0   SwiftUI.(AtomicBuffer in $22efa14ec)<SwiftUI.ObjectCache<(extension in SwiftUI):SwiftUI.Color.ResolvedHDR, __C.CGColorRef>.(Data in $22ef99e08)<>>  Swift   SwiftUI
       1         48      48.0   SwiftUI.ObjectCache<(extension in SwiftUI):SwiftUI.Color.ResolvedHDR, __C.CGColorRef>  Swift   SwiftUI
       1         48      48.0   TFileFragmentCache                                C++     libFontParser.dylib
       1         48      48.0   TUINSCursorUISwitcher                             ObjC    TextInputUIMacHelper
       1         48      48.0   TaoAppDelegateParent                              ObjC    ferryx
       1         48      48.0   TaoAppDelegateParent.auxState (malloc)            C       ferryx
       1         48      48.0   TaoWindowDelegate                                 ObjC    ferryx
       1         48      48.0   UMUserManager                                     ObjC    UserManagement
       1         48      48.0   WFVoiceShortcutCache                              ObjC    VoiceShortcutClient
       1         48      48.0   WFWorkflowQuery                                   ObjC    VoiceShortcutClient
       1         48      48.0   WKPanGestureController                            ObjC    WebKit
       1         48      48.0   WKViewViewSizeLayoutStrategy                      ObjC    WebKit
       1         48      48.0   WTF::Detail::CallableWrapper<WTF::RunLoop::Timer::Timer<WebCore::IOSurfacePool>(WTF::Ref<WTF::RunLoop, WTF::RawPtrTraits<WTF::RunLoop>, WTF::DefaultRefDerefTraits<WTF::RunLoop>>&&, WTF::ASCIILiteral, WebCore::IOSurfacePool*, void (WebCore::IOSurfacePool::*)())::'lambda'(), void>  C++     WebCore
       1         48      48.0   WebCore::WheelEventDeltaFilterMac                 C++     WebCore
       1         48      48.0   WebKit::LogStream                                 C++     WebKit
       1         48      48.0   WebKit::WebScriptMessageHandler                   C++     WebKit
       1         48      48.0   XPCRelaunchNotificationHandler                    Swift   UIIntelligenceSupport
       1         48      48.0   _NSFocusStackElement                              ObjC    AppKit
       1         48      48.0   _NSServicesPrincipalMenuUpdater                   ObjC    AppKit
       1         48      48.0   _NSServicesShortcutsState                         ObjC    AppKit
       1         48      48.0   _NSWorkspacePowerNotifier._notificationPort (struct IONotificationPort)  C       AppKit
       1         48      48.0   _TimeZoneICU                                      Swift   Foundation
       1         48      48.0   icu::Locale                                       C++     libicucore.A.dylib
       1         48      48.0   icu::UVector                                      C++     libicucore.A.dylib
       1         48      48.0   std::__function::__func<void CASmartPreferences::AddHandler<long long>(__CFString const*, __CFString const*, long long (*)(void const*, bool&), std::function<void (long long)>)::'lambda'(void const*), bool (void const*)>  C++     AudioToolboxCore
       1         48      48.0   std::__shared_ptr_pointer<CGSBacktrace*, void (*)(CGSBacktrace*)>  C++     SkyLight
       1         48      48.0   wry::wkwebview::class::wry_web_view_delegate::WryWebViewDelegate0.55.1  ObjC    ferryx
       1         48      48.0   wry::wkwebview::class::wry_web_view_ui_delegate::WryWebViewUIDelegate0.55.1  ObjC    ferryx
       1         48      48.0   wry::wkwebview::class::wry_web_view_ui_delegate::WryWebViewUIDelegate0.55.1.ivars (malloc)  C       ferryx
       1         32      32.0   ..NSKVONotifying_wry::wkwebview::class::wry_web_view::WryWebView0.55.1._iconLoadingDelegate (unique_ptr<WebKit::IconLoadingDelegate>)  C++     ferryx
       1         32      32.0   ..NSKVONotifying_wry::wkwebview::class::wry_web_view::WryWebView0.55.1._resourceLoadDelegate (unique_ptr<WebKit::ResourceLoadDelegate>)  C++     ferryx
       1         32      32.0   AFInstanceInfo                                    ObjC    AssistantServices
       1         32      32.0   AFSystemAssistantExperienceStatusManager          ObjC    AssistantServices
       1         32      32.0   AKAuthorizationController                         ObjC    AuthKit
       1         32      32.0   AppAppKitBridge                                   Swift   _AppIntents_AppKit
       1         32      32.0   AppManagerXPCCache                                Swift   AppIntents
       1         32      32.0   AvailabilityNotificationObservation               Swift   GenerativeModels
       1         32      32.0   BKSHIDEventDeferringPredicate                     ObjC    BackBoardServices
       1         32      32.0   BKSHIDEventDeferringTarget                        ObjC    BackBoardServices
       1         32      32.0   BSMachServiceAliases                              ObjC    BaseBoard
       1         32      32.0   BSServiceCompoundQueue                            ObjC    BoardServices
       1         32      32.0   BSServiceQuality                                  ObjC    BoardServices
       1         32      32.0   BSServicesConfiguration                           ObjC    BoardServices
       1         32      32.0   CA::Render::NamedFunction                         C++     QuartzCore
       1         32      32.0   CABasicAnimation                                  ObjC    QuartzCore
       1         32      32.0   CAGradientLayer                                   ObjC    QuartzCore
       1         32      32.0   CATransition                                      ObjC    QuartzCore
       1         32      32.0   CAValueFunction                                   ObjC    QuartzCore
       1         32      32.0   CFMainExecutor                                    Swift   libswift_Concurrency.dylib
       1         32      32.0   CoreNLP::mecab::KoreanNameTokenizer               C++     CoreNLP
       1         32      32.0   DiskCookieStorage::Journal                        C++     CFNetwork
       1         32      32.0   ExclusionRelationCache                            Swift   Gestures
       1         32      32.0   FBSWorkspaceCoupler                               ObjC    FrontBoardServices
       1         32      32.0   FPSupport_PowerStateSingleton                     ObjC    MediaToolbox
       1         32      32.0   FluidAnimationManager                             Swift   AppKit
       1         32      32.0   Foundation.LockedState<Foundation._TimeZoneICU.State>.(_Buffer in $18348d314)<>  Swift   Foundation
       1         32      32.0   Foundation.LockedState<Swift.Dictionary<Foundation.Calendar.Identifier, Foundation._CalendarProtocol & Swift.AnyObject>>.(_Buffer in $18348d314)<>  Swift   Foundation
       1         32      32.0   Foundation.LockedState<Swift.Optional<Foundation._NSSwiftLocale>>.(_Buffer in $18348d314)<>  Swift   Foundation
       1         32      32.0   Foundation.LockedState<Swift.UnsafePointer<Swift.Optional<Swift.UnsafeMutableRawPointer>>>.(_Buffer in $18348d314)<>  Swift   Foundation
       1         32      32.0   GenerativeModelsAvailability.Notifications        Swift   GenerativeModels
       1         32      32.0   HTTPHeaderDict                                    CFType  CFNetwork
       1         32      32.0   IIOMemoryHash                                     C++     ImageIO
       1         32      32.0   IOGPUMemoryInfo                                   ObjC    IOGPU
       1         32      32.0   IPC::FunctionDispatcherQueue                      C++     WebKit
       1         32      32.0   ISIconFactory                                     ObjC    IconServices
       1         32      32.0   ISImageCache                                      ObjC    IconServices
       1         32      32.0   IntelligenceCollectionCoordinator                 Swift   UIIntelligenceSupport
       1         32      32.0   LNProcessInstanceRegistryClient                   ObjC    AppIntents
       1         32      32.0   NSAccessibilityNotificationTable                  ObjC    AppKit
       1         32      32.0   NSApplicationIntelligence                         Swift   AppKit
       1         32      32.0   NSCGSScreenEDRObserver                            Swift   AppKit
       1         32      32.0   NSCellMouseTrackingInfo                           ObjC    AppKit
       1         32      32.0   NSCoreDockScreenDockProvider                      Swift   AppKit
       1         32      32.0   NSFileManager                                     ObjC    Foundation
       1         32      32.0   NSFocusStack                                      ObjC    AppKit
       1         32      32.0   NSHTTPCookieStorage                               ObjC    CFNetwork
       1         32      32.0   NSIATransliterationState                          ObjC    AppKit
       1         32      32.0   NSISEngine._engineVarTable (struct)               C       CoreAutoLayout
       1         32      32.0   NSISEngine._variablesObservations (struct NSISObjectTable)  C       CoreAutoLayout
       1         32      32.0   NSImageCatalogRepProvider                         ObjC    AppKit
       1         32      32.0   NSImageISIconRepProvider                          ObjC    AppKit
       1         32      32.0   NSIndexPath                                       ObjC    Foundation
       1         32      32.0   NSInputAnalytics                                  ObjC    AppKit
       1         32      32.0   NSInputAnalyticsProxy                             ObjC    AppKit
       1         32      32.0   NSKVONotifying_NSSystemAppearanceProxy            ObjC    AppKit
       1         32      32.0   NSKVONotifying_TaoWindow._sizeLimits (struct CGSize)  C       ferryx
       1         32      32.0   NSLifeguard                                       ObjC    AppKit
       1         32      32.0   NSMenuBarDisplayManager                           ObjC    AppKit
       1         32      32.0   NSMenuBarWindowManager                            ObjC    AppKit
       1         32      32.0   NSMenuKEUniquer                                   ObjC    AppKit
       1         32      32.0   NSMutableURLRequest                               ObjC    CFNetwork
       1         32      32.0   NSPersistentUICrashHistory                        ObjC    AppKit
       1         32      32.0   NSScreenLayout                                    ObjC    AppKit
       1         32      32.0   NSSelfExpression                                  ObjC    Foundation
       1         32      32.0   NSTextMathCompletionReceiver                      ObjC    AppKit
       1         32      32.0   NSTrackingAreaReservedIVars                       ObjC    AppKit
       1         32      32.0   NSUIActivityManager                               ObjC    AppKit
       1         32      32.0   NSURLRequest                                      ObjC    CFNetwork
       1         32      32.0   NSVBHostAppAuxiliaryConnection                    ObjC    ViewBridge
       1         32      32.0   NSWindowMenuItem                                  ObjC    AppKit
       1         32      32.0   NSWindowRestorationOptions                        ObjC    AppKit
       1         32      32.0   NSXPCSpellServerProxyClient                       ObjC    AppKit
       1         32      32.0   NWConcrete_nw_context.identifier (char[])         C       Network
       1         32      32.0   NetworkContext                                    Swift   Network
       1         32      32.0   OS_nw_dictionary                                  ObjC    Network
       1         32      32.0   PKDaemonClient                                    ObjC    PlugInKit
       1         32      32.0   ParserBlendDictionary                             C++     libFontParser.dylib
       1         32      32.0   ParserPrivateBlendDictionary                      C++     libFontParser.dylib
       1         32      32.0   RBSProcessInstance                                ObjC    RunningBoardServices
       1         32      32.0   RBSWorkloop                                       ObjC    RunningBoardServices
       1         32      32.0   RunLoopUpdateDriver                               Swift   Gestures
       1         32      32.0   SLSIconAppearanceConfiguration                    ObjC    SkyLight
       1         32      32.0   SLSecureCursorAssertionManager                    ObjC    SkyLight
       1         32      32.0   SOConfigurationClient                             ObjC    AppSSOCore
       1         32      32.0   SOConfigurationVersion                            ObjC    AppSSOCore
       1         32      32.0   SOServiceConnection                               ObjC    AppSSOCore
       1         32      32.0   SSBLookupContext                                  ObjC    SafariSafeBrowsing
       1         32      32.0   Swift.ManagedBuffer<GenerativeModels.GenerativeModelsAvailability.HardwareEligibilityCache, __C.os_unfair_lock_s>  Swift   libswiftCore.dylib
       1         32      32.0   Swift.SwiftDeferredNSArray                        Swift   libswiftCore.dylib
       1         32      32.0   SwiftUI.ColorBox<SwiftUI.AppKitPlatformColorProvider>  Swift   SwiftUI
       1         32      32.0   SwiftUI.MutableBox<Swift.Dictionary<Swift.ObjectIdentifier, SwiftUI.DynamicPropertyCache.Fields>>  Swift   SwiftUI
       1         32      32.0   SwiftUI.ThreadSpecific<SwiftUI.ObservationCenter>  Swift   SwiftUI
       1         32      32.0   TFontFamilyNameAliasesIndex                       C++     libFontRegistry.dylib
       1         32      32.0   TFontFamilyNameIndex                              C++     libFontRegistry.dylib
       1         32      32.0   TFontFullNameIndex                                C++     libFontRegistry.dylib
       1         32      32.0   TFontPostScriptNameAliasesIndex                   C++     libFontRegistry.dylib
       1         32      32.0   TFontPostScriptNameIndex                          C++     libFontRegistry.dylib
       1         32      32.0   TOS2UnicodeRanges                                 C++     libFontParser.dylib
       1         32      32.0   TParserPrivateDictionary                          C++     libFontParser.dylib
       1         32      32.0   TUICursorAccessoryAssertionController             ObjC    TextInputUIMacHelper
       1         32      32.0   TUINSCursorLocationCache                          ObjC    TextInputUIMacHelper
       1         32      32.0   UndoManagerCache                                  Swift   AppIntents
       1         32      32.0   WFDatabaseResultState                             ObjC    VoiceShortcutClient
       1         32      32.0   WFImageCache                                      ObjC    VoiceShortcutClient
       1         32      32.0   WKPreferenceObserver                              ObjC    WebKit
       1         32      32.0   WKWindowVisibilityObserver                        ObjC    WebKit
       1         32      32.0   WTF::Detail::CallableWrapper<WTF::RunLoop::Timer::Timer<WebCore::GameControllerGamepadProvider>(WTF::Ref<WTF::RunLoop, WTF::RawPtrTraits<WTF::RunLoop>, WTF::DefaultRefDerefTraits<WTF::RunLoop>>&&, WTF::ASCIILiteral, WebCore::GameControllerGamepadProvider*, void (WebCore::GameControllerGamepadProvider::*)())::'lambda'(), void>  C++     WebCore
       1         32      32.0   WTF::Detail::CallableWrapper<WTF::RunLoop::Timer::Timer<WebKit::AuthenticatorManager>(WTF::Ref<WTF::RunLoop, WTF::RawPtrTraits<WTF::RunLoop>, WTF::DefaultRefDerefTraits<WTF::RunLoop>>&&, WTF::ASCIILiteral, WebKit::AuthenticatorManager*, void (WebKit::AuthenticatorManager::*)())::'lambda'(), void>  C++     WebKit
       1         32      32.0   WTF::Detail::CallableWrapper<WTF::RunLoop::Timer::Timer<WebKit::DrawingAreaProxy>(WTF::Ref<WTF::RunLoop, WTF::RawPtrTraits<WTF::RunLoop>, WTF::DefaultRefDerefTraits<WTF::RunLoop>>&&, WTF::ASCIILiteral, WebKit::DrawingAreaProxy*, void (WebKit::DrawingAreaProxy::*)())::'lambda'(), void>  C++     WebKit
       1         32      32.0   WTF::Detail::CallableWrapper<WTF::RunLoop::Timer::Timer<WebKit::NavigationState>(WTF::Ref<WTF::RunLoop, WTF::RawPtrTraits<WTF::RunLoop>, WTF::DefaultRefDerefTraits<WTF::RunLoop>>&&, WTF::ASCIILiteral, WebKit::NavigationState*, void (WebKit::NavigationState::*)())::'lambda'(), void>  C++     WebKit
       1         32      32.0   WTF::Detail::CallableWrapper<WTF::RunLoop::Timer::Timer<WebKit::PerActivityStateCPUUsageSampler>(WTF::Ref<WTF::RunLoop, WTF::RawPtrTraits<WTF::RunLoop>, WTF::DefaultRefDerefTraits<WTF::RunLoop>>&&, WTF::ASCIILiteral, WebKit::PerActivityStateCPUUsageSampler*, void (WebKit::PerActivityStateCPUUsageSampler::*)())::'lambda'(), void>  C++     WebKit
       1         32      32.0   WTF::Detail::CallableWrapper<WTF::RunLoop::Timer::Timer<WebKit::UIGamepadProvider>(WTF::Ref<WTF::RunLoop, WTF::RawPtrTraits<WTF::RunLoop>, WTF::DefaultRefDerefTraits<WTF::RunLoop>>&&, WTF::ASCIILiteral, WebKit::UIGamepadProvider*, void (WebKit::UIGamepadProvider::*)())::'lambda'(), void>  C++     WebKit
       1         32      32.0   WTF::Detail::CallableWrapper<WTF::RunLoop::Timer::Timer<WebKit::WebInspectorUIProxy>(WTF::Ref<WTF::RunLoop, WTF::RawPtrTraits<WTF::RunLoop>, WTF::DefaultRefDerefTraits<WTF::RunLoop>>&&, WTF::ASCIILiteral, WebKit::WebInspectorUIProxy*, void (WebKit::WebInspectorUIProxy::*)())::'lambda'(), void>  C++     WebKit
       1         32      32.0   WTF::Detail::CallableWrapper<WTF::RunLoop::Timer::Timer<WebKit::WebProcessCache>(WTF::Ref<WTF::RunLoop, WTF::RawPtrTraits<WTF::RunLoop>, WTF::DefaultRefDerefTraits<WTF::RunLoop>>&&, WTF::ASCIILiteral, WebKit::WebProcessCache*, void (WebKit::WebProcessCache::*)())::'lambda'(), void>  C++     WebKit
       1         32      32.0   WTF::Detail::CallableWrapper<WTF::RunLoop::create(WTF::ASCIILiteral, WTF::ThreadType, WTF::Thread::QOS)::$_0, void>  C++     JavaScriptCore
       1         32      32.0   WebCore::LocalWebLockRegistry                     C++     WebCore
       1         32      32.0   WebKit::DiagnosticLoggingClient                   C++     WebKit
       1         32      32.0   WebKit::DisplayLinkProcessProxyClient             C++     WebKit
       1         32      32.0   WebKit::FindClient                                C++     WebKit
       1         32      32.0   WebKit::FullscreenClient                          C++     WebKit
       1         32      32.0   WebKit::NetworkProcessProxy::XPCEventHandler      C++     WebKit
       1         32      32.0   WebKit::RemoteLayerTreeDisplayLinkClient          C++     WebKit
       1         32      32.0   WebKit::RemoteLayerTreeEventDispatcherDisplayLinkClient  C++     WebKit
       1         32      32.0   WebKit::SecItemShimProxy                          C++     WebKit
       1         32      32.0   WebKit::UIDelegate::UIClient                      C++     WebKit
       1         32      32.0   WebKit::WebInspectorBackendProxy                  C++     WebKit
       1         32      32.0   WebKit::WebLockRegistryProxy                      C++     WebKit
       1         32      32.0   WebKit::WebPageProxyTesting                       C++     WebKit
       1         32      32.0   WebKit::WebPermissionControllerProxy              C++     WebKit
       1         32      32.0   WebKit::WebProcessProxy::WebProcessXPCEventHandler  C++     WebKit
       1         32      32.0   WebScrollerImpPairDelegateMac                     ObjC    WebCore
       1         32      32.0   XPCSession                                        Swift   libswiftXPC.dylib
       1         32      32.0   XTFontStaticRegistry                              ObjC    libFontRegistry.dylib
       1         32      32.0   _CSStore2DataContainer                            ObjC    CoreServicesStore
       1         32      32.0   _LSDServiceDomain                                 ObjC    LaunchServices
       1         32      32.0   _MTLPipelineCache                                 ObjC    Metal
       1         32      32.0   _NSCGSWindowLocalOrderingState                    ObjC    AppKit
       1         32      32.0   _NSCGSWindowOrdering                              ObjC    AppKit
       1         32      32.0   _NSDisplayOperationStack                          ObjC    AppKit
       1         32      32.0   _NSLocalNotificationCenter                        ObjC    Foundation
       1         32      32.0   _NSMenuItemHotKeyManager                          ObjC    AppKit
       1         32      32.0   _NSScrollerStyleRecommender                       ObjC    AppKit
       1         32      32.0   _NSScrollingPredominantAxisFilter                 ObjC    AppKit
       1         32      32.0   _NSViewLayoutInvalidator                          ObjC    AppKit
       1         32      32.0   _NSViewNotification                               ObjC    AppKit
       1         32      32.0   _ProcessInfo                                      Swift   Foundation
       1         32      32.0   _TUIGeneratorResultAccumulatorCache               ObjC    TextInputUI
       1         32      32.0   _WKWebViewTextInputNotifications                  ObjC    WebKit
       1         32      32.0   icu::SharedObject                                 C++     libicucore.A.dylib
       1         32      32.0   icu::UCharCharacterIterator                       C++     libicucore.A.dylib
       1         32      32.0   icu::UVector32                                    C++     libicucore.A.dylib
       1         32      32.0   ipcURLSchemeHandler_101954b90                     ObjC    libobjc.A.dylib
       1         32      32.0   std::__shared_ptr_pointer<APComponent_Carbon*, std::shared_ptr<APComponent_Carbon>::__shared_ptr_default_delete<APComponent_Carbon, APComponent_Carbon>>  C++     AudioToolboxCore
       1         32      32.0   std::__shared_ptr_pointer<MTLCompilerCache*, std::shared_ptr<MTLCompilerCache>::__shared_ptr_default_delete<MTLCompilerCache, MTLCompilerCache>>  C++     Metal
       1         32      32.0   tauriURLSchemeHandler_101954b90                   ObjC    libobjc.A.dylib
       1         16      16.0   AFInstanceContextHost                             ObjC    AssistantServices
       1         16      16.0   AGXG16CDevice._pipelineLibraryBuilder (struct MTLPipelineLibraryBuilder)  C       AGXMetalG16X
       1         16      16.0   AKAuthorizationClientImpl                         ObjC    AuthKit
       1         16      16.0   API::AutomationClient                             C++     WebKit
       1         16      16.0   API::FindMatchesClient                            C++     WebKit
       1         16      16.0   API::FormClient                                   C++     WebKit
       1         16      16.0   API::HistoryClient                                C++     WebKit
       1         16      16.0   API::IconLoadingClient                            C++     WebKit
       1         16      16.0   API::InjectedBundleClient                         C++     WebKit
       1         16      16.0   API::InspectorClient                              C++     WebKit
       1         16      16.0   API::LegacyContextHistoryClient                   C++     WebKit
       1         16      16.0   API::NotificationProvider                         C++     WebKit
       1         16      16.0   AppNotificationEventClient                        Swift   AppIntents
       1         16      16.0   AudioComponentRegistrarClient                     ObjC    AudioToolboxCore
       1         16      16.0   BKSHIDServiceConnectionFactory                    ObjC    BackBoardServices
       1         16      16.0   CALocalDisplay                                    ObjC    QuartzCore
       1         16      16.0   CFTaskExecutor                                    Swift   libswift_Concurrency.dylib
       1         16      16.0   CSDeviceListener                                  ObjC    CoreSpotlight
       1         16      16.0   CSRequestQueue._workItems (struct)                C       CoreSpotlight
       1         16      16.0   CoreNLP::KoreanLineBreakConnector                 C++     CoreNLP
       1         16      16.0   CoreNLP::MeCabImpl                                C++     CoreNLP
       1         16      16.0   FBSPseudoSceneUpdater                             ObjC    FrontBoardServices
       1         16      16.0   FPSupport_VideoRangeSingleton                     ObjC    MediaToolbox
       1         16      16.0   ISPlatformInfo                                    ObjC    IconServices
       1         16      16.0   LNAppContext                                      Swift   AppIntents
       1         16      16.0   LSApplicationWorkspace                            ObjC    LaunchServices
       1         16      16.0   LSDatabaseContext                                 ObjC    LaunchServices
       1         16      16.0   LockdownModeManager                               ObjC    LockdownMode
       1         16      16.0   LockdownModeManagerInternal                       Swift   LockdownMode
       1         16      16.0   NSAnimationManager                                ObjC    AppKit
       1         16      16.0   NSAppleEventManager                               ObjC    Foundation
       1         16      16.0   NSCFTimer                                         ObjC    Foundation
       1         16      16.0   NSCGSScreenLocalizationProvider                   Swift   AppKit
       1         16      16.0   NSIATrackedActionsManager                         ObjC    AppKit
       1         16      16.0   NSIndexPath._indexes (uint64_t[])                 C       Foundation
       1         16      16.0   NSKVONotifying_NSWorkspace                        ObjC    AppKit
       1         16      16.0   NSMutableOrderedSet.cow (struct __cow_state_t)    C       CoreFoundation
       1         16      16.0   NSPersistentUIEncodingQueue                       ObjC    AppKit
       1         16      16.0   NSPressureConfiguration                           ObjC    AppKit
       1         16      16.0   NSRemoteViewHostAppListenerDelegate               ObjC    ViewBridge
       1         16      16.0   NSScreenMenuBarProvider                           Swift   AppKit
       1         16      16.0   NSScreenSystemUIProvider                          Swift   AppKit
       1         16      16.0   NSThemeFrameBackgroundDelegate                    ObjC    AppKit
       1         16      16.0   NSWMDeferredTransactionQueue                      Swift   AppKit
       1         16      16.0   PKApplicationWorkspaceProxy                       ObjC    PlugInKit
       1         16      16.0   PKFilesystemProvider                              ObjC    PlugInKit
       1         16      16.0   PKLaunchProvider                                  ObjC    PlugInKit
       1         16      16.0   PKLaunchServicesProvider                          ObjC    PlugInKit
       1         16      16.0   PKRunningBoardProvider                            ObjC    PlugInKit
       1         16      16.0   PKSandboxProvider                                 ObjC    PlugInKit
       1         16      16.0   PKSystemProvider                                  ObjC    PlugInKit
       1         16      16.0   RBSAcquisitionCompletionAttribute                 ObjC    RunningBoardServices
       1         16      16.0   RetainableTypedDict<__CFDictionary const*, PrivateHTTPCookieStorage*>  C++     CFNetwork
       1         16      16.0   SHKPlugInObservingCache                           ObjC    ShareKit
       1         16      16.0   SOClient                                          ObjC    AppSSOCore
       1         16      16.0   SwiftUI.(LayoutEngineBox in $22ef92010)<SwiftUI.LayoutComputer.DefaultEngine>  Swift   SwiftUI
       1         16      16.0   TUIMathCompletionGenerator                        ObjC    TextInputUI
       1         16      16.0   UTType                                            ObjC    UniformTypeIdentifiers
       1         16      16.0   Update.TraceHost                                  Swift   SwiftUICore
       1         16      16.0   WFSystemSurfaceWorkflowStatusRegistry             ObjC    VoiceShortcutClient
       1         16      16.0   WKAccessibilitySettingsObserver                   ObjC    WebKit
       1         16      16.0   WKEditorUndoTarget                                ObjC    WebKit
       1         16      16.0   WKFullKeyboardAccessWatcher                       ObjC    WebKit
       1         16      16.0   WKMouseTrackingObserver                           ObjC    WebKit
       1         16      16.0   WKPanGestureController._page (struct)             C       WebKit
       1         16      16.0   WKPanGestureController._viewImpl (struct)         C       WebKit
       1         16      16.0   WKProcessPoolWeakObserver                         ObjC    WebKit
       1         16      16.0   WKProcessPoolWeakObserver.m_weakPtr (struct)      C       WebKit
       1         16      16.0   WKSOAuthorizationDelegate                         ObjC    WebKit
       1         16      16.0   WKWebInspectorPreferenceObserver                  ObjC    WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<IPC::StreamConnectionWorkQueue::startProcessingThread()::$_0, void>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::NetworkProcessProxy::NetworkProcessProxy()::$_0, void>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::RemoteLayerTreeEventDispatcher::RemoteLayerTreeEventDispatcher(WebKit::RemoteScrollingCoordinatorProxyMac&, WTF::ObjectIdentifierGeneric<WebCore::PageIdentifierType, WTF::ObjectIdentifierMainThreadAccessTraits<unsigned long long>, unsigned long long>)::$_0, void, PAL::HysteresisState>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebPageProxy::Internals::Internals(WebKit::WebPageProxy&, std::optional<WebCore::SecurityOriginData>)::$_0, void, PAL::HysteresisState>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebPageProxy::Internals::Internals(WebKit::WebPageProxy&, std::optional<WebCore::SecurityOriginData>)::$_1, void, PAL::HysteresisState>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebPageProxy::WebPageProxy(WebKit::PageClient&, WebKit::WebProcessProxy&, WTF::Ref<API::PageConfiguration, WTF::RawPtrTraits<API::PageConfiguration>, WTF::DefaultRefDerefTraits<API::PageConfiguration>>&&)::$_1, void>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebPageProxy::WebPageProxy(WebKit::PageClient&, WebKit::WebProcessProxy&, WTF::Ref<API::PageConfiguration, WTF::RawPtrTraits<API::PageConfiguration>, WTF::DefaultRefDerefTraits<API::PageConfiguration>>&&)::$_2, void>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebProcessPool::WebProcessPool(API::ProcessPoolConfiguration&)::$_0, void, WTF::RefCounterEvent>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebProcessPool::WebProcessPool(API::ProcessPoolConfiguration&)::$_1, void, WTF::RefCounterEvent>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebProcessPool::WebProcessPool(API::ProcessPoolConfiguration&)::$_2, void, WTF::RefCounterEvent>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebProcessPool::WebProcessPool(API::ProcessPoolConfiguration&)::$_3, void>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebProcessPool::WebProcessPool(API::ProcessPoolConfiguration&)::$_4, void, WTF::RefCounterEvent>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebProcessPool::WebProcessPool(API::ProcessPoolConfiguration&)::$_5, void, WTF::RefCounterEvent>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebProcessPool::WebProcessPool(API::ProcessPoolConfiguration&)::$_6, void, WTF::RefCounterEvent>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebProcessPool::WebProcessPool(API::ProcessPoolConfiguration&)::$_7, void, WTF::RefCounterEvent>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebProcessPool::WebProcessPool(API::ProcessPoolConfiguration&)::$_8, void>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebProcessPool::WebProcessPool(API::ProcessPoolConfiguration&)::$_9, void>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebProcessPool::observeScriptTrackingPrivacyUpdatesIfNeeded()::$_0, void>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebProcessPool::platformInitialize(WebKit::WebProcessPool::NeedsGlobalStaticInitialization)::$_0, void>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebProcessPool::platformInitialize(WebKit::WebProcessPool::NeedsGlobalStaticInitialization)::$_1, void>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebProcessPool::registerNotificationObservers()::$_1, void>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebProcessPool::registerNotificationObservers()::$_2, void, bool>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebProcessProxy::WebProcessProxy(WebKit::WebProcessPool&, WebKit::WebsiteDataStore*, WebKit::WebProcessProxy::IsPrewarmed, WebCore::CrossOriginMode, WebKit::WebProcessProxy::LockdownMode, WebKit::EnhancedSecurity)::$_0, void, WTF::RefCounterEvent>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebProcessProxy::WebProcessProxy(WebKit::WebProcessPool&, WebKit::WebsiteDataStore*, WebKit::WebProcessProxy::IsPrewarmed, WebCore::CrossOriginMode, WebKit::WebProcessProxy::LockdownMode, WebKit::EnhancedSecurity)::$_1, void, WTF::RefCounterEvent>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebProcessProxy::registerNotifyObservers()::$_0, void>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::WebViewImpl::WebViewImpl(WKWebView*, WebKit::WebProcessPool&, WTF::Ref<API::PageConfiguration, WTF::RawPtrTraits<API::PageConfiguration>, WTF::DefaultRefDerefTraits<API::PageConfiguration>>&&)::$_0, void, PAL::HysteresisState>  C++     WebKit
       1         16      16.0   WTF::Detail::CallableWrapper<WebKit::installMemoryPressureHandler()::$_0, void, WTF::Critical, WTF::Synchronous>  C++     WebKit
       1         16      16.0   WebActionDisablingCALayerDelegate                 ObjC    WebCore
       1         16      16.0   WebKit::AdTaggingListHandler                      C++     WebKit
       1         16      16.0   WebKit::LegacyCustomProtocolManagerClient         C++     WebKit
       1         16      16.0   WebKit::NavigationState::NavigationClient         C++     WebKit
       1         16      16.0   WebKit::UIDelegate::ContextMenuClient             C++     WebKit
       1         16      16.0   WebKit::WebsiteDataStoreClient                    C++     WebKit
       1         16      16.0   XTypeXPCClient                                    ObjC    libFontRegistry.dylib
       1         16      16.0   _BKSHIDEventDeferringRuleIdentity                 ObjC    BackBoardServices
       1         16      16.0   _EXDefaults                                       ObjC    ExtensionFoundation
       1         16      16.0   _LSEmptyPropertyList                              ObjC    LaunchServices
       1         16      16.0   _NSAppearanceCustomizationProxy                   ObjC    AppKit
       1         16      16.0   _NSCGSDisplayConfigurationChangeObserver          ObjC    AppKit
       1         16      16.0   _NSCGSWindowMovementGroup                         ObjC    AppKit
       1         16      16.0   _NSFileManagerBridge                              Swift   Foundation
       1         16      16.0   _NSMenuShortcutUpdater                            ObjC    AppKit
       1         16      16.0   _NSSelectorSet                                    ObjC    AppKit
       1         16      16.0   _NSSwiftCalendar                                  Swift   Foundation
       1         16      16.0   _NSWindowSpringLoadingController                  ObjC    AppKit
       1         16      16.0   _NSZeroData                                       ObjC    Foundation
       1         16      16.0   _SLSRemoteViewEventClientDefaultConfig            ObjC    SkyLight
       1         16      16.0   ipcURLSchemeHandler_101954b90.webview_id (char[])  C       libobjc.A.dylib
       1         16      16.0   tauriURLSchemeHandler_101954b90.webview_id (char[])  C       libobjc.A.dylib


```

### ps -p 70940,45506,64274 -o pid,ppid,lstart,rss,command

Exit code: 0

```text
  PID  PPID STARTED                         RSS COMMAND
45506     1 Sat Sep  5 08:15:17 2026      12224 /Applications/Ferryx.app/Contents/MacOS/ferryx --daemon
64274     1 Sat Sep  5 09:16:11 2026      29616 /Applications/Ferryx.app/Contents/MacOS/ferryx --daemon
70940     1 Sat Sep  5 12:18:20 2026     157952 /Applications/Ferryx.app/Contents/MacOS/ferryx

```

