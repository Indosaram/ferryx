use std::collections::HashMap;
use std::path::PathBuf;
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
    lock_account_dir, normalize_email, now_secs, random_token, token_hash, AccountStore,
    EnrollmentCodeRecord, GrantRecord, LoginCodeRecord, MachineRecord, SessionRecord, UserRecord,
    DEFAULT_LOGIN_REQUESTS_PER_HOUR, DEFAULT_MAX_BODY_BYTES, ENROLLMENT_CODE_TTL, LOGIN_CODE_TTL,
    SESSION_TTL,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
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
    login_attempts: Mutex<HashMap<String, Vec<Instant>>>,
    challenges: Mutex<HashMap<String, EnrollChallenge>>,
}

#[derive(Debug, Clone)]
struct EnrollChallenge {
    nonce: String,
    expires_at: u64,
}

impl AccountState {
    pub fn new(
        data_dir: impl Into<PathBuf>,
        origin: impl Into<String>,
        mailer: Arc<dyn Mailer>,
    ) -> Self {
        Self {
            data_dir: data_dir.into(),
            origin: origin.into(),
            relay_origin: crate::remote::state::DEFAULT_RELAY_URL.to_string(),
            mailer,
            login_requests_per_hour: DEFAULT_LOGIN_REQUESTS_PER_HOUR,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            login_attempts: Mutex::new(HashMap::new()),
            challenges: Mutex::new(HashMap::new()),
        }
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

    fn mutate<T>(
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

async fn parse_json<T: for<'de> Deserialize<'de>>(body: Bytes) -> Result<T, ApiError> {
    serde_json::from_slice(&body)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, "BAD_REQUEST", error.to_string()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAuthRequestBody {
    pub email: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAuthResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
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
}

pub async fn device_request(
    State(state): State<Arc<AccountState>>,
    body: Bytes,
) -> Result<Json<DeviceAuthResponse>, ApiError> {
    let request: DeviceAuthRequestBody = parse_json(body).await?;
    let email = request.email.as_deref().map(normalize_email);
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
    let verification_uri_complete = format!("{origin}/api/account/v1/device/approve?code={user_code}");

    if let Some(ref to_email) = email {
        let _ = state.mailer.send_magic_link(
            to_email,
            &verification_uri_complete,
        );
    }

    let expires_at = now_secs() + 900;
    state.mutate(|store| {
        store.device_auths.retain(|_, d| d.expires_at > now_secs());
        store.device_auths.insert(
            token_hash(&device_code),
            crate::account::store::DeviceAuthRecord {
                device_code_hash: token_hash(&device_code),
                user_code: user_code.clone(),
                email: email.clone(),
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
        verification_uri_complete,
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
    state.mutate(|store| {
        let (found_key, user_id, origin) = {
            let mut found = None;
            for (key, record) in store.device_auths.iter() {
                if record.user_code == code_clean && record.expires_at > now {
                    let email = record.email.clone().unwrap_or_else(|| "cli-user@local".into());
                    let user_id = store.users.values()
                        .find(|u| u.email == email)
                        .map(|u| u.user_id.clone())
                        .unwrap_or_else(|| {
                            let uid = format!("usr_{}", &random_token()[..16]);
                            store.users.insert(uid.clone(), crate::account::store::UserRecord {
                                user_id: uid.clone(),
                                email,
                                created_at: now,
                            });
                            uid
                        });
                    found = Some((key.clone(), user_id, state.origin.clone()));
                    break;
                }
            }
            found.ok_or_else(|| {
                ApiError::new(StatusCode::NOT_FOUND, "DEVICE_CODE_NOT_FOUND", "code not found or expired")
            })?
        };

        let enrollment_code = random_token();
        store.enrollment_codes.insert(
            token_hash(&enrollment_code),
            crate::account::store::EnrollmentCodeRecord {
                user_id,
                account_origin: origin,
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
        .route("/api/account/v1/login/request", post(login_request))
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

    #[tokio::test]
    async fn device_flow_lifecycle_request_poll_approve() {
        let tmp = tempfile::tempdir().unwrap();
        let mailer = Arc::new(crate::account::mailer::FileMailer::with_dir(tmp.path().join("mail")));
        let state = Arc::new(AccountState::new(tmp.path(), "https://relay.test", mailer));

        let req_body = serde_json::to_vec(&serde_json::json!({
            "email": "headless@test.local"
        })).unwrap();
        let resp = device_request(State(state.clone()), Bytes::from(req_body)).await.unwrap().0;
        assert_eq!(resp.interval, 2);
        assert!(!resp.user_code.is_empty());

        let poll_body = serde_json::to_vec(&serde_json::json!({
            "deviceCode": resp.device_code
        })).unwrap();
        let pending = device_poll(State(state.clone()), Bytes::from(poll_body.clone())).await.unwrap().0;
        assert_eq!(pending.status, "authorization_pending");
        assert!(pending.enrollment_code.is_none());

        let approve_res = device_approve_get(
            State(state.clone()),
            axum::extract::Query(DeviceApproveQuery { code: resp.user_code.clone() }),
        ).await;
        assert!(approve_res.is_ok());

        let approved = device_poll(State(state.clone()), Bytes::from(poll_body)).await.unwrap().0;
        assert_eq!(approved.status, "approved");
        assert!(approved.enrollment_code.is_some());
    }
}
