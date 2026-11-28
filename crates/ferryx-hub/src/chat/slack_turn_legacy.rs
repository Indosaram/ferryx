//! Recovery of a legacy thread whose engine session no longer owns a live run.
use super::*;

/// Keeps the old session file for inspection; only its resume pointer is discarded.
pub(super) async fn recover(
    pool: &SqlitePool,
    key: &SessionKey,
    state: &mut SessionState,
) -> Result<Option<String>> {
    let Some(thread) = state.metadata.get("omo_thread_id").and_then(Value::as_str) else {
        return if state.metadata.get("legacy_recap_pending") == Some(&json!(true)) {
            recap(pool, key).await.map(Some)
        } else {
            Ok(None)
        };
    };
    // The hub's own engine is still settling this session's last run (mhc records it active
    // through its post-turn work): the session is owned, and the turn takes that engine over
    // or waits for it to close.
    if omo_client::is_held(thread) {
        return Ok(None);
    }
    let Some(lifecycle) = omo_client::latest_lifecycle(thread).await else {
        return Ok(None);
    };
    if !lifecycle.unsettled() {
        return Ok(None);
    }
    state.metadata.remove("omo_thread_id");
    state.metadata.insert("legacy_recap_pending".into(), json!(true));
    save_state(pool, key, state).await?;
    recap(pool, key).await.map(Some)
}

/// A fresh engine receives the same session's twenty messages before the current one (already
/// recorded, and sent as the prompt itself), oldest first.
pub(super) async fn recap(pool: &SqlitePool, key: &SessionKey) -> Result<String> {
    let messages: Vec<(String, String)> = sqlx::query_as(
        "SELECT role, content FROM
         (SELECT role, content, sequence FROM messages WHERE session_key = ?
          ORDER BY sequence DESC LIMIT 20 OFFSET 1) ORDER BY sequence",
    )
    .bind(key.storage_key())
    .fetch_all(pool)
    .await?;
    let mut recap = String::from("<previous-thread-context>\n");
    for (role, content) in messages {
        recap.push_str(&format!("{role}: {content}\n"));
    }
    recap.push_str("</previous-thread-context>\n");
    Ok(recap)
}
