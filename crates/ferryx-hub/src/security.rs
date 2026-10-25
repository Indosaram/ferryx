//! Auto-answer policy. Classifiers only decide whether an already-allowed reading
//! may be answered without the owner. They never widen the source rule.
use serde::Deserialize;

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
    fn auto_answer_never_answers_for_the_owner() {
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
}
