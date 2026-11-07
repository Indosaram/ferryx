mod bitwarden_json;
mod csv_sources;
mod onepux;

use std::collections::{BTreeSet, HashSet};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::ImportResult;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PasswordImportSource {
    OnePassword,
    Bitwarden,
    ApplePasswords,
    KeePassXc,
    KeePassClassic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PasswordImportFileFormat {
    Csv,
    OnePux,
    Json,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PasswordImportSourceFormat {
    OnePasswordCsv,
    OnePasswordPux,
    BitwardenIndividualCsv,
    BitwardenOrganizationCsv,
    BitwardenJson,
    ApplePasswordsCsv,
    KeePassXcCsv,
    KeePassClassicCsv,
}

impl PasswordImportSourceFormat {
    pub const fn source(self) -> PasswordImportSource {
        match self {
            Self::OnePasswordCsv | Self::OnePasswordPux => PasswordImportSource::OnePassword,
            Self::BitwardenIndividualCsv | Self::BitwardenOrganizationCsv | Self::BitwardenJson => {
                PasswordImportSource::Bitwarden
            }
            Self::ApplePasswordsCsv => PasswordImportSource::ApplePasswords,
            Self::KeePassXcCsv => PasswordImportSource::KeePassXc,
            Self::KeePassClassicCsv => PasswordImportSource::KeePassClassic,
        }
    }

    pub const fn file_format(self) -> PasswordImportFileFormat {
        match self {
            Self::OnePasswordCsv
            | Self::BitwardenIndividualCsv
            | Self::BitwardenOrganizationCsv
            | Self::ApplePasswordsCsv
            | Self::KeePassXcCsv
            | Self::KeePassClassicCsv => PasswordImportFileFormat::Csv,
            Self::OnePasswordPux => PasswordImportFileFormat::OnePux,
            Self::BitwardenJson => PasswordImportFileFormat::Json,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::OnePasswordCsv => "1Password CSV",
            Self::OnePasswordPux => "1PUX",
            Self::BitwardenIndividualCsv => "Bitwarden individual CSV",
            Self::BitwardenOrganizationCsv => "Bitwarden organization CSV",
            Self::BitwardenJson => "Bitwarden JSON",
            Self::ApplePasswordsCsv => "Apple Passwords CSV",
            Self::KeePassXcCsv => "KeePassXC CSV",
            Self::KeePassClassicCsv => "KeePass classic CSV",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PasswordImportPreview {
    pub imported: usize,
    pub skipped: usize,
    pub duplicates: usize,
    pub blank_passwords: usize,
    pub unsupported_fields: usize,
    pub safe_messages: Vec<String>,
    pub safe_errors: Vec<String>,
}

#[derive(Clone)]
pub struct PasswordImportCredential {
    pub origin_url: String,
    pub username: String,
    pub password: Zeroizing<String>,
}

pub struct PasswordImportParsedFile {
    pub preview: PasswordImportPreview,
    pub credentials: Vec<PasswordImportCredential>,
}

impl PasswordImportPreview {
    pub fn surface_text(&self) -> String {
        [self.safe_messages.as_slice(), self.safe_errors.as_slice()]
            .concat()
            .join("\n")
    }
}

pub fn parse_password_import_source_preview(
    source_format: PasswordImportSourceFormat,
    contents: &[u8],
) -> ImportResult<PasswordImportPreview> {
    parse_password_import_source_file(source_format, contents).map(|parsed| parsed.preview)
}

pub fn parse_password_import_source_file(
    source_format: PasswordImportSourceFormat,
    contents: &[u8],
) -> ImportResult<PasswordImportParsedFile> {
    match source_format {
        PasswordImportSourceFormat::OnePasswordCsv => csv_sources::parse_onepassword_csv(contents),
        PasswordImportSourceFormat::OnePasswordPux => onepux::parse_onepux(contents),
        PasswordImportSourceFormat::BitwardenIndividualCsv => {
            csv_sources::parse_bitwarden_individual_csv(contents)
        }
        PasswordImportSourceFormat::BitwardenOrganizationCsv => {
            csv_sources::parse_bitwarden_organization_csv(contents)
        }
        PasswordImportSourceFormat::BitwardenJson => bitwarden_json::parse_bitwarden_json(contents),
        PasswordImportSourceFormat::ApplePasswordsCsv => csv_sources::parse_apple_csv(contents),
        PasswordImportSourceFormat::KeePassXcCsv => csv_sources::parse_keepassxc_csv(contents),
        PasswordImportSourceFormat::KeePassClassicCsv => {
            csv_sources::parse_keepass_classic_csv(contents)
        }
    }
}

pub(super) struct CredentialCandidate {
    pub origin_url: String,
    pub username: String,
    pub password: String,
}

pub(super) struct PreviewBuilder {
    preview: PasswordImportPreview,
    credentials: Vec<PasswordImportCredential>,
    seen_credentials: HashSet<(String, String)>,
    unsupported_fields: BTreeSet<&'static str>,
}

impl PreviewBuilder {
    pub(super) fn new() -> Self {
        Self {
            preview: PasswordImportPreview::default(),
            credentials: Vec::new(),
            seen_credentials: HashSet::new(),
            unsupported_fields: BTreeSet::new(),
        }
    }

    pub(super) fn import_credential(&mut self, candidate: CredentialCandidate) {
        if candidate.password.is_empty() {
            self.preview.skipped += 1;
            self.preview.blank_passwords += 1;
            self.preview
                .safe_messages
                .push("skipped credential row with blank password".into());
            return;
        }

        let key = (candidate.origin_url.clone(), candidate.username.clone());
        if !self.seen_credentials.insert(key) {
            self.preview.duplicates += 1;
            self.preview
                .safe_messages
                .push("duplicate credential candidate detected by origin and username".into());
        }
        self.preview.imported += 1;
        self.credentials.push(PasswordImportCredential {
            origin_url: candidate.origin_url,
            username: candidate.username,
            password: Zeroizing::new(candidate.password),
        });
    }

    pub(super) fn record_unsupported_field(&mut self, label: &'static str) {
        if self.unsupported_fields.insert(label) {
            self.preview
                .safe_messages
                .push(format!("skipped unsupported field: {label}"));
        }
    }

    pub(super) fn skip_unsupported_item(&mut self, label: &'static str) {
        self.preview.skipped += 1;
        self.record_unsupported_field(label);
        self.preview
            .safe_messages
            .push(format!("skipped unsupported item kind: {label}"));
    }

    pub(super) fn finish(mut self) -> PasswordImportParsedFile {
        self.preview.unsupported_fields = self.unsupported_fields.len();
        PasswordImportParsedFile {
            preview: self.preview,
            credentials: self.credentials,
        }
    }
}
