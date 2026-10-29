//! Todo 14 (.omo/plans/ghostty-1-4-upgrade.md): `ghostty_search` may only ever stand in for
//! `search_grid` where both give identical results. These tests pin that identity on mixed
//! ASCII/CJK/emoji/combining text and pin which queries must stay on `search_grid`.

use ferryx_lib::native_terminal::{ghostty_search_eligible, NativeTerminal, TerminalEngine};

fn mixed_terminal() -> NativeTerminal {
    let mut term = NativeTerminal::new(40, 6).expect("create terminal");
    let rows = [
        "aAaA error ERROR Error",
        "界a 日志 Error 处理 完成",
        "👩‍💻 build 🚀 Error ✅ done",
        "e\u{301}tape e x\u{301} ΟΣΑ İ",
        "tab\there   gap   end",
        "wraps-past-forty-columns-and-keeps-error-going",
    ];
    let mut text = rows.join("\r\n");
    // Push the first rows into scrollback so both engines must search history too.
    for i in 0..30 {
        text.push_str(&format!("\r\nfill-{i:02} error"));
    }
    term.feed_str(&text).expect("feed mixed corpus");
    term
}

#[test]
fn ghostty_search_matches_search_grid_for_eligible_queries() {
    let mut term = mixed_terminal();
    let queries = [
        "error", "ERROR", "Error", "aa", "a", "e", "x", "tape", "done", "build", "gap   end",
        "tab", "here", "fill-0", "error going", "-error-", "ab", "absent", "Σ-free", "wraps",
    ];
    let mut compared = 0;
    for query in queries {
        if !ghostty_search_eligible(query, false) {
            continue;
        }
        let grid = term.search_grid(query, false).expect("search_grid");
        let ghostty = term
            .search_with_ghostty(query, false)
            .expect("ghostty_search");
        assert_eq!(ghostty, grid, "query {query:?}");
        compared += 1;
    }
    assert!(compared >= 15, "only {compared} queries compared");
    // Guard against vacuous identity: the corpus must produce real hits, history included.
    let error_hits = term.search_grid("error", false).expect("search_grid");
    assert!(error_hits.len() >= 30, "got {error_hits:?}");
    assert!(error_hits.iter().any(|&(row, _, _)| row == 0));
}

#[test]
fn ghostty_search_skips_partial_graphemes_like_search_grid() {
    let mut term = NativeTerminal::new(20, 3).expect("create terminal");
    term.feed_str("e\u{301} x\u{301}e").expect("feed combining marks");
    // Cells: 0 = e+U+0301, 1 = space, 2 = x+U+0301, 3 = e.
    assert_eq!(term.search_grid("e", false).unwrap(), vec![(0, 3, 3)]);
    assert_eq!(term.search_with_ghostty("e", false).unwrap(), vec![(0, 3, 3)]);
    assert_eq!(term.search_grid("x", false).unwrap(), Vec::new());
    assert_eq!(term.search_with_ghostty("x", false).unwrap(), Vec::new());
}

#[test]
fn non_ascii_and_case_sensitive_queries_route_to_search_grid() {
    // Case-insensitive non-ASCII: ghostty_search folds ASCII only, search_grid folds Unicode.
    assert!(!ghostty_search_eligible("ÄBC", false));
    let mut term = NativeTerminal::new(20, 3).expect("create terminal");
    term.feed_str("äbc").expect("feed");
    assert_eq!(term.search_grid("ÄBC", false).unwrap(), vec![(0, 0, 2)]);
    assert!(term.search_with_ghostty("ÄBC", false).is_err());

    // Every case-sensitive query: ghostty_search has no case-sensitive mode.
    for query in ["abc", "ABC", "äbc", "error"] {
        assert!(!ghostty_search_eligible(query, true), "{query:?}");
        assert!(term.search_with_ghostty(query, true).is_err(), "{query:?}");
    }

    // Leading/trailing spaces: the formatter trims trailing blanks that search_grid reads as spaces.
    assert!(!ghostty_search_eligible(" abc", false));
    assert!(!ghostty_search_eligible("abc ", false));
    // KELVIN SIGN lowercases to ASCII 'k' in search_grid but not in ghostty_search.
    assert!(!ghostty_search_eligible("ok", false));
    term.feed_str("\r\n\u{212A}").expect("feed kelvin sign");
    assert_eq!(term.search_grid("k", false).unwrap(), vec![(1, 0, 0)]);
    // Control bytes and the empty query never reach ghostty_search.
    assert!(!ghostty_search_eligible("a\tb", false));
    assert!(!ghostty_search_eligible("", false));

    assert!(ghostty_search_eligible("abc", false));
    assert!(ghostty_search_eligible("error: retry 1", false));
}
