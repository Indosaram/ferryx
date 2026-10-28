//! Routine prompt answers on the owner's behalf.
//!
//! A worker pane sometimes asks a routine semantic question (which of two layouts, which file
//! name). The supervisor may answer it only from a decision that already exists for the goal's
//! current revision: an owner decision or one written into the current contract, naming the
//! exact prompt id and the exact option label. Everything else goes to the owner:
//! - a permission, approval or plan prompt: any permission expansion stays owner-required,
//!   whatever a decision says ([`crate::security::auto_answer`] never answers either);
//! - an unreadable screen: held, never guessed;
//! - a decision of an older goal revision: an owner correction superseded it;
//! - a decision for another prompt id, or a label that is not exactly one of the options;
//! - two current decisions for the same prompt that disagree.
//!
//! The answer carries the prompt id and the option's index and label, so the typed answer can
//! be checked against the prompt it was decided for.
use serde::{Deserialize, Serialize};

/// What kind of prompt the pane shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptKind {
    /// A semantic choice between named options.
    Question,
    /// A tool, command, file or network permission.
    Permission,
    /// An approval of a plan or a step.
    Approval,
}

/// One prompt a worker pane shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerPrompt {
    pub prompt_id: String,
    pub kind: PromptKind,
    /// False when the observer could not read the screen reliably.
    pub screen_readable: bool,
    pub options: Vec<String>,
}

/// Where a recorded decision came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionSource {
    Owner,
    Contract,
}

/// A decision for one prompt, recorded at one goal revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptDecision {
    pub goal_revision: i64,
    pub prompt_id: String,
    pub option_label: String,
    pub source: DecisionSource,
}

/// Why a prompt goes to the owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OwnerReason {
    /// A permission, approval or plan prompt.
    PermissionRequired,
    /// The screen could not be read.
    Unreadable,
    /// No decision of the current revision names this prompt id.
    NoCurrentDecision,
    /// The decided label is not exactly one of the prompt's options.
    LabelNotOffered,
    /// Current decisions for this prompt disagree.
    ConflictingDecisions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "decision")]
pub enum AnswerDecision {
    Answer {
        prompt_id: String,
        option_index: u32,
        option_label: String,
        source: DecisionSource,
    },
    Owner {
        reason: OwnerReason,
    },
}

fn owner(reason: OwnerReason) -> AnswerDecision {
    AnswerDecision::Owner { reason }
}

/// Decides whether `prompt` may be answered from `decisions` at `goal_revision`.
pub fn routine_answer(
    prompt: &WorkerPrompt,
    goal_revision: i64,
    decisions: &[PromptDecision],
) -> AnswerDecision {
    if prompt.kind != PromptKind::Question {
        return owner(OwnerReason::PermissionRequired);
    }
    if !prompt.screen_readable {
        return owner(OwnerReason::Unreadable);
    }
    let current: Vec<&PromptDecision> = decisions
        .iter()
        .filter(|decision| {
            decision.goal_revision == goal_revision && decision.prompt_id == prompt.prompt_id
        })
        .collect();
    let Some(first) = current.first() else {
        return owner(OwnerReason::NoCurrentDecision);
    };
    if current
        .iter()
        .any(|decision| decision.option_label != first.option_label)
    {
        return owner(OwnerReason::ConflictingDecisions);
    }
    let matches: Vec<usize> = prompt
        .options
        .iter()
        .enumerate()
        .filter(|(_, label)| **label == first.option_label)
        .map(|(index, _)| index)
        .collect();
    let [index] = matches.as_slice() else {
        return owner(OwnerReason::LabelNotOffered);
    };
    let Ok(option_index) = u32::try_from(*index) else {
        return owner(OwnerReason::LabelNotOffered);
    };
    // An owner decision outranks a contract decision only by being present; equal labels agree.
    let source = if current
        .iter()
        .any(|decision| decision.source == DecisionSource::Owner)
    {
        DecisionSource::Owner
    } else {
        DecisionSource::Contract
    };
    AnswerDecision::Answer {
        prompt_id: prompt.prompt_id.clone(),
        option_index,
        option_label: first.option_label.clone(),
        source,
    }
}
