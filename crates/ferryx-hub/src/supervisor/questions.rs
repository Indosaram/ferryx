//! Durable owner clarification questions for supervisor work submissions.
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::admission::{AdmissionError, SubmitWork};
use super::{ChannelProvider, SupervisorError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuestionOption {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuestionDraft {
    pub work: SubmitWork,
    pub supervisor_id: String,
    pub owner_principal: String,
    pub adapter: ChannelProvider,
    pub conversation_id: String,
    pub channel_id: String,
    pub thread_id: String,
    pub missing: String,
}

pub async fn ask(
    pool: &SqlitePool,
    draft: &QuestionDraft,
    options: &[QuestionOption],
) -> Result<(String, String), AdmissionError> {
    let question_id = uuid::Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now().to_rfc3339();
    let options_json = serde_json::to_string(options)
        .map_err(|error| AdmissionError::Store(SupervisorError::Invalid(error.to_string())))?;
    let draft_json = serde_json::to_string(draft)
        .map_err(|error| AdmissionError::Store(SupervisorError::Invalid(error.to_string())))?;
    let recommended_index: Option<i64> = if options.is_empty() { None } else { Some(1) };
    sqlx::query(
        "INSERT INTO supervisor_questions(question_id, supervisor_id, owner_principal, adapter,
         conversation_id, channel_id, thread_id, missing, options_json, draft_json, recommended_index, status, created_at)
         VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'open', ?)",
    )
    .bind(&question_id)
    .bind(&draft.supervisor_id)
    .bind(&draft.owner_principal)
    .bind(draft.adapter.as_str())
    .bind(&draft.conversation_id)
    .bind(&draft.channel_id)
    .bind(&draft.thread_id)
    .bind(&draft.missing)
    .bind(options_json)
    .bind(draft_json)
    .bind(recommended_index)
    .bind(created_at)
    .execute(pool)
    .await?;
    let message = format!(
        "Question {question_id}: choose the {}:\n{}",
        draft.missing,
        options
            .iter()
            .enumerate()
            .map(|(index, option)| {
                if index == 0 {
                    format!("{}. {} (recommended)", index + 1, option.name)
                } else {
                    format!("{}. {}", index + 1, option.name)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    );
    Ok((question_id, message))
}

pub async fn answer_question(
    pool: &SqlitePool,
    owner_principal: &str,
    question_id: &str,
    answer_text: &str,
) -> Result<Option<(QuestionDraft, String)>, AdmissionError> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT status, draft_json, options_json FROM supervisor_questions
         WHERE question_id = ? AND owner_principal = ?",
    )
    .bind(question_id)
    .bind(owner_principal)
    .fetch_optional(pool)
    .await?;
    let Some((status, draft_json, options_json)) = row else {
        return Ok(None);
    };
    if status != "open" {
        return Ok(None);
    }
    let draft: QuestionDraft = serde_json::from_str(&draft_json)
        .map_err(|error| AdmissionError::Store(SupervisorError::Invalid(error.to_string())))?;
    let options: Vec<QuestionOption> = serde_json::from_str(&options_json)
        .map_err(|error| AdmissionError::Store(SupervisorError::Invalid(error.to_string())))?;
    let answer = answer_text.trim();
    let selected = answer
        .parse::<usize>()
        .ok()
        .and_then(|number| number.checked_sub(1))
        .and_then(|index| options.get(index))
        .or_else(|| options.iter().find(|option| option.name.eq_ignore_ascii_case(answer)))
        .or_else(|| {
            answer
                .split_whitespace()
                .next()
                .and_then(|token| token.trim_end_matches('.').parse::<usize>().ok())
                .and_then(|number| number.checked_sub(1))
                .and_then(|index| options.get(index))
        });
    let Some(selected) = selected else {
        return Ok(None);
    };
    let selected_id = selected.id.clone();
    let mut tx = pool.begin().await?;
    let changed = sqlx::query(
        "UPDATE supervisor_questions SET status = 'answered', answered_at = ?
         WHERE question_id = ? AND owner_principal = ? AND status = 'open'",
    )
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(question_id)
    .bind(owner_principal)
    .execute(&mut *tx)
    .await?;
    if changed.rows_affected() != 1 {
        return Ok(None);
    }
    tx.commit().await?;
    Ok(Some((draft, selected_id)))
}
