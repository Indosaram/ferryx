//! grok prompt family: the numbered permission rows `<n> (○|●) <label>`.
//!
//! Facts (under `.omo-parity/facts/`):
//!
//! * Screen, measured live on grok 1.0.67: `screens/grok/approval.screen` (`remote-grok.md:111-125`): a
//!   `┃`-barred block with the tool header (`›  probe`), the command, `← → narrow scope`, rows
//!   `1 (○) Yes, and don't ask again …`, `2 (●) Yes, proceed`, `3 (○) No, reject …`, `4 (○) Never allow: …`,
//!   and the footer `1/4:select  │  Tab:next option  │ … │  Esc:scrollback` as the last line.
//! * Keys (`remote-grok.md:134-143`, source `xai-grok-pager/src/app/agent_view/interactions.rs#L91-L99`):
//!   `1`..`9` select option N directly; measured: `2` approved and the tool ran (`after-approve.screen`).
//!   The fact file says to parse the rows rather than hard-code a row number, which this reader does.

use regex::Regex;
use std::sync::OnceLock;

use super::prompts::{bare_screen_lines, ReferencePromptFamily, ScreenPromptCard};
use super::types::{ReferenceKeyStep, ReferencePrompt, ReferencePromptKind};

/// The registry id this family reads.
pub const REFERENCE_GROK_AGENT: &str = "grok";

fn row_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"^(\d) \(([○●])\) (.+)$").expect("the grok row pattern is valid"))
}

fn footer_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        Regex::new(r"^1/(\d):select\s+│\s+Tab:next option\b").expect("the grok footer pattern is valid")
    })
}

/// The frozen detector shape.
pub fn detect_reference_grok_prompt(agent: &str, screen: &str) -> Option<ReferencePrompt> {
    (agent == REFERENCE_GROK_AGENT).then(|| parse_grok_prompt(screen)).flatten().map(|card| card.prompt)
}

/// The permission rows, while their footer is the last line of the screen.
pub fn parse_grok_prompt(screen: &str) -> Option<ScreenPromptCard> {
    let lines = bare_screen_lines(screen);
    let footer = lines.len().checked_sub(1)?;
    let count: usize = footer_re().captures(&lines[footer])?[1].parse().ok()?;
    let first_row = footer.checked_sub(count)?;
    let mut labels = Vec::new();
    let mut option_keys = Vec::new();
    let mut highlighted = 0;
    for (index, line) in lines[first_row..footer].iter().enumerate() {
        let captures = row_re().captures(line)?;
        if captures[1] != (index + 1).to_string() {
            return None;
        }
        if &captures[2] == "●" {
            highlighted += 1;
        }
        labels.push(captures[3].to_string());
        option_keys.push(Some(vec![ReferenceKeyStep::typed(captures[1].to_string())]));
    }
    if highlighted != 1 {
        return None;
    }
    // The block above the rows: its first line is the tool header, the rest the request.
    let mut start = first_row;
    while start > 0 && !lines[start - 1].starts_with('›') && first_row - start < 8 {
        start -= 1;
    }
    let header = lines
        .get(start.checked_sub(1)?)
        .filter(|line| line.starts_with('›'))?
        .trim_start_matches('›')
        .trim()
        .to_string();
    let body = lines[start..first_row].join("\n");
    Some(ScreenPromptCard::new(
        ReferencePromptFamily::Grok,
        ReferencePromptKind::Approval,
        header.clone(),
        header,
        (!body.is_empty()).then_some(body),
        labels,
        option_keys,
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::reference_chat::types::ReferencePromptAnswer;

    const APPROVAL: &str = include_str!("fixtures/grok/prompt/approval.screen");
    const IDLE: &str = include_str!("fixtures/grok/prompt/idle.screen");
    const WORKING: &str = include_str!("fixtures/grok/prompt/working.screen");

    fn pick(index: u32) -> ReferencePromptAnswer {
        ReferencePromptAnswer { option_index: Some(index), ..Default::default() }
    }

    #[test]
    fn grok_permission_rows_are_an_approval_answered_by_their_digit() {
        let card = parse_grok_prompt(APPROVAL).expect("the captured rows are a prompt");
        assert_eq!(card.prompt.agent, "grok");
        assert_eq!(card.prompt.kind, ReferencePromptKind::Approval);
        assert_eq!(card.prompt.title, "probe");
        assert_eq!(
            card.prompt.body.as_deref(),
            Some("rm -f parity-approval-probe.txt && curl -s https://example.com -o /dev/null\n← → narrow scope")
        );
        let labels: Vec<&str> = card.prompt.options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Yes, and don't ask again for anything (always-approve mode)",
                "Yes, proceed",
                "No, reject (type to add feedback)",
                "Never allow: rm -f",
            ]
        );
        // Measured: `2` approved the call.
        assert_eq!(card.answer_steps(&pick(1)).unwrap(), vec![ReferenceKeyStep::typed("2")]);
        assert_eq!(card.answer_steps(&pick(2)).unwrap(), vec![ReferenceKeyStep::typed("3")]);
    }

    #[test]
    fn grok_idle_and_working_screens_are_not_prompts() {
        assert!(parse_grok_prompt(IDLE).is_none());
        assert!(parse_grok_prompt(WORKING).is_none());
        assert!(detect_reference_grok_prompt("crush", APPROVAL).is_none());
    }
}
