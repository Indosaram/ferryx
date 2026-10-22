//! Hub control socket. `stop` drops chat, cron, and HTTP tasks. The daemon and its
//! PTYs stay up. `start` is the signal to bind them again.
use serde_json::Value;

use crate::error::{HubError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlOp {
    Stop,
    Start,
}

pub fn parse_control(line: &str) -> Result<ControlOp> {
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|error| HubError::message(format!("hub control: {error}")))?;
    match value.get("op").and_then(Value::as_str) {
        Some("stop") => Ok(ControlOp::Stop),
        Some("start") => Ok(ControlOp::Start),
        _ => Err(HubError::message("hub control expected stop or start")),
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
    }
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
    fn stop_and_start_are_the_only_ops() {
        assert_eq!(
            parse_control("{\"op\":\"stop\"}\n").unwrap(),
            ControlOp::Stop
        );
        assert_eq!(
            parse_control("{\"op\":\"start\"}").unwrap(),
            ControlOp::Start
        );
        assert!(parse_control("{\"op\":\"restart-daemon\"}").is_err());
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
