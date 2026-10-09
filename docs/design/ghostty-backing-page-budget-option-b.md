# Ghostty Option B: Vendor ABI Design for Bounding Aggregate Decoded Page & Metadata Memory

## 1. Problem Statement & Empirical Root Cause

In `libghostty-vt`, terminal memory is partitioned into two distinct tiers:

1. **Tier 1: Userland Heap (`gen_alloc`)**:
   Passed as `GhosttyAllocator` to `ghostty_snapshot_decoder_new_buf` / `ghostty_terminal_new`.
   Used exclusively for:
   - Linked-list node wrappers (`NodePool` in `PageList.zig:347`, ~48 bytes/node).
   - Viewport pins (`PinPool` in `PageList.zig:349`).
   - Tracked pin sets (`PinSet` in `PageList.zig:639`).
   - Out-of-band style tables (`StyleSet`), hyperlinks, and multi-codepoint grapheme clusters (`page.zig:200–250`).
   - Terminal & Screen metadata structs (`Terminal.zig`, `Screen.zig`).

2. **Tier 2: OS Virtual Memory Pages (`page_alloc` / `PageAlloc`)**:
   Hardcoded in vendor source to bypass the custom userland allocator:
   - `PageList.zig:543`:
     ```zig
     inline fn pageAllocator() Allocator {
         if (builtin.is_test) return std.testing.allocator;
         if (!builtin.target.os.tag.isDarwin()) return std.heap.page_allocator;
         const mach = @import("../os/mach.zig");
         return mach.taggedPageAllocator(.application_specific_1);
     }
     ```
   - `PageList.zig:351`: `var page_pool = try PagePool.initCapacity(page_alloc, preheat);` preheats `page_preheat` pooled pages (each `std_size = 4096`).
   - `PageList.zig:617`: `PageList.init` passes `pageAllocator()` into `MemoryPool.init`.
   - `PageList.zig:1120`: `PageList.clone` passes `pageAllocator()` into `MemoryPool.init`.
   - `page.zig:245`: `Page.init` calls `PageAlloc.alloc(l.total_size)` directly for non-pooled / initial page buffers (`VirtualAlloc` on Windows, `mmap` on POSIX).
   - `page.zig:318`: `Page.deinit` calls `PageAlloc.free(self.memory)`.
   - `page.zig:651`: `Page.clone` calls `PageAlloc.alloc(self.memory.len)` directly for page cloning.

### Failure of Lowered-Metadata Budgets (Empirical Observation)
*Note: Numerical estimates below are fixture-specific observations, not universal constants.*
In tests using the 5,600-row plain ASCII history fixture (`Row {i:05}\r\n`):
- Backing page virtual memory observed: ~11.4 MiB across ~2,800 pages.
- Custom allocator metadata observed: ~134 KiB across ~2,800 nodes.
Lowering the metadata budget to `ready_allocated + 1024` only tested exhaustion of the 48-byte linked-list node metadata. It did not bound or meter the ~11.4 MiB of OS virtual memory pages. To guarantee Plan 36's aggregate 32 MiB uncompressed cap, backing page virtual memory must be bounded alongside heap metadata.

---

## 2. Distinction: Node Metadata OOM vs. Backing-Page OOM

| Characteristic | Node Metadata OOM | Backing-Page OOM | Aggregate 32 MiB OOM |
|---|---|---|---|
| **Memory Domain** | Userland heap (`malloc` / `std.mem.Allocator`) | OS Virtual Memory (`VirtualAlloc` / `mmap`) | Combined sum of Heap + OS Virtual Memory |
| **Trigger Point** | `GhosttyAllocatorVtable.alloc` returns `NULL` | Page budget exhausted before `PageAlloc.alloc` | $(\text{Heap Bytes} + \text{Backing Page Bytes}) > 32\text{ MiB}$ |
| **Object Type** | `Node`, `Pin`, `StyleSet`, `GraphemeAlloc` | `Page.memory` (`std_size = 4096` or heap page) | Total resident memory footprint of the terminal |
| **Failure Mechanism**| Zig returns `error.OutOfMemory` | Pre-allocation check rejects page before OS call | Hard stop returning `NativeTerminalError::OutOfMemory` |
| **Rollback Scope** | Decrements atomic heap counter | Decrements atomic page counter | Drops terminal, releasing virtual memory and heap |

---

## 3. Option B Vendor ABI Specification

### A. Public C ABI Addition (`include/ghostty/vt/snapshot.h`)
To prevent breaking existing C consumers, `GhosttyAllocatorVtable` is **not modified**.
Instead, an opt-in memory budget callback struct is introduced:

```c
#ifndef GHOSTTY_VT_BUDGET_H
#define GHOSTTY_VT_BUDGET_H

#include <stdbool.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Externally-owned budget reservation callbacks.
 *
 * `ctx` must remain valid for the entire lifetime of the decoder and any
 * returned terminal until the terminal is freed.
 */
typedef struct {
    void *ctx;
    bool (*reserve)(void *ctx, size_t bytes);
    void (*release)(void *ctx, size_t bytes);
} GhosttyMemoryBudget;

#ifdef __cplusplus
}
#endif

#endif /* GHOSTTY_VT_BUDGET_H */
```

### B. Decoder Option Extension (`include/ghostty/vt/snapshot.h`)
In `GhosttySnapshotDecoderOption`:
```c
typedef enum GHOSTTY_ENUM_TYPED {
  GHOSTTY_SNAPSHOT_DECODER_OPT_MAX_CONTINUATION_BYTES = 0,
  GHOSTTY_SNAPSHOT_DECODER_OPT_RETAIN_CONTINUATION = 1,

  /**
   * Memory budget callbacks for bounding backing pages and shared memory.
   * Must be set before ghostty_snapshot_decoder_ready() or decoder_next().
   *
   * Input type: const GhosttyMemoryBudget *
   */
  GHOSTTY_SNAPSHOT_DECODER_OPT_MEMORY_BUDGET = 2,

  GHOSTTY_SNAPSHOT_DECODER_OPT_MAX_VALUE = GHOSTTY_ENUM_MAX_VALUE,
} GhosttySnapshotDecoderOption;
```

---

## 4. Vendor Engine Integration & Pointer Stability

### A. Stable Pointer & Zero-Overhead OS Zeroing
1. **Stable Pointer**:
   `GhosttyMemoryBudget` is passed by pointer and retained in `DecoderWrapper`.
   Because `GhosttyMemoryBudget.ctx` is owned externally (by Rust's `Arc<SnapshotAllocatorOwner>`), the Zig allocator's `ctx` points directly to `budget.ctx` or `*const GhosttyMemoryBudget`.
   `PageList` moves or clones never invalidate the allocator pointer because the budget context is externally allocated and stable.

2. **Preserving OS Zeroing Semantics**:
   Do **not** use `alignedAlloc` (which does not guarantee zeroed memory).
   The budgeted page allocator delegates directly to `PageAlloc.alloc` (`VirtualAlloc` on Windows, `mmap` on POSIX), preserving kernel-level zero-page semantics:

```zig
pub const BudgetedPageAllocator = struct {
    budget: *const GhosttyMemoryBudget,

    pub fn allocator(self: *const BudgetedPageAllocator) Allocator {
        return .{
            .ptr = @constCast(@ptrCast(self)),
            .vtable = &.{
                .alloc = alloc,
                .resize = resize,
                .remap = remap,
                .free = free,
            },
        };
    }

    fn alloc(ctx: *anyopaque, len: usize, ptr_align: u8, ret_addr: usize) ?[*]u8 {
        _ = ptr_align;
        _ = ret_addr;
        const self: *const BudgetedPageAllocator = @ptrCast(@alignCast(ctx));

        // Exact page-aligned rounding to match OS virtual memory allocation:
        const rounded_bytes = std.mem.alignForward(usize, len, std.heap.page_size_min);

        // Pre-allocation check: reject before requesting OS virtual memory
        if (!self.budget.reserve(self.budget.ctx, rounded_bytes)) return null;

        // Allocate zeroed virtual memory from OS:
        const mem = PageAlloc.alloc(rounded_bytes) catch {
            self.budget.release(self.budget.ctx, rounded_bytes);
            return null;
        };

        return mem.ptr;
    }

    fn free(ctx: *anyopaque, buf: []u8, buf_align: u8, ret_addr: usize) void {
        _ = buf_align;
        _ = ret_addr;
        const self: *const BudgetedPageAllocator = @ptrCast(@alignCast(ctx));
        const rounded_bytes = std.mem.alignForward(usize, buf.len, std.heap.page_size_min);

        PageAlloc.free(@alignCast(buf));
        self.budget.release(self.budget.ctx, rounded_bytes);
    }

    fn resize(ctx: *anyopaque, buf: []u8, buf_align: u8, new_len: usize, ret_addr: usize) bool {
        _ = ctx; _ = buf; _ = buf_align; _ = new_len; _ = ret_addr;
        return false; // Terminal pages are immutable in size
    }

    fn remap(ctx: *anyopaque, memory: []u8, alignment: u8, new_len: usize, ret_addr: usize) ?[*]u8 {
        _ = ctx; _ = memory; _ = alignment; _ = new_len; _ = ret_addr;
        return null;
    }
};
```

### B. Exact Vendor Callsite Integration
1. **Configuration Stage (`c/snapshot.zig:200`)**:
   In `decoder_set`:
   When `option == .memory_budget`:
   Store `budget` in `wrapper.memory_budget`.
   This occurs while `wrapper.state == .configuring`, **before** any terminal or preheat.
2. **`PageList.zig:617` (`PageList.init`)**:
   Pass `budgeted_page_alloc` into `MemoryPool.init`.
   - `PageList.zig:351`: `MemoryPool.init` preheats `page_pool` using `page_alloc`. All preheat memory is reserved from `budget`.
3. **`PageList.zig:1120` (`PageList.clone`)**:
   Passes the inherited `budgeted_page_alloc` into the cloned `MemoryPool.init`.
4. **`page.zig:245` (`Page.init`) and `page.zig:651` (`Page.clone`)**:
   Oversized and cloned pages allocate through `page_alloc` using exact page-rounded bytes.
5. **`page.zig:318` (`Page.deinit`) & `PagePool.deinit`**:
   Frees call `page_alloc.free()`, releasing the exact page-rounded bytes.

---

## 5. Post-Decode Continuity & Lifetime Coupling

1. **Shared Budget Instance**:
   The host maintains a single atomic counter `Arc<AtomicUsize>` for both:
   - Userland heap metadata (via `GhosttyAllocatorVtable`)
   - OS virtual memory pages (via `GhosttyMemoryBudget.reserve` / `release`)
2. **Terminal Survival After Decoder Drop**:
   When `ghostty_snapshot_decoder_free` is called, decoder-transient reader memory is released.
   The returned `GhosttyTerminal` retains the budget reference.
   Subsequent `vt_write` calls that trigger scrollback page allocations or reflow continue to allocate through `budgeted_page_alloc`, strictly bounded by the aggregate 32 MiB cap.
3. **Rollback Verification**:
   When `ghostty_terminal_free` is called:
   `Terminal.deinit()` -> `ScreenSet.deinit()` -> `PageList.deinit()`.
   Every pooled and oversized page is freed via `PageAlloc.free()`, releasing all reserved page bytes.
   All heap metadata is freed via `GhosttyAllocatorVtable.free()`.
   The aggregate budget returns to **strictly 0 bytes**.
