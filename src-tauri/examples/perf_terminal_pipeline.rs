//! Offline real-production pipeline QA/measurement. No daemon, socket, GUI, or GPU is started.
//! cargo run --offline --locked --manifest-path src-tauri/Cargo.toml --example perf_terminal_pipeline -- search
//! Modes: qa, search, history, reconstruct64, reconstruct8m, all. Run the identical source/profile on both revisions.
//! Timings exclude fixture preparation unless named; RSS is the process lifetime high-water mark,
//! not live heap size or a phase delta. CPU is process user+system time (Unix only).

use base64::Engine;
use ferryx_lib::native_terminal::{NativeTerminal, TerminalEngine};
use ferryx_lib::terminal::output_hub::{retained_history::RetentionBudget, TerminalOutputHub};
use serde_json::json;
use std::{hint::black_box, time::Instant};

const RUNS: usize = 5;

#[derive(Clone, Copy)]
struct Resources { cpu_ms: f64, max_rss_kib: u64 }

#[cfg(unix)]
fn resources() -> Resources {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage writes exactly one rusage to valid writable storage.
    assert_eq!(unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) }, 0);
    // SAFETY: the successful call above initialized the structure.
    let usage = unsafe { usage.assume_init() };
    let seconds = usage.ru_utime.tv_sec as f64 + usage.ru_stime.tv_sec as f64;
    let micros = usage.ru_utime.tv_usec as f64 + usage.ru_stime.tv_usec as f64;
    let rss = usage.ru_maxrss as u64;
    Resources {
        cpu_ms: seconds * 1000.0 + micros / 1000.0,
        max_rss_kib: if cfg!(target_os = "macos") { rss / 1024 } else { rss },
    }
}

#[cfg(not(unix))]
fn resources() -> Resources { Resources { cpu_ms: 0.0, max_rss_kib: 0 } }

fn measure(name: &str, mut operation: impl FnMut() -> usize) {
    // Keep setup/validation and two warm-up calls out of the reported latency/CPU samples.
    let expected = operation();
    assert_eq!(operation(), expected);
    let mut samples = Vec::new();
    for sample in 0..RUNS {
        let before = resources();
        let start = Instant::now();
        let checksum = black_box(operation());
        let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
        let after = resources();
        assert_eq!(checksum, expected);
        samples.push(wall_ms);
        println!("{}", json!({"phase": name, "sample": sample, "wall_ms": wall_ms,
            "cpu_ms": after.cpu_ms - before.cpu_ms, "process_peak_rss_kib": after.max_rss_kib,
            "checksum": checksum}));
    }
    samples.sort_by(f64::total_cmp);
    println!("{}", json!({"phase": name, "samples": RUNS, "median_ms": samples[RUNS / 2],
        "min_ms": samples[0], "max_ms": samples[RUNS - 1]}));
}

fn terminal(cols: u16, rows: u16) -> NativeTerminal {
    let mut terminal = NativeTerminal::new(cols, rows).expect("Ghostty terminal");
    terminal.resize(cols, rows, 8, 16).expect("cell geometry");
    terminal
}

fn search_qa() {
    let mut term = terminal(40, 6);
    term.feed_str("aAaA\r\n界a e\u{301} ΟΣΑ İK").unwrap();
    for (query, sensitive, expected) in [
        ("aa", false, vec![(0, 0, 1), (0, 1, 2), (0, 2, 3)]),
        ("aA", true, vec![(0, 0, 1), (0, 2, 3)]),
        ("界", true, vec![(1, 0, 0)]),
        ("e", true, vec![]),
        ("e\u{301}", true, vec![(1, 4, 4)]),
        ("ος", false, vec![(1, 6, 7)]),
        ("οσα", false, vec![(1, 6, 8)]),
        ("i\u{307}", false, vec![(1, 10, 10)]),
        ("k", false, vec![(1, 11, 11)]),
        ("absent", true, vec![]),
        ("", true, vec![]),
    ] {
        assert_eq!(term.search_grid(query, sensitive).unwrap(), expected,
            "real Ghostty search query={query:?} sensitive={sensitive}");
    }
    let mut history = terminal(40, 4);
    let rows = (0..80).map(|i| format!("row-{i:03}")).collect::<Vec<_>>().join("\r\n");
    history.feed_str(&rows).unwrap();
    assert_eq!(history.search_grid("row-001", true).unwrap(), vec![(1, 0, 6)]);
    println!("{}", json!({"qa":"search", "passed":true, "cases":12}));
}

fn search_measurements() {
    for (name, line, query, sensitive, rows, per_row) in [
        ("search_ascii_missing", "x".repeat(230), "a".repeat(96), false, 2000, 0),
        ("search_ascii_overlaps", "A".repeat(230), "a".repeat(96), false, 2000, 135),
        ("search_unicode_sensitive", "é".repeat(230), "é".repeat(32), true, 2000, 199),
        ("search_unicode_insensitive", "É".repeat(230), "é".repeat(32), false, 200, 199),
    ] {
        let mut term = terminal(240, 24);
        let input = vec![line; rows].join("\r\n");
        term.feed(input.as_bytes()).unwrap();
        let matches = term.search_grid(&query, sensitive).unwrap();
        assert_eq!(matches.len(), rows * per_row, "fixture {name}");
        println!("{}", json!({"fixture":name, "bytes":input.len(), "rows":rows,
            "columns":240, "query_bytes":query.len(), "expected_matches":matches.len()}));
        measure(name, || term.search_grid(black_box(&query), sensitive).unwrap().len());
    }
}

fn publish(hub: &TerminalOutputHub, session: &str, bytes: &[u8]) {
    // Preserve awkward PTY-sized boundaries; the production parser sees split escape sequences.
    for chunk in bytes.chunks(8191) {
        hub.publish(session, chunk.to_vec()).expect("publish");
    }
}

fn assert_same_screen(left: &NativeTerminal, right: &NativeTerminal) {
    let a = left.render_snapshot().unwrap();
    let b = right.render_snapshot().unwrap();
    assert_eq!((a.cols, a.rows), (b.cols, b.rows));
    for row in 0..a.rows as usize { assert_eq!(a.row_text(row), b.row_text(row), "row {row}"); }
    assert_eq!(a.images.len(), b.images.len());
    for (a, b) in a.images.iter().zip(&b.images) {
        assert_eq!(a.image.rgba, b.image.rgba);
        assert_eq!((a.viewport_col, a.viewport_row), (b.viewport_col, b.viewport_row));
    }
}

fn history_qa() {
    let hub = TerminalOutputHub::default();
    drop(hub.register_session("qa"));
    hub.record_initial_size("qa", 40, 24);
    let mut live = terminal(40, 24);
    // Valid two-pixel red/green RGB image, split around interleaved text and a resize.
    let first = b"OLDEST\r\n\x1b_Ga=T,f=24,s=2,v=1,i=902,c=2,r=1,q=2,m=1;/wAA\x1b\\";
    let middle = b"INTERLEAVED-TEXT-WRAPS-PAST-TWENTY\r\n\x1b[5;4H";
    let tail = b"\x1b_Gm=0;AP8A\x1b\\AFTER";
    live.feed(first).unwrap(); publish(&hub, "qa", first);
    live.resize(20, 24, 8, 16).unwrap(); hub.record_resize("qa", 20, 24).unwrap();
    live.feed(middle).unwrap(); publish(&hub, "qa", middle);
    let snapshot = hub.subscribe_with_sequence("qa", None).unwrap().snapshot;
    let mut restored = terminal(40, 24);
    for segment in snapshot.history_segments {
        restored.resize(segment.cols.unwrap(), segment.rows.unwrap(), 8, 16).unwrap();
        restored.feed(&segment.bytes).unwrap();
    }
    live.feed(tail).unwrap(); restored.feed(tail).unwrap(); publish(&hub, "qa", tail);
    assert_same_screen(&live, &restored);
    assert_eq!(restored.render_snapshot().unwrap().images.len(), 1);
    // This independent handover case fits the recovery ring, unlike the saturation fixture.
    let exported = hub.export_session_state("qa").unwrap();
    let successor = TerminalOutputHub::default();
    successor.import_session_state("qa", exported).unwrap();
    let snapshot = successor.subscribe_with_sequence("qa", None).unwrap().snapshot;
    let mut handover = terminal(40, 24);
    for segment in snapshot.history_segments {
        handover.resize(segment.cols.unwrap(), segment.rows.unwrap(), 8, 16).unwrap();
        handover.feed(&segment.bytes).unwrap();
    }
    assert_same_screen(&live, &handover);
    println!("{}", json!({"qa":"multipart_image_resize_snapshot_handover", "passed":true}));
}

fn history_measurements() {
    // Representative mixed terminal output, below both retention budgets: text plus an
    // actual decoded RGB image. This guards against optimizing only the saturation case.
    let mut mixed = (0..100).map(|i| format!("BEFORE-{i:03}\r\n")).collect::<String>();
    mixed.push_str(&format!("\x1b_Ga=T,f=24,s=64,v=64,i=2,c=8,r=4,C=1,q=2;{}\x1b\\",
        base64::prelude::BASE64_STANDARD.encode([255_u8, 0, 0].repeat(64 * 64))));
    for i in 0..20 { mixed.push_str(&format!("AFTER-{i:03}\r\n")); }
    let expected_rgba = [255_u8, 0, 0, 255].repeat(64 * 64);
    println!("{}", json!({"fixture":"mixed_text_rgb_image", "bytes":mixed.len(),
        "image_pixels":4096, "text_lines":120}));
    measure("mixed_hub_publish_snapshot_ghostty_reconstruct", || {
        let hub = TerminalOutputHub::default();
        drop(hub.register_session("mixed"));
        publish(&hub, "mixed", mixed.as_bytes());
        let snapshot = hub.subscribe_with_sequence("mixed", None).unwrap().snapshot;
        let mut restored = terminal(80, 24);
        restored.feed(&snapshot.history).unwrap();
        assert_eq!(restored.search_grid("BEFORE-000", true).unwrap().len(), 1);
        assert_eq!(restored.search_grid("AFTER-019", true).unwrap().len(), 1);
        let screen = restored.render_snapshot().unwrap();
        assert_eq!(screen.images.len(), 1, "representative decoded image must be visible");
        assert_eq!(screen.images[0].image.rgba.as_slice(), expected_rgba.as_slice());
        snapshot.history.len()
    });
    const COMMAND: &[u8] = b"\x1b_Ga=p,i=1,p=1,q=2;\x1b\\";
    const MARKER: &[u8] = b"RETAINED-TEXT\r\n";
    for image_budget in [64 * 1024, 8 * 1024 * 1024] {
        let groups = image_budget / COMMAND.len();
        let hub = TerminalOutputHub::new_with_retention(512 * 1024,
            RetentionBudget { text_bytes: 512 * 1024, image_bytes: image_budget });
        drop(hub.register_session("measure"));
        publish(&hub, "measure", MARKER);
        // Valid image first, then repeated placements. Only one live placement is updated;
        // retention still has to account for every protocol command as production does.
        publish(&hub, "measure", b"\x1b_Ga=T,f=24,s=1,v=1,i=1,p=1,q=2;/wAA\x1b\\");
        for _ in 0..groups { hub.publish("measure", COMMAND.to_vec()).unwrap(); }
        println!("{}", json!({"fixture":"image_placements", "image_budget":image_budget,
            "command_bytes":COMMAND.len(), "retained_groups":groups, "pushes_per_sample":512}));
        measure(&format!("hub_publish_saturated_{image_budget}"), || {
            for _ in 0..512 { hub.publish("measure", black_box(COMMAND).to_vec()).unwrap(); }
            512 * COMMAND.len()
        });
        let snapshot = hub.subscribe_with_sequence("measure", None).unwrap().snapshot;
        assert!(snapshot.history.starts_with(MARKER));
        assert_eq!(snapshot.history.len(), MARKER.len() + groups * COMMAND.len());
        assert!(snapshot.history[MARKER.len()..].chunks_exact(COMMAND.len()).all(|part| part == COMMAND));
        measure(&format!("hub_snapshot_{image_budget}"), || {
            hub.subscribe_with_sequence("measure", None).unwrap().snapshot.history.len()
        });
        measure(&format!("ghostty_reconstruct_{image_budget}"), || {
            let mut reconstructed = terminal(80, 24);
            reconstructed.feed(black_box(&snapshot.history)).unwrap();
            assert!(reconstructed.render_snapshot().unwrap().row_text(0).contains("RETAINED-TEXT"));
            snapshot.history.len()
        });
    }
}

// Focused follow-up for the small 64 KiB reconstruction movement. This preserves
// the original fixture and timed operation, but amortizes clock/CPU probes across
// 100 reconstructions per sample. Run paired, alternating binaries on an idle host.
fn reconstruction_64k_measurements() {
    const COMMAND: &[u8] = b"\x1b_Ga=p,i=1,p=1,q=2;\x1b\\";
    const MARKER: &[u8] = b"RETAINED-TEXT\r\n";
    const BATCH: usize = 100;
    const SAMPLES: usize = 30;
    const WARMUP_BATCHES: usize = 3;
    let image_budget = 64 * 1024;
    let groups = image_budget / COMMAND.len();
    let hub = TerminalOutputHub::new_with_retention(512 * 1024,
        RetentionBudget { text_bytes: 512 * 1024, image_bytes: image_budget });
    drop(hub.register_session("measure"));
    publish(&hub, "measure", MARKER);
    publish(&hub, "measure", b"\x1b_Ga=T,f=24,s=1,v=1,i=1,p=1,q=2;/wAA\x1b\\");
    for _ in 0..groups { hub.publish("measure", COMMAND.to_vec()).unwrap(); }
    // The original history mode runs two warmups and five 512-publish samples
    // before freezing this snapshot. Keep that preparation identical, untimed.
    for _ in 0..(2 + RUNS) * 512 { hub.publish("measure", COMMAND.to_vec()).unwrap(); }
    let snapshot = hub.subscribe_with_sequence("measure", None).unwrap().snapshot;
    let expected = [MARKER, COMMAND.repeat(groups).as_slice()].concat();
    assert_eq!(snapshot.history.as_slice(), expected.as_slice());
    let mut fixture_fnv1a64 = 0xcbf29ce484222325u64;
    for &byte in &snapshot.history {
        fixture_fnv1a64 ^= u64::from(byte);
        fixture_fnv1a64 = fixture_fnv1a64.wrapping_mul(0x100000001b3);
    }
    println!("{}", json!({"fixture":"original_65536_reconstruction", "bytes":snapshot.history.len(),
        "retained_groups":groups, "command_bytes":COMMAND.len(), "fnv1a64":format!("{fixture_fnv1a64:016x}"),
        "batch":BATCH, "samples":SAMPLES, "warmup_batches":WARMUP_BATCHES,
        "operation":"new+resize+feed+render_snapshot+row_text_assert+drop; no search or hub work timed"}));
    let operation = || {
        let mut reconstructed = terminal(80, 24);
        reconstructed.feed(black_box(&snapshot.history)).unwrap();
        assert!(reconstructed.render_snapshot().unwrap().row_text(0).contains("RETAINED-TEXT"));
        snapshot.history.len()
    };
    for _ in 0..WARMUP_BATCHES * BATCH { black_box(operation()); }
    for sample in 0..SAMPLES {
        let before = resources();
        let start = Instant::now();
        for _ in 0..BATCH { assert_eq!(black_box(operation()), expected.len()); }
        let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
        let after = resources();
        println!("{}", json!({"phase":"ghostty_reconstruct_65536_batched", "sample":sample,
            "iterations":BATCH, "wall_total_ms":wall_ms, "wall_ms":wall_ms / BATCH as f64,
            "cpu_ms":(after.cpu_ms-before.cpu_ms) / BATCH as f64,
            "process_peak_rss_kib":after.max_rss_kib, "checksum":expected.len()}));
    }
}

// Run in a separate process BEFORE isolated timing: compare every byte against
// the original hub pipeline, including its exact publish preconditioning count.
fn verify_8m_fixture() {
    const COMMAND: &[u8] = b"\x1b_Ga=p,i=1,p=1,q=2;\x1b\\";
    const MARKER: &[u8] = b"RETAINED-TEXT\r\n";
    let image_budget = 8 * 1024 * 1024;
    let groups = image_budget / COMMAND.len();
    let hub = TerminalOutputHub::new_with_retention(512 * 1024,
        RetentionBudget { text_bytes: 512 * 1024, image_bytes: image_budget });
    drop(hub.register_session("measure"));
    publish(&hub, "measure", MARKER);
    publish(&hub, "measure", b"\x1b_Ga=T,f=24,s=1,v=1,i=1,p=1,q=2;/wAA\x1b\\");
    for _ in 0..groups { hub.publish("measure", COMMAND.to_vec()).unwrap(); }
    for _ in 0..(2 + RUNS) * 512 { hub.publish("measure", COMMAND.to_vec()).unwrap(); }
    let snapshot = hub.subscribe_with_sequence("measure", None).unwrap().snapshot;
    let expected = [MARKER, COMMAND.repeat(groups).as_slice()].concat();
    assert_eq!(snapshot.history.as_slice(), expected.as_slice(), "full original snapshot bytes differ");
    let mut fnv = 0xcbf29ce484222325u64;
    for &byte in &snapshot.history { fnv ^= u64::from(byte); fnv = fnv.wrapping_mul(0x100000001b3); }
    println!("{}", json!({"qa":"original_8m_snapshot_full_byte_equality", "status":"PASS",
        "bytes":snapshot.history.len(), "groups":groups, "fnv1a64":format!("{fnv:016x}"),
        "preconditioning_publishes":(2 + RUNS) * 512}));
}

// Follow-up for the distinct 8 MiB signal; do not infer this result from 64 KiB.
fn reconstruction_8m_isolated_measurements() {
    const COMMAND: &[u8] = b"\x1b_Ga=p,i=1,p=1,q=2;\x1b\\";
    const MARKER: &[u8] = b"RETAINED-TEXT\r\n";
    const BATCH: usize = 5;
    const SAMPLES: usize = 10;
    const WARMUP_BATCHES: usize = 6;
    let image_budget = 8 * 1024 * 1024;
    let groups = image_budget / COMMAND.len();
    // Exactly the original retained bytes, but no hub publish/snapshot preconditioning.
    // This isolates reconstruction; it does not reproduce the original full pipeline.
    let history = [MARKER, COMMAND.repeat(groups).as_slice()].concat();
    assert_eq!(history.len(), 8_388_612);
    assert_eq!(groups, 399_457);
    let mut fixture_fnv1a64 = 0xcbf29ce484222325u64;
    for &byte in &history {
        fixture_fnv1a64 ^= u64::from(byte);
        fixture_fnv1a64 = fixture_fnv1a64.wrapping_mul(0x100000001b3);
    }
    println!("{}", json!({"fixture":"original_8388608_reconstruction_isolated", "bytes":history.len(),
        "retained_groups":groups, "command_bytes":COMMAND.len(), "fnv1a64":format!("{fixture_fnv1a64:016x}"),
        "preconditioning":"direct identical buffer; no hub publish or snapshot", "batch":BATCH, "samples":SAMPLES, "warmup_batches":WARMUP_BATCHES,
        "operation":"new+resize+feed+render_snapshot+row_text_assert+drop; no search or hub work timed"}));
    let operation = || {
        let mut reconstructed = terminal(80, 24);
        reconstructed.feed(black_box(&history)).unwrap();
        assert!(reconstructed.render_snapshot().unwrap().row_text(0).contains("RETAINED-TEXT"));
        history.len()
    };
    for _ in 0..WARMUP_BATCHES * BATCH { black_box(operation()); }
    for sample in 0..SAMPLES {
        let before = resources();
        let start = Instant::now();
        for _ in 0..BATCH { assert_eq!(black_box(operation()), history.len()); }
        let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
        let after = resources();
        println!("{}", json!({"phase":"ghostty_reconstruct_8388608_isolated_batched", "sample":sample,
            "iterations":BATCH, "wall_total_ms":wall_ms, "wall_ms":wall_ms / BATCH as f64,
            "cpu_ms":(after.cpu_ms-before.cpu_ms) / BATCH as f64,
            "process_peak_rss_kib":after.max_rss_kib, "checksum":history.len()}));
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "qa".into());
    println!("{}", json!({"harness":"real-terminal-pipeline", "mode":mode,
        "os":std::env::consts::OS, "arch":std::env::consts::ARCH,
        "debug_assertions":cfg!(debug_assertions), "cpu_rss_available":cfg!(unix),
        "scope":"Ghostty/parser/hub only; excludes GPU, window compositor, actual PTY and user-input latency"}));
    search_qa(); history_qa();
    match mode.as_str() {
        "qa" => {},
        "search" => search_measurements(),
        "history" => history_measurements(),
        "reconstruct64" => reconstruction_64k_measurements(),
        "reconstruct8m" => reconstruction_8m_isolated_measurements(),
        "reconstruct8m-verify" => verify_8m_fixture(),
        "all" => { search_measurements(); history_measurements(); },
        _ => panic!("expected qa, search, history, reconstruct64, reconstruct8m, reconstruct8m-verify, or all"),
    }
}
