use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::mailer::Mailer;
use super::origin::DeploymentMode;
use super::store::{
    lock_account_dir, normalize_email, now_secs, random_token, signing_key_path, token_hash,
    write_private_json, AccountSigningKeyRecord, AccountStore, EnrollmentCodeRecord, GrantRecord,
    LoginCodeRecord, MachineRecord, SessionRecord, UserRecord, DEFAULT_LOGIN_REQUESTS_PER_HOUR,
    DEFAULT_MAX_BODY_BYTES, ENROLLMENT_CODE_TTL, LOGIN_CODE_TTL, SESSION_TTL,
};
use crate::remote::account_grants::{submission_signing_input, GrantSubmission};
use crate::remote::account_protocol::{
    AccountEnrollChallenge, AccountEnrollRequest, AccountEnrollResponse, AccountGrantOffer,
    AccountGrantOfferEnvelope, AccountGrantRequest, AccountGrantResponse, AccountGrantScope,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};

pub const GRANT_TTL: Duration = Duration::from_secs(600);

/// Freshness window for determining machine online status from `last_seen_at`.
///
/// In Ferryx, daemons establish active presence through control channels and periodic
/// keepalives. When the account service is hosted separately or without a direct
/// in-memory handle to the relay's control-channel registry, machine liveness is
/// derived from the recency of `last_seen_at`.
///
/// A 300-second (5-minute) freshness window provides an optimal balance: it tolerates
/// transient network reconnects, relay restarts, and client backoff cycles (which typically
/// operate on 30-60 second intervals) without prematurely showing an active daemon as
/// OFFLINE, while reliably reporting machines offline after 5 minutes of silence.
pub const MACHINE_ONLINE_FRESHNESS_WINDOW_SECS: u64 = 300;

pub type MachineLivenessProbe = Arc<dyn Fn(&str) -> bool + Send + Sync>;

pub struct AccountState {
    pub data_dir: PathBuf,
    pub origin: String,
    pub relay_origin: String,
    pub deployment_mode: DeploymentMode,
    pub mailer: Arc<dyn Mailer>,
    pub login_requests_per_hour: u32,
    pub max_body_bytes: usize,
    http_client: reqwest::Client,
    login_attempts: Mutex<HashMap<String, Vec<Instant>>>,
    challenges: Mutex<HashMap<String, EnrollChallenge>>,
    signing_key: Mutex<Option<Arc<SigningKey>>>,
    liveness_probe: Mutex<Option<MachineLivenessProbe>>,
}

#[derive(Debug, Clone)]
struct EnrollChallenge {
    nonce: String,
    expires_at: u64,
}

fn parse_signing_key(bytes: &[u8]) -> Result<SigningKey, String> {
    let record: AccountSigningKeyRecord = serde_json::from_slice(bytes)
        .map_err(|error| format!("ACCOUNT_SIGNING_KEY_CORRUPT: {error}"))?;
    let secret_bytes = STANDARD
        .decode(&record.private_key)
        .map_err(|error| format!("ACCOUNT_SIGNING_KEY_INVALID: {error}"))?;
    let secret_arr: [u8; 32] = secret_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "ACCOUNT_SIGNING_KEY_INVALID: private key must be 32 bytes".to_string())?;
    let key = SigningKey::from_bytes(&secret_arr);
    let public_str = STANDARD.encode(key.verifying_key().as_bytes());
    if public_str != record.public_key {
        return Err("ACCOUNT_SIGNING_KEY_MISMATCH: public key does not match private key".into());
    }
    Ok(key)
}

fn load_or_generate_account_signing_key(dir: &Path) -> Result<SigningKey, String> {
    let path = signing_key_path(dir);
    match std::fs::read(&path) {
        Ok(bytes) => return parse_signing_key(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Failed to read account signing key: {error}")),
    }

    let _guard = lock_account_dir(dir)
        .map_err(|error| format!("Failed to lock account directory for signing key: {error}"))?;

    match std::fs::read(&path) {
        Ok(bytes) => return parse_signing_key(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Failed to read account signing key: {error}")),
    }

    let key = SigningKey::generate(&mut rand::rngs::OsRng);
    let record = AccountSigningKeyRecord {
        public_key: STANDARD.encode(key.verifying_key().as_bytes()),
        private_key: STANDARD.encode(key.to_bytes()),
    };
    write_private_json(&path, &record)
        .map_err(|error| format!("Failed to persist account signing key: {error}"))?;
    Ok(key)
}

impl AccountState {
    pub fn new(
        data_dir: impl Into<PathBuf>,
        origin: impl Into<String>,
        mailer: Arc<dyn Mailer>,
    ) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        Self {
            data_dir: data_dir.into(),
            origin: origin.into(),
            relay_origin: crate::remote::state::DEFAULT_RELAY_URL.to_string(),
            deployment_mode: DeploymentMode::SelfHost,
            mailer,
            login_requests_per_hour: DEFAULT_LOGIN_REQUESTS_PER_HOUR,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            http_client,
            login_attempts: Mutex::new(HashMap::new()),
            challenges: Mutex::new(HashMap::new()),
            signing_key: Mutex::new(None),
            liveness_probe: Mutex::new(None),
        }
    }

    pub fn with_deployment_mode(mut self, deployment_mode: DeploymentMode) -> Self {
        self.deployment_mode = deployment_mode;
        self
    }

    pub fn with_liveness_probe(self, probe: MachineLivenessProbe) -> Self {
        *self.liveness_probe.lock() = Some(probe);
        self
    }

    pub fn set_liveness_probe(&self, probe: Option<MachineLivenessProbe>) {
        *self.liveness_probe.lock() = probe;
    }

    pub fn is_machine_online(&self, machine: &MachineRecord, now: u64) -> bool {
        if let Some(ref probe) = *self.liveness_probe.lock() {
            if probe(&machine.machine_id) {
                return true;
            }
        }
        if machine.last_seen_at == 0 {
            return false;
        }
        machine.last_seen_at.abs_diff(now) <= MACHINE_ONLINE_FRESHNESS_WINDOW_SECS
    }

    pub fn machine_view(&self, record: &MachineRecord, now: u64) -> MachineViewResponse {
        let online = self.is_machine_online(record, now);
        MachineViewResponse::from_record(record, online)
    }

    pub fn signing_key(&self) -> Result<Arc<SigningKey>, String> {
        let mut guard = self.signing_key.lock();
        if let Some(key) = guard.as_ref() {
            return Ok(key.clone());
        }
        let key = load_or_generate_account_signing_key(&self.data_dir)?;
        let arc_key = Arc::new(key);
        *guard = Some(arc_key.clone());
        Ok(arc_key)
    }

    pub fn account_public_key(&self) -> String {
        let key = self
            .signing_key()
            .expect("account signing key must be available");
        STANDARD.encode(key.verifying_key().as_bytes())
    }

    pub fn with_relay_origin(mut self, relay_origin: impl Into<String>) -> Self {
        self.relay_origin = relay_origin.into();
        self
    }

    pub fn with_limits(mut self, per_hour: u32, max_body_bytes: usize) -> Self {
        self.login_requests_per_hour = per_hour;
        self.max_body_bytes = max_body_bytes;
        self
    }

    fn allow_login_request(&self, email: &str) -> bool {
        let mut attempts = self.login_attempts.lock();
        let window = attempts.entry(email.to_string()).or_default();
        window.retain(|at| at.elapsed() < Duration::from_secs(3600));
        if window.len() as u32 >= self.login_requests_per_hour {
            return false;
        }
        window.push(Instant::now());
        true
    }

    fn open_challenge(&self, machine_id: &str) -> AccountEnrollChallenge {
        let nonce = random_token();
        let timestamp = now_secs();
        let expires_at = timestamp + 120;
        let mut challenges = self.challenges.lock();
        challenges.retain(|_, challenge| challenge.expires_at > timestamp);
        challenges.insert(
            machine_id.to_string(),
            EnrollChallenge {
                nonce: nonce.clone(),
                expires_at,
            },
        );
        AccountEnrollChallenge {
            nonce,
            timestamp,
            audience: self.origin.clone(),
            expires_at,
        }
    }

    fn take_challenge(&self, machine_id: &str, nonce: &str, now: u64) -> bool {
        let mut challenges = self.challenges.lock();
        let Some(challenge) = challenges.remove(machine_id) else {
            return false;
        };
        challenge.nonce == nonce && challenge.expires_at > now
    }

    pub(crate) fn load(&self) -> Result<AccountStore, String> {
        AccountStore::load(&self.data_dir)
    }

    pub(crate) fn mutate<T>(
        &self,
        change: impl FnOnce(&mut AccountStore) -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        AccountStore::mutate_transaction(&self.data_dir, change)
    }

    fn read<T>(
        &self,
        read: impl FnOnce(&AccountStore) -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        let store = self.load().map_err(ApiError::internal)?;
        read(&store)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

impl From<String> for ApiError {
    fn from(error: String) -> Self {
        Self::internal(error)
    }
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            details: None,
        }
    }

    /// Attaches the structured `details` object clients branch on (AGENTS.md: no regex matching
    /// on error strings). Omitted from the wire body when absent.
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", message)
    }

    fn unauthorized(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, code, message)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorBody {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<serde_json::Value>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                code: self.code,
                message: self.message,
                details: self.details,
            }),
        )
            .into_response()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountPublicKeyResponse {
    pub public_key: String,
}

pub async fn get_public_key(
    State(state): State<Arc<AccountState>>,
) -> Result<Json<AccountPublicKeyResponse>, ApiError> {
    let key = state
        .signing_key()
        .map_err(|error| ApiError::internal(format!("signing key unavailable: {error}")))?;
    Ok(Json(AccountPublicKeyResponse {
        public_key: STANDARD.encode(key.verifying_key().as_bytes()),
    }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountHealthResponse {
    pub ok: bool,
}

/// Cheap, side-effect-free probe so a client can tell whether this origin serves the account API.
pub async fn health_check() -> Json<AccountHealthResponse> {
    Json(AccountHealthResponse { ok: true })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoginRequestBody {
    pub email: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequestResponse {
    pub login_handle: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoginPollBody {
    pub login_handle: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginPollResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoginConsumeBody {
    pub code: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginConsumeResponse {
    pub token: String,
    pub account_id: String,
    pub email: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrollmentCodeResponse {
    pub code: String,
    pub expires_at: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineViewResponse {
    pub machine_record_id: String,
    pub machine_id: String,
    pub display_name: String,
    pub public_key: String,
    pub attach_public_key: String,
    pub relay_origin: String,
    pub platform: String,
    pub online: bool,
    pub enrollment_epoch: u64,
    pub last_seen_at: u64,
}

impl MachineViewResponse {
    pub fn from_record(record: &MachineRecord, online: bool) -> Self {
        Self {
            machine_record_id: record.machine_record_id.clone(),
            machine_id: record.machine_id.clone(),
            display_name: record.display_name.clone(),
            public_key: record.public_key.clone(),
            attach_public_key: record.attach_public_key.clone(),
            relay_origin: record.relay_origin.clone(),
            platform: record.platform.clone(),
            online,
            enrollment_epoch: record.enrollment_epoch,
            last_seen_at: record.last_seen_at,
        }
    }
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    value
        .strip_prefix("Bearer ")
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
}

fn require_user(state: &AccountState, headers: &HeaderMap) -> Result<UserRecord, ApiError> {
    let token = bearer(headers)
        .ok_or_else(|| ApiError::unauthorized("UNAUTHORIZED", "missing bearer token"))?;
    let now = now_secs();
    state.read(|store| {
        store
            .user_for_session(&token, now)
            .cloned()
            .ok_or_else(|| ApiError::unauthorized("UNAUTHORIZED", "unknown or expired session"))
    })
}

pub(crate) fn authenticate_user(
    state: &AccountState,
    headers: &HeaderMap,
) -> Result<UserRecord, ApiError> {
    require_user(state, headers)
}

pub(crate) fn machine_for_owner(
    state: &AccountState,
    user_id: &str,
    machine_id: &str,
) -> Option<MachineRecord> {
    state
        .read(|store| {
            Ok(store
                .machines
                .values()
                .find(|m| m.machine_id == machine_id && m.owner_user_id == user_id)
                .cloned())
        })
        .ok()
        .flatten()
}

/// Looks up an enrolled machine by the daemon's own machine id, regardless of owner.
///
/// The relay's control-channel admission uses this: on an account-enabled relay a machine
/// that enrolled itself with its account may open its control tunnel using the machine key
/// it enrolled, without the operator having to hand out a static enrollment token for it.
/// Ownership is proven by the exact `machine_id` + `public_key` pair recorded at enrollment.
pub fn enrolled_machine_by_id(state: &AccountState, machine_id: &str) -> Option<MachineRecord> {
    try_enrolled_machine_by_id(state, machine_id).ok().flatten()
}

/// Looks up an enrolled machine by machine id, propagating any underlying store read error.
///
/// Used by relay control admission when billing evaluation is enabled so that a store read
/// failure fails closed rather than bypassing entitlement checks.
pub fn try_enrolled_machine_by_id(
    state: &AccountState,
    machine_id: &str,
) -> Result<Option<MachineRecord>, ApiError> {
    state.read(|store| {
        Ok(store
            .machines
            .values()
            .find(|m| m.machine_id == machine_id)
            .cloned())
    })
}

pub fn touch_enrolled_machine(state: &AccountState, machine_id: &str) -> bool {
    let now = now_secs();
    state
        .mutate(|store| {
            if let Some(m) = store
                .machines
                .values_mut()
                .find(|m| m.machine_id == machine_id)
            {
                m.last_seen_at = now;
                Ok(true)
            } else {
                Ok(false)
            }
        })
        .unwrap_or(false)
}

async fn parse_json<T: for<'de> Deserialize<'de>>(body: Bytes) -> Result<T, ApiError> {
    serde_json::from_slice(&body)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, "BAD_REQUEST", error.to_string()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAuthRequestBody {
    pub email: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAuthResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePollRequestBody {
    pub device_code: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePollResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enrollment_code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeviceApproveQuery {
    pub code: String,
    pub token: String,
}

pub async fn device_request(
    State(state): State<Arc<AccountState>>,
    body: Bytes,
) -> Result<Json<DeviceAuthResponse>, ApiError> {
    let request: DeviceAuthRequestBody = parse_json(body).await?;
    let email = normalize_email(&request.email);
    if email.is_empty() || !email.contains('@') {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "valid email is required",
        ));
    }
    let device_code = random_token();
    let mut rng = rand::rngs::OsRng;
    let chars: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut part1 = String::new();
    let mut part2 = String::new();
    for _ in 0..4 {
        part1.push(chars[rand::Rng::gen_range(&mut rng, 0..chars.len())] as char);
        part2.push(chars[rand::Rng::gen_range(&mut rng, 0..chars.len())] as char);
    }
    let user_code = format!("{part1}-{part2}");
    let origin = state.origin.trim_end_matches('/');
    let verification_uri = format!("{origin}/device");
    let email_token = random_token();
    let email_token_hash = token_hash(&email_token);
    let email_approve_url =
        format!("{origin}/api/account/v1/device/approve?code={user_code}&token={email_token}");
    state
        .mailer
        .send_magic_link(&email, &email_approve_url)
        .map_err(|e| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "MAIL_FAILED",
                e.to_string(),
            )
        })?;

    let expires_at = now_secs() + 900;
    state.mutate(|store| {
        store.device_auths.retain(|_, d| d.expires_at > now_secs());
        store.device_auths.insert(
            token_hash(&device_code),
            crate::account::store::DeviceAuthRecord {
                device_code_hash: token_hash(&device_code),
                user_code: user_code.clone(),
                email: email.clone(),
                email_token_hash,
                enrollment_code: None,
                expires_at,
            },
        );
        Ok(())
    })?;

    Ok(Json(DeviceAuthResponse {
        device_code,
        user_code,
        verification_uri,
        expires_in: 900,
        interval: 2,
    }))
}

pub async fn device_poll(
    State(state): State<Arc<AccountState>>,
    body: Bytes,
) -> Result<Json<DevicePollResponse>, ApiError> {
    let request: DevicePollRequestBody = parse_json(body).await?;
    let now = now_secs();
    let code_hash = token_hash(&request.device_code);
    state.mutate(|store| {
        let record = store.device_auths.get(&code_hash).ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "DEVICE_CODE_INVALID",
                "invalid code",
            )
        })?;
        if record.expires_at <= now {
            store.device_auths.remove(&code_hash);
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "DEVICE_CODE_EXPIRED",
                "expired",
            ));
        }
        if let Some(ref enrollment_code) = record.enrollment_code {
            let code = enrollment_code.clone();
            store.device_auths.remove(&code_hash);
            Ok(Json(DevicePollResponse {
                status: "approved".into(),
                enrollment_code: Some(code),
            }))
        } else {
            Ok(Json(DevicePollResponse {
                status: "authorization_pending".into(),
                enrollment_code: None,
            }))
        }
    })
}

pub async fn device_approve_get(
    State(state): State<Arc<AccountState>>,
    axum::extract::Query(query): axum::extract::Query<DeviceApproveQuery>,
) -> Result<axum::response::Html<&'static str>, ApiError> {
    let now = now_secs();
    let code_clean = query.code.trim().to_uppercase();
    let token_clean = query.token.trim();
    if token_clean.is_empty() {
        return Err(ApiError::unauthorized(
            "EMAIL_TOKEN_REQUIRED",
            "email verification token is required",
        ));
    }
    let token_hash_expected = token_hash(token_clean);
    state.mutate(|store| {
        let mut found: Option<(String, String, String)> = None;
        for (key, record) in store.device_auths.iter() {
            if record.user_code == code_clean && record.expires_at > now {
                if record.email_token_hash != token_hash_expected {
                    return Err(ApiError::unauthorized(
                        "EMAIL_TOKEN_INVALID",
                        "invalid email token",
                    ));
                }
                let email = record.email.clone();
                let user_id = store
                    .users
                    .values()
                    .find(|u| u.email == email)
                    .map(|u| u.user_id.clone())
                    .unwrap_or_else(|| {
                        let uid = format!("usr_{}", &random_token()[..16]);
                        store.users.insert(
                            uid.clone(),
                            crate::account::store::UserRecord {
                                user_id: uid.clone(),
                                email,
                                created_at: now,
                            },
                        );
                        uid
                    });
                found = Some((key.clone(), user_id, state.origin.clone()));
                break;
            }
        }
        let (found_key, user_id, account_origin) = found.ok_or_else(|| {
            ApiError::unauthorized("EMAIL_TOKEN_INVALID", "email token is invalid or expired")
        })?;

        let enrollment_code = random_token();
        store.enrollment_codes.insert(
            token_hash(&enrollment_code),
            crate::account::store::EnrollmentCodeRecord {
                user_id,
                account_origin,
                expires_at: now + 600,
            },
        );

        if let Some(record) = store.device_auths.get_mut(&found_key) {
            record.enrollment_code = Some(enrollment_code);
        }
        Ok(())
    })?;

    Ok(axum::response::Html(
        "<!DOCTYPE html><html><body style=\"font-family: sans-serif; text-align: center; padding: 40px;\">\
        <h2 style=\"color: #10b981;\">&#10003; Ferryx Machine Approved!</h2>\
        <p>You can return to your terminal. This browser window can now be closed.</p>\
        </body></html>",
    ))
}

pub async fn login_request(
    State(state): State<Arc<AccountState>>,
    body: Bytes,
) -> Result<(StatusCode, Json<LoginRequestResponse>), ApiError> {
    let request: LoginRequestBody = parse_json(body).await?;
    let email = normalize_email(&request.email);
    if email.is_empty() || !email.contains('@') {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "email is required",
        ));
    }
    if !state.allow_login_request(&email) {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "LOGIN_RATE_LIMITED",
            "too many login requests for this address",
        ));
    }

    let code = random_token();
    let login_handle = random_token();
    let login_handle_hash = token_hash(&login_handle);
    let url = format!("{}/login?code={}", state.origin.trim_end_matches('/'), code);
    let delivered = state.mailer.send_magic_link(&email, &url);
    if let Err(error) = delivered {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "MAIL_FAILED",
            error.to_string(),
        ));
    }

    let expires_at = now_secs() + LOGIN_CODE_TTL.as_secs();
    state.mutate(|store| {
        store
            .login_codes
            .retain(|_, code| code.expires_at > now_secs());
        store.login_codes.insert(
            token_hash(&code),
            LoginCodeRecord {
                email: email.clone(),
                expires_at,
                consumed_at: None,
                login_handle_hash: Some(login_handle_hash),
            },
        );
        Ok(())
    })?;
    Ok((
        StatusCode::ACCEPTED,
        Json(LoginRequestResponse { login_handle }),
    ))
}

/// Polls for completion of a magic link sign-in request.
///
/// When the user opens the emailed link in a browser, that browser hits `/login?code=...`
/// which calls `login_consume`, minting a session for the browser and marking the code
/// as consumed (`consumed_at = Some(now)`).
///
/// That consumption serves as the approval signal for this endpoint: while `consumed_at`
/// is none, `login_poll` returns `{ status: "pending" }`. Once consumed, `login_poll`
/// mints a session for the requesting app and removes the record so both sides
/// (browser and app) mint at most one session each, and subsequent polls fail.
pub async fn login_poll(
    State(state): State<Arc<AccountState>>,
    body: Bytes,
) -> Result<Json<LoginPollResponse>, ApiError> {
    let request: LoginPollBody = parse_json(body).await?;
    let now = now_secs();
    let handle_hash = token_hash(request.login_handle.trim());

    let maybe_session = state.mutate(|store| {
        let found = store
            .login_codes
            .iter()
            .find(|(_, r)| r.login_handle_hash.as_deref() == Some(&handle_hash))
            .map(|(k, r)| (k.clone(), r.email.clone(), r.expires_at, r.consumed_at));
        let (key, email, expires_at, consumed_at) = found.ok_or_else(|| {
            ApiError::unauthorized("LOGIN_HANDLE_INVALID", "login handle is invalid")
        })?;
        if expires_at <= now {
            store.login_codes.remove(&key);
            return Err(ApiError::unauthorized(
                "LOGIN_CODE_EXPIRED",
                "login code has expired",
            ));
        }
        if consumed_at.is_none() {
            return Ok(None);
        }

        // The browser consumed the code; now mint a session for this polling app
        // and remove the record so this poll handle cannot be used again.
        store.login_codes.remove(&key);
        let user = store
            .user_by_email(&email)
            .cloned()
            .unwrap_or_else(|| UserRecord {
                user_id: uuid::Uuid::new_v4().to_string(),
                email: email.clone(),
                created_at: now,
            });
        store.users.insert(user.user_id.clone(), user.clone());
        let bearer_token = random_token();
        store.sessions.insert(
            token_hash(&bearer_token),
            SessionRecord {
                user_id: user.user_id.clone(),
                expires_at: now + SESSION_TTL.as_secs(),
            },
        );
        Ok(Some((bearer_token, user)))
    })?;

    if let Some((token, user)) = maybe_session {
        Ok(Json(LoginPollResponse {
            status: "approved".into(),
            token: Some(token),
            account_id: Some(user.user_id),
            email: Some(user.email),
        }))
    } else {
        Ok(Json(LoginPollResponse {
            status: "pending".into(),
            token: None,
            account_id: None,
            email: None,
        }))
    }
}

pub async fn login_consume(
    State(state): State<Arc<AccountState>>,
    body: Bytes,
) -> Result<Json<LoginConsumeResponse>, ApiError> {
    let request: LoginConsumeBody = parse_json(body).await?;
    let now = now_secs();
    let (bearer_token, user) = state.mutate(|store| {
        let key = token_hash(&request.code);
        let (email, expires_at, consumed_at) = match store.login_codes.get(&key) {
            Some(r) => (r.email.clone(), r.expires_at, r.consumed_at),
            None => {
                return Err(ApiError::unauthorized(
                    "LOGIN_CODE_USED",
                    "login code is unknown or already used",
                ));
            }
        };
        if consumed_at.is_some() {
            return Err(ApiError::unauthorized(
                "LOGIN_CODE_USED",
                "login code is unknown or already used",
            ));
        }
        if expires_at <= now {
            store.login_codes.remove(&key);
            return Err(ApiError::unauthorized(
                "LOGIN_CODE_EXPIRED",
                "login code has expired",
            ));
        }

        if let Some(record) = store.login_codes.get_mut(&key) {
            record.consumed_at = Some(now);
        }

        let user = store
            .user_by_email(&email)
            .cloned()
            .unwrap_or_else(|| UserRecord {
                user_id: uuid::Uuid::new_v4().to_string(),
                email: email.clone(),
                created_at: now,
            });
        store.users.insert(user.user_id.clone(), user.clone());
        let bearer_token = random_token();
        store.sessions.insert(
            token_hash(&bearer_token),
            SessionRecord {
                user_id: user.user_id.clone(),
                expires_at: now + SESSION_TTL.as_secs(),
            },
        );
        Ok((bearer_token, user))
    })?;

    Ok(Json(LoginConsumeResponse {
        token: bearer_token,
        account_id: user.user_id,
        email: user.email,
    }))
}

pub async fn logout(
    State(state): State<Arc<AccountState>>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let token = bearer(&headers)
        .ok_or_else(|| ApiError::unauthorized("UNAUTHORIZED", "missing bearer token"))?;
    let key = token_hash(&token);
    state.mutate(|store| {
        store.sessions.remove(&key);
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn create_enrollment_code(
    State(state): State<Arc<AccountState>>,
    headers: HeaderMap,
) -> Result<Json<EnrollmentCodeResponse>, ApiError> {
    let user = require_user(&state, &headers)?;
    let code = random_token();
    let expires_at = now_secs() + ENROLLMENT_CODE_TTL.as_secs();
    state.mutate(|store| {
        store.enrollment_codes.insert(
            token_hash(&code),
            EnrollmentCodeRecord {
                user_id: user.user_id.clone(),
                account_origin: state.origin.clone(),
                expires_at,
            },
        );
        Ok(())
    })?;
    Ok(Json(EnrollmentCodeResponse { code, expires_at }))
}

pub async fn list_machines(
    State(state): State<Arc<AccountState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<MachineViewResponse>>, ApiError> {
    let user = require_user(&state, &headers)?;
    let now = now_secs();
    state
        .read(|store| {
            Ok(store
                .machines
                .values()
                .filter(|machine| machine.owner_user_id == user.user_id)
                .map(|machine| state.machine_view(machine, now))
                .collect())
        })
        .map(Json)
}

impl From<&MachineRecord> for MachineViewResponse {
    fn from(record: &MachineRecord) -> Self {
        let now = now_secs();
        let online = record.last_seen_at > 0
            && record.last_seen_at.abs_diff(now) <= MACHINE_ONLINE_FRESHNESS_WINDOW_SECS;
        Self::from_record(record, online)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EnrollChallengeBody {
    machine_id: String,
}

fn platform_name(platform: &crate::remote::machine_protocol::Platform) -> String {
    use crate::remote::machine_protocol::Platform;
    match platform {
        Platform::Linux => "linux",
        Platform::Macos => "macos",
        Platform::Windows => "windows",
    }
    .to_string()
}

fn scope_name(scope: AccountGrantScope) -> String {
    match scope {
        AccountGrantScope::Mirror => "mirror",
        AccountGrantScope::Machine => "machine",
    }
    .to_string()
}

fn plan_key_str(plan: crate::account::billing::entitlement::PlanKey) -> &'static str {
    match plan {
        crate::account::billing::entitlement::PlanKey::Free => "free",
        crate::account::billing::entitlement::PlanKey::ProMonthly => "pro_monthly",
        crate::account::billing::entitlement::PlanKey::ProAnnual => "pro_annual",
        crate::account::billing::entitlement::PlanKey::TeamMonthly => "team_monthly",
        crate::account::billing::entitlement::PlanKey::TeamAnnual => "team_annual",
    }
}

fn entitlement_status_str(
    status: crate::account::billing::entitlement::EntitlementStatus,
) -> &'static str {
    match status {
        crate::account::billing::entitlement::EntitlementStatus::Ok => "ok",
        crate::account::billing::entitlement::EntitlementStatus::OverLimit => "over_limit",
        crate::account::billing::entitlement::EntitlementStatus::PastDue => "past_due",
        crate::account::billing::entitlement::EntitlementStatus::Stopped => "stopped",
    }
}

fn machines_used_for_user(store: &AccountStore, user_id: &str) -> u32 {
    let org_member = store
        .org_members
        .values()
        .filter(|m| m.user_id == user_id && store.orgs.contains_key(&m.org_id))
        .min_by(|left, right| left.org_id.cmp(&right.org_id));
    if let Some(member) = org_member {
        let org_users: std::collections::HashSet<&str> = store
            .org_members
            .values()
            .filter(|m| m.org_id == member.org_id)
            .map(|m| m.user_id.as_str())
            .collect();
        store
            .machines
            .values()
            .filter(|m| org_users.contains(m.owner_user_id.as_str()))
            .count() as u32
    } else {
        store
            .machines
            .values()
            .filter(|m| m.owner_user_id == user_id)
            .count() as u32
    }
}

fn owner_key_for_user(store: &AccountStore, user_id: &str) -> String {
    let org_member = store
        .org_members
        .values()
        .filter(|m| m.user_id == user_id && store.orgs.contains_key(&m.org_id))
        .min_by(|left, right| left.org_id.cmp(&right.org_id));
    match org_member {
        Some(m) => format!("org:{}", m.org_id),
        None => user_id.to_string(),
    }
}

pub(crate) fn owner_stopped_at(state: &AccountState, user_id: &str) -> Option<u64> {
    state
        .read(|store| {
            let key = owner_key_for_user(store, user_id);
            Ok(store.billing_states.get(&key).and_then(|b| b.stopped_at))
        })
        .ok()
        .flatten()
}

pub async fn enroll_challenge(
    State(state): State<Arc<AccountState>>,
    body: Bytes,
) -> Result<Json<AccountEnrollChallenge>, ApiError> {
    let request: EnrollChallengeBody = parse_json(body).await?;
    if request.machine_id.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "machineId is required",
        ));
    }
    Ok(Json(state.open_challenge(request.machine_id.trim())))
}

pub async fn enroll(
    State(state): State<Arc<AccountState>>,
    body: Bytes,
) -> Result<Json<AccountEnrollResponse>, ApiError> {
    let request: AccountEnrollRequest = parse_json(body).await?;
    if request.api_version != 1 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "ACCOUNT_VERSION_UNSUPPORTED",
            "only account api version 1 is supported",
        ));
    }
    let now = now_secs();
    let billing_now = crate::account::billing::routes::billing_now();
    if !state.take_challenge(&request.machine_id, &request.nonce, now) {
        return Err(ApiError::unauthorized(
            "ENROLL_CHALLENGE_INVALID",
            "unknown, reused, or expired enrollment challenge",
        ));
    }
    if now.abs_diff(request.timestamp) > 120 {
        return Err(ApiError::unauthorized(
            "ENROLL_STALE",
            "enrollment timestamp is outside the accepted window",
        ));
    }
    let code_hash = crate::remote::auth::enrollment_code_hash(&request.enrollment_code);
    if !crate::remote::auth::verify_account_enrollment(
        &request.public_key,
        &request.machine_id,
        &state.origin,
        &code_hash,
        &request.nonce,
        request.timestamp,
        &request.signature,
    ) {
        return Err(ApiError::unauthorized(
            "ENROLL_SIGNATURE_INVALID",
            "enrollment signature did not verify against the submitted machine key",
        ));
    }

    let state_clone = state.clone();
    let request_clone = request.clone();
    let (user_id, machine_record_id, epoch) = crate::ipc::run_blocking(move || {
        Ok((|| -> Result<(String, String, u64), ApiError> {
            let decision = state_clone.mutate(|store| {
                let record = store
                .enrollment_codes
                .get(&code_hash)
                .cloned()
                .ok_or_else(|| {
                    ApiError::unauthorized("ENROLL_CODE_INVALID", "unknown enrollment code")
                })?;
            if record.expires_at <= now {
                store.enrollment_codes.remove(&code_hash);
                return Err(ApiError::unauthorized(
                    "ENROLL_CODE_EXPIRED",
                    "enrollment code has expired",
                ));
            }
            if record.account_origin != state_clone.origin {
                return Err(ApiError::unauthorized(
                    "ENROLL_ORIGIN_MISMATCH",
                    "enrollment code was issued for a different account origin",
                ));
            }
            let user_id = record.user_id.clone();

            let existing = store
                .machines
                .values()
                .find(|machine| machine.machine_id == request_clone.machine_id)
                .cloned();

            if let Some(ref machine) = existing {
                if machine.owner_user_id != user_id {
                    return Err(ApiError::new(
                        StatusCode::CONFLICT,
                        "ACCOUNT_MACHINE_CLAIMED",
                        "this machine id is already enrolled to a different account",
                    ));
                }
            }

            if state_clone.deployment_mode.is_billing_enabled() {
                let entitlement = crate::account::billing::routes::entitlement_for_user_in_store(
                    store,
                    &user_id,
                    billing_now,
                );
                if entitlement.status == crate::account::billing::entitlement::EntitlementStatus::Stopped {
                    let owner_key = owner_key_for_user(store, &user_id);
                    let stopped_at = store
                        .billing_states
                        .get(&owner_key)
                        .and_then(|b| b.stopped_at)
                        .or(Some(billing_now));
                    let refusal = ApiError::new(
                        StatusCode::PAYMENT_REQUIRED,
                        "REMOTE_SUSPENDED",
                        "remote access is suspended for this account",
                    )
                    .with_details(serde_json::json!({
                        "plan": plan_key_str(entitlement.effective_plan),
                        "status": entitlement_status_str(entitlement.status),
                        "graceEndsAt": entitlement.grace_ends_at,
                        "stoppedAt": stopped_at,
                    }));
                    // Return Ok(Err(refusal)) so the updated billing state is committed to SQLite,
                    // while the machine is not enrolled and the enrollment code is preserved.
                    return Ok(Err(refusal));
                }

                if existing.is_none() && !entitlement.may_enroll_new {
                    let used = machines_used_for_user(store, &user_id);
                    let refusal = ApiError::new(
                        StatusCode::PAYMENT_REQUIRED,
                        "PLAN_LIMIT_REACHED",
                        "plan machine limit reached",
                    )
                    .with_details(serde_json::json!({
                        "plan": plan_key_str(entitlement.effective_plan),
                        "limit": entitlement.machine_limit,
                        "used": used,
                    }));
                    // Return Ok(Err(refusal)) so the updated billing state is committed to SQLite,
                    // while the machine is not enrolled and the enrollment code is preserved.
                    return Ok(Err(refusal));
                }
            }

            store.enrollment_codes.remove(&code_hash);
            let platform = platform_name(&request_clone.platform);
            let (machine_record_id, epoch) = match existing {
                Some(machine) => {
                    let epoch = machine.enrollment_epoch + 1;
                    let updated = MachineRecord {
                        machine_record_id: machine.machine_record_id.clone(),
                        owner_user_id: machine.owner_user_id.clone(),
                        machine_id: machine.machine_id.clone(),
                        display_name: request_clone.display_name.clone(),
                        public_key: request_clone.public_key.clone(),
                        attach_public_key: request_clone.attach_public_key.clone(),
                        relay_origin: state_clone.relay_origin.clone(),
                        platform,
                        enrollment_epoch: epoch,
                        enrolled_at: machine.enrolled_at,
                        last_seen_at: now,
                    };
                    store
                        .machines
                        .insert(updated.machine_record_id.clone(), updated.clone());
                    (updated.machine_record_id, epoch)
                }
                None => {
                    let record = MachineRecord {
                        machine_record_id: uuid::Uuid::new_v4().to_string(),
                        owner_user_id: user_id.clone(),
                        machine_id: request_clone.machine_id.clone(),
                        display_name: request_clone.display_name.clone(),
                        public_key: request_clone.public_key.clone(),
                        attach_public_key: request_clone.attach_public_key.clone(),
                        relay_origin: state_clone.relay_origin.clone(),
                        platform,
                        enrollment_epoch: 1,
                        enrolled_at: now,
                        last_seen_at: now,
                    };
                    store
                        .machines
                        .insert(record.machine_record_id.clone(), record.clone());
                    (record.machine_record_id, 1)
                }
            };
            Ok(Ok((user_id, machine_record_id, epoch)))
        })?;
        decision
        })())
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))??;

    Ok(Json(AccountEnrollResponse {
        account_id: user_id,
        machine_record_id,
        relay_origin: state.relay_origin.clone(),
        enrolled_at: now,
        enrollment_epoch: epoch.to_string(),
    }))
}

pub async fn issue_grant(
    State(state): State<Arc<AccountState>>,
    axum::extract::Path(machine_record_id): axum::extract::Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<AccountGrantResponse>, ApiError> {
    let request: AccountGrantRequest = parse_json(body).await?;
    let now = now_secs();
    let billing_now = crate::account::billing::routes::billing_now();

    let state_clone = state.clone();
    let machine_record_id_clone = machine_record_id.clone();
    let request_clone = request.clone();
    let headers_clone = headers.clone();

    let (grant, pairing_token, machine, offer, sealed_offer, submission) =
        crate::ipc::run_blocking(move || {
            Ok((|| -> Result<_, ApiError> {
                let user = require_user(&state_clone, &headers_clone)?;

                let decision = state_clone.mutate(|store| {
                let machine = store
                    .machines
                    .get(&machine_record_id_clone)
                    .cloned()
                    .ok_or_else(|| {
                        ApiError::new(
                            StatusCode::NOT_FOUND,
                            "MACHINE_NOT_FOUND",
                            "no such machine",
                        )
                    })?;
                if machine.owner_user_id != user.user_id {
                    return Err(ApiError::new(
                        StatusCode::NOT_FOUND,
                        "MACHINE_NOT_FOUND",
                        "no such machine on this account",
                    ));
                }
                if machine.enrollment_epoch.to_string() != request_clone.enrollment_epoch {
                    return Err(ApiError::new(
                        StatusCode::CONFLICT,
                        "ACCOUNT_ENROLLMENT_EPOCH_MISMATCH",
                        "machine re-enrolled since this view was fetched",
                    ));
                }

                if state_clone.deployment_mode.is_billing_enabled() {
                    let entitlement = crate::account::billing::routes::entitlement_for_user_in_store(
                        store,
                        &user.user_id,
                        billing_now,
                    );
                    if !entitlement.remote_allowed {
                        let owner_key = owner_key_for_user(store, &user.user_id);
                        let stopped_at = store
                            .billing_states
                            .get(&owner_key)
                            .and_then(|b| b.stopped_at)
                            .or(Some(billing_now));
                        let refusal = ApiError::new(
                            StatusCode::PAYMENT_REQUIRED,
                            "REMOTE_SUSPENDED",
                            "remote access is suspended for this account",
                        )
                        .with_details(serde_json::json!({
                            "plan": plan_key_str(entitlement.effective_plan),
                            "status": entitlement_status_str(entitlement.status),
                            "graceEndsAt": entitlement.grace_ends_at,
                            "stoppedAt": stopped_at,
                        }));
                        // Return Ok(Err(refusal)) so the updated billing state is committed to SQLite,
                        // while the grant is not issued.
                        return Ok(Err(refusal));
                    }
                }

                let pairing_token = random_token();
                let grant = GrantRecord {
                    grant_id: uuid::Uuid::new_v4().to_string(),
                    machine_record_id: machine.machine_record_id.clone(),
                    owner_user_id: machine.owner_user_id.clone(),
                    pairing_token_hash: token_hash(&pairing_token),
                    grant_scope: scope_name(request_clone.grant_scope),
                    device_attach_public_key: request_clone.attach_public_key.clone(),
                    installation_id: request_clone.installation_id.clone(),
                    issued_at: now,
                    expires_at: now + GRANT_TTL.as_secs(),
                };
                store.grants.insert(grant.grant_id.clone(), grant.clone());
                let offer = AccountGrantOffer {
                    grant_id: grant.grant_id.clone(),
                    machine_id: machine.machine_id.clone(),
                    enrollment_epoch: machine.enrollment_epoch.to_string(),
                    pairing_token: pairing_token.clone(),
                    device_label: request_clone.device_label.clone(),
                    installation_id: request_clone.installation_id.clone(),
                    grant_scope: request_clone.grant_scope,
                    expires_at: grant.expires_at,
                    device_attach_public_key: request_clone.attach_public_key.clone(),
                };
                Ok(Ok((grant, pairing_token, machine, offer)))
            })?;

            let (grant, pairing_token, machine, offer) = decision?;

            let sealed_offer = serde_json::to_vec(&offer)
                .map_err(|error| ApiError::internal(error.to_string()))
                .and_then(|plaintext| {
                    crate::remote::sealed_offer::seal_offer(
                        &machine.attach_public_key,
                        &machine.machine_id,
                        &offer.enrollment_epoch,
                        &plaintext,
                    )
                    .map_err(|error| ApiError::internal(error.to_string()))
                })
                .map(|sealed| AccountGrantOfferEnvelope {
                    machine_id: machine.machine_id.clone(),
                    enrollment_epoch: offer.enrollment_epoch.clone(),
                    sealed: STANDARD.encode(sealed),
                })?;

            let signing_key = state_clone
                .signing_key()
                .map_err(|error| ApiError::internal(format!("signing key unavailable: {error}")))?;
            let mut submission = GrantSubmission {
                machine_id: machine.machine_id.clone(),
                enrollment_epoch: offer.enrollment_epoch.clone(),
                envelope: sealed_offer.clone(),
                signature: String::new(),
            };
            let signing_input = submission_signing_input(&submission);
            let signature = signing_key.sign(&signing_input);
            submission.signature = STANDARD.encode(signature.to_bytes());

            Ok((grant, pairing_token, machine, offer, sealed_offer, submission))
            })())
        })
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;

    let relay_url = format!(
        "{}/api/v1/attach/grant",
        machine.relay_origin.trim_end_matches('/')
    );
    let rollback_grant = || {
        let rollback_state = state.clone();
        let rollback_grant_id = grant.grant_id.clone();
        let log_grant_id = rollback_grant_id.clone();
        async move {
            let rollback_res = crate::ipc::run_blocking(move || {
                Ok(rollback_state.mutate(|store| {
                    store.grants.remove(&rollback_grant_id);
                    Ok(())
                }))
            })
            .await;
            match rollback_res {
                Ok(Ok(())) => Ok(()),
                Ok(Err(api_err)) => {
                    eprintln!("failed to rollback grant {log_grant_id}: {api_err:?}");
                    Err(api_err)
                }
                Err(ipc_err) => {
                    eprintln!("failed to dispatch rollback grant {log_grant_id}: {ipc_err}");
                    Err(ApiError::internal(ipc_err.to_string()))
                }
            }
        }
    };

    let delivery_response = match state
        .http_client
        .post(&relay_url)
        .timeout(Duration::from_secs(5))
        .json(&submission)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(error) => {
            if let Err(rb_err) = rollback_grant().await {
                eprintln!("rollback failed on delivery send error: {rb_err:?}");
            }
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "GRANT_DELIVERY_FAILED",
                format!("failed to deliver grant to relay at {relay_url}: {error}"),
            ));
        }
    };

    if !delivery_response.status().is_success() {
        let status = delivery_response.status();
        let body = delivery_response
            .text()
            .await
            .unwrap_or_else(|_| "<unreadable response body>".to_string());
        if let Err(rb_err) = rollback_grant().await {
            eprintln!("rollback failed on delivery status error: {rb_err:?}");
        }
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "GRANT_DELIVERY_FAILED",
            format!("relay refused grant delivery with status {status}: {body}"),
        ));
    }

    let body_text = delivery_response.text().await.unwrap_or_default();
    let value: serde_json::Value = match serde_json::from_str(&body_text) {
        Ok(v) => v,
        Err(_) => {
            if let Err(rb_err) = rollback_grant().await {
                eprintln!("rollback failed on delivery response parse error: {rb_err:?}");
            }
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "GRANT_DELIVERY_FAILED",
                "relay returned invalid grant delivery response",
            ));
        }
    };

    if value.get("accepted").and_then(|v| v.as_bool()) == Some(false) {
        if let Err(rb_err) = rollback_grant().await {
            eprintln!("rollback failed on delivery rejected: {rb_err:?}");
        }
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "GRANT_DELIVERY_FAILED",
            "relay refused grant delivery",
        ));
    }

    let status = value
        .get("delivered")
        .and_then(|delivered| delivered.get("status"))
        .and_then(|s| s.as_str());

    match status {
        Some("ready") => {}
        Some(other) => {
            if let Err(rb_err) = rollback_grant().await {
                eprintln!("rollback failed on machine delivery error: {rb_err:?}");
            }
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "GRANT_DELIVERY_FAILED",
                format!("machine refused grant delivery: {other}"),
            ));
        }
        None => {
            if let Err(rb_err) = rollback_grant().await {
                eprintln!("rollback failed on missing delivered status: {rb_err:?}");
            }
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "GRANT_DELIVERY_FAILED",
                "relay grant delivery response missing delivered status",
            ));
        }
    }

    Ok(Json(AccountGrantResponse {
        grant_id: grant.grant_id,
        machine_id: machine.machine_id,
        relay_origin: machine.relay_origin,
        pairing_token,
        machine_attach_public_key: machine.attach_public_key,
        grant_scope: request.grant_scope,
        expires_at: grant.expires_at,
        sealed_offer: Some(sealed_offer),
    }))
}

pub fn router(state: Arc<AccountState>) -> Router {
    let limit = state.max_body_bytes;
    let mut router = Router::new()
        .route("/api/account/v1/public-key", get(get_public_key))
        .route("/api/account/v1/login/request", post(login_request))
        .route("/api/account/v1/login/poll", post(login_poll))
        .route("/api/account/v1/health", get(health_check))
        .route("/api/account/v1/login/consume", post(login_consume))
        .route("/api/account/v1/device/request", post(device_request))
        .route("/api/account/v1/device/poll", post(device_poll))
        .route("/api/account/v1/device/approve", get(device_approve_get))
        .route("/api/account/v1/logout", post(logout))
        .route(
            "/api/account/v1/enrollment-codes",
            post(create_enrollment_code),
        )
        .route("/api/account/v1/machines", get(list_machines))
        .route(
            "/api/account/v1/machines/enroll/challenge",
            post(enroll_challenge),
        )
        .route("/api/account/v1/machines/enroll", post(enroll))
        .route(
            "/api/account/v1/machines/{machine_record_id}/grants",
            post(issue_grant),
        );

    // Self-hosted deployments never expose billing, lease or team routes: nothing is registered,
    // so every billing path answers the router default 404.
    if state.deployment_mode.is_billing_enabled() {
        router = router.merge(crate::account::billing::routes::billing_routes(
            crate::account::billing::lemonsqueezy::LemonSqueezyConfig::from_env(),
        ));
    }

    router.layer(DefaultBodyLimit::max(limit)).with_state(state)
}

pub async fn serve(
    listener: tokio::net::TcpListener,
    state: Arc<AccountState>,
) -> Result<(), String> {
    // The grace, suspension and recovery notices are clock-driven and must fire while the
    // entitlement state stays unchanged, so commercial deployments run one background notice
    // sweeper. Self-host never enables billing and must not start it.
    let sweeper = state
        .deployment_mode
        .is_billing_enabled()
        .then(|| crate::account::billing::notices::spawn_billing_notice_sweeper(Arc::clone(&state)));
    let served = axum::serve(listener, router(state))
        .await
        .map_err(|error| format!("account server failed: {error}"));
    if let Some(sweeper) = sweeper {
        sweeper.abort();
    }
    served
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_magic_link(mail_dir: &std::path::Path) -> String {
        let entries: Vec<_> = std::fs::read_dir(mail_dir)
            .expect("mail dir")
            .filter_map(|entry| entry.ok())
            .collect();
        assert_eq!(
            entries.len(),
            1,
            "device request sends exactly one magic link"
        );
        std::fs::read_to_string(entries[0].path()).expect("magic link content")
    }

    fn extract_token(approve_url: &str) -> String {
        assert!(
            approve_url.contains("/api/account/v1/device/approve?code="),
            "magic link must target the device approve endpoint: {approve_url}"
        );
        let token = approve_url
            .split("token=")
            .nth(1)
            .expect("magic link carries the email token")
            .to_string();
        assert!(!token.is_empty(), "email token must not be empty");
        token
    }

    #[tokio::test]
    async fn device_flow_lifecycle_request_poll_approve() {
        let tmp = tempfile::tempdir().unwrap();
        let mail_dir = tmp.path().join("mail");
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            mail_dir.clone(),
        ));
        let state = Arc::new(AccountState::new(tmp.path(), "https://relay.test", mailer));

        let req_body = serde_json::to_vec(&serde_json::json!({
            "email": "headless@test.local"
        }))
        .unwrap();
        let resp = device_request(State(state.clone()), Bytes::from(req_body))
            .await
            .unwrap()
            .0;
        assert_eq!(resp.interval, 2);
        assert!(!resp.user_code.is_empty());
        assert_eq!(resp.verification_uri, "https://relay.test/device");

        let wire = serde_json::to_value(&resp).expect("serialize response");
        assert!(
            wire.get("verificationUriComplete").is_none(),
            "the approval URL must never be returned to the client"
        );

        let approve_url = read_magic_link(&mail_dir);
        assert!(
            approve_url.contains(&format!("code={}", resp.user_code)),
            "magic link must carry the displayed user code"
        );
        let valid_token = extract_token(&approve_url);

        let poll_body = serde_json::to_vec(&serde_json::json!({
            "deviceCode": resp.device_code
        }))
        .unwrap();
        let pending = device_poll(State(state.clone()), Bytes::from(poll_body.clone()))
            .await
            .unwrap()
            .0;
        assert_eq!(pending.status, "authorization_pending");
        assert!(pending.enrollment_code.is_none());

        let rejected = device_approve_get(
            State(state.clone()),
            axum::extract::Query(DeviceApproveQuery {
                code: resp.user_code.clone(),
                token: "wrong_token".into(),
            }),
        )
        .await;
        let error = match rejected {
            Err(error) => error,
            Ok(_) => panic!("approval without the emailed token must fail"),
        };
        assert_eq!(error.status, StatusCode::UNAUTHORIZED);
        assert_eq!(error.code, "EMAIL_TOKEN_INVALID");

        let still_pending = device_poll(State(state.clone()), Bytes::from(poll_body.clone()))
            .await
            .unwrap()
            .0;
        assert_eq!(still_pending.status, "authorization_pending");
        assert!(still_pending.enrollment_code.is_none());

        let approve_res = device_approve_get(
            State(state.clone()),
            axum::extract::Query(DeviceApproveQuery {
                code: resp.user_code.clone(),
                token: valid_token,
            }),
        )
        .await;
        assert!(approve_res.is_ok());

        let approved = device_poll(State(state.clone()), Bytes::from(poll_body))
            .await
            .unwrap()
            .0;
        assert_eq!(approved.status, "approved");
        assert!(approved.enrollment_code.is_some());
    }

    #[tokio::test]
    async fn login_poll_returns_pending_until_the_browser_consumes_the_code() {
        let tmp = tempfile::tempdir().unwrap();
        let mail_dir = tmp.path().join("mail");
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            mail_dir.clone(),
        ));
        let state = Arc::new(AccountState::new(tmp.path(), "https://relay.test", mailer));

        let req_body = serde_json::to_vec(&serde_json::json!({
            "email": "user@test.local"
        }))
        .unwrap();
        let (status, resp) = login_request(State(state.clone()), Bytes::from(req_body))
            .await
            .unwrap();
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(!resp.login_handle.is_empty());

        let mail_url = read_magic_link(&mail_dir);
        assert!(
            mail_url.contains("/login?code="),
            "mailed url must be browser flow /login?code=: {mail_url}"
        );
        assert!(
            !mail_url.contains(&resp.login_handle),
            "mailed url must never contain loginHandle: {mail_url}"
        );
        let code = mail_url
            .split("code=")
            .nth(1)
            .expect("code in mail url")
            .to_string();

        // 1. Poll before browser consumption -> pending, no token
        let poll_body = serde_json::to_vec(&serde_json::json!({
            "loginHandle": resp.login_handle
        }))
        .unwrap();
        let pending = login_poll(State(state.clone()), Bytes::from(poll_body.clone()))
            .await
            .unwrap()
            .0;
        assert_eq!(pending.status, "pending");
        assert!(pending.token.is_none());

        // 2. Browser consumes the code -> returns a token
        let consume_body = serde_json::to_vec(&serde_json::json!({
            "code": code
        }))
        .unwrap();
        let browser_session =
            login_consume(State(state.clone()), Bytes::from(consume_body.clone()))
                .await
                .unwrap()
                .0;
        assert!(!browser_session.token.is_empty());
        assert_eq!(browser_session.email, "user@test.local");

        // 3. Poll after browser consumption -> approved, mints session for app
        let approved = login_poll(State(state.clone()), Bytes::from(poll_body.clone()))
            .await
            .unwrap()
            .0;
        assert_eq!(approved.status, "approved");
        assert!(approved.token.is_some());
        assert_eq!(approved.email.as_deref(), Some("user@test.local"));
        assert_eq!(
            approved.account_id.as_deref(),
            Some(browser_session.account_id.as_str())
        );

        // 4. Poll third time -> fails with 401 LOGIN_HANDLE_INVALID (record was removed)
        let third_poll_err = login_poll(State(state.clone()), Bytes::from(poll_body))
            .await
            .unwrap_err();
        assert_eq!(third_poll_err.status, StatusCode::UNAUTHORIZED);
        assert_eq!(third_poll_err.code, "LOGIN_HANDLE_INVALID");

        // 5. Second consume with same code -> fails with 401 LOGIN_CODE_USED
        let second_consume_err = login_consume(State(state.clone()), Bytes::from(consume_body))
            .await
            .unwrap_err();
        assert_eq!(second_consume_err.status, StatusCode::UNAUTHORIZED);
        assert_eq!(second_consume_err.code, "LOGIN_CODE_USED");
    }

    #[tokio::test]
    async fn device_request_rejects_invalid_email() {
        let tmp = tempfile::tempdir().unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let state = Arc::new(AccountState::new(tmp.path(), "https://relay.test", mailer));

        for email in ["", "not-an-email"] {
            let req_body = serde_json::to_vec(&serde_json::json!({ "email": email })).unwrap();
            let error = device_request(State(state.clone()), Bytes::from(req_body))
                .await
                .expect_err("invalid email must be rejected");
            assert_eq!(error.status, StatusCode::BAD_REQUEST);
            assert_eq!(error.code, "BAD_REQUEST");
            assert_eq!(error.message, "valid email is required");
        }
    }

    #[tokio::test]
    async fn account_signing_key_is_stable_across_restarts() {
        let tmp = tempfile::tempdir().unwrap();
        let mail_dir = tmp.path().join("mail");
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(mail_dir));

        let state1 = Arc::new(AccountState::new(
            tmp.path(),
            "https://account.test",
            mailer.clone(),
        ));
        let pk1 = state1.account_public_key();
        assert!(!pk1.is_empty(), "public key must not be empty");

        let key_file = tmp.path().join("account-signing-key.json");
        assert!(
            key_file.exists(),
            "signing key file must be created on disk"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&key_file).unwrap();
            assert_eq!(
                meta.permissions().mode() & 0o777,
                0o600,
                "signing key file must be mode 0600 on unix"
            );
        }

        let state2 = Arc::new(AccountState::new(
            tmp.path(),
            "https://account.test",
            mailer,
        ));
        let pk2 = state2.account_public_key();
        assert_eq!(
            pk1, pk2,
            "reloading the same directory must yield the same public key"
        );
    }

    #[tokio::test]
    async fn public_key_endpoint_returns_base64_pinned_key() {
        let tmp = tempfile::tempdir().unwrap();
        let mail_dir = tmp.path().join("mail");
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(mail_dir));
        let state = Arc::new(AccountState::new(
            tmp.path(),
            "https://account.test",
            mailer,
        ));
        let expected_key = state.account_public_key();

        let resp = get_public_key(State(state))
            .await
            .expect("valid public key")
            .0;
        assert_eq!(resp.public_key, expected_key);

        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(
            json.get("publicKey").and_then(|v| v.as_str()),
            Some(expected_key.as_str())
        );
    }

    #[tokio::test]
    async fn public_key_endpoint_corrupt_key_returns_500_without_panicking() {
        let tmp = tempfile::tempdir().unwrap();
        let key_file = tmp.path().join("account-signing-key.json");
        std::fs::write(
            &key_file,
            b"{\"publicKey\":\"corrupt\",\"privateKey\":\"invalid\"}",
        )
        .unwrap();

        let mail_dir = tmp.path().join("mail");
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(mail_dir));
        let state = Arc::new(AccountState::new(
            tmp.path(),
            "https://account.test",
            mailer,
        ));

        let err = get_public_key(State(state))
            .await
            .expect_err("corrupt signing key file must return an error response, not panic");
        assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.code, "INTERNAL_ERROR");
        assert!(err.message.contains("signing key unavailable"));
    }

    #[tokio::test]
    async fn health_endpoint_reports_ok_and_unknown_path_is_404() {
        let tmp = tempfile::tempdir().unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let state = Arc::new(AccountState::new(
            tmp.path(),
            "https://account.test",
            mailer,
        ));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let served_state = state.clone();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router(served_state)).await;
        });

        let health = state
            .http_client
            .get(format!("{origin}/api/account/v1/health"))
            .send()
            .await
            .expect("health probe must be served");
        assert_eq!(health.status(), StatusCode::OK);
        let body = health.text().await.expect("health body must be readable");
        assert!(
            body.contains("\"ok\":true"),
            "health body must carry the ok:true marker: {body}"
        );

        // An unknown path under the same prefix must stay a 404, the signal the client keys off.
        let missing = state
            .http_client
            .get(format!("{origin}/api/account/v1/nope"))
            .send()
            .await
            .expect("unknown account path must still answer");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    fn setup_test_user_and_machine(
        state: &AccountState,
        relay_origin: &str,
    ) -> (String, MachineRecord) {
        let now = now_secs();
        let user_id = uuid::Uuid::new_v4().to_string();
        let session_token = random_token();
        let attach_secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
        let attach_public = x25519_dalek::PublicKey::from(&attach_secret);
        let attach_public_key = STANDARD.encode(attach_public.as_bytes());

        let machine = MachineRecord {
            machine_record_id: uuid::Uuid::new_v4().to_string(),
            owner_user_id: user_id.clone(),
            // machines_machine_id_unique is a store-level unique index, so a fixed literal
            // collides as soon as one test mints a second machine into the same store.
            machine_id: format!("test-machine-{user_id}"),
            display_name: "Test Machine".to_string(),
            public_key: STANDARD.encode([1u8; 32]),
            attach_public_key,
            relay_origin: relay_origin.to_string(),
            platform: "macos".to_string(),
            enrollment_epoch: 1,
            enrolled_at: now,
            last_seen_at: now,
        };

        state
            .mutate(|store| {
                store.users.insert(
                    user_id.clone(),
                    UserRecord {
                        user_id: user_id.clone(),
                        // users_email_unique is a store-level unique index; the email must differ
                        // per fixture user even though the conflict target is only user_id.
                        email: format!("tester-{user_id}@example.com"),
                        created_at: now,
                    },
                );
                store.sessions.insert(
                    token_hash(&session_token),
                    SessionRecord {
                        user_id: user_id.clone(),
                        expires_at: now + 3600,
                    },
                );
                store
                    .machines
                    .insert(machine.machine_record_id.clone(), machine.clone());
                Ok(())
            })
            .unwrap();

        (session_token, machine)
    }

    #[tokio::test]
    async fn grant_request_delivers_verifiable_submission_to_relay() {
        use crate::remote::account_grants::verify_grant_signature;

        let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, GrantSubmission)>(1);
        let stub_relay = Router::new().route(
            "/api/v1/attach/grant",
            post(move |body: Bytes| {
                let tx = tx.clone();
                async move {
                    let raw_body = String::from_utf8(body.to_vec()).expect("utf8 json");
                    let sub: GrantSubmission =
                        serde_json::from_str(&raw_body).expect("valid submission json");
                    let _ = tx.send((raw_body, sub)).await;
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "accepted": true,
                            "delivered": { "grantId": "g1", "status": "ready" }
                        })),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay_addr = listener.local_addr().unwrap();
        let relay_origin = format!("http://{relay_addr}");
        tokio::spawn(async move {
            let _ = axum::serve(listener, stub_relay).await;
        });

        let tmp = tempfile::tempdir().unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let state = Arc::new(AccountState::new(
            tmp.path(),
            "https://account.test",
            mailer,
        ));

        let (session_token, machine) = setup_test_user_and_machine(&state, &relay_origin);

        let device_attach_secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
        let device_attach_public = x25519_dalek::PublicKey::from(&device_attach_secret);
        let device_attach_key = STANDARD.encode(device_attach_public.as_bytes());

        let req_body = serde_json::to_vec(&AccountGrantRequest {
            machine_record_id: machine.machine_record_id.clone(),
            enrollment_epoch: "1".into(),
            device_label: "iPhone".into(),
            installation_id: "inst-test".into(),
            grant_scope: AccountGrantScope::Machine,
            attach_public_key: device_attach_key,
        })
        .unwrap();

        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {session_token}").parse().unwrap(),
        );

        let response = issue_grant(
            State(state.clone()),
            axum::extract::Path(machine.machine_record_id.clone()),
            headers,
            Bytes::from(req_body),
        )
        .await
        .expect("grant issuance and delivery must succeed");

        assert_eq!(response.0.machine_id, machine.machine_id);
        assert!(response.0.sealed_offer.is_some());

        let (raw_body, submission) = rx.recv().await.expect("relay must receive submission");
        assert_eq!(submission.machine_id, machine.machine_id);
        assert_eq!(submission.enrollment_epoch, "1");

        // Verify that the relay NEVER sees the pairing token or plaintext grant offer JSON
        let pairing_token = &response.0.pairing_token;
        assert!(
            !raw_body.contains(pairing_token),
            "relay submission body must never contain the plaintext pairing token"
        );
        let sealed_plaintext_json_fragment = format!("\"grantId\":\"{}\"", response.0.grant_id);
        assert!(
            !raw_body.contains(&sealed_plaintext_json_fragment),
            "relay submission body must not contain plaintext offer json fields"
        );
        assert!(
            !raw_body.contains("\"grantScope\""),
            "relay submission body must carry only ciphertext envelope, never unencrypted offer json"
        );

        let sealed_bytes = STANDARD
            .decode(&submission.envelope.sealed)
            .expect("valid base64 sealed offer");
        assert!(
            !String::from_utf8_lossy(&sealed_bytes).contains(pairing_token),
            "sealed envelope bytes must be encrypted ciphertext, never plaintext"
        );

        let account_pk = Some(state.account_public_key());
        assert_eq!(
            verify_grant_signature(&account_pk, &submission),
            Ok(()),
            "submission signature must verify against the account public key"
        );

        let foreign = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
        let foreign_key = Some(STANDARD.encode(foreign.verifying_key().as_bytes()));
        assert_eq!(
            verify_grant_signature(&foreign_key, &submission),
            Err(StatusCode::FORBIDDEN),
            "submission signature must fail against a valid but foreign key"
        );

        // Operator pinned an invalid Ed25519 point: distinct from signature mismatch
        let malformed_key = Some(STANDARD.encode([8u8; 32]));
        assert_eq!(
            verify_grant_signature(&malformed_key, &submission),
            Err(StatusCode::INTERNAL_SERVER_ERROR),
            "malformed pinned key must yield 500 internal server error"
        );

        assert_eq!(
            verify_grant_signature(&None, &submission),
            Err(StatusCode::FORBIDDEN),
            "submission signature must fail with no pinned key"
        );

        let grant_count = state.read(|s| Ok(s.grants.len())).unwrap();
        assert_eq!(grant_count, 1, "delivered grant must be recorded in store");
    }

    #[tokio::test]
    async fn delivery_failure_relay_refuses_or_unreachable_yields_502() {
        // Case A: Relay returns 403 Forbidden
        let stub_relay_403 = Router::new().route(
            "/api/v1/attach/grant",
            post(|| async { (StatusCode::FORBIDDEN, "unpinned relay refuses grant") }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay_origin_403 = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let _ = axum::serve(listener, stub_relay_403).await;
        });

        let tmp = tempfile::tempdir().unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let state = Arc::new(AccountState::new(
            tmp.path(),
            "https://account.test",
            mailer,
        ));

        let (session_token, machine) = setup_test_user_and_machine(&state, &relay_origin_403);
        let device_attach_key = STANDARD.encode([3u8; 32]);
        let req_body = serde_json::to_vec(&AccountGrantRequest {
            machine_record_id: machine.machine_record_id.clone(),
            enrollment_epoch: "1".into(),
            device_label: "iPhone".into(),
            installation_id: "inst-test".into(),
            grant_scope: AccountGrantScope::Machine,
            attach_public_key: device_attach_key.clone(),
        })
        .unwrap();

        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {session_token}").parse().unwrap(),
        );

        let err_403 = issue_grant(
            State(state.clone()),
            axum::extract::Path(machine.machine_record_id.clone()),
            headers.clone(),
            Bytes::from(req_body.clone()),
        )
        .await
        .expect_err("relay 403 must fail grant issuance");

        assert_eq!(err_403.status, StatusCode::BAD_GATEWAY);
        assert_eq!(err_403.code, "GRANT_DELIVERY_FAILED");
        assert!(err_403.message.contains("403"));

        let grants_after_403 = state.read(|s| Ok(s.grants.len())).unwrap();
        assert_eq!(
            grants_after_403, 0,
            "refused delivery must not leave orphan grant in store"
        );

        // Case B: Nothing is listening (closed port)
        let (session_token2, machine_dead) =
            setup_test_user_and_machine(&state, "http://127.0.0.1:1");
        let mut headers2 = HeaderMap::new();
        headers2.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {session_token2}").parse().unwrap(),
        );
        let req_body2 = serde_json::to_vec(&AccountGrantRequest {
            machine_record_id: machine_dead.machine_record_id.clone(),
            enrollment_epoch: "1".into(),
            device_label: "iPhone".into(),
            installation_id: "inst-test-2".into(),
            grant_scope: AccountGrantScope::Machine,
            attach_public_key: device_attach_key.clone(),
        })
        .unwrap();

        let err_unreachable = issue_grant(
            State(state.clone()),
            axum::extract::Path(machine_dead.machine_record_id.clone()),
            headers2,
            Bytes::from(req_body2),
        )
        .await
        .expect_err("unreachable relay must fail grant issuance");

        assert_eq!(err_unreachable.status, StatusCode::BAD_GATEWAY);
        assert_eq!(err_unreachable.code, "GRANT_DELIVERY_FAILED");

        let grants_after_unreachable = state.read(|s| Ok(s.grants.len())).unwrap();
        assert_eq!(
            grants_after_unreachable, 0,
            "unreachable delivery must not leave orphan grant in store"
        );

        // Case C: Machine refused grant offer (status != "ready", e.g. ACCOUNT_OFFER_UNSEAL_FAILED)
        let stub_relay_refusal = Router::new().route(
            "/api/v1/attach/grant",
            post(|| async {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "accepted": true,
                        "delivered": {
                            "grantId": "g1",
                            "status": "ACCOUNT_OFFER_UNSEAL_FAILED"
                        }
                    })),
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay_origin_refusal = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let _ = axum::serve(listener, stub_relay_refusal).await;
        });

        let (session_token3, machine_refused) =
            setup_test_user_and_machine(&state, &relay_origin_refusal);
        let mut headers3 = HeaderMap::new();
        headers3.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {session_token3}").parse().unwrap(),
        );
        let req_body3 = serde_json::to_vec(&AccountGrantRequest {
            machine_record_id: machine_refused.machine_record_id.clone(),
            enrollment_epoch: "1".into(),
            device_label: "iPhone".into(),
            installation_id: "inst-test-3".into(),
            grant_scope: AccountGrantScope::Machine,
            attach_public_key: device_attach_key.clone(),
        })
        .unwrap();

        let err_refusal = issue_grant(
            State(state.clone()),
            axum::extract::Path(machine_refused.machine_record_id.clone()),
            headers3,
            Bytes::from(req_body3),
        )
        .await
        .expect_err("machine refusal status must fail grant issuance");

        assert_eq!(err_refusal.status, StatusCode::BAD_GATEWAY);
        assert_eq!(err_refusal.code, "GRANT_DELIVERY_FAILED");
        assert!(
            err_refusal.message.contains("ACCOUNT_OFFER_UNSEAL_FAILED"),
            "refusal message must include status verbatim: {}",
            err_refusal.message
        );

        let grants_after_refusal = state.read(|s| Ok(s.grants.len())).unwrap();
        assert_eq!(
            grants_after_refusal, 0,
            "refused delivery must not leave orphan grant in store"
        );

        // Case D: Relay returns 200 with non-JSON body (e.g. captive portal or HTML)
        let stub_relay_html = Router::new().route(
            "/api/v1/attach/grant",
            post(|| async { (StatusCode::OK, "<html><body>Captive Portal</body></html>") }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay_origin_html = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let _ = axum::serve(listener, stub_relay_html).await;
        });

        let (session_token4, machine_html) =
            setup_test_user_and_machine(&state, &relay_origin_html);
        let mut headers4 = HeaderMap::new();
        headers4.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {session_token4}").parse().unwrap(),
        );
        let req_body4 = serde_json::to_vec(&AccountGrantRequest {
            machine_record_id: machine_html.machine_record_id.clone(),
            enrollment_epoch: "1".into(),
            device_label: "iPhone".into(),
            installation_id: "inst-test-4".into(),
            grant_scope: AccountGrantScope::Machine,
            attach_public_key: device_attach_key.clone(),
        })
        .unwrap();

        let err_html = issue_grant(
            State(state.clone()),
            axum::extract::Path(machine_html.machine_record_id.clone()),
            headers4,
            Bytes::from(req_body4),
        )
        .await
        .expect_err("non-JSON 200 body must fail grant issuance");

        assert_eq!(err_html.status, StatusCode::BAD_GATEWAY);
        assert_eq!(err_html.code, "GRANT_DELIVERY_FAILED");

        let grants_after_html = state.read(|s| Ok(s.grants.len())).unwrap();
        assert_eq!(
            grants_after_html, 0,
            "non-JSON response must not leave orphan grant in store"
        );

        // Case E: Relay returns 200 with JSON but delivered is absent
        let stub_relay_no_delivered = Router::new().route(
            "/api/v1/attach/grant",
            post(|| async {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "accepted": true
                    })),
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay_origin_no_delivered = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let _ = axum::serve(listener, stub_relay_no_delivered).await;
        });

        let (session_token5, machine_no_delivered) =
            setup_test_user_and_machine(&state, &relay_origin_no_delivered);
        let mut headers5 = HeaderMap::new();
        headers5.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {session_token5}").parse().unwrap(),
        );
        let req_body5 = serde_json::to_vec(&AccountGrantRequest {
            machine_record_id: machine_no_delivered.machine_record_id.clone(),
            enrollment_epoch: "1".into(),
            device_label: "iPhone".into(),
            installation_id: "inst-test-5".into(),
            grant_scope: AccountGrantScope::Machine,
            attach_public_key: device_attach_key,
        })
        .unwrap();

        let err_no_delivered = issue_grant(
            State(state.clone()),
            axum::extract::Path(machine_no_delivered.machine_record_id.clone()),
            headers5,
            Bytes::from(req_body5),
        )
        .await
        .expect_err("absent delivered field must fail grant issuance");

        assert_eq!(err_no_delivered.status, StatusCode::BAD_GATEWAY);
        assert_eq!(err_no_delivered.code, "GRANT_DELIVERY_FAILED");

        let grants_after_no_delivered = state.read(|s| Ok(s.grants.len())).unwrap();
        assert_eq!(
            grants_after_no_delivered, 0,
            "absent delivered response must not leave orphan grant in store"
        );
    }

    #[tokio::test]
    async fn machine_online_status_freshness_and_liveness_probe() {
        let tmp = tempfile::tempdir().unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let state = Arc::new(AccountState::new(
            tmp.path(),
            "https://account.test",
            mailer,
        ));
        let now = now_secs();

        // 1. Fresh machine (last_seen_at = now) -> online: true
        let (session_token, fresh_machine) =
            setup_test_user_and_machine(&state, "https://relay.test");
        let view = state.machine_view(&fresh_machine, now);
        assert!(
            view.online,
            "a machine seen within the freshness window must report online: true"
        );

        // From<&MachineRecord> fallback also reports true for fresh machine
        let from_view = MachineViewResponse::from(&fresh_machine);
        assert!(
            from_view.online,
            "From conversion must also report online: true for fresh machine"
        );

        // 2. Stale machine (last_seen_at = now - 600s, beyond 300s window) -> online: false
        let mut stale_machine = fresh_machine.clone();
        stale_machine.last_seen_at = now.saturating_sub(MACHINE_ONLINE_FRESHNESS_WINDOW_SECS + 300);
        let stale_view = state.machine_view(&stale_machine, now);
        assert!(
            !stale_view.online,
            "a stale machine must report online: false"
        );

        // 3. Machine never seen (last_seen_at = 0) -> online: false
        let mut unseen_machine = fresh_machine.clone();
        unseen_machine.last_seen_at = 0;
        let unseen_view = state.machine_view(&unseen_machine, now);
        assert!(
            !unseen_view.online,
            "a machine with last_seen_at == 0 must report online: false"
        );

        // 4. Stale machine with live control channel probe -> online: true
        let live_machine_id = stale_machine.machine_id.clone();
        state.set_liveness_probe(Some(Arc::new(move |id| id == live_machine_id)));
        let live_view = state.machine_view(&stale_machine, now);
        assert!(
            live_view.online,
            "a machine with a live control channel probe must report online: true even if last_seen_at is stale"
        );

        // 5. Test through HTTP GET /api/account/v1/machines endpoint
        state.set_liveness_probe(None);
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {session_token}").parse().unwrap(),
        );

        let machines_response = list_machines(State(state.clone()), headers)
            .await
            .expect("list_machines succeeds")
            .0;
        assert_eq!(machines_response.len(), 1);
        assert!(
            machines_response[0].online,
            "list_machines endpoint must report newly enrolled machine online: true"
        );

        // Update the machine in store to be stale
        state
            .mutate(|store| {
                if let Some(m) = store.machines.get_mut(&fresh_machine.machine_record_id) {
                    m.last_seen_at = now.saturating_sub(MACHINE_ONLINE_FRESHNESS_WINDOW_SECS + 100);
                }
                Ok(())
            })
            .unwrap();

        let mut headers2 = HeaderMap::new();
        headers2.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {session_token}").parse().unwrap(),
        );
        let stale_response = list_machines(State(state.clone()), headers2)
            .await
            .expect("list_machines succeeds")
            .0;
        assert_eq!(stale_response.len(), 1);
        assert!(
            !stale_response[0].online,
            "list_machines endpoint must report stale machine online: false"
        );
    }

    #[tokio::test]
    async fn mutate_rolls_back_on_error_and_commits_on_success() {
        let tmp = tempfile::tempdir().unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let state = Arc::new(AccountState::new(
            tmp.path(),
            "https://account.test",
            mailer,
        ));

        // 1. Initial successful mutation
        state
            .mutate(|store| {
                store.users.insert(
                    "user_initial".into(),
                    UserRecord {
                        user_id: "user_initial".into(),
                        email: "initial@example.com".into(),
                        created_at: 1000,
                    },
                );
                Ok(())
            })
            .expect("initial mutation must commit");

        let initial_store = state.load().expect("load store");
        assert_eq!(initial_store.users.len(), 1);
        assert!(initial_store.users.contains_key("user_initial"));

        // 2. Failing mutation rolls back: closure modifies store in-memory then returns Err
        let mutate_err = state.mutate(|store| -> Result<(), ApiError> {
            store.users.insert(
                "user_transient".into(),
                UserRecord {
                    user_id: "user_transient".into(),
                    email: "transient@example.com".into(),
                    created_at: 2000,
                },
            );
            Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INTENTIONAL_ERROR",
                "simulated failure before commit",
            ))
        });
        assert!(
            mutate_err.is_err(),
            "mutating closure returning Err must yield error"
        );
        let err = mutate_err.unwrap_err();
        assert_eq!(err.code, "INTENTIONAL_ERROR");
        assert_eq!(err.status, StatusCode::BAD_REQUEST);

        // Verify rollback: user_transient was never committed to sqlite
        let store_after_abort = state.load().expect("load store after abort");
        assert_eq!(store_after_abort.users.len(), 1);
        assert!(!store_after_abort.users.contains_key("user_transient"));
        assert!(store_after_abort.users.contains_key("user_initial"));

        // 3. Second successful mutation commits cleanly
        state
            .mutate(|store| {
                store.users.insert(
                    "user_second".into(),
                    UserRecord {
                        user_id: "user_second".into(),
                        email: "second@example.com".into(),
                        created_at: 3000,
                    },
                );
                Ok(())
            })
            .expect("second mutation must commit");

        let store_after_second = state.load().expect("load store after second");
        assert_eq!(store_after_second.users.len(), 2);
        assert!(store_after_second.users.contains_key("user_initial"));
        assert!(store_after_second.users.contains_key("user_second"));
    }

    fn test_identity(machine_id: &str) -> crate::remote::auth::MachineIdentity {
        let key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
        crate::remote::auth::MachineIdentity {
            machine_id: machine_id.into(),
            display_name: format!("{machine_id}-box"),
            public_key: STANDARD.encode(key.verifying_key().as_bytes()),
            private_key: STANDARD.encode(key.to_bytes()),
        }
    }

    fn test_enroll_request(
        state: &AccountState,
        identity: &crate::remote::auth::MachineIdentity,
        code: &str,
    ) -> AccountEnrollRequest {
        let challenge = state.open_challenge(&identity.machine_id);
        let now = now_secs();
        let code_hash = crate::remote::auth::enrollment_code_hash(code);
        let signature = crate::remote::auth::sign_account_enrollment(
            identity,
            &state.origin,
            &code_hash,
            &challenge.nonce,
            now,
        )
        .expect("sign enrollment");
        AccountEnrollRequest {
            api_version: 1,
            enrollment_code: code.to_string(),
            machine_id: identity.machine_id.clone(),
            display_name: identity.display_name.clone(),
            public_key: identity.public_key.clone(),
            attach_public_key: identity.public_key.clone(),
            platform: crate::remote::machine_protocol::Platform::Linux,
            app_version: "2026.930.1".into(),
            nonce: challenge.nonce,
            timestamp: now,
            signature,
        }
    }

    #[tokio::test]
    async fn free_second_machine_enroll_is_402() {
        let tmp = tempfile::tempdir().unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let state = Arc::new(
            AccountState::new(tmp.path(), "https://relay.test", mailer)
                .with_deployment_mode(DeploymentMode::Commercial),
        );

        let now = now_secs();
        let user_id = "test-user-free";
        let code1 = "free-code-1";
        let code2 = "free-code-2";

        state
            .mutate(|store| {
                store.users.insert(
                    user_id.into(),
                    UserRecord {
                        user_id: user_id.into(),
                        email: "free@example.com".into(),
                        created_at: now,
                    },
                );
                store.enrollment_codes.insert(
                    crate::remote::auth::enrollment_code_hash(code1),
                    EnrollmentCodeRecord {
                        user_id: user_id.into(),
                        account_origin: "https://relay.test".into(),
                        expires_at: now + 3600,
                    },
                );
                store.enrollment_codes.insert(
                    crate::remote::auth::enrollment_code_hash(code2),
                    EnrollmentCodeRecord {
                        user_id: user_id.into(),
                        account_origin: "https://relay.test".into(),
                        expires_at: now + 3600,
                    },
                );
                Ok(())
            })
            .unwrap();

        let id1 = test_identity("free-machine-1");
        let req1 = test_enroll_request(&state, &id1, code1);
        let resp1 = enroll(
            State(state.clone()),
            Bytes::from(serde_json::to_vec(&req1).unwrap()),
        )
        .await
        .expect("first free machine enrollment must succeed");
        assert_eq!(resp1.0.account_id, user_id);

        let id2 = test_identity("free-machine-2");
        let req2 = test_enroll_request(&state, &id2, code2);
        let err2 = enroll(
            State(state.clone()),
            Bytes::from(serde_json::to_vec(&req2).unwrap()),
        )
        .await
        .expect_err("second free machine enrollment must fail with 402");
        assert_eq!(err2.status, StatusCode::PAYMENT_REQUIRED);
        assert_eq!(err2.code, "PLAN_LIMIT_REACHED");
        let details = err2.details.expect("PLAN_LIMIT_REACHED carries details");
        assert_eq!(details["plan"], "free");
        assert_eq!(details["limit"], 1);
        assert_eq!(details["used"], 1);
    }

    #[tokio::test]
    async fn reenroll_is_not_counted() {
        let tmp = tempfile::tempdir().unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let state = Arc::new(
            AccountState::new(tmp.path(), "https://relay.test", mailer)
                .with_deployment_mode(DeploymentMode::Commercial),
        );

        let now = now_secs();
        let user_id = "test-user-reenroll";
        let code1 = "reenroll-code-1";
        let code2 = "reenroll-code-2";

        state
            .mutate(|store| {
                store.users.insert(
                    user_id.into(),
                    UserRecord {
                        user_id: user_id.into(),
                        email: "reenroll@example.com".into(),
                        created_at: now,
                    },
                );
                store.enrollment_codes.insert(
                    crate::remote::auth::enrollment_code_hash(code1),
                    EnrollmentCodeRecord {
                        user_id: user_id.into(),
                        account_origin: "https://relay.test".into(),
                        expires_at: now + 3600,
                    },
                );
                store.enrollment_codes.insert(
                    crate::remote::auth::enrollment_code_hash(code2),
                    EnrollmentCodeRecord {
                        user_id: user_id.into(),
                        account_origin: "https://relay.test".into(),
                        expires_at: now + 3600,
                    },
                );
                Ok(())
            })
            .unwrap();

        let id1 = test_identity("reenroll-machine");
        let req1 = test_enroll_request(&state, &id1, code1);
        let resp1 = enroll(
            State(state.clone()),
            Bytes::from(serde_json::to_vec(&req1).unwrap()),
        )
        .await
        .expect("initial enrollment must succeed");
        assert_eq!(resp1.0.enrollment_epoch, "1");
        let machine_rec_id = resp1.0.machine_record_id.clone();

        // Re-enrolling the same machine id: exempt from machine limit
        let req2 = test_enroll_request(&state, &id1, code2);
        let resp2 = enroll(
            State(state.clone()),
            Bytes::from(serde_json::to_vec(&req2).unwrap()),
        )
        .await
        .expect("re-enrollment must succeed and not be blocked by capacity");
        assert_eq!(resp2.0.enrollment_epoch, "2");
        assert_eq!(
            resp2.0.machine_record_id, machine_rec_id,
            "machine_record_id must be preserved across re-enrollment"
        );

        let store_after = state.load().expect("load store after reenroll");
        assert_eq!(
            store_after.machines.len(),
            1,
            "stored machines count must remain exactly 1 after reenrollment"
        );
        let stored_machine = store_after
            .machines
            .values()
            .find(|m| m.machine_id == id1.machine_id)
            .expect("machine must exist in store");
        assert_eq!(stored_machine.machine_record_id, machine_rec_id);
        assert_eq!(stored_machine.enrollment_epoch, 2);
    }

    #[tokio::test]
    async fn stopped_owner_reenroll_is_refused_and_preserves_epoch() {
        let tmp = tempfile::tempdir().unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let state = Arc::new(
            AccountState::new(tmp.path(), "https://relay.test", mailer)
                .with_deployment_mode(DeploymentMode::Commercial),
        );

        let now = now_secs();
        let billing_now = crate::account::billing::routes::billing_now();
        let user_id = "test-user-stopped-reenroll";
        let code1 = "stopped-reenroll-code-1";
        let code2 = "stopped-reenroll-code-2";

        state
            .mutate(|store| {
                store.users.insert(
                    user_id.into(),
                    UserRecord {
                        user_id: user_id.into(),
                        email: "stopped-reenroll@example.com".into(),
                        created_at: now,
                    },
                );
                store.enrollment_codes.insert(
                    crate::remote::auth::enrollment_code_hash(code1),
                    EnrollmentCodeRecord {
                        user_id: user_id.into(),
                        account_origin: "https://relay.test".into(),
                        expires_at: now + 3600,
                    },
                );
                store.enrollment_codes.insert(
                    crate::remote::auth::enrollment_code_hash(code2),
                    EnrollmentCodeRecord {
                        user_id: user_id.into(),
                        account_origin: "https://relay.test".into(),
                        expires_at: now + 3600,
                    },
                );
                Ok(())
            })
            .unwrap();

        let id1 = test_identity("stopped-reenroll-machine");
        let req1 = test_enroll_request(&state, &id1, code1);
        let resp1 = enroll(
            State(state.clone()),
            Bytes::from(serde_json::to_vec(&req1).unwrap()),
        )
        .await
        .expect("initial enrollment must succeed");
        assert_eq!(resp1.0.enrollment_epoch, "1");
        let machine_rec_id = resp1.0.machine_record_id.clone();

        // Populate a second machine owned by the user so machines_used (2) > limit (1) creates an
        // active over_limit violation; with grace_started_at > GRACE_SECS in the past, evaluate_owner
        // legitimately computes EntitlementStatus::Stopped.
        state
            .mutate(|store| {
                store.machines.insert(
                    "rec-second-reenroll".into(),
                    MachineRecord {
                        machine_record_id: "rec-second-reenroll".into(),
                        owner_user_id: user_id.into(),
                        machine_id: "second-machine-reenroll".into(),
                        display_name: "Second Machine".into(),
                        public_key: "pubkey-second-reenroll".into(),
                        attach_public_key: "attachkey-second-reenroll".into(),
                        relay_origin: "https://relay.test".into(),
                        platform: "linux".into(),
                        enrollment_epoch: 1,
                        enrolled_at: now,
                        last_seen_at: now,
                    },
                );
                store.billing_states.insert(
                    user_id.into(),
                    crate::account::store::BillingStateRecord {
                        owner_key: user_id.into(),
                        grace_started_at: Some(
                            billing_now
                                .saturating_sub(crate::account::billing::entitlement::GRACE_SECS + 1000),
                        ),
                        stopped_at: Some(billing_now.saturating_sub(100)),
                        last_notice: None,
                    },
                );
                Ok(())
            })
            .unwrap();

        // Assert computed status before reenroll is legitimately Stopped
        let eval_before =
            crate::account::billing::routes::entitlement_for_user(&state, user_id, billing_now)
                .expect("entitlement evaluation");
        assert_eq!(
            eval_before.status,
            crate::account::billing::entitlement::EntitlementStatus::Stopped,
            "computed status before reenroll must be Stopped"
        );
        assert!(
            !eval_before.remote_allowed,
            "remote access must not be allowed when stopped"
        );

        // Re-enrolling while stopped must be refused with 402 REMOTE_SUSPENDED
        let req2 = test_enroll_request(&state, &id1, code2);
        let err2 = enroll(
            State(state.clone()),
            Bytes::from(serde_json::to_vec(&req2).unwrap()),
        )
        .await
        .expect_err("re-enrollment while stopped must fail with 402 REMOTE_SUSPENDED");
        assert_eq!(err2.status, StatusCode::PAYMENT_REQUIRED);
        assert_eq!(err2.code, "REMOTE_SUSPENDED");

        let store_after = state.load().expect("load store after stopped reenroll");
        let stored_machine = store_after
            .machines
            .values()
            .find(|m| m.machine_id == id1.machine_id)
            .expect("machine must exist in store");
        assert_eq!(
            stored_machine.enrollment_epoch, 1,
            "enrollment epoch must NOT advance on stopped refusal"
        );
        assert_eq!(
            stored_machine.machine_record_id, machine_rec_id,
            "machine_record_id must be preserved"
        );
    }

    #[tokio::test]
    async fn stopped_owner_grant_is_402() {
        let tmp = tempfile::tempdir().unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let state = Arc::new(
            AccountState::new(tmp.path(), "https://relay.test", mailer)
                .with_deployment_mode(DeploymentMode::Commercial),
        );

        let now = now_secs();
        let billing_now = crate::account::billing::routes::billing_now();
        let user_id = "test-user-stopped";
        let session_token = "valid-session-token-stopped";
        let id1 = test_identity("stopped-machine");
        let id2 = test_identity("stopped-machine-2");
        let device_id = test_identity("stopped-device");

        state
            .mutate(|store| {
                store.users.insert(
                    user_id.into(),
                    UserRecord {
                        user_id: user_id.into(),
                        email: "stopped@example.com".into(),
                        created_at: now,
                    },
                );
                store.sessions.insert(
                    token_hash(session_token),
                    SessionRecord {
                        user_id: user_id.into(),
                        expires_at: now + 3600,
                    },
                );
                store.machines.insert(
                    "rec-stopped".into(),
                    MachineRecord {
                        machine_record_id: "rec-stopped".into(),
                        owner_user_id: user_id.into(),
                        machine_id: id1.machine_id.clone(),
                        display_name: id1.display_name.clone(),
                        public_key: id1.public_key.clone(),
                        attach_public_key: id1.public_key.clone(),
                        relay_origin: "https://relay.test".into(),
                        platform: "linux".into(),
                        enrollment_epoch: 1,
                        enrolled_at: now,
                        last_seen_at: now,
                    },
                );
                // Second machine owned by user creates an active over_limit violation (2 > 1 limit on Free)
                store.machines.insert(
                    "rec-stopped-2".into(),
                    MachineRecord {
                        machine_record_id: "rec-stopped-2".into(),
                        owner_user_id: user_id.into(),
                        machine_id: id2.machine_id.clone(),
                        display_name: id2.display_name.clone(),
                        public_key: id2.public_key.clone(),
                        attach_public_key: id2.public_key.clone(),
                        relay_origin: "https://relay.test".into(),
                        platform: "linux".into(),
                        enrollment_epoch: 1,
                        enrolled_at: now,
                        last_seen_at: now,
                    },
                );
                store.billing_states.insert(
                    user_id.into(),
                    crate::account::store::BillingStateRecord {
                        owner_key: user_id.into(),
                        grace_started_at: Some(
                            billing_now
                                .saturating_sub(crate::account::billing::entitlement::GRACE_SECS + 1000),
                        ),
                        stopped_at: Some(billing_now.saturating_sub(100)),
                        last_notice: None,
                    },
                );
                Ok(())
            })
            .unwrap();

        // Assert computed status before grant call is legitimately Stopped
        let eval_before =
            crate::account::billing::routes::entitlement_for_user(&state, user_id, billing_now)
                .expect("entitlement evaluation");
        assert_eq!(
            eval_before.status,
            crate::account::billing::entitlement::EntitlementStatus::Stopped,
            "computed status before grant issuance must be Stopped"
        );
        assert!(
            !eval_before.remote_allowed,
            "remote access must not be allowed when stopped"
        );

        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {session_token}").parse().unwrap(),
        );

        let grant_req = AccountGrantRequest {
            machine_record_id: "rec-stopped".into(),
            enrollment_epoch: "1".into(),
            device_label: "test-device".into(),
            installation_id: "inst-1".into(),
            grant_scope: AccountGrantScope::Machine,
            attach_public_key: device_id.public_key.clone(),
        };

        let err = issue_grant(
            State(state.clone()),
            axum::extract::Path("rec-stopped".into()),
            headers,
            Bytes::from(serde_json::to_vec(&grant_req).unwrap()),
        )
        .await
        .expect_err("issue_grant on stopped owner must fail with 402");

        assert_eq!(err.status, StatusCode::PAYMENT_REQUIRED);
        assert_eq!(err.code, "REMOTE_SUSPENDED");
        let details = err.details.expect("REMOTE_SUSPENDED carries details");
        assert_eq!(details["status"], "stopped");
        assert_eq!(details["plan"], "free");
        let expected_stopped_at = state
            .load()
            .expect("load store")
            .billing_states
            .get(user_id)
            .and_then(|b| b.stopped_at)
            .expect("persisted billing state must have stopped_at");
        assert_eq!(
            details["stoppedAt"],
            expected_stopped_at
        );
    }

    #[tokio::test]
    async fn selfhost_never_checks() {
        let tmp = tempfile::tempdir().unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let state = Arc::new(
            AccountState::new(tmp.path(), "https://relay.test", mailer)
                .with_deployment_mode(DeploymentMode::SelfHost),
        );

        let now = now_secs();
        let user_id = "test-user-selfhost";
        let code1 = "selfhost-code-1";
        let code2 = "selfhost-code-2";

        state
            .mutate(|store| {
                store.users.insert(
                    user_id.into(),
                    UserRecord {
                        user_id: user_id.into(),
                        email: "selfhost@example.com".into(),
                        created_at: now,
                    },
                );
                store.enrollment_codes.insert(
                    crate::remote::auth::enrollment_code_hash(code1),
                    EnrollmentCodeRecord {
                        user_id: user_id.into(),
                        account_origin: "https://relay.test".into(),
                        expires_at: now + 3600,
                    },
                );
                store.enrollment_codes.insert(
                    crate::remote::auth::enrollment_code_hash(code2),
                    EnrollmentCodeRecord {
                        user_id: user_id.into(),
                        account_origin: "https://relay.test".into(),
                        expires_at: now + 3600,
                    },
                );
                Ok(())
            })
            .unwrap();

        let id1 = test_identity("selfhost-machine-1");
        let req1 = test_enroll_request(&state, &id1, code1);
        let resp1 = enroll(
            State(state.clone()),
            Bytes::from(serde_json::to_vec(&req1).unwrap()),
        )
        .await
        .expect("first machine enrollment must succeed in SelfHost mode");
        assert_eq!(resp1.0.account_id, user_id);

        let id2 = test_identity("selfhost-machine-2");
        let req2 = test_enroll_request(&state, &id2, code2);
        let resp2 = enroll(
            State(state.clone()),
            Bytes::from(serde_json::to_vec(&req2).unwrap()),
        )
        .await
        .expect("second machine enrollment must succeed in SelfHost mode without limit");
        assert_eq!(resp2.0.account_id, user_id);
    }

    #[tokio::test]
    async fn concurrent_free_enroll_atomic_transaction_race() {
        let tmp = tempfile::tempdir().unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let state = Arc::new(
            AccountState::new(tmp.path(), "https://relay.test", mailer)
                .with_deployment_mode(DeploymentMode::Commercial),
        );

        let now = now_secs();
        let user_id = "test-user-race";
        let code1 = "race-code-1";
        let code2 = "race-code-2";

        state
            .mutate(|store| {
                store.users.insert(
                    user_id.into(),
                    UserRecord {
                        user_id: user_id.into(),
                        email: "race@example.com".into(),
                        created_at: now,
                    },
                );
                store.enrollment_codes.insert(
                    crate::remote::auth::enrollment_code_hash(code1),
                    EnrollmentCodeRecord {
                        user_id: user_id.into(),
                        account_origin: "https://relay.test".into(),
                        expires_at: now + 3600,
                    },
                );
                store.enrollment_codes.insert(
                    crate::remote::auth::enrollment_code_hash(code2),
                    EnrollmentCodeRecord {
                        user_id: user_id.into(),
                        account_origin: "https://relay.test".into(),
                        expires_at: now + 3600,
                    },
                );
                Ok(())
            })
            .unwrap();

        let id1 = test_identity("race-machine-1");
        let req1 = test_enroll_request(&state, &id1, code1);
        let id2 = test_identity("race-machine-2");
        let req2 = test_enroll_request(&state, &id2, code2);

        let barrier = Arc::new(tokio::sync::Barrier::new(2));

        let state1 = state.clone();
        let barrier1 = barrier.clone();
        let mut task1 = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(5), barrier1.wait())
                .await
                .expect("barrier1 wait must not time out");
            enroll(
                State(state1),
                Bytes::from(serde_json::to_vec(&req1).unwrap()),
            )
            .await
        });

        let state2 = state.clone();
        let barrier2 = barrier.clone();
        let mut task2 = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(5), barrier2.wait())
                .await
                .expect("barrier2 wait must not time out");
            enroll(
                State(state2),
                Bytes::from(serde_json::to_vec(&req2).unwrap()),
            )
            .await
        });

        let joined = tokio::time::timeout(
            Duration::from_secs(10),
            async { tokio::join!(&mut task1, &mut task2) },
        )
        .await;

        let (res1, res2) = match joined {
            Ok((r1, r2)) => (
                r1.expect("task1 panicked"),
                r2.expect("task2 panicked"),
            ),
            Err(_) => {
                task1.abort();
                task2.abort();
                panic!("concurrent enrollment tasks timed out after 10s");
            }
        };

        let ok_count = (res1.is_ok() as usize) + (res2.is_ok() as usize);
        let limit_err_count = (matches!(&res1, Err(e) if e.status == StatusCode::PAYMENT_REQUIRED && e.code == "PLAN_LIMIT_REACHED") as usize)
            + (matches!(&res2, Err(e) if e.status == StatusCode::PAYMENT_REQUIRED && e.code == "PLAN_LIMIT_REACHED") as usize);

        assert_eq!(
            ok_count, 1,
            "exactly one concurrent enrollment on Free tier must succeed"
        );
        assert_eq!(
            limit_err_count, 1,
            "the concurrent sibling must be rejected with 402 PLAN_LIMIT_REACHED"
        );

        let err = if res1.is_err() {
            res1.unwrap_err()
        } else {
            res2.unwrap_err()
        };
        assert_eq!(err.status, StatusCode::PAYMENT_REQUIRED);
        assert_eq!(err.code, "PLAN_LIMIT_REACHED");
        let details = err.details.expect("PLAN_LIMIT_REACHED carries details");
        assert_eq!(details["plan"], "free");
        assert_eq!(details["limit"], 1);
        assert_eq!(details["used"], 1);

        let store_after = state.load().expect("load store after race");
        assert_eq!(
            store_after.machines.len(),
            1,
            "final stored machines must be exactly 1"
        );
        assert_eq!(
            store_after.enrollment_codes.len(),
            1,
            "unconsumed rejected enrollment code must remain in store"
        );
        assert_eq!(
            store_after.grants.len(),
            0,
            "grants state must remain unchanged"
        );
    }

    #[tokio::test]
    async fn team_member_pool_aggregate_capacity_enforced() {
        let tmp = tempfile::tempdir().unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let state = Arc::new(
            AccountState::new(tmp.path(), "https://relay.test", mailer)
                .with_deployment_mode(DeploymentMode::Commercial),
        );

        let now = now_secs();
        let owner_user_id = "team-owner-user";
        let member_user_id = "team-member-user";
        let org_id = "org-team-pool";
        let member_code_21 = "member-enroll-code-21";

        state
            .mutate(|store| {
                // Insert both users (required by foreign key)
                store.users.insert(
                    owner_user_id.into(),
                    UserRecord {
                        user_id: owner_user_id.into(),
                        email: "owner@team.example".into(),
                        created_at: now,
                    },
                );
                store.users.insert(
                    member_user_id.into(),
                    UserRecord {
                        user_id: member_user_id.into(),
                        email: "member@team.example".into(),
                        created_at: now,
                    },
                );
                // Insert Organization
                store.orgs.insert(
                    org_id.into(),
                    crate::account::store::OrgRecord {
                        org_id: org_id.into(),
                        owner_user_id: owner_user_id.into(),
                        name: "Team Engineering".into(),
                        created_at: now,
                    },
                );
                // Insert Org Memberships
                store.org_members.insert(
                    format!("{org_id}:{owner_user_id}"),
                    crate::account::store::OrgMemberRecord {
                        org_id: org_id.into(),
                        user_id: owner_user_id.into(),
                        role: "owner".into(),
                        joined_at: now,
                    },
                );
                store.org_members.insert(
                    format!("{org_id}:{member_user_id}"),
                    crate::account::store::OrgMemberRecord {
                        org_id: org_id.into(),
                        user_id: member_user_id.into(),
                        role: "member".into(),
                        joined_at: now,
                    },
                );
                // Team subscription: 2 seats (minimum seats = 2), 0 host packs => machineLimit = 20
                store.subscriptions.insert(
                    "sub-team-1".into(),
                    crate::account::store::SubscriptionRecord {
                        subscription_id: "sub-team-1".into(),
                        owner_user_id: owner_user_id.into(),
                        org_id: Some(org_id.into()),
                        plan_key: "team_monthly".into(),
                        seats: 2,
                        host_packs: 0,
                        kind: "base".into(),
                        status: "active".into(),
                        ends_at: None,
                        ls_customer_id: Some("cust-1".into()),
                        ls_updated_at: now,
                        manage_url: None,
                    },
                );

                // Populate 20 machines in the pool: 10 owned by owner, 10 owned by member
                for i in 0..10 {
                    store.machines.insert(
                        format!("rec-owner-{i}"),
                        MachineRecord {
                            machine_record_id: format!("rec-owner-{i}"),
                            owner_user_id: owner_user_id.into(),
                            machine_id: format!("owner-machine-{i}"),
                            display_name: format!("Owner Machine {i}"),
                            public_key: format!("pubkey-owner-{i}"),
                            attach_public_key: format!("attachkey-owner-{i}"),
                            relay_origin: "https://relay.test".into(),
                            platform: "linux".into(),
                            enrollment_epoch: 1,
                            enrolled_at: now,
                            last_seen_at: now,
                        },
                    );
                }
                for i in 0..10 {
                    store.machines.insert(
                        format!("rec-member-{i}"),
                        MachineRecord {
                            machine_record_id: format!("rec-member-{i}"),
                            owner_user_id: member_user_id.into(),
                            machine_id: format!("member-machine-{i}"),
                            display_name: format!("Member Machine {i}"),
                            public_key: format!("pubkey-member-{i}"),
                            attach_public_key: format!("attachkey-member-{i}"),
                            relay_origin: "https://relay.test".into(),
                            platform: "linux".into(),
                            enrollment_epoch: 1,
                            enrolled_at: now,
                            last_seen_at: now,
                        },
                    );
                }

                // Member gets an enrollment code to enroll the 21st machine
                store.enrollment_codes.insert(
                    crate::remote::auth::enrollment_code_hash(member_code_21),
                    EnrollmentCodeRecord {
                        user_id: member_user_id.into(),
                        account_origin: "https://relay.test".into(),
                        expires_at: now + 3600,
                    },
                );

                Ok(())
            })
            .unwrap();

        // Member attempts to enroll machine 21
        let member_id_21 = test_identity("member-machine-21");
        let req21 = test_enroll_request(&state, &member_id_21, member_code_21);

        let err21 = enroll(
            State(state.clone()),
            Bytes::from(serde_json::to_vec(&req21).unwrap()),
        )
        .await
        .expect_err("enrolling 21st machine in 20-machine team pool must fail with 402");

        assert_eq!(err21.status, StatusCode::PAYMENT_REQUIRED);
        assert_eq!(err21.code, "PLAN_LIMIT_REACHED");
        let details = err21.details.expect("PLAN_LIMIT_REACHED carries details");
        assert_eq!(details["plan"], "team_monthly");
        assert_eq!(details["limit"], 20);
        assert_eq!(details["used"], 20);

        let store_after = state.load().expect("load store after refusal");
        assert_eq!(
            store_after.machines.len(),
            20,
            "machine count must remain exactly 20 (rejected machine was not added)"
        );
        assert_eq!(
            store_after.enrollment_codes.len(),
            1,
            "member enrollment code must remain unconsumed"
        );
    }

    #[test]
    fn try_enrolled_machine_by_id_propagates_read_error_on_corrupt_store() {
        let tmp = tempfile::tempdir().unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(
            tmp.path().join("mail"),
        ));
        let state = Arc::new(AccountState::new(
            tmp.path().join("account"),
            "https://relay.test",
            mailer,
        ));
        let now = now_secs();
        state
            .mutate(|store| {
                store.users.insert(
                    "u-1".into(),
                    UserRecord {
                        user_id: "u-1".into(),
                        email: "u-1-corrupt@example.test".into(),
                        created_at: now,
                    },
                );
                store.machines.insert(
                    "rec-1".into(),
                    MachineRecord {
                        machine_record_id: "rec-1".into(),
                        owner_user_id: "u-1".into(),
                        machine_id: "m-1".into(),
                        display_name: "Machine 1".into(),
                        public_key: "pk-1".into(),
                        attach_public_key: "apk-1".into(),
                        relay_origin: "https://relay.test".into(),
                        platform: "linux".into(),
                        enrollment_epoch: 1,
                        enrolled_at: now,
                        last_seen_at: now,
                    },
                );
                Ok(())
            })
            .unwrap();

        // Before corruption: machine found
        let found = try_enrolled_machine_by_id(&state, "m-1").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().machine_id, "m-1");

        // Before corruption: nonexistent machine returns Ok(None)
        let not_found = try_enrolled_machine_by_id(&state, "nonexistent").unwrap();
        assert!(not_found.is_none());

        // Corrupt SQLite store
        let db_path = tmp
            .path()
            .join("account")
            .join(crate::account::store::store_sqlite::STORE_SQLITE_FILENAME);
        let wal_path = tmp
            .path()
            .join("account")
            .join(format!("{}-wal", crate::account::store::store_sqlite::STORE_SQLITE_FILENAME));
        if wal_path.exists() {
            let _ = std::fs::remove_file(&wal_path);
        }
        let shm_path = tmp
            .path()
            .join("account")
            .join(format!("{}-shm", crate::account::store::store_sqlite::STORE_SQLITE_FILENAME));
        if shm_path.exists() {
            let _ = std::fs::remove_file(&shm_path);
        }
        std::fs::write(&db_path, b"corrupted sqlite header garbage data").unwrap();

        // After corruption: try_enrolled_machine_by_id returns Err
        let err = try_enrolled_machine_by_id(&state, "m-1")
            .expect_err("must return Err when store is corrupted");
        assert_eq!(err.code, "INTERNAL_ERROR");

        // enrolled_machine_by_id returns None (swallows error for non-critical callers)
        assert!(enrolled_machine_by_id(&state, "m-1").is_none());
    }
}
