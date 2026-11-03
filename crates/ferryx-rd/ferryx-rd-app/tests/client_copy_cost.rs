//! Headless operation/allocation measurements, not wall-clock performance gates.
#![cfg(test)]
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    hint::black_box,
    sync::Arc,
    time::{Duration, Instant},
};

use base64::Engine;
use ferryx_rd_app::{encode_nv12_screenshot, nv12_to_rgb, LatencyRecorder, ScreenshotFormat};
use ferryx_rd_decode::HevcDecoder;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Default)]
struct Counts {
    allocs: usize,
    reallocs: usize,
    bytes: usize,
}
thread_local! { static COUNTS: Cell<Option<Counts>> = const { Cell::new(None) }; }
struct CountingAllocator;
fn allocation(bytes: usize, realloc: bool) {
    // Const TLS initialization does not allocate. A destroyed TLS slot is not
    // part of a measurement; allocator teardown must not panic.
    let _ = COUNTS.try_with(|slot| {
        if let Some(mut count) = slot.get() {
            count.allocs = count.allocs.saturating_add(usize::from(!realloc));
            count.reallocs = count.reallocs.saturating_add(usize::from(realloc));
            count.bytes = count.bytes.saturating_add(bytes);
            slot.set(Some(count));
        }
    });
}
// SAFETY: System receives the unchanged GlobalAlloc layouts/pointers. Counters
// are thread-local, never dereference allocations, and cannot allocate or unwind.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        allocation(layout.size(), false);
        // SAFETY: GlobalAlloc caller supplies a valid nonzero layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        allocation(layout.size(), false);
        // SAFETY: GlobalAlloc caller supplies a valid nonzero layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: Every allocation above came from System with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        allocation(size, true);
        // SAFETY: Caller guarantees a live System allocation and valid new size.
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn measured<T>(job: impl FnOnce() -> T) -> (T, Counts) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            COUNTS.set(None);
        }
    }
    assert!(COUNTS.get().is_none());
    let reset = Reset;
    COUNTS.set(Some(Counts::default()));
    let started = Instant::now();
    let output = black_box(job());
    let elapsed = started.elapsed();
    let counts = COUNTS.get().unwrap();
    drop(reset);
    println!("COUNTS {counts:?} elapsed_us={}", elapsed.as_micros());
    (output, counts)
}

fn access_units() -> Vec<Vec<u8>> {
    include_str!("fixtures/hevc-continuity.hex")
        .lines()
        .map(|line| {
            line.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        })
        .collect()
}

#[test]
fn exact_owned_pixels_when_native_decoder_consumes_fixture() {
    // Given native FFmpeg and a reference hash from ffmpeg CLI raw NV12 output.
    let units = access_units();
    let (_, mut decoder) = HevcDecoder::from_keyframe_auto(&units[0]).unwrap();
    println!("DECODE acceleration={:?} aus=16", decoder.acceleration());
    // When the real production decoder receives all access units, retaining output.
    let (frames, counts) = measured(|| {
        let mut frames = Vec::with_capacity(16);
        for (index, unit) in units.iter().enumerate() {
            frames.extend(
                decoder
                    .decode(unit, i64::try_from(index).unwrap() * 33)
                    .unwrap(),
            );
        }
        frames.extend(decoder.flush().unwrap());
        frames
    });
    drop(decoder);
    // Then retained planes survive decoder drop with exact pixels and timestamps.
    assert_eq!(frames.len(), 16);
    let mut hash = Sha256::new();
    for (index, frame) in frames.iter().enumerate() {
        assert_eq!(
            (frame.width, frame.height, frame.y_stride, frame.uv_stride),
            (32, 32, 32, 32)
        );
        assert_eq!((frame.y_plane.len(), frame.uv_plane.len()), (1024, 512));
        assert_eq!(frame.timestamp_ms, i64::try_from(index).unwrap() * 33);
        hash.update(&frame.y_plane);
        hash.update(&frame.uv_plane);
    }
    assert_eq!(
        format!("{:x}", hash.finalize()),
        "44612d07ac725f769cf6fbcecdacf3c08e3eb2e33d849331ec988b2ec0a7e32b"
    );
    println!("DECODE frames=16 owned_plane_bytes=24576 rust_counts={counts:?}");
    // Direct FFmpeg packet writes remove one Rust allocation per access unit:
    // 16 allocations and the fixture's 1266 compressed bytes from the baseline.
    assert_eq!(
        (counts.allocs, counts.reallocs, counts.bytes),
        (65, 2, 32896)
    );
}

#[test]
fn identical_screenshots_when_snapshot_ownership_is_shared() {
    // Given neutral and saturated chroma pixel oracles plus an owned 1080p frame.
    assert_eq!(
        nv12_to_rgb(2, 2, &[0, 64, 128, 255, 128, 128]).unwrap(),
        [0, 0, 0, 64, 64, 64, 128, 128, 128, 255, 255, 255]
    );
    assert_eq!(
        nv12_to_rgb(2, 2, &[128, 128, 128, 128, 0, 255]).unwrap(),
        [255, 92, 0].repeat(4)
    );
    let mut source = vec![128; 1920 * 1080 * 3 / 2];
    for (i, y) in source[..1920 * 1080].iter_mut().enumerate() {
        *y = u8::try_from(i % 256).unwrap();
    }
    let shared = Arc::new(source);
    println!("SNAPSHOT Vec clones=128 nv12_bytes_each={}", shared.len());
    let (_, copied) = measured(|| {
        for _ in 0..128 {
            black_box(shared.as_ref().clone());
        }
    });
    println!(
        "SNAPSHOT Arc clones=128 retained_nv12_bytes={}",
        shared.len()
    );
    let (_, retained) = measured(|| {
        for _ in 0..128 {
            black_box(Arc::clone(&shared));
        }
    });
    assert_eq!((copied.allocs, copied.bytes), (128, 398131200));
    assert_eq!(
        (retained.allocs, retained.reallocs, retained.bytes),
        (0, 0, 0)
    );
    // When both ownership strategies feed the identical public encoder.
    for format in [ScreenshotFormat::Png, ScreenshotFormat::Jpeg] {
        println!("SCREENSHOT {format:?} Vec snapshot+encode 1920x1080");
        let (a, copied) = measured(|| {
            encode_nv12_screenshot(1920, 1080, &shared.as_ref().clone(), format).unwrap()
        });
        println!("SCREENSHOT {format:?} Arc snapshot+encode 1920x1080");
        let (b, retained) =
            measured(|| encode_nv12_screenshot(1920, 1080, &Arc::clone(&shared), format).unwrap());
        // Then encoded bytes match exactly, with one full-frame copy removed.
        assert_eq!(a, b);
        assert_eq!(copied.allocs, retained.allocs + 1);
        assert_eq!(copied.bytes, retained.bytes + shared.len());
        let encoded = base64::prelude::BASE64_STANDARD.decode(&a).unwrap();
        let decoded = image::load_from_memory(&encoded).unwrap().to_rgb8();
        assert_eq!(decoded.dimensions(), (1920, 1080));
        if format == ScreenshotFormat::Png {
            assert_eq!(decoded.as_raw(), &nv12_to_rgb(1920, 1080, &shared).unwrap());
        }
        println!(
            "SCREENSHOT {format:?} encoded_bytes={} base64_bytes={} sha256={:x}",
            encoded.len(),
            a.len(),
            Sha256::digest(&encoded)
        );
    }
    let retained = Arc::clone(&shared);
    drop(shared);
    assert_eq!(retained.len(), 3110400);
}

#[test]
fn exact_statistics_when_using_receive_to_decode_end_boundary() {
    // Given independent local instants, without sleeps or clock-rate assumptions.
    let receive = Instant::now();
    let decode_start = receive + Duration::from_micros(700);
    let decode_end = decode_start + Duration::from_micros(300);
    let mut recorder = LatencyRecorder::with_capacity(4096);
    // When real recorder APIs receive CLI-style local receive and decode-end instants.
    recorder.record_sample(Some(receive), decode_start, decode_end);
    assert_eq!(recorder.stats().p50_us, 1000);
    let mut fallback = LatencyRecorder::with_capacity(1);
    fallback.record_sample(None, decode_start, decode_end);
    assert_eq!(fallback.stats().p50_us, 300);
    println!("STATS record_us samples=8192 retained_capacity=4096");
    let (_, record) = measured(|| {
        for i in 1..=8192 {
            recorder.record_us(i);
        }
    });
    println!("STATS query retained_samples=4096");
    let (stats, query) = measured(|| recorder.stats());
    // Then recording does not allocate; query clones once; ranks cover latest 4096.
    assert_eq!((record.allocs, record.reallocs, record.bytes), (0, 0, 0));
    assert_eq!((query.allocs, query.reallocs, query.bytes), (1, 0, 32768));
    assert_eq!(
        (
            stats.frames,
            stats.p50_us,
            stats.p95_us,
            stats.p99_us,
            stats.max_us
        ),
        (8193, 6145, 7987, 8151, 8192)
    );
    println!("STATS {stats:?} receive_boundary_us=1000 decode_only_us=300");
}
