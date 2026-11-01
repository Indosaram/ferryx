use std::path::PathBuf;

use serde::Serialize;

use crate::{
    parse_password_import_source_file, ImportError, PasswordImportCredential,
    PasswordImportFileFormat, PasswordImportParsedFile, PasswordImportPreview,
    PasswordImportSource, PasswordImportSourceFormat,
};

#[derive(Clone, Debug)]
pub enum PasswordImportInput {
    Path(PathBuf),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug)]
pub struct PasswordImportJobConfig {
    pub source_format: PasswordImportSourceFormat,
    pub input: PasswordImportInput,
}

impl PasswordImportJobConfig {
    pub const fn new(
        source_format: PasswordImportSourceFormat,
        input: PasswordImportInput,
    ) -> Self {
        Self {
            source_format,
            input,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordImportPreviewReceipt {
    pub preview_token: String,
    pub source: PasswordImportSource,
    pub file_format: PasswordImportFileFormat,
    pub source_format: PasswordImportSourceFormat,
    pub source_label: &'static str,
    pub preview: PasswordImportPreview,
    pub terminal_result_count: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordImportCommitSummary {
    pub committed: usize,
    pub failed: usize,
    pub terminal_result_count: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordImportCancelSummary {
    pub terminal_result_count: u8,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PasswordImportCommitError {
    #[error("destination is locked")]
    DestinationLocked,
    #[error("destination rejected credential: {0}")]
    DestinationRejected(String),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PasswordImportJobError {
    #[error("invalid import file: {0}")]
    InvalidFile(String),
    #[error("no password import preview is pending")]
    NoPreview,
    #[error("preview token is stale")]
    StalePreviewToken,
    #[error("destination is locked")]
    DestinationLocked,
    #[error("commit failed: {0}")]
    CommitFailed(String),
}

pub trait PasswordImportCommitDestination {
    fn add_login(
        &mut self,
        credential: PasswordImportCredential,
    ) -> Result<(), PasswordImportCommitError>;
}

#[derive(Default)]
pub struct PasswordImportJob {
    active: Option<ActivePasswordImportPreview>,
    next_token: u64,
}

struct ActivePasswordImportPreview {
    token: String,
    config: PasswordImportJobConfig,
    parsed: PasswordImportParsedFile,
}

impl PasswordImportJob {
    pub fn preview(
        &mut self,
        config: PasswordImportJobConfig,
    ) -> Result<PasswordImportPreviewReceipt, PasswordImportJobError> {
        let bytes = read_import_input(&config.input)?;
        let parsed = parse_password_import_source_file(config.source_format, &bytes)
            .map_err(import_error_to_job_error)?;
        let token = self.next_preview_token();
        let receipt = PasswordImportPreviewReceipt {
            preview_token: token.clone(),
            source: config.source_format.source(),
            file_format: config.source_format.file_format(),
            source_format: config.source_format,
            source_label: config.source_format.label(),
            preview: parsed.preview.clone(),
            terminal_result_count: 1,
        };
        self.active = Some(ActivePasswordImportPreview {
            token,
            config,
            parsed,
        });
        Ok(receipt)
    }

    pub fn cancel(&mut self) -> PasswordImportCancelSummary {
        self.active = None;
        PasswordImportCancelSummary {
            terminal_result_count: 1,
        }
    }

    pub fn commit<D>(
        &mut self,
        preview_token: &str,
        destination: &mut D,
    ) -> Result<PasswordImportCommitSummary, PasswordImportJobError>
    where
        D: PasswordImportCommitDestination,
    {
        let Some(active) = self.active.take() else {
            return Err(PasswordImportJobError::NoPreview);
        };
        if active.token != preview_token {
            self.active = Some(active);
            return Err(PasswordImportJobError::StalePreviewToken);
        }

        let mut committed = 0usize;
        let mut failed = 0usize;
        for credential in active.parsed.credentials {
            match destination.add_login(credential) {
                Ok(()) => committed += 1,
                Err(PasswordImportCommitError::DestinationLocked) => {
                    return Err(PasswordImportJobError::DestinationLocked);
                }
                Err(PasswordImportCommitError::DestinationRejected(message)) => {
                    failed += 1;
                    if message.is_empty() {
                        return Err(PasswordImportJobError::CommitFailed(
                            "destination rejected credential".to_string(),
                        ));
                    }
                }
            }
        }

        let _ = active.config;
        Ok(PasswordImportCommitSummary {
            committed,
            failed,
            terminal_result_count: 1,
        })
    }

    fn next_preview_token(&mut self) -> String {
        self.next_token = self.next_token.saturating_add(1);
        format!("password-import-preview-{}", self.next_token)
    }
}

fn read_import_input(input: &PasswordImportInput) -> Result<Vec<u8>, PasswordImportJobError> {
    match input {
        PasswordImportInput::Path(path) => std::fs::read(path)
            .map_err(|error| PasswordImportJobError::InvalidFile(error.to_string())),
        PasswordImportInput::Bytes(bytes) => Ok(bytes.clone()),
    }
}

fn import_error_to_job_error(error: ImportError) -> PasswordImportJobError {
    PasswordImportJobError::InvalidFile(error.to_string())
}
