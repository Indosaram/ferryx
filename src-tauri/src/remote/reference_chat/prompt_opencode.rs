//! opencode prompt family: the `△ Permission required` box.
//!
//! Facts (paths under `.omo-parity/facts/` of the agent-parity worktree):
//!
//! * Screen, measured live on opencode 1.18.18: `screens/opencode/approval.screen`,
//!   `lc-approval.screen`, `local1-approval.screen` - the header `△ Permission required`, a
//!   `# <kind>` line, the body, then the button row `Allow once   Allow always   Reject` with the
//!   hint `⇆ select  enter confirm` (`local.md:168`, `local-1.md:217-224`).
//! * Keys: Enter confirms the highlighted button and the first button is highlighted when the
//!   box opens - measured, Enter replied `once` (`local-1.md:226`, plugin event
//!   `permission.replied … "reply":"once"`). `⇆ select` moves between the buttons; the installed
//!   binary binds `left`/`right` to the previous/next permission option and `return` to
//!   select it, starting from `selected:h[0]` (opencode-darwin-arm64 `bin/opencode`, the
//!   `"Previous permission option"`/`"Next permission option"` bindings).

use super::prompts::{bare_screen_lines, ReferencePromptFamily, ScreenPromptCard};
use super::types::{ReferenceKeyStep, ReferencePrompt, ReferencePromptKind};

/// The registry id this family reads.
pub const REFERENCE_OPENCODE_AGENT: &str = "opencode";

const HEADER: &str = "△ Permission required";
const BUTTONS: [&str; 3] = ["Allow once", "Allow always", "Reject"];
const HINT: &str = "enter confirm";

/// The frozen detector shape.
pub fn detect_reference_opencode_prompt(agent: &str, screen: &str) -> Option<ReferencePrompt> {
    (agent == REFERENCE_OPENCODE_AGENT).then(|| parse_opencode_prompt(screen)).flatten().map(|card| card.prompt)
}

/// The permission box, while it is still the bottom of the screen.
pub fn parse_opencode_prompt(screen: &str) -> Option<ScreenPromptCard> {
    let lines = bare_screen_lines(screen);
    let header = lines.iter().rposition(|line| line == HEADER)?;
    let buttons = header + lines[header..].iter().position(|line| is_button_row(line))?;
    // The button row is the box's last line: nothing the agent drew after it.
    if buttons + 1 != lines.len() {
        return None;
    }
    let kind_line = lines.get(header + 1).filter(|line| line.starts_with("# "))?;
    let body: Vec<&str> = lines[header + 2..buttons].iter().map(String::as_str).collect();
    // The first button is highlighted when the box opens: `right` once per button, then Enter.
    let option_keys = (0..BUTTONS.len())
        .map(|index| {
            let mut steps = vec![ReferenceKeyStep::keys(["right"]); index];
            steps.push(ReferenceKeyStep::keys(["enter"]));
            Some(steps)
        })
        .collect();
    Some(ScreenPromptCard::new(
        ReferencePromptFamily::Opencode,
        ReferencePromptKind::Approval,
        HEADER.trim_start_matches('△').trim().to_string(),
        kind_line.trim_start_matches("# ").to_string(),
        (!body.is_empty()).then(|| body.join("\n")),
        BUTTONS.iter().map(|label| label.to_string()).collect(),
        option_keys,
        None,
    ))
}

fn is_button_row(line: &str) -> bool {
    let mut rest = line;
    for label in BUTTONS {
        match rest.find(label) {
            Some(at) => rest = &rest[at + label.len()..],
            None => return false,
        }
    }
    rest.contains(HINT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::reference_chat::types::ReferencePromptAnswer;

    const APPROVAL: &str = include_str!("fixtures/opencode/prompt/approval.screen");
    const APPROVAL_LC: &str = include_str!("fixtures/opencode/prompt/lc-approval.screen");
    const IDLE: &str = include_str!("fixtures/opencode/prompt/idle.screen");
    const WORKING: &str = include_str!("fixtures/opencode/prompt/working.screen");

    fn pick(index: u32) -> ReferencePromptAnswer {
        ReferencePromptAnswer { option_index: Some(index), ..Default::default() }
    }

    #[test]
    fn opencode_permission_box_is_an_approval_with_its_three_buttons() {
        for screen in [APPROVAL, APPROVAL_LC] {
            let card = parse_opencode_prompt(screen).expect("the captured box is a prompt");
            assert_eq!(card.prompt.agent, "opencode");
            assert_eq!(card.prompt.kind, ReferencePromptKind::Approval);
            assert_eq!(card.prompt.title, "Permission required");
            assert_eq!(card.prompt.question, "Shell command");
            let labels: Vec<&str> = card.prompt.options.iter().map(|o| o.label.as_str()).collect();
            assert_eq!(labels, vec!["Allow once", "Allow always", "Reject"]);
            assert_eq!(card.prompt.custom_option_index, None);
        }
        let card = parse_opencode_prompt(APPROVAL).unwrap();
        assert_eq!(card.prompt.body.as_deref(), Some("$ touch created.txt"));
    }

    #[test]
    fn opencode_answers_with_the_measured_enter_and_the_bound_arrows() {
        let card = parse_opencode_prompt(APPROVAL).unwrap();
        assert_eq!(card.answer_steps(&pick(0)).unwrap(), vec![ReferenceKeyStep::keys(["enter"])]);
        assert_eq!(
            card.answer_steps(&pick(2)).unwrap(),
            vec![
                ReferenceKeyStep::keys(["right"]),
                ReferenceKeyStep::keys(["right"]),
                ReferenceKeyStep::keys(["enter"]),
            ]
        );
        assert!(card
            .answer_steps(&ReferencePromptAnswer { custom_text: Some("x".into()), ..Default::default() })
            .is_err());
    }

    #[test]
    fn opencode_idle_and_working_screens_are_not_prompts() {
        assert!(parse_opencode_prompt(IDLE).is_none());
        assert!(parse_opencode_prompt(WORKING).is_none());
        assert!(detect_reference_opencode_prompt("claude", APPROVAL).is_none());
    }
}
