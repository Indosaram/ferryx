//! Cron runs as a shell script, a turn on the meta pane, or a turn on a new pane.
//! `wakeAgent: false` on the script's last line skips the agent turn.
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::agent_control::{AgentControl, AgentOp, TaskLauncher};
use crate::error::{HubError, Result};
use crate::storage::now_rfc3339;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CronTarget {
    Shell,
    MetaPane,
    NewPane,
}

impl CronTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::MetaPane => "meta_pane",
            Self::NewPane => "new_pane",
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "shell" => Ok(Self::Shell),
            "meta_pane" => Ok(Self::MetaPane),
            "new_pane" => Ok(Self::NewPane),
            other => Err(HubError::config(format!("unknown cron target {other}"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub target: CronTarget,
    pub script: String,
    pub schedule: String,
}

pub fn parse_wake_gate(stdout: &str) -> bool {
    let lines: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let Some(last) = lines.last() else {
        return true;
    };
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(last) {
        if let Some(wake) = value.get("wakeAgent").or_else(|| value.get("wake_agent")) {
            if let Some(flag) = wake.as_bool() {
                return flag;
            }
        }
    }
    let lower = last.to_ascii_lowercase();
    if lower == "wakeagent: false"
        || lower == "wake_agent: false"
        || lower == "{\"wakeagent\": false}"
        || lower == "{\"wakeagent\":false}"
    {
        return false;
    }
    true
}

pub fn is_cron_silence_response(text: &str) -> bool {
    let upper = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase();
    matches!(
        upper.as_str(),
        "[SILENT]" | "SILENT" | "NO_REPLY" | "NO REPLY"
    )
}

pub async fn insert_job(pool: &SqlitePool, job: &CronJob) -> Result<()> {
    sqlx::query(
        "INSERT INTO cron_jobs(id, name, target, script, schedule, created_at) VALUES(?,?,?,?,?,?)",
    )
    .bind(&job.id)
    .bind(&job.name)
    .bind(job.target.as_str())
    .bind(&job.script)
    .bind(&job.schedule)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_jobs(pool: &SqlitePool) -> Result<Vec<CronJob>> {
    let rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, name, target, script, schedule FROM cron_jobs ORDER BY created_at",
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|(id, name, target, script, schedule)| {
            Ok(CronJob {
                id,
                name,
                target: CronTarget::parse(&target)?,
                script,
                schedule,
            })
        })
        .collect()
}

pub async fn execute(
    pool: &SqlitePool,
    job_id: &str,
    control: &dyn AgentControl,
    launcher: &dyn TaskLauncher,
    meta_session_id: &str,
) -> Result<String> {
    let row: Option<(String, String, String)> =
        sqlx::query_as("SELECT target, script, name FROM cron_jobs WHERE id = ?")
            .bind(job_id)
            .fetch_optional(pool)
            .await?;
    let Some((target, script, name)) = row else {
        return Err(HubError::message(format!("cron job {job_id} not found")));
    };
    let target = CronTarget::parse(&target)?;
    let script_output = if script.trim().is_empty() {
        String::new()
    } else {
        run_script(&script).await?
    };
    if !parse_wake_gate(&script_output) || target == CronTarget::Shell {
        return Ok(script_output);
    }
    if is_cron_silence_response(&script_output) {
        return Ok(String::new());
    }
    let session_id = match target {
        CronTarget::Shell => return Ok(script_output),
        CronTarget::MetaPane => meta_session_id.to_owned(),
        CronTarget::NewPane => launcher.launch_pane(".", &[]).await?,
    };
    let text = if script_output.trim().is_empty() {
        format!("cron {name}")
    } else {
        script_output.clone()
    };
    control
        .execute(
            None,
            AgentOp::Send {
                session_id,
                text,
                request_id: format!("cron-{job_id}"),
            },
        )
        .await?;
    Ok(script_output)
}

async fn run_script(script: &str) -> Result<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .output(),
    )
    .await
    .map_err(|_| HubError::message("cron script timed out"))?
    .map_err(|error| HubError::message(error.to_string()))?;
    if !output.status.success() {
        return Err(HubError::message(format!(
            "cron script failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub async fn execute_with(
    job: &CronJob,
    control: Arc<dyn AgentControl>,
    launcher: Arc<dyn TaskLauncher>,
    meta: &str,
    pool: &SqlitePool,
) -> Result<String> {
    insert_job(pool, job).await.ok();
    execute(pool, &job.id, control.as_ref(), launcher.as_ref(), meta).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::{FixedLauncher, RecordingControl};

    #[test]
    fn wake_agent_false_on_the_last_line_skips_the_agent() {
        assert!(!parse_wake_gate("hello\nwakeAgent: false\n"));
        assert!(!parse_wake_gate("{\"wakeAgent\":false}"));
        assert!(parse_wake_gate("hello"));
        assert!(is_cron_silence_response("NO_REPLY"));
    }

    #[tokio::test]
    async fn a_shell_job_does_not_send_and_a_new_pane_job_does() {
        let pool = crate::storage::memory().await.unwrap();
        let control = RecordingControl::default();
        let launcher = FixedLauncher::new("pane-cron");
        insert_job(
            &pool,
            &CronJob {
                id: "job-shell".into(),
                name: "echo".into(),
                target: CronTarget::Shell,
                script: "printf wakeAgent:false".into(),
                schedule: "".into(),
            },
        )
        .await
        .unwrap();
        let output = execute(&pool, "job-shell", &control, &launcher, "meta")
            .await
            .unwrap();
        assert!(
            output.contains("wakeAgent:false") || output.to_ascii_lowercase().contains("wakeagent")
        );
        assert!(control.ops().is_empty());

        insert_job(
            &pool,
            &CronJob {
                id: "job-pane".into(),
                name: "look".into(),
                target: CronTarget::NewPane,
                script: "printf ready".into(),
                schedule: "".into(),
            },
        )
        .await
        .unwrap();
        execute(&pool, "job-pane", &control, &launcher, "meta")
            .await
            .unwrap();
        assert!(matches!(control.ops()[0].1, AgentOp::Send { .. }));
        assert_eq!(launcher.launched.lock().unwrap().len(), 1);
    }
}
