//! Header-equality ABI guard: every integer constant hand-mirrored from the
//! pinned libghostty-vt headers must equal the value the headers declare.
//! Headers and Rust sources are parsed at test time; no bindgen, no FFI calls.

#[path = "support/ghostty_header_abi.rs"]
mod ghostty_header_abi;

use ghostty_header_abi::{header_values, parse_int, rust_consts};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Rust names that differ from their header name.
const NAME_ALIASES: &[(&str, &str)] = &[("GHOSTTY_MODE_SYNCHRONIZED_OUTPUT", "GHOSTTY_MODE_SYNC_OUTPUT")];

/// Snapshot envelope facts that live in the snapshot.h format docs, not in an enum.
const SNAPSHOT_ENVELOPE_CONSTS: &[&str] = &["GHOSTTY_SNAPSHOT_MAGIC", "GHOSTTY_SNAPSHOT_VERSION_1"];

/// sys/kitty.rs short names -> header names. Every const in kitty.rs must be listed.
const KITTY_NAMES: &[(&str, &str)] = &[
    ("STORAGE_LIMIT", "GHOSTTY_TERMINAL_OPT_KITTY_IMAGE_STORAGE_LIMIT"),
    ("GRAPHICS", "GHOSTTY_TERMINAL_DATA_KITTY_GRAPHICS"),
    ("DECODE_PNG", "GHOSTTY_SYS_OPT_DECODE_PNG"),
    ("PLACEMENT_ITERATOR", "GHOSTTY_KITTY_GRAPHICS_DATA_PLACEMENT_ITERATOR"),
    ("GENERATION", "GHOSTTY_KITTY_GRAPHICS_DATA_GENERATION"),
    ("PLACEMENT_IMAGE_ID", "GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_IMAGE_ID"),
    ("PLACEMENT_ID", "GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_PLACEMENT_ID"),
    ("PLACEMENT_IS_VIRTUAL", "GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_IS_VIRTUAL"),
    ("PLACEMENT_X_OFFSET", "GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_X_OFFSET"),
    ("PLACEMENT_Y_OFFSET", "GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_Y_OFFSET"),
    ("PLACEMENT_Z", "GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_Z"),
    ("IMAGE_WIDTH", "GHOSTTY_KITTY_IMAGE_DATA_WIDTH"),
    ("IMAGE_HEIGHT", "GHOSTTY_KITTY_IMAGE_DATA_HEIGHT"),
    ("IMAGE_FORMAT", "GHOSTTY_KITTY_IMAGE_DATA_FORMAT"),
    ("IMAGE_DATA", "GHOSTTY_KITTY_IMAGE_DATA_DATA_PTR"),
    ("IMAGE_LEN", "GHOSTTY_KITTY_IMAGE_DATA_DATA_LEN"),
    ("IMAGE_GENERATION", "GHOSTTY_KITTY_IMAGE_DATA_GENERATION"),
];

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn vt_include_dir() -> PathBuf {
    manifest_dir().join("vendor/ghostty/include/ghostty/vt")
}

fn sys_file(name: &str) -> PathBuf {
    manifest_dir().join("src/native_terminal/sys").join(name)
}

/// Compares each `(rust_name, header_name, rust_value)` and returns one line per problem.
fn compare(
    file: &Path,
    header: &BTreeMap<String, i64>,
    rows: impl IntoIterator<Item = (String, String, String)>,
) -> (usize, Vec<String>) {
    let mut checked = 0;
    let mut problems = Vec::new();
    for (rust_name, header_name, raw) in rows {
        let Some(rust_value) = parse_int(&raw) else {
            problems.push(format!("{}: {rust_name} = {raw} is not an integer literal", file.display()));
            continue;
        };
        match header.get(&header_name) {
            None => problems.push(format!(
                "{}: {rust_name} maps to {header_name}, which the v1.4 headers do not declare",
                file.display()
            )),
            Some(&h) if h != rust_value => problems.push(format!(
                "{}: {rust_name} = {rust_value} but header {header_name} = {h}",
                file.display()
            )),
            Some(_) => checked += 1,
        }
    }
    (checked, problems)
}

#[test]
fn ghostty_prefixed_constants_match_headers() {
    let header = header_values(&vt_include_dir());
    let mut checked = 0;
    let mut problems = Vec::new();
    for file in [sys_file("constants.rs"), sys_file("types.rs")] {
        let rows = rust_consts(&file)
            .into_iter()
            .filter(|(name, _)| name.starts_with("GHOSTTY_") && !SNAPSHOT_ENVELOPE_CONSTS.contains(&name.as_str()))
            .map(|(name, value)| {
                let header_name = NAME_ALIASES
                    .iter()
                    .find(|(rust, _)| *rust == name)
                    .map_or(name.clone(), |(_, h)| (*h).to_string());
                (name, header_name, value)
            });
        let (n, mut p) = compare(&file, &header, rows);
        checked += n;
        problems.append(&mut p);
    }
    assert!(problems.is_empty(), "ABI constant drift vs v1.4 headers:\n{}", problems.join("\n"));
    assert!(checked >= 120, "guard checked only {checked} constants; parser regression?");
}

#[test]
fn kitty_constants_match_headers() {
    let header = header_values(&vt_include_dir());
    let file = sys_file("kitty.rs");
    let consts = rust_consts(&file);
    let mut unmapped = Vec::new();
    let rows: Vec<_> = consts
        .into_iter()
        .filter_map(|(name, value)| match KITTY_NAMES.iter().find(|(rust, _)| *rust == name) {
            Some((_, h)) => Some((name, (*h).to_string(), value)),
            None => {
                unmapped.push(name);
                None
            }
        })
        .collect();
    assert!(unmapped.is_empty(), "add these kitty.rs consts to KITTY_NAMES: {unmapped:?}");
    let (checked, problems) = compare(&file, &header, rows);
    assert!(problems.is_empty(), "Kitty ABI constant drift vs v1.4 headers:\n{}", problems.join("\n"));
    assert_eq!(checked, KITTY_NAMES.len());
}

#[test]
fn snapshot_envelope_constants_match_header_docs() {
    let snapshot_h = fs::read_to_string(vt_include_dir().join("snapshot.h")).expect("read snapshot.h");
    let consts: BTreeMap<String, String> = rust_consts(&sys_file("constants.rs")).into_iter().collect();
    assert_eq!(consts["GHOSTTY_SNAPSHOT_MAGIC"], "b\"GHOSTSNP\"");
    assert!(
        snapshot_h.contains("| \"GHOSTSNP\"") && snapshot_h.contains("8-byte magic  | u16"),
        "snapshot.h envelope table no longer documents an 8-byte GHOSTSNP magic followed by a u16 version"
    );
    assert_eq!(consts["GHOSTTY_SNAPSHOT_VERSION_1"], "1");
    assert!(
        snapshot_h.contains("Snapshot format version 1 "),
        "snapshot.h no longer documents snapshot format version 1"
    );
}

#[test]
fn header_parser_sees_known_values() {
    let header = header_values(&vt_include_dir());
    for (name, value) in [
        ("GHOSTTY_NO_VALUE", -4),
        ("GHOSTTY_KEY_A", 20),
        ("GHOSTTY_KEY_F12", 132),
        ("GHOSTTY_SCROLL_VIEWPORT_ROW", 3),
        ("GHOSTTY_TERMINAL_BOUNDARY_OSC_END", 1 << 6),
        ("GHOSTTY_MODE_KAM", 2 | 0x8000),
        ("GHOSTTY_MODE_SYNC_OUTPUT", 2026),
        ("GHOSTTY_SNAPSHOT_DECODER_DATA_PROGRESS_KIND", 11),
        ("GHOSTTY_SNAPSHOT_DECODER_PROGRESS_KIND_IMAGE", 1),
    ] {
        assert_eq!(header.get(name), Some(&value), "{name}");
    }
}
