//! Durable lane posts. An uncertain send is not retried: the message may already
//! be in the thread, and a second copy would be a second status post.
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::{HubError, Result};
use crate::storage::now_rfc3339;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PostState {
    Pending,
    Sending,
    Sent,
    Uncertain,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Post {
    pub source: String,
    pub thread: Option<String>,
    pub body: String,
    pub nonce: String,
    pub state: PostState,
    pub message_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    Ack(String),
    Ambiguous,
    Rejected(String),
}

/// Pending and sending accept an ack, an ambiguous transport result, or a rejection.
/// Uncertain and sent stay where they are. A rejection before an ack is terminal.
pub fn apply_outcome(post: &mut Post, outcome: SendOutcome) {
    match (&post.state, outcome) {
        (PostState::Pending | PostState::Sending, SendOutcome::Ack(id)) => {
            post.state = PostState::Sent;
            post.message_id = Some(id);
            post.error = None;
        }
        (PostState::Pending | PostState::Sending, SendOutcome::Ambiguous) => {
            post.state = PostState::Uncertain;
        }
        (PostState::Pending | PostState::Sending, SendOutcome::Rejected(error)) => {
            post.state = PostState::Failed;
            post.error = Some(error);
        }
        _ => {}
    }
}

pub async fn enqueue(
    pool: &SqlitePool,
    lane_key: &str,
    thread: Option<&str>,
    source: &str,
    body: &str,
) -> Result<bool> {
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM lane_transcript WHERE lane_key = ? AND role = 'thread_post' AND json_valid(text) AND json_extract(text, '$.source') = ?",
    )
    .bind(lane_key)
    .bind(source)
    .fetch_optional(pool)
    .await?;
    if existing.is_some() {
        return Ok(false);
    }
    let post = Post {
        source: source.to_owned(),
        thread: thread.map(str::to_owned),
        body: body.chars().take(2000).collect(),
        nonce: {
            let compact = uuid::Uuid::new_v4().to_string().replace('-', "");
            compact[..25].to_owned()
        },
        state: PostState::Pending,
        message_id: None,
        error: None,
    };
    let text =
        serde_json::to_string(&post).map_err(|error| HubError::message(error.to_string()))?;
    sqlx::query(
        "INSERT INTO lane_transcript(lane_key, at, role, text) VALUES(?,?, 'thread_post', ?)",
    )
    .bind(lane_key)
    .bind(now_rfc3339())
    .bind(text)
    .execute(pool)
    .await?;
    Ok(true)
}

pub async fn store(pool: &SqlitePool, lane_key: &str, post: &Post) -> Result<()> {
    let text = serde_json::to_string(post).map_err(|error| HubError::message(error.to_string()))?;
    sqlx::query(
        "UPDATE lane_transcript SET text = ? WHERE lane_key = ? AND role = 'thread_post' AND json_valid(text) AND json_extract(text, '$.source') = ?",
    )
    .bind(text)
    .bind(lane_key)
    .bind(&post.source)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_uncertain_send_is_not_retried() {
        let mut post = Post {
            source: "status".into(),
            thread: Some("slack:C0123:1.2".into()),
            body: "working".into(),
            nonce: "n".into(),
            state: PostState::Sending,
            message_id: None,
            error: None,
        };
        apply_outcome(&mut post, SendOutcome::Ambiguous);
        assert_eq!(post.state, PostState::Uncertain);
        apply_outcome(&mut post, SendOutcome::Ack("1".into()));
        assert_eq!(post.state, PostState::Uncertain);
        assert!(post.message_id.is_none());
    }

    #[tokio::test]
    async fn a_status_post_is_stored_once_and_can_be_marked_sent() {
        let pool = crate::storage::memory().await.unwrap();
        assert!(enqueue(
            &pool,
            "gate",
            Some("slack:C0123:1.2"),
            "status",
            "🔄 lane gate: Ship"
        )
        .await
        .unwrap());
        assert!(
            !enqueue(&pool, "gate", Some("slack:C0123:1.2"), "status", "again")
                .await
                .unwrap()
        );
        let raw: String = sqlx::query_scalar(
            "SELECT text FROM lane_transcript WHERE lane_key = 'gate' AND role = 'thread_post'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let mut post: Post = serde_json::from_str(&raw).unwrap();
        apply_outcome(&mut post, SendOutcome::Ack("1700.0002".into()));
        store(&pool, "gate", &post).await.unwrap();
        let stored: String = sqlx::query_scalar(
            "SELECT json_extract(text, '$.state') FROM lane_transcript WHERE lane_key = 'gate'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored, "sent");
    }
}
