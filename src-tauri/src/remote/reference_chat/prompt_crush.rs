//! crush prompt family: the `Permission Required` dialog.
//!
//! Facts (under `.omo-parity/facts/`):
//!
//! * Screen, measured live on crush v0.98.1: `screens/crush/approval.screen` (`remote-crush.md:48,59,66-73`).
//!   The dialog floats over the chat and the sidebar keeps drawing to its right on the same rows, so
//!   the dialog is read between its own top corners (`╭` … `╮`). Inside: `Permission Required ╱╱╱…`,
//!   `Tool <name>`, `Path <cwd>`, `Desc <description>`, the command, the buttons
//!   `Allow      Allow for Session      Deny` and the help `←/→ choose • enter confirm • esc exit`.
//! * Keys (`remote-crush.md:66-70`, source `internal/ui/dialog/permissions.go:100-160`): `a` = Allow
//!   (measured: `a` approved the call, `after-allow.screen`), `s` = Allow for Session, `d` = Deny.

use super::prompts::{raw_screen_lines, strip_box_bars, ReferencePromptFamily, ScreenPromptCard};
use super::types::{ReferenceKeyStep, ReferencePrompt, ReferencePromptKind};

/// The registry id this family reads.
pub const REFERENCE_CRUSH_AGENT: &str = "crush";

const TITLE: &str = "Permission Required";
const BUTTONS: [(&str, &str); 3] = [("Allow", "a"), ("Allow for Session", "s"), ("Deny", "d")];
const HELP: &str = "←/→ choose • enter confirm • esc exit";

/// The frozen detector shape.
pub fn detect_reference_crush_prompt(agent: &str, screen: &str) -> Option<ReferencePrompt> {
    (agent == REFERENCE_CRUSH_AGENT).then(|| parse_crush_prompt(screen)).flatten().map(|card| card.prompt)
}

/// The permission dialog, read between its own corners.
pub fn parse_crush_prompt(screen: &str) -> Option<ScreenPromptCard> {
    let raw = raw_screen_lines(screen);
    let title_row = raw.iter().rposition(|line| line.contains(TITLE))?;
    let top = raw[..title_row].iter().rposition(|line| line.contains('╭'))?;
    let top_chars: Vec<char> = raw[top].chars().collect();
    let left = top_chars.iter().position(|c| *c == '╭')?;
    let right = left + top_chars[left..].iter().position(|c| *c == '╮')?;
    let mut inside: Vec<String> = Vec::new();
    let mut closed = false;
    for line in &raw[top + 1..] {
        let chars: Vec<char> = line.chars().collect();
        if chars.get(left) == Some(&'╰') {
            closed = true;
            break;
        }
        if chars.get(left) != Some(&'│') || chars.get(right) != Some(&'│') {
            return None;
        }
        let row = strip_box_bars(&chars[left..=right].iter().collect::<String>());
        if !row.is_empty() {
            inside.push(row);
        }
    }
    if !closed || inside.last().map(String::as_str) != Some(HELP) {
        return None;
    }
    let buttons = inside.len().checked_sub(2)?;
    let labels: Vec<&str> = inside[buttons].split("  ").map(str::trim).filter(|s| !s.is_empty()).collect();
    if labels != BUTTONS.iter().map(|(label, _)| *label).collect::<Vec<_>>() {
        return None;
    }
    if !inside.first()?.starts_with(TITLE) {
        return None;
    }
    let body = inside[1..buttons].join("\n");
    Some(ScreenPromptCard::new(
        ReferencePromptFamily::Crush,
        ReferencePromptKind::Approval,
        TITLE.to_string(),
        TITLE.to_string(),
        (!body.is_empty()).then_some(body),
        BUTTONS.iter().map(|(label, _)| label.to_string()).collect(),
        BUTTONS.iter().map(|(_, key)| Some(vec![ReferenceKeyStep::typed(*key)])).collect(),
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::reference_chat::types::ReferencePromptAnswer;

    const APPROVAL: &str = include_str!("fixtures/crush/prompt/approval.screen");
    const IDLE: &str = include_str!("fixtures/crush/prompt/idle.screen");
    const WORKING: &str = include_str!("fixtures/crush/prompt/working.screen");

    fn pick(index: u32) -> ReferencePromptAnswer {
        ReferencePromptAnswer { option_index: Some(index), ..Default::default() }
    }

    #[test]
    fn crush_permission_dialog_is_read_between_its_corners() {
        let card = parse_crush_prompt(APPROVAL).expect("the captured dialog is a prompt");
        assert_eq!(card.prompt.agent, "crush");
        assert_eq!(card.prompt.kind, ReferencePromptKind::Approval);
        assert_eq!(
            card.prompt.body.as_deref(),
            Some("Tool bash\nPath ~/agent-parity-facts/crush/proj\nDesc create probe file\ntouch parity-probe.txt")
        );
        let labels: Vec<&str> = card.prompt.options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, vec!["Allow", "Allow for Session", "Deny"]);
        assert_eq!(card.answer_steps(&pick(0)).unwrap(), vec![ReferenceKeyStep::typed("a")]);
        assert_eq!(card.answer_steps(&pick(1)).unwrap(), vec![ReferenceKeyStep::typed("s")]);
        assert_eq!(card.answer_steps(&pick(2)).unwrap(), vec![ReferenceKeyStep::typed("d")]);
    }

    #[test]
    fn crush_idle_and_working_screens_are_not_prompts() {
        assert!(parse_crush_prompt(IDLE).is_none());
        assert!(parse_crush_prompt(WORKING).is_none());
        assert!(detect_reference_crush_prompt("opencode", APPROVAL).is_none());
    }
}
