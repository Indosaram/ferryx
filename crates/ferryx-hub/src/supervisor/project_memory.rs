//! Owner project decisions carried into hooked agent sessions (agent-attention-parity todo 8).
//!
//! Reads SQLite only; the memory engine is never called. An item is one owner `teach` or
//! `correct` provenance row ([`super::memory`]) whose receipt the engine holds live
//! ([`LIVE_RECEIPT_STATES`]), for a project whose
//! repository identity is the one asked about, from an adapter other than `import`, and that no
//! later `correct` or `forget` (other than a failed one) targets. Owner-ingest rows, imported
//! sessions, task-only exceptions, lessons and agent-authored text are never items: none of them
//! is a `teach`/`correct` row from an owner adapter.
//!
//! Injection is off per project until the owner turns it on
//! (`supervisor_project_memory_injection`, migration 0049, keyed by repository identity). A
//! daemon whose hub is not open (a standby, or no hub) serves no block at all; the caller of
//! [`agent_memory_block`] decides that, this module only reads the database it is given.
use serde::Serialize;
use sqlx::SqlitePool;

/// Most items one block carries: the newest ones.
pub const MAX_ITEMS: usize = 8;
/// Longest single item line in a block; a longer one is cut.
pub const MAX_ITEM_CHARS: usize = 400;

/// Receipt states of a successful teach or correction, as `memory.rs` stores them verbatim
/// from the engine: `persisted` once the engine committed the item (what the pinned mhc
/// 2026.11.x answers a teach with), `applied` once a session used it. Its receipt fold
/// (`memory-core` `receipts/owner.rs`, `fold_owner_receipts`) closes a view as `applied` when a
/// session applied it, else `persisted` when committed, else `accepted`; the other states are
/// `forgotten` and `corrected` on a closed receipt. So `pending` (no answer yet), `failed` and
/// the uncommitted `accepted` are never items, and a closed receipt is excluded through the
/// later row that closed it.
pub const LIVE_RECEIPT_STATES: [&str; 2] = ["persisted", "applied"];

/// The owner-taught items of the project with `repo_identity`, oldest first, at most
/// [`MAX_ITEMS`] (the newest ones). Each is the stored body exactly as the owner's request
/// recorded it.
pub async fn project_items(pool: &SqlitePool, repo_identity: &str) -> Result<Vec<String>, sqlx::Error> {
    let mut items: Vec<String> = sqlx::query_scalar(
        "SELECT s.body
           FROM supervisor_memory_sources s
          WHERE s.action IN ('teach', 'correct')
            AND s.state IN ('persisted', 'applied')
            AND s.source_adapter <> 'import'
            AND s.receipt_id IS NOT NULL
            AND s.project_id IN (SELECT p.project_id FROM supervisor_projects p
                                  WHERE p.repo_identity = ? AND p.supervisor_id = s.supervisor_id)
            AND NOT EXISTS (SELECT 1 FROM supervisor_memory_sources later
                             WHERE later.action IN ('correct', 'forget')
                               AND later.target_receipt_id = s.receipt_id
                               AND later.state <> 'failed')
          ORDER BY s.created_at DESC, s.rowid DESC
          LIMIT ?",
    )
    .bind(repo_identity)
    .bind(MAX_ITEMS as i64)
    .fetch_all(pool)
    .await?;
    items.reverse();
    Ok(items)
}

/// Whether the owner turned injection on for `repo_identity`. No row is off.
pub async fn injection_enabled(pool: &SqlitePool, repo_identity: &str) -> Result<bool, sqlx::Error> {
    let enabled: Option<i64> = sqlx::query_scalar(
        "SELECT enabled FROM supervisor_project_memory_injection WHERE repo_identity = ?",
    )
    .bind(repo_identity)
    .fetch_optional(pool)
    .await?;
    Ok(enabled == Some(1))
}

/// Whether any project has injection on: when none has, a caller can skip resolving the
/// session's repository at all.
pub async fn any_injection_enabled(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM supervisor_project_memory_injection WHERE enabled = 1)")
        .fetch_one(pool)
        .await
}

/// Turns injection on or off for `repo_identity`.
pub async fn set_injection(pool: &SqlitePool, repo_identity: &str, enabled: bool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO supervisor_project_memory_injection(repo_identity, enabled, updated_at)
         VALUES (?, ?, ?)
         ON CONFLICT(repo_identity) DO UPDATE SET enabled = excluded.enabled, updated_at = excluded.updated_at",
    )
    .bind(repo_identity)
    .bind(i64::from(enabled))
    .bind(crate::storage::now_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

/// One supervised project and its injection switch, as the setup surface lists them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMemorySwitch {
    pub project_id: String,
    pub name: String,
    pub repo_identity: String,
    pub enabled: bool,
    /// Items a session would receive now (switch aside).
    pub items: usize,
}

/// Every supervised project with its switch, by name.
pub async fn project_switches(pool: &SqlitePool) -> Result<Vec<ProjectMemorySwitch>, sqlx::Error> {
    let rows: Vec<(String, String, String, Option<i64>)> = sqlx::query_as(
        "SELECT p.project_id, p.name, p.repo_identity, i.enabled
           FROM supervisor_projects p
           LEFT JOIN supervisor_project_memory_injection i ON i.repo_identity = p.repo_identity
          ORDER BY p.name, p.repo_identity",
    )
    .fetch_all(pool)
    .await?;
    let mut switches = Vec::with_capacity(rows.len());
    for (project_id, name, repo_identity, enabled) in rows {
        let items = project_items(pool, &repo_identity).await?.len();
        switches.push(ProjectMemorySwitch {
            project_id,
            name,
            repo_identity,
            enabled: enabled == Some(1),
            items,
        });
    }
    Ok(switches)
}

/// The items as one block an agent reads as context, fenced as owner notes. `None` when there
/// is nothing to give.
pub fn memory_block(items: &[String]) -> Option<String> {
    let lines: Vec<String> = items
        .iter()
        .map(|item| item.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .map(|line| line.chars().take(MAX_ITEM_CHARS).collect())
        .collect();
    if lines.is_empty() {
        return None;
    }
    let mut block = String::from(
        "Owner notes for this project (decisions the owner taught Ferryx; follow them unless the owner says otherwise in this session):\n<owner-notes>",
    );
    for line in lines {
        block.push_str("\n- ");
        // Escaped, so no item can open or close a tag around or inside the fence.
        block.push_str(&line.replace('<', "&lt;").replace('>', "&gt;"));
    }
    block.push_str("\n</owner-notes>");
    Some(block)
}

/// The block for a new agent session in the repository `repo_identity`: `None` while the
/// project's switch is off or no item applies.
pub async fn agent_memory_block(pool: &SqlitePool, repo_identity: &str) -> Result<Option<String>, sqlx::Error> {
    if !injection_enabled(pool, repo_identity).await? {
        return Ok(None);
    }
    Ok(memory_block(&project_items(pool, repo_identity).await?))
}

/// Named so `cargo test project_items` selects every test of this module.
#[cfg(test)]
mod project_items_tests {
    use super::*;
    use crate::supervisor::{create_project, ensure_identity};

    const OWNER: &str = "desktop:owner";
    const REPO_X: &str = "github.com/example/x";
    const REPO_Y: &str = "github.com/example/y";

    struct Fixture {
        pool: SqlitePool,
        supervisor_id: String,
        x: String,
        y: String,
    }

    async fn fixture() -> Fixture {
        let pool = crate::storage::memory().await.unwrap();
        let identity = ensure_identity(&pool, OWNER).await.unwrap();
        let x = create_project(&pool, &identity.supervisor_id, "x", REPO_X).await.unwrap();
        let y = create_project(&pool, &identity.supervisor_id, "y", REPO_Y).await.unwrap();
        Fixture {
            pool,
            supervisor_id: identity.supervisor_id,
            x: x.project_id,
            y: y.project_id,
        }
    }

    struct Row<'a> {
        source: &'a str,
        adapter: &'a str,
        action: &'a str,
        project: Option<&'a str>,
        body: &'a str,
        target: Option<&'a str>,
        receipt: Option<&'a str>,
        state: &'a str,
        at: &'a str,
    }

    impl Default for Row<'_> {
        fn default() -> Self {
            Row {
                source: "desktop:t1",
                adapter: "desktop",
                action: "teach",
                project: None,
                body: "",
                target: None,
                receipt: None,
                state: "applied",
                at: "2026-11-11T00:00:00Z",
            }
        }
    }

    async fn insert(fixture: &Fixture, row: Row<'_>) {
        sqlx::query(
            "INSERT INTO supervisor_memory_sources(source_id, supervisor_id, owner_principal,
                source_adapter, source_message_id, action, project_id, body, body_sha256,
                target_receipt_id, receipt_id, state, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(row.source)
        .bind(&fixture.supervisor_id)
        .bind(OWNER)
        .bind(row.adapter)
        .bind(row.source)
        .bind(row.action)
        .bind(row.project)
        .bind(row.body)
        .bind("0".repeat(64))
        .bind(row.target)
        .bind(row.receipt)
        .bind(row.state)
        .bind(row.at)
        .bind(row.at)
        .execute(&fixture.pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn project_items_returns_the_owner_teach_of_this_project() {
        let fixture = fixture().await;
        insert(
            &fixture,
            Row {
                project: Some(&fixture.x),
                body: "In project github.com/example/x: use bun not npm",
                receipt: Some("r1"),
                ..Row::default()
            },
        )
        .await;
        assert_eq!(
            project_items(&fixture.pool, REPO_X).await.unwrap(),
            ["In project github.com/example/x: use bun not npm"]
        );
    }

    #[tokio::test]
    async fn another_project_item_never_appears() {
        let fixture = fixture().await;
        insert(
            &fixture,
            Row {
                source: "desktop:x",
                project: Some(&fixture.x),
                body: "x rule",
                receipt: Some("rx"),
                ..Row::default()
            },
        )
        .await;
        insert(
            &fixture,
            Row {
                source: "desktop:y",
                project: Some(&fixture.y),
                body: "y rule",
                receipt: Some("ry"),
                ..Row::default()
            },
        )
        .await;
        // An unscoped (global) item belongs to no project either.
        insert(
            &fixture,
            Row {
                source: "desktop:g",
                body: "global rule",
                receipt: Some("rg"),
                ..Row::default()
            },
        )
        .await;
        assert_eq!(project_items(&fixture.pool, REPO_X).await.unwrap(), ["x rule"]);
        assert_eq!(project_items(&fixture.pool, REPO_Y).await.unwrap(), ["y rule"]);
        assert!(project_items(&fixture.pool, "github.com/example/z").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_non_owner_adapter_item_never_appears() {
        let fixture = fixture().await;
        insert(
            &fixture,
            Row {
                source: "import:s1",
                adapter: "import",
                project: Some(&fixture.x),
                body: "imported text",
                receipt: Some("ri"),
                ..Row::default()
            },
        )
        .await;
        assert!(project_items(&fixture.pool, REPO_X).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn project_items_serves_every_live_state_and_no_other() {
        let fixture = fixture().await;
        for state in ["accepted", "pending", "failed", "forgotten", "corrected", "ingested", "persisted", "applied"] {
            let source = format!("desktop:{state}");
            let receipt = format!("r-{state}");
            insert(
                &fixture,
                Row {
                    source: &source,
                    project: Some(&fixture.x),
                    body: state,
                    receipt: Some(&receipt),
                    state,
                    ..Row::default()
                },
            )
            .await;
        }
        let mut items = project_items(&fixture.pool, REPO_X).await.unwrap();
        items.sort();
        let mut live: Vec<String> = LIVE_RECEIPT_STATES.iter().map(|state| (*state).to_owned()).collect();
        live.sort();
        assert_eq!(items, live);
    }

    /// The engine stand-in answers a teach and a correction the way the pinned mhc 2026.11.x
    /// does: the new receipt is `persisted` (its commit is what settles it), a correction's
    /// answer carries the replaced receipt as `corrected`. Nothing here writes a state; the
    /// rows under test are exactly what `memory.rs` stores from these answers.
    struct PersistingMemory;

    #[async_trait::async_trait]
    impl crate::meta::mhc_supervisor::OwnerMemory for PersistingMemory {
        async fn memory_request(
            &self,
            name: &str,
            data: serde_json::Value,
        ) -> Result<serde_json::Value, crate::meta::mhc_supervisor::MemoryRequestError> {
            let receipt = format!("rcpt-{}", data["sourceId"].as_str().unwrap_or_default());
            let mut answer = serde_json::json!({"changed": true,
                "receipt": {"receiptId": receipt, "state": "persisted", "applied": []}});
            if name == "memory_correct" {
                answer["corrected"] = data["receiptId"].clone();
            }
            Ok(answer)
        }

        async fn ingest_owner_text(
            &self,
            _source_message_id: &str,
            _text: &str,
        ) -> Result<crate::meta::mhc_supervisor::IngestOutcome, crate::meta::mhc_fault::SupervisorFault> {
            Ok(crate::meta::mhc_supervisor::IngestOutcome::Duplicate)
        }
    }

    #[tokio::test]
    async fn project_items_serves_a_teach_the_engine_persisted() {
        use crate::supervisor::request::{handle, RequestContext};
        let fixture = fixture().await;
        let context = RequestContext {
            principal: OWNER.into(),
            adapter: crate::supervisor::ChannelProvider::Cli,
            channel_binding_id: None,
            control: None,
            memory: Some(std::sync::Arc::new(PersistingMemory) as std::sync::Arc<dyn crate::meta::mhc_supervisor::OwnerMemory>),
        };
        let taught = handle(
            &fixture.pool,
            &context,
            serde_json::json!({"requestId": "t1", "projectId": fixture.x, "action": "teach",
                "payload": {"text": "use npm"}}),
        )
        .await
        .unwrap();
        assert_eq!(taught["memory"]["state"], "persisted");
        assert_eq!(
            project_items(&fixture.pool, REPO_X).await.unwrap(),
            ["In project github.com/example/x: use npm"]
        );
        set_injection(&fixture.pool, REPO_X, true).await.unwrap();
        assert!(agent_memory_block(&fixture.pool, REPO_X).await.unwrap().is_some());
        // A correction through the same path replaces it; the old receipt's row drops out.
        let receipt = taught["memory"]["receiptId"].as_str().unwrap().to_owned();
        handle(
            &fixture.pool,
            &context,
            serde_json::json!({"requestId": "c1", "projectId": fixture.x, "action": "teach",
                "payload": {"text": "use bun", "replaces": receipt}}),
        )
        .await
        .unwrap();
        assert_eq!(
            project_items(&fixture.pool, REPO_X).await.unwrap(),
            ["In project github.com/example/x: use bun"]
        );
        assert!(project_items(&fixture.pool, REPO_Y).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_ingest_row_never_appears() {
        let fixture = fixture().await;
        for (source, state) in [("slack:m1", "ingested"), ("slack:m2", "applied")] {
            insert(
                &fixture,
                Row {
                    source,
                    adapter: "slack",
                    action: "ingest",
                    project: Some(&fixture.x),
                    body: "owner chat text",
                    state,
                    ..Row::default()
                },
            )
            .await;
        }
        // A teach the engine never persisted (pending, failed) is not an item either.
        for (source, state) in [("desktop:p", "pending"), ("desktop:f", "failed")] {
            insert(
                &fixture,
                Row {
                    source,
                    project: Some(&fixture.x),
                    body: "not applied",
                    state,
                    ..Row::default()
                },
            )
            .await;
        }
        assert!(project_items(&fixture.pool, REPO_X).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_corrected_item_shows_only_its_replacement() {
        let fixture = fixture().await;
        insert(
            &fixture,
            Row {
                source: "desktop:t1",
                project: Some(&fixture.x),
                body: "use npm",
                receipt: Some("r1"),
                state: "corrected",
                at: "2026-11-11T00:00:00Z",
                ..Row::default()
            },
        )
        .await;
        insert(
            &fixture,
            Row {
                source: "desktop:c1",
                action: "correct",
                project: Some(&fixture.x),
                body: "use bun",
                target: Some("r1"),
                receipt: Some("r2"),
                at: "2026-11-11T00:01:00Z",
                ..Row::default()
            },
        )
        .await;
        assert_eq!(project_items(&fixture.pool, REPO_X).await.unwrap(), ["use bun"]);
        // Even a live teach row drops out once a correction targets it.
        insert(
            &fixture,
            Row {
                source: "desktop:t3",
                project: Some(&fixture.x),
                body: "deploy on fridays",
                receipt: Some("r3"),
                at: "2026-11-11T00:02:00Z",
                ..Row::default()
            },
        )
        .await;
        insert(
            &fixture,
            Row {
                source: "desktop:f3",
                action: "forget",
                body: "",
                target: Some("r3"),
                receipt: Some("r3"),
                state: "forgotten",
                at: "2026-11-11T00:03:00Z",
                ..Row::default()
            },
        )
        .await;
        assert_eq!(project_items(&fixture.pool, REPO_X).await.unwrap(), ["use bun"]);
    }

    #[tokio::test]
    async fn project_items_keeps_the_newest_eight_oldest_first() {
        let fixture = fixture().await;
        for n in 0..10 {
            let source = format!("desktop:t{n}");
            let body = format!("rule {n}");
            let receipt = format!("r{n}");
            let at = format!("2026-11-11T00:{n:02}:00Z");
            insert(
                &fixture,
                Row {
                    source: &source,
                    project: Some(&fixture.x),
                    body: &body,
                    receipt: Some(&receipt),
                    at: &at,
                    ..Row::default()
                },
            )
            .await;
        }
        let items = project_items(&fixture.pool, REPO_X).await.unwrap();
        assert_eq!(items.len(), MAX_ITEMS);
        assert_eq!(items.first().map(String::as_str), Some("rule 2"));
        assert_eq!(items.last().map(String::as_str), Some("rule 9"));
    }

    #[tokio::test]
    async fn project_items_switch_is_off_by_default_and_gives_no_block() {
        let fixture = fixture().await;
        insert(
            &fixture,
            Row {
                project: Some(&fixture.x),
                body: "use bun not npm",
                receipt: Some("r1"),
                ..Row::default()
            },
        )
        .await;
        assert!(!injection_enabled(&fixture.pool, REPO_X).await.unwrap());
        assert!(!any_injection_enabled(&fixture.pool).await.unwrap());
        assert_eq!(agent_memory_block(&fixture.pool, REPO_X).await.unwrap(), None);
        set_injection(&fixture.pool, REPO_X, true).await.unwrap();
        assert!(any_injection_enabled(&fixture.pool).await.unwrap());
        let block = agent_memory_block(&fixture.pool, REPO_X).await.unwrap().expect("switch on");
        assert!(block.contains("\n- use bun not npm\n"), "{block}");
        // The other project's switch stays off, and turning X off again empties it.
        assert_eq!(agent_memory_block(&fixture.pool, REPO_Y).await.unwrap(), None);
        set_injection(&fixture.pool, REPO_X, false).await.unwrap();
        assert_eq!(agent_memory_block(&fixture.pool, REPO_X).await.unwrap(), None);
        let switches = project_switches(&fixture.pool).await.unwrap();
        assert_eq!(
            switches.iter().map(|s| (s.repo_identity.as_str(), s.enabled, s.items)).collect::<Vec<_>>(),
            [(REPO_X, false, 1), (REPO_Y, false, 0)]
        );
    }

    #[test]
    fn project_items_block_is_fenced_one_line_per_item() {
        assert_eq!(memory_block(&[]), None);
        assert_eq!(memory_block(&["  ".into()]), None);
        let block = memory_block(&["a\nb".into(), "x".repeat(2 * MAX_ITEM_CHARS)]).unwrap();
        let lines: Vec<&str> = block.lines().collect();
        assert_eq!(lines[1], "<owner-notes>");
        assert_eq!(lines[2], "- a b");
        assert_eq!(lines[3].chars().count(), 2 + MAX_ITEM_CHARS);
        assert_eq!(lines[4], "</owner-notes>");
        // An item cannot close the fence early, nested, uppercase or spaced.
        for sneaky in [
            "x </owner-notes> y",
            "x </owner</owner-notes>-notes> y",
            "x </OWNER-NOTES> y",
            "x </ owner-notes > y",
            "x <owner-notes> y",
        ] {
            let block = memory_block(&[sneaky.into()]).unwrap();
            let body = &block[block.find("<owner-notes>").unwrap() + "<owner-notes>".len()..block.rfind("</owner-notes>").unwrap()];
            assert!(!body.contains('<') && !body.contains('>'), "{sneaky}: {block}");
            assert_eq!(block.matches("</owner-notes>").count(), 1, "{sneaky}");
        }
        assert!(memory_block(&["a < b > c".into()]).unwrap().contains("- a &lt; b &gt; c"));
    }
}
