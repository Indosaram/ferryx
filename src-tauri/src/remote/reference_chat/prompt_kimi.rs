//! kimi prompt family: the approval panel of kimi-cli's shell UI.
//!
//! No live capture: the parity run never reached a prompt (`.omo-parity/facts/local-2.md:241-246`,
//! authorization expired). Every string below is read from the installed kimi-cli 1.12.0 package
//! on this Mac (`~/.local/share/uv/tools/kimi-cli/lib/python3.13/site-packages/kimi_cli/`):
//!
//! * `ui/shell/visualize.py:266-270` - the options `Approve once`, `Approve for this session`,
//!   `Reject, tell Kimi what to do instead`, `selected_index = 0`.
//! * `ui/shell/visualize.py:329-360` - the header `⚠ {sender} is requesting approval to {action}:`,
//!   the content blocks, then each option as `→ <label>` (selected) or `  <label>`.
//! * `ui/shell/visualize.py:541-543` - the panel is followed only by the status block.
//! * `ui/shell/visualize.py:609-617` and `ui/shell/keyboard.py:176-177,245-246` - `\x1b[A`/`\x1b[B`
//!   move the selection, Enter (`\r`) resolves the selected option.
//! * `tools/shell/__init__.py:34,56-58` - the shell tool asks as sender `Shell`, action `run command`.
//!
//! The test screen is rendered from those strings, not captured; the panel's exact padding on a
//! live terminal is unmeasured, so this reader keys on the strings only, not on columns.

use super::prompts::{bare_screen_lines, screen_row_moves, ReferencePromptFamily, ScreenPromptCard};
use super::types::{ReferenceKeyStep, ReferencePrompt, ReferencePromptKind};

/// The registry id this family reads.
pub const REFERENCE_KIMI_AGENT: &str = "kimi";

const OPTIONS: [&str; 3] =
    ["Approve once", "Approve for this session", "Reject, tell Kimi what to do instead"];
const HEADER_MARK: &str = " is requesting approval to ";

/// The frozen detector shape.
pub fn detect_reference_kimi_prompt(agent: &str, screen: &str) -> Option<ReferencePrompt> {
    (agent == REFERENCE_KIMI_AGENT).then(|| parse_kimi_prompt(screen)).flatten().map(|card| card.prompt)
}

/// The approval panel, while only the status block is drawn under its options.
pub fn parse_kimi_prompt(screen: &str) -> Option<ScreenPromptCard> {
    let lines = bare_screen_lines(screen);
    let first = lines.iter().rposition(|line| option_label(line) == Some(OPTIONS[0]))?;
    let rows = lines.get(first..first + OPTIONS.len())?;
    if rows.iter().map(|line| option_label(line)).collect::<Vec<_>>()
        != OPTIONS.iter().map(|label| Some(*label)).collect::<Vec<_>>()
    {
        return None;
    }
    let selected: Vec<usize> =
        rows.iter().enumerate().filter(|(_, line)| line.starts_with('→')).map(|(index, _)| index).collect();
    if selected.len() != 1 {
        return None;
    }
    // Only the status block follows the panel (`compose`, visualize.py:541-543).
    if lines[first + OPTIONS.len()..].iter().any(|line| !line.contains("context: ")) {
        return None;
    }
    let header = lines[..first].iter().rposition(|line| line.contains(HEADER_MARK) && line.ends_with(':'))?;
    let question = lines[header].trim_start_matches('⚠').trim().to_string();
    let body = lines[header + 1..first].join("\n");
    let option_keys = (0..OPTIONS.len())
        .map(|index| {
            let mut steps = screen_row_moves(selected[0], index);
            steps.push(ReferenceKeyStep::keys(["enter"]));
            Some(steps)
        })
        .collect();
    Some(ScreenPromptCard::new(
        ReferencePromptFamily::Kimi,
        ReferencePromptKind::Approval,
        question.clone(),
        question,
        (!body.is_empty()).then_some(body),
        OPTIONS.iter().map(|label| label.to_string()).collect(),
        option_keys,
        None,
    ))
}

fn option_label(line: &str) -> Option<&str> {
    let label = line.trim_start_matches('→').trim();
    OPTIONS.into_iter().find(|option| *option == label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::reference_chat::types::ReferencePromptAnswer;

    // Live kimi captures (`.omo-parity/facts/screens/kimi/`): no approval was reached.
    const IDLE: &str = include_str!("fixtures/kimi/prompt/idle.screen");
    const WORKING: &str = include_str!("fixtures/kimi/prompt/working.screen");

    /// The panel as `_ApprovalRequestPanel.render` lays out its strings (visualize.py:329-360) for
    /// the shell tool (tools/shell/__init__.py:56-58), followed by the status line the live
    /// working capture shows. Rendered from source, not captured.
    fn panel(selected: usize) -> String {
        let mut lines = vec![
            "  ⚠ Shell is requesting approval to run command:".to_string(),
            String::new(),
            "  touch probe.txt".to_string(),
            String::new(),
        ];
        for (index, label) in OPTIONS.iter().enumerate() {
            lines.push(format!("{}{label}", if index == selected { "→ " } else { "  " }));
        }
        lines.push(String::new());
        lines.push("context: 0.0%".to_string());
        lines.join("\n")
    }

    fn pick(index: u32) -> ReferencePromptAnswer {
        ReferencePromptAnswer { option_index: Some(index), ..Default::default() }
    }

    #[test]
    fn kimi_approval_panel_is_an_approval_answered_with_arrows_and_enter() {
        let card = parse_kimi_prompt(&panel(0)).expect("the source-rendered panel is a prompt");
        assert_eq!(card.prompt.agent, "kimi");
        assert_eq!(card.prompt.kind, ReferencePromptKind::Approval);
        assert_eq!(card.prompt.question, "Shell is requesting approval to run command:");
        assert_eq!(card.prompt.body.as_deref(), Some("touch probe.txt"));
        let labels: Vec<&str> = card.prompt.options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, OPTIONS.to_vec());
        assert_eq!(card.answer_steps(&pick(0)).unwrap(), vec![ReferenceKeyStep::keys(["enter"])]);
        assert_eq!(
            card.answer_steps(&pick(2)).unwrap(),
            vec![
                ReferenceKeyStep::keys(["down"]),
                ReferenceKeyStep::keys(["down"]),
                ReferenceKeyStep::keys(["enter"]),
            ]
        );
        let moved = parse_kimi_prompt(&panel(1)).unwrap();
        assert_eq!(moved.prompt.id, card.prompt.id, "the cursor is not part of the id");
        assert_eq!(
            moved.answer_steps(&pick(0)).unwrap(),
            vec![ReferenceKeyStep::keys(["up"]), ReferenceKeyStep::keys(["enter"])]
        );
    }

    #[test]
    fn kimi_idle_working_and_followed_panels_are_not_prompts() {
        assert!(parse_kimi_prompt(IDLE).is_none());
        assert!(parse_kimi_prompt(WORKING).is_none());
        assert!(parse_kimi_prompt(&format!("{}\nlater output", panel(0))).is_none());
        assert!(detect_reference_kimi_prompt("grok", &panel(0)).is_none());
    }
}
