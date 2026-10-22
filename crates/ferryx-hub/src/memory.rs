//! Term-count cosine memory, the same scoring omo uses for a session's memories.
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use serde_json::Value;
use sqlx::SqlitePool;

use crate::error::{HubError, Result};
use crate::storage::now_rfc3339;

const MAX_SEARCH_SCAN_ROWS: i64 = 10_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Memory {
    pub id: String,
    pub session_key: String,
    pub content: String,
    pub metadata: Value,
    pub created_at: String,
    pub updated_at: String,
    pub score: f64,
}

pub struct MemoryStore {
    pool: SqlitePool,
}

impl MemoryStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn remember(
        &self,
        session_key: &str,
        content: impl Into<String>,
        metadata: Value,
    ) -> Result<Memory> {
        let content = content.into();
        let id = uuid::Uuid::new_v4().to_string();
        let metadata_json = serde_json::to_string(&metadata)
            .map_err(|error| HubError::message(error.to_string()))?;
        let now = now_rfc3339();
        sqlx::query(
            "INSERT INTO memories(id, session_key, content, metadata_json, created_at, updated_at) VALUES(?,?,?,?,?,?)",
        )
        .bind(&id)
        .bind(session_key)
        .bind(&content)
        .bind(metadata_json)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.get(&id)
            .await?
            .ok_or_else(|| HubError::message("inserted memory could not be read"))
    }

    pub async fn get(&self, id: &str) -> Result<Option<Memory>> {
        let row: Option<(String, String, String, String, String, String)> = sqlx::query_as(
            "SELECT id, session_key, content, metadata_json, created_at, updated_at FROM memories WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| row_to_memory(row, 0.0)).transpose()
    }

    pub async fn search(
        &self,
        session_key: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        if limit == 0 || query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
            "SELECT id, session_key, content, metadata_json, created_at, updated_at
             FROM memories WHERE session_key = ? ORDER BY updated_at DESC LIMIT ?",
        )
        .bind(session_key)
        .bind(MAX_SEARCH_SCAN_ROWS)
        .fetch_all(&self.pool)
        .await?;
        let query_terms = term_counts(query);
        let query_set: HashSet<_> = query_terms.keys().cloned().collect();
        let query_lower = query.to_lowercase();
        let mut scored = Vec::new();
        for row in rows {
            let terms = term_counts(&row.2);
            let overlap = query_set
                .iter()
                .filter(|term| terms.contains_key(*term))
                .count();
            let cosine = cosine_similarity(&query_terms, &terms);
            let phrase = row.2.to_lowercase().contains(&query_lower);
            let score = cosine + overlap as f64 * 0.25 + if phrase { 1.0 } else { 0.0 };
            if score > 0.0 {
                scored.push((row, score));
            }
        }
        scored.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(Ordering::Equal)
                .then_with(|| right.0 .5.cmp(&left.0 .5))
        });
        let mut memories = Vec::new();
        for (row, score) in scored.into_iter().take(limit) {
            let id = row.0.clone();
            match row_to_memory(row, score) {
                Ok(memory) => memories.push(memory),
                Err(error) => {
                    tracing::warn!(%error, memory = %id, "skipping memory with unreadable metadata");
                }
            }
        }
        Ok(memories)
    }
}

fn row_to_memory(
    row: (String, String, String, String, String, String),
    score: f64,
) -> Result<Memory> {
    let metadata =
        serde_json::from_str(&row.3).map_err(|error| HubError::message(error.to_string()))?;
    Ok(Memory {
        id: row.0,
        session_key: row.1,
        content: row.2,
        metadata,
        created_at: row.4,
        updated_at: row.5,
        score,
    })
}

pub fn term_counts(value: &str) -> HashMap<String, usize> {
    let mut terms = HashMap::new();
    for term in value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
    {
        *terms.entry(term.to_lowercase()).or_default() += 1;
    }
    terms
}

pub fn cosine_similarity(left: &HashMap<String, usize>, right: &HashMap<String, usize>) -> f64 {
    let dot = left
        .iter()
        .map(|(term, count)| *count as f64 * right.get(term).copied().unwrap_or(0) as f64)
        .sum::<f64>();
    let left_norm = left
        .values()
        .map(|count| (*count as f64).powi(2))
        .sum::<f64>()
        .sqrt();
    let right_norm = right
        .values()
        .map(|count| (*count as f64).powi(2))
        .sum::<f64>()
        .sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        0.0
    } else {
        dot / (left_norm * right_norm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn search_ranks_the_closer_memory_and_skips_corrupt_metadata() {
        let pool = crate::storage::memory().await.unwrap();
        let store = MemoryStore::new(pool.clone());
        store
            .remember("s", "the deploy shipped to production", json!({}))
            .await
            .unwrap();
        store
            .remember("s", "unrelated weather note", json!({}))
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO memories(id, session_key, content, metadata_json, created_at, updated_at) VALUES('bad','s','deploy again','{',?,?)",
        )
        .bind(crate::storage::now_rfc3339())
        .bind(crate::storage::now_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
        let found = store.search("s", "deploy shipped", 5).await.unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].content.contains("production"));
        assert!(found[0].score > 0.0);
    }
}
