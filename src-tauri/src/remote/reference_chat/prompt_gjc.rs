//! gjc prompt family: the `ask` tool's selector.
//!
//! gjc does not reuse the OmO form reader: its selector is a `│`-barred list between two rules with
//! the hint `↑/↓ select  enter  esc …` under it, not OmO's `Submit (n/m answered)` form
//! (`fixtures/prompt-omo/`).
//!
//! Facts (under `.omo-parity/facts/`):
//!
//! * Screen, measured live on gjc v0.18.12: `screens/gjc/approval-ask.screen` (`remote-gjc.md:55-72`):
//!   the question, a rule, rows `│❯ Red │`, `│  Blue │`, `│  Other (type your own) │`, a rule, and
//!   `↑/↓ select  enter  esc  PgUp/PgDn/Ctrl+u/d: question · Wheel: transcript`. The last row is always
//!   the synthetic `Other (type your own)` (`hook-selector.ts:67`, `ask-contract.ts:63`).
//! * Keys: Down then Enter selected `Blue` (measured, `after-answer.screen`); in the free-text editor
//!   Enter submits (footer `enter submit  esc back to options …`, `hook-selector.ts:716`).
//! * No ordinary tool approval appeared in default config (`remote-gjc.md:57,107`).

use super::prompts::{bare_screen_lines, raw_screen_lines, screen_row_moves, strip_box_bars, ReferencePromptFamily, ScreenPromptCard};
use super::types::{ReferenceKeyStep, ReferencePrompt, ReferencePromptKind};

/// The registry id this family reads.
pub const REFERENCE_GJC_AGENT: &str = "gjc";

const HINT_PREFIX: &str = "↑/↓ select  enter  esc";
const CUSTOM_ROW: &str = "Other (type your own)";

/// The frozen detector shape.
pub fn detect_reference_gjc_prompt(agent: &str, screen: &str) -> Option<ReferencePrompt> {
    (agent == REFERENCE_GJC_AGENT).then(|| parse_gjc_prompt(screen)).flatten().map(|card| card.prompt)
}

/// The ask selector, while its hint is the last line of the screen.
pub fn parse_gjc_prompt(screen: &str) -> Option<ScreenPromptCard> {
    if !bare_screen_lines(screen).last()?.starts_with(HINT_PREFIX) {
        return None;
    }
    let raw = raw_screen_lines(screen);
    let hint = raw.iter().rposition(|line| line.trim_start().starts_with(HINT_PREFIX))?;
    // Walk up over blanks and the rule under the menu, then over the barred rows.
    let mut index = hint;
    while index > 0 && !raw[index - 1].starts_with('│') {
        index -= 1;
        if hint - index > 3 {
            return None;
        }
    }
    let rows_end = index;
    while index > 0 && raw[index - 1].starts_with('│') {
        index -= 1;
    }
    let rows: Vec<String> = raw[index..rows_end].iter().map(|line| strip_box_bars(line)).collect();
    let selected: Vec<usize> =
        rows.iter().enumerate().filter(|(_, row)| row.starts_with('❯')).map(|(at, _)| at).collect();
    let labels: Vec<String> = rows.iter().map(|row| row.trim_start_matches('❯').trim().to_string()).collect();
    if labels.len() < 2 || labels.last().map(String::as_str) != Some(CUSTOM_ROW) || selected.len() != 1 {
        return None;
    }
    let question = raw[..index]
        .iter()
        .rev()
        .map(|line| line.trim())
        .find(|line| !line.is_empty() && !line.chars().all(|c| c == '─'))?
        .to_string();
    let cursor = selected[0];
    let options = &labels[..labels.len() - 1];
    let pick = |row: usize| {
        let mut steps = screen_row_moves(cursor, row);
        steps.push(ReferenceKeyStep::keys(["enter"]));
        steps
    };
    Some(ScreenPromptCard::new(
        ReferencePromptFamily::Gjc,
        ReferencePromptKind::Question,
        "Question".to_string(),
        question,
        None,
        options.to_vec(),
        (0..options.len()).map(|row| Some(pick(row))).collect(),
        Some((pick(options.len()), vec![ReferenceKeyStep::keys(["enter"])])),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::reference_chat::types::ReferencePromptAnswer;

    const ASK: &str = include_str!("fixtures/gjc/prompt/approval-ask.screen");
    const AFTER_ANSWER: &str = include_str!("fixtures/gjc/prompt/after-answer.screen");
    const IDLE: &str = include_str!("fixtures/gjc/prompt/idle.screen");
    const WORKING: &str = include_str!("fixtures/gjc/prompt/working.screen");

    fn pick(index: u32) -> ReferencePromptAnswer {
        ReferencePromptAnswer { option_index: Some(index), ..Default::default() }
    }

    #[test]
    fn gjc_ask_selector_is_a_question_with_a_custom_row() {
        let card = parse_gjc_prompt(ASK).expect("the captured selector is a prompt");
        assert_eq!(card.prompt.agent, "gjc");
        assert_eq!(card.prompt.kind, ReferencePromptKind::Question);
        assert_eq!(card.prompt.question, "Which color should I use?");
        let labels: Vec<&str> = card.prompt.options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, vec!["Red", "Blue"]);
        assert_eq!(card.prompt.custom_option_index, Some(2));
        // Measured: Down then Enter selected Blue.
        assert_eq!(
            card.answer_steps(&pick(1)).unwrap(),
            vec![ReferenceKeyStep::keys(["down"]), ReferenceKeyStep::keys(["enter"])]
        );
        assert_eq!(
            card.answer_steps(&ReferencePromptAnswer { custom_text: Some("green".into()), ..Default::default() })
                .unwrap(),
            vec![
                ReferenceKeyStep::keys(["down"]),
                ReferenceKeyStep::keys(["down"]),
                ReferenceKeyStep::keys(["enter"]),
                ReferenceKeyStep::typed("green"),
                ReferenceKeyStep::keys(["enter"]),
            ]
        );
    }

    #[test]
    fn gjc_idle_working_and_answered_screens_are_not_prompts() {
        assert!(parse_gjc_prompt(IDLE).is_none());
        assert!(parse_gjc_prompt(WORKING).is_none());
        assert!(parse_gjc_prompt(AFTER_ANSWER).is_none());
        assert!(detect_reference_gjc_prompt("omp", ASK).is_none());
    }
}
