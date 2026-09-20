//! Terminal URL span detection for whole-URL selection gestures.
//!
//! Ghostty's word selection splits on `:`, `/` is kept, `?`/`#` are kept, so a
//! double click inside `https://example.com/a?b` only yields `//example.com/a?b`.
//! Terminals are expected to select the *entire* URL on double click, so the
//! selection layer asks this module for the URL span covering the clicked cell.
//!
//! The accepted shape intentionally mirrors `ui/src/lib/linkRouting.ts` so that
//! double-click selection and Cmd+click opening agree on where a URL ends.

use std::sync::LazyLock;

use regex::Regex;

/// Schemes that are detected in terminal output. Mirrors the UI link router plus
/// the common non-http schemes Ghostty highlights.
static URL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:https?|ftp|file|ssh|git|gemini|gopher|ipfs|ipns)://[^\s<>"'`|]+|(?i)mailto:[^\s<>"'`|]+"#)
        .expect("terminal URL pattern is a valid regex")
});

/// Trailing characters that are almost always sentence punctuation rather than
/// part of the URL.
const TRAILING_PUNCTUATION: [char; 6] = ['.', ',', ';', ':', '!', '?'];

fn trim_trailing(candidate: &str) -> &str {
    let mut end = candidate.len();
    loop {
        let trimmed = &candidate[..end];
        let Some(last) = trimmed.chars().next_back() else {
            return trimmed;
        };

        let drop_last = if TRAILING_PUNCTUATION.contains(&last) {
            true
        } else if last == ')' {
            trimmed.matches('(').count() < trimmed.matches(')').count()
        } else if last == ']' {
            trimmed.matches('[').count() < trimmed.matches(']').count()
        } else {
            false
        };

        if !drop_last {
            return trimmed;
        }
        end -= last.len_utf8();
    }
}

/// Returns the character range `[start, end)` of the URL covering `char_index`,
/// or `None` when the clicked character is not inside a URL.
pub fn url_span_at(text: &str, char_index: usize) -> Option<(usize, usize)> {
    for candidate in URL_PATTERN.find_iter(text) {
        let matched = trim_trailing(candidate.as_str());
        if matched.is_empty() {
            continue;
        }
        let start_char = text[..candidate.start()].chars().count();
        let end_char = start_char + matched.chars().count();
        if char_index >= start_char && char_index < end_char {
            return Some((start_char, end_char));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::url_span_at;

    fn span_text(line: &str, char_index: usize) -> Option<String> {
        let (start, end) = url_span_at(line, char_index)?;
        Some(line.chars().skip(start).take(end - start).collect())
    }

    #[test]
    fn url_span_covers_scheme_host_and_path_from_any_click_position() {
        let line = "Visit https://ferryx.dev/docs?tab=a#b now";
        let url = "https://ferryx.dev/docs?tab=a#b";
        let start = line.find(url).expect("url present");
        for offset in 0..url.chars().count() {
            assert_eq!(
                span_text(line, start + offset).as_deref(),
                Some(url),
                "click offset {offset} must resolve the whole URL"
            );
        }
    }

    #[test]
    fn url_span_is_none_outside_the_url() {
        let line = "Visit https://ferryx.dev/docs now";
        assert_eq!(span_text(line, 0), None);
        assert_eq!(span_text(line, line.chars().count() - 1), None);
    }

    #[test]
    fn url_span_drops_sentence_punctuation_and_unbalanced_brackets() {
        assert_eq!(
            span_text("see https://ferryx.dev/docs.", 10).as_deref(),
            Some("https://ferryx.dev/docs"),
        );
        assert_eq!(
            span_text("(https://ferryx.dev/docs)", 5).as_deref(),
            Some("https://ferryx.dev/docs"),
        );
        assert_eq!(
            span_text("see https://en.wikipedia.org/wiki/Rust_(game) ok", 10).as_deref(),
            Some("https://en.wikipedia.org/wiki/Rust_(game)"),
        );
    }

    #[test]
    fn url_span_handles_non_http_schemes_and_char_indices_after_wide_text() {
        assert_eq!(
            span_text("run ssh://host.example/repo end", 8).as_deref(),
            Some("ssh://host.example/repo"),
        );
        let line = "한글 https://ferryx.dev/ko end";
        let click = line.chars().position(|c| c == 'f').expect("host char");
        assert_eq!(
            span_text(line, click).as_deref(),
            Some("https://ferryx.dev/ko")
        );
    }
}
