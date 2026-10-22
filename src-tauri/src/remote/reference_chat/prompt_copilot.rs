//! copilot prompt family: the boxed `Do you want to run this command?` menu.
//!
//! Facts (under `.omo-parity/facts/`):
//!
//! * Screen, measured live (copilot 0.0.375, default approval mode): `screens/copilot/approval.screen`,
//!   `local3-approval.screen` (`local-3.md:49-66`, `local.md:121`): an outer box holding the intent line, a
//!   nested box with the command, the question, the numbered rows `❯ 1. Yes`, `2. Yes, and approve …`,
//!   `3. No, and tell Copilot what to do differently (Esc)`, and the footer
//!   `Confirm with number keys or ↑↓ keys and Enter, Cancel with Esc`.
//! * Keys (`local-3.md:66`): `1` approves once (verified: the file was created), `2` approves for the
//!   session, `3`/Esc denies. Each row is answered with its own number key, as the footer says.

use regex::Regex;
use std::sync::OnceLock;

use super::prompts::{bare_screen_lines, raw_screen_lines, ReferencePromptFamily, ScreenPromptCard};
use super::types::{ReferenceKeyStep, ReferencePrompt, ReferencePromptKind};

/// The registry id this family reads.
pub const REFERENCE_COPILOT_AGENT: &str = "copilot";

const FOOTER: &str = "Confirm with number keys or ↑↓ keys and Enter, Cancel with Esc";

fn row_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"^(?:❯\s*)?(\d)\. (.+)$").expect("the copilot row pattern is valid"))
}

/// The frozen detector shape.
pub fn detect_reference_copilot_prompt(agent: &str, screen: &str) -> Option<ReferencePrompt> {
    (agent == REFERENCE_COPILOT_AGENT).then(|| parse_copilot_prompt(screen)).flatten().map(|card| card.prompt)
}

/// The approval box, while its footer is still the bottom of the screen.
pub fn parse_copilot_prompt(screen: &str) -> Option<ScreenPromptCard> {
    let lines = bare_screen_lines(screen);
    // The footer is the box's last row: nothing the agent drew after it.
    if lines.last().map(String::as_str) != Some(FOOTER) {
        return None;
    }
    let footer = lines.len() - 1;
    let mut first_row = footer;
    while first_row > 0 && row_re().is_match(&lines[first_row - 1]) {
        first_row -= 1;
    }
    let rows: Vec<(u32, String, bool)> = lines[first_row..footer]
        .iter()
        .filter_map(|line| {
            let captures = row_re().captures(line)?;
            Some((captures[1].parse().ok()?, captures[2].to_string(), line.starts_with('❯')))
        })
        .collect();
    let numbered_in_order = rows.iter().enumerate().all(|(index, row)| row.0 == index as u32 + 1);
    if rows.len() < 2 || !numbered_in_order || rows.iter().filter(|row| row.2).count() != 1 {
        return None;
    }
    let question = lines.get(first_row.checked_sub(1)?).filter(|line| line.ends_with('?'))?.clone();
    // The body is the box's own rows above the question: the outer box starts on the last raw
    // line that opens a box at its left edge (a nested box's top sits behind the outer bar).
    let raw = raw_screen_lines(screen);
    let question_raw = raw.iter().rposition(|line| line.contains(question.as_str()))?;
    let top = raw[..question_raw].iter().rposition(|line| line.trim_start().starts_with('╭'))?;
    let body: Vec<String> = bare_screen_lines(&raw[top + 1..question_raw].join("\n"));
    let option_keys = rows
        .iter()
        .map(|row| Some(vec![ReferenceKeyStep::typed(row.0.to_string())]))
        .collect();
    Some(ScreenPromptCard::new(
        ReferencePromptFamily::Copilot,
        ReferencePromptKind::Approval,
        question.clone(),
        question,
        (!body.is_empty()).then(|| body.join("\n")),
        rows.into_iter().map(|row| row.1).collect(),
        option_keys,
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::reference_chat::types::ReferencePromptAnswer;

    const APPROVAL: &str = include_str!("fixtures/copilot/prompt/approval.screen");
    const APPROVAL_LOCAL3: &str = include_str!("fixtures/copilot/prompt/local3-approval.screen");
    const IDLE: &str = include_str!("fixtures/copilot/prompt/idle.screen");
    const WORKING: &str = include_str!("fixtures/copilot/prompt/working.screen");

    fn pick(index: u32) -> ReferencePromptAnswer {
        ReferencePromptAnswer { option_index: Some(index), ..Default::default() }
    }

    #[test]
    fn copilot_run_command_box_is_an_approval_with_its_numbered_rows() {
        for screen in [APPROVAL, APPROVAL_LOCAL3] {
            let card = parse_copilot_prompt(screen).expect("the captured box is a prompt");
            assert_eq!(card.prompt.agent, "copilot");
            assert_eq!(card.prompt.kind, ReferencePromptKind::Approval);
            assert_eq!(card.prompt.question, "Do you want to run this command?");
            assert_eq!(
                card.prompt.body.as_deref(),
                Some("Create an empty file named probe.txt\ntouch probe.txt")
            );
            let labels: Vec<&str> = card.prompt.options.iter().map(|o| o.label.as_str()).collect();
            assert_eq!(
                labels,
                vec![
                    "Yes",
                    "Yes, and approve `touch` for the rest of the running session",
                    "No, and tell Copilot what to do differently (Esc)",
                ]
            );
        }
    }

    #[test]
    fn copilot_answers_each_row_with_its_number_key() {
        let card = parse_copilot_prompt(APPROVAL).unwrap();
        assert_eq!(card.answer_steps(&pick(0)).unwrap(), vec![ReferenceKeyStep::typed("1")]);
        assert_eq!(card.answer_steps(&pick(2)).unwrap(), vec![ReferenceKeyStep::typed("3")]);
        assert!(card.answer_steps(&pick(3)).is_err());
    }

    #[test]
    fn copilot_idle_and_working_screens_are_not_prompts() {
        assert!(parse_copilot_prompt(IDLE).is_none());
        assert!(parse_copilot_prompt(WORKING).is_none());
        assert!(detect_reference_copilot_prompt("codex", APPROVAL).is_none());
    }
}
