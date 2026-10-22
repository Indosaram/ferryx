//! aider prompt family: the `<question> (Y)es/(N)o[…] [Yes]:` confirm line.
//!
//! Facts (under `.omo-parity/facts/`):
//!
//! * Screens, measured live on aider v0.86.2: `screens/aider/approval-run-shell.screen`,
//!   `approval-add-file.screen`, `approval-gitignore-confirm.screen` (`remote-aider.md:54-71`). Every
//!   confirm is `InputOutput.confirm_ask`, printed as
//!   `<question> (Y)es/(N)o[/(A)ll/(S)kip all][/(D)on't ask again] [Yes]: ` on the bottom line, with the
//!   subject (file name or command) on the line above (`aider/io.py#L831-L857`).
//! * Keys (`remote-aider.md:62`, `aider/io.py#L866-L906`): type a prefix of the answer and Enter -
//!   `y\r`, `n\r`, `a\r`, `s\r`, `d\r`.

use regex::Regex;
use std::sync::OnceLock;

use super::prompts::{bare_screen_lines, ReferencePromptFamily, ScreenPromptCard};
use super::types::{ReferenceKeyStep, ReferencePrompt, ReferencePromptKind};

/// The registry id this family reads.
pub const REFERENCE_AIDER_AGENT: &str = "aider";

fn confirm_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        Regex::new(r"^(.*\?) (\(Y\)es/\(N\)o(?:/\(A\)ll)?(?:/\(S\)kip all)?(?:/\(D\)on't ask again)?) \[(?:Yes|No)\]:$")
            .expect("the aider confirm pattern is valid")
    })
}

/// The choices `confirm_ask` prints, each with the prefix that answers it.
const CHOICES: [(&str, &str, &str); 5] = [
    ("(Y)es", "Yes", "y"),
    ("(N)o", "No", "n"),
    ("(A)ll", "All", "a"),
    ("(S)kip all", "Skip all", "s"),
    ("(D)on't ask again", "Don't ask again", "d"),
];

/// The frozen detector shape.
pub fn detect_reference_aider_prompt(agent: &str, screen: &str) -> Option<ReferencePrompt> {
    (agent == REFERENCE_AIDER_AGENT).then(|| parse_aider_prompt(screen)).flatten().map(|card| card.prompt)
}

/// The confirm line, while it is still the bottom line of the screen (an answered one is
/// followed by the answer and later output).
pub fn parse_aider_prompt(screen: &str) -> Option<ScreenPromptCard> {
    let lines = bare_screen_lines(screen);
    let last = lines.last()?;
    let captures = confirm_re().captures(last)?;
    let question = captures[1].to_string();
    let shown: Vec<&str> = captures[2].split('/').collect();
    let mut labels = Vec::new();
    let mut option_keys = Vec::new();
    for (marker, label, key) in CHOICES {
        if shown.contains(&marker) {
            labels.push(label.to_string());
            option_keys.push(Some(vec![ReferenceKeyStep::typed(key), ReferenceKeyStep::keys(["enter"])]));
        }
    }
    let subject = lines.len().checked_sub(2).map(|index| lines[index].clone());
    Some(ScreenPromptCard::new(
        ReferencePromptFamily::Aider,
        ReferencePromptKind::Approval,
        question.clone(),
        question,
        subject,
        labels,
        option_keys,
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::reference_chat::types::ReferencePromptAnswer;

    const RUN_SHELL: &str = include_str!("fixtures/aider/prompt/approval-run-shell.screen");
    const ADD_FILE: &str = include_str!("fixtures/aider/prompt/approval-add-file.screen");
    const GITIGNORE: &str = include_str!("fixtures/aider/prompt/approval-gitignore-confirm.screen");
    const IDLE: &str = include_str!("fixtures/aider/prompt/idle.screen");
    const WORKING: &str = include_str!("fixtures/aider/prompt/working.screen");

    fn pick(index: u32) -> ReferencePromptAnswer {
        ReferencePromptAnswer { option_index: Some(index), ..Default::default() }
    }

    fn labels(card: &ScreenPromptCard) -> Vec<&str> {
        card.prompt.options.iter().map(|o| o.label.as_str()).collect()
    }

    #[test]
    fn aider_confirm_lines_are_approvals_with_their_printed_choices() {
        let shell = parse_aider_prompt(RUN_SHELL).expect("the run-shell confirm is a prompt");
        assert_eq!(shell.prompt.agent, "aider");
        assert_eq!(shell.prompt.kind, ReferencePromptKind::Approval);
        assert_eq!(shell.prompt.question, "Run shell command?");
        assert_eq!(
            shell.prompt.body.as_deref(),
            Some(r#"python3 -c "import calc; print(calc.sub(5,3))""#)
        );
        assert_eq!(labels(&shell), vec!["Yes", "No", "Don't ask again"]);

        let add = parse_aider_prompt(ADD_FILE).expect("the add-file confirm is a prompt");
        assert_eq!(add.prompt.question, "Add file to the chat?");
        assert_eq!(add.prompt.body.as_deref(), Some("calc.py"));

        let gitignore = parse_aider_prompt(GITIGNORE).expect("the gitignore confirm is a prompt");
        assert_eq!(gitignore.prompt.question, "Add .aider* to .gitignore (recommended)?");
        assert_eq!(labels(&gitignore), vec!["Yes", "No"]);
    }

    #[test]
    fn aider_answers_with_the_choice_prefix_then_enter() {
        let card = parse_aider_prompt(RUN_SHELL).unwrap();
        assert_eq!(
            card.answer_steps(&pick(0)).unwrap(),
            vec![ReferenceKeyStep::typed("y"), ReferenceKeyStep::keys(["enter"])]
        );
        assert_eq!(
            card.answer_steps(&pick(2)).unwrap(),
            vec![ReferenceKeyStep::typed("d"), ReferenceKeyStep::keys(["enter"])]
        );
    }

    #[test]
    fn aider_idle_working_and_answered_screens_are_not_prompts() {
        assert!(parse_aider_prompt(IDLE).is_none());
        assert!(parse_aider_prompt(WORKING).is_none());
        // The add-file capture still shows the answered gitignore confirm above: only the live
        // bottom line is a prompt.
        assert_eq!(parse_aider_prompt(ADD_FILE).unwrap().prompt.question, "Add file to the chat?");
        assert!(detect_reference_aider_prompt("grok", RUN_SHELL).is_none());
    }
}
