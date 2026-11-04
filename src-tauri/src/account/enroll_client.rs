use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::remote::account_protocol::{
    AccountEnrollRequest, AccountEnrollResponse, AccountGrantRequest, AccountGrantResponse,
    AccountGrantScope,
};
use crate::remote::attach_identity::load_or_generate_canonical_attach_identity;
use crate::remote::auth::{
    canonical_remote_dir, enrollment_code_hash, load_or_generate_machine_identity,
    sign_account_enrollment, write_private_json,
};
use crate::remote::machine_protocol::Platform;

pub const ACCOUNT_ENROLLMENT_FILE: &str = "account-enrollment.json";

/// The account session this machine signed in with (`ferryx account login`, or the desktop's
/// link step), next to the enrollment record and readable by the owner only. The daemon reads
/// it to pair itself to the supervisor lease holder when `FERRYX_ACCOUNT_SESSION_TOKEN` is unset.
pub const ACCOUNT_SESSION_FILE: &str = "account-session.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredAccountSession {
    pub account_origin: String,
    pub session_token: String,
}

pub fn account_session_path() -> Option<PathBuf> {
    canonical_remote_dir().map(|dir| dir.join(ACCOUNT_SESSION_FILE))
}

/// Writes the session owner-only: mode 0600 on Unix (by [`write_private_json`]), and on Windows
/// an ACL granting only the current user, as the SSH helper's private files are restricted.
pub fn save_account_session_at(path: &std::path::Path, account_origin: &str, session_token: &str) -> Result<(), String> {
    save_account_session_restricted(path, account_origin, session_token, restrict_session_file)
}

/// The owner-only step after the write: nothing more on Unix ([`write_private_json`] created
/// the file 0600), an owner-only ACL on Windows.
fn restrict_session_file(path: &std::path::Path) -> Result<(), String> {
    #[cfg(windows)]
    crate::ssh::helper_runtime::private_file(path)?;
    #[cfg(not(windows))]
    let _ = path;
    Ok(())
}

/// [`save_account_session_at`] with the owner-only step `restrict`. If it fails the file is
/// removed before the error is returned, so a 30-day bearer never stays on disk with
/// inherited permissions.
fn save_account_session_restricted(
    path: &std::path::Path,
    account_origin: &str,
    session_token: &str,
    restrict: impl FnOnce(&std::path::Path) -> Result<(), String>,
) -> Result<(), String> {
    let record = StoredAccountSession {
        account_origin: account_origin.trim_end_matches('/').to_string(),
        session_token: session_token.trim().to_string(),
    };
    if record.session_token.is_empty() {
        return Err("ACCOUNT_SESSION_EMPTY: no session token to save".into());
    }
    write_private_json(path, &record).map_err(|error| format!("ACCOUNT_SESSION_SAVE_FAILED: {error}"))?;
    if let Err(error) = restrict(path) {
        return match std::fs::remove_file(path) {
            Ok(()) => Err(format!("ACCOUNT_SESSION_SAVE_FAILED: the file could not be made owner-only and was removed: {error}")),
            Err(remove) => Err(format!(
                "ACCOUNT_SESSION_SAVE_FAILED: the file could not be made owner-only ({error}) nor removed ({remove}): delete {}",
                path.display()
            )),
        };
    }
    Ok(())
}

pub fn save_account_session(account_origin: &str, session_token: &str) -> Result<(), String> {
    let path = account_session_path().ok_or("ACCOUNT_IDENTITY_DIR_UNRESOLVED: cannot resolve identity dir")?;
    save_account_session_at(&path, account_origin, session_token)
}

pub fn load_account_session_at(path: &std::path::Path) -> Option<StoredAccountSession> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn load_account_session() -> Option<StoredAccountSession> {
    load_account_session_at(&account_session_path()?)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountEnrollmentRecord {
    pub account_id: String,
    pub machine_record_id: String,
    pub account_origin: String,
    pub relay_origin: String,
    pub enrollment_epoch: String,
    pub enrolled_at: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnrollError {
    pub code: String,
    pub message: String,
    /// Account-service HTTP status; `0` for failures detected locally.
    pub status: u16,
    /// Structured `details` from the `{code, message, details}` envelope, when the server sends it.
    pub details: Option<serde_json::Value>,
}

impl EnrollError {
    fn local(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            status: 0,
            details: None,
        }
    }

    fn remote(status: u16, body: &str) -> Self {
        let value: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
        let code = value["code"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format!("ACCOUNT_HTTP_{status}"));
        let message = value["message"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| body.to_string());
        let details = value
            .get("details")
            .filter(|details| !details.is_null())
            .cloned();
        Self {
            code,
            message,
            status,
            details,
        }
    }
}

impl std::fmt::Display for EnrollError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

pub fn enrollment_record_path() -> Option<PathBuf> {
    canonical_remote_dir().map(|dir| dir.join(ACCOUNT_ENROLLMENT_FILE))
}

pub fn load_enrollment_record() -> Option<AccountEnrollmentRecord> {
    let path = enrollment_record_path()?;
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn platform_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Linux => "linux",
        Platform::Macos => "macos",
        Platform::Windows => "windows",
    }
}

fn current_platform() -> Platform {
    if cfg!(target_os = "macos") {
        Platform::Macos
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Linux
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

pub async fn enroll_machine(
    account_origin: &str,
    enrollment_code: &str,
) -> Result<AccountEnrollmentRecord, Box<EnrollError>> {
    enroll_machine_named(account_origin, enrollment_code, None).await
}

/// [`enroll_machine`] with an optional display name that replaces the identity's own name in the
/// account record (`ferryx-cli up --name`). The name is not part of the signed payload.
pub async fn enroll_machine_named(
    account_origin: &str,
    enrollment_code: &str,
    display_name: Option<&str>,
) -> Result<AccountEnrollmentRecord, Box<EnrollError>> {
    let origin = account_origin.trim_end_matches('/');
    let dir = canonical_remote_dir()
        .ok_or_else(|| EnrollError::local("ACCOUNT_IDENTITY_DIR_UNRESOLVED", "cannot resolve identity dir"))?;
    let identity = load_or_generate_machine_identity(&dir).map_err(|error| {
        EnrollError::local("ACCOUNT_IDENTITY_UNAVAILABLE", error)
    })?;
    let attach = load_or_generate_canonical_attach_identity().map_err(|error| {
        EnrollError::local("ATTACH_IDENTITY_UNAVAILABLE", error)
    })?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| EnrollError::local("ACCOUNT_CLIENT_FAILED", error.to_string()))?;

    let challenge_url = format!("{origin}/api/account/v1/machines/enroll/challenge");
    let challenge_response = client
        .post(&challenge_url)
        .json(&serde_json::json!({ "machineId": identity.machine_id }))
        .send()
        .await
        .map_err(|error| EnrollError::local("ACCOUNT_UNREACHABLE", error.to_string()))?;
    let status = challenge_response.status().as_u16();
    let body = challenge_response
        .text()
        .await
        .map_err(|error| EnrollError::local("ACCOUNT_UNREACHABLE", error.to_string()))?;
    if status != 200 {
        return Err(EnrollError::remote(status, &body).into());
    }
    let challenge: serde_json::Value = serde_json::from_str(&body)
        .map_err(|error| EnrollError::local("ACCOUNT_BAD_RESPONSE", error.to_string()))?;
    let nonce = challenge["nonce"]
        .as_str()
        .ok_or_else(|| EnrollError::local("ACCOUNT_BAD_RESPONSE", "challenge had no nonce"))?
        .to_string();
    let timestamp = now_secs();
    let signature = sign_account_enrollment(
        &identity,
        origin,
        &enrollment_code_hash(enrollment_code),
        &nonce,
        timestamp,
    )
    .map_err(|error| EnrollError::local("ACCOUNT_SIGN_FAILED", error))?;

    let request = AccountEnrollRequest {
        api_version: 1,
        enrollment_code: enrollment_code.to_string(),
        machine_id: identity.machine_id.clone(),
        display_name: requested_display_name(display_name)
            .unwrap_or_else(|| identity.display_name.clone()),
        public_key: identity.public_key.clone(),
        attach_public_key: attach.public_key.clone(),
        platform: current_platform(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        nonce,
        timestamp,
        signature,
    };

    let enroll_url = format!("{origin}/api/account/v1/machines/enroll");
    let response = client
        .post(&enroll_url)
        .json(&request)
        .send()
        .await
        .map_err(|error| EnrollError::local("ACCOUNT_UNREACHABLE", error.to_string()))?;
    let status = response.status().as_u16();
    let body = response
        .text()
        .await
        .map_err(|error| EnrollError::local("ACCOUNT_UNREACHABLE", error.to_string()))?;
    if status != 200 {
        return Err(EnrollError::remote(status, &body).into());
    }
    let enrolled: AccountEnrollResponse = serde_json::from_str(&body)
        .map_err(|error| EnrollError::local("ACCOUNT_BAD_RESPONSE", error.to_string()))?;

    let record = AccountEnrollmentRecord {
        account_id: enrolled.account_id,
        machine_record_id: enrolled.machine_record_id,
        account_origin: origin.to_string(),
        relay_origin: enrolled.relay_origin,
        enrollment_epoch: enrolled.enrollment_epoch,
        enrolled_at: enrolled.enrolled_at,
    };
    let path = enrollment_record_path()
        .ok_or_else(|| EnrollError::local("ACCOUNT_IDENTITY_DIR_UNRESOLVED", "cannot resolve identity dir"))?;
    write_private_json(&path, &record)
        .map_err(|error| EnrollError::local("ACCOUNT_ENROLLMENT_SAVE_FAILED", error.to_string()))?;
    // Pin the account public key now, over the channel enrollment just used, so the direct lease
    // and account auto-trust work without an operator env var. A failure here is retried lazily.
    if let Err(error) = crate::remote::direct_lease::ensure_account_public_key(None, &client).await {
        tracing::warn!("account public key not pinned at enrollment: {error}");
    }

    Ok(record)
}

pub async fn request_machine_grant(
    account_origin: &str,
    session_bearer: &str,
    enrollment_epoch: &str,
    device_label: &str,
    installation_id: &str,
    grant_scope: AccountGrantScope,
) -> Result<AccountGrantResponse, Box<EnrollError>> {
    let record = load_enrollment_record().ok_or_else(|| {
        EnrollError::local("ACCOUNT_NOT_ENROLLED", "run ferryx-cli account enroll first")
    })?;
    let attach = load_or_generate_canonical_attach_identity().map_err(|error| {
        EnrollError::local("ATTACH_IDENTITY_UNAVAILABLE", error)
    })?;
    let origin = account_origin.trim_end_matches('/');
    let url = format!(
        "{origin}/api/account/v1/machines/{}/grants",
        record.machine_record_id
    );
    let request = AccountGrantRequest {
        machine_record_id: record.machine_record_id.clone(),
        enrollment_epoch: if enrollment_epoch.is_empty() {
            record.enrollment_epoch.clone()
        } else {
            enrollment_epoch.to_string()
        },
        device_label: device_label.to_string(),
        installation_id: installation_id.to_string(),
        grant_scope,
        attach_public_key: attach.public_key.clone(),
        requester_machine: None,
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| EnrollError::local("ACCOUNT_CLIENT_FAILED", error.to_string()))?;
    let response = client
        .post(&url)
        .bearer_auth(session_bearer)
        .json(&request)
        .send()
        .await
        .map_err(|error| EnrollError::local("ACCOUNT_UNREACHABLE", error.to_string()))?;
    let status = response.status().as_u16();
    let body = response
        .text()
        .await
        .map_err(|error| EnrollError::local("ACCOUNT_UNREACHABLE", error.to_string()))?;
    if status != 200 {
        return Err(EnrollError::remote(status, &body).into());
    }
    serde_json::from_str(&body)
        .map_err(|error| EnrollError::local("ACCOUNT_BAD_RESPONSE", error.to_string())).map_err(Box::new)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAuthClientResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

pub async fn request_device_auth(
    account_origin: &str,
    email: &str,
) -> Result<DeviceAuthClientResponse, Box<EnrollError>> {
    let origin = account_origin.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| EnrollError::local("ACCOUNT_CLIENT_FAILED", e.to_string()))?;
    let url = format!("{origin}/api/account/v1/device/request");
    let resp = client
        .post(&url)
        .json(&serde_json::json!({ "email": email }))
        .send()
        .await
        .map_err(|e| EnrollError::local("ACCOUNT_UNREACHABLE", e.to_string()))?;
    let status = resp.status().as_u16();
    let body = resp
        .text()
        .await
        .map_err(|e| EnrollError::local("ACCOUNT_UNREACHABLE", e.to_string()))?;
    if status != 200 {
        return Err(EnrollError::remote(status, &body).into());
    }
    serde_json::from_str(&body)
        .map_err(|e| EnrollError::local("ACCOUNT_BAD_RESPONSE", e.to_string())).map_err(Box::new)
}

/// An approved headless login: the enrollment code, and the account session the service minted
/// with it (absent from an account service that predates it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceAuthApproval {
    pub enrollment_code: String,
    pub session_token: Option<String>,
}

pub async fn poll_device_auth(
    account_origin: &str,
    device_code: &str,
) -> Result<Option<DeviceAuthApproval>, Box<EnrollError>> {
    let origin = account_origin.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| EnrollError::local("ACCOUNT_CLIENT_FAILED", e.to_string()))?;
    let url = format!("{origin}/api/account/v1/device/poll");
    let resp = client
        .post(&url)
        .json(&serde_json::json!({ "deviceCode": device_code }))
        .send()
        .await
        .map_err(|e| EnrollError::local("ACCOUNT_UNREACHABLE", e.to_string()))?;
    let status = resp.status().as_u16();
    let body = resp
        .text()
        .await
        .map_err(|e| EnrollError::local("ACCOUNT_UNREACHABLE", e.to_string()))?;
    if status != 200 {
        return Err(EnrollError::remote(status, &body).into());
    }
    let val: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| EnrollError::local("ACCOUNT_BAD_RESPONSE", e.to_string()))?;
    if val["status"].as_str() == Some("approved") {
        let code = val["enrollmentCode"]
            .as_str()
            .ok_or_else(|| EnrollError::local("ACCOUNT_BAD_RESPONSE", "missing enrollmentCode"))?;
        let session_token = val["sessionToken"]
            .as_str()
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(str::to_string);
        Ok(Some(DeviceAuthApproval { enrollment_code: code.to_string(), session_token }))
    } else {
        Ok(None)
    }
}

/// What `ferryx account login` did with this machine's enrollment.
#[derive(Debug)]
pub struct LoginOutcome {
    pub record: AccountEnrollmentRecord,
    /// True when the machine was already enrolled to the approving owner at this origin and
    /// its enrollment (and so its enrollment epoch) was kept as it was.
    pub kept_enrollment: bool,
    /// The result of saving the approval's account session; `None` when it carried none.
    pub session_saved: Option<Result<(), String>>,
}

/// Finishes a headless login in the identity directory `dir`. A machine already enrolled at
/// `origin` to the owner who approved the login keeps its enrollment untouched: re-enrolling
/// would bump its enrollment epoch, which every paired client binds into the attach
/// handshake, and so break every existing pairing to it. Only the account session is saved.
/// A machine not enrolled there, or enrolled to another owner, is enrolled with `enroll`.
/// Without a session in the approval (an older account service) ownership cannot be checked,
/// so the machine is enrolled as before.
pub async fn finish_login_at<F, Fut>(
    dir: &std::path::Path,
    origin: &str,
    approval: &DeviceAuthApproval,
    enroll: F,
) -> Result<LoginOutcome, Box<EnrollError>>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<AccountEnrollmentRecord, Box<EnrollError>>>,
{
    let origin = origin.trim_end_matches('/');
    let existing = std::fs::read(dir.join(ACCOUNT_ENROLLMENT_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<AccountEnrollmentRecord>(&bytes).ok())
        .filter(|record| record.account_origin.trim_end_matches('/') == origin);
    let kept = match (existing, approval.session_token.as_deref()) {
        (Some(record), Some(session)) => {
            let same_owner = owner_has_machine(origin, session, &record.machine_record_id).await?;
            same_owner.then_some(record)
        }
        _ => None,
    };
    let kept_enrollment = kept.is_some();
    let record = match kept {
        Some(record) => record,
        None => enroll().await?,
    };
    let session_saved = approval
        .session_token
        .as_deref()
        .map(|session| save_account_session_at(&dir.join(ACCOUNT_SESSION_FILE), origin, session));
    Ok(LoginOutcome { record, kept_enrollment, session_saved })
}

/// Whether the owner of `session` has the machine record `machine_record_id` at `origin`
/// (`GET /api/account/v1/machines` lists only the session owner's machines).
async fn owner_has_machine(origin: &str, session: &str, machine_record_id: &str) -> Result<bool, Box<EnrollError>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| EnrollError::local("ACCOUNT_CLIENT_FAILED", error.to_string()))?;
    let response = client
        .get(format!("{origin}/api/account/v1/machines"))
        .bearer_auth(session)
        .send()
        .await
        .map_err(|error| EnrollError::local("ACCOUNT_UNREACHABLE", error.to_string()))?;
    let status = response.status().as_u16();
    let body = response
        .text()
        .await
        .map_err(|error| EnrollError::local("ACCOUNT_UNREACHABLE", error.to_string()))?;
    if status != 200 {
        return Err(EnrollError::remote(status, &body).into());
    }
    let machines: Vec<serde_json::Value> = serde_json::from_str(&body)
        .map_err(|error| EnrollError::local("ACCOUNT_BAD_RESPONSE", error.to_string()))?;
    Ok(machines
        .iter()
        .any(|machine| machine["machineRecordId"].as_str() == Some(machine_record_id)))
}

fn requested_display_name(display_name: Option<&str>) -> Option<String> {
    display_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn account_http_client() -> Result<reqwest::Client, Box<EnrollError>> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| EnrollError::local("ACCOUNT_CLIENT_FAILED", error.to_string())).map_err(Box::new)
}

/// Error code the account service answers a machine-request poll with when the client polls
/// faster than the advertised interval (RFC 8628 `slow_down`).
pub const MACHINE_REQUEST_SLOW_DOWN: &str = "slow_down";

/// `POST /api/account/v1/machine-requests` response: an RFC 8628 shaped approval request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineRequestClientResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// One answer of `POST /api/account/v1/machine-requests/poll`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineRequestPoll {
    Pending,
    /// The poll came faster than the interval; the caller must lengthen it.
    SlowDown,
    /// `session_token`: the approving owner's account session, when the service sends one.
    Approved { enrollment_code: String, session_token: Option<String> },
    Denied,
}

/// Asks the account service to let its owner approve this machine. Posts the machine identity
/// (created on first use) and the optional `email` hint; nothing secret leaves the machine.
pub async fn create_machine_request(
    account_origin: &str,
    email: Option<&str>,
    display_name: Option<&str>,
) -> Result<MachineRequestClientResponse, Box<EnrollError>> {
    let origin = account_origin.trim_end_matches('/');
    let dir = canonical_remote_dir()
        .ok_or_else(|| EnrollError::local("ACCOUNT_IDENTITY_DIR_UNRESOLVED", "cannot resolve identity dir"))?;
    let identity = load_or_generate_machine_identity(&dir)
        .map_err(|error| EnrollError::local("ACCOUNT_IDENTITY_UNAVAILABLE", error))?;
    let mut body = serde_json::json!({
        "machineId": identity.machine_id,
        "publicKey": identity.public_key,
        "displayName": requested_display_name(display_name).unwrap_or(identity.display_name),
        "platform": platform_name(current_platform()),
        "appVersion": env!("CARGO_PKG_VERSION"),
    });
    if let Some(email) = email.map(str::trim).filter(|email| !email.is_empty()) {
        body["email"] = serde_json::Value::String(email.to_string());
    }
    let response = account_http_client()?
        .post(format!("{origin}/api/account/v1/machine-requests"))
        .json(&body)
        .send()
        .await
        .map_err(|error| EnrollError::local("ACCOUNT_UNREACHABLE", error.to_string()))?;
    let status = response.status().as_u16();
    let body = response
        .text()
        .await
        .map_err(|error| EnrollError::local("ACCOUNT_UNREACHABLE", error.to_string()))?;
    if !(200..300).contains(&status) {
        return Err(EnrollError::remote(status, &body).into());
    }
    serde_json::from_str(&body)
        .map_err(|error| EnrollError::local("ACCOUNT_BAD_RESPONSE", error.to_string())).map_err(Box::new)
}

/// Polls one machine request. `slow_down` is an answer, not an error; an expired request is
/// the structured `MACHINE_REQUEST_EXPIRED` error.
pub async fn poll_machine_request(
    account_origin: &str,
    device_code: &str,
) -> Result<MachineRequestPoll, Box<EnrollError>> {
    let origin = account_origin.trim_end_matches('/');
    let response = account_http_client()?
        .post(format!("{origin}/api/account/v1/machine-requests/poll"))
        .json(&serde_json::json!({ "deviceCode": device_code }))
        .send()
        .await
        .map_err(|error| EnrollError::local("ACCOUNT_UNREACHABLE", error.to_string()))?;
    let status = response.status().as_u16();
    let body = response
        .text()
        .await
        .map_err(|error| EnrollError::local("ACCOUNT_UNREACHABLE", error.to_string()))?;
    parse_machine_request_poll(status, &body)
}

/// Classifies one poll response by its HTTP status and structured body.
pub fn parse_machine_request_poll(status: u16, body: &str) -> Result<MachineRequestPoll, Box<EnrollError>> {
    if !(200..300).contains(&status) {
        let error = EnrollError::remote(status, body);
        let rfc_error = serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|value| value.get("error").and_then(|error| error.as_str()).map(str::to_string));
        if error.code == MACHINE_REQUEST_SLOW_DOWN
            || rfc_error.as_deref() == Some(MACHINE_REQUEST_SLOW_DOWN)
        {
            return Ok(MachineRequestPoll::SlowDown);
        }
        return Err(error.into());
    }
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|error| EnrollError::local("ACCOUNT_BAD_RESPONSE", error.to_string()))?;
    match value["status"].as_str() {
        Some("pending") => Ok(MachineRequestPoll::Pending),
        Some("denied") => Ok(MachineRequestPoll::Denied),
        Some("approved") => value["enrollmentCode"]
            .as_str()
            .filter(|code| !code.is_empty())
            .map(|code| MachineRequestPoll::Approved {
                enrollment_code: code.to_string(),
                session_token: value["sessionToken"]
                    .as_str()
                    .map(str::trim)
                    .filter(|token| !token.is_empty())
                    .map(str::to_owned),
            })
            .ok_or_else(|| EnrollError::local("ACCOUNT_BAD_RESPONSE", "approved poll had no enrollmentCode").into()),
        other => Err(EnrollError::local(
            "ACCOUNT_BAD_RESPONSE",
            format!("unknown machine request status {other:?}"),
        ).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::auth::{
        enrollment_code_hash, verify_account_enrollment, AuthManager, DevicePermission,
    };

    /// F2 MINOR-3: when the owner-only step fails (on Windows: no USERNAME or icacls refused)
    /// the written session is removed and the error returned; nothing is left behind.
    #[test]
    fn a_session_that_cannot_be_made_owner_only_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ACCOUNT_SESSION_FILE);
        let error = save_account_session_restricted(&path, "https://account.test", "secret", |written| {
            assert!(written.exists(), "the step runs on the written file");
            Err("REMOTE_PERMISSION_DENIED: cannot restrict runtime ACL".into())
        })
        .unwrap_err();
        assert!(error.starts_with("ACCOUNT_SESSION_SAVE_FAILED"), "{error}");
        assert!(error.contains("REMOTE_PERMISSION_DENIED"), "{error}");
        assert!(!path.exists(), "the unrestricted session file was removed");
        assert_eq!(load_account_session_at(&path), None);

        save_account_session_restricted(&path, "https://account.test/", " secret ", |_| Ok(())).unwrap();
        let saved = load_account_session_at(&path).unwrap();
        assert_eq!((saved.account_origin.as_str(), saved.session_token.as_str()), ("https://account.test", "secret"));
    }

    #[test]
    fn enrollment_leaves_existing_device_tokens_alone() {
        let dir = tempfile::tempdir().expect("temp");
        let remote = dir.path().join("remote");
        std::fs::create_dir_all(&remote).expect("remote dir");
        let identity_path = remote.join("identity.json");
        let identity = load_or_generate_machine_identity(&remote).expect("identity");
        let identity_bytes = std::fs::read(&identity_path).expect("identity bytes");

        let auth_path = remote.join("remote-auth.json");
        let manager = AuthManager::with_persistence(Some(auth_path.clone()));
        let code = manager.create_pairing_code(DevicePermission::Control);
        let (legacy_bearer, _device) = manager
            .exchange_pairing_code(&code, "Old Phone")
            .expect("legacy pairing");
        let auth_before = std::fs::read(&auth_path).expect("auth file");

        let record = AccountEnrollmentRecord {
            account_id: "acct-1".into(),
            machine_record_id: "record-1".into(),
            account_origin: "https://account.example".into(),
            relay_origin: "https://relay.ferryx.dev".into(),
            enrollment_epoch: "1".into(),
            enrolled_at: 1,
        };
        write_private_json(&remote.join(ACCOUNT_ENROLLMENT_FILE), &record).expect("write record");

        assert_eq!(
            std::fs::read(&identity_path).expect("identity bytes"),
            identity_bytes,
            "enrollment must not rewrite the machine identity"
        );
        assert_eq!(
            std::fs::read(&auth_path).expect("auth file"),
            auth_before,
            "enrollment must not touch the device grant store"
        );
        let reloaded = AuthManager::with_persistence(Some(auth_path));
        assert!(
            reloaded.validate_token(&legacy_bearer).is_ok(),
            "a device paired before enrollment must still authenticate after it"
        );
        assert_eq!(reloaded.list_devices().len(), 1);
        assert!(!identity.machine_id.is_empty());
    }

    #[test]
    fn stored_machine_identity_signs_and_verifies() {
        let dir = tempfile::tempdir().expect("temp");
        let generated = load_or_generate_machine_identity(dir.path()).expect("generate");
        let reloaded = load_or_generate_machine_identity(dir.path()).expect("reload");
        assert_eq!(generated.machine_id, reloaded.machine_id, "reload must return the stored identity");
        let hash = enrollment_code_hash("probe-code");
        let signature = sign_account_enrollment(&reloaded, "http://127.0.0.1:43922", &hash, "n", 7)
            .expect("sign");
        assert!(
            verify_account_enrollment(
                &reloaded.public_key,
                &reloaded.machine_id,
                "http://127.0.0.1:43922",
                &hash,
                "n",
                7,
                &signature,
            ),
            "the key pair written by one process must verify the signature of another"
        );
    }

    #[test]
    fn account_enrollment_record_round_trips_beside_identity() {
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().join(ACCOUNT_ENROLLMENT_FILE);
        let record = AccountEnrollmentRecord {
            account_id: "acct-1".into(),
            machine_record_id: "machine-record-1".into(),
            account_origin: "https://account.example".into(),
            relay_origin: "https://relay.ferryx.dev".into(),
            enrollment_epoch: "2".into(),
            enrolled_at: 1,
        };
        write_private_json(&path, &record).expect("write");
        let round_tripped: AccountEnrollmentRecord =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("parse");
        assert_eq!(round_tripped, record);
        let identity_path = dir.path().join("identity.json");
        assert!(!identity_path.exists(), "enrollment never creates the machine identity");
    }

    #[test]
    fn remote_enrollment_errors_keep_the_http_status_and_details() {
        let limited = EnrollError::remote(
            402,
            r#"{"code":"PLAN_LIMIT_REACHED","message":"the free plan allows one computer","details":{"plan":"free","limit":1,"used":2}}"#,
        );
        assert_eq!(limited.code, "PLAN_LIMIT_REACHED");
        assert_eq!(limited.status, 402);
        assert_eq!(limited.message, "the free plan allows one computer");
        assert_eq!(
            limited.details,
            Some(serde_json::json!({"plan": "free", "limit": 1, "used": 2}))
        );

        let suspended = EnrollError::remote(
            402,
            r#"{"code":"REMOTE_SUSPENDED","message":"remote access is stopped","details":{"plan":"pro_monthly","status":"stopped","graceEndsAt":1793491200,"stoppedAt":1793491260}}"#,
        );
        assert_eq!(suspended.status, 402);
        assert_eq!(
            suspended
                .details
                .as_ref()
                .and_then(|details| details.get("graceEndsAt"))
                .and_then(serde_json::Value::as_u64),
            Some(1_793_491_200)
        );

        let offline = EnrollError::local("ACCOUNT_UNREACHABLE", "connection refused");
        assert_eq!(offline.code, "ACCOUNT_UNREACHABLE");
        assert_eq!(offline.status, 0, "locally detected failures carry no HTTP status");
        assert_eq!(offline.details, None);
    }

    #[test]
    fn machine_request_poll_classifies_every_answer() {
        assert_eq!(
            parse_machine_request_poll(200, r#"{"status":"pending"}"#),
            Ok(MachineRequestPoll::Pending)
        );
        assert_eq!(
            parse_machine_request_poll(200, r#"{"status":"denied"}"#),
            Ok(MachineRequestPoll::Denied)
        );
        assert_eq!(
            parse_machine_request_poll(200, r#"{"status":"approved","enrollmentCode":"abc"}"#),
            Ok(MachineRequestPoll::Approved {
                enrollment_code: "abc".into(),
                session_token: None,
            })
        );
        assert_eq!(
            parse_machine_request_poll(200, r#"{"status":"approved","enrollmentCode":"abc","sessionToken":"s1"}"#),
            Ok(MachineRequestPoll::Approved {
                enrollment_code: "abc".into(),
                session_token: Some("s1".into()),
            })
        );
        assert_eq!(
            parse_machine_request_poll(400, r#"{"code":"slow_down","message":"poll slower"}"#),
            Ok(MachineRequestPoll::SlowDown)
        );
        assert_eq!(
            parse_machine_request_poll(400, r#"{"error":"slow_down"}"#),
            Ok(MachineRequestPoll::SlowDown)
        );
    }

    #[test]
    fn machine_request_poll_keeps_structured_failures() {
        let expired = parse_machine_request_poll(
            400,
            r#"{"code":"MACHINE_REQUEST_EXPIRED","message":"request expired"}"#,
        )
        .expect_err("expired is an error");
        assert_eq!(expired.code, "MACHINE_REQUEST_EXPIRED");
        assert_eq!(expired.status, 400);

        let missing_code = parse_machine_request_poll(200, r#"{"status":"approved"}"#)
            .expect_err("approved without a code is malformed");
        assert_eq!(missing_code.code, "ACCOUNT_BAD_RESPONSE");

        let unknown = parse_machine_request_poll(200, r#"{"status":"weird"}"#)
            .expect_err("unknown status");
        assert_eq!(unknown.code, "ACCOUNT_BAD_RESPONSE");
    }

    #[test]
    fn machine_request_response_decodes_the_wire_shape() {
        let decoded: MachineRequestClientResponse = serde_json::from_str(
            r#"{"deviceCode":"d","userCode":"ABCD-EFGH","verificationUri":"https://a/approve","verificationUriComplete":"https://a/approve?code=ABCD-EFGH","expiresIn":900,"interval":3}"#,
        )
        .expect("decode");
        assert_eq!(decoded.user_code, "ABCD-EFGH");
        assert_eq!(decoded.verification_uri_complete, "https://a/approve?code=ABCD-EFGH");
        assert_eq!(decoded.interval, 3);
        assert_eq!(requested_display_name(Some("  ")), None);
        assert_eq!(requested_display_name(Some(" box ")), Some("box".into()));
    }
}
