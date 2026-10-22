//! Markdown to Slack mrkdwn. Fenced and inline code stay verbatim.
use std::sync::LazyLock;

use regex::Regex;

static TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<(?:[@#!][^<>\s|]+(?:\|[^<>]*)?|(?:https?|mailto):[^<>\s|]+(?:\|[^<>]*)?)>")
        .unwrap()
});
static LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]\n]*)\]\(([^)\s]+)\)").unwrap());
static STAR_BOLD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\*\*(\S(?:[^\n]*?\S)?)\*\*").unwrap());
static UNDERSCORE_BOLD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"__(\S(?:[^\n]*?\S)?)__").unwrap());
static ITALIC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(^|[^\w*])\*([^\s*](?:[^*\n]*?[^\s*])?)\*").unwrap());
static STRIKE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"~~(\S(?:[^\n]*?\S)?)~~").unwrap());
static BULLET_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\s*)[-*+]\s+(.*)$").unwrap());
static BROADCAST_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<!(?:here|channel|everyone|subteam\^[^<>|]*)(?:\|[^<>]*)?>").unwrap()
});

pub fn to_mrkdwn(markdown: &str) -> String {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in markdown.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.trim_start().starts_with("```") {
            out.push("```".to_owned());
            fenced = !fenced;
            continue;
        }
        if fenced {
            out.push(line.to_owned());
            continue;
        }
        out.push(convert_line(line));
    }
    if fenced {
        out.push("```".to_owned());
    }
    neutralize(&out.join("\n"))
}

fn convert_line(line: &str) -> String {
    let trimmed = line.trim_start();
    if let Some(marks) = trimmed.split_whitespace().next() {
        if (1..=6).contains(&marks.len()) && marks.chars().all(|character| character == '#') {
            let rest = trimmed[marks.len()..].trim();
            if !rest.is_empty()
                && trimmed
                    .as_bytes()
                    .get(marks.len())
                    .is_some_and(|byte| byte.is_ascii_whitespace())
            {
                let inner = convert_inline(&rest.replace("**", "").replace("__", ""));
                return if inner.is_empty() {
                    String::new()
                } else {
                    format!("*{inner}*")
                };
            }
        }
    }
    if let Some(rest) = trimmed.strip_prefix('>') {
        let indent = &line[..line.len() - trimmed.len()];
        return format!("{indent}> {}", convert_inline(rest.trim_start()));
    }
    if let Some(captures) = BULLET_RE.captures(line) {
        return format!("{}• {}", &captures[1], convert_inline(&captures[2]));
    }
    convert_inline(line)
}

fn convert_inline(text: &str) -> String {
    let (masked, mut store) = stash_code(text);
    let linked = LINK_RE.replace_all(&masked, |captures: &regex::Captures| {
        let label = escape(captures[1].trim());
        let url = captures[2].replace('&', "&amp;");
        let link = if label.is_empty() {
            format!("<{url}>")
        } else {
            format!("<{url}|{label}>")
        };
        stash(&mut store, link)
    });
    let tokened = TOKEN_RE.replace_all(&linked, |captures: &regex::Captures| {
        stash(&mut store, captures[0].to_owned())
    });
    let bold = STAR_BOLD_RE.replace_all(&tokened, "\u{E002}$1\u{E002}");
    let bold = UNDERSCORE_BOLD_RE.replace_all(&bold, "\u{E002}$1\u{E002}");
    let italic = ITALIC_RE.replace_all(&bold, |captures: &regex::Captures| {
        format!("{}_{}_", &captures[1], &captures[2])
    });
    let strike = STRIKE_RE.replace_all(&italic, "~$1~");
    let escaped = escape_outside_code(&strike).replace('\u{E002}', "*");
    unstash(&escaped, &store)
}

fn stash(store: &mut Vec<String>, value: String) -> String {
    store.push(value);
    format!("\u{E000}{}\u{E001}", store.len() - 1)
}

fn stash_code(text: &str) -> (String, Vec<String>) {
    let mut out = String::new();
    let mut store = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('`') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find('`') {
            store.push(format!("`{}`", &after[..end]));
            out.push_str(&format!("\u{E000}{}\u{E001}", store.len() - 1));
            rest = &after[end + 1..];
        } else {
            out.push('`');
            rest = after;
        }
    }
    out.push_str(rest);
    (out, store)
}

fn unstash(text: &str, store: &[String]) -> String {
    let mut out = text.to_owned();
    for (index, value) in store.iter().enumerate() {
        out = out.replace(&format!("\u{E000}{index}\u{E001}"), value);
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_outside_code(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

fn neutralize(text: &str) -> String {
    BROADCAST_RE
        .replace_all(text, |captures: &regex::Captures| {
            captures[0].replace('<', "&lt;").replace('>', "&gt;")
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_becomes_slack_mrkdwn() {
        assert_eq!(to_mrkdwn("**bold** and __also__"), "*bold* and *also*");
        assert_eq!(to_mrkdwn("an *italic* word"), "an _italic_ word");
        assert_eq!(to_mrkdwn("**b** then *i*"), "*b* then _i_");
        assert_eq!(to_mrkdwn("~~gone~~"), "~gone~");
        assert_eq!(to_mrkdwn("2 * 3 * 4"), "2 * 3 * 4");
        assert_eq!(to_mrkdwn("_already_ slack"), "_already_ slack");
        assert_eq!(
            to_mrkdwn("see [docs](https://e.com/a) now"),
            "see <https://e.com/a|docs> now"
        );
        assert_eq!(to_mrkdwn("[](https://e.com)"), "<https://e.com>");
        assert_eq!(to_mrkdwn("# Title"), "*Title*");
        assert_eq!(to_mrkdwn("#hashtag"), "#hashtag");
        assert_eq!(to_mrkdwn("- one\n* two"), "• one\n• two");
        assert_eq!(to_mrkdwn("a < b & c > d"), "a &lt; b &amp; c &gt; d");
        assert_eq!(to_mrkdwn("hey <!here> <@U1>"), "hey &lt;!here&gt; <@U1>");
        assert_eq!(
            to_mrkdwn("use `a **b**` then **ok**"),
            "use `a **b**` then *ok*"
        );
        assert!(to_mrkdwn("```\nopen <x>").ends_with("```"));
    }
}
