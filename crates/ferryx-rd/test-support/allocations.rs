use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct TrackingAlloc;
thread_local! {
    static ALLOC_COUNT: Cell<Option<usize>> = const { Cell::new(None) };
}

// SAFETY: forwards every allocation and deallocation unchanged to System.
// The allocation-free, thread-local counter never accesses allocated memory.
unsafe impl GlobalAlloc for TrackingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.with(|count| {
            if let Some(value) = count.get() {
                count.set(Some(value + 1));
            }
        });
        // SAFETY: GlobalAlloc's caller provides the valid layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: pointer and layout are forwarded to their original allocator.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: TrackingAlloc = TrackingAlloc;

pub(crate) fn allocations<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            ALLOC_COUNT.with(|count| count.set(None));
        }
    }
    ALLOC_COUNT.with(|count| {
        assert_eq!(count.replace(Some(0)), None);
    });
    let reset = Reset;
    let result = operation();
    let count = ALLOC_COUNT.with(|count| count.get().unwrap());
    drop(reset);
    (result, count)
}
