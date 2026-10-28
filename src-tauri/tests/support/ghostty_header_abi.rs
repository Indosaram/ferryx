//! Test-time parser for the pinned libghostty-vt C headers and for the
//! hand-mirrored Rust constant files. Shared by the src-tauri and ferryx-vt
//! ABI contract tests so both read the headers the same way.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Every integer `GHOSTTY_*` name declared by the headers under `vt_dir`
/// (enumerators, with implicit increments counted, and integer `#define`s,
/// with `ghostty_mode_new(N, ansi)` packed exactly like modes.h does).
pub fn header_values(vt_dir: &Path) -> BTreeMap<String, i64> {
    let mut files = Vec::new();
    collect_headers(vt_dir, &mut files);
    assert!(
        !files.is_empty(),
        "no headers under {} (is the ghostty submodule checked out?)",
        vt_dir.display()
    );
    let mut values = BTreeMap::new();
    for file in &files {
        let raw = fs::read_to_string(file).unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        let src = strip_comments(&raw);
        for line in src.lines() {
            let Some(rest) = line.trim_start().strip_prefix('#') else { continue };
            let Some(rest) = rest.trim_start().strip_prefix("define") else { continue };
            let rest = rest.trim();
            let (name, expr) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            if !name.starts_with("GHOSTTY_") || name.contains('(') {
                continue;
            }
            if let Some(value) = eval(expr, &BTreeMap::new(), &values) {
                insert(&mut values, name, value, file);
            }
        }
        let mut rest = src.as_str();
        while let Some(pos) = find_keyword(rest, "enum") {
            let after = &rest[pos + "enum".len()..];
            match (after.find('{'), after.find(';')) {
                (Some(open), semi) if semi.is_none_or(|s| open < s) => {
                    let body = &after[open + 1..];
                    let close = body
                        .find('}')
                        .unwrap_or_else(|| panic!("{}: unterminated enum body", file.display()));
                    parse_enum_body(&body[..close], &mut values, file);
                    rest = &body[close + 1..];
                }
                _ => rest = after,
            }
        }
    }
    values
}

/// `(NAME, VALUE)` for every single-line `pub const NAME: T = VALUE;` in a Rust
/// source file. A `pub const` that does not fit that shape fails loudly so a
/// new constant can never slip past the guard.
pub fn rust_consts(path: &Path) -> Vec<(String, String)> {
    let src = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut out = Vec::new();
    for (idx, line) in src.lines().enumerate() {
        let Some(rest) = line.trim_start().strip_prefix("pub const ") else { continue };
        if rest.starts_with("fn ") {
            continue;
        }
        let at = || format!("{}:{}", path.display(), idx + 1);
        let (name, rest) = rest
            .split_once(':')
            .unwrap_or_else(|| panic!("{}: unparsed const `{line}`", at()));
        let value = rest
            .split_once('=')
            .and_then(|(_, v)| v.trim().strip_suffix(';'))
            .unwrap_or_else(|| panic!("{}: const must be single-line `pub const NAME: T = VALUE;`", at()));
        out.push((name.trim().to_string(), value.trim().to_string()));
    }
    assert!(!out.is_empty(), "no `pub const` found in {}", path.display());
    out
}

/// Decimal or `0x` hex integer literal, optionally negative, `_` separators allowed.
pub fn parse_int(raw: &str) -> Option<i64> {
    let s = raw.trim().replace('_', "");
    let (neg, digits) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.as_str()),
    };
    let value = match digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
        Some(hex) => i64::from_str_radix(hex, 16).ok()?,
        None => digits.parse::<i64>().ok()?,
    };
    Some(if neg { -value } else { value })
}

fn collect_headers(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .map(|entry| entry.expect("header dir entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_headers(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "h") {
            out.push(path);
        }
    }
}

fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < src.len() {
        let rest = &src[i..];
        if rest.starts_with("/*") {
            out.push(' ');
            i = rest[2..].find("*/").map_or(src.len(), |end| i + 2 + end + 2);
        } else if rest.starts_with("//") {
            i = rest.find('\n').map_or(src.len(), |end| i + end);
        } else {
            let ch = rest.chars().next().expect("non-empty rest");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn find_keyword(hay: &str, kw: &str) -> Option<usize> {
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut from = 0;
    while let Some(off) = hay[from..].find(kw) {
        let at = from + off;
        let before_ok = hay[..at].chars().next_back().is_none_or(|c| !is_ident(c));
        let after_ok = hay[at + kw.len()..].chars().next().is_none_or(|c| !is_ident(c));
        if before_ok && after_ok {
            return Some(at);
        }
        from = at + kw.len();
    }
    None
}

fn parse_enum_body(body: &str, values: &mut BTreeMap<String, i64>, file: &Path) {
    let mut scope = BTreeMap::new();
    let mut next = 0i64;
    for entry in body.split(',').map(str::trim).filter(|e| !e.is_empty()) {
        let (name, value) = match entry.split_once('=') {
            Some((name, expr)) => {
                let name = name.trim();
                let value = eval(expr, &scope, values).unwrap_or_else(|| {
                    panic!("{}: cannot evaluate enumerator {name} = {}", file.display(), expr.trim())
                });
                (name, value)
            }
            None => (entry, next),
        };
        assert!(
            name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "{}: unexpected enum entry `{entry}`",
            file.display()
        );
        scope.insert(name.to_string(), value);
        insert(values, name, value, file);
        next = value + 1;
    }
}

fn insert(values: &mut BTreeMap<String, i64>, name: &str, value: i64, file: &Path) {
    if let Some(prev) = values.insert(name.to_string(), value) {
        assert_eq!(prev, value, "{}: {name} redefined with a different value", file.display());
    }
}

fn fully_parenthesized(e: &str) -> bool {
    if !e.starts_with('(') || !e.ends_with(')') {
        return false;
    }
    let mut depth = 0i32;
    for (i, c) in e.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 && i + 1 < e.len() {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

fn eval(expr: &str, scope: &BTreeMap<String, i64>, global: &BTreeMap<String, i64>) -> Option<i64> {
    let mut e = expr.trim();
    while fully_parenthesized(e) {
        e = e[1..e.len() - 1].trim();
    }
    if e == "GHOSTTY_ENUM_MAX_VALUE" {
        return Some(i64::from(i32::MAX));
    }
    if let Some(value) = parse_int(e.trim_end_matches(['u', 'U', 'l', 'L'])) {
        return Some(value);
    }
    if let Some((lhs, rhs)) = e.split_once("<<") {
        return Some(eval(lhs, scope, global)? << eval(rhs, scope, global)?);
    }
    if let Some(args) = e.strip_prefix("ghostty_mode_new(").and_then(|r| r.strip_suffix(')')) {
        let (value, ansi) = args.split_once(',')?;
        let ansi = match ansi.trim() {
            "true" => 1,
            "false" => 0,
            _ => return None,
        };
        return Some((parse_int(value)? & 0x7FFF) | (ansi << 15));
    }
    scope.get(e).or_else(|| global.get(e)).copied()
}
