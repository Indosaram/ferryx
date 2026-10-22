//! cursor prompt family: the cursor-agent shell approval card `Run this command?`.
//!
//! Facts (under `.omo-parity/facts/`):
//!
//! * Screen, measured live on cursor-agent 2026.10.19-6bbf2c4 (logged in, default approvals):
//!   `screens/cursor-agent/live-approval.screen` (`cursor-live-screens.md`): the tool row
//!   `$ touch probe.txt Waiting for approval...`, a full-width `─` rule, the command line
//!   `$  touch probe.txt in .`, the question `Run this command?`, the note
//!   `Not in allowlist: touch`, then the option rows `→ Run (once) (y)`,
//!   `Add Shell(touch) to allowlist? (tab)`, `Run Everything (shift+tab)` and
//!   `Skip & tell the agent what to do instead (esc or n)`, with nothing drawn under them.
//! * Keys: each row names its own key in its trailing parentheses. `y` was pressed and ran the
//!   command (measured, `live-postapprove-*.screen`); `tab`, `shift+tab` and `esc or n` are
//!   the keys the card itself prints, answered as printed (`n` for the skip row).

use regex::Regex;
use std::sync::OnceLock;

use super::prompts::{bare_screen_lines, raw_screen_lines, ReferencePromptFamily, ScreenPromptCard};
use super::types::{ReferenceKeyStep, ReferencePrompt, ReferencePromptKind};

/// The registry id this family reads.
pub const REFERENCE_CURSOR_AGENT: &str = "cursor";
/// The launcher name, which the agent registry also accepts for cursor (`ipc/agents.rs`).
pub const REFERENCE_CURSOR_AGENT_ALIAS: &str = "cursor-agent";

const QUESTION: &str = "Run this command?";

fn row_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        Regex::new(r"^(→\s+)?(.+?) \(([^()]+)\)$").expect("the cursor option row pattern is valid")
    })
}

/// Does the family read a card on a pane this agent names?
pub fn cursor_reads_agent(agent: &str) -> bool {
    agent == REFERENCE_CURSOR_AGENT || agent == REFERENCE_CURSOR_AGENT_ALIAS
}

/// The frozen detector shape.
pub fn detect_reference_cursor_prompt(agent: &str, screen: &str) -> Option<ReferencePrompt> {
    cursor_reads_agent(agent).then(|| parse_cursor_prompt(screen)).flatten().map(|card| card.prompt)
}

/// The keys a row's printed key text names, or `None` for text the card has never shown.
fn row_keys(printed: &str) -> Option<Vec<ReferenceKeyStep>> {
    Some(match printed {
        "y" => vec![ReferenceKeyStep::typed("y")],
        "tab" => vec![ReferenceKeyStep::keys(["tab"])],
        "shift+tab" => vec![ReferenceKeyStep::keys(["shift+tab"])],
        "esc or n" => vec![ReferenceKeyStep::typed("n")],
        _ => return None,
    })
}

/// The approval card, while its option rows are still the bottom of the screen.
pub fn parse_cursor_prompt(screen: &str) -> Option<ScreenPromptCard> {
    let lines = bare_screen_lines(screen);
    let question = lines.iter().rposition(|line| line == QUESTION)?;
    // The option rows end the screen; the lines between the question and the first row are notes.
    let mut first_row = lines.len();
    while first_row > question + 1 && row_re().is_match(&lines[first_row - 1]) {
        first_row -= 1;
    }
    let rows: Vec<(bool, String, String)> = lines[first_row..]
        .iter()
        .filter_map(|line| {
            let captures = row_re().captures(line)?;
            Some((captures.get(1).is_some(), captures[2].to_string(), captures[3].to_string()))
        })
        .collect();
    if rows.len() < 2 || rows.iter().filter(|row| row.0).count() != 1 {
        return None;
    }
    if !rows.iter().any(|row| row.2 == "y") || !rows.iter().any(|row| row.2 == "esc or n") {
        return None;
    }
    // The command sits between the rule over the card and the question.
    let raw = raw_screen_lines(screen);
    let question_raw = raw.iter().rposition(|line| line.trim() == QUESTION)?;
    let rule = raw[..question_raw]
        .iter()
        .rposition(|line| !line.trim().is_empty() && line.trim().chars().all(|c| c == '─'))?;
    let mut body = bare_screen_lines(&raw[rule + 1..question_raw].join("\n"));
    if body.is_empty() {
        return None;
    }
    body.extend(lines[question + 1..first_row].iter().cloned());
    Some(ScreenPromptCard::new(
        ReferencePromptFamily::Cursor,
        ReferencePromptKind::Approval,
        QUESTION.to_string(),
        QUESTION.to_string(),
        Some(body.join("\n")),
        rows.iter().map(|row| row.1.clone()).collect(),
        rows.iter().map(|row| row_keys(&row.2)).collect(),
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::reference_chat::prompts::{
        detect_reference_prompt_card_in_session, ReferencePromptSession,
    };
    use crate::remote::reference_chat::types::ReferencePromptAnswer;

    const APPROVAL: &str = include_str!("fixtures/cursor/prompt/approval.screen");
    const IDLE: &str = include_str!("fixtures/cursor/prompt/idle.screen");
    const WORKING: &str = include_str!("fixtures/cursor/prompt/working.screen");
    const RUNNING: &str = include_str!("fixtures/cursor/prompt/running.screen");

    fn pick(index: u32) -> ReferencePromptAnswer {
        ReferencePromptAnswer { option_index: Some(index), ..Default::default() }
    }

    #[test]
    fn cursor_run_command_card_is_an_approval_with_its_printed_keys() {
        let card = parse_cursor_prompt(APPROVAL).expect("the captured card is a prompt");
        assert_eq!(card.prompt.agent, "cursor");
        assert_eq!(card.prompt.kind, ReferencePromptKind::Approval);
        assert_eq!(card.prompt.title, "Run this command?");
        assert_eq!(card.prompt.question, "Run this command?");
        assert_eq!(
            card.prompt.body.as_deref(),
            Some("$  touch probe.txt in .\nNot in allowlist: touch")
        );
        let labels: Vec<&str> = card.prompt.options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Run (once)",
                "Add Shell(touch) to allowlist?",
                "Run Everything",
                "Skip & tell the agent what to do instead",
            ]
        );
        assert_eq!(card.answer_steps(&pick(0)).unwrap(), vec![ReferenceKeyStep::typed("y")]);
        assert_eq!(card.answer_steps(&pick(1)).unwrap(), vec![ReferenceKeyStep::keys(["tab"])]);
        assert_eq!(
            card.answer_steps(&pick(2)).unwrap(),
            vec![ReferenceKeyStep::keys(["shift+tab"])]
        );
        assert_eq!(card.answer_steps(&pick(3)).unwrap(), vec![ReferenceKeyStep::typed("n")]);
        assert!(card.answer_steps(&pick(4)).is_err());
    }

    #[test]
    fn cursor_idle_and_working_screens_are_not_prompts() {
        assert!(parse_cursor_prompt(IDLE).is_none());
        assert!(parse_cursor_prompt(WORKING).is_none());
        assert!(parse_cursor_prompt(RUNNING).is_none());
        assert!(detect_reference_cursor_prompt("copilot", APPROVAL).is_none());
    }

    // Fails if the cursor family is dropped from the chain: the route's submit guard and
    // pending-prompt read only run for an agent with a family.
    #[test]
    fn cursor_card_is_reached_through_the_chain_for_both_registry_names() {
        for agent in ["cursor", "cursor-agent"] {
            assert_eq!(ReferencePromptFamily::of_agent(agent), Some(ReferencePromptFamily::Cursor));
            let card = detect_reference_prompt_card_in_session(
                agent,
                APPROVAL,
                &ReferencePromptSession::default(),
            )
            .unwrap_or_else(|| panic!("{agent}: the captured card is read through the chain"));
            assert_eq!(card.family(), ReferencePromptFamily::Cursor);
            assert!(
                detect_reference_prompt_card_in_session(agent, IDLE, &ReferencePromptSession::default())
                    .is_none()
            );
        }
    }
}
