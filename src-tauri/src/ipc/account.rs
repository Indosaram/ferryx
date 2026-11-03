use serde::Serialize;

use super::error::{IpcError, IpcErrorCode};
use super::run_blocking;
use crate::account::enroll_client::{self, AccountEnrollmentRecord};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountEnrollmentStatus {
    pub enrolled: bool,
    pub account_origin: Option<String>,
    pub enrolled_at: Option<u64>,
}

impl From<Option<AccountEnrollmentRecord>> for AccountEnrollmentStatus {
    fn from(record: Option<AccountEnrollmentRecord>) -> Self {
        match record {
            Some(record) => Self {
                enrolled: true,
                account_origin: Some(record.account_origin),
                enrolled_at: Some(record.enrolled_at),
            },
            None => Self {
                enrolled: false,
                account_origin: None,
                enrolled_at: None,
            },
        }
    }
}

#[tauri::command]
pub async fn cmd_account_enrollment_status() -> Result<AccountEnrollmentStatus, IpcError> {
    run_blocking(|| Ok(enroll_client::load_enrollment_record().into())).await
}

#[tauri::command]
pub async fn cmd_account_enroll_this_machine(
    origin: String,
    enrollment_code: String,
    session_token: Option<String>,
) -> Result<AccountEnrollmentStatus, IpcError> {
    let origin = crate::account::origin::normalize_account_origin(&origin)
        .map_err(|error| IpcError::new(IpcErrorCode::InvalidArgument, error.to_string()))?;
    let code = enrollment_code.trim().to_string();
    if code.is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidArgument,
            "enrollment code is required",
        ));
    }
    // enroll_machine does sync identity-file I/O, so it must not run on a reactor thread.
    let handle = tokio::runtime::Handle::current();
    run_blocking(move || {
        let record = handle.block_on(enroll_client::enroll_machine(&origin, &code));
        // The signed-in session lets the daemon pair to the supervisor lease holder by itself.
        if let (Ok(_), Some(session)) = (&record, session_token.as_deref()) {
            if let Err(error) = enroll_client::save_account_session(&origin, session) {
                tracing::warn!(%error, "the account session was not saved");
            }
        }
        record
            .map(|record| Some(record).into())
            .map_err(|error| {
                IpcError::new(IpcErrorCode::from_code_str(&error.code), error.message)
            })
    })
    .await
}
