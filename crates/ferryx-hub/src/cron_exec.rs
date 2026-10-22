//! Cron payload kinds from omo-gateway `56dca45`.
//!
//! A native job is `script` / `command` / `shell`, an agent `prompt` or `brief_file`,
//! or both. A Hermes job also carries `schedule`, `skills` / `skill`, `no_agent`,
//! and `monitor_script` / `monitor_url`. The agent turn goes to the omo app-server
//! (`ws://127.0.0.1:19742`, override `FERRYX_HUB_OMO_URL`). Skill files are read from
//! the Hermes skills root and prepended to that turn. `[SILENT]` and `wakeAgent: false`
//! suppress delivery.
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::{HubError, Result};
use crate::lanes::kibitzer::ModelTurn;
use crate::schedule::script_from_payload;

pub const OMO_APP_SERVER: &str = "ws://127.0.0.1:19742";
pub const CRON_HINT: &str = "[IMPORTANT: You are running as a scheduled cron job. DELIVERY: Your final response will be automatically delivered to the user — do NOT use send_message or try to deliver the output yourself. Just produce your report/output as your final response and the system handles the rest. SILENT: If there is genuinely nothing new to report, respond with exactly \"[SILENT]\" (nothing else) to suppress delivery. Never combine [SILENT] with content — either report your findings normally, or say [SILENT] and nothing more.]\n\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Executed {
    pub output: String,
    pub deliver: bool,
    /// Set when a monitor script changed. The caller commits it only after the run succeeds.
    pub monitor_hash: Option<String>,
}

pub fn payload_kinds(payload: &Value) -> Vec<&'static str> {
    let mut kinds = Vec::new();
    if payload.get("schedule").is_some() {
        kinds.push("hermes");
    }
    if script_from_payload(payload).is_some() {
        kinds.push("script");
    }
    let prompt = payload
        .get("prompt")
        .or_else(|| payload.get("message"))
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty());
    let brief = payload
        .get("brief_file")
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty());
    if prompt || brief {
        kinds.push("agent");
    }
    if !skill_names(payload).is_empty() {
        kinds.push("skill");
    }
    if payload.get("no_agent").and_then(Value::as_bool) == Some(true) {
        kinds.push("no_agent");
    }
    if payload
        .get("monitor_script")
        .and_then(Value::as_str)
        .is_some()
        || payload.get("monitor_url").and_then(Value::as_str).is_some()
    {
        kinds.push("monitor");
    }
    if kinds.is_empty() {
        kinds.push("empty");
    }
    kinds
}

pub fn skill_names(payload: &Value) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(list) = payload.get("skills").and_then(Value::as_array) {
        for item in list {
            if let Some(name) = item.as_str().map(str::trim).filter(|name| !name.is_empty()) {
                if !names.iter().any(|existing| existing == name) {
                    names.push(name.to_owned());
                }
            }
        }
    }
    if let Some(name) = payload
        .get("skill")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        if !names.iter().any(|existing| existing == name) {
            names.push(name.to_owned());
        }
    }
    names
}

pub fn find_skill_file(root: &Path, name: &str) -> Option<PathBuf> {
    let direct = [
        root.join(format!("{name}.md")),
        root.join(name).join("SKILL.md"),
    ];
    direct.into_iter().find(|path| path.is_file())
}

pub async fn load_skills(root: &Path, names: &[String]) -> Result<String> {
    if names.is_empty() {
        return Ok(String::new());
    }
    let mut body = String::new();
    let mut missing = Vec::new();
    for name in names {
        match find_skill_file(root, name) {
            Some(path) => {
                let text = tokio::fs::read_to_string(&path)
                    .await
                    .map_err(|error| HubError::message(error.to_string()))?;
                body.push_str(&format!("[Skill {name}]\n{text}\n"));
            }
            None => missing.push(name.clone()),
        }
    }
    if body.is_empty() {
        return Err(HubError::message(format!(
            "cron skills missing: {}",
            missing.join(", ")
        )));
    }
    if !missing.is_empty() {
        body.push_str(&format!(
            "⚠️ Skill(s) not found and skipped: {}\n",
            missing.join(", ")
        ));
    }
    Ok(body)
}

pub async fn run_payload(
    pool: &sqlx::SqlitePool,
    job_id: &str,
    payload: &Value,
    model: &dyn ModelTurn,
    skills_root: Option<&Path>,
) -> Result<Executed> {
    if payload.get("monitor_script").is_some() && payload.get("monitor_url").is_some() {
        return Err(HubError::message(format!(
            "cron job {job_id} sets both monitor_script and monitor_url"
        )));
    }
    let script_output = if let Some(script) = script_from_payload(payload) {
        Some(run_script(&script, timeout_secs(payload)).await?)
    } else {
        None
    };
    if let Some(output) = script_output.as_deref() {
        if !crate::cron::parse_wake_gate(output) {
            return Ok(Executed {
                output: output.to_owned(),
                deliver: false,
                monitor_hash: None,
            });
        }
    }
    let monitor_hash = match monitor_hash(pool, job_id, payload).await? {
        Monitor::Same => {
            return Ok(Executed {
                output: String::new(),
                deliver: false,
                monitor_hash: None,
            });
        }
        Monitor::Changed(hash) => Some(hash),
        Monitor::Absent => None,
    };
    let no_agent = payload.get("no_agent").and_then(Value::as_bool) == Some(true);
    if no_agent {
        let output = script_output.clone().unwrap_or_default();
        if output.trim().is_empty() {
            return Err(HubError::message(format!(
                "cron job {job_id} specifies no_agent but has no script output"
            )));
        }
        return Ok(Executed {
            deliver: should_post(&output),
            output,
            monitor_hash,
        });
    }
    let prompt = read_prompt(payload).await?;
    let names = skill_names(payload);
    let skills = match skills_root {
        Some(root) if !names.is_empty() => load_skills(root, &names).await?,
        _ if !names.is_empty() => {
            return Err(HubError::message(format!(
                "cron job {job_id} names skills but no skills root is configured"
            )));
        }
        _ => String::new(),
    };
    if prompt.trim().is_empty() && script_output.is_none() && skills.trim().is_empty() {
        return Err(HubError::message(format!(
            "cron job {job_id} has neither prompt nor executable script or skill"
        )));
    }
    if prompt.trim().is_empty() && skills.trim().is_empty() {
        let output = script_output.unwrap_or_default();
        return Ok(Executed {
            deliver: should_post(&output),
            output,
            monitor_hash,
        });
    }
    let mut task = String::from(CRON_HINT);
    if !skills.is_empty() {
        task.push_str(&skills);
        if !prompt.trim().is_empty() {
            task.push_str("\n\n[Task]\n");
        }
    }
    task.push_str(&prompt);
    if let Some(output) = script_output
        .as_deref()
        .filter(|output| !output.trim().is_empty())
    {
        task.push_str("\n\n[Script output]\n");
        task.push_str(output);
    }
    if let Some(context) = context_from_block(pool, payload).await? {
        task.push('\n');
        task.push_str(&context);
    }
    let output = model.complete(&task).await?;
    Ok(Executed {
        deliver: should_post(&output),
        output,
        monitor_hash,
    })
}

pub async fn commit_monitor(pool: &sqlx::SqlitePool, job_id: &str, hash: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO cron_monitor_states(job_id, last_hash, updated_at) VALUES(?,?,?)
         ON CONFLICT(job_id) DO UPDATE SET last_hash = excluded.last_hash, updated_at = excluded.updated_at",
    )
    .bind(job_id)
    .bind(hash)
    .bind(crate::storage::now_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

enum Monitor {
    Absent,
    Same,
    Changed(String),
}

fn should_post(output: &str) -> bool {
    let trimmed = output.trim();
    !trimmed.is_empty()
        && crate::cron::parse_wake_gate(trimmed)
        && !crate::cron::is_cron_silence_response(trimmed)
}

fn timeout_secs(payload: &Value) -> u64 {
    payload
        .get("timeout_secs")
        .or_else(|| payload.get("timeout_seconds"))
        .or_else(|| payload.get("timeout"))
        .or_else(|| payload.get("script_timeout"))
        .and_then(Value::as_u64)
        .unwrap_or(30)
}

async fn read_prompt(payload: &Value) -> Result<String> {
    if let Some(path) = payload.get("brief_file").and_then(Value::as_str) {
        return tokio::fs::read_to_string(path)
            .await
            .map_err(|error| HubError::message(format!("brief_file {path}: {error}")));
    }
    Ok(payload
        .get("prompt")
        .or_else(|| payload.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned())
}

async fn monitor_hash(pool: &sqlx::SqlitePool, job_id: &str, payload: &Value) -> Result<Monitor> {
    let script = payload.get("monitor_script").and_then(Value::as_str);
    let Some(script) = script else {
        return Ok(Monitor::Absent);
    };
    let output = run_script(script, timeout_secs(payload)).await?;
    let hash = format!("{:x}", Sha256::digest(output.as_bytes()));
    let previous: Option<String> =
        sqlx::query_scalar("SELECT last_hash FROM cron_monitor_states WHERE job_id = ?")
            .bind(job_id)
            .fetch_optional(pool)
            .await?;
    if previous.as_deref() == Some(hash.as_str()) {
        Ok(Monitor::Same)
    } else {
        Ok(Monitor::Changed(hash))
    }
}

async fn context_from_block(pool: &sqlx::SqlitePool, payload: &Value) -> Result<Option<String>> {
    let Some(ids) = payload.get("context_from").and_then(Value::as_array) else {
        return Ok(None);
    };
    let mut body = String::from("\n[Context]\n");
    let mut any = false;
    for id in ids {
        let Some(id) = id.as_str() else {
            continue;
        };
        let output: Option<String> = sqlx::query_scalar(
            "SELECT output FROM cron_runs WHERE job_id = ? AND status = 'succeeded' ORDER BY finished_at DESC LIMIT 1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        if let Some(output) = output {
            any = true;
            let clipped: String = output.chars().take(2_000).collect();
            body.push_str(&format!("# {id}\n{clipped}\n"));
        }
    }
    Ok(any.then_some(body))
}

async fn run_script(script: &str, timeout_secs: u64) -> Result<String> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
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

pub struct StaticModel(pub String);

#[async_trait]
impl ModelTurn for StaticModel {
    async fn complete(&self, prompt: &str) -> Result<String> {
        if prompt.is_empty() {
            return Err(HubError::message("empty cron prompt"));
        }
        Ok(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;

    #[test]
    fn payload_kinds_cover_script_agent_skill_and_hermes() {
        assert_eq!(
            payload_kinds(&serde_json::json!({"command": "printf hi"})),
            ["script"]
        );
        assert_eq!(
            payload_kinds(&serde_json::json!({"prompt": "report"})),
            ["agent"]
        );
        assert_eq!(
            payload_kinds(&serde_json::json!({
                "schedule": {"kind": "cron"},
                "prompt": "report",
                "skill": "status",
                "script": "true",
                "no_agent": true,
                "monitor_script": "printf x"
            })),
            ["hermes", "script", "agent", "skill", "no_agent", "monitor"]
        );
    }

    #[tokio::test]
    async fn an_agent_job_sends_the_skill_and_the_prompt_to_the_model() {
        let root = std::env::temp_dir().join(format!("ferryx-skills-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("status")).unwrap();
        std::fs::write(root.join("status").join("SKILL.md"), "be brief").unwrap();
        let pool = crate::storage::memory().await.unwrap();
        let model = Capture(std::sync::Mutex::new(String::new()));
        let executed = run_payload(
            &pool,
            "daily",
            &serde_json::json!({"prompt": "what shipped", "skill": "status", "deliver": "slack:C1"}),
            &model,
            Some(&root),
        )
        .await
        .unwrap();
        assert!(executed.deliver);
        assert_eq!(executed.output, "shipped");
        let prompt = model.0.lock().unwrap().clone();
        assert!(prompt.contains(CRON_HINT));
        assert!(prompt.contains("be brief"));
        assert!(prompt.contains("what shipped"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn wake_agent_false_skips_the_model_and_a_script_alone_is_the_output() {
        let pool = crate::storage::memory().await.unwrap();
        let model = StaticModel("should-not-run".into());
        let skipped = run_payload(
            &pool,
            "gate",
            &serde_json::json!({"script": "printf '{\"wakeAgent\":false}'", "prompt": "go"}),
            &model,
            None,
        )
        .await
        .unwrap();
        assert!(!skipped.deliver);
        let script = run_payload(
            &pool,
            "echo",
            &serde_json::json!({"command": "printf hi"}),
            &model,
            None,
        )
        .await
        .unwrap();
        assert_eq!(script.output, "hi");
        assert!(script.deliver);
    }

    #[tokio::test]
    async fn a_monitor_hash_is_committed_only_after_success_and_context_is_prepended() {
        let pool = crate::storage::memory().await.unwrap();
        let model = Capture(std::sync::Mutex::new(String::new()));
        sqlx::query("INSERT INTO cron_runs(id, job_id, status, started_at, finished_at, output) VALUES('r1','pred','succeeded','t','t','previous output')")
            .execute(&pool)
            .await
            .unwrap();
        let first = run_payload(
            &pool,
            "watch",
            &serde_json::json!({"prompt": "check", "monitor_script": "printf same", "context_from": ["pred"]}),
            &model,
            None,
        )
        .await
        .unwrap();
        assert!(first.monitor_hash.is_some());
        let stored: Option<String> =
            sqlx::query_scalar("SELECT last_hash FROM cron_monitor_states WHERE job_id = 'watch'")
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert!(stored.is_none());
        commit_monitor(&pool, "watch", first.monitor_hash.as_deref().unwrap())
            .await
            .unwrap();
        let second = run_payload(
            &pool,
            "watch",
            &serde_json::json!({"prompt": "check", "monitor_script": "printf same"}),
            &model,
            None,
        )
        .await
        .unwrap();
        assert!(!second.deliver);
        assert!(second.output.is_empty());
        let prompt = model.0.lock().unwrap().clone();
        assert!(prompt.contains("previous output"));
    }

    #[tokio::test]
    async fn the_scheduler_sends_an_agent_job_to_the_model() {
        let pool = crate::storage::memory().await.unwrap();
        let due = (chrono::Utc::now() - chrono::TimeDelta::minutes(1)).to_rfc3339();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled, next_run_at, delivery_json) VALUES('agent','every 10m',?,1,?,'[]')")
            .bind(r#"{"prompt":"what shipped","deliver":"slack:C1"}"#)
            .bind(&due)
            .execute(&pool)
            .await
            .unwrap();
        let model = StaticModel("from-model".into());
        let deliveries = crate::schedule::drive_due_with(&pool, chrono::Utc::now(), &model, None)
            .await
            .unwrap();
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].text, "from-model");
    }

    struct Capture(std::sync::Mutex<String>);

    #[async_trait]
    impl ModelTurn for Capture {
        async fn complete(&self, prompt: &str) -> Result<String> {
            *self.0.lock().unwrap() = prompt.to_owned();
            Ok("shipped".into())
        }
    }
}
