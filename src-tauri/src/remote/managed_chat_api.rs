use crate::remote::{
    auth::{DeviceAccessScope, DeviceInfo, DevicePermission},
    state::RemoteGatewayState,
};
use crate::scoped_contracts::{
    ChatDraft, DeliveryReceipt, DeliveryStage, Epoch, ScopeError, ScopeErrorCode, TargetRef,
    ATTACHMENT_MAX_FILE_BYTES, ATTACHMENT_MAX_FILES_PER_TURN, ATTACHMENT_MAX_TURN_BYTES,
};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
    time::Instant,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedChatSendRequest {
    pub request_id: String,
    pub target: TargetRef,
    pub draft: ChatDraft,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedChatReplyRequest {
    pub request_id: String,
    pub target: TargetRef,
    pub callback_id: Value,
    pub thread_id: String,
    pub turn_id: String,
    pub callback_incarnation: u64,
    #[serde(default)]
    pub kind: Option<String>,
    pub result: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedChatStopRequest {
    pub request_id: String,
    pub target: TargetRef,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCallbacksQuery {
    pub backend_session_id: String,
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveCallbackSummary {
    pub callback_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub callback_incarnation: u64,
    pub target: TargetRef,
    pub kind: CallbackKind,
    pub text: Option<String>,
    pub questions: Option<Vec<CallbackQuestion>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CallbackQuestion {
    pub id: String,
    pub question: String,
    #[serde(default)]
    pub is_secret: Option<bool>,
    #[serde(default)]
    pub options: Option<Vec<CallbackOption>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CallbackOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallbackResolutionReceipt {
    pub callback_id: String,
    pub resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStopReceipt {
    pub stopped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CallbackKind {
    Approval,
    Question,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallbackStatus {
    Pending,
    Dispatching,
    Resolved {
        resolved_at: Instant,
        result: Value,
    },
    Invalidated {
        invalidated_at: Instant,
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct LiveCallbackEntry {
    pub callback_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub callback_incarnation: u64,
    pub target: TargetRef,
    pub kind: CallbackKind,
    pub text: Option<String>,
    pub questions: Option<Vec<CallbackQuestion>>,
    pub status: CallbackStatus,
    pub created_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallbackError {
    NotFound,
    AlreadyResolved,
    StaleTurn,
    Invalidated(String),
    TargetMismatch(String),
    ThreadMismatch,
    TurnMismatch,
    InvalidDecision,
    InvalidAnswers(String),
    IncarnationMismatch,
}

pub fn normalize_callback_id(val: &Value) -> Option<String> {
    match val {
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[derive(Default)]
pub struct LiveCallbackRegistry {
    callbacks: HashMap<(String, String), LiveCallbackEntry>,
    active_turns: HashMap<(String, String), String>,
    next_incarnation: u64,
}

impl LiveCallbackRegistry {
    pub fn register(&mut self, mut entry: LiveCallbackEntry) -> Result<u64, CallbackError> {
        let thread_key = (
            entry.target.backend_session_id.clone(),
            entry.thread_id.clone(),
        );

        if let Some(active_turn) = self.active_turns.get(&thread_key) {
            if active_turn != &entry.turn_id {
                let session_id = entry.target.backend_session_id.clone();
                let old_turn = active_turn.clone();
                for ((sid, _), cb) in self.callbacks.iter_mut() {
                    if sid == &session_id && cb.turn_id == old_turn && matches!(&cb.status, CallbackStatus::Pending | CallbackStatus::Dispatching) {
                        cb.status = CallbackStatus::Invalidated {
                            invalidated_at: Instant::now(),
                            reason: "TURN_ADVANCED".to_string(),
                        };
                    }
                }
            }
        }

        self.active_turns.insert(thread_key, entry.turn_id.clone());
        let key = (
            entry.target.backend_session_id.clone(),
            entry.callback_id.clone(),
        );
        if let Some(previous) = self.callbacks.get_mut(&key) {
            if previous.status == CallbackStatus::Pending || previous.status == CallbackStatus::Dispatching {
                previous.status = CallbackStatus::Invalidated {
                    invalidated_at: Instant::now(),
                    reason: "CALLBACK_REPLACED".to_string(),
                };
            }
        }
        self.next_incarnation = self.next_incarnation.saturating_add(1).max(1);
        entry.callback_incarnation = self.next_incarnation;
        let incarnation = entry.callback_incarnation;
        self.callbacks.insert(key, entry);
        Ok(incarnation)
    }

    pub fn get(
        &self,
        backend_session_id: &str,
        callback_id: &str,
    ) -> Option<&LiveCallbackEntry> {
        self.callbacks
            .get(&(backend_session_id.to_string(), callback_id.to_string()))
    }

    pub fn list_pending(
        &self,
        backend_session_id: &str,
        thread_id: Option<&str>,
    ) -> Vec<LiveCallbackSummary> {
        self.callbacks
            .values()
            .filter(|cb| {
                cb.target.backend_session_id == backend_session_id
                    && cb.status == CallbackStatus::Pending
                    && thread_id.map_or(true, |t| cb.thread_id == t)
            })
            .map(|cb| LiveCallbackSummary {
                callback_id: cb.callback_id.clone(),
                thread_id: cb.thread_id.clone(),
                turn_id: cb.turn_id.clone(),
                callback_incarnation: cb.callback_incarnation,
                target: cb.target.clone(),
                kind: cb.kind.clone(),
                text: cb.text.clone(),
                questions: cb.questions.clone(),
            })
            .collect()
    }

    pub fn resolve_reply(
        &mut self,
        target: &TargetRef,
        callback_id: &str,
        thread_id: &str,
        turn_id: &str,
        callback_incarnation: u64,
        result: &Value,
    ) -> Result<LiveCallbackEntry, CallbackError> {
        let key = (target.backend_session_id.clone(), callback_id.to_string());
        let entry = self.callbacks.get_mut(&key).ok_or(CallbackError::NotFound)?;

        if entry.target.owner_id != target.owner_id {
            return Err(CallbackError::TargetMismatch("Owner mismatch".into()));
        }
        if entry.target.host_id != target.host_id {
            return Err(CallbackError::TargetMismatch(format!(
                "Host ID mismatch: expected {}, got {}",
                entry.target.host_id, target.host_id
            )));
        }
        if entry.target.epoch != target.epoch {
            return Err(CallbackError::TargetMismatch(format!(
                "Target epoch mismatch: expected {}, got {}",
                entry.target.epoch.0, target.epoch.0
            )));
        }
        if entry.target.backend_session_id != target.backend_session_id {
            return Err(CallbackError::TargetMismatch(format!(
                "Session ID mismatch: expected {}, got {}",
                entry.target.backend_session_id, target.backend_session_id
            )));
        }

        if entry.thread_id != thread_id {
            return Err(CallbackError::ThreadMismatch);
        }
        if entry.turn_id != turn_id {
            return Err(CallbackError::TurnMismatch);
        }
        if entry.callback_incarnation != callback_incarnation {
            return Err(CallbackError::IncarnationMismatch);
        }

        let thread_key = (target.backend_session_id.clone(), thread_id.to_string());
        if let Some(active_turn) = self.active_turns.get(&thread_key) {
            if active_turn != turn_id {
                return Err(CallbackError::StaleTurn);
            }
        }

        match &entry.status {
            CallbackStatus::Resolved { .. } => return Err(CallbackError::AlreadyResolved),
            CallbackStatus::Invalidated { reason, .. } => {
                return Err(CallbackError::Invalidated(reason.clone()))
            }
            CallbackStatus::Pending => {}
            CallbackStatus::Dispatching => return Err(CallbackError::AlreadyResolved),
        }

        match entry.kind {
            CallbackKind::Approval => {
                let decision = result.get("decision").and_then(Value::as_str);
                if !matches!(decision, Some("accept" | "decline" | "cancel")) {
                    return Err(CallbackError::InvalidDecision);
                }
            }
            CallbackKind::Question => {
                let answers = result
                    .get("answers")
                    .and_then(Value::as_object)
                    .ok_or_else(|| {
                        CallbackError::InvalidAnswers("Missing answers object".into())
                    })?;

                if let Some(questions) = &entry.questions {
                    for q in questions {
                        let answer_entry = answers.get(&q.id).ok_or_else(|| {
                            CallbackError::InvalidAnswers(format!("Missing answer for {}", q.id))
                        })?;
                        let answer_list = answer_entry
                            .get("answers")
                            .and_then(Value::as_array)
                            .ok_or_else(|| {
                                CallbackError::InvalidAnswers(format!(
                                    "answers field must be array for {}",
                                    q.id
                                ))
                            })?;
                        if answer_list.is_empty() {
                            return Err(CallbackError::InvalidAnswers(format!(
                                "Answer list empty for {}",
                                q.id
                            )));
                        }
                        if answer_list.len() > 16 {
                            return Err(CallbackError::InvalidAnswers(format!(
                                "Too many answers for {}",
                                q.id
                            )));
                        }
                        for val in answer_list {
                            let text = val.as_str().ok_or_else(|| {
                                CallbackError::InvalidAnswers(format!(
                                    "Answer must be string for {}",
                                    q.id
                                ))
                            })?;
                            if text.len() > 8192 {
                                return Err(CallbackError::InvalidAnswers(format!(
                                    "Answer text too long for {}",
                                    q.id
                                )));
                            }
                        }
                    }
                }
            }
        }

        entry.status = CallbackStatus::Dispatching;

        Ok(entry.clone())
    }

    pub fn finish_dispatch(&mut self, entry: &LiveCallbackEntry, result: &Value, success: bool) {
        let key = (entry.target.backend_session_id.clone(), entry.callback_id.clone());
        if let Some(current) = self.callbacks.get_mut(&key) {
            if current.callback_incarnation == entry.callback_incarnation
                && current.status == CallbackStatus::Dispatching
            {
                current.status = if success {
                    CallbackStatus::Resolved { resolved_at: Instant::now(), result: result.clone() }
                } else {
                    CallbackStatus::Pending
                };
            }
        }
    }

    pub fn invalidate_session(&mut self, backend_session_id: &str, reason: &str) {
        for ((sid, _), cb) in self.callbacks.iter_mut() {
            if sid == backend_session_id && matches!(&cb.status, CallbackStatus::Pending | CallbackStatus::Dispatching) {
                cb.status = CallbackStatus::Invalidated {
                    invalidated_at: Instant::now(),
                    reason: reason.to_string(),
                };
            }
        }
    }

    pub fn clear_for_test(&mut self) {
        self.callbacks.clear();
        self.active_turns.clear();
    }
}

pub static LIVE_CALLBACKS: LazyLock<Mutex<LiveCallbackRegistry>> =
    LazyLock::new(|| Mutex::new(LiveCallbackRegistry::default()));

#[async_trait::async_trait]
pub trait ManagedChatProvider: Send + Sync {
    async fn send_turn(
        &self,
        target: &TargetRef,
        input: Vec<Value>,
    ) -> Result<DeliveryReceipt, String>;

    async fn reply_callback(
        &self,
        callback_id: Value,
        thread_id: &str,
        turn_id: &str,
        result: Value,
    ) -> Result<(), String>;

    async fn stop_agent(&self, target: &TargetRef) -> Result<(), String>;
}

pub static MANAGED_PROVIDERS: LazyLock<Mutex<HashMap<String, Arc<dyn ManagedChatProvider>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static PROVIDER_PUMPS: LazyLock<Mutex<HashMap<String, tokio::sync::watch::Sender<bool>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn register_managed_provider(
    backend_session_id: &str,
    provider: Arc<dyn ManagedChatProvider>,
) {
    let mut providers = MANAGED_PROVIDERS.lock();
    if let Some(cancel) = PROVIDER_PUMPS.lock().remove(backend_session_id) {
        cancel.send_replace(true);
    }
    LIVE_CALLBACKS.lock().invalidate_session(backend_session_id, "PROVIDER_REPLACED");
    providers.insert(backend_session_id.to_string(), provider);
}

pub fn unregister_managed_provider(backend_session_id: &str) {
    let mut providers = MANAGED_PROVIDERS.lock();
    providers.remove(backend_session_id);
    if let Some(cancel) = PROVIDER_PUMPS.lock().remove(backend_session_id) {
        cancel.send_replace(true);
    }
    LIVE_CALLBACKS
        .lock()
        .invalidate_session(backend_session_id, "PROVIDER_UNREGISTERED");
}

pub fn clear_managed_providers_for_test() {
    let mut providers = MANAGED_PROVIDERS.lock();
    for (_, cancel) in PROVIDER_PUMPS.lock().drain() { cancel.send_replace(true); }
    providers.clear();
}

pub struct SupervisorManagedProvider {
    target: TargetRef,
    supervisor: Arc<crate::ferryx_scope::chat::Supervisor>,
    thread_id: Mutex<Option<String>>,
}

impl SupervisorManagedProvider {
    pub fn new(
        target: TargetRef,
        supervisor: Arc<crate::ferryx_scope::chat::Supervisor>,
        initial_thread_id: Option<String>,
    ) -> Self {
        Self {
            target,
            supervisor,
            thread_id: Mutex::new(initial_thread_id),
        }
    }
}

#[async_trait::async_trait]
impl ManagedChatProvider for SupervisorManagedProvider {
    async fn send_turn(
        &self,
        target: &TargetRef,
        input: Vec<Value>,
    ) -> Result<DeliveryReceipt, String> {
        if target != &self.target {
            return Err("Managed provider target expired".into());
        }
        let thread_id = self
            .thread_id
            .lock()
            .clone()
            .ok_or_else(|| "Managed thread has not been initialized".to_string())?;

        let payload = json!({
            "threadId": thread_id,
            "input": input,
        });

        self.supervisor
            .request("turn/start", payload)
            .await
            .map_err(|e| format!("turn/start failed: {:?}", e))?;

        Ok(DeliveryReceipt {
            request_id: uuid::Uuid::new_v4().to_string(),
            target: target.clone(),
            stage: DeliveryStage::Accepted,
        })
    }

    async fn reply_callback(
        &self,
        callback_id: Value,
        thread_id: &str,
        turn_id: &str,
        result: Value,
    ) -> Result<(), String> {
        self.supervisor
            .callback(callback_id, thread_id, turn_id, result)
            .await
            .map_err(|e| format!("callback resolution failed: {:?}", e))
    }

    async fn stop_agent(&self, target: &TargetRef) -> Result<(), String> {
        if target != &self.target {
            return Err("Managed provider target expired".into());
        }
        self.supervisor
            .stop()
            .await
            .map_err(|e| format!("stop failed: {:?}", e))
    }
}

pub fn register_supervisor_provider(
    target: TargetRef,
    supervisor: Arc<crate::ferryx_scope::chat::Supervisor>,
    initial_thread_id: Option<String>,
    state: Option<Arc<RemoteGatewayState>>,
) -> tokio::task::JoinHandle<()> {
    let session_id = target.backend_session_id.clone();
    let provider: Arc<dyn ManagedChatProvider> = Arc::new(SupervisorManagedProvider::new(
        target.clone(),
        Arc::clone(&supervisor),
        initial_thread_id,
    ));
    let (cancel, mut cancelled) = tokio::sync::watch::channel(false);
    {
        let mut providers = MANAGED_PROVIDERS.lock();
        if let Some(old) = PROVIDER_PUMPS.lock().insert(session_id.clone(), cancel) {
            old.send_replace(true);
        }
        LIVE_CALLBACKS.lock().invalidate_session(&session_id, "PROVIDER_REPLACED");
        providers.insert(session_id.clone(), provider.clone());
    }

    tokio::spawn(async move {
        loop {
            let frame = tokio::select! {
                biased;
                _ = cancelled.changed() => break,
                frame = supervisor.next() => match frame {
                    Ok(frame) => frame,
                    Err(crate::ferryx_scope::chat::ChatError::Timeout) => continue,
                    Err(_) => break,
                },
            };
            let Some(id_val) = frame.get("id") else {
                continue;
            };
            let Some(norm_id) = normalize_callback_id(id_val) else {
                continue;
            };

            let method = frame.get("method").and_then(Value::as_str).unwrap_or("");
            let params = frame.get("params").unwrap_or(&Value::Null);

            let (Some(thread_id), Some(turn_id)) =
                (params["threadId"].as_str(), params["turnId"].as_str()) else { continue };
            let thread_id = thread_id.to_string();
            let turn_id = turn_id.to_string();
            if !matches!(method, "item/tool/requestUserInput" | "item/commandExecution/requestApproval" | "item/fileChange/requestApproval") {
                continue;
            }

            let (kind, questions, text) = if method == "item/tool/requestUserInput" {
                let questions_parsed = params
                    .get("questions")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|q| {
                                serde_json::from_value::<CallbackQuestion>(q.clone()).ok()
                            })
                            .collect::<Vec<_>>()
                    });
                (CallbackKind::Question, questions_parsed, None)
            } else {
                let prompt_text = params
                    .get("message")
                    .or_else(|| params.get("command"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                (CallbackKind::Approval, None, prompt_text)
            };

            let entry = LiveCallbackEntry {
                callback_id: norm_id.clone(),
                thread_id: thread_id.clone(),
                turn_id: turn_id.clone(),
                callback_incarnation: 0,
                target: target.clone(),
                kind: kind.clone(),
                text: text.clone(),
                questions: questions.clone(),
                status: CallbackStatus::Pending,
                created_at: Instant::now(),
            };

            let providers = MANAGED_PROVIDERS.lock();
            if !providers.get(&session_id).is_some_and(|current| Arc::ptr_eq(current, &provider)) {
                break;
            }
            let mut registry = LIVE_CALLBACKS.lock();
            let Ok(callback_incarnation) = registry.register(entry) else { continue };

            if let Some(state_ref) = &state {
                if let Some(services) = &state_ref.machine_services {
                    let questions_val = questions.as_ref().map(|qs| json!(qs));
                    let cb_json = json!({
                        "id": norm_id,
                        "threadId": thread_id,
                        "turnId": turn_id,
                        "callbackIncarnation": callback_incarnation,
                        "target": target,
                        "kind": match kind {
                            CallbackKind::Approval => "approval",
                            CallbackKind::Question => "question",
                        },
                        "text": text,
                        "questions": questions_val,
                    });
                    services.workspaces.machine_events.publish_callback(
                        &session_id,
                        cb_json,
                    );
                }
            }
        }

        if let Err(error) = supervisor.stop().await {
            tracing::warn!(?error, "Managed supervisor cleanup failed");
        }
        let mut providers = MANAGED_PROVIDERS.lock();
        if providers.get(&session_id).is_some_and(|current| Arc::ptr_eq(current, &provider)) {
            providers.remove(&session_id);
            PROVIDER_PUMPS.lock().remove(&session_id);
            LIVE_CALLBACKS.lock().invalidate_session(&session_id, "PROVIDER_EXITED");
        }
    })
}

pub(super) fn scoped_success<T: Serialize>(data: T, request_id: &str) -> Response {
    Json(json!({
        "ok": true,
        "data": data,
        "requestId": request_id,
    }))
    .into_response()
}

pub(super) fn scoped_error(
    status: StatusCode,
    code: ScopeErrorCode,
    message: impl Into<String>,
    request_id: &str,
) -> Response {
    let err = ScopeError {
        code,
        message: message.into(),
        retryable: false,
        details: json!({}),
    };
    (
        status,
        Json(json!({
            "ok": false,
            "error": err,
            "requestId": request_id,
        })),
    )
        .into_response()
}

pub(crate) async fn authorize_chat_target(
    state: &RemoteGatewayState,
    headers: &HeaderMap,
    target: &TargetRef,
    request_id: &str,
) -> Result<DeviceInfo, Response> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(|| {
            scoped_error(
                StatusCode::UNAUTHORIZED,
                ScopeErrorCode::Unauthorized,
                "Authorization token missing or invalid",
                request_id,
            )
        })?;

    let device = state.auth_manager.validate_token(token).map_err(|_| {
        scoped_error(
            StatusCode::UNAUTHORIZED,
            ScopeErrorCode::Unauthorized,
            "Token validation failed",
            request_id,
        )
    })?;

    if device.permission != DevicePermission::Control || device.access_scope != DeviceAccessScope::Machine {
        return Err(scoped_error(
            StatusCode::FORBIDDEN,
            ScopeErrorCode::Forbidden,
            "Control permission required for managed chat operations",
            request_id,
        ));
    }

    let daemon_epoch = state
        .daemon_epoch
        .load(std::sync::atomic::Ordering::Relaxed);
    if target.epoch.0 != daemon_epoch {
        return Err(scoped_error(
            StatusCode::CONFLICT,
            ScopeErrorCode::TargetExpired,
            format!(
                "Target epoch {} does not match current daemon epoch {}",
                target.epoch.0, daemon_epoch
            ),
            request_id,
        ));
    }

    if target.owner_id.is_empty() || target.owner_id != device.id {
        return Err(scoped_error(
            StatusCode::FORBIDDEN,
            ScopeErrorCode::Forbidden,
            format!(
                "Target owner_id '{}' does not match authenticated device '{}'",
                target.owner_id, device.id
            ),
            request_id,
        ));
    }

    if state
        .session_backend
        .describe_session(&target.backend_session_id)
        .await
        .is_err()
    {
        return Err(scoped_error(
            StatusCode::NOT_FOUND,
            ScopeErrorCode::NotFound,
            format!("Session '{}' not found", target.backend_session_id),
            request_id,
        ));
    }

    Ok(device)
}

pub async fn managed_chat_send(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(req): Json<ManagedChatSendRequest>,
) -> Response {
    if let Err(resp) = authorize_chat_target(&state, &headers, &req.target, &req.request_id).await {
        return resp;
    }

    let text_len = req.draft.text.len();
    if text_len > 65536 {
        return scoped_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            ScopeErrorCode::PayloadTooLarge,
            "Draft text exceeds maximum allowed size of 65536 bytes",
            &req.request_id,
        );
    }

    if req.draft.attachments.len() > ATTACHMENT_MAX_FILES_PER_TURN {
        return scoped_error(
            StatusCode::BAD_REQUEST,
            ScopeErrorCode::InvalidRequest,
            format!(
                "Too many attachments ({} > {})",
                req.draft.attachments.len(),
                ATTACHMENT_MAX_FILES_PER_TURN
            ),
            &req.request_id,
        );
    }

    let total_attachment_bytes = req.draft.attachments.iter().try_fold(0u64, |total, receipt| {
        total.checked_add(receipt.size_bytes)
    });
    let Some(total_attachment_bytes) = total_attachment_bytes else {
        return scoped_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            ScopeErrorCode::PayloadTooLarge,
            "Total attachment size exceeds maximum turn limit of 20MB",
            &req.request_id,
        );
    };
    if req.draft.attachments.iter().any(|a| a.size_bytes > ATTACHMENT_MAX_FILE_BYTES) {
        return scoped_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            ScopeErrorCode::PayloadTooLarge,
            "Attachment exceeds maximum file size of 10MB",
            &req.request_id,
        );
    }
    if total_attachment_bytes > ATTACHMENT_MAX_TURN_BYTES {
        return scoped_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            ScopeErrorCode::PayloadTooLarge,
            "Total attachment size exceeds maximum turn limit of 20MB",
            &req.request_id,
        );
    }

    let staging = super::attachment_api::GatewayStagedAttachments;
    for receipt in &req.draft.attachments {
        use crate::ferryx_scope::chat::attachments::StagedAttachments;
        if let Err(err) = staging.verified_input(&req.target, receipt) {
            return scoped_error(
                StatusCode::BAD_REQUEST,
                ScopeErrorCode::InvalidRequest,
                format!("Invalid or unverified attachment {}: {:?}", receipt.attachment_id, err),
                &req.request_id,
            );
        }
    }

    let provider = {
        let providers = MANAGED_PROVIDERS.lock();
        providers.get(&req.target.backend_session_id).cloned()
    };

    let Some(provider) = provider else {
        return scoped_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            ScopeErrorCode::Unsupported,
            format!(
                "No active managed agent provider attached to session '{}'; raw terminal PTY cannot be treated as conversation",
                req.target.backend_session_id
            ),
            &req.request_id,
        );
    };

    let inputs = vec![json!({
        "type": "text",
        "text": req.draft.text,
    })];

    match provider.send_turn(&req.target, inputs).await {
        Ok(mut receipt) => {
            receipt.request_id = req.request_id.clone();
            scoped_success(receipt, &req.request_id)
        },
        Err(err) => scoped_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            ScopeErrorCode::Unsupported,
            format!("Provider failed to accept turn: {err}"),
            &req.request_id,
        ),
    }
}

pub async fn managed_chat_reply(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(req): Json<ManagedChatReplyRequest>,
) -> Response {
    if let Err(resp) = authorize_chat_target(&state, &headers, &req.target, &req.request_id).await {
        return resp;
    }

    let callback_id = match normalize_callback_id(&req.callback_id) {
        Some(id) => id,
        None => {
            return scoped_error(
                StatusCode::BAD_REQUEST,
                ScopeErrorCode::InvalidRequest,
                "Missing or invalid callback ID",
                &req.request_id,
            )
        }
    };

    if req.thread_id.is_empty() {
        return scoped_error(
            StatusCode::BAD_REQUEST,
            ScopeErrorCode::InvalidRequest,
            "Missing thread ID in callback reply",
            &req.request_id,
        );
    }

    if req.turn_id.is_empty() {
        return scoped_error(
            StatusCode::BAD_REQUEST,
            ScopeErrorCode::InvalidRequest,
            "Missing turn ID in callback reply",
            &req.request_id,
        );
    }

    let provider = MANAGED_PROVIDERS.lock().get(&req.target.backend_session_id).cloned();
    let Some(provider) = provider else {
        return scoped_error(StatusCode::UNPROCESSABLE_ENTITY, ScopeErrorCode::Unsupported,
            "Managed provider is no longer available", &req.request_id);
    };
    let resolved_entry = {
        let mut registry = LIVE_CALLBACKS.lock();
        registry.resolve_reply(
            &req.target,
            &callback_id,
            &req.thread_id,
            &req.turn_id,
            req.callback_incarnation,
            &req.result,
        )
    };

    let entry = match resolved_entry {
        Ok(entry) => entry,
        Err(CallbackError::NotFound) => {
            return scoped_error(
                StatusCode::NOT_FOUND,
                ScopeErrorCode::NotFound,
                format!(
                    "Callback '{}' not found in live registry; transcript prose cannot authorize replies",
                    callback_id
                ),
                &req.request_id,
            );
        }
        Err(CallbackError::AlreadyResolved) => {
            return scoped_error(
                StatusCode::CONFLICT,
                ScopeErrorCode::RequestConflict,
                format!("Callback '{}' has already been resolved; replay rejected", callback_id),
                &req.request_id,
            );
        }
        Err(CallbackError::IncarnationMismatch) => {
            return scoped_error(
                StatusCode::CONFLICT,
                ScopeErrorCode::StaleCallback,
                format!("Callback '{}' incarnation is stale", callback_id),
                &req.request_id,
            );
        }
        Err(CallbackError::StaleTurn) | Err(CallbackError::TurnMismatch) => {
            return scoped_error(
                StatusCode::CONFLICT,
                ScopeErrorCode::TargetExpired,
                format!("Turn '{}' is stale or mismatched for callback '{}'", req.turn_id, callback_id),
                &req.request_id,
            );
        }
        Err(CallbackError::ThreadMismatch) => {
            return scoped_error(
                StatusCode::CONFLICT,
                ScopeErrorCode::InvalidRequest,
                format!("Thread ID mismatch for callback '{}'", callback_id),
                &req.request_id,
            );
        }
        Err(CallbackError::TargetMismatch(msg)) => {
            return scoped_error(
                StatusCode::CONFLICT,
                ScopeErrorCode::TargetExpired,
                format!("Target mismatch for callback '{}': {}", callback_id, msg),
                &req.request_id,
            );
        }
        Err(CallbackError::Invalidated(reason)) => {
            return scoped_error(
                StatusCode::CONFLICT,
                ScopeErrorCode::TargetExpired,
                format!("Callback '{}' was invalidated ({})", callback_id, reason),
                &req.request_id,
            );
        }
        Err(CallbackError::InvalidDecision) => {
            return scoped_error(
                StatusCode::BAD_REQUEST,
                ScopeErrorCode::InvalidRequest,
                "Invalid decision: approval decision must be 'accept', 'decline', or 'cancel'",
                &req.request_id,
            );
        }
        Err(CallbackError::InvalidAnswers(reason)) => {
            return scoped_error(
                StatusCode::BAD_REQUEST,
                ScopeErrorCode::InvalidRequest,
                format!("Invalid question answers: {}", reason),
                &req.request_id,
            );
        }
    };

    let dispatch = provider
            .reply_callback(
                req.callback_id.clone(),
                &entry.thread_id,
                &entry.turn_id,
                req.result.clone(),
            )
            .await;
    {
        let mut registry = LIVE_CALLBACKS.lock();
        registry.finish_dispatch(&entry, &req.result, dispatch.is_ok());
    }
    if let Err(err) = dispatch {
            return scoped_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                ScopeErrorCode::Unsupported,
                format!("PROVIDER_DISPATCH_FAILED: Provider callback dispatch failed: {err}"),
                &req.request_id,
            );
    }

    if let Some(services) = &state.machine_services {
        services.workspaces.machine_events.publish_callback_resolved(
            &req.target.backend_session_id,
            &entry.callback_id,
        );
    }

    scoped_success(
        CallbackResolutionReceipt {
            callback_id: entry.callback_id,
            resolved: true,
        },
        &req.request_id,
    )
}

pub async fn managed_chat_stop(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Json(req): Json<ManagedChatStopRequest>,
) -> Response {
    if let Err(resp) = authorize_chat_target(&state, &headers, &req.target, &req.request_id).await {
        return resp;
    }

    {
        let mut callbacks = LIVE_CALLBACKS.lock();
        callbacks.invalidate_session(&req.target.backend_session_id, "AGENT_STOPPED");
    }

    let provider = {
        let providers = MANAGED_PROVIDERS.lock();
        providers.get(&req.target.backend_session_id).cloned()
    };

    let Some(provider) = provider else {
        return scoped_error(StatusCode::UNPROCESSABLE_ENTITY, ScopeErrorCode::Unsupported,
            "No active managed provider", &req.request_id);
    };
    if let Err(error) = provider.stop_agent(&req.target).await {
        return scoped_error(StatusCode::BAD_GATEWAY, ScopeErrorCode::Unsupported, error, &req.request_id);
    }
    {
        let mut providers = MANAGED_PROVIDERS.lock();
        if providers.get(&req.target.backend_session_id).is_some_and(|current| Arc::ptr_eq(current, &provider)) {
            providers.remove(&req.target.backend_session_id);
            if let Some(cancel) = PROVIDER_PUMPS.lock().remove(&req.target.backend_session_id) { cancel.send_replace(true); }
        }
    }

    scoped_success(ChatStopReceipt { stopped: true }, &req.request_id)
}

pub async fn managed_chat_list_callbacks(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Query(query): Query<ChatCallbacksQuery>,
) -> Response {
    let token = match headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
    {
        Some(t) => t,
        None => {
            return scoped_error(
                StatusCode::UNAUTHORIZED,
                ScopeErrorCode::Unauthorized,
                "Authorization token missing",
                "chat-callbacks-query",
            )
        }
    };

    let device = match state.auth_manager.validate_token(token) {
        Ok(device) if device.access_scope == DeviceAccessScope::Machine && device.permission == DevicePermission::Control => device,
        _ => return scoped_error(
            StatusCode::UNAUTHORIZED,
            ScopeErrorCode::Unauthorized,
            "Invalid token",
            "chat-callbacks-query",
        ),
    };

    let summaries = {
        let registry = LIVE_CALLBACKS.lock();
        registry.list_pending(
            &query.backend_session_id,
            query.thread_id.as_deref(),
        ).into_iter().filter(|callback| callback.target.owner_id == device.id && callback.target.epoch.0 == state.daemon_epoch.load(std::sync::atomic::Ordering::Relaxed)).collect::<Vec<_>>()
    };

    scoped_success(summaries, "chat-callbacks-query")
}

pub fn managed_chat_router(state: Arc<RemoteGatewayState>) -> Router {
    Router::new()
        .route("/api/v1/chat/start", post(super::managed_chat_lifecycle::managed_chat_start))
        .route("/api/v1/chat/send", post(managed_chat_send))
        .route("/api/v1/chat/reply", post(managed_chat_reply))
        .route("/api/v1/chat/stop", post(managed_chat_stop))
        .route("/api/v1/chat/callbacks", get(managed_chat_list_callbacks))
        .with_state(state)
}
