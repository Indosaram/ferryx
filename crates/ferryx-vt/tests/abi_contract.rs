//! Header-equality ABI guard for the constants hand-mirrored in
//! `src/sys.rs`. The pinned libghostty-vt headers and the Rust source are
//! parsed at test time (the `sys` module is private to the crate).

#[path = "../../../src-tauri/tests/support/ghostty_header_abi.rs"]
mod ghostty_header_abi;

use ghostty_header_abi::{header_values, parse_int, rust_consts};
use std::path::PathBuf;

/// Exact short-name -> header-name mappings, checked before the prefix table.
const EXACT_NAMES: &[(&str, &str)] = &[
    ("SUCCESS", "GHOSTTY_SUCCESS"),
    ("OUT_OF_SPACE", "GHOSTTY_OUT_OF_SPACE"),
    ("NO_VALUE", "GHOSTTY_NO_VALUE"),
    ("CELL_WIDE", "GHOSTTY_CELL_DATA_WIDE"),
    ("ROW_WRAP", "GHOSTTY_ROW_DATA_WRAP"),
];

/// Short-name prefix -> header prefix; first match wins, so longer prefixes come first.
const PREFIXES: &[(&str, &str)] = &[
    ("COMPRESSION_MODE_", "GHOSTTY_TERMINAL_COMPRESSION_MODE_"),
    ("COMPRESSION_RESULT_", "GHOSTTY_TERMINAL_COMPRESSION_RESULT_"),
    ("CLIPBOARD_WRITE_", "GHOSTTY_CLIPBOARD_WRITE_RESULT_"),
    ("CLIPBOARD_", "GHOSTTY_CLIPBOARD_LOCATION_"),
    ("OPT_", "GHOSTTY_TERMINAL_OPT_"),
    ("DATA_", "GHOSTTY_TERMINAL_DATA_"),
    ("POINT_", "GHOSTTY_POINT_TAG_"),
    ("STYLE_COLOR_", "GHOSTTY_STYLE_COLOR_"),
    ("RS_", "GHOSTTY_RENDER_STATE_DATA_"),
    ("WIDE_", "GHOSTTY_CELL_WIDE_"),
];

fn header_name(rust: &str) -> Option<String> {
    if let Some((_, h)) = EXACT_NAMES.iter().find(|(r, _)| *r == rust) {
        return Some((*h).to_string());
    }
    PREFIXES
        .iter()
        .find_map(|(p, h)| rust.strip_prefix(p).map(|rest| format!("{h}{rest}")))
}

#[test]
fn sys_constants_match_headers() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let header = header_values(&manifest.join("../../src-tauri/vendor/ghostty/include/ghostty/vt"));
    let file = manifest.join("src/sys.rs");
    let consts = rust_consts(&file);
    let mut problems = Vec::new();
    for (name, raw) in &consts {
        let Some(h_name) = header_name(name) else {
            problems.push(format!("{name}: no header mapping; extend EXACT_NAMES or PREFIXES"));
            continue;
        };
        let Some(value) = parse_int(raw) else {
            problems.push(format!("{name} = {raw} is not an integer literal"));
            continue;
        };
        match header.get(&h_name) {
            None => problems.push(format!("{name} maps to {h_name}, which the v1.4 headers do not declare")),
            Some(&h) if h != value => problems.push(format!("{name} = {value} but header {h_name} = {h}")),
            Some(_) => {}
        }
    }
    assert!(
        problems.is_empty(),
        "{}: ABI constant drift vs v1.4 headers:\n{}",
        file.display(),
        problems.join("\n")
    );
    assert!(consts.len() >= 40, "guard saw only {} constants; parser regression?", consts.len());
}
