//! omo-gateway's `[[measure ...]]` report-claim markers (`src/report/claims.rs`). The marker
//! is stripped from every Slack reply as omo does; re-measuring the claim is not ported.

pub const ENABLED_ENV: &str = "OMON_REPORT_CLAIMS_ENABLED";

pub fn report_claims_enabled() -> bool {
    !matches!(
        crate::hub_env::var(ENABLED_ENV)
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref(),
        Ok("0" | "false" | "no" | "off")
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedClaim {
    pub sentence: String,
    pub argv: Vec<String>,
    pub expect_regex: String,
}

pub fn strip_and_extract_claims(text: &str) -> (String, Vec<ExtractedClaim>) {
    let mut cleaned = String::with_capacity(text.len());
    let mut claims = Vec::new();
    let mut cursor = 0;

    while let Some(rel_start) = text[cursor..].find("[[measure") {
        let start = cursor + rel_start;
        let before = &text[cursor..start];
        cleaned.push_str(before);

        let after_open = &text[start..];
        let Some(end_rel) = find_marker_end(after_open) else {
            cleaned.push_str(&text[start..start + 2]);
            cursor = start + 2;
            continue;
        };
        let marker_body = &after_open[2..end_rel - 2];
        match parse_measure_marker(marker_body) {
            Some((argv, expect_regex)) => claims.push(ExtractedClaim {
                sentence: extract_preceding_sentence(&cleaned),
                argv,
                expect_regex,
            }),
            None => cleaned.push_str(&text[start..start + end_rel]),
        }
        cursor = start + end_rel;
    }

    if cursor < text.len() {
        cleaned.push_str(&text[cursor..]);
    }

    let normalized = collapse_double_spaces(&cleaned);
    (normalized, claims)
}

fn find_marker_end(s: &str) -> Option<usize> {
    let mut in_quotes = false;
    let mut escaped = false;
    let bytes = s.as_bytes();
    let mut i = 2;
    while i + 1 < bytes.len() {
        let b = bytes[i];
        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        if b == b'\\' && in_quotes {
            escaped = true;
            i += 1;
            continue;
        }
        if b == b'"' {
            in_quotes = !in_quotes;
            i += 1;
            continue;
        }
        if !in_quotes && b == b']' && bytes[i + 1] == b']' {
            return Some(i + 2);
        }
        i += 1;
    }
    None
}

fn parse_measure_marker(body: &str) -> Option<(Vec<String>, String)> {
    let rest = body.trim().strip_prefix("measure")?.trim();
    let bracket_start = rest.find('[')?;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut esc = false;
    let mut bracket_end = None;
    for (idx, ch) in rest[bracket_start..].char_indices() {
        if esc {
            esc = false;
            continue;
        }
        if ch == '\\' && in_str {
            esc = true;
            continue;
        }
        if ch == '"' {
            in_str = !in_str;
            continue;
        }
        if !in_str {
            if ch == '[' {
                depth += 1;
            } else if ch == ']' {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    bracket_end = Some(bracket_start + idx + 1);
                    break;
                }
            }
        }
    }
    let end = bracket_end?;
    let argv_slice = &rest[bracket_start..end];
    let argv: Vec<String> = serde_json::from_str(argv_slice).ok()?;
    if argv.is_empty() {
        return None;
    }
    let after_argv = rest[end..].trim();
    let after_expect = after_argv.strip_prefix("expect~")?.trim();
    let expect_regex: String = if after_expect.starts_with('"') {
        quoted_expect_value(after_expect)?
    } else {
        after_expect.to_string()
    };
    Some((argv, expect_regex))
}

/// A quoted `expect~` value: strict JSON when the model escaped it, otherwise a single properly
/// closed quoted span (`expect~"^\d+$"` is a valid regex but not valid JSON). Anything with text
/// after the closing quote is malformed and yields no claim, so a broken marker is never turned
/// into a garbage regex.
fn quoted_expect_value(raw: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<String>(raw) {
        return Some(value);
    }
    let inner = raw.strip_prefix('"')?;
    let mut chars = inner.char_indices();
    while let Some((index, ch)) = chars.next() {
        match ch {
            '\\' => {
                chars.next()?;
            }
            '"' => return (index + 1 == inner.len()).then(|| inner[..index].to_string()),
            _ => {}
        }
    }
    None
}

fn extract_preceding_sentence(cleaned_so_far: &str) -> String {
    let trimmed = cleaned_so_far.trim_end();
    if trimmed.is_empty() {
        return String::new();
    }
    let last_line = trimmed.lines().last().unwrap_or(trimmed).trim();
    for sep in [". ", "! ", "? ", "다. ", "요. "] {
        if let Some((_, tail)) = last_line.rsplit_once(sep) {
            if !tail.trim().is_empty() {
                return tail.trim().to_string();
            }
        }
    }
    last_line.to_string()
}

fn collapse_double_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, line) in s.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.trim_end());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unclosed_measure_marker_keeps_rest_of_reply() {
        let text = "Result is ready [[measure [\"echo\", \"test\"] expect~\"test\" but forgot closing. And more text.";
        let (cleaned, claims) = strip_and_extract_claims(text);
        assert!(cleaned.contains("And more text."));
        assert!(cleaned.contains("[[measure"));
        assert!(claims.is_empty());

        let unbalanced = "Notice [[measure [\"echo\"] expect~\"say \"hi\"\" extra]] final words.";
        let (cleaned_unbalanced, claims_unbalanced) = strip_and_extract_claims(unbalanced);
        assert!(cleaned_unbalanced.contains("final words."));
        assert!(claims_unbalanced.is_empty());
    }

    #[test]
    fn a_measure_marker_is_stripped_like_omo() {
        let (cleaned, claims) =
            strip_and_extract_claims("CPU is fine. [[measure [\"uptime\"] expect~\"load\"]]\nnext");
        assert_eq!(cleaned, "CPU is fine.\nnext");
        assert_eq!(claims[0].argv, ["uptime"]);
    }
}
