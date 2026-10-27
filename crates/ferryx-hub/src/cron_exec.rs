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

/// Payload key holding the model the owner picked for one job. It is separate from the
/// imported omo `model` key, which stays ignored so stored data cannot move a job off the
/// background model by accident.
pub const MODEL_OVERRIDE_KEY: &str = "model_override";

/// Environment variable carrying the job's model to its script and monitor script.
pub const CRON_MODEL_ENV: &str = "FERRYX_CRON_MODEL";

/// A model id the hub accepts for a job: `provider/model`, no whitespace.
pub fn validate_model_id(model: &str) -> Result<String> {
    let model = model.trim();
    let valid = model
        .split_once('/')
        .is_some_and(|(provider, id)| !provider.is_empty() && !id.is_empty())
        && !model.chars().any(char::is_whitespace);
    if valid {
        Ok(model.to_owned())
    } else {
        Err(HubError::message(format!(
            "cron model must look like provider/model, got {model:?}"
        )))
    }
}

/// The job's own model when it set a valid `model_override`.
pub fn model_override(payload: &Value) -> Option<String> {
    payload
        .get(MODEL_OVERRIDE_KEY)
        .and_then(Value::as_str)
        .and_then(|model| validate_model_id(model).ok())
}

/// The model a job runs on: its `model_override`, else the background model.
pub fn job_model(payload: &Value) -> String {
    model_override(payload).unwrap_or_else(crate::model_policy::background_model)
}

/// One job and the model it runs on, for `ferryx hub cron list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobModel {
    pub id: String,
    pub enabled: bool,
    pub model: String,
    /// True when the job set its own `model_override`.
    pub overridden: bool,
}

pub async fn job_models(pool: &sqlx::SqlitePool) -> Result<Vec<JobModel>> {
    let rows: Vec<(String, i64, String)> =
        sqlx::query_as("SELECT id, enabled, payload_json FROM scheduled_jobs ORDER BY id")
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(id, enabled, payload_json)| {
            let payload: Value = serde_json::from_str(&payload_json).unwrap_or(Value::Null);
            JobModel {
                id,
                enabled: enabled == 1,
                model: job_model(&payload),
                overridden: model_override(&payload).is_some(),
            }
        })
        .collect())
}

/// Sets (`Some`) or clears (`None`) a job's `model_override`. Returns the model the job
/// now runs on, or `None` when no such job exists.
pub async fn set_model_override(
    pool: &sqlx::SqlitePool,
    job_id: &str,
    model: Option<&str>,
) -> Result<Option<String>> {
    let model = model.map(validate_model_id).transpose()?;
    let mut tx = pool.begin().await?;
    let stored: Option<String> =
        sqlx::query_scalar("SELECT payload_json FROM scheduled_jobs WHERE id = ?")
            .bind(job_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(stored) = stored else {
        return Ok(None);
    };
    let mut payload: Value = serde_json::from_str(&stored)
        .map_err(|error| HubError::message(format!("cron job {job_id} payload: {error}")))?;
    let Some(object) = payload.as_object_mut() else {
        return Err(HubError::message(format!(
            "cron job {job_id} payload is not an object"
        )));
    };
    match &model {
        Some(model) => {
            object.insert(MODEL_OVERRIDE_KEY.to_owned(), Value::String(model.clone()));
        }
        None => {
            object.remove(MODEL_OVERRIDE_KEY);
        }
    }
    sqlx::query("UPDATE scheduled_jobs SET payload_json = ?, updated_at = ? WHERE id = ?")
        .bind(payload.to_string())
        .bind(crate::storage::now_rfc3339())
        .bind(job_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Some(job_model(&payload)))
}
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
    if job_id == crate::inbox::DIGEST_JOB_ID {
        if payload.get("deliver").and_then(Value::as_str) != Some("origin") {
            return Err(HubError::message(
                "digest delivery must remain owner DM only",
            ));
        }
        let text = crate::inbox::build_scheduled_digest(pool, chrono::Utc::now()).await?;
        return Ok(Executed {
            deliver: text.is_some(),
            output: text.unwrap_or_default(),
            monitor_hash: None,
        });
    }
    if payload.get("monitor_script").is_some() && payload.get("monitor_url").is_some() {
        return Err(HubError::message(format!(
            "cron job {job_id} sets both monitor_script and monitor_url"
        )));
    }
    let cwd = script_working_directory(pool).await;
    let model_id = job_model(payload);
    let script = script_from_payload(payload);
    let script_output = if let Some(script) = script.as_deref() {
        Some(
            run_script(
                script,
                timeout_secs(payload),
                cwd.as_deref(),
                Some(&model_id),
            )
            .await?,
        )
    } else {
        None
    };
    let no_agent = payload.get("no_agent").and_then(Value::as_bool) == Some(true);
    if no_agent && script.is_none() {
        return Err(HubError::message(format!(
            "Hermes job {job_id} specifies no_agent=true but has no script"
        )));
    }
    if let Some(output) = script_output.as_deref() {
        if !crate::cron::parse_wake_gate(output) {
            return Ok(Executed {
                output: output.to_owned(),
                deliver: false,
                monitor_hash: None,
            });
        }
        let prompt_waiting = payload
            .get("prompt")
            .or_else(|| payload.get("message"))
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty())
            || payload
                .get("brief_file")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.trim().is_empty());
        if output.trim().is_empty() && prompt_waiting {
            return Ok(Executed {
                output: String::new(),
                deliver: false,
                monitor_hash: None,
            });
        }
    }
    if no_agent {
        let output = script_output.clone().unwrap_or_default();
        return Ok(Executed {
            deliver: should_post(&output),
            output,
            monitor_hash: None,
        });
    }
    let monitor_hash = match monitor_hash(pool, job_id, payload, cwd.as_deref(), &model_id).await? {
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
    // An agent cron turn runs on the job's `model_override` or the background model; the
    // imported payload `model` does not change it.
    let toolsets = payload
        .get("enabled_toolsets")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let output = model
        .complete_with(&task, Some(&model_id), &toolsets)
        .await?;
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
        .or_else(|| payload.get("script_timeout_seconds"))
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_SCRIPT_TIMEOUT_SECS)
}

/// A cron script with no timeout of its own. 30s killed report and sync scripts that take a
/// few minutes (2026-10-23/24 cron failures).
pub const DEFAULT_SCRIPT_TIMEOUT_SECS: u64 = 300;

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

async fn monitor_hash(
    pool: &sqlx::SqlitePool,
    job_id: &str,
    payload: &Value,
    cwd: Option<&Path>,
    model: &str,
) -> Result<Monitor> {
    let script = payload.get("monitor_script").and_then(Value::as_str);
    let Some(script) = script else {
        return Ok(Monitor::Absent);
    };
    let output = run_script(script, timeout_secs(payload), cwd, Some(model)).await?;
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

async fn script_working_directory(pool: &sqlx::SqlitePool) -> Option<std::path::PathBuf> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = 'omo_working_directory'")
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
}

async fn run_script(
    script: &str,
    timeout_secs: u64,
    cwd: Option<&Path>,
    model: Option<&str>,
) -> Result<String> {
    let mut command = crate::child_env::command("sh");
    command.arg("-c").arg(script);
    if let Some(model) = model {
        command.env(CRON_MODEL_ENV, model);
    }
    if let Some(cwd) = cwd.filter(|cwd| !cwd.as_os_str().is_empty()) {
        command.current_dir(cwd);
    }
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        command.output(),
    )
    .await
    .map_err(|_| HubError::message("cron script timed out"))?
    .map_err(|error| HubError::message(error.to_string()))?;
    if !output.status.success() {
        return Err(HubError::message(format!(
            "cron script failed with {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if let Some(line) = missing_command_line(&stdout).or_else(|| missing_command_line(&stderr)) {
        // A script that catches its own failure still exits 0 (omocat alerts printed
        // "bun: command not found: zele" and was recorded as succeeded for hours). A missing
        // program is an environment failure, so the run is failed and its output is not
        // delivered.
        return Err(HubError::message(format!(
            "cron script reported a missing command: {line}"
        )));
    }
    Ok(stdout)
}

/// First line that reports a program the shell could not find, as sh, bash, zsh and bun print it.
pub fn missing_command_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| {
            let lower = line.to_ascii_lowercase();
            lower.contains("command not found")
                || (lower.contains(": not found") && lower.starts_with("sh:"))
        })
        .map(|line| line.chars().take(300).collect())
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
    fn a_job_model_is_its_valid_override_else_the_background_model() {
        let background = crate::model_policy::background_model();
        assert_eq!(job_model(&serde_json::json!({"script": "x"})), background);
        assert_eq!(
            job_model(&serde_json::json!({"model": "mahoquot/gemini-3.8-flash-high"})),
            background
        );
        assert_eq!(
            job_model(&serde_json::json!({"model_override": " mahoquot/claude-sonnet-5-5 "})),
            "mahoquot/claude-sonnet-5-5"
        );
        assert_eq!(
            job_model(&serde_json::json!({"model_override": "sonnet"})),
            background
        );
        assert!(validate_model_id("a b/c").is_err());
        assert!(validate_model_id("/c").is_err());
        assert!(validate_model_id("mahoquot/").is_err());
    }

    #[tokio::test]
    async fn a_script_and_its_monitor_see_the_job_model() {
        let pool = crate::storage::memory().await.unwrap();
        let background = crate::model_policy::background_model();
        let plain = run_payload(
            &pool,
            "plain",
            &serde_json::json!({"no_agent": true, "script": "printf %s \"$FERRYX_CRON_MODEL\""}),
            &StaticModel(String::new()),
            None,
        )
        .await
        .unwrap();
        assert_eq!(plain.output, background);
        let chosen = run_payload(
            &pool,
            "chosen",
            &serde_json::json!({
                "no_agent": true,
                "model_override": "cb/deepseek-v4.1-flash",
                "script": "printf %s \"$FERRYX_CRON_MODEL\""
            }),
            &StaticModel(String::new()),
            None,
        )
        .await
        .unwrap();
        assert_eq!(chosen.output, "cb/deepseek-v4.1-flash");
    }

    #[tokio::test]
    async fn the_model_override_is_set_and_cleared_without_touching_other_keys() {
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO scheduled_jobs(id, expression, payload_json, enabled, created_at, updated_at) VALUES('j', '0 * * * *', ?, 1, 'now', 'now')")
            .bind(r#"{"script":"true","model":"mahoquot/gpt-6-luna","last_status":"succeeded"}"#)
            .execute(&pool)
            .await
            .unwrap();
        let stored = |pool: sqlx::SqlitePool| async move {
            let json: String =
                sqlx::query_scalar("SELECT payload_json FROM scheduled_jobs WHERE id = 'j'")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            serde_json::from_str::<Value>(&json).unwrap()
        };
        assert_eq!(
            set_model_override(&pool, "j", Some("mahoquot/claude-sonnet-5-5"))
                .await
                .unwrap()
                .as_deref(),
            Some("mahoquot/claude-sonnet-5-5")
        );
        let payload = stored(pool.clone()).await;
        assert_eq!(payload["model_override"], "mahoquot/claude-sonnet-5-5");
        assert_eq!(payload["last_status"], "succeeded");
        assert_eq!(payload["script"], "true");
        assert!(set_model_override(&pool, "j", Some("sonnet"))
            .await
            .is_err());
        assert_eq!(
            set_model_override(&pool, "j", None).await.unwrap(),
            Some(crate::model_policy::background_model())
        );
        assert!(stored(pool.clone()).await.get("model_override").is_none());
        assert_eq!(
            set_model_override(&pool, "missing", None).await.unwrap(),
            None
        );
    }

    #[test]
    fn a_script_without_a_timeout_gets_five_minutes() {
        assert_eq!(timeout_secs(&serde_json::json!({"script": "x"})), 300);
        assert_eq!(timeout_secs(&serde_json::json!({"timeout_secs": 30})), 30);
    }

    #[test]
    fn missing_commands_are_found_in_script_output() {
        assert!(missing_command_line("ok\n").is_none());
        assert!(missing_command_line("found nothing to do\n").is_none());
        assert_eq!(
            missing_command_line(
                "x\n⚠️ 캘린더 확인 실패: zele cal events failed: bun: command not found: zele\n"
            )
            .as_deref(),
            Some("⚠️ 캘린더 확인 실패: zele cal events failed: bun: command not found: zele")
        );
        assert!(missing_command_line("zsh: command not found: bun").is_some());
        assert!(missing_command_line("sh: 1: bun: not found").is_some());
    }

    #[tokio::test]
    async fn a_script_that_hides_a_missing_command_behind_exit_zero_fails() {
        let error = run_script(
            "echo 'alerts: bun: command not found: zele'; exit 0",
            10,
            None,
            None,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("missing command"), "{error}");
        assert_eq!(
            run_script("printf fine", 10, None, None).await.unwrap(),
            "fine"
        );
    }

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
    async fn a_relative_script_runs_in_the_imported_working_directory() {
        let dir = std::env::temp_dir().join(format!(
            "ferryx-cwd-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("token"), "from-omo").unwrap();
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO hub_meta(key, value) VALUES('omo_working_directory', ?)")
            .bind(dir.to_string_lossy().as_ref())
            .execute(&pool)
            .await
            .unwrap();
        let executed = run_payload(
            &pool,
            "rel",
            &serde_json::json!({"command": "cat token", "no_agent": true}),
            &StaticModel(String::new()),
            None,
        )
        .await
        .unwrap();
        assert_eq!(executed.output, "from-omo");
        let _ = std::fs::remove_dir_all(&dir);
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

    struct Routed(std::sync::Mutex<(String, Vec<String>)>);

    #[async_trait]
    impl ModelTurn for Routed {
        async fn complete(&self, _prompt: &str) -> Result<String> {
            Ok("unused".into())
        }

        async fn complete_with(
            &self,
            prompt: &str,
            model: Option<&str>,
            toolsets: &[String],
        ) -> Result<String> {
            *self.0.lock().unwrap() = (model.unwrap_or("").to_owned(), toolsets.to_vec());
            assert!(prompt.contains("[Script output]"));
            assert!(prompt.contains("rows=3"));
            Ok("report".into())
        }
    }

    #[tokio::test]
    async fn live_job_shapes_match_the_gateway() {
        let pool = crate::storage::memory().await.unwrap();
        let model = StaticModel("should-not-run".into());
        let missing = run_payload(
            &pool,
            "bare",
            &serde_json::json!({"no_agent": true, "deliver": "local"}),
            &model,
            None,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(missing.contains("no_agent=true but has no script"));
        let failed = run_payload(
            &pool,
            "scripted",
            &serde_json::json!({"no_agent": true, "script": "echo nope 1>&2; exit 3", "timeout_seconds": 30}),
            &model,
            None,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(failed.contains("cron script failed with Some(3)"));
        let skipped = run_payload(
            &pool,
            "empty-script",
            &serde_json::json!({"script": "true", "prompt": "write the report", "timeout_secs": 30}),
            &model,
            None,
        )
        .await
        .unwrap();
        assert!(!skipped.deliver);
        assert!(skipped.output.is_empty());
        let routed = Routed(std::sync::Mutex::new((String::new(), Vec::new())));
        let agent = run_payload(
            &pool,
            "vpn-mall",
            &serde_json::json!({
                "script": "printf rows=3",
                "prompt": "publish",
                "model": "mahoquot/gemini-3.8-flash-high",
                "enabled_toolsets": ["terminal", "browser", "file"],
                "deliver": "slack:C0C9XG5FP8W",
                "schedule": {"kind": "cron", "expr": "0 9 * * *", "timezone": "Asia/Seoul"}
            }),
            &routed,
            None,
        )
        .await
        .unwrap();
        assert_eq!(agent.output, "report");
        let (model_id, toolsets) = routed.0.lock().unwrap().clone();
        assert_eq!(model_id, crate::model_policy::background_model());
        assert_ne!(model_id, "mahoquot/gemini-3.8-flash-high");
        assert_eq!(toolsets, vec!["terminal", "browser", "file"]);
        let chosen = run_payload(
            &pool,
            "vpn-mall",
            &serde_json::json!({
                "script": "printf rows=3",
                "prompt": "publish",
                "model": "mahoquot/gemini-3.8-flash-high",
                "model_override": "mahoquot/claude-sonnet-5-5"
            }),
            &routed,
            None,
        )
        .await
        .unwrap();
        assert_eq!(chosen.output, "report");
        assert_eq!(routed.0.lock().unwrap().0, "mahoquot/claude-sonnet-5-5");
        let timed = run_payload(
            &pool,
            "slow",
            &serde_json::json!({"no_agent": true, "script": "sleep 5", "timeout_seconds": 1}),
            &model,
            None,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(timed.contains("timed out"));
    }

    #[tokio::test]
    async fn the_inbox_digest_job_lists_owner_work_and_skips_a_second_run_today() {
        let pool = crate::storage::memory().await.unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO inbox_items(id, source, source_ref, title, snippet, received_at, raw_hash, collected_at) VALUES('m1','mail','r','Sign','please',?, 'h', ?)",
        )
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO inbox_decisions(item_id, class, reason, status, decided_at) VALUES('m1','owner','needs you','open',?)",
        )
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        let payload = serde_json::json!({
            "deliver": "origin",
            "attach_to_session": false,
            "origin": {"platform": "slack", "chat_id": "D0C9HNVUK5M", "user_id": "U04", "bot_id": "B0"},
            "schedule": {"kind": "cron", "expr": "0 9 * * *", "timezone": "Asia/Seoul"}
        });
        let model = StaticModel("no".into());
        let first = run_payload(&pool, "omon:inbox-digest", &payload, &model, None)
            .await
            .unwrap();
        assert!(first.deliver);
        assert!(first.output.contains("## 대표 확인 필요 — 1곳, 1건"));
        assert!(first.output.contains("Sign"));
        assert!(first
            .output
            .contains("어제: jev 0회(폴백 0, 상한 도달 false)"));
        sqlx::query(
            "INSERT INTO cron_runs(id, job_id, status, started_at, finished_at, output) VALUES('d1','omon:inbox-digest','succeeded',?,?,?)",
        )
        .bind(&now)
        .bind(&now)
        .bind(&first.output)
        .execute(&pool)
        .await
        .unwrap();
        let second = run_payload(&pool, "omon:inbox-digest", &payload, &model, None)
            .await
            .unwrap();
        assert!(!second.deliver);
        assert!(second.output.is_empty());
    }
}
