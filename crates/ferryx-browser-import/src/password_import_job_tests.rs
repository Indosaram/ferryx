use crate::{
    PasswordImportCommitDestination, PasswordImportCommitError, PasswordImportInput,
    PasswordImportJob, PasswordImportJobConfig, PasswordImportJobError, PasswordImportSourceFormat,
};

const VALID_ONEPASSWORD_CSV: &[u8] = b"Title,Website,Username,Password,One-time password,Favorite status,Archived status,Tags,Notes\n\
Example,https://import.example/login,user@example.test,SYNTH-IMPORT-JOB-PASSWORD,,false,false,synthetic,Synthetic note\n";

const SECOND_VALID_ONEPASSWORD_CSV: &[u8] = b"Title,Website,Username,Password,One-time password,Favorite status,Archived status,Tags,Notes\n\
Second,https://second-import.example/login,second@example.test,SYNTH-IMPORT-JOB-PASSWORD-2,,false,false,synthetic,Synthetic note\n";

const INVALID_ONEPASSWORD_CSV: &[u8] = b"Title,Website,Username,One-time password\n\
Broken,https://broken.example,user@example.test,otpauth://totp/Broken\n";

#[derive(Default)]
struct RecordingDestination {
    entries: Vec<(String, String, String)>,
    next_error: Option<PasswordImportCommitError>,
}

impl RecordingDestination {
    fn locked() -> Self {
        Self {
            entries: Vec::new(),
            next_error: Some(PasswordImportCommitError::DestinationLocked),
        }
    }
}

impl PasswordImportCommitDestination for RecordingDestination {
    fn add_login(
        &mut self,
        credential: crate::PasswordImportCredential,
    ) -> Result<(), PasswordImportCommitError> {
        if let Some(error) = self.next_error.take() {
            return Err(error);
        }
        self.entries.push((
            credential.origin_url,
            credential.username,
            credential.password.to_string(),
        ));
        Ok(())
    }
}

fn bytes_config(bytes: &'static [u8]) -> PasswordImportJobConfig {
    PasswordImportJobConfig::new(
        PasswordImportSourceFormat::OnePasswordCsv,
        PasswordImportInput::Bytes(bytes.to_vec()),
    )
}

#[test]
fn import_preview_is_secret_free_and_does_not_write_before_confirmation() {
    // Given a Settings import job with one user-exported password file.
    let mut job = PasswordImportJob::default();
    let mut destination = RecordingDestination::default();

    // When the file is previewed but not committed.
    let receipt = job.preview(bytes_config(VALID_ONEPASSWORD_CSV)).unwrap();

    // Then preview is metadata-only and no destination write has happened.
    assert_eq!(receipt.preview.imported, 1);
    assert_eq!(receipt.preview.skipped, 0);
    let preview_json = serde_json::to_string(&receipt).unwrap();
    assert!(!preview_json.contains("SYNTH-IMPORT-JOB-PASSWORD"));
    assert!(destination.entries.is_empty());

    // When the explicit preview token is committed.
    let summary = job
        .commit(receipt.preview_token.as_str(), &mut destination)
        .unwrap();

    // Then exactly that confirmed operation writes one credential.
    assert_eq!(summary.committed, 1);
    assert_eq!(summary.failed, 0);
    assert_eq!(summary.terminal_result_count, 1);
    assert_eq!(destination.entries.len(), 1);
    assert_eq!(destination.entries[0].0, "https://import.example/login");
}

#[test]
fn import_preview_cancel_clears_pending_confirmation() {
    // Given a previewed import job.
    let mut job = PasswordImportJob::default();
    let receipt = job.preview(bytes_config(VALID_ONEPASSWORD_CSV)).unwrap();
    let mut destination = RecordingDestination::default();

    // When the user cancels before commit.
    let cancel = job.cancel();

    // Then the token no longer authorizes a write and the operation has one terminal result.
    assert_eq!(cancel.terminal_result_count, 1);
    let err = job
        .commit(receipt.preview_token.as_str(), &mut destination)
        .unwrap_err();
    assert!(matches!(err, PasswordImportJobError::NoPreview));
    assert!(destination.entries.is_empty());
}

#[test]
fn import_preview_retry_after_invalid_file_keeps_no_write_and_allows_valid_retry() {
    // Given a new job and an invalid user-selected file.
    let mut job = PasswordImportJob::default();
    let mut destination = RecordingDestination::default();

    // When preview parsing fails and the user retries with a valid file.
    let invalid = job
        .preview(bytes_config(INVALID_ONEPASSWORD_CSV))
        .unwrap_err();
    let valid = job.preview(bytes_config(VALID_ONEPASSWORD_CSV)).unwrap();

    // Then the invalid attempt did not write and the valid retry can commit.
    assert!(matches!(invalid, PasswordImportJobError::InvalidFile(_)));
    assert!(destination.entries.is_empty());
    let summary = job.commit(&valid.preview_token, &mut destination).unwrap();
    assert_eq!(summary.committed, 1);
    assert_eq!(destination.entries.len(), 1);
}

#[test]
fn import_preview_reselect_replaces_token_and_makes_old_preview_stale() {
    // Given a previewed file.
    let mut job = PasswordImportJob::default();
    let first = job.preview(bytes_config(VALID_ONEPASSWORD_CSV)).unwrap();

    // When the user reselects a different file before commit.
    let second = job
        .preview(bytes_config(SECOND_VALID_ONEPASSWORD_CSV))
        .unwrap();
    let mut destination = RecordingDestination::default();

    // Then the old token is stale and only the new preview can commit.
    assert_ne!(first.preview_token, second.preview_token);
    let stale = job
        .commit(&first.preview_token, &mut destination)
        .unwrap_err();
    assert!(matches!(stale, PasswordImportJobError::StalePreviewToken));
    assert!(destination.entries.is_empty());

    let summary = job.commit(&second.preview_token, &mut destination).unwrap();
    assert_eq!(summary.committed, 1);
    assert_eq!(
        destination.entries[0].0,
        "https://second-import.example/login"
    );
}

#[test]
fn import_preview_stale_token_fails_closed_after_successful_commit() {
    // Given a confirmed import.
    let mut job = PasswordImportJob::default();
    let receipt = job.preview(bytes_config(VALID_ONEPASSWORD_CSV)).unwrap();
    let mut destination = RecordingDestination::default();
    job.commit(&receipt.preview_token, &mut destination)
        .unwrap();

    // When the same token is reused.
    let replay = job
        .commit(&receipt.preview_token, &mut destination)
        .unwrap_err();

    // Then replay is rejected and no second write occurs.
    assert!(matches!(replay, PasswordImportJobError::NoPreview));
    assert_eq!(destination.entries.len(), 1);
}

#[test]
fn import_preview_invalid_file_reports_safe_error() {
    // Given a malformed selected export file.
    let mut job = PasswordImportJob::default();

    // When preview parses it.
    let err = job
        .preview(bytes_config(INVALID_ONEPASSWORD_CSV))
        .unwrap_err();

    // Then the error is safe and carries no raw password marker.
    let message = err.to_string();
    assert!(matches!(err, PasswordImportJobError::InvalidFile(_)));
    assert!(message.contains("missing required"));
    assert!(!message.contains("SYNTH-"));
}

#[test]
fn import_preview_destination_locked_at_commit_time_writes_nothing() {
    // Given a preview that was generated while no write was attempted.
    let mut job = PasswordImportJob::default();
    let receipt = job.preview(bytes_config(VALID_ONEPASSWORD_CSV)).unwrap();
    let mut destination = RecordingDestination::locked();

    // When the destination is locked at commit time.
    let err = job
        .commit(&receipt.preview_token, &mut destination)
        .unwrap_err();

    // Then commit fails closed before any credential is written.
    assert!(matches!(err, PasswordImportJobError::DestinationLocked));
    assert!(destination.entries.is_empty());
}
