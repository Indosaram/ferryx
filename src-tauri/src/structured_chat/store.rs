//! The structured-chat session store: one owner per session, an append-only per-session journal
//! under the Ferryx data dir, and the long-poll event primitive the routes serve.
//!
//! Persistence contract: every session owns `<data>/structured-chat/<sessionId>.jsonl`. The
//! first line is the meta record; every later line is one stamped part. A gateway restart finds
//! the journals, rebuilds each session with state `ended` and no live provider, and the next
//! turn resumes the persisted thread through [`StructuredProvider::start`]. `rollback` and
//! approval answers rewrite the journal from the in-memory part list, so history stays exactly
//! what the routes serve.
//!
//! The journal lives under `crate::worktree::naming::data_dir_with_env` (`FERRYX_DATA_DIR`), the
//! same data dir every other Ferryx store uses. All journal IO is async (`tokio::fs`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch, RwLock};

use super::acp::AcpProvider;
use super::antigravity::AntigravityProvider;
use super::claude::ClaudeProvider;
use super::codex::CodexProvider;
use super::maho_rpc::MahoRpcProvider;
use super::revert_files::{self, TurnSnapshot};
use super::types::{
    ApprovalDecision, Capabilities, CreateSessionBody, EditMessageBody, ExtensionUiResponseBody,
    ForkResult, MediaBlob, ModelPreferencesBody, NavigateTreeBody, PartKind, PermissionMode,
    SessionSettings, SessionState, SessionStats, SessionSummary, SessionTree, SlashCommandItem,
    SteerBody, StructuredChatError, StructuredPart, TurnBody,
};
use super::{PartDraft, ProviderEvent, StructuredProvider};

/// How long the events long-poll may wait for a new part before returning the empty page.
pub const EVENTS_LONG_POLL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

/// The environment variable that names the roots a session cwd must live under, as a path list.
const CWD_ROOTS_ENV: &str = "FERRYX_STRUCTURED_CHAT_ROOTS";

/// Page size `history` uses when the caller names none, and the largest page it will serve.
pub const HISTORY_PAGE_DEFAULT: usize = 200;
pub const HISTORY_PAGE_MAX: usize = 1000;

/// One `history` read: a forward cursor (`after`), a backwards cursor (`before`), and a bound.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryQuery {
    #[serde(default)]
    pub after: Option<u64>,
    #[serde(default)]
    pub before: Option<u64>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// One bounded history page plus the cursor that walks further back.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub parts: Vec<StructuredPart>,
    /// Older parts exist below this page; the transcript draws its "load older" control.
    pub has_older: bool,
    /// Pass back as `?before=` to fetch the page just below this one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_before: Option<u64>,
    /// The session held more parts than `limit`, so the page is a window, not the whole journal.
    pub truncated: bool,
}

/// The per-session journal, one JSONL record per line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
enum JournalRecord {
    Meta {
        session_id: String,
        provider: String,
        cwd: String,
        thread_id: Option<String>,
        settings: SessionSettings,
        state: SessionState,
    },
    Part {
        part: StructuredPart,
    },
}

struct SessionInner {
    /// Shared, so a route can clone it out and call the provider without holding the session
    /// mutex: a provider round trip must never block a history read.
    provider: Option<Arc<dyn StructuredProvider>>,
    thread_id: Option<String>,
    settings: SessionSettings,
    state: SessionState,
    parts: Vec<StructuredPart>,
    /// Highest `seq` already published to external readers (`history` / `wait_for_events`).
    /// Parts appended by `spawn_event_pump` stay invisible until their journal flush lands so
    /// readers never observe un-journaled parts before durability completes.
    published_seq: u64,
    /// False for ended sessions discovered at startup whose `.jsonl` parts are loaded lazily
    /// on first history/resume access instead of held in RAM forever.
    parts_loaded: bool,
    /// The next `seq` to hand out. Monotonic for the session's whole life: a rollback drops parts
    /// but never lowers this, so `seq` stays strictly increasing across a rollback.
    next_seq: u64,
    /// The provider turn id of every local turn, in order. `/clear` keeps this: the turns of the
    /// previous thread still own the parts they stamped, and a later rollback must not delete
    /// them.
    turn_ids: Vec<String>,
    turn_snapshots: HashMap<String, TurnSnapshot>,
    /// Where the current thread's turns begin in `turn_ids` (`/clear` moves it forward). Rollback
    /// never reaches below it: `thread/revert` only knows the live thread's turns.
    thread_first_turn: usize,
    /// A turn is being started: the provider round trip is in flight outside the mutex, so the
    /// reservation lives here. Without it two concurrent `start_turn` calls both see no active
    /// turn and both start one.
    turn_starting: bool,
    active_turn: Option<String>,
    /// The last turn the provider reported complete. A provider can finish a turn before
    /// `provider.turn` even returns (a scripted or very fast turn), and the pump cannot clear an
    /// `active_turn` that is not set yet; `start_turn` reads this so such a turn is not left
    /// looking in flight forever.
    last_completed_turn: Option<String>,
    events_tx: watch::Sender<u64>,
    journal_path: PathBuf,
}

impl SessionInner {
    fn summary(&self, session_id: &str, provider: &str, cwd: &Path) -> SessionSummary {
        SessionSummary {
            session_id: session_id.to_owned(),
            provider: provider.to_owned(),
            cwd: cwd.to_string_lossy().to_string(),
            state: self.state,
            settings: self.settings.clone(),
            // The in-flight turn, so a client enables Stop from the session row instead of
            // guessing from history.
            active_turn_id: self.active_turn.clone(),
        }
    }

    fn append_part_quiet(&mut self, draft: PartDraft) -> u64 {
        let seq = self.next_seq.max(1);
        self.next_seq = seq + 1;
        self.parts.push(StructuredPart {
            seq,
            turn_id: draft.turn_id,
            ts_ms: now_ms(),
            payload: draft.payload,
        });
        seq
    }

    fn append_part(&mut self, draft: PartDraft) -> u64 {
        let seq = self.append_part_quiet(draft);
        self.published_seq = self.published_seq.max(seq);
        let _ = self.events_tx.send_replace(seq);
        seq
    }

    fn visible_parts(&self) -> &[StructuredPart] {
        let end = self.parts.partition_point(|part| part.seq <= self.published_seq);
        &self.parts[..end]
    }

    fn has_parts_after(&self, after: u64) -> bool {
        self.visible_parts()
            .last()
            .is_some_and(|part| part.seq > after)
    }

    fn parts_after(&self, after: u64) -> Vec<StructuredPart> {
        let visible = self.visible_parts();
        let idx = visible.partition_point(|part| part.seq <= after);
        visible[idx..].to_vec()
    }

    fn all_parts_after(&self, after: u64) -> Vec<StructuredPart> {
        let idx = self.parts.partition_point(|part| part.seq <= after);
        self.parts[idx..].to_vec()
    }

    fn meta_record(&self, handle: &SessionHandle) -> JournalRecord {
        JournalRecord::Meta {
            session_id: handle.session_id.clone(),
            provider: handle.provider_id.clone(),
            cwd: handle.cwd.to_string_lossy().to_string(),
            thread_id: self.thread_id.clone(),
            settings: self.settings.clone(),
            state: self.state,
        }
    }
}

/// Persistent serialized journal writer held outside `SessionInner` so streaming
/// delta appends never hold the session state mutex across disk I/O or reopen
/// the file descriptor on every delta.
struct SessionJournal {
    path: PathBuf,
    writer: Option<tokio::io::BufWriter<tokio::fs::File>>,
    flushed_seq: u64,
}

impl SessionJournal {
    fn new(path: PathBuf) -> Self {
        Self::with_flushed_seq(path, 0)
    }

    fn with_flushed_seq(path: PathBuf, flushed_seq: u64) -> Self {
        Self {
            path,
            writer: None,
            flushed_seq,
        }
    }

    async fn append_parts(&mut self, parts: &[StructuredPart]) -> Result<(), StructuredChatError> {
        use tokio::io::AsyncWriteExt;
        if parts.is_empty() {
            return Ok(());
        }
        let mut encoded = Vec::with_capacity(parts.len());
        for part in parts {
            let line = serde_json::to_string(&JournalRecord::Part {
                part: part.clone(),
            })
            .map_err(|error| StructuredChatError::Provider(error.to_string()))?;
            encoded.push((part.seq, line));
        }
        if self.writer.is_none() {
            let file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .await
                .map_err(|error| StructuredChatError::Provider(error.to_string()))?;
            self.writer = Some(tokio::io::BufWriter::new(file));
        }
        let writer = self.writer.as_mut().expect("writer initialized");
        let write_result: std::io::Result<()> = async {
            for (_, line) in &encoded {
                writer.write_all(line.as_bytes()).await?;
                writer.write_all(b"\n").await?;
            }
            writer.flush().await
        }
        .await;
        if let Err(error) = write_result {
            self.writer = None;
            return Err(StructuredChatError::Provider(error.to_string()));
        }
        if let Some((last_seq, _)) = encoded.last() {
            self.flushed_seq = self.flushed_seq.max(*last_seq);
        }
        Ok(())
    }

    async fn rewrite(
        &mut self,
        meta: &JournalRecord,
        parts: &[StructuredPart],
        last_seq: u64,
    ) -> Result<(), StructuredChatError> {
        // Drop any open append handle before atomic rename so Windows allows
        // replacing the target file and Unix does not keep appending to an
        // unlinked inode.
        self.writer = None;
        rewrite_journal_file(&self.path, meta, parts).await?;
        self.flushed_seq = last_seq;
        Ok(())
    }
}

/// One structured-chat session: a live provider (or a resumable ended one) plus its journal.
///
/// Lock order invariant: whenever both locks are needed, ALWAYS acquire `journal` before `inner`,
/// take only a fast in-memory snapshot under `inner`, and drop `inner` before any disk I/O.
pub struct SessionHandle {
    session_id: String,
    provider_id: String,
    cwd: PathBuf,
    inner: tokio::sync::Mutex<SessionInner>,
    journal: tokio::sync::Mutex<SessionJournal>,
}

impl SessionHandle {
    async fn flush_pending_parts(&self) -> Result<(), StructuredChatError> {
        let mut journal = self.journal.lock().await;
        let pending = {
            let inner = self.inner.lock().await;
            inner.all_parts_after(journal.flushed_seq)
        };
        journal.append_parts(&pending).await?;
        let flushed = journal.flushed_seq;
        let mut inner = self.inner.lock().await;
        if flushed > inner.published_seq {
            inner.published_seq = flushed;
            let _ = inner.events_tx.send_replace(flushed);
        }
        Ok(())
    }

    async fn rewrite_journal_snapshot(&self) -> Result<(), StructuredChatError> {
        self.rewrite_journal_snapshot_with_end(false).await
    }

    async fn rewrite_journal_snapshot_with_end(
        &self,
        mark_ended: bool,
    ) -> Result<(), StructuredChatError> {
        let mut journal = self.journal.lock().await;
        let (meta, parts, last_seq) = {
            let inner = self.inner.lock().await;
            let last_seq = inner.parts.last().map_or(0, |part| part.seq);
            let mut meta = inner.meta_record(self);
            if mark_ended {
                if let JournalRecord::Meta { ref mut state, .. } = meta {
                    *state = SessionState::Ended;
                }
            }
            (meta, inner.parts.clone(), last_seq)
        };
        journal.rewrite(&meta, &parts, last_seq).await?;
        let mut inner = self.inner.lock().await;
        if mark_ended {
            inner.state = SessionState::Ended;
            inner.provider = None;
        }
        inner.published_seq = inner.published_seq.max(last_seq);
        let _ = inner.events_tx.send_replace(inner.published_seq);
        Ok(())
    }
}

/// The process-local structured-chat store.
pub struct StructuredChatStore {
    sessions: RwLock<HashMap<String, Arc<SessionHandle>>>,
    journal_dir: PathBuf,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

/// The journal directory, under the Ferryx data dir.
pub fn journal_dir() -> Option<PathBuf> {
    crate::worktree::naming::data_dir_with_env(|key| std::env::var_os(key))
        .map(|dir| dir.join("structured-chat"))
}

/// The cwd guard: an absolute, control-character-free path that exists and is a directory, the
/// same discipline the gateway applies to client-opened session directories.
///
/// On top of that shape check the path is jailed: a session may only run inside a directory this
/// process already trusts - the user's home, or a repository/worktree root the workspace registry
/// knows. A bare `/`, `/etc` or `/usr` passes every shape check and must still be refused: a
/// Control device could otherwise point an agent with full filesystem access at the whole disk.
fn validate_cwd(raw: &str) -> Result<PathBuf, StructuredChatError> {
    validate_cwd_in(raw, &allowed_cwd_roots())
}

/// The directory roots a structured-chat session may run in.
///
/// The user's home is the trust anchor (the same root the gateway's file routes expand `~`
/// against), and `FERRYX_STRUCTURED_CHAT_ROOTS` adds or replaces it with an explicit
/// path-list, the seam the tests and an embedding host use. A filesystem root is never a root
/// itself: it is exactly the jail escape.
fn allowed_cwd_roots() -> Vec<PathBuf> {
    if let Some(list) = std::env::var_os(CWD_ROOTS_ENV) {
        return std::env::split_paths(&list)
            .filter(|root| root.parent().is_some())
            .collect();
    }
    crate::ipc::file_link::home_dir()
        .into_iter()
        .filter(|home| home.parent().is_some())
        .collect()
}

fn validate_cwd_in(raw: &str, roots: &[PathBuf]) -> Result<PathBuf, StructuredChatError> {
    let raw = raw.trim();
    if raw.is_empty()
        || raw.len() > 4096
        || raw.chars().any(char::is_control)
        || !Path::new(raw).is_absolute()
    {
        return Err(StructuredChatError::InvalidRequest(
            "cwd must be an absolute directory path".into(),
        ));
    }
    let path = PathBuf::from(raw);
    let canonical = std::fs::canonicalize(&path).map_err(|_| {
        StructuredChatError::InvalidRequest("cwd must be an existing directory".into())
    })?;
    if !canonical.is_dir() {
        return Err(StructuredChatError::InvalidRequest(
            "cwd must be an existing directory".into(),
        ));
    }
    if canonical.parent().is_none() {
        return Err(StructuredChatError::InvalidRequest(
            "cwd must not be the filesystem root".into(),
        ));
    }
    if !roots.iter().any(|root| canonical.starts_with(root)) {
        return Err(StructuredChatError::InvalidRequest(
            "cwd must be inside the user's home or a known workspace".into(),
        ));
    }
    Ok(canonical)
}

/// Validates attachment paths up front, so a turn refuses before anything spawns.
fn validate_attachments(turn: &TurnBody) -> Result<Vec<String>, StructuredChatError> {
    let mut paths = Vec::new();
    for attachment in turn.attachments.iter().flatten() {
        let raw = attachment.path.trim();
        if raw.is_empty() || raw.chars().any(char::is_control) {
            return Err(StructuredChatError::InvalidRequest(
                "attachment path must be a plain path".into(),
            ));
        }
        let path = PathBuf::from(raw);
        let canonical = std::fs::canonicalize(&path).map_err(|_| {
            StructuredChatError::InvalidRequest(format!(
                "attachment does not exist: {}",
                attachment.path
            ))
        })?;
        paths.push(canonical.to_string_lossy().to_string());
    }
    Ok(paths)
}

impl StructuredChatStore {
    /// Opens the store and replays any journals a previous gateway process left behind.
    pub async fn open() -> Result<Arc<Self>, StructuredChatError> {
        let dir = journal_dir().ok_or_else(|| {
            StructuredChatError::Provider("no Ferryx data dir for structured-chat".into())
        })?;
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|error| StructuredChatError::Provider(error.to_string()))?;
        let scan_dir = dir.clone();
        let reloaded = crate::ipc::run_blocking(move || Ok(scan_ended_sessions(&scan_dir)))
            .await
            .unwrap_or_default();
        let mut initial_sessions = HashMap::with_capacity(reloaded.len());
        for (session_id, handle) in reloaded {
            initial_sessions.insert(session_id, handle);
        }
        let store = Arc::new(Self {
            sessions: RwLock::new(initial_sessions),
            journal_dir: dir,
        });
        Ok(store)
    }

    #[cfg(test)]
    fn reload_ended_sessions(&self) {
        let reloaded = scan_ended_sessions(&self.journal_dir);
        if let Ok(mut sessions) = self.sessions.try_write() {
            for (session_id, handle) in reloaded {
                sessions.insert(session_id, handle);
            }
        }
    }

    /// Creates a session: spawns the provider, starts its thread, journals the meta record.
    pub async fn create_session(
        &self,
        body: &CreateSessionBody,
    ) -> Result<SessionSummary, StructuredChatError> {
        if !is_supported_provider(&body.provider) {
            return Err(StructuredChatError::InvalidRequest(format!(
                "unknown provider {} (this gateway speaks: codex, claude, maho, omo, antigravity, opencode, cline)",
                body.provider
            )));
        }
        let provider_id = body.provider.as_str();
        let cwd = validate_cwd(&body.cwd)?;
        let settings = SessionSettings {
            model: body.model.clone(),
            effort: body.effort.clone(),
            permission_mode: Some(body.permission_mode.unwrap_or(PermissionMode::FullAccess)),
            execution_mode: body.execution_mode.clone(),
            agent_name: body.agent_name.clone(),
            fast_mode: body.fast_mode,
            additional_dirs: body.additional_dirs.clone(),
            ..SessionSettings::default()
        };
        let session_id = uuid::Uuid::new_v4().to_string();
        let journal_path = self.journal_dir.join(format!("{session_id}.jsonl"));
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let provider: Arc<dyn StructuredProvider> = spawn_provider(provider_id, cwd.clone(), events_tx, &settings)?;
        let thread_id = provider.start(None).await?;
        let handle = Arc::new(SessionHandle {
            session_id: session_id.clone(),
            provider_id: provider_id.to_owned(),
            cwd: cwd.clone(),
            inner: tokio::sync::Mutex::new(SessionInner {
                provider: Some(provider),
                thread_id: Some(thread_id.clone()),
                settings: settings.clone(),
                state: SessionState::Running,
                parts: Vec::new(),
                published_seq: 0,
                parts_loaded: true,
                next_seq: 1,
                turn_ids: Vec::new(),
                turn_snapshots: HashMap::new(),
                thread_first_turn: 0,
                turn_starting: false,
                active_turn: None,
                last_completed_turn: None,
                events_tx: watch::channel(0).0,
                journal_path: journal_path.clone(),
            }),
            journal: tokio::sync::Mutex::new(SessionJournal::new(journal_path)),
        });
        spawn_event_pump(Arc::clone(&handle), events_rx);
        let summary = {
            let inner = handle.inner.lock().await;
            inner.summary(&session_id, provider_id, &cwd)
        };
        handle.rewrite_journal_snapshot().await?;
        self.sessions
            .write()
            .await
            .insert(session_id.clone(), Arc::clone(&handle));
        Ok(summary)
    }

    /// Starts one user turn, resuming the thread first when the session was ended.
    pub async fn start_turn(
        &self,
        session_id: &str,
        body: &TurnBody,
    ) -> Result<u64, StructuredChatError> {
        if body.text.trim().is_empty() {
            return Err(StructuredChatError::InvalidRequest(
                "text must not be empty".into(),
            ));
        }
        let attachments = validate_attachments(body)?;
        let handle = self.handle(session_id).await?;
        ensure_running(&handle).await?;
        // Reserve the turn under the lock, then let it go: `provider.turn` is a round trip that
        // can take the provider's whole request timeout, and holding the session mutex across it
        // would block every history read and every event append behind it.
        let (provider, settings) = {
            let mut inner = handle.inner.lock().await;
            if inner.active_turn.is_some() || inner.turn_starting {
                return Err(StructuredChatError::Conflict(
                    "TURN_ACTIVE: a turn is already running; use steer or interrupt".into(),
                ));
            }
            let provider = inner
                .provider
                .as_ref()
                .expect("provider running after ensure_running")
                .clone();
            inner.turn_starting = true;
            (provider, inner.settings.clone())
        };
        let pre_turn_snapshot = revert_files::capture_snapshot(&handle.cwd).await.ok().flatten();
        let turn = match provider.turn(body.text.clone(), attachments, &settings).await {
            Ok(turn) => turn,
            Err(error) => {
                handle.inner.lock().await.turn_starting = false;
                return Err(error);
            }
        };
        let seq = {
            let mut inner = handle.inner.lock().await;
            inner.turn_starting = false;
            if let Some(snapshot) = pre_turn_snapshot {
                inner.turn_snapshots.insert(turn.turn_id.clone(), snapshot);
            }
            // `turn/started` may already have recorded the turn (the notification races the
            // response), so the id is only added when it is not there yet.
            if !inner.turn_ids.contains(&turn.turn_id) {
                inner.turn_ids.push(turn.turn_id.clone());
            }
            // The provider may have died while this turn was being started: the pump saw `Ended`
            // before `active_turn` existed, so it could not fail the turn. Record the failure here so
            // a turn never starts into a dead provider and then vanishes.
            let provider_gone = inner.provider.is_none() || inner.state == SessionState::Ended;
            // The user message part is stamped with the turn id the provider assigned.
            let seq = inner.append_part(PartDraft {
                turn_id: Some(turn.turn_id.clone()),
                payload: PartKind::UserMessage {
                    text: body.text.clone(),
                },
            });
            if provider_gone {
                inner.append_part(PartDraft {
                    turn_id: Some(turn.turn_id),
                    payload: PartKind::Error {
                        message: "the provider exited before the turn completed".into(),
                    },
                });
            } else {
                inner.active_turn = Some(turn.turn_id.clone());
                // The turn may have completed while the provider round trip was in flight (the pump saw
                // `TurnCompleted` before `active_turn` existed and could not clear it). Read that back, so
                // a finished turn never looks in flight and blocks the next one.
                if inner.last_completed_turn.as_deref() == Some(turn.turn_id.as_str()) {
                    inner.active_turn = None;
                }
            }
            seq
        };
        handle.flush_pending_parts().await?;
        Ok(seq)
    }

    /// Steers the active turn.
    pub async fn steer(
        &self,
        session_id: &str,
        body: &SteerBody,
    ) -> Result<u64, StructuredChatError> {
        if body.text.trim().is_empty() {
            return Err(StructuredChatError::InvalidRequest(
                "text must not be empty".into(),
            ));
        }
        let handle = self.handle(session_id).await?;
        ensure_running(&handle).await?;
        let (provider, expected_turn) = {
            let inner = handle.inner.lock().await;
            let expected_turn = inner.active_turn.clone().ok_or_else(|| {
                StructuredChatError::Conflict("NO_ACTIVE_TURN: no turn is running to steer".into())
            })?;
            let provider = inner
                .provider
                .as_ref()
                .expect("provider running after ensure_running")
                .clone();
            (provider, expected_turn)
        };
        provider.steer(&expected_turn, body.text.clone()).await?;
        let seq = {
            let mut inner = handle.inner.lock().await;
            inner.append_part(PartDraft {
                turn_id: Some(expected_turn),
                payload: PartKind::UserMessage {
                    text: body.text.clone(),
                },
            })
        };
        handle.flush_pending_parts().await?;
        Ok(seq)
    }

    /// Interrupts the active turn; a no-op when none is running.
    pub async fn interrupt(&self, session_id: &str) -> Result<(), StructuredChatError> {
        let handle = self.handle(session_id).await?;
        let (provider, turn) = {
            let inner = handle.inner.lock().await;
            let Some(provider) = inner.provider.clone() else {
                return Ok(());
            };
            (provider, inner.active_turn.clone())
        };
        if let Some(turn) = turn {
            provider.interrupt(&turn).await?;
        }
        Ok(())
    }

    /// Rolls back the last `turns` whole turns of the current thread and drops their history.
    ///
    /// Only the current thread is reachable: `thread/revert` knows the live thread's turns, and a
    /// `/clear` divider means the previous thread's parts are not this thread's to take back.
    /// Rolls back the last `turns` whole turns and drops their history, leaving files alone.
    pub async fn rollback(&self, session_id: &str, turns: u32) -> Result<(), StructuredChatError> {
        self.rollback_turns(session_id, turns, false).await
    }

    /// Rolls back the last `turns` whole turns, optionally restoring the files they changed.
    ///
    /// t3code offers "Revert and keep changes" and "Revert files too" as distinct choices
    /// (composer.md:191-194). The second is refused as a typed error when the provider does not
    /// advertise `revert_files`, because a provider that reverts its thread only cannot put the
    /// workspace back - and a client told "ok" would believe its files were restored.
    pub async fn rollback_turns(
        &self,
        session_id: &str,
        turns: u32,
        revert_files: bool,
    ) -> Result<(), StructuredChatError> {
        if turns == 0 {
            return Err(StructuredChatError::InvalidRequest(
                "turns must be >= 1".into(),
            ));
        }
        let handle = self.handle(session_id).await?;
        ensure_running(&handle).await?;
        let (provider, before_turn_id, keep, keep_ids, target_snapshot) = {
            let inner = handle.inner.lock().await;
            if revert_files {
                let provider_supports = inner
                    .provider
                    .as_ref()
                    .map(|provider| provider.capabilities().revert_files)
                    .unwrap_or(false);
                let git_supports = revert_files::supports_revert(&handle.cwd);
                if !provider_supports && !git_supports {
                    return Err(StructuredChatError::Unsupported(
                        "REVERT_FILES_UNSUPPORTED: this provider reverts the conversation only".into(),
                    ));
                }
            }
            if inner.active_turn.is_some() || inner.turn_starting {
                return Err(StructuredChatError::Conflict(
                    "TURN_ACTIVE: a turn is running; interrupt it before rolling back".into(),
                ));
            }
            let current = &inner.turn_ids[inner.thread_first_turn..];
            if turns as usize > current.len() {
                return Err(StructuredChatError::InvalidRequest(
                    "the session does not have that many turns".into(),
                ));
            }
            let keep = inner.turn_ids.len() - turns as usize;
            let before_turn_id = inner.turn_ids[keep].clone();
            let target_snapshot = if revert_files {
                inner.turn_snapshots.get(&before_turn_id).cloned()
            } else {
                None
            };
            if revert_files && target_snapshot.is_none() {
                let provider_supports = inner
                    .provider
                    .as_ref()
                    .map(|provider| provider.capabilities().revert_files)
                    .unwrap_or(false);
                if !provider_supports {
                    return Err(StructuredChatError::Unsupported(
                        "REVERT_FILES_UNSUPPORTED: no workspace snapshot recorded for that turn".into(),
                    ));
                }
            }
            let keep_ids: std::collections::HashSet<String> =
                inner.turn_ids[..keep].iter().cloned().collect();
            let provider = inner
                .provider
                .as_ref()
                .expect("provider running after ensure_running")
                .clone();
            (provider, before_turn_id, keep, keep_ids, target_snapshot)
        };
        if let Some(snapshot) = target_snapshot.as_ref() {
            revert_files::restore_snapshot(&handle.cwd, snapshot).await?;
        }
        let reopened = provider.rollback(&before_turn_id).await?;
        {
            let mut inner = handle.inner.lock().await;
            inner.thread_id = Some(reopened);
            inner.turn_ids.truncate(keep);
            inner.turn_snapshots.retain(|turn_id, _| keep_ids.contains(turn_id));
            inner.active_turn = None;
            // A part with no turn id (a divider, or an error the provider never stamped) belongs to
            // the current thread: only the parts stamped with a rolled-back turn go.
            inner.parts.retain(|part| match part.turn_id.as_ref() {
                Some(id) => keep_ids.contains(id),
                None => true,
            });
            // The marker is appended, not a rewrite of the numbering: `seq` keeps climbing, so a client
            // that holds a cursor across a rollback sees the marker and never a re-used number.
            inner.append_part(PartDraft {
                turn_id: None,
                payload: PartKind::Rollback { turns },
            });
        }
        // One atomic rewrite drops the rolled-back rows from the journal and keeps the marker, so a
        // reload cannot resurrect what the rollback took back.
        handle.rewrite_journal_snapshot().await?;
        Ok(())
    }

    /// Answers one approval request and marks its part answered.
    pub async fn answer_approval(
        &self,
        session_id: &str,
        request_id: &str,
        decision: ApprovalDecision,
    ) -> Result<(), StructuredChatError> {
        let handle = self.handle(session_id).await?;
        ensure_running(&handle).await?;
        let provider = {
            let inner = handle.inner.lock().await;
            inner
                .provider
                .as_ref()
                .expect("provider running after ensure_running")
                .clone()
        };
        provider.answer_approval(request_id, decision).await?;
        {
            let mut inner = handle.inner.lock().await;
            for part in &mut inner.parts {
                if let PartKind::ApprovalRequest {
                    request_id: named,
                    answered,
                    ..
                } = &mut part.payload
                {
                    if named == request_id {
                        *answered = true;
                    }
                }
            }
        }
        handle.rewrite_journal_snapshot().await?;
        Ok(())
    }

    /// Applies settings from the next turn on.
    pub async fn update_settings(
        &self,
        session_id: &str,
        settings: &SessionSettings,
    ) -> Result<SessionSettings, StructuredChatError> {
        let handle = self.handle(session_id).await?;
        ensure_parts_loaded(&handle).await?;
        let updated = {
            let mut inner = handle.inner.lock().await;
            if settings.model.is_some() {
                inner.settings.model = settings.model.clone();
            }
            if settings.effort.is_some() {
                inner.settings.effort = settings.effort.clone();
            }
            if settings.permission_mode.is_some() {
                inner.settings.permission_mode = settings.permission_mode;
            }
            if settings.execution_mode.is_some() {
                inner.settings.execution_mode = settings.execution_mode.clone();
            }
            if settings.agent_name.is_some() {
                inner.settings.agent_name = settings.agent_name.clone();
            }
            if settings.fast_mode.is_some() {
                inner.settings.fast_mode = settings.fast_mode;
            }
            if !settings.additional_dirs.is_empty() {
                inner.settings.additional_dirs = settings.additional_dirs.clone();
            }
            if settings.steering_mode.is_some() {
                inner.settings.steering_mode = settings.steering_mode;
            }
            if settings.follow_up_mode.is_some() {
                inner.settings.follow_up_mode = settings.follow_up_mode;
            }
            if settings.auto_compaction.is_some() {
                inner.settings.auto_compaction = settings.auto_compaction;
            }
            if settings.auto_retry.is_some() {
                inner.settings.auto_retry = settings.auto_retry;
            }
            if settings.compaction_mode.is_some() {
                inner.settings.compaction_mode = settings.compaction_mode.clone();
            }
            inner.settings.clone()
        };
        handle.rewrite_journal_snapshot().await?;
        Ok(updated)
    }

    pub async fn capabilities(
        &self,
        session_id: &str,
    ) -> Result<Capabilities, StructuredChatError> {
        let handle = self.handle(session_id).await?;
        let capabilities = {
            let inner = handle.inner.lock().await;
            match inner.provider.as_ref() {
                Some(provider) => provider.capabilities(),
                None => static_capabilities(&handle.provider_id),
            }
        };
        let (models, slash_commands) = discover_models_with_commands(&handle.provider_id, &handle.cwd).await?;
        let revert_files = capabilities.revert_files || revert_files::supports_revert(&handle.cwd);
        Ok(Capabilities {
            models,
            slash_commands,
            revert_files,
            ..capabilities
        })
    }

    /// `/clear`: a new thread in the same session, plus the `newSession` divider part.
    ///
    /// The previous thread's turns stay in `turn_ids` and its parts stay in history: the divider
    /// separates two threads, it does not delete the older one. `thread_first_turn` moves the
    /// rollback boundary to the new thread's first turn.
    pub async fn clear(&self, session_id: &str) -> Result<u64, StructuredChatError> {
        let handle = self.handle(session_id).await?;
        ensure_running(&handle).await?;
        let provider = {
            let inner = handle.inner.lock().await;
            inner
                .provider
                .as_ref()
                .expect("provider running after ensure_running")
                .clone()
        };
        let thread_id = provider.clear().await?;
        let seq = {
            let mut inner = handle.inner.lock().await;
            inner.thread_id = Some(thread_id);
            // Everything already known belongs to threads before this one; the next turn opens this
            // thread's range.
            inner.thread_first_turn = inner.turn_ids.len();
            inner.active_turn = None;
            inner.append_part(PartDraft {
                turn_id: None,
                payload: PartKind::NewSession,
            })
        };
        handle.rewrite_journal_snapshot().await?;
        Ok(seq)
    }

    pub async fn compact(&self, session_id: &str) -> Result<(), StructuredChatError> {
        self.compact_with_instructions(session_id, None).await
    }

    pub async fn compact_with_instructions(
        &self,
        session_id: &str,
        instructions: Option<&str>,
    ) -> Result<(), StructuredChatError> {
        let handle = self.handle(session_id).await?;
        let provider = {
            let inner = handle.inner.lock().await;
            inner.provider.clone()
        };
        let Some(provider) = provider else {
            return Err(StructuredChatError::Conflict(
                "SESSION_ENDED: resume the session with a turn before compacting".into(),
            ));
        };
        provider.compact_with_instructions(instructions).await
    }

    pub async fn commands(
        &self,
        session_id: &str,
    ) -> Result<Vec<SlashCommandItem>, StructuredChatError> {
        let handle = self.handle(session_id).await?;
        let provider = {
            let inner = handle.inner.lock().await;
            inner.provider.clone()
        };
        match provider {
            Some(provider) => provider.commands().await,
            None => Ok(static_capabilities(&handle.provider_id)
                .slash_commands
                .into_iter()
                .map(|name| SlashCommandItem {
                    syntax: if name.starts_with('$') { "dollar".into() } else { "slash".into() },
                    name: name.trim_start_matches(['/', '$']).to_owned(),
                    description: None,
                    source: "builtin".into(),
                })
                .collect()),
        }
    }

    pub async fn answer_extension_ui(
        &self,
        session_id: &str,
        request_id: &str,
        response: ExtensionUiResponseBody,
    ) -> Result<(), StructuredChatError> {
        let handle = self.handle(session_id).await?;
        let provider = {
            let inner = handle.inner.lock().await;
            inner.provider.clone()
        };
        let Some(provider) = provider else {
            return Err(StructuredChatError::Conflict(
                "SESSION_ENDED: provider is no longer running".into(),
            ));
        };
        provider.answer_extension_ui(request_id, response).await?;
        {
            let mut inner = handle.inner.lock().await;
            for part in &mut inner.parts {
                if let PartKind::ExtensionUiRequest {
                    request_id: id,
                    answered,
                    ..
                } = &mut part.payload
                {
                    if id == request_id {
                        *answered = true;
                    }
                }
            }
        }
        handle.rewrite_journal_snapshot().await?;
        Ok(())
    }

    pub async fn tree(&self, session_id: &str) -> Result<SessionTree, StructuredChatError> {
        let handle = self.handle(session_id).await?;
        ensure_running(&handle).await?;
        let provider = handle.inner.lock().await.provider.clone().expect("running");
        provider.get_tree().await
    }

    pub async fn navigate_tree(
        &self,
        session_id: &str,
        body: &NavigateTreeBody,
    ) -> Result<(), StructuredChatError> {
        let handle = self.handle(session_id).await?;
        ensure_running(&handle).await?;
        let provider = handle.inner.lock().await.provider.clone().expect("running");
        provider.navigate_tree(body).await
    }

    pub async fn fork(
        &self,
        session_id: &str,
        entry_id: &str,
    ) -> Result<ForkResult, StructuredChatError> {
        let handle = self.handle(session_id).await?;
        ensure_running(&handle).await?;
        let provider = handle.inner.lock().await.provider.clone().expect("running");
        let thread_id = provider.fork(entry_id).await?;
        Ok(ForkResult {
            session_id: session_id.to_owned(),
            thread_id: Some(thread_id),
        })
    }

    pub async fn clone_session(&self, session_id: &str) -> Result<ForkResult, StructuredChatError> {
        let handle = self.handle(session_id).await?;
        ensure_running(&handle).await?;
        let provider = handle.inner.lock().await.provider.clone().expect("running");
        let thread_id = provider.clone_session().await?;
        Ok(ForkResult {
            session_id: session_id.to_owned(),
            thread_id: Some(thread_id),
        })
    }

    pub async fn edit_message(
        &self,
        session_id: &str,
        body: &EditMessageBody,
    ) -> Result<(), StructuredChatError> {
        let handle = self.handle(session_id).await?;
        ensure_running(&handle).await?;
        let provider = handle.inner.lock().await.provider.clone().expect("running");
        provider.edit_message(body).await
    }

    pub async fn rename_session(
        &self,
        session_id: &str,
        name: &str,
    ) -> Result<(), StructuredChatError> {
        let handle = self.handle(session_id).await?;
        ensure_running(&handle).await?;
        let provider = handle.inner.lock().await.provider.clone().expect("running");
        provider.rename_session(name).await
    }

    pub async fn stats(&self, session_id: &str) -> Result<SessionStats, StructuredChatError> {
        let handle = self.handle(session_id).await?;
        let provider = handle.inner.lock().await.provider.clone();
        if let Some(provider) = provider {
            if let Ok(stats) = provider.session_stats().await {
                return Ok(stats);
            }
        }
        ensure_parts_loaded(&handle).await?;
        let inner = handle.inner.lock().await;
        let mut stats = SessionStats {
            turns: inner.turn_ids.len() as u64,
            ..SessionStats::default()
        };
        for part in &inner.parts {
            if let PartKind::Usage {
                total_tokens,
                input_tokens,
                output_tokens,
                context_window,
                ..
            } = &part.payload
            {
                stats.total_tokens = stats.total_tokens.saturating_add(*total_tokens);
                stats.input_tokens = stats.input_tokens.saturating_add(*input_tokens);
                stats.output_tokens = stats.output_tokens.saturating_add(*output_tokens);
                if context_window.is_some() {
                    stats.context_window = *context_window;
                }
            }
        }
        Ok(stats)
    }

    pub async fn reload(&self, session_id: &str) -> Result<(), StructuredChatError> {
        let handle = self.handle(session_id).await?;
        ensure_running(&handle).await?;
        let provider = handle.inner.lock().await.provider.clone().expect("running");
        provider.reload().await
    }

    pub async fn get_media(
        &self,
        session_id: &str,
        tool_call_id: &str,
        content_index: u32,
    ) -> Result<MediaBlob, StructuredChatError> {
        let handle = self.handle(session_id).await?;
        ensure_running(&handle).await?;
        let provider = handle.inner.lock().await.provider.clone().expect("running");
        provider.get_media(tool_call_id, content_index).await
    }

    pub async fn clear_queue(&self, session_id: &str) -> Result<(), StructuredChatError> {
        let handle = self.handle(session_id).await?;
        ensure_running(&handle).await?;
        let provider = handle.inner.lock().await.provider.clone().expect("running");
        provider.clear_queue().await
    }

    pub async fn set_model_preferences(
        &self,
        session_id: &str,
        body: &ModelPreferencesBody,
    ) -> Result<(), StructuredChatError> {
        let handle = self.handle(session_id).await?;
        ensure_running(&handle).await?;
        let provider = handle.inner.lock().await.provider.clone().expect("running");
        provider.set_model_preferences(body).await
    }

    /// `/resume`: continue a stopped run through the provider's native continuation.
    ///
    /// The provider reports the resumed turn through `TurnStarted`, so `active_turn` and
    /// `turn_ids` pick it up in the event pump like any other turn; nothing to add here.
    pub async fn resume(&self, session_id: &str) -> Result<(), StructuredChatError> {
        let handle = self.handle(session_id).await?;
        ensure_running(&handle).await?;
        let provider = {
            let inner = handle.inner.lock().await;
            inner
                .provider
                .as_ref()
                .expect("provider running after ensure_running")
                .clone()
        };
        provider.resume().await
    }

    /// One bounded page of history, newest-first when `before` is set.
    ///
    /// `after` is the forward cursor the events stream uses; `before` pages backwards for the
    /// transcript's "load older". `limit` bounds the page so a long session does not serialize
    /// its whole journal into one response.
    pub async fn history(
        &self,
        session_id: &str,
        query: &HistoryQuery,
    ) -> Result<HistoryPage, StructuredChatError> {
        let handle = self.handle(session_id).await?;
        ensure_parts_loaded(&handle).await?;
        let inner = handle.inner.lock().await;
        let visible = inner.visible_parts();
        let limit = query.limit.unwrap_or(HISTORY_PAGE_DEFAULT).clamp(1, HISTORY_PAGE_MAX);
        let start_idx = visible.partition_point(|part| part.seq <= query.after.unwrap_or(0));
        let end_idx = match query.before {
            Some(before) => visible.partition_point(|part| part.seq < before),
            None => visible.len(),
        };
        let candidates = if start_idx <= end_idx {
            &visible[start_idx..end_idx]
        } else {
            &[]
        };
        let truncated = candidates.len() > limit;
        // A read with no `after` cursor is the transcript opening: serve the newest window, so the
        // first page is what the reader is looking at and `before` walks back from there. A read
        // with `after` is an incremental tail, so serve the window adjacent to that cursor.
        let window = if query.after.is_some() {
            &candidates[..candidates.len().min(limit)]
        } else {
            let start = candidates.len().saturating_sub(limit);
            &candidates[start..]
        };
        let oldest = window.first().map(|part| part.seq);
        let parts = window.to_vec();
        Ok(HistoryPage {
            parts,
            // Older rows exist below the page only when the session holds parts under it.
            has_older: oldest
                .is_some_and(|seq| visible.first().is_some_and(|first| first.seq < seq)),
            next_before: oldest,
            truncated,
        })
    }

    /// The long-poll primitive: resolves as soon as a part past `after` exists, or after the
    /// bounded wait, whichever comes first. The same shape the reference-chat watch serves.
    pub async fn wait_for_events(
        &self,
        session_id: &str,
        after: u64,
    ) -> Result<Vec<StructuredPart>, StructuredChatError> {
        let handle = self.handle(session_id).await?;
        ensure_parts_loaded(&handle).await?;
        let has_pending = {
            let inner = handle.inner.lock().await;
            let pending = inner.has_parts_after(after);
            if pending {
                None
            } else {
                Some(inner.events_tx.subscribe())
            }
        };
        if let Some(mut rx) = has_pending {
            // The inner lock is NOT held during the wait, so the event pump can keep appending.
            let _ = tokio::time::timeout(
                EVENTS_LONG_POLL_TIMEOUT,
                rx.wait_for(|latest| *latest > after),
            )
            .await;
        }
        let inner = handle.inner.lock().await;
        Ok(inner.parts_after(after))
    }

    /// Every session row this store knows.
    pub async fn list_sessions(&self) -> Vec<SessionSummary> {
        let sessions = self.sessions.read().await;
        let mut rows = Vec::new();
        for handle in sessions.values() {
            let inner = handle.inner.lock().await;
            rows.push(inner.summary(&handle.session_id, &handle.provider_id, &handle.cwd));
        }
        rows.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        rows
    }

    /// Closes the session and kills its provider process.
    pub async fn close_session(&self, session_id: &str) -> Result<(), StructuredChatError> {
        let handle = self.handle(session_id).await?;
        ensure_parts_loaded(&handle).await?;
        let provider = {
            let mut inner = handle.inner.lock().await;
            inner.state = SessionState::Ended;
            inner.active_turn = None;
            inner.provider.take()
        };
        if let Some(provider) = provider {
            provider.close().await;
        }
        handle.rewrite_journal_snapshot().await?;
        self.sessions.write().await.remove(session_id);
        Ok(())
    }

    async fn handle(&self, session_id: &str) -> Result<Arc<SessionHandle>, StructuredChatError> {
        self.sessions
            .read()
            .await
            .get(session_id)
            .cloned()
            .ok_or(StructuredChatError::NotFound)
    }
}

/// Spawns the provider a session names, with the settings it was created under.
///
/// The launch settings matter to Claude in particular: its permission mode and model are argv
/// flags fixed at spawn time, so the store must not open a Claude child without them.
fn is_supported_provider(provider_id: &str) -> bool {
    matches!(
        provider_id,
        "codex" | "claude" | "maho" | "omo" | "antigravity" | "opencode" | "cline"
    )
}

fn spawn_provider(
    provider_id: &str,
    cwd: PathBuf,
    events: mpsc::UnboundedSender<ProviderEvent>,
    settings: &SessionSettings,
) -> Result<Arc<dyn StructuredProvider>, StructuredChatError> {
    match provider_id {
        "codex" => Ok(Arc::new(CodexProvider::spawn(cwd, events)?)),
        "claude" => Ok(Arc::new(ClaudeProvider::spawn(cwd, events, settings))),
        "maho" => Ok(Arc::new(MahoRpcProvider::spawn("maho", cwd, events, settings)?)),
        "omo" => Ok(Arc::new(MahoRpcProvider::spawn("omo", cwd, events, settings)?)),
        "antigravity" => Ok(Arc::new(AntigravityProvider::spawn(cwd, events, settings)?)),
        "opencode" => Ok(Arc::new(AcpProvider::spawn("opencode", cwd, events, settings)?)),
        "cline" => Ok(Arc::new(AcpProvider::spawn("cline", cwd, events, settings)?)),
        other => Err(StructuredChatError::InvalidRequest(format!(
            "unknown provider {other} (this gateway speaks: codex, claude, maho, omo, antigravity, opencode, cline)"
        ))),
    }
}

fn static_capabilities(provider_id: &str) -> Capabilities {
    match provider_id {
        "claude" => crate::structured_chat::claude::claude_capabilities(),
        "maho" | "omo" => crate::structured_chat::maho_rpc::maho_capabilities(),
        "antigravity" => crate::structured_chat::antigravity::antigravity_capabilities(),
        "opencode" | "cline" => crate::structured_chat::acp::acp_capabilities(provider_id),
        _ => crate::structured_chat::codex::codex_capabilities(),
    }
}

async fn discover_models_with_commands(
    provider_id: &str,
    cwd: &Path,
) -> Result<(Vec<crate::structured_chat::ModelInfo>, Vec<String>), StructuredChatError> {
    match provider_id {
        "claude" => crate::structured_chat::claude::discover_models_with_commands(cwd).await,
        "maho" | "omo" => {
            crate::structured_chat::maho_rpc::discover_models_with_commands(provider_id, cwd).await
        }
        "antigravity" => crate::structured_chat::antigravity::discover_models_with_commands(cwd).await,
        "opencode" | "cline" => {
            crate::structured_chat::acp::discover_models_with_commands(provider_id, cwd).await
        }
        _ => crate::structured_chat::codex::discover_models_with_commands(cwd).await,
    }
}

/// Lightweight peek for startup reload so ended sessions do not parse or retain
/// historical part payloads in memory until a caller reads or resumes them.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
enum JournalHeaderPeek {
    Meta {
        session_id: String,
        provider: String,
        cwd: String,
        thread_id: Option<String>,
        settings: SessionSettings,
    },
    Part {
        part: JournalPartSeq,
    },
}

#[derive(Deserialize)]
struct JournalPartSeq {
    seq: u64,
}

fn scan_ended_sessions(journal_dir: &Path) -> Vec<(String, Arc<SessionHandle>)> {
    use std::io::BufRead;
    let Ok(entries) = std::fs::read_dir(journal_dir) else {
        return Vec::new();
    };
    let mut loaded = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };
        let mut reader = std::io::BufReader::new(file);
        let mut line = String::new();
        let mut meta: Option<(String, String, String, Option<String>, SessionSettings)> = None;
        let mut latest_seq = 0u64;
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }
            let Ok(record) = serde_json::from_str::<JournalHeaderPeek>(line.trim()) else {
                continue;
            };
            match record {
                JournalHeaderPeek::Meta {
                    session_id,
                    provider,
                    cwd,
                    thread_id,
                    settings,
                } => meta = Some((session_id, provider, cwd, thread_id, settings)),
                JournalHeaderPeek::Part { part } => {
                    latest_seq = latest_seq.max(part.seq);
                }
            }
        }
        let Some((session_id, provider, cwd, thread_id, settings)) = meta else {
            continue;
        };
        let (events_tx, _events_rx) = watch::channel(latest_seq);
        let handle = Arc::new(SessionHandle {
            session_id: session_id.clone(),
            provider_id: provider,
            cwd: PathBuf::from(&cwd),
            inner: tokio::sync::Mutex::new(SessionInner {
                provider: None,
                thread_id,
                settings,
                state: SessionState::Ended,
                parts: Vec::new(),
                published_seq: latest_seq,
                parts_loaded: false,
                next_seq: latest_seq + 1,
                turn_ids: Vec::new(),
                turn_snapshots: HashMap::new(),
                thread_first_turn: 0,
                turn_starting: false,
                active_turn: None,
                last_completed_turn: None,
                events_tx,
                journal_path: path.clone(),
            }),
            journal: tokio::sync::Mutex::new(SessionJournal::with_flushed_seq(path, latest_seq)),
        });
        loaded.push((session_id, handle));
    }
    loaded
}

fn load_journal_parts_sync(path: &Path) -> (Vec<StructuredPart>, u64) {
    use std::io::BufRead;
    let Ok(file) = std::fs::File::open(path) else {
        return (Vec::new(), 0);
    };
    let mut reader = std::io::BufReader::new(file);
    let mut line = String::new();
    let mut parts = Vec::new();
    let mut latest_seq = 0u64;
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        if let Ok(JournalRecord::Part { part }) = serde_json::from_str::<JournalRecord>(line.trim())
        {
            latest_seq = latest_seq.max(part.seq);
            parts.push(part);
        }
    }
    if !parts.windows(2).all(|w| w[0].seq <= w[1].seq) {
        parts.sort_by_key(|p| p.seq);
    }
    (parts, latest_seq)
}

async fn ensure_parts_loaded(handle: &Arc<SessionHandle>) -> Result<(), StructuredChatError> {
    let journal_path = {
        let inner = handle.inner.lock().await;
        if inner.parts_loaded {
            return Ok(());
        }
        inner.journal_path.clone()
    };
    let (loaded_parts, latest_seq) =
        crate::ipc::run_blocking(move || Ok(load_journal_parts_sync(&journal_path)))
            .await
            .map_err(|error| StructuredChatError::Provider(error.to_string()))?;
    let mut journal = handle.journal.lock().await;
    let mut inner = handle.inner.lock().await;
    if !inner.parts_loaded {
        inner.parts = loaded_parts;
        inner.next_seq = inner.next_seq.max(latest_seq + 1);
        inner.published_seq = inner.published_seq.max(latest_seq);
        inner.parts_loaded = true;
        journal.flushed_seq = journal.flushed_seq.max(latest_seq);
    }
    Ok(())
}

/// A session ended by a gateway restart has no provider; the next mutation resumes the thread.
async fn ensure_running(handle: &Arc<SessionHandle>) -> Result<(), StructuredChatError> {
    ensure_parts_loaded(handle).await?;
    let events_rx = {
        let mut inner = handle.inner.lock().await;
        if inner.provider.is_some() {
            return Ok(());
        }
        let Some(thread_id) = inner.thread_id.clone() else {
            return Err(StructuredChatError::Conflict(
                "SESSION_ENDED: the thread id was lost; start a new session".into(),
            ));
        };
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let provider: Arc<dyn StructuredProvider> =
            spawn_provider(&handle.provider_id, handle.cwd.clone(), events_tx, &inner.settings)?;
        let resumed = provider.start(Some(thread_id)).await?;
        inner.thread_id = Some(resumed);
        inner.state = SessionState::Running;
        inner.provider = Some(provider);
        events_rx
    };
    handle.rewrite_journal_snapshot().await?;
    spawn_event_pump(Arc::clone(handle), events_rx);
    Ok(())
}

/// Pumps provider events into the session: stamps parts, journals them, tracks turn lifecycle,
/// and marks the session `ended` when the provider process is gone.
fn spawn_event_pump(handle: Arc<SessionHandle>, mut events: mpsc::UnboundedReceiver<ProviderEvent>) {
    tokio::spawn(async move {
        while let Some(first_event) = events.recv().await {
            #[derive(Clone, Copy, PartialEq, Eq)]
            enum PendingJournal {
                None,
                Flush,
                Rewrite,
            }
            let (pending, notify_seq) = {
                let mut inner = handle.inner.lock().await;
                let mut pending = PendingJournal::None;
                let mut notify_seq: Option<u64> = None;
                let mut next = Some(first_event);
                while let Some(event) = next {
                    match event {
                        ProviderEvent::Part(draft) => {
                            notify_seq = Some(inner.append_part_quiet(draft));
                            if pending == PendingJournal::None {
                                pending = PendingJournal::Flush;
                            }
                        }
                        ProviderEvent::TurnStarted { turn_id } => {
                            // A resumed turn arrives only this way (`thread/queue/start` reports it as a
                            // notification), so record it here as well: steer, interrupt and rollback all
                            // address turns out of `turn_ids`.
                            if !inner.turn_ids.contains(&turn_id) {
                                inner.turn_ids.push(turn_id.clone());
                            }
                            inner.active_turn = Some(turn_id);
                        }
                        ProviderEvent::TurnCompleted { turn_id, error } => {
                            if inner.active_turn.as_deref() == Some(turn_id.as_str()) {
                                inner.active_turn = None;
                            }
                            inner.last_completed_turn = Some(turn_id.clone());
                            if let Some(message) = error {
                                notify_seq = Some(inner.append_part_quiet(PartDraft {
                                    turn_id: Some(turn_id),
                                    payload: PartKind::Error { message },
                                }));
                                if pending == PendingJournal::None {
                                    pending = PendingJournal::Flush;
                                }
                            }
                        }
                        ProviderEvent::Ended { reason } => {
                            // A turn still in flight when the process dies is a failed turn: record why,
                            // so history does not end with a user message and silence.
                            if let Some(turn_id) = inner.active_turn.take() {
                                notify_seq = Some(inner.append_part_quiet(PartDraft {
                                    turn_id: Some(turn_id),
                                    payload: PartKind::Error {
                                        message: format!(
                                            "the provider exited before the turn completed: {reason}"
                                        ),
                                    },
                                }));
                            }
                            eprintln!("structured-chat session ended: {reason}");
                            pending = PendingJournal::Rewrite;
                        }
                    }
                    next = events.try_recv().ok();
                }
                (pending, notify_seq)
            };
            match pending {
                PendingJournal::None => {}
                PendingJournal::Flush => {
                    if let Err(error) = handle.flush_pending_parts().await {
                        eprintln!("structured-chat journal append failed: {error}");
                        if let Some(seq) = notify_seq {
                            let mut inner = handle.inner.lock().await;
                            inner.published_seq = inner.published_seq.max(seq);
                            let _ = inner.events_tx.send_replace(seq);
                        }
                    }
                }
                PendingJournal::Rewrite => {
                    if handle.rewrite_journal_snapshot_with_end(true).await.is_err() {
                        let mut inner = handle.inner.lock().await;
                        inner.state = SessionState::Ended;
                        inner.provider = None;
                        if let Some(seq) = notify_seq {
                            inner.published_seq = inner.published_seq.max(seq);
                            let _ = inner.events_tx.send_replace(seq);
                        }
                    }
                }
            }
        }
    });
}

async fn rewrite_journal_file(
    path: &Path,
    meta: &JournalRecord,
    parts: &[StructuredPart],
) -> Result<(), StructuredChatError> {
    let mut body = serde_json::to_string(meta)
        .map_err(|error| StructuredChatError::Provider(error.to_string()))?;
    body.push('\n');
    for part in parts {
        body.push_str(
            &serde_json::to_string(&JournalRecord::Part { part: part.clone() })
                .map_err(|error| StructuredChatError::Provider(error.to_string()))?,
        );
        body.push('\n');
    }
    let temp = path.with_extension("jsonl.tmp");
    tokio::fs::write(&temp, body)
        .await
        .map_err(|error| StructuredChatError::Provider(error.to_string()))?;
    tokio::fs::rename(&temp, path)
        .await
        .map_err(|error| StructuredChatError::Provider(error.to_string()))
}

#[cfg(test)]
#[path = "store_falsify.rs"]
mod falsify_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A scripted in-process provider the store tests drive without any child process.
    struct FakeProvider {
        events: mpsc::UnboundedSender<ProviderEvent>,
        turns: AtomicU32,
    }

    impl FakeProvider {
        fn emit(&self, event: ProviderEvent) {
            let _ = self.events.send(event);
        }
    }

    #[async_trait::async_trait]
    impl StructuredProvider for FakeProvider {
        fn provider_id(&self) -> &'static str {
            "fake"
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities {
                steer: true,
                rollback: true,
                resume: true,
                revert_files: false,
                history_paging: true,
                permission_modes: PermissionMode::all().to_vec(),
                models: Vec::new(),
                slash_commands: Vec::new(),
                ..Capabilities::default()
            }
        }

        async fn start(&self, resume_thread: Option<String>) -> Result<String, StructuredChatError> {
            Ok(resume_thread.unwrap_or_else(|| "th-fake".into()))
        }

        async fn turn(
            &self,
            text: String,
            _attachments: Vec<String>,
            _settings: &SessionSettings,
        ) -> Result<crate::structured_chat::TurnRef, StructuredChatError> {
            let n = self.turns.fetch_add(1, Ordering::SeqCst) + 1;
            let turn_id = format!("t{n}");
            self.emit(ProviderEvent::Part(PartDraft {
                turn_id: Some(turn_id.clone()),
                payload: PartKind::AssistantMessage {
                    text: format!("reply to {text}"),
                },
            }));
            self.emit(ProviderEvent::TurnCompleted {
                turn_id,
                error: None,
            });
            Ok(crate::structured_chat::TurnRef {
                turn_id: format!("t{n}"),
            })
        }

        async fn steer(&self, _expected_turn: &str, _text: String) -> Result<(), StructuredChatError> {
            Ok(())
        }

        async fn interrupt(&self, _turn_id: &str) -> Result<(), StructuredChatError> {
            Ok(())
        }

        async fn rollback(&self, _before_turn_id: &str) -> Result<String, StructuredChatError> {
            // A reopened conversation, so a store test can tell whether the id was persisted.
            Ok("th-fake-rolled".into())
        }

        async fn answer_approval(
            &self,
            _request_id: &str,
            _decision: ApprovalDecision,
        ) -> Result<(), StructuredChatError> {
            Ok(())
        }

        async fn clear(&self) -> Result<String, StructuredChatError> {
            Ok("th-fake-2".into())
        }

        async fn compact(&self) -> Result<(), StructuredChatError> {
            Err(StructuredChatError::Unsupported(
                "COMPACT_UNSUPPORTED: no compact method".into(),
            ))
        }

        async fn resume(&self) -> Result<(), StructuredChatError> {
            Ok(())
        }

        async fn close(&self) {}

        fn event_sink(&self) -> mpsc::UnboundedSender<ProviderEvent> {
            self.events.clone()
        }
    }

    /// A provider with no native continuation, standing in for one that advertises `resume: false`.
    struct NoResumeProvider;

    #[async_trait::async_trait]
    impl StructuredProvider for NoResumeProvider {
        fn provider_id(&self) -> &'static str {
            "fake"
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities {
                steer: false,
                rollback: false,
                resume: false,
                revert_files: false,
                history_paging: true,
                permission_modes: Vec::new(),
                models: Vec::new(),
                slash_commands: Vec::new(),
                ..Capabilities::default()
            }
        }

        async fn start(&self, resume_thread: Option<String>) -> Result<String, StructuredChatError> {
            Ok(resume_thread.unwrap_or_else(|| "th-fake".into()))
        }

        async fn turn(
            &self,
            _text: String,
            _attachments: Vec<String>,
            _settings: &SessionSettings,
        ) -> Result<crate::structured_chat::TurnRef, StructuredChatError> {
            Ok(crate::structured_chat::TurnRef { turn_id: "turn-1".into() })
        }

        async fn steer(&self, _expected_turn: &str, _text: String) -> Result<(), StructuredChatError> {
            Ok(())
        }

        async fn interrupt(&self, _turn_id: &str) -> Result<(), StructuredChatError> {
            Ok(())
        }

        async fn rollback(&self, _before_turn_id: &str) -> Result<String, StructuredChatError> {
            Ok("th-fake".into())
        }

        async fn answer_approval(
            &self,
            _request_id: &str,
            _decision: ApprovalDecision,
        ) -> Result<(), StructuredChatError> {
            Ok(())
        }

        async fn clear(&self) -> Result<String, StructuredChatError> {
            Ok("th-fake-2".into())
        }

        async fn compact(&self) -> Result<(), StructuredChatError> {
            Ok(())
        }

        async fn resume(&self) -> Result<(), StructuredChatError> {
            Err(StructuredChatError::Unsupported(
                "RESUME_UNSUPPORTED: no continuation".into(),
            ))
        }

        async fn close(&self) {}

        fn event_sink(&self) -> mpsc::UnboundedSender<ProviderEvent> {
            mpsc::unbounded_channel().0
        }
    }

    fn store_in(temp: &Path) -> Arc<StructuredChatStore> {
        Arc::new(StructuredChatStore {
            sessions: RwLock::new(HashMap::new()),
            journal_dir: temp.to_path_buf(),
        })
    }

    async fn insert_fake_session(
        store: &StructuredChatStore,
        temp: &Path,
    ) -> Arc<SessionHandle> {
        let session_id = "s-test".to_owned();
        let journal_path = temp.join(format!("{session_id}.jsonl"));
        let handle = Arc::new(SessionHandle {
            session_id: session_id.clone(),
            provider_id: "fake".to_owned(),
            cwd: temp.to_path_buf(),
            inner: tokio::sync::Mutex::new(SessionInner {
                provider: None,
                thread_id: Some("th-fake".into()),
                settings: SessionSettings {
                    model: None,
                    effort: None,
                    permission_mode: Some(PermissionMode::Supervised),
                    ..SessionSettings::default()
                },
                state: SessionState::Running,
                parts: Vec::new(),
                published_seq: 0,
                parts_loaded: true,
                next_seq: 1,
                turn_ids: Vec::new(),
                turn_snapshots: HashMap::new(),
                thread_first_turn: 0,
                turn_starting: false,
                active_turn: None,
                last_completed_turn: None,
                events_tx: watch::channel(0).0,
                journal_path: journal_path.clone(),
            }),
            journal: tokio::sync::Mutex::new(SessionJournal::new(journal_path)),
        });
        store
            .sessions
            .write()
            .await
            .insert(session_id, Arc::clone(&handle));
        // A real session always has its meta line: the journal replay and rollback rewrites
        // both assume it, so the fixture must not be lazy about it.
        handle
            .rewrite_journal_snapshot()
            .await
            .expect("meta record");
        handle
    }

    async fn attach_fake_provider(handle: &Arc<SessionHandle>) {
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let mut inner = handle.inner.lock().await;
        inner.provider = Some(Arc::new(FakeProvider {
            events: events_tx.clone(),
            turns: AtomicU32::new(0),
        }));
        drop(inner);
        spawn_event_pump(Arc::clone(handle), events_rx);
    }

    async fn wait_for_parts(store: &StructuredChatStore, count: usize) -> Vec<StructuredPart> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let parts = store.history("s-test", &HistoryQuery::default()).await.unwrap().parts;
            if parts.len() >= count {
                return parts;
            }
            assert!(std::time::Instant::now() < deadline, "parts never arrived");
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    #[tokio::test]
    async fn cwd_guard_refuses_relative_missing_and_accepts_real_dirs() {
        assert!(validate_cwd("relative/path").is_err());
        assert!(validate_cwd("").is_err());
        assert!(validate_cwd("/definitely/not/a/dir").is_err());
        // The jail admits a real directory only under a trusted root, so the accepted case names
        // its root explicitly instead of relying on where the test happens to run.
        let dir = std::env::temp_dir();
        let canonical = dir.canonicalize().unwrap_or(dir.clone());
        assert_eq!(
            validate_cwd_in(dir.to_str().unwrap(), std::slice::from_ref(&canonical)),
            Ok(canonical.clone())
        );
        assert!(validate_cwd_in(dir.to_str().unwrap(), &[canonical.join("elsewhere")]).is_err());
    }

    #[tokio::test]
    async fn turn_refuses_when_a_turn_is_active_and_steer_needs_one() {
        let temp = std::env::temp_dir().join(format!("ferryx-sc-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp).await.unwrap();
        let store = store_in(&temp);
        let handle = insert_fake_session(&store, &temp).await;
        attach_fake_provider(&handle).await;
        handle.inner.lock().await.active_turn = Some("t1".into());
        let body = TurnBody { text: "again".into(), attachments: None };
        let error = store.start_turn("s-test", &body).await.unwrap_err();
        assert_eq!(error.code(), "CONFLICT");
        let steer = crate::structured_chat::types::SteerBody { text: "nudge".into() };
        store.steer("s-test", &steer).await.unwrap();
        handle.inner.lock().await.active_turn = None;
        let error = store.steer("s-test", &steer).await.unwrap_err();
        assert!(matches!(error, StructuredChatError::Conflict(_)));
        assert_eq!(
            error.message(),
            "NO_ACTIVE_TURN: no turn is running to steer"
        );
        let _ = tokio::fs::remove_dir_all(&temp).await;
    }

    /// Gap item 2 (chat-t3-parity plan): "Revert files too" is honoured only where the provider
    /// advertises it, and refused as a typed error where it does not - never a silent success that
    /// would leave the client believing its workspace was restored.
    #[tokio::test]
    async fn revert_files_is_refused_by_a_provider_that_only_rewinds_the_thread() {
        let temp = std::env::temp_dir().join(format!("ferryx-sc-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp).await.unwrap();
        let store = store_in(&temp);
        let handle = insert_fake_session(&store, &temp).await;
        attach_fake_provider(&handle).await;
        store
            .start_turn("s-test", &TurnBody { text: "one".into(), attachments: None })
            .await
            .unwrap();
        wait_for_parts(&store, 2).await;

        let error = store.rollback_turns("s-test", 1, true).await.unwrap_err();
        assert_eq!(error.code(), "REVERT_FILES_UNSUPPORTED");
        assert!(error.message().contains("REVERT_FILES_UNSUPPORTED"));
        let parts = store.history("s-test", &HistoryQuery::default()).await.unwrap().parts;
        assert_eq!(parts.len(), 2);

        store.rollback_turns("s-test", 1, false).await.unwrap();
        let after = store.history("s-test", &HistoryQuery::default()).await.unwrap().parts;
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].payload, PartKind::Rollback { turns: 1 });
        let _ = tokio::fs::remove_dir_all(&temp).await;
    }

    #[tokio::test]
    async fn turn_appends_a_user_part_then_provider_parts_flow_through_the_pump() {
        let temp = std::env::temp_dir().join(format!("ferryx-sc-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp).await.unwrap();
        let store = store_in(&temp);
        let handle = insert_fake_session(&store, &temp).await;
        attach_fake_provider(&handle).await;
        let seq = store
            .start_turn("s-test", &TurnBody { text: "hello".into(), attachments: None })
            .await
            .unwrap();
        assert_eq!(seq, 1);
        let parts = wait_for_parts(&store, 2).await;
        assert_eq!(parts[0].payload, PartKind::UserMessage { text: "hello".into() });
        assert_eq!(
            parts[1].payload,
            PartKind::AssistantMessage { text: "reply to hello".into() }
        );
        assert_eq!(parts[1].seq, 2);
        // The journal holds both parts.
        let journal = tokio::fs::read_to_string(temp.join("s-test.jsonl")).await.unwrap();
        assert!(journal.contains("reply to hello"));
        let _ = tokio::fs::remove_dir_all(&temp).await;
    }

    #[tokio::test]
    async fn rollback_persists_the_thread_id_the_provider_reopened() {
        let temp = std::env::temp_dir().join(format!("ferryx-sc-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp).await.unwrap();
        let store = store_in(&temp);
        let handle = insert_fake_session(&store, &temp).await;
        attach_fake_provider(&handle).await;
        store
            .start_turn("s-test", &TurnBody { text: "one".into(), attachments: None })
            .await
            .unwrap();
        wait_for_parts(&store, 2).await;
        store.rollback("s-test", 1).await.unwrap();

        // A provider that expresses a rollback as a reopen answers with a different id; the store
        // must keep it, or the next resume would reopen the conversation the user rolled away from.
        assert_eq!(
            handle.inner.lock().await.thread_id.as_deref(),
            Some("th-fake-rolled")
        );
        let journal = tokio::fs::read_to_string(temp.join("s-test.jsonl")).await.unwrap();
        assert!(
            journal.contains("\"threadId\":\"th-fake-rolled\""),
            "the meta record must carry the reopened thread id: {journal}"
        );
        let _ = tokio::fs::remove_dir_all(&temp).await;
    }

    #[tokio::test]
    async fn rollback_drops_the_last_turns_parts_from_history_and_journal() {
        let temp = std::env::temp_dir().join(format!("ferryx-sc-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp).await.unwrap();
        let store = store_in(&temp);
        let handle = insert_fake_session(&store, &temp).await;
        attach_fake_provider(&handle).await;
        for text in ["one", "two"] {
            store
                .start_turn("s-test", &TurnBody { text: text.into(), attachments: None })
                .await
                .unwrap();
        }
        wait_for_parts(&store, 4).await;
        store.rollback("s-test", 1).await.unwrap();
        let parts = store.history("s-test", &HistoryQuery::default()).await.unwrap().parts;
        // Two surviving parts plus the rollback marker the store appends: the marker is how `seq`
        // stays strictly increasing across a rollback instead of re-using numbers.
        assert_eq!(parts.len(), 3);
        assert_eq!(
            parts[1].payload,
            PartKind::AssistantMessage { text: "reply to one".into() }
        );
        assert_eq!(parts[2].payload, PartKind::Rollback { turns: 1 });
        // Strictly increasing across the rollback, marker included.
        assert!(parts.windows(2).all(|w| w[0].seq < w[1].seq));
        let journal = tokio::fs::read_to_string(temp.join("s-test.jsonl")).await.unwrap();
        assert!(!journal.contains("reply to two"));
        assert!(journal.contains("reply to one"));
        // The meta record survived the rewrite with its identity.
        assert!(journal.contains("\"sessionId\":\"s-test\""));
        assert!(journal.contains("th-fake"));
        let _ = tokio::fs::remove_dir_all(&temp).await;
    }

    #[tokio::test]
    async fn approval_answer_marks_the_part_answered_in_memory_and_journal() {
        let temp = std::env::temp_dir().join(format!("ferryx-sc-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp).await.unwrap();
        let store = store_in(&temp);
        let handle = insert_fake_session(&store, &temp).await;
        attach_fake_provider(&handle).await;
        {
            let mut inner = handle.inner.lock().await;
            inner.append_part(PartDraft {
                turn_id: Some("t1".into()),
                payload: PartKind::ApprovalRequest {
                    request_id: "900".into(),
                    kind: "command".into(),
                    summary: "rm -rf /".into(),
                    reason: None,
                    answered: false,
                },
            });
        }
        handle.flush_pending_parts().await.unwrap();
        store
            .answer_approval("s-test", "900", ApprovalDecision::Decline)
            .await
            .unwrap();
        let parts = store.history("s-test", &HistoryQuery::default()).await.unwrap().parts;
        match &parts[0].payload {
            PartKind::ApprovalRequest { answered, .. } => assert!(*answered),
            other => panic!("unexpected part {other:?}"),
        }
        let journal = tokio::fs::read_to_string(temp.join("s-test.jsonl")).await.unwrap();
        assert!(journal.contains("\"answered\":true"));
        let _ = tokio::fs::remove_dir_all(&temp).await;
    }

    #[tokio::test]
    async fn clear_emits_a_new_session_divider_and_compact_maps_typed_unsupported() {
        let temp = std::env::temp_dir().join(format!("ferryx-sc-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp).await.unwrap();
        let store = store_in(&temp);
        let handle = insert_fake_session(&store, &temp).await;
        attach_fake_provider(&handle).await;
        let seq = store.clear("s-test").await.unwrap();
        let parts = store.history("s-test", &HistoryQuery::default()).await.unwrap().parts;
        assert_eq!(parts.last().unwrap().seq, seq);
        assert_eq!(parts.last().unwrap().payload, PartKind::NewSession);
        let error = store.compact("s-test").await.unwrap_err();
        assert_eq!(error.code(), "COMPACT_UNSUPPORTED");
        let _ = tokio::fs::remove_dir_all(&temp).await;
    }

    #[tokio::test]
    async fn an_ended_session_without_a_thread_id_conflicts_instead_of_silently_starting() {
        let temp = std::env::temp_dir().join(format!("ferryx-sc-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp).await.unwrap();
        let store = store_in(&temp);
        let handle = insert_fake_session(&store, &temp).await;
        {
            let mut inner = handle.inner.lock().await;
            inner.provider = None;
            inner.thread_id = None;
            inner.state = SessionState::Ended;
        }
        let body = TurnBody { text: "resume me".into(), attachments: None };
        let error = store.start_turn("s-test", &body).await.unwrap_err();
        assert!(matches!(error, StructuredChatError::Conflict(_)));
        assert_eq!(error.message(), "SESSION_ENDED: the thread id was lost; start a new session");
        let _ = tokio::fs::remove_dir_all(&temp).await;
    }

    #[tokio::test]
    async fn settings_update_applies_from_the_next_turn_and_is_partial() {
        let temp = std::env::temp_dir().join(format!("ferryx-sc-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp).await.unwrap();
        let store = store_in(&temp);
        let handle = insert_fake_session(&store, &temp).await;
        attach_fake_provider(&handle).await;
        let settings = store
            .update_settings(
                "s-test",
                &SessionSettings {
                    model: Some("gpt-5.1".into()),
                    effort: Some("high".into()),
                    permission_mode: Some(PermissionMode::Auto),
                    ..SessionSettings::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(settings.model.as_deref(), Some("gpt-5.1"));
        assert_eq!(settings.effort.as_deref(), Some("high"));
        assert_eq!(settings.permission_mode, Some(PermissionMode::Auto));
        let settings = store
            .update_settings(
                "s-test",
                &SessionSettings {
                    model: None,
                    effort: Some("low".into()),
                    permission_mode: None,
                    ..SessionSettings::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(settings.model.as_deref(), Some("gpt-5.1"));
        assert_eq!(settings.effort.as_deref(), Some("low"));
        let _ = tokio::fs::remove_dir_all(&temp).await;
    }

    #[tokio::test]
    async fn unknown_sessions_are_not_found_and_close_is_idempotent_per_session() {
        let temp = std::env::temp_dir().join(format!("ferryx-sc-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp).await.unwrap();
        let store = store_in(&temp);
        assert_eq!(
            store.history("missing", &HistoryQuery::default()).await.unwrap_err(),
            StructuredChatError::NotFound
        );
        assert_eq!(
            store.close_session("missing").await.unwrap_err(),
            StructuredChatError::NotFound
        );
        let handle = insert_fake_session(&store, &temp).await;
        attach_fake_provider(&handle).await;
        store.close_session("s-test").await.unwrap();
        assert!(store.list_sessions().await.is_empty());
        let journal = tokio::fs::read_to_string(temp.join("s-test.jsonl")).await.unwrap();
        assert!(journal.contains("\"state\":\"ended\""));
        let _ = tokio::fs::remove_dir_all(&temp).await;
    }

    #[tokio::test]
    async fn journal_replay_rebuilds_an_ended_session_with_its_parts() {
        let temp = std::env::temp_dir().join(format!("ferryx-sc-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp).await.unwrap();
        let store = store_in(&temp);
        let handle = insert_fake_session(&store, &temp).await;
        attach_fake_provider(&handle).await;
        store
            .start_turn("s-test", &TurnBody { text: "kept".into(), attachments: None })
            .await
            .unwrap();
        wait_for_parts(&store, 2).await;
        // Simulate a gateway restart: a fresh store over the same journal dir.
        let reloaded = store_in(&temp);
        reloaded.reload_ended_sessions();
        let sessions = reloaded.list_sessions().await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "s-test");
        assert_eq!(sessions[0].state, SessionState::Ended);
        let parts = reloaded.history("s-test", &HistoryQuery::default()).await.unwrap().parts;
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1].payload, PartKind::AssistantMessage { text: "reply to kept".into() });
        let _ = tokio::fs::remove_dir_all(&temp).await;
    }

    #[test]
    fn events_long_poll_timeout_is_bounded() {
        assert_eq!(
            EVENTS_LONG_POLL_TIMEOUT,
            std::time::Duration::from_secs(25)
        );
    }

    /// Gap item 3: `history` serves a bounded page and reports whether older rows exist, so the
    /// transcript's `hasOlderPage` is no longer hard-wired to false.
    #[tokio::test]
    async fn history_serves_a_bounded_page_with_an_older_cursor() {
        let temp = std::env::temp_dir().join(format!("ferryx-sc-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp).await.unwrap();
        let store = store_in(&temp);
        let handle = insert_fake_session(&store, &temp).await;
        attach_fake_provider(&handle).await;
        for text in ["one", "two", "three"] {
            store
                .start_turn("s-test", &TurnBody { text: text.into(), attachments: None })
                .await
                .unwrap();
        }
        wait_for_parts(&store, 6).await;

        let first = store
            .history("s-test", &HistoryQuery { limit: Some(4), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(first.parts.len(), 4, "the newest window");
        assert!(first.truncated);
        assert!(first.has_older, "the newest page has the session start below it");
        let next_before = first.next_before.expect("an older page is addressable");
        // The first page is the newest window, so it ends at the newest part.
        let newest = store
            .history("s-test", &HistoryQuery::default())
            .await
            .unwrap()
            .parts
            .last()
            .map(|part| part.seq)
            .unwrap();
        assert_eq!(first.parts.last().map(|part| part.seq), Some(newest));

        let older = store
            .history(
                "s-test",
                &HistoryQuery { before: Some(next_before), limit: Some(4), ..Default::default() },
            )
            .await
            .unwrap();
        assert_eq!(older.parts.len(), 2, "the remainder of the journal");
        assert!(!older.has_older, "the first part of the session is on this page");
        // The two pages are adjacent and non-overlapping.
        assert!(older.parts.iter().all(|part| part.seq < next_before));
        assert_eq!(older.parts.first().map(|part| part.seq), Some(1));
        let _ = tokio::fs::remove_dir_all(&temp).await;
    }

    /// Gap item 4: a rollback appends a marker and never re-uses a `seq`, so a client holding a
    /// cursor sees strictly increasing numbers across the rollback.
    #[tokio::test]
    async fn seq_stays_strictly_increasing_across_a_rollback() {
        let temp = std::env::temp_dir().join(format!("ferryx-sc-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp).await.unwrap();
        let store = store_in(&temp);
        let handle = insert_fake_session(&store, &temp).await;
        attach_fake_provider(&handle).await;
        for text in ["one", "two"] {
            store
                .start_turn("s-test", &TurnBody { text: text.into(), attachments: None })
                .await
                .unwrap();
        }
        wait_for_parts(&store, 4).await;
        let before: Vec<u64> = store
            .history("s-test", &HistoryQuery::default())
            .await
            .unwrap()
            .parts
            .iter()
            .map(|part| part.seq)
            .collect();
        let highest = *before.last().expect("parts before the rollback");

        store.rollback("s-test", 1).await.unwrap();
        let after = store.history("s-test", &HistoryQuery::default()).await.unwrap().parts;
        let marker = after.last().expect("the rollback marker is appended");
        assert_eq!(marker.payload, PartKind::Rollback { turns: 1 });
        assert!(
            marker.seq > highest,
            "the marker must sit above every part the rollback kept"
        );
        assert!(after.windows(2).all(|w| w[0].seq < w[1].seq));
        let _ = tokio::fs::remove_dir_all(&temp).await;
    }

    /// Gap item 1: `/resume` reaches the provider's native continuation, and a provider that has
    /// none surfaces as a typed 409 rather than a silent success.
    #[tokio::test]
    async fn resume_reaches_the_provider_and_reports_an_unsupported_protocol() {
        let temp = std::env::temp_dir().join(format!("ferryx-sc-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp).await.unwrap();
        let store = store_in(&temp);
        let handle = insert_fake_session(&store, &temp).await;
        attach_fake_provider(&handle).await;
        store.resume("s-test").await.expect("the fake provider resumes");

        // A provider without a continuation is a typed conflict, not a silent success: this is what
        // the route maps to 409 and the UI reads to fall back to sending a message.
        let refuse_journal = temp.join("s-refuse.jsonl");
        let refusing = std::sync::Arc::new(SessionHandle {
            session_id: "s-refuse".into(),
            provider_id: "fake".into(),
            cwd: temp.clone(),
            inner: tokio::sync::Mutex::new(SessionInner {
                provider: Some(Arc::new(NoResumeProvider)),
                thread_id: Some("th-fake".into()),
                settings: SessionSettings {
                    model: None,
                    effort: None,
                    permission_mode: Some(PermissionMode::Supervised),
                    ..SessionSettings::default()
                },
                state: SessionState::Running,
                parts: Vec::new(),
                published_seq: 0,
                parts_loaded: true,
                next_seq: 1,
                turn_ids: Vec::new(),
                turn_snapshots: HashMap::new(),
                thread_first_turn: 0,
                turn_starting: false,
                active_turn: None,
                last_completed_turn: None,
                events_tx: watch::channel(0).0,
                journal_path: refuse_journal.clone(),
            }),
            journal: tokio::sync::Mutex::new(SessionJournal::new(refuse_journal)),
        });
        store
            .sessions
            .write()
            .await
            .insert("s-refuse".to_string(), refusing);
        assert_eq!(
            store.resume("s-refuse").await.unwrap_err(),
            StructuredChatError::Unsupported("RESUME_UNSUPPORTED: no continuation".into())
        );
        let _ = tokio::fs::remove_dir_all(&temp).await;
    }

    /// Gap item 2: codex advertises `revertFiles: false`, because `thread/revert` documents that it
    /// does not revert local file changes. The flag is what the UI reads, so it must be present.
    #[test]
    fn capabilities_advertise_resume_and_no_file_revert() {
        let capabilities = crate::structured_chat::codex::codex_capabilities();
        assert!(capabilities.resume, "codex has a native continuation");
        assert!(!capabilities.revert_files, "thread/revert does not revert files");
        assert!(capabilities.history_paging, "history serves bounded pages");
    }

    #[tokio::test]
    async fn journal_interleaved_rewrite_and_append_preserves_all_parts_and_monotonic_seq() {
        let temp = std::env::temp_dir().join(format!("ferryx-sc-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp).await.unwrap();
        let store = store_in(&temp);
        let handle = insert_fake_session(&store, &temp).await;
        let journal_path = temp.join("s-test.jsonl");

        // Seed parts 1 and 2 and flush them to disk.
        {
            let mut inner = handle.inner.lock().await;
            inner.append_part(PartDraft {
                turn_id: Some("t1".into()),
                payload: PartKind::UserMessage { text: "one".into() },
            });
            inner.append_part(PartDraft {
                turn_id: Some("t1".into()),
                payload: PartKind::AssistantMessage { text: "two".into() },
            });
        }
        handle.flush_pending_parts().await.unwrap();
        assert_eq!(handle.journal.lock().await.flushed_seq, 2);

        // Phase 1: Deterministic interleaving — hold `journal` while mutating `inner`
        // (both a settings change and new parts #3 and #4), then release `journal` and
        // race a snapshot rewrite against concurrent flushes.
        {
            let _journal_guard = handle.journal.lock().await;
            let mut inner = handle.inner.lock().await;
            inner.settings.model = Some("gpt-5.2".into());
            inner.append_part(PartDraft {
                turn_id: Some("t2".into()),
                payload: PartKind::UserMessage {
                    text: "three".into(),
                },
            });
            inner.append_part(PartDraft {
                turn_id: Some("t2".into()),
                payload: PartKind::AssistantMessage {
                    text: "four".into(),
                },
            });
        }
        let (r_rewrite, r_flush_a, r_flush_b) = tokio::join!(
            handle.rewrite_journal_snapshot(),
            handle.flush_pending_parts(),
            handle.flush_pending_parts(),
        );
        r_rewrite.unwrap();
        r_flush_a.unwrap();
        r_flush_b.unwrap();
        assert_eq!(handle.journal.lock().await.flushed_seq, 4);

        // Phase 2: Concurrent writers interleaving appends and rewrites.
        let mut tasks = Vec::new();
        for i in 0..16usize {
            let h = Arc::clone(&handle);
            tasks.push(tokio::spawn(async move {
                if i % 3 == 0 {
                    {
                        let mut inner = h.inner.lock().await;
                        inner.settings.effort = Some(format!("effort-{i}"));
                    }
                    h.rewrite_journal_snapshot().await.unwrap();
                } else {
                    {
                        let mut inner = h.inner.lock().await;
                        inner.append_part(PartDraft {
                            turn_id: Some(format!("t-par-{i}")),
                            payload: PartKind::UserMessage {
                                text: format!("msg-{i}"),
                            },
                        });
                    }
                    h.flush_pending_parts().await.unwrap();
                }
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }

        let expected_count = handle.inner.lock().await.parts.len();
        assert_eq!(expected_count, 4 + 10);
        assert_eq!(
            handle.journal.lock().await.flushed_seq,
            expected_count as u64
        );

        // Verify the raw `.jsonl` lines on disk (without defensive sorting) are
        // strictly monotonic, complete, and duplicate-free, and preserve the latest meta.
        let raw = tokio::fs::read_to_string(&journal_path).await.unwrap();
        assert!(raw.contains("\"model\":\"gpt-5.2\""));
        let mut raw_seqs = Vec::new();
        for line in raw.lines() {
            if let Ok(JournalRecord::Part { part }) = serde_json::from_str::<JournalRecord>(line) {
                raw_seqs.push(part.seq);
            }
        }
        assert_eq!(
            raw_seqs,
            (1..=expected_count as u64).collect::<Vec<_>>(),
            "on-disk journal parts must be complete, deduplicated, and strictly monotonic"
        );
        let _ = tokio::fs::remove_dir_all(&temp).await;
    }

    #[tokio::test]
    async fn load_journal_parts_sync_sorts_out_of_order_jsonl_for_partition_point() {
        let temp = std::env::temp_dir().join(format!("ferryx-sc-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp).await.unwrap();
        let journal_path = temp.join("s-unordered.jsonl");

        let meta = JournalRecord::Meta {
            session_id: "s-unordered".into(),
            provider: "fake".into(),
            cwd: temp.to_string_lossy().to_string(),
            thread_id: Some("th-unordered".into()),
            settings: SessionSettings::default(),
            state: SessionState::Ended,
        };
        let mut body = serde_json::to_string(&meta).unwrap();
        body.push('\n');
        for seq in [1u64, 4, 2, 6, 5, 3] {
            let rec = JournalRecord::Part {
                part: StructuredPart {
                    seq,
                    turn_id: Some("t1".into()),
                    ts_ms: seq,
                    payload: PartKind::UserMessage {
                        text: format!("part-{seq}"),
                    },
                },
            };
            body.push_str(&serde_json::to_string(&rec).unwrap());
            body.push('\n');
        }
        tokio::fs::write(&journal_path, body).await.unwrap();

        let (parts, latest_seq) = load_journal_parts_sync(&journal_path);
        assert_eq!(latest_seq, 6);
        assert_eq!(
            parts.iter().map(|p| p.seq).collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5, 6]
        );

        // Reload through the store and verify `partition_point` queries (`history`) and
        // `has_parts_after` work on the defensively sorted parts, and `flushed_seq` tracks `6`.
        let reloaded = store_in(&temp);
        reloaded.reload_ended_sessions();
        let after_3 = reloaded
            .history(
                "s-unordered",
                &HistoryQuery {
                    after: Some(3),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            after_3.parts.iter().map(|p| p.seq).collect::<Vec<_>>(),
            vec![4, 5, 6]
        );
        let window = reloaded
            .history(
                "s-unordered",
                &HistoryQuery {
                    after: Some(2),
                    before: Some(5),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            window.parts.iter().map(|p| p.seq).collect::<Vec<_>>(),
            vec![3, 4]
        );
        let handle = reloaded.handle("s-unordered").await.unwrap();
        {
            let inner = handle.inner.lock().await;
            assert!(inner.has_parts_after(5));
            assert!(!inner.has_parts_after(6));
        }
        assert_eq!(handle.journal.lock().await.flushed_seq, 6);
        let _ = tokio::fs::remove_dir_all(&temp).await;
    }
}
