//! Auto-answer policy. Classifiers only decide whether an already-allowed reading
//! may be answered without the owner. They never widen the source rule.
use serde::Deserialize;

use crate::supervisor::answer_history::irreversible_or_external;
use crate::supervisor::panel_contract::{AnswerDecision, PendingPrompt};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub state_source: String,
    pub kind: Option<String>,
    pub screen_readable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoDecision {
    /// Safe to answer from the provider or a question card.
    Answer { text: String },
    /// A person has to decide. Screen-tier approvals always land here.
    EscalateOwner,
    /// `screenReadable` is false: do not answer and do not guess.
    Hold,
}

/// A suggested answer that runs a destructive command is not auto-answered.
pub fn classifier_allows(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    const DENY: &[&str] = &[
        "rm -rf",
        "curl | sh",
        "curl|sh",
        "wget | sh",
        "wget|sh",
        "mkfs",
        ":(){",
        "dd if=",
        "chmod 777",
    ];
    !DENY.iter().any(|pattern| lower.contains(pattern))
}

/// What the hub does with a prompt it sees in a pane.
///
/// The hub has no answer of its own: `suggested` is only the prompt's question text, and the
/// pane-side answer path types an option index, so answering here would pick option 1 for the
/// owner. Every readable prompt goes to the owner; an unreadable screen is held. `Answer` stays
/// for a caller that has a real answer source (a judge verdict), which none has today.
pub fn auto_answer(reading: &Reading, _suggested: &str) -> AutoDecision {
    if !reading.screen_readable {
        return AutoDecision::Hold;
    }
    AutoDecision::EscalateOwner
}

/// Turns the supervisor's answer decision ([`crate::supervisor::answer_history::decide`]) into
/// what the hub does with the prompt. An irreversible or external step (plan D1a), a permission
/// or approval prompt and an unreadable screen are never answered, whatever `decision` says.
pub fn auto_answer_decided(
    reading: &Reading,
    prompt: &PendingPrompt,
    worktree: Option<&str>,
    decision: &AnswerDecision,
) -> AutoDecision {
    if !reading.screen_readable {
        return AutoDecision::Hold;
    }
    if irreversible_or_external(prompt, worktree).is_some()
        || matches!(reading.kind.as_deref(), Some("permission" | "approval"))
    {
        return AutoDecision::EscalateOwner;
    }
    match decision {
        AnswerDecision::Answer { option_index, .. } => match prompt.options.get(*option_index) {
            Some(text) => AutoDecision::Answer { text: text.clone() },
            None => AutoDecision::EscalateOwner,
        },
        AnswerDecision::Escalate { .. } => AutoDecision::EscalateOwner,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(source: &str, kind: Option<&str>, readable: bool) -> Reading {
        Reading {
            state_source: source.into(),
            kind: kind.map(str::to_owned),
            screen_readable: readable,
        }
    }

    #[test]
    fn auto_answer_never_answers_d1a_items_for_the_owner() {
        assert_eq!(
            auto_answer(&reading("provider", Some("approval"), true), "yes"),
            AutoDecision::EscalateOwner
        );
        assert_eq!(
            auto_answer(
                &reading("screen", Some("question"), true),
                "the first option"
            ),
            AutoDecision::EscalateOwner
        );
        assert_eq!(
            auto_answer(&reading("provider", Some("question"), true), "Which plan?"),
            AutoDecision::EscalateOwner
        );
        assert_eq!(
            auto_answer(&reading("screen", Some("approval"), true), "yes"),
            AutoDecision::EscalateOwner
        );
        assert_eq!(
            auto_answer(&reading("provider", Some("question"), false), "yes"),
            AutoDecision::Hold
        );
        assert_eq!(
            auto_answer(&reading("provider", Some("question"), true), "rm -rf /"),
            AutoDecision::EscalateOwner
        );
        assert_eq!(
            auto_answer(&reading("screen", Some("plan"), true), "ok"),
            AutoDecision::EscalateOwner
        );
    }

    #[test]
    fn decided_answer_is_typed_only_for_routine_questions() {
        let prompt = |question: &str, command: Option<&str>| PendingPrompt {
            prompt_id: "p1".into(),
            kind: "question".into(),
            question: question.into(),
            command: command.map(str::to_owned),
            options: vec!["Allow".into(), "Deny".into()],
            screen_revision: None,
        };
        let answer = AnswerDecision::Answer {
            option_index: 0,
            basis: "You chose Allow the last 3 times".into(),
        };
        let question = reading("provider", Some("question"), true);
        assert_eq!(
            auto_answer_decided(&question, &prompt("Use tabs?", None), None, &answer),
            AutoDecision::Answer {
                text: "Allow".into()
            }
        );
        // D1a items are never auto-answered, even when the decision says Answer.
        for (text, command) in [
            ("Run the migration?", Some("sqlx migrate run --database-url postgres://prod/app")),
            ("Deploy to production?", None),
            ("Run it?", Some("git push --force origin main")),
            ("Run it?", Some("rm -rf /var/lib/app")),
            ("Paste the API token?", None),
        ] {
            assert_eq!(
                auto_answer_decided(&question, &prompt(text, command), None, &answer),
                AutoDecision::EscalateOwner,
                "{text} {command:?}"
            );
        }
        assert_eq!(
            auto_answer_decided(
                &reading("provider", Some("permission"), true),
                &prompt("Use tabs?", None),
                None,
                &answer
            ),
            AutoDecision::EscalateOwner
        );
        assert_eq!(
            auto_answer_decided(
                &reading("provider", Some("question"), false),
                &prompt("Use tabs?", None),
                None,
                &answer
            ),
            AutoDecision::Hold
        );
    }
}
