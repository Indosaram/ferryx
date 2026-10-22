//! cline prompt family: the `Cline needs permission` / `Approve tool call?` prompt.
//!
//! Facts (under `.omo-parity/facts/`):
//!
//! * Screen, measured live on cline 3.0.70 with auto-approve off: `screens/cline/local3-approval.screen`
//!   (`local-3.md:255-269`): `Cline needs permission`, `Approve tool call?`, the tool name, the
//!   command, then `[y] Approve   [n] Deny`, under which only the footer (model line, directory line,
//!   `Auto-approve all disabled (Shift+Tab)`) is drawn.
//! * Keys (`local-3.md:269`): `y` approves (verified twice), `n` denies.

use super::prompts::{bare_screen_lines, ReferencePromptFamily, ScreenPromptCard};
use super::types::{ReferenceKeyStep, ReferencePrompt, ReferencePromptKind};

/// The registry id this family reads.
pub const REFERENCE_CLINE_AGENT: &str = "cline";

const TITLE: &str = "Cline needs permission";
const QUESTION: &str = "Approve tool call?";
const FOOTER_PREFIX: &str = "Auto-approve all ";
/// The footer is three lines: model, directory, auto-approve state (`local-3.md:246`).
const FOOTER_LINES: usize = 3;

/// The frozen detector shape.
pub fn detect_reference_cline_prompt(agent: &str, screen: &str) -> Option<ReferencePrompt> {
    (agent == REFERENCE_CLINE_AGENT).then(|| parse_cline_prompt(screen)).flatten().map(|card| card.prompt)
}

/// The permission prompt, while only the footer is drawn under its buttons.
pub fn parse_cline_prompt(screen: &str) -> Option<ScreenPromptCard> {
    let lines = bare_screen_lines(screen);
    let title = lines.iter().rposition(|line| line == TITLE)?;
    if lines.get(title + 1).map(String::as_str) != Some(QUESTION) {
        return None;
    }
    let buttons = title + lines[title..].iter().position(|line| is_button_row(line))?;
    if buttons + 1 + FOOTER_LINES != lines.len() || !lines[lines.len() - 1].contains(FOOTER_PREFIX) {
        return None;
    }
    let body = lines[title + 2..buttons].join("\n");
    Some(ScreenPromptCard::new(
        ReferencePromptFamily::Cline,
        ReferencePromptKind::Approval,
        TITLE.to_string(),
        QUESTION.to_string(),
        (!body.is_empty()).then_some(body),
        vec!["Approve".to_string(), "Deny".to_string()],
        vec![Some(vec![ReferenceKeyStep::typed("y")]), Some(vec![ReferenceKeyStep::typed("n")])],
        None,
    ))
}

fn is_button_row(line: &str) -> bool {
    line.split_whitespace().collect::<Vec<_>>() == ["[y]", "Approve", "[n]", "Deny"]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::reference_chat::types::ReferencePromptAnswer;

    const APPROVAL: &str = include_str!("fixtures/cline/prompt/approval.screen");
    const IDLE: &str = include_str!("fixtures/cline/prompt/idle.screen");
    const WORKING: &str = include_str!("fixtures/cline/prompt/working.screen");

    fn pick(index: u32) -> ReferencePromptAnswer {
        ReferencePromptAnswer { option_index: Some(index), ..Default::default() }
    }

    #[test]
    fn cline_permission_prompt_is_an_approval_answered_with_y_or_n() {
        let card = parse_cline_prompt(APPROVAL).expect("the captured prompt is a card");
        assert_eq!(card.prompt.agent, "cline");
        assert_eq!(card.prompt.kind, ReferencePromptKind::Approval);
        assert_eq!(card.prompt.title, "Cline needs permission");
        assert_eq!(card.prompt.question, "Approve tool call?");
        assert_eq!(card.prompt.body.as_deref(), Some("run_commands\n$ touch probe.txt"));
        let labels: Vec<&str> = card.prompt.options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, vec!["Approve", "Deny"]);
        assert_eq!(card.answer_steps(&pick(0)).unwrap(), vec![ReferenceKeyStep::typed("y")]);
        assert_eq!(card.answer_steps(&pick(1)).unwrap(), vec![ReferenceKeyStep::typed("n")]);
    }

    #[test]
    fn cline_idle_and_working_screens_are_not_prompts() {
        assert!(parse_cline_prompt(IDLE).is_none());
        assert!(parse_cline_prompt(WORKING).is_none());
        assert!(detect_reference_cline_prompt("copilot", APPROVAL).is_none());
    }
}
