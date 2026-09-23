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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrollError {
    pub code: String,
    pub message: String,
}

impl EnrollError {
    fn local(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
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
        Self { code, message }
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
) -> Result<AccountEnrollmentRecord, EnrollError> {
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
        return Err(EnrollError::remote(status, &body));
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
        display_name: identity.display_name.clone(),
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
        return Err(EnrollError::remote(status, &body));
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

    Ok(record)
}

pub async fn request_machine_grant(
    account_origin: &str,
    session_bearer: &str,
    enrollment_epoch: &str,
    device_label: &str,
    installation_id: &str,
    grant_scope: AccountGrantScope,
) -> Result<AccountGrantResponse, EnrollError> {
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
        return Err(EnrollError::remote(status, &body));
    }
    serde_json::from_str(&body)
        .map_err(|error| EnrollError::local("ACCOUNT_BAD_RESPONSE", error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::auth::{enrollment_code_hash, verify_account_enrollment};

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
            relay_origin: "https://relay.checka.cc".into(),
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
}
