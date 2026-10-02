use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::scoped_contracts::TargetRef;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoveryRecord {
    pub logical_session_id: String,
    pub host: String,
    pub exact_previous_target: TargetRef,
    pub boot_id: String,
    pub project_id: String,
    pub project_root: PathBuf,
    pub worktree: Option<String>,
    pub cwd: PathBuf,
    pub cols: u16,
    pub rows: u16,
    pub agent: Option<String>,
    pub provider_session: Option<Value>,
    pub disabled: bool,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreLaunchMarker {
    pub logical_session_id: String,
    pub attempt_id: String,
    pub host: String,
    pub boot_id: String,
    pub source_target: TargetRef,
    pub provider_key: Option<String>,
    pub timestamp: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompletionReceipt {
    pub logical_session_id: String,
    pub attempt_id: String,
    pub source_target: TargetRef,
    pub target: TargetRef,
    pub pid: u32,
    pub boot_id: String,
    pub provider_key: Option<String>,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResumedCommand {
    pub program: String,
    pub args: Vec<String>,
    pub provider_key: String,
}

pub struct RecoveryStore {
    base_dir: PathBuf,
    host: String,
    host_dir: PathBuf,
    sessions_dir: PathBuf,
    markers_dir: PathBuf,
    receipts_dir: PathBuf,
    provider_receipts_dir: PathBuf,
    provider_markers_dir: PathBuf,
    active_recovery_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl RecoveryStore {
    pub fn new(base_dir: PathBuf, host: &str) -> Result<Self, String> {
        let host_hash = hash_identifier(host);
        let host_dir = base_dir.join("hosts").join(format!("h_{host_hash}"));
        let sessions_dir = host_dir.join("sessions");
        let markers_dir = host_dir.join("in_progress");
        let receipts_dir = host_dir.join("receipts");
        let provider_receipts_dir = host_dir.join("provider_receipts");
        let provider_markers_dir = host_dir.join("provider_markers");

        ensure_private_directory(&base_dir)?;
        ensure_private_directory(&host_dir)?;
        ensure_private_directory(&sessions_dir)?;
        ensure_private_directory(&markers_dir)?;
        ensure_private_directory(&receipts_dir)?;
        ensure_private_directory(&provider_receipts_dir)?;
        ensure_private_directory(&provider_markers_dir)?;

        let lock_path = host_dir.join("host.lock");
        if lock_path.symlink_metadata().is_ok() {
            super::validate_private(&lock_path)?;
        }

        let mut opts = OpenOptions::new();
        opts.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let lock_file = opts
            .open(&lock_path)
            .map_err(|e| format!("CANNOT_OPEN_HOST_LOCK: {e}"))?;
        super::private_file(&lock_path)?;
        drop(lock_file);

        Ok(Self {
            base_dir,
            host: host.to_string(),
            host_dir,
            sessions_dir,
            markers_dir,
            receipts_dir,
            provider_receipts_dir,
            provider_markers_dir,
            active_recovery_locks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Always acquire this cross-process lock before any logical-session mutex.
    /// Closing this independent handle releases only this operation's lock.
    pub fn transaction(&self) -> Result<File, String> {
        let path = self.host_dir.join("host.lock");
        super::validate_private(&path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(&path).map_err(|e| format!("CANNOT_OPEN_HOST_LOCK: {e}"))?;
        file.lock().map_err(|e| format!("CANNOT_LOCK_RECOVERY_TRANSACTION: {e}"))?;
        Ok(file)
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    fn session_file_path(&self, logical_id: &str) -> Result<PathBuf, String> {
        validate_logical_session_id(logical_id)?;
        let digest = hash_identifier(logical_id);
        Ok(self.sessions_dir.join(format!("s_{digest}.json")))
    }

    fn marker_file_path(&self, logical_id: &str) -> Result<PathBuf, String> {
        validate_logical_session_id(logical_id)?;
        let digest = hash_identifier(logical_id);
        Ok(self.markers_dir.join(format!("m_{digest}.json")))
    }

    fn receipt_file_path(&self, logical_id: &str) -> Result<PathBuf, String> {
        validate_logical_session_id(logical_id)?;
        let digest = hash_identifier(logical_id);
        Ok(self.receipts_dir.join(format!("r_{digest}.json")))
    }

    fn provider_receipt_file_path(&self, provider_key: &str) -> Result<PathBuf, String> {
        let digest = hash_identifier(provider_key);
        Ok(self.provider_receipts_dir.join(format!("pr_{digest}.json")))
    }

    fn provider_marker_file_path(&self, provider_key: &str) -> Result<PathBuf, String> {
        let digest = hash_identifier(provider_key);
        Ok(self.provider_markers_dir.join(format!("pm_{digest}.json")))
    }

    pub fn get_recovery_lock(&self, logical_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self.active_recovery_locks.lock().unwrap();
        locks
            .entry(logical_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    pub fn save_record(&self, record: &RecoveryRecord) -> Result<(), String> {
        let _transaction = self.transaction()?;
        let lock = self.get_recovery_lock(&record.logical_session_id);
        let _guard = lock.lock().map_err(|e| e.to_string())?;
        if let Some(previous) = self.load_record_unlocked(&record.logical_session_id)? {
            if previous.exact_previous_target != record.exact_previous_target {
                return Err("REQUEST_CONFLICT: logical session already has a recovery identity".into());
            }
        }
        self.save_record_unlocked(record)
    }

    pub fn save_record_unlocked(&self, record: &RecoveryRecord) -> Result<(), String> {
        let path = self.session_file_path(&record.logical_session_id)?;
        atomic_write_synced(&path, record)
    }

    pub fn load_record(&self, logical_id: &str) -> Result<Option<RecoveryRecord>, String> {
        let _transaction = self.transaction()?;
        let lock = self.get_recovery_lock(logical_id);
        let _guard = lock.lock().map_err(|e| e.to_string())?;
        self.load_record_unlocked(logical_id)
    }

    pub fn load_record_unlocked(&self, logical_id: &str) -> Result<Option<RecoveryRecord>, String> {
        let path = self.session_file_path(logical_id)?;
        if !path.exists() {
            return Ok(None);
        }
        super::validate_private(&path)?;
        let content = std::fs::read(&path)
            .map_err(|e| format!("FAILED_TO_READ_RECOVERY_RECORD: {e}"))?;
        let record: RecoveryRecord = serde_json::from_slice(&content)
            .map_err(|e| format!("CORRUPT_RECOVERY_RECORD: {e}"))?;
        if record.logical_session_id != logical_id || record.host != self.host
            || record.exact_previous_target.host_id != self.host
        {
            return Err("CORRUPT_RECOVERY_RECORD: logical session or host mismatch".into());
        }
        Ok(Some(record))
    }

    pub fn update_agent_state_for_target(
        &self,
        logical_id: &str,
        expected_target: &TargetRef,
        agent: Option<String>,
        provider_session: Option<Value>,
    ) -> Result<(), String> {
        let _transaction = self.transaction()?;
        let lock = self.get_recovery_lock(logical_id);
        let _guard = lock.lock().map_err(|e| e.to_string())?;

        let mut record = match self.load_record_unlocked(logical_id)? {
            Some(r) => r,
            None => {
                return Err(format!(
                    "RECOVERY_RECORD_MISSING: cannot update agent state for nonexistent logical session {logical_id}"
                ));
            }
        };

        if record.disabled || record.exact_previous_target != *expected_target {
            return Err("TARGET_EXPIRED: agent report belongs to a retired execution".into());
        }
        // Heartbeats without identity must not erase the last authenticated recipe.
        if let Some(agent) = agent {
            match provider_session {
                Some(provider) => {
                    if let Err(error) = extract_provider_session(&provider) {
                        record.agent = Some(agent);
                        record.provider_session = None;
                        record.updated_at = current_timestamp()?;
                        self.save_record_unlocked(&record)?;
                        return Err(error);
                    }
                    record.agent = Some(agent);
                    record.provider_session = Some(provider);
                }
                None if record.agent.as_deref() != Some(agent.as_str()) => {
                    record.agent = Some(agent);
                    record.provider_session = None;
                }
                None => {}
            }
        }
        record.updated_at = current_timestamp()?;
        self.save_record_unlocked(&record)
    }

    pub fn mark_disabled_for_target(&self, logical_id: &str, target: &TargetRef) -> Result<(), String> {
        let _transaction = self.transaction()?;
        let lock = self.get_recovery_lock(logical_id);
        let _guard = lock.lock().map_err(|e| e.to_string())?;

        let mut record = match self.load_record_unlocked(logical_id)? {
            Some(r) => r,
            None => return Ok(()),
        };
        if record.exact_previous_target != *target {
            return Err("TARGET_EXPIRED: stop belongs to a retired execution".into());
        }
        record.disabled = true;
        record.updated_at = current_timestamp()?;
        self.save_record_unlocked(&record)
    }

    #[cfg(test)]
    pub fn mark_disabled(&self, logical_id: &str) -> Result<(), String> {
        let record = self.load_record(logical_id)?.ok_or("RECOVERY_RECORD_MISSING")?;
        self.mark_disabled_for_target(logical_id, &record.exact_previous_target)
    }

    #[cfg(test)]
    pub fn update_agent_state(
        &self,
        logical_id: &str,
        agent: Option<String>,
        provider_session: Option<Value>,
    ) -> Result<(), String> {
        let record = self.load_record(logical_id)?.ok_or("RECOVERY_RECORD_MISSING")?;
        self.update_agent_state_for_target(logical_id, &record.exact_previous_target, agent, provider_session)
    }

    pub fn record_pre_launch(&self, marker: &PreLaunchMarker) -> Result<(), String> {
        let path = self.marker_file_path(&marker.logical_session_id)?;
        atomic_write_synced(&path, marker)?;
        if let Some(pk) = &marker.provider_key {
            let p_path = self.provider_marker_file_path(pk)?;
            atomic_write_synced(&p_path, marker)?;
        }
        Ok(())
    }

    pub fn load_pre_launch(&self, logical_id: &str) -> Result<Option<PreLaunchMarker>, String> {
        let path = self.marker_file_path(logical_id)?;
        if !path.exists() {
            return Ok(None);
        }
        super::validate_private(&path)?;
        let content = std::fs::read(&path)
            .map_err(|e| format!("FAILED_TO_READ_PRE_LAUNCH: {e}"))?;
        let marker: PreLaunchMarker = serde_json::from_slice(&content)
            .map_err(|e| format!("CORRUPT_PRE_LAUNCH: {e}"))?;
        if marker.logical_session_id != logical_id {
            return Err("CORRUPT_PRE_LAUNCH: logical session ID mismatch".into());
        }
        Ok(Some(marker))
    }

    pub fn check_provider_pre_launch(&self, provider_key: &str) -> Result<Option<PreLaunchMarker>, String> {
        let path = self.provider_marker_file_path(provider_key)?;
        if !path.exists() {
            return Ok(None);
        }
        super::validate_private(&path)?;
        let content = std::fs::read(&path)
            .map_err(|e| format!("FAILED_TO_READ_PROVIDER_PRE_LAUNCH: {e}"))?;
        let marker: PreLaunchMarker = serde_json::from_slice(&content)
            .map_err(|e| format!("CORRUPT_PROVIDER_PRE_LAUNCH: {e}"))?;
        if marker.provider_key.as_deref() != Some(provider_key)
            || marker.host != self.host || marker.source_target.host_id != self.host
        {
            return Err("CORRUPT_PROVIDER_PRE_LAUNCH: provider or host mismatch".into());
        }
        Ok(Some(marker))
    }

    pub fn remove_pre_launch(&self, logical_id: &str, provider_key: Option<&str>) -> Result<(), String> {
        let path = self.marker_file_path(logical_id)?;
        if path.exists() {
            super::validate_private(&path)?;
            std::fs::remove_file(&path)
                .map_err(|e| format!("FAILED_TO_REMOVE_PRE_LAUNCH: {e}"))?;
            sync_parent(&path)?;
        }
        if let Some(pk) = provider_key {
            let p_path = self.provider_marker_file_path(pk)?;
            if p_path.exists() {
                super::validate_private(&p_path)?;
                std::fs::remove_file(&p_path)
                    .map_err(|e| format!("FAILED_TO_REMOVE_PROVIDER_PRE_LAUNCH: {e}"))?;
                sync_parent(&p_path)?;
            }
        }
        Ok(())
    }

    pub fn record_completion(&self, receipt: &CompletionReceipt) -> Result<(), String> {
        let path = self.receipt_file_path(&receipt.logical_session_id)?;
        atomic_write_synced(&path, receipt)?;
        if let Some(pk) = &receipt.provider_key {
            let p_path = self.provider_receipt_file_path(pk)?;
            atomic_write_synced(&p_path, receipt)?;
        }
        self.remove_pre_launch(&receipt.logical_session_id, receipt.provider_key.as_deref())?;
        Ok(())
    }

    pub fn load_completion(&self, logical_id: &str) -> Result<Option<CompletionReceipt>, String> {
        let path = self.receipt_file_path(logical_id)?;
        if !path.exists() {
            return Ok(None);
        }
        super::validate_private(&path)?;
        let content = std::fs::read(&path)
            .map_err(|e| format!("FAILED_TO_READ_COMPLETION: {e}"))?;
        let receipt: CompletionReceipt = serde_json::from_slice(&content)
            .map_err(|e| format!("CORRUPT_COMPLETION: {e}"))?;
        if receipt.logical_session_id != logical_id || receipt.target.host_id != self.host
            || receipt.source_target.host_id != self.host
        {
            return Err("CORRUPT_COMPLETION: logical session or host mismatch".into());
        }
        Ok(Some(receipt))
    }

    pub fn load_completion_by_provider(&self, provider_key: &str) -> Result<Option<CompletionReceipt>, String> {
        let path = self.provider_receipt_file_path(provider_key)?;
        if !path.exists() {
            return Ok(None);
        }
        super::validate_private(&path)?;
        let content = std::fs::read(&path)
            .map_err(|e| format!("FAILED_TO_READ_PROVIDER_COMPLETION: {e}"))?;
        let receipt: CompletionReceipt = serde_json::from_slice(&content)
            .map_err(|e| format!("CORRUPT_PROVIDER_COMPLETION: {e}"))?;
        if receipt.provider_key.as_deref() != Some(provider_key)
            || receipt.target.host_id != self.host || receipt.source_target.host_id != self.host
        {
            return Err("CORRUPT_PROVIDER_COMPLETION: provider or host mismatch".into());
        }
        Ok(Some(receipt))
    }
}

pub fn resolve_resume_command(
    record: &RecoveryRecord,
    cwd: &Path,
) -> Result<ResumedCommand, String> {
    let agent_name = record
        .agent
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "REMOTE_RECOVERY_UNSUPPORTED: missing authoritative agent reference".to_string()
        })?;

    let provider = record.provider_session.as_ref().ok_or_else(|| {
        "REMOTE_RECOVERY_UNSUPPORTED: missing providerSession in recovery record".to_string()
    })?;

    let (session_id, transcript_path) = extract_provider_session(provider)?;
    validate_agent_session_id(&session_id)?;

    verify_agent_transcript_exists(agent_name, &session_id, transcript_path.as_deref(), cwd)?;

    let provider_key = format!("{}:{session_id}", agent_name.to_ascii_lowercase());

    match agent_name.to_ascii_lowercase().as_str() {
        "omo" => Ok(ResumedCommand {
            program: "omo".to_string(),
            args: vec!["--session".to_string(), session_id],
            provider_key,
        }),
        "claude" => Ok(ResumedCommand {
            program: "claude".to_string(),
            args: vec!["--resume".to_string(), session_id],
            provider_key,
        }),
        "codex" => Ok(ResumedCommand {
            program: "codex".to_string(),
            args: vec!["resume".to_string(), session_id],
            provider_key,
        }),
        other => Err(format!(
            "REMOTE_RECOVERY_UNSUPPORTED: agent {other} is not supported for reboot recovery"
        )),
    }
}

pub fn extract_provider_session(val: &Value) -> Result<(String, Option<String>), String> {
    let map = val
        .as_object()
        .ok_or_else(|| "REMOTE_RECOVERY_INVALID: providerSession must be an object".to_string())?;

    let key = map
        .get("key")
        .and_then(Value::as_str)
        .map(str::trim)
        .ok_or_else(|| "REMOTE_RECOVERY_INVALID: providerSession missing key field".to_string())?;

    if !key.eq_ignore_ascii_case("session_id") && !key.eq_ignore_ascii_case("sessionId") {
        return Err(format!(
            "REMOTE_RECOVERY_INVALID: providerSession key must be session_id, got: {key}"
        ));
    }

    let id = map
        .get("id")
        .or_else(|| map.get("sessionId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .ok_or_else(|| "REMOTE_RECOVERY_INVALID: providerSession missing id field".to_string())?;

    if id.is_empty() {
        return Err("REMOTE_RECOVERY_INVALID: providerSession id cannot be empty".into());
    }

    let transcript = map
        .get("transcriptPath")
        .or_else(|| map.get("transcript_path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);

    Ok((id.to_string(), transcript))
}

pub fn verify_agent_transcript_exists(
    agent_name: &str,
    session_id: &str,
    transcript_path: Option<&str>,
    cwd: &Path,
) -> Result<PathBuf, String> {
    if agent_name.eq_ignore_ascii_case("omo") {
        return verify_omo_transcript_exists(session_id, transcript_path, cwd);
    }

    if let Some(path_str) = transcript_path {
        let path = Path::new(path_str);
        if !path.exists() {
            return Err(format!(
                "REMOTE_RECOVERY_REFUSED: transcript path does not exist for agent {agent_name}: {path_str}"
            ));
        }
        validate_journal_path(path)?;
        use std::io::{BufRead, BufReader};
        let mut first_line = String::new();
        BufReader::new(File::open(path).map_err(|e| e.to_string())?)
            .read_line(&mut first_line).map_err(|e| e.to_string())?;
        let header: Value = serde_json::from_str(&first_line)
            .map_err(|e| format!("REMOTE_RECOVERY_REFUSED: invalid provider journal header: {e}"))?;
        let matches = match agent_name.to_ascii_lowercase().as_str() {
            "claude" => header.get("sessionId").and_then(Value::as_str) == Some(session_id),
            "codex" => header.get("type").and_then(Value::as_str) == Some("session_meta")
                && header.pointer("/payload/id").and_then(Value::as_str) == Some(session_id),
            _ => return Err("REMOTE_RECOVERY_UNSUPPORTED: provider has no verified journal adapter".into()),
        };
        if !matches {
            return Err("REMOTE_RECOVERY_REFUSED: provider journal does not identify the requested conversation".into());
        }
        return Ok(path.to_path_buf());
    }

    Err(format!(
        "REMOTE_RECOVERY_UNSUPPORTED: verified session transcript reference on disk is required for agent {agent_name} recovery"
    ))
}

pub fn validate_recovery_directory(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("FORBIDDEN: recovery working directory must be a real directory".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("FORBIDDEN: recovery working directory cannot be a reparse point".into());
        }
    }
    Ok(())
}

pub fn resume_command_builder(program: &str, args: &[String]) -> Result<portable_pty::CommandBuilder, String> {
    let path = Path::new(program);
    let executable = if path.is_absolute() {
        path.canonicalize().map_err(|e| format!("REMOTE_RECOVERY_EXECUTABLE_MISSING: {e}"))?
    } else {
        let mut directories: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|value| std::env::split_paths(&value).filter(|path| path.is_absolute()).collect())
            .unwrap_or_default();
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
        if let Some(home) = home {
            let home = PathBuf::from(home);
            directories.extend([home.join(".bun/bin"), home.join(".local/bin"), home.join(".cargo/bin")]);
            #[cfg(windows)]
            directories.push(home.join("AppData/Roaming/npm"));
        }
        #[cfg(unix)]
        directories.extend([PathBuf::from("/usr/local/bin"), PathBuf::from("/opt/homebrew/bin")]);
        #[cfg(windows)]
        let suffixes = [".exe", ".cmd", ".bat", ""];
        #[cfg(not(windows))]
        let suffixes = [""];
        directories.into_iter().flat_map(|directory| {
            suffixes.map(move |suffix| directory.join(format!("{program}{suffix}")))
        }).find(|candidate| candidate.is_file())
            .ok_or_else(|| format!("REMOTE_RECOVERY_EXECUTABLE_MISSING: {program} is not installed"))?
            .canonicalize().map_err(|e| e.to_string())?
    };
    #[cfg(windows)]
    if executable.extension().and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat"))
    {
        let executable = super::prepare_spawn_cwd(&executable);
        let script = executable.to_str().ok_or("REMOTE_RECOVERY_INVALID: executable path is not UTF-8")?;
        if script.contains(['"', '%', '!', '^', '&', '|', '<', '>', '(', ')', '\r', '\n'])
            || args.iter().any(|argument| argument.contains(['"', '%', '!', '^', '&', '|', '<', '>', '\r', '\n']))
        {
            return Err("REMOTE_RECOVERY_INVALID: unsafe command wrapper arguments".into());
        }
        let system_root = std::env::var_os("SystemRoot").ok_or("REMOTE_RECOVERY_EXECUTABLE_MISSING: SystemRoot unavailable")?;
        let mut command = portable_pty::CommandBuilder::new(PathBuf::from(system_root).join("System32").join("cmd.exe"));
        command.args(["/d", "/s", "/c", "call"]);
        command.arg(executable);
        command.args(args);
        return Ok(command);
    }
    let mut command = portable_pty::CommandBuilder::new(executable);
    command.args(args);
    Ok(command)
}

fn verify_omo_transcript_exists(
    session_id: &str,
    transcript_path: Option<&str>,
    cwd: &Path,
) -> Result<PathBuf, String> {
    if let Some(path_str) = transcript_path {
        let path = Path::new(path_str);
        if !path.exists() {
            return Err(format!(
                "REMOTE_RECOVERY_REFUSED: transcript path does not exist: {path_str}"
            ));
        }
        validate_journal_path(path)?;
        validate_transcript_file(path, session_id)?;
        return Ok(path.to_path_buf());
    }

    let mut search_roots = Vec::new();
    for env_var in ["OMO_CODING_AGENT_DIR", "SENPI_CODING_AGENT_DIR"] {
        if let Ok(dir) = std::env::var(env_var) {
            let p = PathBuf::from(dir).join("sessions");
            if p.is_dir() {
                search_roots.push(p);
            }
        }
    }
    for env_var in [
        "OMO_CODING_AGENT_SESSION_DIR",
        "SENPI_CODING_AGENT_SESSION_DIR",
    ] {
        if let Ok(dir) = std::env::var(env_var) {
            let p = PathBuf::from(dir);
            if p.is_dir() {
                search_roots.push(p);
            }
        }
    }
    #[cfg(windows)]
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let omo = PathBuf::from(profile).join(".omo");
        search_roots.push(omo.join("agent").join("sessions"));
        search_roots.push(omo.join("sessions"));
    }
    #[cfg(not(windows))]
    if let Ok(home) = std::env::var("HOME") {
        let omo = PathBuf::from(home).join(".omo");
        search_roots.push(omo.join("agent").join("sessions"));
        search_roots.push(omo.join("sessions"));
    }

    search_roots.push(cwd.join(".omo").join("sessions"));
    search_roots.push(cwd.join(".senpi").join("sessions"));

    let suffix = format!("_{session_id}.jsonl");
    let bare_suffix = format!("{session_id}.jsonl");
    let json_suffix = format!("{session_id}.json");

    for root in search_roots {
        if !root.is_dir() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&root) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    if let Ok(sub_entries) = std::fs::read_dir(&p) {
                        for sub in sub_entries.flatten() {
                            let sp = sub.path();
                            if matches_session_file(&sp, &suffix, &bare_suffix, &json_suffix) {
                                if validate_transcript_file(&sp, session_id).is_ok() {
                                    return Ok(sp);
                                }
                            }
                        }
                    }
                } else if matches_session_file(&p, &suffix, &bare_suffix, &json_suffix) {
                    if validate_transcript_file(&p, session_id).is_ok() {
                        return Ok(p);
                    }
                }
            }
        }
    }

    Err(format!(
        "REMOTE_RECOVERY_REFUSED: transcript for OMO session {session_id} does not exist on disk"
    ))
}

fn matches_session_file(path: &Path, suffix: &str, bare_suffix: &str, json_suffix: &str) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.ends_with(suffix) || name.ends_with(bare_suffix) || name.ends_with(json_suffix)
}

fn validate_transcript_file(path: &Path, expected_id: &str) -> Result<(), String> {
    use std::io::BufRead;
    validate_journal_path(path)?;
    let file = File::open(path).map_err(|e| format!("CANNOT_OPEN_TRANSCRIPT: {e}"))?;
    let mut reader = std::io::BufReader::new(file);
    let mut first_line = String::new();
    reader
        .read_line(&mut first_line)
        .map_err(|e| format!("CANNOT_READ_TRANSCRIPT: {e}"))?;
    if first_line.trim().is_empty() {
        return Err("EMPTY_TRANSCRIPT".into());
    }
    let val: Value = serde_json::from_str(&first_line)
        .map_err(|e| format!("INVALID_TRANSCRIPT_HEADER: {e}"))?;

    let kind = val.get("type").and_then(Value::as_str).unwrap_or("");
    let id = val
        .get("id")
        .or_else(|| val.get("sessionId"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if kind == "session" && id == expected_id {
        Ok(())
    } else {
        Err(format!(
            "TRANSCRIPT_ID_MISMATCH: expected {expected_id}, found {id}"
        ))
    }
}

fn validate_journal_path(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("REMOTE_RECOVERY_REFUSED: journal must be a regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o022 != 0 || metadata.nlink() != 1
        {
            return Err("REMOTE_RECOVERY_REFUSED: journal must be owned by this user and not writable by others".into());
        }
    }
    #[cfg(windows)]
    super::validate_private(path)?;
    Ok(())
}

fn validate_agent_session_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 128 {
        return Err("REMOTE_RECOVERY_INVALID: invalid agent session ID length".into());
    }
    if !id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
    {
        return Err("REMOTE_RECOVERY_INVALID: illegal characters in agent session ID".into());
    }
    Ok(())
}

fn validate_logical_session_id(id: &str) -> Result<(), String> {
    let trimmed = id.trim();
    if trimmed.is_empty() || trimmed.len() > 256 {
        return Err("INVALID_REQUEST: logical session ID invalid length".into());
    }
    Ok(())
}

fn hash_identifier(id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(id.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn ensure_private_directory(path: &Path) -> Result<(), String> {
    if path.symlink_metadata().is_ok() {
        super::validate_private(path)?;
        if !path.is_dir() {
            return Err(format!("FORBIDDEN: path is not a directory: {:?}", path));
        }
    } else {
        if let Some(parent) = path.parent() {
            if parent.symlink_metadata().is_ok() {
                let metadata = std::fs::symlink_metadata(parent).map_err(|e| e.to_string())?;
                if metadata.file_type().is_symlink() {
                    return Err("FORBIDDEN: recovery parent cannot be a symlink".into());
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err("FORBIDDEN: recovery parent cannot be a reparse point".into());
                    }
                }
            } else {
                ensure_private_directory(parent)?;
            }
        }
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path).map_err(|e| e.to_string())?;
        super::private_file(path)?;
        super::validate_private(path)?;
    }
    Ok(())
}

fn atomic_write_synced<T: Serialize>(dest_path: &Path, value: &T) -> Result<(), String> {
    if dest_path.symlink_metadata().is_ok() {
        super::validate_private(dest_path)?;
    }
    let parent = dest_path
        .parent()
        .ok_or_else(|| "DEST_HAS_NO_PARENT".to_string())?;
    ensure_private_directory(parent)?;

    let temp_name = format!(".tmp-atomic-{}.tmp", uuid::Uuid::new_v4());
    let temp_path = parent.join(temp_name);

    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| format!("FAILED_TO_SERIALIZE_RECORD: {e}"))?;

    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }

    let write_res = (|| -> Result<(), String> {
        let mut file = opts
            .open(&temp_path)
            .map_err(|e| format!("FAILED_TO_CREATE_TEMP_FILE: {e}"))?;
        super::private_file(&temp_path)?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| format!("FAILED_TO_WRITE_AND_SYNC_FILE: {e}"))?;
        drop(file);

        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            let temp_wide: Vec<u16> = temp_path.as_os_str().encode_wide().chain(Some(0)).collect();
            let dest_wide: Vec<u16> = dest_path.as_os_str().encode_wide().chain(Some(0)).collect();
            use windows_sys::Win32::Storage::FileSystem::{
                MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
            };
            // SAFETY: both UTF-16 buffers are NUL-terminated and live through this synchronous Win32 call.
            let res = unsafe {
                MoveFileExW(
                    temp_wide.as_ptr(),
                    dest_wide.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            };
            if res == 0 {
                return Err(format!(
                    "FAILED_TO_MOVE_FILE_ATOMIC: Windows error {}",
                    std::io::Error::last_os_error()
                ));
            }
        }

        #[cfg(not(windows))]
        {
            std::fs::rename(&temp_path, dest_path)
                .map_err(|e| format!("FAILED_TO_RENAME_TEMP_FILE: {e}"))?;
            sync_parent(dest_path)?;
        }

        super::private_file(dest_path)?;
        super::validate_private(dest_path)?;
        Ok(())
    })();

    if write_res.is_err() {
        if let Err(error) = std::fs::remove_file(&temp_path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!("Ferryx recovery temporary file cleanup failed: {error}");
            }
        }
    }
    write_res
}

fn sync_parent(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        let parent = path.parent().ok_or("DEST_HAS_NO_PARENT")?;
        File::open(parent).and_then(|file| file.sync_all())
            .map_err(|e| format!("FAILED_TO_SYNC_RECOVERY_DIRECTORY: {e}"))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(super) fn current_timestamp() -> Result<u64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| format!("RECOVERY_CLOCK_INVALID: {e}"))
}

pub fn default_production_storage_root() -> Result<PathBuf, String> {
    if let Ok(explicit) = std::env::var("FERRYX_RECOVERY_ROOT") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            let p = PathBuf::from(trimmed);
            if !p.is_absolute() {
                return Err("FERRYX_RECOVERY_ROOT must be an absolute path".into());
            }
            return Ok(p);
        }
    }

    #[cfg(windows)]
    {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            let trimmed = local_app_data.trim();
            if !trimmed.is_empty() {
                return Ok(PathBuf::from(trimmed).join("ferryx").join("recovery"));
            }
        }
        if let Ok(profile) = std::env::var("USERPROFILE") {
            let trimmed = profile.trim();
            if !trimmed.is_empty() {
                return Ok(PathBuf::from(trimmed).join(".ferryx").join("recovery"));
            }
        }
        Err("CANNOT_DETERMINE_USER_STORAGE: LOCALAPPDATA and USERPROFILE are not set".into())
    }

    #[cfg(not(windows))]
    {
        if let Ok(home) = std::env::var("HOME") {
            let trimmed = home.trim();
            if !trimmed.is_empty() {
                return Ok(PathBuf::from(trimmed).join(".ferryx").join("recovery"));
            }
        }
        Err("CANNOT_DETERMINE_USER_STORAGE: HOME environment variable is not set".into())
    }
}
