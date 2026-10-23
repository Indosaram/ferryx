//! Machine approval requests (RFC 8628 shape): an unauthenticated machine posts its identity,
//! the account owner binds it by user code and approves or denies, and the machine polls once for
//! an enrollment code that only enrolls that exact machine id and public key.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};

use super::service::{authenticate_user, parse_json, run_account_blocking, AccountState, ApiError};
use super::store::{
    normalize_email, now_secs, random_token, token_hash, AccountStore, EnrollmentCodeRecord,
    MachineRequestRecord, ENROLLMENT_CODE_TTL, MACHINE_REQUEST_APPROVED,
    MACHINE_REQUEST_DELIVERED, MACHINE_REQUEST_DENIED, MACHINE_REQUEST_PENDING,
};

pub const MACHINE_REQUEST_TTL_SECS: u64 = 900;
pub const MACHINE_REQUEST_POLL_INTERVAL_SECS: u64 = 3;
pub const MACHINE_REQUEST_CREATE_PER_IP_PER_MINUTE: usize = 10;
pub const MACHINE_REQUEST_MAX_PENDING: usize = 1000;
pub const MACHINE_REQUEST_LOOKUP_PER_USER_PER_MINUTE: usize = 20;
pub const USER_CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const RATE_WINDOW: Duration = Duration::from_secs(60);
const FIELD_MAX_CHARS: usize = 200;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateMachineRequestBody {
    pub machine_id: String,
    pub public_key: String,
    pub display_name: String,
    pub platform: String,
    pub app_version: String,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CreateMachineRequestResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PollMachineRequestBody {
    pub device_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PollMachineRequestResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enrollment_code: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserCodeBody {
    pub user_code: String,
}

/// What the owner sees before deciding. Never carries the device code or the enrollment code.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MachineRequestView {
    pub request_id: String,
    pub user_code: String,
    pub machine_id: String,
    pub display_name: String,
    pub platform: String,
    pub app_version: String,
    pub requester_ip: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub status: String,
}

impl From<&MachineRequestRecord> for MachineRequestView {
    fn from(record: &MachineRequestRecord) -> Self {
        Self {
            request_id: record.request_id.clone(),
            user_code: record.user_code.clone(),
            machine_id: record.machine_id.clone(),
            display_name: record.display_name.clone(),
            platform: record.platform.clone(),
            app_version: record.app_version.clone(),
            requester_ip: record.requester_ip.clone(),
            created_at: record.created_at,
            expires_at: record.expires_at,
            status: record.status.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApproveMachineRequestResponse {
    pub machine_id: String,
}

/// First `cf-connecting-ip` value, else the first `x-forwarded-for` hop, else `unknown`.
pub fn requester_ip(headers: &HeaderMap) -> String {
    let first = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    first("cf-connecting-ip")
        .or_else(|| first("x-forwarded-for"))
        .unwrap_or_else(|| "unknown".to_string())
}

/// `XXXX-XXXX` from [`USER_CODE_ALPHABET`].
pub fn generate_user_code() -> String {
    let mut rng = rand::rngs::OsRng;
    let mut pick = || {
        USER_CODE_ALPHABET[rand::Rng::gen_range(&mut rng, 0..USER_CODE_ALPHABET.len())] as char
    };
    let left: String = (0..4).map(|_| pick()).collect();
    let right: String = (0..4).map(|_| pick()).collect();
    format!("{left}-{right}")
}

/// Accepts `abcd-efgh`, `ABCDEFGH` or surrounding whitespace; returns `ABCD-EFGH` or `None`.
pub fn normalize_user_code(raw: &str) -> Option<String> {
    let compact: String = raw
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if compact.len() != 8 || !compact.bytes().all(|b| USER_CODE_ALPHABET.contains(&b)) {
        return None;
    }
    Some(format!("{}-{}", &compact[..4], &compact[4..]))
}

fn bounded(value: &str) -> String {
    value.trim().chars().take(FIELD_MAX_CHARS).collect()
}

fn bad_request(message: &str) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "BAD_REQUEST", message)
}

fn rate_limited(message: &str) -> ApiError {
    ApiError::new(StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED", message)
}

fn expired() -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "MACHINE_REQUEST_EXPIRED",
        "machine approval request has expired",
    )
}

fn not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "MACHINE_REQUEST_NOT_FOUND",
        "no such machine approval request",
    )
}

fn foreign_account() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "MACHINE_REQUEST_FOREIGN_ACCOUNT",
        "this machine asked to join a different account",
    )
}

fn ensure_hint_matches(record: &MachineRequestRecord, user_email: &str) -> Result<(), ApiError> {
    match record.email_hint.as_deref() {
        Some(hint) if hint != normalize_email(user_email) => Err(foreign_account()),
        _ => Ok(()),
    }
}

/// `POST /api/account/v1/machine-requests` (unauthenticated)
pub async fn create_machine_request(
    State(state): State<Arc<AccountState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<CreateMachineRequestResponse>, ApiError> {
    let request: CreateMachineRequestBody = parse_json(body).await?;
    let machine_id = bounded(&request.machine_id);
    let public_key = request.public_key.trim().to_string();
    if machine_id.is_empty() {
        return Err(bad_request("machineId is required"));
    }
    let key_ok = STANDARD
        .decode(&public_key)
        .map(|bytes| bytes.len() == 32)
        .unwrap_or(false);
    if !key_ok {
        return Err(bad_request("publicKey must be a base64 Ed25519 public key"));
    }
    let email_hint = match request.email.as_deref().map(normalize_email) {
        Some(email) if email.is_empty() => None,
        Some(email) if !email.contains('@') => {
            return Err(bad_request("email must be a valid address"));
        }
        other => other,
    };
    let ip = requester_ip(&headers);
    if !state.allow_onboarding_rate(
        &format!("machine-request-create:{ip}"),
        MACHINE_REQUEST_CREATE_PER_IP_PER_MINUTE,
        RATE_WINDOW,
    ) {
        return Err(rate_limited("too many machine requests from this address"));
    }

    let device_code = random_token();
    let device_code_hash = token_hash(&device_code);
    let display_name = bounded(&request.display_name);
    let platform = bounded(&request.platform);
    let app_version = bounded(&request.app_version);
    let blocking_state = Arc::clone(&state);
    let user_code = run_account_blocking(move || {
        let now = now_secs();
        blocking_state.mutate(|store| {
            let pending = store
                .machine_requests
                .values()
                .filter(|r| r.status == MACHINE_REQUEST_PENDING && r.expires_at > now)
                .count();
            if pending >= MACHINE_REQUEST_MAX_PENDING {
                return Err(rate_limited("too many pending machine requests"));
            }
            let user_code = loop {
                let candidate = generate_user_code();
                let taken = store
                    .machine_requests
                    .values()
                    .any(|r| r.user_code == candidate && r.expires_at > now);
                if !taken {
                    break candidate;
                }
            };
            let request_id = format!("mreq_{}", &random_token()[..24]);
            store.machine_requests.insert(
                request_id.clone(),
                MachineRequestRecord {
                    request_id,
                    device_code_hash: device_code_hash.clone(),
                    user_code: user_code.clone(),
                    machine_id: machine_id.clone(),
                    public_key: public_key.clone(),
                    display_name: if display_name.is_empty() {
                        machine_id.clone()
                    } else {
                        display_name.clone()
                    },
                    platform: platform.clone(),
                    app_version: app_version.clone(),
                    email_hint: email_hint.clone(),
                    requester_ip: ip.clone(),
                    created_at: now,
                    expires_at: now + MACHINE_REQUEST_TTL_SECS,
                    status: MACHINE_REQUEST_PENDING.to_string(),
                    decided_by: None,
                    decided_at: None,
                    enrollment_code: None,
                    last_polled_at: None,
                },
            );
            Ok(user_code)
        })
    })
    .await?;

    let origin = state.origin.trim_end_matches('/');
    Ok(Json(CreateMachineRequestResponse {
        device_code,
        verification_uri: format!("{origin}/approve"),
        verification_uri_complete: format!("{origin}/approve?code={user_code}"),
        user_code,
        expires_in: MACHINE_REQUEST_TTL_SECS,
        interval: MACHINE_REQUEST_POLL_INTERVAL_SECS,
    }))
}

/// `POST /api/account/v1/machine-requests/poll` (unauthenticated; the device code is the secret)
pub async fn poll_machine_request(
    State(state): State<Arc<AccountState>>,
    body: Bytes,
) -> Result<Json<PollMachineRequestResponse>, ApiError> {
    let request: PollMachineRequestBody = parse_json(body).await?;
    let device_code_hash = token_hash(request.device_code.trim());
    run_account_blocking(move || {
        let now = now_secs();
        state.mutate(|store| {
            let record = store
                .machine_requests
                .values_mut()
                .find(|r| r.device_code_hash == device_code_hash)
                .ok_or_else(|| {
                    ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "MACHINE_REQUEST_INVALID",
                        "unknown device code",
                    )
                })?;
            if record.expires_at <= now {
                return Err(expired());
            }
            if let Some(last) = record.last_polled_at {
                if now < last.saturating_add(MACHINE_REQUEST_POLL_INTERVAL_SECS) {
                    return Ok(Err(ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "slow_down",
                        "polling faster than the advertised interval",
                    )
                    .with_details(serde_json::json!({
                        "interval": MACHINE_REQUEST_POLL_INTERVAL_SECS,
                    }))));
                }
            }
            record.last_polled_at = Some(now);
            let status = record.status.clone();
            let response = match status.as_str() {
                MACHINE_REQUEST_PENDING => PollMachineRequestResponse {
                    status: MACHINE_REQUEST_PENDING.into(),
                    enrollment_code: None,
                },
                MACHINE_REQUEST_DENIED => PollMachineRequestResponse {
                    status: MACHINE_REQUEST_DENIED.into(),
                    enrollment_code: None,
                },
                MACHINE_REQUEST_APPROVED => {
                    let code = record.enrollment_code.take();
                    record.status = MACHINE_REQUEST_DELIVERED.to_string();
                    PollMachineRequestResponse {
                        status: MACHINE_REQUEST_APPROVED.into(),
                        enrollment_code: code,
                    }
                }
                _ => {
                    return Ok(Err(ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "MACHINE_REQUEST_DELIVERED",
                        "the approved enrollment code was already delivered",
                    )));
                }
            };
            Ok(Ok(response))
        })?
    })
    .await
    .map(Json)
}

/// `GET /api/account/v1/machine-requests`: pending requests addressed to the caller's email.
pub async fn list_pending_machine_requests(
    State(state): State<Arc<AccountState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<MachineRequestView>>, ApiError> {
    run_account_blocking(move || {
        let user = authenticate_user(&state, &headers)?;
        let email = normalize_email(&user.email);
        let now = now_secs();
        let store = state.load().map_err(ApiError::internal)?;
        let mut views: Vec<MachineRequestView> = store
            .machine_requests
            .values()
            .filter(|r| {
                r.status == MACHINE_REQUEST_PENDING
                    && r.expires_at > now
                    && r.email_hint.as_deref() == Some(email.as_str())
            })
            .map(MachineRequestView::from)
            .collect();
        views.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.request_id.cmp(&right.request_id))
        });
        Ok(views)
    })
    .await
    .map(Json)
}

/// `POST /api/account/v1/machine-requests/lookup`
pub async fn lookup_machine_request(
    State(state): State<Arc<AccountState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<MachineRequestView>, ApiError> {
    let request: UserCodeBody = parse_json(body).await?;
    run_account_blocking(move || {
        let user = authenticate_user(&state, &headers)?;
        if !state.allow_onboarding_rate(
            &format!("machine-request-lookup:{}", user.user_id),
            MACHINE_REQUEST_LOOKUP_PER_USER_PER_MINUTE,
            RATE_WINDOW,
        ) {
            return Err(rate_limited("too many approval code lookups"));
        }
        let user_code = normalize_user_code(&request.user_code).ok_or_else(not_found)?;
        let now = now_secs();
        let store = state.load().map_err(ApiError::internal)?;
        let record = store
            .machine_requests
            .values()
            .filter(|r| r.user_code == user_code)
            .max_by_key(|r| r.created_at)
            .ok_or_else(not_found)?;
        if record.expires_at <= now {
            return Err(expired());
        }
        ensure_hint_matches(record, &user.email)?;
        Ok(MachineRequestView::from(record))
    })
    .await
    .map(Json)
}

/// Shared approve/deny preconditions: the request exists under this id and user code, is not
/// expired, is addressed to this user (when it carries an email hint) and is still pending.
fn pending_request_for_decision<'a>(
    store: &'a mut AccountStore,
    request_id: &str,
    user_code: &str,
    user_email: &str,
    now: u64,
) -> Result<&'a mut MachineRequestRecord, ApiError> {
    let record = store
        .machine_requests
        .get_mut(request_id)
        .filter(|r| r.user_code == user_code)
        .ok_or_else(not_found)?;
    if record.expires_at <= now {
        return Err(expired());
    }
    ensure_hint_matches(record, user_email)?;
    if record.status != MACHINE_REQUEST_PENDING {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "MACHINE_REQUEST_DECIDED",
            "this machine request was already approved or denied",
        ));
    }
    Ok(record)
}

/// `POST /api/account/v1/machine-requests/{requestId}/approve`
pub async fn approve_machine_request(
    State(state): State<Arc<AccountState>>,
    Path(request_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ApproveMachineRequestResponse>, ApiError> {
    let request: UserCodeBody = parse_json(body).await?;
    run_account_blocking(move || {
        let user = authenticate_user(&state, &headers)?;
        let user_code = normalize_user_code(&request.user_code).ok_or_else(not_found)?;
        let now = now_secs();
        let origin = state.origin.clone();
        state.mutate(|store| {
            let (machine_id, public_key) = {
                let record =
                    pending_request_for_decision(store, &request_id, &user_code, &user.email, now)?;
                (record.machine_id.clone(), record.public_key.clone())
            };
            let claimed = store
                .machines
                .values()
                .any(|m| m.machine_id == machine_id && m.owner_user_id != user.user_id);
            if claimed {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "ACCOUNT_MACHINE_CLAIMED",
                    "this machine id is already enrolled to a different account",
                ));
            }
            let enrollment_code = random_token();
            store.enrollment_codes.insert(
                crate::remote::auth::enrollment_code_hash(&enrollment_code),
                EnrollmentCodeRecord {
                    user_id: user.user_id.clone(),
                    account_origin: origin.clone(),
                    expires_at: now + ENROLLMENT_CODE_TTL.as_secs(),
                    created_at: now,
                    bound_machine_id: Some(machine_id.clone()),
                    bound_public_key: Some(public_key),
                    ..Default::default()
                },
            );
            if let Some(record) = store.machine_requests.get_mut(&request_id) {
                record.status = MACHINE_REQUEST_APPROVED.to_string();
                record.decided_by = Some(user.user_id.clone());
                record.decided_at = Some(now);
                record.enrollment_code = Some(enrollment_code);
            }
            Ok(ApproveMachineRequestResponse { machine_id })
        })
    })
    .await
    .map(Json)
}

/// `POST /api/account/v1/machine-requests/{requestId}/deny`
pub async fn deny_machine_request(
    State(state): State<Arc<AccountState>>,
    Path(request_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let request: UserCodeBody = parse_json(body).await?;
    run_account_blocking(move || {
        let user = authenticate_user(&state, &headers)?;
        let user_code = normalize_user_code(&request.user_code).ok_or_else(not_found)?;
        let now = now_secs();
        state.mutate(|store| {
            let record =
                pending_request_for_decision(store, &request_id, &user_code, &user.email, now)?;
            record.status = MACHINE_REQUEST_DENIED.to_string();
            record.decided_by = Some(user.user_id.clone());
            record.decided_at = Some(now);
            Ok(())
        })
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_code_format_and_normalization() {
        for _ in 0..50 {
            let code = generate_user_code();
            assert_eq!(code.len(), 9);
            assert_eq!(&code[4..5], "-");
            assert!(code
                .bytes()
                .filter(|b| *b != b'-')
                .all(|b| USER_CODE_ALPHABET.contains(&b)));
            assert_eq!(normalize_user_code(&code).as_deref(), Some(code.as_str()));
        }
        assert_eq!(normalize_user_code(" abcd-efgh ").as_deref(), Some("ABCD-EFGH"));
        assert_eq!(normalize_user_code("ABCDEFGH").as_deref(), Some("ABCD-EFGH"));
        assert_eq!(normalize_user_code("ABCD-EFG"), None);
        assert_eq!(normalize_user_code("ABCD-EFG0"), None, "0 is not in the alphabet");
    }

    #[test]
    fn requester_ip_prefers_cloudflare_then_forwarded_for() {
        let mut headers = HeaderMap::new();
        assert_eq!(requester_ip(&headers), "unknown");
        headers.insert("x-forwarded-for", "203.0.113.9, 10.0.0.1".parse().unwrap());
        assert_eq!(requester_ip(&headers), "203.0.113.9");
        headers.insert("cf-connecting-ip", "198.51.100.7".parse().unwrap());
        assert_eq!(requester_ip(&headers), "198.51.100.7");
    }
}
