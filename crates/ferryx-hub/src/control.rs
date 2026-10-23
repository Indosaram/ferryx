//! Hub control socket. `stop` drops chat, cron, and HTTP tasks. The daemon and its
//! PTYs stay up. `start` is the signal to bind them again. `status` reports what is live.
use serde_json::{json, Value};

use crate::error::{HubError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlOp {
    Stop,
    Start,
    Status,
}

pub fn parse_control(line: &str) -> Result<ControlOp> {
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|error| HubError::message(format!("hub control: {error}")))?;
    match value.get("op").and_then(Value::as_str) {
        Some("stop") => Ok(ControlOp::Stop),
        Some("start") => Ok(ControlOp::Start),
        Some("status") => Ok(ControlOp::Status),
        _ => Err(HubError::message(
            "hub control expected stop, start, or status",
        )),
    }
}

pub fn control_socket(home: &std::path::Path) -> std::path::PathBuf {
    home.join(".ferryx").join("hub").join("control.sock")
}

/// `hub stop` writes `hub_stopped = 1` and `hub start` writes `hub_stopped = 0`.
/// A hub database with neither is a fresh install. It stays stopped too, because the
/// hub reads omo's own `~/.omon/secrets` Slack tokens and would otherwise open a second
/// Socket Mode connection next to a running omo-gateway. Only an explicit `hub start`
/// (the cutover script sends one after omo is unloaded) or `FERRYX_HUB=on` opens it.
pub async fn mark_stopped(pool: &sqlx::SqlitePool) -> Result<()> {
    sqlx::query(
        "INSERT INTO hub_meta(key, value) VALUES('hub_stopped', '1')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clear_stopped(pool: &sqlx::SqlitePool) -> Result<()> {
    sqlx::query(
        "INSERT INTO hub_meta(key, value) VALUES('hub_stopped', '0')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn is_stopped(pool: &sqlx::SqlitePool) -> Result<bool> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = 'hub_stopped'")
            .fetch_optional(pool)
            .await?;
    Ok(value.as_deref() != Some("0"))
}

pub fn control_line(op: ControlOp) -> &'static str {
    match op {
        ControlOp::Stop => "{\"op\":\"stop\"}\n",
        ControlOp::Start => "{\"op\":\"start\"}\n",
        ControlOp::Status => "{\"op\":\"status\"}\n",
    }
}

/// What the daemon knows about its own hub tasks. The control socket fills it in.
#[derive(Debug, Clone, Default)]
pub struct LiveHub {
    /// A generation of chat, cron, and HTTP tasks is spawned.
    pub running: bool,
    pub http_addr: String,
    /// The HTTP listener is bound right now.
    pub http_bound: bool,
    /// The daemon was built with Slack and a Slack app token is present.
    pub slack_configured: bool,
    pub slack: Option<crate::chat::slack::SocketModeStatus>,
    pub discord_configured: bool,
    pub version: String,
}

/// The `status` reply. Counts and states only: no tokens, message text, or watcher stderr.
/// `healthy` is false when the hub is stopped, HTTP is not bound, a configured Slack app has
/// no open socket, or an inbox watcher's last run failed; each reason is in `problems`.
pub async fn status_report(pool: &sqlx::SqlitePool, live: &LiveHub) -> Value {
    let mut problems = Vec::new();
    let stopped_flag = is_stopped(pool).await.unwrap_or(true);
    if !live.running {
        problems.push("hub tasks are not running".to_owned());
    }
    if live.running && !live.http_bound {
        problems.push(format!("http {} is not bound", live.http_addr));
    }
    let slack_connected = live.slack.as_ref().is_some_and(|slack| slack.connected);
    if live.running && live.slack_configured && !slack_connected {
        problems.push("slack socket mode is not connected".to_owned());
    }
    let (jobs, enabled): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(CASE WHEN enabled = 1 THEN 1 ELSE 0 END), 0) FROM scheduled_jobs",
    )
    .fetch_one(pool)
    .await
    .unwrap_or((0, 0));
    let watchers: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT source, status, updated_at FROM inbox_watcher_health ORDER BY source",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for (source, status, _) in &watchers {
        if live.running && matches!(status.as_str(), "failed" | "auth_failed") {
            problems.push(format!("inbox watcher {source} {status}"));
        }
    }
    let notify = crate::sessions::watch::effective_notify_mode(pool)
        .await
        .map(|mode| mode.as_str())
        .unwrap_or("actionable");
    json!({
        "status": "ok",
        "op": "status",
        "version": live.version,
        "running": live.running,
        "stopped_flag": stopped_flag,
        "healthy": problems.is_empty(),
        "problems": problems,
        "http": {"addr": live.http_addr, "bound": live.http_bound},
        "slack": {
            "configured": live.slack_configured,
            "running": live.slack.as_ref().is_some_and(|slack| slack.running),
            "connected": slack_connected,
            "connects": live.slack.as_ref().map_or(0, |slack| slack.connects),
            "last_error": live.slack.as_ref().and_then(|slack| slack.last_error.clone()),
        },
        "discord": {"configured": live.discord_configured},
        "cron": {"jobs": jobs, "enabled": enabled},
        "notify_mode": notify,
        "inbox_watchers": watchers
            .iter()
            .map(|(source, status, updated_at)| json!({"source": source, "status": status, "updated_at": updated_at}))
            .collect::<Vec<_>>(),
    })
}

/// Whether a control reply is `"status":"ok"`. A failed stop or start must not look like one.
pub fn reply_ok(reply: &str) -> bool {
    serde_json::from_str::<Value>(reply.trim())
        .ok()
        .and_then(|value| {
            value
                .get("status")
                .and_then(Value::as_str)
                .map(|s| s == "ok")
        })
        .unwrap_or(false)
}

/// Whether a `status` reply says the hub is healthy.
pub fn reply_healthy(reply: &str) -> bool {
    reply_ok(reply)
        && serde_json::from_str::<Value>(reply.trim())
            .ok()
            .and_then(|value| value.get("healthy").and_then(Value::as_bool))
            .unwrap_or(false)
}

/// Writes one control line and reads the reply. The daemon process is not signaled.
#[cfg(unix)]
pub async fn send_control(path: &std::path::Path, op: ControlOp) -> Result<String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let mut stream = tokio::net::UnixStream::connect(path)
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    stream
        .write_all(control_line(op).as_bytes())
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    let mut reader = tokio::io::BufReader::new(stream);
    let mut response = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        reader.read_line(&mut response),
    )
    .await
    .map_err(|_| HubError::message("hub control timed out"))?
    .map_err(|error| HubError::message(error.to_string()))?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_start_and_status_are_the_only_ops() {
        assert_eq!(
            parse_control("{\"op\":\"stop\"}\n").unwrap(),
            ControlOp::Stop
        );
        assert_eq!(
            parse_control("{\"op\":\"start\"}").unwrap(),
            ControlOp::Start
        );
        assert_eq!(
            parse_control("{\"op\":\"status\"}").unwrap(),
            ControlOp::Status
        );
        assert_eq!(
            parse_control(control_line(ControlOp::Status)).unwrap(),
            ControlOp::Status
        );
        assert!(parse_control("{\"op\":\"restart-daemon\"}").is_err());
    }

    #[test]
    fn a_failed_reply_is_not_ok() {
        assert!(reply_ok("{\"status\":\"ok\",\"op\":\"start\"}\n"));
        assert!(!reply_ok(
            "{\"status\":\"failed\",\"error\":\"hub start was not persisted\"}"
        ));
        assert!(!reply_ok(""));
        assert!(!reply_ok("garbage"));
        assert!(!reply_healthy("{\"status\":\"ok\",\"healthy\":false}"));
        assert!(reply_healthy("{\"status\":\"ok\",\"healthy\":true}"));
    }

    fn live_slack(connected: bool) -> crate::chat::slack::SocketModeStatus {
        crate::chat::slack::SocketModeStatus {
            running: true,
            connected,
            connects: u64::from(connected),
            last_error: None,
        }
    }

    #[tokio::test]
    async fn status_names_each_reason_the_hub_is_not_healthy() {
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled) VALUES('a','every 1m','{}',1),('b','every 1m','{}',0)")
            .execute(&pool)
            .await
            .unwrap();
        let stopped = status_report(
            &pool,
            &LiveHub {
                http_addr: "127.0.0.1:9119".into(),
                ..LiveHub::default()
            },
        )
        .await;
        assert_eq!(stopped["healthy"], false);
        assert_eq!(stopped["running"], false);
        assert_eq!(stopped["stopped_flag"], true);
        assert_eq!(stopped["cron"]["jobs"], 2);
        assert_eq!(stopped["cron"]["enabled"], 1);
        assert_eq!(stopped["problems"][0], "hub tasks are not running");

        clear_stopped(&pool).await.unwrap();
        let mut live = LiveHub {
            running: true,
            http_addr: "127.0.0.1:9119".into(),
            http_bound: true,
            slack_configured: true,
            slack: Some(live_slack(true)),
            discord_configured: false,
            version: "test".into(),
        };
        let up = status_report(&pool, &live).await;
        assert_eq!(up["healthy"], true, "{up}");
        assert_eq!(up["stopped_flag"], false);
        assert_eq!(up["slack"]["connected"], true);
        assert!(reply_healthy(&up.to_string()));

        live.slack = Some(live_slack(false));
        live.http_bound = false;
        let down = status_report(&pool, &live).await;
        assert_eq!(down["healthy"], false);
        let problems = down["problems"].to_string();
        assert!(
            problems.contains("http 127.0.0.1:9119 is not bound"),
            "{problems}"
        );
        assert!(
            problems.contains("slack socket mode is not connected"),
            "{problems}"
        );

        live.slack = Some(live_slack(true));
        live.http_bound = true;
        sqlx::query("INSERT INTO inbox_watcher_health(source, status, detail, updated_at) VALUES('imsg','failed','secret stderr','2026-10-23T00:00:00Z')")
            .execute(&pool)
            .await
            .unwrap();
        let watcher = status_report(&pool, &live).await;
        assert_eq!(watcher["healthy"], false);
        assert_eq!(watcher["problems"][0], "inbox watcher imsg failed");
        assert!(
            !watcher.to_string().contains("secret stderr"),
            "watcher stderr is not part of status"
        );
    }

    #[tokio::test]
    async fn a_stop_stays_stopped_until_start_clears_it() {
        let pool = crate::storage::memory().await.unwrap();
        assert!(
            is_stopped(&pool).await.unwrap(),
            "a fresh hub stays down until an explicit start"
        );
        clear_stopped(&pool).await.unwrap();
        assert!(!is_stopped(&pool).await.unwrap());
        mark_stopped(&pool).await.unwrap();
        assert!(is_stopped(&pool).await.unwrap());
        clear_stopped(&pool).await.unwrap();
        assert!(!is_stopped(&pool).await.unwrap());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stop_reaches_the_socket_and_the_reply_names_the_op() {
        let dir = std::env::temp_dir().join(format!(
            "ferryx-control-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control.sock");
        let listener = crate::schedule::bind_native_socket(&path).await.unwrap();
        let path_for_client = path.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = tokio::io::BufReader::new(stream);
            let mut line = String::new();
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
            reader.read_line(&mut line).await.unwrap();
            let op = parse_control(&line).unwrap();
            let mut stream = reader.into_inner();
            stream
                .write_all(format!("{{\"status\":\"ok\",\"op\":\"{:?}\"}}\n", op).as_bytes())
                .await
                .unwrap();
        });
        let reply = send_control(&path_for_client, ControlOp::Stop)
            .await
            .unwrap();
        server.await.unwrap();
        assert!(reply.contains("Stop"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
