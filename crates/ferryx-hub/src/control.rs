//! Hub control socket. `stop` drops chat, cron, and HTTP tasks. The daemon and its
//! PTYs stay up. `start` is the signal to bind them again. `status` reports what is live.
//! `cutover` moves the owner's supervision from the legacy intake to the unified supervisor
//! (refused unless the mhc engine is ready with every required capability), `rollback` moves
//! it back without touching receipts or PTYs, and `intake` reports which one is open
//! ([`crate::supervisor::cutover`]).
use serde::Serialize;
use serde_json::{json, Value};

use crate::error::{HubError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlOp {
    Stop,
    Start,
    Status,
    /// `ferryx hub cron approve-rerun <id>`: the owner approves one more verified rerun of a
    /// cron problem waiting on them. Reaching this socket (0600, owner-only) is the authority;
    /// no secret travels through chat.
    ApproveRerun(i64),
    Cutover,
    Rollback,
    Intake,
}

pub fn parse_control(line: &str) -> Result<ControlOp> {
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|error| HubError::message(format!("hub control: {error}")))?;
    match value.get("op").and_then(Value::as_str) {
        Some("stop") => Ok(ControlOp::Stop),
        Some("start") => Ok(ControlOp::Start),
        Some("status") => Ok(ControlOp::Status),
        Some("approve_rerun") => value
            .get("problem")
            .and_then(Value::as_i64)
            .map(ControlOp::ApproveRerun)
            .ok_or_else(|| HubError::message("approve_rerun needs a numeric problem id")),
        Some("cutover") => Ok(ControlOp::Cutover),
        Some("rollback") => Ok(ControlOp::Rollback),
        Some("intake") => Ok(ControlOp::Intake),
        _ => Err(HubError::message(
            "hub control expected stop, start, status, approve_rerun, cutover, rollback, or intake",
        )),
    }
}

pub fn control_socket(home: &std::path::Path) -> std::path::PathBuf {
    home.join(".ferryx").join("hub").join("control.sock")
}

/// `hub stop` writes `hub_stopped = 1` and `hub start` writes `hub_stopped = 0`.
/// A hub database with neither is a fresh install. It stays stopped too, because the
/// hub uses the Slack tokens omo-gateway used and would otherwise open a second
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

pub fn control_line(op: ControlOp) -> String {
    match op {
        ControlOp::Stop => "{\"op\":\"stop\"}\n".to_owned(),
        ControlOp::Start => "{\"op\":\"start\"}\n".to_owned(),
        ControlOp::Status => "{\"op\":\"status\"}\n".to_owned(),
        ControlOp::ApproveRerun(problem) => {
            format!("{}\n", json!({"op": "approve_rerun", "problem": problem}))
        }
        ControlOp::Cutover => "{\"op\":\"cutover\"}\n".to_owned(),
        ControlOp::Rollback => "{\"op\":\"rollback\"}\n".to_owned(),
        ControlOp::Intake => "{\"op\":\"intake\"}\n".to_owned(),
    }
}

/// The control reply to `approve_rerun`. The rerun itself runs in the background and reports
/// to the owner like any remediation outcome.
pub async fn approve_rerun_reply(problem: i64) -> String {
    let reply = match crate::cron_remediate::owner_approve(problem).await {
        Ok(()) => {
            json!({"status": "ok", "op": "approve_rerun", "problem": problem, "rerun": "started"})
        }
        Err(error) => {
            json!({"status": "failed", "op": "approve_rerun", "problem": problem, "error": error.to_string()})
        }
    };
    format!("{reply}\n")
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
    /// The supervisor lease as this daemon sees it; `None` from a caller without one.
    pub lease: Option<LiveLease>,
}

/// This daemon's supervisor lease role, as reported on the wire.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LeaseRole {
    /// Runs the hub (a leased holder, a provisional offline hub, or no lease client at all).
    #[default]
    Holder,
    /// Another machine (or an unknown one) holds the lease; this hub stays off.
    Standby,
    /// The relay cannot arbitrate; the hub stays off.
    Unleased,
}

/// Why a standby would not take the supervisor over now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromotionBlock {
    pub code: String,
    pub message: String,
}

/// An open goal of this machine's archived supervisor the owner may adopt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptableGoal {
    pub goal_id: String,
    pub title: String,
    pub supervisor_id: String,
    pub from_machine: String,
}

/// A host the holder could not push its fence to (`host: None` is this machine's daemon).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnreachableHost {
    pub host: Option<String>,
    pub reason: String,
}

/// The supervisor lease fields of `status` (`supervisor_lease`) and of the snapshot's `hub`
/// object, camelCase on the wire. Ids, labels and counts only: no token or secret.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveLease {
    pub role: LeaseRole,
    pub holder_machine_id: Option<String>,
    pub holder_label: Option<String>,
    pub holder_paired: bool,
    /// The lease fence term this machine knows (0: none known, or an unleased hub).
    pub fence: u64,
    /// Seconds until this holder's grant runs out unless renewed (0: no expiring grant held).
    pub lease_expires_in: u64,
    /// Filled by the checkpoint replication code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_id: Option<String>,
    /// Filled by the checkpoint replication code (seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_age: Option<u64>,
    /// Filled by the auto-promotion code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promotion_block_reason: Option<PromotionBlock>,
    /// Filled by the archived-goal adoption code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adoptable: Option<Vec<AdoptableGoal>>,
    pub observe_dropped: u64,
    pub unmapped_hosts: Vec<String>,
    pub unreachable_hosts: Vec<UnreachableHost>,
    /// A standby that has known no holder for longer than the lease TTL.
    #[serde(skip)]
    pub no_holder_past_ttl: bool,
}

impl LiveLease {
    fn standby(&self) -> bool {
        self.role == LeaseRole::Standby
    }

    /// The plain-text hint `hub status` prints on standby, naming the holder.
    fn status_hint(&self) -> Option<String> {
        if !self.standby() {
            return None;
        }
        let holder = match (&self.holder_label, &self.holder_machine_id) {
            (Some(label), Some(id)) => format!("{label} ({id})"),
            (Some(label), None) => label.clone(),
            (None, Some(id)) => id.clone(),
            (None, None) => "no known machine".to_owned(),
        };
        Some(format!(
            "standby: the supervisor runs on {holder}; this machine's hub stays off while it holds the lease"
        ))
    }
}

/// Host ids an imported checkpoint could not scope to a machine (`unmapped:<id>`), sorted.
pub async fn unmapped_hosts(pool: &sqlx::SqlitePool) -> Vec<String> {
    use crate::checkpoint::{HOST_ID_TABLES, UNMAPPED_HOST_PREFIX};
    let mut hosts = std::collections::BTreeSet::new();
    for table in HOST_ID_TABLES {
        let columns: i64 =
            sqlx::query_scalar("SELECT count(*) FROM pragma_table_info(?) WHERE name = 'host_id'")
                .bind(*table)
                .fetch_one(pool)
                .await
                .unwrap_or(0);
        if columns == 0 {
            continue;
        }
        let found: Vec<String> = sqlx::query_scalar(&format!(
            "SELECT DISTINCT host_id FROM {table} WHERE substr(host_id, 1, ?) = ?"
        ))
        .bind(UNMAPPED_HOST_PREFIX.len() as i64)
        .bind(UNMAPPED_HOST_PREFIX)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
        hosts.extend(found);
    }
    hosts.into_iter().collect()
}

/// The `status` reply. Counts and states only: no tokens, message text, or watcher stderr.
/// `healthy` is false when the hub is stopped, HTTP is not bound, a configured Slack app has
/// no open socket, or an inbox watcher's last run failed; each reason is in `problems`. A
/// standby's stopped hub is healthy, unless it has known no holder for longer than the TTL.
pub async fn status_report(pool: &sqlx::SqlitePool, live: &LiveHub) -> Value {
    let mut problems = Vec::new();
    let stopped_flag = is_stopped(pool).await.unwrap_or(true);
    let standby = live.lease.as_ref().is_some_and(LiveLease::standby);
    if !live.running && !standby {
        problems.push("hub tasks are not running".to_owned());
    }
    if standby && live.lease.as_ref().is_some_and(|lease| lease.no_holder_past_ttl) {
        problems.push("no supervisor holds the lease".to_owned());
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
    let status_hint = live.lease.as_ref().and_then(LiveLease::status_hint);
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
        "supervisor_intake": crate::supervisor::cutover::installation_intake(pool)
            .await
            .ok()
            .and_then(|intake| serde_json::to_value(intake).ok()),
        "inbox_watchers": watchers
            .iter()
            .map(|(source, status, updated_at)| json!({"source": source, "status": status, "updated_at": updated_at}))
            .collect::<Vec<_>>(),
        "supervisor_lease": live.lease,
        "status_hint": status_hint,
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

/// The `status_hint` of a `status` reply, when it has one (a standby naming the holder).
pub fn reply_status_hint(reply: &str) -> Option<String> {
    serde_json::from_str::<Value>(reply.trim())
        .ok()?
        .get("status_hint")?
        .as_str()
        .map(str::to_owned)
}

/// The reply to `cutover`, `rollback` or `intake`: the intake after the op and the steps it
/// committed, or the typed refusal (`MHC_CAPABILITY_MISSING`, `MHC_NOT_RUNNING`, ...).
pub fn intake_reply(
    op: &str,
    result: std::result::Result<crate::supervisor::cutover::Transition, crate::supervisor::SupervisorError>,
) -> Value {
    match result {
        Ok(transition) => json!({
            "status": "ok",
            "op": op,
            "intake": transition.intake,
            "steps": transition.steps,
        }),
        Err(error) => json!({
            "status": "failed",
            "op": op,
            "code": error.code(),
            "error": error.to_string(),
        }),
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
            parse_control(&control_line(ControlOp::Status)).unwrap(),
            ControlOp::Status
        );
        assert_eq!(
            parse_control(&control_line(ControlOp::ApproveRerun(134))).unwrap(),
            ControlOp::ApproveRerun(134)
        );
        assert!(parse_control("{\"op\":\"approve_rerun\"}").is_err());
        for op in [ControlOp::Cutover, ControlOp::Rollback, ControlOp::Intake] {
            assert_eq!(parse_control(&control_line(op)).unwrap(), op);
        }
        assert!(parse_control("{\"op\":\"restart-daemon\"}").is_err());
    }

    #[tokio::test]
    async fn approve_rerun_without_a_remediator_fails_and_says_why() {
        let reply = approve_rerun_reply(7).await;
        assert!(!reply_ok(&reply), "{reply}");
        assert!(reply.contains("cron remediator not installed"), "{reply}");
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
            lease: None,
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

    fn standby_lease() -> LiveLease {
        LiveLease {
            role: LeaseRole::Standby,
            holder_machine_id: Some("mach-b".into()),
            holder_label: Some("Laptop B".into()),
            holder_paired: true,
            observe_dropped: 3,
            ..LiveLease::default()
        }
    }

    #[tokio::test]
    async fn a_standby_with_a_known_holder_is_healthy_and_names_it() {
        let pool = crate::storage::memory().await.unwrap();
        let live = LiveHub {
            http_addr: "127.0.0.1:9119".into(),
            lease: Some(standby_lease()),
            ..LiveHub::default()
        };

        let report = status_report(&pool, &live).await;

        assert_eq!(report["healthy"], true, "{report}");
        assert_eq!(report["running"], false);
        let lease = &report["supervisor_lease"];
        assert_eq!(lease["role"], "standby");
        assert_eq!(lease["holderMachineId"], "mach-b");
        assert_eq!(lease["holderLabel"], "Laptop B");
        assert_eq!(lease["holderPaired"], true);
        assert_eq!(lease["observeDropped"], 3);
        assert_eq!(lease["unmappedHosts"], json!([]));
        assert_eq!(lease["unreachableHosts"], json!([]));
        assert_eq!(lease["fence"], 0);
        assert_eq!(lease["leaseExpiresIn"], 0);
        for absent in ["checkpointId", "checkpointAge", "promotionBlockReason", "adoptable"] {
            assert!(lease.get(absent).is_none(), "{absent} in {lease}");
        }
        let hint = reply_status_hint(&report.to_string()).expect("a standby hint");
        assert!(hint.contains("Laptop B (mach-b)"), "{hint}");
        assert!(reply_healthy(&report.to_string()));
    }

    #[tokio::test]
    async fn a_standby_without_a_holder_past_the_ttl_is_unhealthy() {
        let pool = crate::storage::memory().await.unwrap();
        let live = LiveHub {
            lease: Some(LiveLease {
                holder_machine_id: None,
                holder_label: None,
                no_holder_past_ttl: true,
                ..standby_lease()
            }),
            ..LiveHub::default()
        };

        let report = status_report(&pool, &live).await;

        assert_eq!(report["healthy"], false);
        assert_eq!(report["problems"], json!(["no supervisor holds the lease"]));
        assert!(report["status_hint"].as_str().unwrap().contains("no known machine"));
    }

    #[tokio::test]
    async fn a_holder_reports_its_lease_and_no_hint() {
        let pool = crate::storage::memory().await.unwrap();
        let live = LiveHub {
            lease: Some(LiveLease {
                role: LeaseRole::Holder,
                fence: 7,
                lease_expires_in: 90,
                unreachable_hosts: vec![UnreachableHost { host: Some("machine:mach-c".into()), reason: "timed out".into() }],
                ..LiveLease::default()
            }),
            ..LiveHub::default()
        };

        let report = status_report(&pool, &live).await;

        assert_eq!(report["problems"][0], "hub tasks are not running", "a stopped holder stays unhealthy");
        let lease = &report["supervisor_lease"];
        assert_eq!(lease["role"], "holder");
        assert_eq!(lease["fence"], 7);
        assert_eq!(lease["leaseExpiresIn"], 90);
        assert_eq!(lease["unreachableHosts"], json!([{"host": "machine:mach-c", "reason": "timed out"}]));
        assert!(report["status_hint"].is_null());
        assert!(reply_status_hint(&report.to_string()).is_none());
    }

    #[tokio::test]
    async fn unmapped_hosts_lists_each_unscoped_host_once() {
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO fleet_inventory_cache(host_id, sessions, updated_at) VALUES('unmapped:stranger','[]','t'),('machine:mach-a','[]','t')")
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(unmapped_hosts(&pool).await, ["unmapped:stranger"]);
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
