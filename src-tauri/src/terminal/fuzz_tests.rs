//! Property tests for the daemon's VT parsing paths: the screen mirror, the unit splitter and
//! the retained history behind reconstruction, driven by arbitrary byte streams biased toward
//! escape, control-string and UTF-8 boundary cases.
//!
//! 256 cases per property by default; set `PROPTEST_CASES` for longer runs. The libFuzzer
//! targets in `src-tauri/fuzz/` drive the same entry points without a case budget.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use proptest::prelude::*;

use super::output_hub::retained_history::{RetainedHistory, RetentionBudget};
use super::output_hub::vt_stream::{Unit, VtUnitSplitter};
use super::screen_mirror::{ScreenMirror, ScreenSeed, MIRROR_SCROLLBACK_LINES};

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(256),
        ..ProptestConfig::default()
    }
}

const UTF8_SAMPLE: &str = "e\u{301} \u{4e2d}\u{6587}\u{1f469}\u{200d}\u{1f4bb}\u{fe0f}\u{a0}\u{10ffff}";

fn terminator() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        3 => Just(b"\x1b\\".to_vec()),
        2 => Just(b"\x07".to_vec()),
        1 => Just(Vec::new()),
        1 => any::<u8>().prop_map(|byte| vec![0x1b, byte]),
    ]
}

fn payload(max: usize) -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        "[ -~]{0,40}".prop_map(String::into_bytes),
        prop::collection::vec(any::<u8>(), 0..max),
    ]
}

fn control_string(intros: Vec<&'static [u8]>, max: usize) -> impl Strategy<Value = Vec<u8>> {
    (prop::sample::select(intros), payload(max), terminator()).prop_map(|(intro, body, end)| {
        let mut out = intro.to_vec();
        out.extend(body);
        out.extend(end);
        out
    })
}

/// One chunk of PTY output, biased toward the sequences the splitter and parser special-case.
fn chunk() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        4 => prop::collection::vec(any::<u8>(), 0..64),
        3 => "[a-z0-9 ]{0,40}".prop_map(String::into_bytes),
        // UTF-8 cut at arbitrary byte offsets, including inside a code point.
        2 => (0..=UTF8_SAMPLE.len(), 0..=UTF8_SAMPLE.len())
            .prop_map(|(a, b)| UTF8_SAMPLE.as_bytes()[a.min(b)..a.max(b)].to_vec()),
        // CSI with private markers and intermediates, ending in any final byte or none.
        3 => (
            prop::sample::select(vec!["", "?", ">", "=", "<"]),
            "[0-9;:]{0,24}",
            "[ -/]{0,2}",
            prop::option::weighted(0.9, 0x40u8..=0x7e),
        )
            .prop_map(|(private, params, intermediate, final_byte)| {
                let mut out = format!("\x1b[{private}{params}{intermediate}").into_bytes();
                out.extend(final_byte);
                out
            }),
        // OSC: titles, cwd, notifications, clipboard, hyperlinks, prompts, program status.
        3 => control_string(
            vec![&b"\x1b]0;"[..], b"\x1b]2;", b"\x1b]7;", b"\x1b]8;;", b"\x1b]9;", b"\x1b]52;c;",
                b"\x1b]133;", b"\x1b]777;notify;", b"\x1b]1337;", b"\x1b]7501;", b"\x1b]4;1;", b"\x1b]104;"],
            48,
        ),
        // Kitty graphics APC, single and multipart.
        2 => control_string(
            vec![&b"\x1b_Ga=T,f=24,m=1;"[..], b"\x1b_Gm=1;", b"\x1b_Gm=0;", b"\x1b_Ga=T,f=100;", b"\x1b_Ga=d;",
                b"\x1b_G"],
            64,
        ),
        // DCS (sixel, XTGETTCAP, DECRQSS), SOS/PM, and iTerm inline images.
        2 => control_string(
            vec![&b"\x1bP0;0;0q"[..], b"\x1bP+q", b"\x1bP$q", b"\x1bX", b"\x1b^",
                b"\x1b]1337;File=inline=1:", b"\x1b]1337;MultipartFile=inline=1", b"\x1b]1337;FilePart="],
            64,
        ),
        // Line controls, a lone ESC, and screen-level sequences.
        3 => prop::sample::select(vec![&b"\r\n"[..], b"\n", b"\r", b"\x08", b"\t", b"\x1b",
            b"\x1b[?1049h", b"\x1b[?1049l", b"\x1b[2J", b"\x1b[3J", b"\x1bc",
            b"\x1b7", b"\x1b8", b"\x1b[r", b"\x1b[2;5r", b"\x1b#8", b"\x1b(0", b"\x1b(B"])
            .prop_map(<[u8]>::to_vec),
        // Bursts that scroll many lines, and large chunks that cross unit and flush limits.
        1 => (1usize..600).prop_map(|lines| "x\r\n".repeat(lines).into_bytes()),
        1 => prop::collection::vec(any::<u8>(), 4096..9000),
    ]
}

fn stream() -> impl Strategy<Value = Vec<Vec<u8>>> {
    prop::collection::vec(chunk(), 1..40)
}

proptest! {
    #![proptest_config(config())]
    #[test]
    fn osc_effects(chunks in stream()) {
        let mut mirror = ScreenMirror::new(80, 24).unwrap();
        prop_assert!(mirror.enable_program_status());
        mirror.set_osc52_write(true);
        for bytes in chunks {
            mirror.feed(&bytes);
            for effect in mirror.take_osc_effects() {
                use crate::native_terminal::osc_effects::{sanitize, OscEffect};
                match effect {
                    OscEffect::Notify { title, body } => {
                        prop_assert_eq!(sanitize(&title), title);
                        prop_assert_eq!(sanitize(&body), body);
                    },
                    OscEffect::Clipboard { text } => prop_assert_eq!(sanitize(&text), text),
                }
            }
            mirror.feed_replay(&bytes);
            prop_assert!(mirror.take_osc_effects().is_empty());
        }
    }
}

/// The single place the properties feed a mirror.
fn feed_mirror(mirror: &mut ScreenMirror, bytes: &[u8]) {
    mirror.feed(bytes);
}

/// Exact minimized input retained from verify-linux/gha3/rex-verify-linux-gate/1.log.
/// Keep the acceptance assertion intact while repairing the native formatter.
#[test]
fn malformed_stream_seed_round_trip_regression() {
    #[derive(serde::Deserialize)]
    struct Case { cols: u16, rows: u16, chunks: Vec<Vec<u8>> }
    let case: Case = serde_json::from_str(include_str!("../../tests/fixtures/screen_mirror_malformed_round_trip.json")).unwrap();
    let mut mirror = ScreenMirror::new(case.cols, case.rows).unwrap();
    for chunk in case.chunks { mirror.feed(&chunk); }
    let seed = mirror.seed().unwrap();
    let rebuilt = ScreenMirror::from_seed(&seed).unwrap();
    assert_eq!(without_spacer_heads(rebuilt.grid_text_for_test()), without_spacer_heads(mirror.grid_text_for_test()));
}

/// Narrowest grid whose scrollback row count is bounded by `2 * MIRROR_SCROLLBACK_LINES`.
/// libghostty prunes scrollback a whole page at a time, and a page holds more rows the
/// narrower the grid, so a 2-column mirror can keep several thousand rows over the limit.
/// Narrower grids are still fed and round-tripped; only the row bound is skipped for them.
const MIN_COLS_FOR_ROW_BOUND: u16 = 20;

fn scrollback_within_bound(mirror: &ScreenMirror, cols: u16) -> bool {
    cols < MIN_COLS_FOR_ROW_BOUND || mirror.scrollback_rows_for_test() <= 2 * MIRROR_SCROLLBACK_LINES
}

fn seed_hash(seed: &ScreenSeed) -> u64 {
    let mut hasher = DefaultHasher::new();
    (seed.cols, seed.rows, &seed.bytes).hash(&mut hasher);
    hasher.finish()
}

/// Grid text with one known reconstruction loss folded away: a wide character (possibly a
/// multi-code-point grapheme) that did not fit in a row's last column leaves a spacer-head cell
/// there, which row text reports as that grapheme again, and the formatter does not carry the
/// spacer head into the seed. The rows look the same on screen; only the doubled grapheme
/// differs, so the longest such repeat is dropped from the end of the earlier row.
fn without_spacer_heads(mut rows: Vec<String>) -> Vec<String> {
    for index in 1..rows.len() {
        let next: Vec<char> = rows[index].chars().take(8).collect();
        if next.first().is_none_or(char::is_ascii) {
            continue;
        }
        let previous = &mut rows[index - 1];
        let repeat = (1..=next.len()).rev().find(|&len| {
            let cluster: String = next[..len].iter().collect();
            previous.ends_with(cluster.as_str())
        });
        if let Some(len) = repeat {
            for _ in 0..len {
                previous.pop();
            }
            let trimmed = previous.trim_end().len();
            previous.truncate(trimmed);
        }
    }
    rows
}

fn split_all(chunks: &[Vec<u8>], max_unit: usize, flush: usize) -> (Vec<Unit>, VtUnitSplitter) {
    let mut splitter = VtUnitSplitter::new(max_unit, flush);
    let mut units = Vec::new();
    for (sequence, chunk) in (1u64..).zip(chunks) {
        splitter.push(sequence, chunk, &mut units);
    }
    (units, splitter)
}

/// Splits `bytes` at `parts` boundaries into `(order, piece)`; no parts means one piece.
fn pieces(bytes: &[u8], parts: &[(usize, u64, u64)], order: u64) -> Vec<(u64, Vec<u8>)> {
    if parts.is_empty() {
        return vec![(order, bytes.to_vec())];
    }
    parts
        .iter()
        .enumerate()
        .map(|(index, &(start, order, _))| {
            let end = parts.get(index + 1).map_or(bytes.len(), |part| part.0);
            (order, bytes[start..end].to_vec())
        })
        .collect()
}

/// Every byte the splitter keeps, in stream order: emitted units and the pending tail, split at
/// their parts and ordered by allocation as `RetainedHistory::records_with_pending` does.
fn kept_bytes(units: &[Unit], splitter: &VtUnitSplitter) -> Vec<u8> {
    let mut all: Vec<(u64, Vec<u8>)> = units
        .iter()
        .flat_map(|unit| pieces(&unit.bytes, &unit.parts, unit.order))
        .collect();
    let pending = splitter.pending();
    let next = all.iter().map(|piece| piece.0 + 1).max().unwrap_or(0);
    all.extend(pieces(&pending.bytes, &pending.parts, next));
    all.sort_by_key(|piece| piece.0);
    all.into_iter().flat_map(|piece| piece.1).collect()
}

proptest! {
    #![proptest_config(config())]

    /// Arbitrary output never panics the mirror or grows its history past the scrollback bound,
    /// and its seed is a stable snapshot: taking it twice, passing it through serde and
    /// rebuilding a mirror from it all reproduce the same screen, and a rebuilt mirror's seed
    /// is a fixed point.
    #[test]
    fn screen_mirror_feed_and_seed_round_trip(chunks in stream(), cols in 2u16..120, rows in 2u16..50) {
        let mut mirror = ScreenMirror::new(cols, rows).expect("mirror");
        for chunk in &chunks {
            feed_mirror(&mut mirror, chunk);
            prop_assert!(scrollback_within_bound(&mirror, cols), "scrollback {} rows", mirror.scrollback_rows_for_test());
        }
        let seed = mirror.seed().expect("seed");
        prop_assert_eq!((seed.cols, seed.rows), (cols, rows));
        prop_assert_eq!(seed_hash(&mirror.seed().expect("second seed")), seed_hash(&seed));

        let json = serde_json::to_string(&seed).expect("serialize seed");
        let decoded: ScreenSeed = serde_json::from_str(&json).expect("deserialize seed");
        prop_assert_eq!(seed_hash(&decoded), seed_hash(&seed));

        let mut rebuilt = ScreenMirror::from_seed(&decoded).expect("rebuilt mirror");
        prop_assert_eq!(
            without_spacer_heads(rebuilt.grid_text_for_test()),
            without_spacer_heads(mirror.grid_text_for_test())
        );
        prop_assert!(scrollback_within_bound(&rebuilt, cols), "rebuilt scrollback {} rows", rebuilt.scrollback_rows_for_test());
        let reseed = rebuilt.seed().expect("reseed");
        let mut again = ScreenMirror::from_seed(&reseed).expect("second rebuild");
        prop_assert_eq!(seed_hash(&again.seed().expect("third seed")), seed_hash(&reseed));
    }

    /// The splitter emits non-empty units with distinct orders and sequences it was given, keeps
    /// a pending tail bounded by its limits, and keeps the same bytes however the stream is
    /// chunked.
    #[test]
    fn vt_unit_splitter_is_ordered_bounded_and_chunking_invariant(
        chunks in stream(),
        max_unit in 64usize..4096,
        flush in 1usize..8192,
    ) {
        let (units, splitter) = split_all(&chunks, max_unit, flush);
        let last_sequence = chunks.len() as u64;
        let mut orders: Vec<u64> = units.iter().map(|unit| unit.order).collect();
        orders.sort_unstable();
        orders.dedup();
        prop_assert_eq!(orders.len(), units.len(), "two units share an order");
        for unit in &units {
            prop_assert!(!unit.bytes.is_empty());
            prop_assert!((1..=last_sequence).contains(&unit.sequence));
            for pair in unit.parts.windows(2) {
                prop_assert!(pair[0].0 <= pair[1].0);
            }
        }
        // Open image group and string are each capped by the unit limit; unflushed text by the
        // flush limit plus the text run and control sequence still open when a chunk ended,
        // and one stray ESC per chunk.
        let largest_chunk = chunks.iter().map(Vec::len).max().unwrap_or(0);
        let pending = splitter.pending().bytes.len();
        let bound = 3 * max_unit + flush + 2 * largest_chunk + chunks.len() + 8;
        prop_assert!(pending <= bound, "pending tail {pending} exceeds {bound}");

        let whole = chunks.concat();
        let (one_units, one_splitter) = split_all(std::slice::from_ref(&whole), max_unit, flush);
        prop_assert_eq!(kept_bytes(&units, &splitter), kept_bytes(&one_units, &one_splitter));
    }

    /// Retained history stays inside its text and image budgets whatever arrives, and its
    /// replay feeds a fresh mirror that still produces a seed.
    #[test]
    fn retained_history_stays_within_budget(chunks in stream(), text_budget in 64usize..16384) {
        let budget = RetentionBudget::from_text_capacity(text_budget);
        let mut history = RetainedHistory::new(budget);
        for (sequence, chunk) in (1u64..).zip(&chunks) {
            history.push(sequence, chunk);
            prop_assert!(history.text_bytes() <= budget.text_bytes, "text {} > {}", history.text_bytes(), budget.text_bytes);
            prop_assert!(history.image_bytes() <= budget.image_bytes, "image {} > {}", history.image_bytes(), budget.image_bytes);
            prop_assert_eq!(history.len(), history.text_bytes() + history.image_bytes());
        }
        let replay = history.replay();
        let mut mirror = ScreenMirror::new(80, 24).expect("mirror");
        feed_mirror(&mut mirror, &replay);
        prop_assert!(mirror.seed().is_some());
    }
}
