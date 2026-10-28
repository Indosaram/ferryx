//! Cell-boundary matching, independent of the terminal FFI so it can be regression-tested.

pub(super) fn append_row_matches(
    cells: &[String],
    normalized_query: &str,
    case_sensitive: bool,
    row: u16,
    matches: &mut Vec<(u16, u16, u16)>,
) {
    if normalized_query.is_empty() {
        return;
    }
    // Unicode lowercase can depend on the whole candidate (e.g. final sigma), so do not
    // lowercase individual cells or the entire row. Only ASCII has a bytewise fast path.
    let bytewise =
        case_sensitive || (normalized_query.is_ascii() && cells.iter().all(|cell| cell.is_ascii()));
    for start in 0..cells.len() {
        if bytewise {
            let mut offset = 0;
            for (end, cell) in cells.iter().enumerate().skip(start) {
                let remaining = &normalized_query.as_bytes()[offset..];
                let bytes = cell.as_bytes();
                if bytes.len() > remaining.len() {
                    break;
                }
                let prefix = &remaining[..bytes.len()];
                let equal = if case_sensitive {
                    prefix == bytes
                } else {
                    prefix.eq_ignore_ascii_case(bytes)
                };
                // Once a cell differs, appending more cells cannot make this start match.
                if !equal {
                    break;
                }
                offset += bytes.len();
                if offset == normalized_query.len() {
                    matches.push((row, start as u16, end as u16));
                    break;
                }
            }
        } else {
            let mut candidate = String::new();
            for (end, cell) in cells.iter().enumerate().skip(start) {
                candidate.push_str(cell);
                let normalized_candidate = candidate.to_lowercase();
                if normalized_candidate == normalized_query {
                    matches.push((row, start as u16, end as u16));
                    break;
                }
                if normalized_candidate.len() >= normalized_query.len() {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(cells: &[&str], query: &str, sensitive: bool) -> Vec<(u16, u16, u16)> {
        let cells: Vec<_> = cells.iter().map(|s| s.to_string()).collect();
        let query = if sensitive {
            query.to_owned()
        } else {
            query.to_lowercase()
        };
        let mut matches = Vec::new();
        append_row_matches(&cells, &query, sensitive, 7, &mut matches);
        matches
    }

    #[test]
    fn overlaps_and_case_preserve_column_bounds() {
        assert_eq!(
            find(&["a", "A", "a", "A"], "aa", false),
            vec![(7, 0, 1), (7, 1, 2), (7, 2, 3)]
        );
        assert_eq!(
            find(&["a", "A", "a", "A"], "aA", true),
            vec![(7, 0, 1), (7, 2, 3)]
        );
    }

    #[test]
    fn matches_only_whole_grapheme_cells() {
        assert_eq!(find(&["é", "e\u{301}", "界", " ", "👩‍💻"], "e", true), vec![]);
        assert_eq!(
            find(&["é", "e\u{301}", "界", " ", "👩‍💻"], "e\u{301}界 ", true),
            vec![(7, 1, 3)]
        );
        assert_eq!(find(&["ab", "c"], "b", true), vec![]);
        assert_eq!(find(&["ab", "c"], "a", true), vec![]);
    }

    #[test]
    fn empty_cells_and_queries_preserve_first_matching_end() {
        assert_eq!(
            find(&["", "a", "", "a"], "a", true),
            vec![(7, 0, 1), (7, 1, 1), (7, 2, 3), (7, 3, 3)]
        );
        assert!(find(&["a"], "", false).is_empty());
        assert!(find(&[], "a", true).is_empty());
        assert!(find(&["a"], "long query", false).is_empty());
    }

    #[test]
    fn unicode_lowercase_keeps_context_and_expansions() {
        assert_eq!(find(&["Ο", "Σ", "Α"], "ος", false), vec![(7, 0, 1)]);
        assert_eq!(find(&["Ο", "Σ", "Α"], "οσα", false), vec![(7, 0, 2)]);
        assert_eq!(find(&["İ", "x"], "i\u{307}", false), vec![(7, 0, 0)]);
        assert_eq!(find(&["K", "k"], "K", false), vec![(7, 0, 0), (7, 1, 1)]);
    }

    fn legacy(cells: &[String], query: &str, sensitive: bool) -> Vec<(u16, u16, u16)> {
        let mut matches = Vec::new();
        for start in 0..cells.len() {
            let mut candidate = String::new();
            for (end, cell) in cells.iter().enumerate().skip(start) {
                candidate.push_str(cell);
                let normalized = if sensitive {
                    candidate.clone()
                } else {
                    candidate.to_lowercase()
                };
                if normalized == query {
                    matches.push((7, start as u16, end as u16));
                    break;
                }
                if normalized.len() >= query.len() {
                    break;
                }
            }
        }
        matches
    }

    #[test]
    fn differential_against_original_for_mixed_cells() {
        let alphabet = [
            "",
            "a",
            "A",
            "ab",
            " ",
            "é",
            "e\u{301}",
            "Σ",
            "Ο",
            "İ",
            "界",
            "👩‍💻",
        ];
        for a in alphabet {
            for b in alphabet {
                for c in alphabet {
                    let cells = vec![a.to_owned(), b.to_owned(), c.to_owned()];
                    for query in [
                        a.to_owned(),
                        format!("{a}{b}"),
                        format!("{b}{c}"),
                        "missing".into(),
                    ] {
                        if query.is_empty() {
                            continue;
                        }
                        for sensitive in [true, false] {
                            let query = if sensitive {
                                query.clone()
                            } else {
                                query.to_lowercase()
                            };
                            let mut actual = Vec::new();
                            append_row_matches(&cells, &query, sensitive, 7, &mut actual);
                            assert_eq!(
                                actual,
                                legacy(&cells, &query, sensitive),
                                "cells={cells:?}, query={query:?}, sensitive={sensitive}"
                            );
                        }
                    }
                }
            }
        }
    }
}
