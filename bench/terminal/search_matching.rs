//! Standalone matcher regression tests and CPU/allocation benchmark (no Ghostty/GPU required).
//! From the repository root:
//!   rustc --edition=2021 --test bench/terminal/search_matching.rs -o /tmp/ferryx-search-tests
//!   /tmp/ferryx-search-tests
//!   rustc --edition=2021 -O bench/terminal/search_matching.rs -o /tmp/ferryx-search-bench
//!   /tmp/ferryx-search-bench
//! Measures only matching already-extracted cells, not FFI extraction or UI latency.
//! Allocation counting is a separate pass; timing runs do not increment the counter.

#[path = "../../src-tauri/src/native_terminal/search_matching.rs"]
mod matching;

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

struct CountingAllocator;
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: Every allocation/deallocation is forwarded unchanged to the system allocator.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

type Matcher = fn(&[String], &str, bool, u16, &mut Vec<(u16, u16, u16)>);

fn legacy(
    cells: &[String],
    query: &str,
    sensitive: bool,
    row: u16,
    matches: &mut Vec<(u16, u16, u16)>,
) {
    for start in 0..cells.len() {
        let mut candidate = String::new();
        for (end, cell) in cells.iter().enumerate().skip(start) {
            candidate.push_str(cell);
            let normalized: String = if sensitive {
                candidate.as_str().into()
            } else {
                candidate.to_lowercase()
            };
            if normalized == query {
                matches.push((row, start as u16, end as u16));
                break;
            }
            if normalized.len() >= query.len() {
                break;
            }
        }
    }
}

fn run(
    matcher: Matcher,
    cells: &[String],
    query: &str,
    sensitive: bool,
    rows: u16,
    count: bool,
) -> (f64, usize, usize) {
    // Reserve results outside measurement to isolate temporary matching allocations.
    let mut matches = Vec::with_capacity(usize::from(rows) * cells.len());
    ALLOCATIONS.store(0, Ordering::Relaxed);
    COUNTING.store(count, Ordering::Relaxed);
    let start = Instant::now();
    for row in 0..rows {
        matcher(
            black_box(cells),
            black_box(query),
            sensitive,
            row,
            &mut matches,
        );
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    COUNTING.store(false, Ordering::Relaxed);
    (
        ms,
        ALLOCATIONS.load(Ordering::Relaxed),
        black_box(matches.len()),
    )
}

fn main() {
    let rows = 2000;
    for (name, cells, query, sensitive) in [
        (
            "ASCII insensitive, missing long query",
            vec!["x".to_owned(); 240],
            "a".repeat(96),
            false,
        ),
        (
            "ASCII insensitive, overlapping matches",
            vec!["A".to_owned(); 240],
            "a".repeat(96),
            false,
        ),
        (
            "Unicode sensitive, overlapping matches",
            vec!["界".to_owned(); 240],
            "界".repeat(32),
            true,
        ),
    ] {
        let mut old = Vec::new();
        let mut new = Vec::new();
        legacy(&cells, &query, sensitive, 0, &mut old);
        matching::append_row_matches(&cells, &query, sensitive, 0, &mut new);
        assert_eq!(old, new);
        run(legacy, &cells, &query, sensitive, 10, false);
        run(
            matching::append_row_matches,
            &cells,
            &query,
            sensitive,
            10,
            false,
        );
        let old_count = run(legacy, &cells, &query, sensitive, rows, true).1;
        let new_count = run(
            matching::append_row_matches,
            &cells,
            &query,
            sensitive,
            rows,
            true,
        )
        .1;
        let mut before = Vec::new();
        let mut after = Vec::new();
        for _ in 0..5 {
            before.push(run(legacy, &cells, &query, sensitive, rows, false));
            after.push(run(
                matching::append_row_matches,
                &cells,
                &query,
                sensitive,
                rows,
                false,
            ));
        }
        before.sort_by(|a, b| a.0.total_cmp(&b.0));
        after.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(before[2].2, after[2].2);
        println!(
            "{name}: {rows} x 240 cells, query {} bytes, 5-run median",
            query.len()
        );
        println!(
            "  before: {:.3} ms, {} allocations/reallocations, {} matches",
            before[2].0, old_count, before[2].2
        );
        println!(
            "  after:  {:.3} ms, {} allocations/reallocations, {} matches",
            after[2].0, new_count, after[2].2
        );
    }
}
