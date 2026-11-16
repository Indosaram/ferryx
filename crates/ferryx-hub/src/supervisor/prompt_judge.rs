//! The engine's judgment of a pane prompt (plan task 7, GAP-S7).
//!
//! The decision gate answers what a standing rule, the goal contract or the owner's own past
//! answers support. Everything else used to go straight to the owner. With a judge installed
//! the gate first asks the mhc supervisor engine one question: given what this owner has
//! taught the hub (owner memory, the project's memory, the reference hits for the prompt, the
//! bound goal, the prompt itself and the screen around it), which option would the owner
//! pick? The engine answers with one strict JSON object,
//! `{"optionIndex":n,"confidence":0..1,"reason":"..."}`; the gate answers only a valid index
//! at or above [`MIN_CONFIDENCE`] and otherwise escalates with that index as its
//! recommendation and the engine's reason.
//!
//! Nothing here writes to memory, touches a goal or a dispatch, or types into a pane: the
//! caller ([`crate::supervisor::autonomy::Autonomy::observe`]) still routes the verdict
//! through the irreversible/external and screen-readability guards of [`crate::security`].
use async_trait::async_trait;
use sqlx::SqlitePool;

use super::panel_contract::{FleetSession, PendingPrompt};
use crate::error::{HubError, Result};
use crate::meta::mhc_supervisor::MhcSupervisor;

/// Confidence at or above which the engine's choice is answered without the owner.
pub const MIN_CONFIDENCE: f64 = 0.8;
/// Screen lines given to the engine around the prompt.
pub const SCREEN_LINES: usize = 40;
/// Reference-memory hits given to the engine for the prompt's question.
pub const REFERENCE_HITS: usize = 5;
/// Owner memory rows given to the engine, newest first.
pub const MEMORY_ROWS: i64 = 20;

/// One verdict of the engine: the option it would pick, how sure it is, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct JudgeVerdict {
    pub option_index: usize,
    pub confidence: f64,
    pub reason: String,
}

/// The judge the gate asks. `Err` means the engine failed; the caller escalates, never
/// answers on a failed judgment.
#[async_trait]
pub trait PromptJudge: Send + Sync {
    async fn judge(&self, prompt: &str) -> Result<JudgeVerdict>;
}

/// The goal a session is bound to, as the judgment needs it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BoundGoal {
    pub goal_id: String,
    pub revision: i64,
    /// The owner's admitted request text (the goal's title), empty when unreadable.
    pub title: String,
    /// The goal contract's requirements, in order.
    pub requirements: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct VerdictWire {
    option_index: usize,
    confidence: f64,
    reason: String,
}

/// Parses the engine's verdict. Strict: the whole reply must be one JSON object with exactly
/// these fields, so prose around a verdict, a fenced block or a missing field is a failure
/// (and the caller escalates) instead of a guess at what the engine meant.
pub fn parse_verdict(text: &str) -> Result<JudgeVerdict> {
    let wire: VerdictWire = serde_json::from_str(text.trim())
        .map_err(|error| HubError::message(format!("the judge's verdict is not strict JSON: {error}")))?;
    if !(0.0..=1.0).contains(&wire.confidence) || wire.confidence.is_nan() {
        return Err(HubError::message(
            "the judge's confidence is not between 0 and 1",
        ));
    }
    Ok(JudgeVerdict {
        option_index: wire.option_index,
        confidence: wire.confidence,
        reason: wire.reason.trim().to_owned(),
    })
}

/// The installed mhc supervisor engine as a [`PromptJudge`]: one retained decision turn per
/// prompt, recorded in `supervisor_model_calls` like every other engine call.
pub struct EnginePromptJudge {
    supervisor: MhcSupervisor,
}

impl EnginePromptJudge {
    pub fn new(supervisor: MhcSupervisor) -> Self {
        Self { supervisor }
    }
}

#[async_trait]
impl PromptJudge for EnginePromptJudge {
    async fn judge(&self, prompt: &str) -> Result<JudgeVerdict> {
        let answer = self
            .supervisor
            .decide(prompt)
            .await
            .map_err(|fault| HubError::message(format!("the judge could not decide: {fault}")))?;
        parse_verdict(&answer)
    }
}

/// Owner memory the engine sees: recent taught, corrected and ingested text, newest first.
async fn owner_memory(pool: &SqlitePool) -> Vec<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT body FROM supervisor_memory_sources
          WHERE action IN ('teach','correct','ingest')
          ORDER BY created_at DESC, rowid DESC LIMIT ?",
    )
    .bind(MEMORY_ROWS)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

/// The question the engine is asked, from everything the hub knows about this owner's
/// standing choices and this prompt. Deterministic for the same inputs, so a judgment can be
/// replayed from the action log.
pub async fn judge_prompt(
    pool: &SqlitePool,
    session: &FleetSession,
    prompt: &PendingPrompt,
    goal: Option<&BoundGoal>,
    repo: &str,
) -> String {
    let mut out = String::new();
    out.push_str(
        "You answer one prompt a supervised worker pane is blocked on, on behalf of the owner \
         of this hub, from what the owner has taught. Answer with one JSON object only:\n\
         {\"optionIndex\": <0-based index into the options>, \"confidence\": <0..1>, \
         \"reason\": \"<one short line>\"}\n\
         Answer nothing else. If the prompt is an irreversible or external step, or you are not \
         sure the owner's material supports one option, use a low confidence: the hub then asks \
         the owner instead.\n\n",
    );
    out.push_str("## Pane\n");
    out.push_str(&format!("host: {}\n", session.host_id));
    out.push_str(&format!("agent: {}\n", session.agent));
    out.push_str(&format!("title: {}\n", session.title));
    out.push_str(&format!("cwd: {}\n", session.cwd));
    out.push_str(&format!("repo: {repo}\n"));
    out.push_str(&format!("state: {} ({})\n", session.state, session.state_source));
    if let Some(goal) = goal {
        out.push_str("\n## Goal this session serves\n");
        out.push_str(&format!("goal: {} (revision {})\n", goal.goal_id, goal.revision));
        if !goal.title.is_empty() {
            out.push_str(&format!("request: {}\n", goal.title));
        }
        for requirement in &goal.requirements {
            out.push_str(&format!("requirement: {requirement}\n"));
        }
    }
    out.push_str("\n## Prompt\n");
    out.push_str(&format!("kind: {}\n", prompt.kind));
    out.push_str(&format!("question: {}\n", prompt.question));
    if let Some(command) = &prompt.command {
        out.push_str(&format!("command: {command}\n"));
    }
    for (index, option) in prompt.options.iter().enumerate() {
        out.push_str(&format!("option {index}: {option}\n"));
    }
    let memory = owner_memory(pool).await;
    if !memory.is_empty() {
        out.push_str("\n## What the owner taught you (newest first)\n");
        for line in &memory {
            out.push_str(&format!("- {line}\n"));
        }
    }
    if let Ok(Some(project)) = super::project_memory::agent_memory_block(pool, repo).await {
        out.push_str("\n## This project's decisions\n");
        out.push_str(&project);
        out.push('\n');
    }
    let query = format!("{} {}", prompt.question, session.cwd);
    if let Ok(hits) = super::reference_docs::search_reference(pool, &query, REFERENCE_HITS).await {
        if !hits.is_empty() {
            out.push_str("\n## Reference commands that mention this\n");
            for hit in &hits {
                out.push_str(&format!("- {} ({}): {}\n", hit.path, hit.agent, hit.description));
            }
        }
    }
    out
}

/// The prompt with the last [`SCREEN_LINES`] lines of the pane's screen appended.
pub fn with_screen(mut prompt: String, screen: &str) -> String {
    let lines: Vec<&str> = screen.lines().rev().take(SCREEN_LINES).collect();
    if lines.is_empty() {
        return prompt;
    }
    prompt.push_str("\n## Screen around the prompt (last lines)\n");
    for line in lines.into_iter().rev() {
        prompt.push_str(line);
        prompt.push('\n');
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verdict_is_one_strict_json_object() {
        let verdict = parse_verdict(r#"{"optionIndex":1,"confidence":0.9,"reason":"Bun only"}"#)
            .expect("a strict verdict parses");
        assert_eq!(verdict.option_index, 1);
        assert_eq!(verdict.reason, "Bun only");
        let bare = parse_verdict(r#" {"optionIndex":0,"confidence":1,"reason":""} "#)
            .expect("a bare verdict parses");
        assert_eq!(
            bare,
            JudgeVerdict {
                option_index: 0,
                confidence: 1.0,
                reason: String::new(),
            }
        );
    }

    #[test]
    fn prose_a_fence_or_a_missing_field_is_a_failure_not_a_guess() {
        for reply in [
            "I think option 1.",
            "```json\n{\"optionIndex\":1,\"confidence\":0.9,\"reason\":\"x\"}\n```",
            r#"{"optionIndex":1,"reason":"x"}"#,
            r#"{"optionIndex":1,"confidence":0.9}"#,
            r#"{"optionIndex":1,"confidence":0.9,"reason":"x","extra":1}"#,
            r#"{"optionIndex":1,"confidence":2,"reason":"x"}"#,
            r#"{"optionIndex":1,"confidence":-0.1,"reason":"x"}"#,
            r#"{"optionIndex":"1","confidence":0.9,"reason":"x"}"#,
            "",
            "{}",
        ] {
            assert!(parse_verdict(reply).is_err(), "{reply}");
        }
    }

    #[test]
    fn the_screen_block_keeps_the_last_lines_in_reading_order() {
        let screen = (1..=60).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n");
        let prompt = with_screen("## Prompt\n".to_owned(), &screen);
        assert!(prompt.contains("line 60\n"));
        assert!(prompt.contains("line 21\n"));
        assert!(!prompt.contains("line 20\n"), "{prompt}");
        assert!(prompt.find("line 21\n").unwrap() < prompt.find("line 60\n").unwrap());
        assert_eq!(with_screen("p".to_owned(), ""), "p");
    }
}
