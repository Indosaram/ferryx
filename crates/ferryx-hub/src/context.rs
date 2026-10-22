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
    }
}
