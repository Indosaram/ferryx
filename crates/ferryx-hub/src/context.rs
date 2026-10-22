//! Context pack and the tool names the gateway loop exposes.
pub const TOOLS: &[&str] = &[
    "browser",
    "cron",
    "file",
    "mcp",
    "message_context",
    "skills",
    "terminal",
    "web",
];

pub const MAX_MESSAGE_CHARS: usize = 2_000;
pub const MAX_PACK_CHARS: usize = 4_000;
const MAX_LINE_CHARS: usize = 300;

/// Renders the sections a turn's context pack carries: lanes, precedents, and
/// thread lines. Each line is one physical line, capped, and the whole pack stops
/// at `max_chars`.
pub fn render_pack(sections: &[(&str, &[String])], max_chars: usize) -> String {
    let mut out = String::new();
    for (header, lines) in sections {
        push_pack_line(&mut out, header, max_chars);
        if lines.is_empty() {
            push_pack_line(&mut out, "- none", max_chars);
            continue;
        }
        for line in *lines {
            push_pack_line(&mut out, &one_pack_line(line), max_chars);
        }
    }
    out
}

fn one_pack_line(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_LINE_CHARS)
        .collect()
}

fn push_pack_line(out: &mut String, line: &str, max_chars: usize) {
    if out.chars().count() >= max_chars {
        return;
    }
    let room = max_chars.saturating_sub(out.chars().count());
    let text = format!("{line}\n");
    out.extend(text.chars().take(room));
}

/// Caps each message and prefixes untrusted text so it cannot look like a new instruction.
pub fn pack(messages: &[String], memory: &[String], max_chars: usize) -> String {
    let mut out = String::new();
    for item in memory {
        push_capped(&mut out, "memory", item, max_chars);
    }
    for item in messages {
        push_capped(&mut out, "message", item, max_chars);
    }
    out
}

fn push_capped(out: &mut String, kind: &str, text: &str, max_chars: usize) {
    if out.chars().count() >= max_chars {
        return;
    }
    let capped: String = text.chars().take(MAX_MESSAGE_CHARS).collect();
    let line = format!("{kind}: {capped}\n");
    let room = max_chars.saturating_sub(out.chars().count());
    out.extend(line.chars().take(room));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pack_caps_messages_and_names_the_gateway_tools() {
        assert!(TOOLS.contains(&"message_context"));
        let long = "x".repeat(5_000);
        let packed = pack(&[long], &["remember".into()], 100);
        assert!(packed.chars().count() <= 100);
        assert!(packed.contains("memory: remember"));
        let rendered = render_pack(
            &[
                ("Precedents:", &["allow git status".into()]),
                ("Lanes in this thread:", &[]),
            ],
            MAX_PACK_CHARS,
        );
        assert!(rendered.contains("Precedents:\nallow git status\n"));
        assert!(rendered.contains("Lanes in this thread:\n- none\n"));
    }
}
