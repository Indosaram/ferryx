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
use super::store::{
    lock_account_dir, normalize_email, now_secs, random_token, signing_key_path, token_hash,
    write_private_json, AccountSigningKeyRecord, AccountStore, EnrollmentCodeRecord, GrantRecord,
    LoginCodeRecord, MachineRecord, SessionRecord, UserRecord, DEFAULT_LOGIN_REQUESTS_PER_HOUR,
    DEFAULT_MAX_BODY_BYTES, ENROLLMENT_CODE_TTL, LOGIN_CODE_TTL, SESSION_TTL,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use crate::remote::account_grants::{submission_signing_input, GrantSubmission};
use crate::remote::account_protocol::{
    AccountEnrollChallenge, AccountEnrollRequest, AccountEnrollResponse, AccountGrantOffer,
    AccountGrantOfferEnvelope, AccountGrantRequest, AccountGrantResponse, AccountGrantScope,
};

pub const GRANT_TTL: Duration = Duration::from_secs(600);

pub struct AccountState {
    pub data_dir: PathBuf,
    pub origin: String,
    pub relay_origin: String,
    pub mailer: Arc<dyn Mailer>,
    pub login_requests_per_hour: u32,
    pub max_body_bytes: usize,
    http_client: reqwest::Client,
    login_attempts: Mutex<HashMap<String, Vec<Instant>>>,
    challenges: Mutex<HashMap<String, EnrollChallenge>>,
    signing_key: Mutex<Option<Arc<SigningKey>>>,
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
            mailer,
            login_requests_per_hour: DEFAULT_LOGIN_REQUESTS_PER_HOUR,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            http_client,
            login_attempts: Mutex::new(HashMap::new()),
            challenges: Mutex::new(HashMap::new()),
            signing_key: Mutex::new(None),
        }
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

    fn load(&self) -> Result<AccountStore, String> {
        AccountStore::load(&self.data_dir)
    }

    pub(crate) fn mutate<T>(
        &self,
        change: impl FnOnce(&mut AccountStore) -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        let _guard = lock_account_dir(&self.data_dir).map_err(ApiError::internal)?;
        let mut store = self.load().map_err(ApiError::internal)?;
        let now = now_secs();
        store.purge_expired(now);
        let value = change(&mut store)?;
        store.save(&self.data_dir).map_err(ApiError::internal)?;
        Ok(value)
    }

    fn read<T>(
        &self,
        read: impl FnOnce(&AccountStore) -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        let store = self.load().map_err(ApiError::internal)?;
        read(&store)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
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
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                code: self.code,
                message: self.message,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoginConsumeBody {
    pub code: String,
}

#[derive(Serialize)]
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

fn bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
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
    let email_approve_url = format!(
        "{origin}/api/account/v1/device/approve?code={user_code}&token={email_token}"
    );
    state
        .mailer
        .send_magic_link(&email, &email_approve_url)
        .map_err(|e| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "MAIL_FAILED", e.to_string()))?;

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
            ApiError::new(StatusCode::BAD_REQUEST, "DEVICE_CODE_INVALID", "invalid code")
        })?;
        if record.expires_at <= now {
            store.device_auths.remove(&code_hash);
            return Err(ApiError::new(StatusCode::BAD_REQUEST, "DEVICE_CODE_EXPIRED", "expired"));
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
) -> Result<StatusCode, ApiError> {
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
        store.login_codes.retain(|_, code| code.expires_at > now_secs());
        store.login_codes.insert(
            token_hash(&code),
            LoginCodeRecord {
                email: email.clone(),
                expires_at,
            },
        );
        Ok(())
    })?;
    Ok(StatusCode::ACCEPTED)
}

pub async fn login_consume(
    State(state): State<Arc<AccountState>>,
    body: Bytes,
) -> Result<Json<LoginConsumeResponse>, ApiError> {
    let request: LoginConsumeBody = parse_json(body).await?;
    let now = now_secs();
    let (bearer_token, user) = state.mutate(|store| {
        let key = token_hash(&request.code);
        let record = store.login_codes.get(&key).ok_or_else(|| {
            ApiError::unauthorized("LOGIN_CODE_USED", "login code is unknown or already used")
        })?;
        if record.expires_at <= now {
            store.login_codes.remove(&key);
            return Err(ApiError::unauthorized(
                "LOGIN_CODE_EXPIRED",
                "login code has expired",
            ));
        }
        let email = record.email.clone();
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
    state.read(|store| {
        Ok(store
            .machines
            .values()
            .filter(|machine| machine.owner_user_id == user.user_id)
            .map(MachineViewResponse::from)
            .collect())
    })
    .map(Json)
}

impl From<&MachineRecord> for MachineViewResponse {
    fn from(record: &MachineRecord) -> Self {
        Self {
            machine_record_id: record.machine_record_id.clone(),
            machine_id: record.machine_id.clone(),
            display_name: record.display_name.clone(),
            public_key: record.public_key.clone(),
            attach_public_key: record.attach_public_key.clone(),
            relay_origin: record.relay_origin.clone(),
            platform: record.platform.clone(),
            online: false,
            enrollment_epoch: record.enrollment_epoch,
            last_seen_at: record.last_seen_at,
        }
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

    let (user_id, machine_record_id, epoch) = state.mutate(|store| {
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
        if record.account_origin != state.origin {
            return Err(ApiError::unauthorized(
                "ENROLL_ORIGIN_MISMATCH",
                "enrollment code was issued for a different account origin",
            ));
        }
        store.enrollment_codes.remove(&code_hash);
        let user_id = record.user_id;

        let existing = store
            .machines
            .values()
            .find(|machine| machine.machine_id == request.machine_id)
            .cloned();
        let platform = platform_name(&request.platform);
        let (machine_record_id, epoch) = match existing {
            Some(machine) if machine.owner_user_id != user_id => {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "ACCOUNT_MACHINE_CLAIMED",
                    "this machine id is already enrolled to a different account",
                ));
            }
            Some(machine) => {
                let epoch = machine.enrollment_epoch + 1;
                let updated = MachineRecord {
                    machine_record_id: machine.machine_record_id.clone(),
                    owner_user_id: machine.owner_user_id.clone(),
                    machine_id: machine.machine_id.clone(),
                    display_name: request.display_name.clone(),
                    public_key: request.public_key.clone(),
                    attach_public_key: request.attach_public_key.clone(),
                    relay_origin: state.relay_origin.clone(),
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
                    machine_id: request.machine_id.clone(),
                    display_name: request.display_name.clone(),
                    public_key: request.public_key.clone(),
                    attach_public_key: request.attach_public_key.clone(),
                    relay_origin: state.relay_origin.clone(),
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
        Ok((user_id, machine_record_id, epoch))
    })?;

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
    let user = require_user(&state, &headers)?;
    let request: AccountGrantRequest = parse_json(body).await?;
    let now = now_secs();

    let (grant, pairing_token, machine, offer) = state.mutate(|store| {
        let machine = store
            .machines
            .get(&machine_record_id)
            .cloned()
            .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "MACHINE_NOT_FOUND", "no such machine"))?;
        if machine.owner_user_id != user.user_id {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                "MACHINE_NOT_FOUND",
                "no such machine on this account",
            ));
        }
        if machine.enrollment_epoch.to_string() != request.enrollment_epoch {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "ACCOUNT_ENROLLMENT_EPOCH_MISMATCH",
                "machine re-enrolled since this view was fetched",
            ));
        }
        let pairing_token = random_token();
        let grant = GrantRecord {
            grant_id: uuid::Uuid::new_v4().to_string(),
            machine_record_id: machine.machine_record_id.clone(),
            owner_user_id: machine.owner_user_id.clone(),
            pairing_token_hash: token_hash(&pairing_token),
            grant_scope: scope_name(request.grant_scope),
            device_attach_public_key: request.attach_public_key.clone(),
            installation_id: request.installation_id.clone(),
            issued_at: now,
            expires_at: now + GRANT_TTL.as_secs(),
        };
        store.grants.insert(grant.grant_id.clone(), grant.clone());
        let offer = AccountGrantOffer {
            grant_id: grant.grant_id.clone(),
            machine_id: machine.machine_id.clone(),
            enrollment_epoch: machine.enrollment_epoch.to_string(),
            pairing_token: pairing_token.clone(),
            device_label: request.device_label.clone(),
            installation_id: request.installation_id.clone(),
            grant_scope: request.grant_scope,
            expires_at: grant.expires_at,
            device_attach_public_key: request.attach_public_key.clone(),
        };
        Ok((grant, pairing_token, machine, offer))
    })?;

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

    let signing_key = state
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

    let relay_url = format!(
        "{}/api/v1/attach/grant",
        machine.relay_origin.trim_end_matches('/')
    );
    let delivery_response = state
        .http_client
        .post(&relay_url)
        .timeout(Duration::from_secs(5))
        .json(&submission)
        .send()
        .await
        .map_err(|error| {
            let _ = state.mutate(|store| {
                store.grants.remove(&grant.grant_id);
                Ok(())
            });
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                "GRANT_DELIVERY_FAILED",
                format!("failed to deliver grant to relay at {relay_url}: {error}"),
            )
        })?;

    if !delivery_response.status().is_success() {
        let status = delivery_response.status();
        let body = delivery_response
            .text()
            .await
            .unwrap_or_else(|_| "<unreadable response body>".to_string());
        let _ = state.mutate(|store| {
            store.grants.remove(&grant.grant_id);
            Ok(())
        });
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
            let _ = state.mutate(|store| {
                store.grants.remove(&grant.grant_id);
                Ok(())
            });
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "GRANT_DELIVERY_FAILED",
                "relay returned invalid grant delivery response",
            ));
        }
    };

    if value.get("accepted").and_then(|v| v.as_bool()) == Some(false) {
        let _ = state.mutate(|store| {
            store.grants.remove(&grant.grant_id);
            Ok(())
        });
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
            let _ = state.mutate(|store| {
                store.grants.remove(&grant.grant_id);
                Ok(())
            });
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "GRANT_DELIVERY_FAILED",
                format!("machine refused grant delivery: {other}"),
            ));
        }
        None => {
            let _ = state.mutate(|store| {
                store.grants.remove(&grant.grant_id);
                Ok(())
            });
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
    Router::new()
        .route("/api/account/v1/public-key", get(get_public_key))
        .route("/api/account/v1/login/request", post(login_request))
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
        )
        .layer(DefaultBodyLimit::max(limit))
        .with_state(state)
}

pub async fn serve(listener: tokio::net::TcpListener, state: Arc<AccountState>) -> Result<(), String> {
    axum::serve(listener, router(state))
        .await
        .map_err(|error| format!("account server failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_magic_link(mail_dir: &std::path::Path) -> String {
        let entries: Vec<_> = std::fs::read_dir(mail_dir)
            .expect("mail dir")
            .filter_map(|entry| entry.ok())
            .collect();
        assert_eq!(entries.len(), 1, "device request sends exactly one magic link");
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
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(mail_dir.clone()));
        let state = Arc::new(AccountState::new(tmp.path(), "https://relay.test", mailer));

        let req_body = serde_json::to_vec(&serde_json::json!({
            "email": "headless@test.local"
        })).unwrap();
        let resp = device_request(State(state.clone()), Bytes::from(req_body)).await.unwrap().0;
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
        })).unwrap();
        let pending = device_poll(State(state.clone()), Bytes::from(poll_body.clone())).await.unwrap().0;
        assert_eq!(pending.status, "authorization_pending");
        assert!(pending.enrollment_code.is_none());

        let rejected = device_approve_get(
            State(state.clone()),
            axum::extract::Query(DeviceApproveQuery {
                code: resp.user_code.clone(),
                token: "wrong_token".into(),
            }),
        ).await;
        let error = match rejected {
            Err(error) => error,
            Ok(_) => panic!("approval without the emailed token must fail"),
        };
        assert_eq!(error.status, StatusCode::UNAUTHORIZED);
        assert_eq!(error.code, "EMAIL_TOKEN_INVALID");

        let still_pending = device_poll(State(state.clone()), Bytes::from(poll_body.clone())).await.unwrap().0;
        assert_eq!(still_pending.status, "authorization_pending");
        assert!(still_pending.enrollment_code.is_none());

        let approve_res = device_approve_get(
            State(state.clone()),
            axum::extract::Query(DeviceApproveQuery {
                code: resp.user_code.clone(),
                token: valid_token,
            }),
        ).await;
        assert!(approve_res.is_ok());

        let approved = device_poll(State(state.clone()), Bytes::from(poll_body)).await.unwrap().0;
        assert_eq!(approved.status, "approved");
        assert!(approved.enrollment_code.is_some());
    }

    #[tokio::test]
    async fn device_request_rejects_invalid_email() {
        let tmp = tempfile::tempdir().unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(tmp.path().join("mail")));
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

        let state1 = Arc::new(AccountState::new(tmp.path(), "https://account.test", mailer.clone()));
        let pk1 = state1.account_public_key();
        assert!(!pk1.is_empty(), "public key must not be empty");

        let key_file = tmp.path().join("account-signing-key.json");
        assert!(key_file.exists(), "signing key file must be created on disk");

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

        let state2 = Arc::new(AccountState::new(tmp.path(), "https://account.test", mailer));
        let pk2 = state2.account_public_key();
        assert_eq!(pk1, pk2, "reloading the same directory must yield the same public key");
    }

    #[tokio::test]
    async fn public_key_endpoint_returns_base64_pinned_key() {
        let tmp = tempfile::tempdir().unwrap();
        let mail_dir = tmp.path().join("mail");
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(mail_dir));
        let state = Arc::new(AccountState::new(tmp.path(), "https://account.test", mailer));
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
        let state = Arc::new(AccountState::new(tmp.path(), "https://account.test", mailer));

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
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(tmp.path().join("mail")));
        let state = Arc::new(AccountState::new(tmp.path(), "https://account.test", mailer));

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
            machine_id: "test-machine-1".to_string(),
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
                        email: "tester@example.com".to_string(),
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
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(tmp.path().join("mail")));
        let state = Arc::new(AccountState::new(tmp.path(), "https://account.test", mailer));

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
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(tmp.path().join("mail")));
        let state = Arc::new(AccountState::new(tmp.path(), "https://account.test", mailer));

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
}
