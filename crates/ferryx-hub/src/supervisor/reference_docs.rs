//! A read-only retrieval index over the notes and reference files of the owner's agent memory
//! (`~/.omo/memory/agents/*/repo/{notes,reference}/**`). These files are too many to send to
//! owner ingest, so the desktop app's seeding reads them and hands each one over as a
//! [`ReferenceDoc`]: its frontmatter description and keywords drawn from the description, the
//! file path and the first bytes of the body. [`index_reference_docs`] keeps one row per file
//! in `supervisor_reference_docs`, rewritten only when the body hash changes, and
//! [`search_reference`] ranks rows by keyword overlap with a query. Nothing here reads or
//! writes the memory files themselves.
use std::collections::{BTreeSet, HashMap};

use serde::Serialize;
use sqlx::SqlitePool;

use super::SupervisorError;
use crate::storage::now_rfc3339;

/// One indexed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceDoc {
    /// Path relative to the memory root (`agents/<agent>/repo/...`).
    pub path: String,
    pub agent: String,
    pub description: String,
    pub body_sha256: String,
    /// Lowercase tokens, see [`keywords`].
    pub keywords: Vec<String>,
}

/// What one index pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexReport {
    /// Rows inserted or rewritten because the body hash changed.
    pub indexed: usize,
    /// Rows already at this body hash.
    pub unchanged: usize,
}

/// One search hit, best first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceHit {
    pub path: String,
    pub agent: String,
    pub description: String,
    /// Distinct query tokens found in the row's keywords.
    pub score: usize,
}

/// Distinct lowercase tokens of `text`: runs of letters and digits (any script), at least two
/// characters long, in first-seen order.
pub fn keywords(text: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|token| token.chars().count() >= 2)
        .map(str::to_lowercase)
        .filter(|token| seen.insert(token.clone()))
        .collect()
}

/// Inserts every doc whose path is new or whose body hash changed; a doc already indexed at
/// its hash is left alone.
pub async fn index_reference_docs(pool: &SqlitePool, docs: &[ReferenceDoc]) -> Result<IndexReport, SupervisorError> {
    let stored: HashMap<String, String> =
        sqlx::query_as::<_, (String, String)>("SELECT path, body_sha256 FROM supervisor_reference_docs")
            .fetch_all(pool)
            .await?
            .into_iter()
            .collect();
    let mut report = IndexReport::default();
    let now = now_rfc3339();
    for doc in docs {
        if stored.get(&doc.path) == Some(&doc.body_sha256) {
            report.unchanged += 1;
            continue;
        }
        sqlx::query(
            "INSERT INTO supervisor_reference_docs(path, agent, description, body_sha256, keywords, indexed_at)
             VALUES(?,?,?,?,?,?)
             ON CONFLICT(path) DO UPDATE SET
                agent = excluded.agent, description = excluded.description,
                body_sha256 = excluded.body_sha256, keywords = excluded.keywords,
                indexed_at = excluded.indexed_at",
        )
        .bind(&doc.path)
        .bind(&doc.agent)
        .bind(&doc.description)
        .bind(&doc.body_sha256)
        .bind(doc.keywords.join(" "))
        .bind(&now)
        .execute(pool)
        .await?;
        report.indexed += 1;
    }
    Ok(report)
}

/// The `limit` rows sharing the most distinct tokens with `query` (ties by path); rows sharing
/// none are not returned.
pub async fn search_reference(pool: &SqlitePool, query: &str, limit: usize) -> Result<Vec<ReferenceHit>, SupervisorError> {
    let wanted = keywords(query);
    if wanted.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let rows: Vec<(String, String, String, String)> =
        sqlx::query_as("SELECT path, agent, description, keywords FROM supervisor_reference_docs")
            .fetch_all(pool)
            .await?;
    let mut hits: Vec<ReferenceHit> = rows
        .into_iter()
        .filter_map(|(path, agent, description, row_keywords)| {
            let have: BTreeSet<&str> = row_keywords.split(' ').collect();
            let score = wanted.iter().filter(|token| have.contains(token.as_str())).count();
            (score > 0).then_some(ReferenceHit { path, agent, description, score })
        })
        .collect();
    hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    hits.truncate(limit);
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(path: &str, description: &str, body: &str) -> ReferenceDoc {
        ReferenceDoc {
            path: path.to_owned(),
            agent: "a1".to_owned(),
            description: description.to_owned(),
            body_sha256: crate::usage::sha256_hex(body),
            keywords: keywords(&format!("{path} {description} {body}")),
        }
    }

    #[test]
    fn keywords_are_distinct_lowercase_tokens_of_any_script() {
        assert_eq!(keywords("Maho-mac build, BUILD on maho-mac; a 빌드 호스트"), ["maho", "mac", "build", "on", "빌드", "호스트"]);
    }

    #[tokio::test]
    async fn an_unchanged_doc_is_not_rewritten_and_a_changed_one_is() {
        let pool = crate::storage::memory().await.unwrap();
        let first = [doc("agents/a1/repo/notes/x.md", "x", "one"), doc("agents/a1/repo/notes/y.md", "y", "two")];
        assert_eq!(index_reference_docs(&pool, &first).await.unwrap(), IndexReport { indexed: 2, unchanged: 0 });
        assert_eq!(index_reference_docs(&pool, &first).await.unwrap(), IndexReport { indexed: 0, unchanged: 2 });
        let changed = [doc("agents/a1/repo/notes/x.md", "x", "one changed"), first[1].clone()];
        assert_eq!(index_reference_docs(&pool, &changed).await.unwrap(), IndexReport { indexed: 1, unchanged: 1 });
        let stored: String = sqlx::query_scalar("SELECT body_sha256 FROM supervisor_reference_docs WHERE path = ?")
            .bind("agents/a1/repo/notes/x.md")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(stored, crate::usage::sha256_hex("one changed"));
    }

    #[tokio::test]
    async fn search_ranks_by_shared_tokens_and_drops_rows_sharing_none() {
        let pool = crate::storage::memory().await.unwrap();
        let docs = [
            doc("agents/a1/repo/notes/ferryx/build-host.md", "Ferryx builds run on maho-mac", "cargo only on maho-mac"),
            doc("agents/a1/repo/notes/ferryx/mobile.md", "Mobile build notes", "xcode archive"),
            doc("agents/a1/repo/reference/cooking.md", "Recipes", "pasta"),
        ];
        index_reference_docs(&pool, &docs).await.unwrap();
        let hits = search_reference(&pool, "maho-mac build", 10).await.unwrap();
        let paths: Vec<_> = hits.iter().map(|hit| (hit.path.as_str(), hit.score)).collect();
        assert_eq!(
            paths,
            [("agents/a1/repo/notes/ferryx/build-host.md", 3), ("agents/a1/repo/notes/ferryx/mobile.md", 1)]
        );
        assert_eq!(hits[0].description, "Ferryx builds run on maho-mac");
        assert_eq!(search_reference(&pool, "maho-mac build", 1).await.unwrap().len(), 1);
        assert!(search_reference(&pool, "?", 10).await.unwrap().is_empty());
    }
}
