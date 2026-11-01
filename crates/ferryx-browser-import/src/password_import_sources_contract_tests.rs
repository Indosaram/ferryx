//! Red contract tests for user-exported password-manager import sources.
//!
use base64::{engine::general_purpose::STANDARD, Engine as _};

use crate::{parse_password_import_source_preview, PasswordImportSourceFormat as SourceFormat};

enum FixturePayload {
    Text(&'static str),
    Base64Zip(&'static str),
    Bytes(&'static [u8]),
}

impl FixturePayload {
    fn materialize(&self) -> Vec<u8> {
        match self {
            Self::Text(value) => value.as_bytes().to_vec(),
            Self::Base64Zip(value) => STANDARD.decode(value.trim()).unwrap_or_else(|err| {
                panic!("1PUX base64 fixture must decode into a zip archive: {err}")
            }),
            Self::Bytes(value) => value.to_vec(),
        }
    }
}

#[derive(Clone, Copy)]
struct ExpectedPreview {
    imported: usize,
    skipped: usize,
    duplicates: usize,
    blank_passwords: usize,
    unsupported_fields: usize,
}

struct ValidCase {
    name: &'static str,
    source: SourceFormat,
    payload: FixturePayload,
    expected: ExpectedPreview,
}

struct InvalidCase {
    name: &'static str,
    source: SourceFormat,
    payload: FixturePayload,
    expected_fragment: &'static str,
}

fn valid_cases() -> Vec<ValidCase> {
    vec![
        ValidCase { name: "1password_csv_valid_mixed", source: SourceFormat::OnePasswordCsv, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/onepassword/valid_mixed.csv")), expected: ExpectedPreview { imported: 2, skipped: 1, duplicates: 1, blank_passwords: 1, unsupported_fields: 4 } },
        ValidCase { name: "1password_1pux_valid_container", source: SourceFormat::OnePasswordPux, payload: FixturePayload::Base64Zip(include_str!("../test_data/fixtures/password_import_sources/onepassword/valid.1pux.base64")), expected: ExpectedPreview { imported: 2, skipped: 2, duplicates: 1, blank_passwords: 1, unsupported_fields: 2 } },
        ValidCase { name: "bitwarden_individual_csv_valid_mixed", source: SourceFormat::BitwardenIndividualCsv, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/bitwarden/individual_valid_mixed.csv")), expected: ExpectedPreview { imported: 2, skipped: 1, duplicates: 1, blank_passwords: 1, unsupported_fields: 1 } },
        ValidCase { name: "bitwarden_organization_csv_valid_mixed", source: SourceFormat::BitwardenOrganizationCsv, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/bitwarden/organization_valid_mixed.csv")), expected: ExpectedPreview { imported: 2, skipped: 1, duplicates: 1, blank_passwords: 1, unsupported_fields: 1 } },
        ValidCase { name: "bitwarden_json_valid_mixed", source: SourceFormat::BitwardenJson, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/bitwarden/valid_mixed.json")), expected: ExpectedPreview { imported: 2, skipped: 2, duplicates: 1, blank_passwords: 1, unsupported_fields: 2 } },
        ValidCase { name: "apple_passwords_csv_valid_mixed", source: SourceFormat::ApplePasswordsCsv, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/apple/valid_mixed.csv")), expected: ExpectedPreview { imported: 2, skipped: 1, duplicates: 1, blank_passwords: 1, unsupported_fields: 1 } },
        ValidCase { name: "keepassxc_csv_valid_mixed", source: SourceFormat::KeePassXcCsv, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/keepass/keepassxc_valid_mixed.csv")), expected: ExpectedPreview { imported: 2, skipped: 1, duplicates: 1, blank_passwords: 1, unsupported_fields: 1 } },
        ValidCase { name: "keepass_classic_csv_valid_mixed", source: SourceFormat::KeePassClassicCsv, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/keepass/keepass_classic_valid_mixed.csv")), expected: ExpectedPreview { imported: 2, skipped: 1, duplicates: 1, blank_passwords: 1, unsupported_fields: 1 } },
    ]
}

fn invalid_cases() -> Vec<InvalidCase> {
    vec![
        InvalidCase { name: "1password_csv_missing_password", source: SourceFormat::OnePasswordCsv, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/onepassword/missing_required_columns.csv")), expected_fragment: "missing required" },
        InvalidCase { name: "1password_csv_malformed_quote", source: SourceFormat::OnePasswordCsv, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/onepassword/malformed_unclosed_quote.csv")), expected_fragment: "malformed" },
        InvalidCase { name: "1password_1pux_corrupt_archive", source: SourceFormat::OnePasswordPux, payload: FixturePayload::Bytes(b"not a zip archive; synthetic corrupt 1PUX fixture"), expected_fragment: "corrupt 1pux" },
        InvalidCase { name: "bitwarden_csv_missing_password", source: SourceFormat::BitwardenIndividualCsv, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/bitwarden/individual_missing_required_columns.csv")), expected_fragment: "missing required" },
        InvalidCase { name: "bitwarden_csv_malformed_quote", source: SourceFormat::BitwardenIndividualCsv, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/bitwarden/individual_malformed_unclosed_quote.csv")), expected_fragment: "malformed" },
        InvalidCase { name: "bitwarden_org_csv_missing_password", source: SourceFormat::BitwardenOrganizationCsv, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/bitwarden/organization_missing_required_columns.csv")), expected_fragment: "missing required" },
        InvalidCase { name: "bitwarden_org_csv_malformed_quote", source: SourceFormat::BitwardenOrganizationCsv, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/bitwarden/organization_malformed_unclosed_quote.csv")), expected_fragment: "malformed" },
        InvalidCase { name: "bitwarden_json_missing_password", source: SourceFormat::BitwardenJson, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/bitwarden/missing_required_password.json")), expected_fragment: "missing required" },
        InvalidCase { name: "bitwarden_json_malformed", source: SourceFormat::BitwardenJson, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/bitwarden/malformed_json.txt")), expected_fragment: "malformed" },
        InvalidCase { name: "apple_csv_missing_password", source: SourceFormat::ApplePasswordsCsv, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/apple/missing_required_columns.csv")), expected_fragment: "missing required" },
        InvalidCase { name: "apple_csv_malformed_quote", source: SourceFormat::ApplePasswordsCsv, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/apple/malformed_unclosed_quote.csv")), expected_fragment: "malformed" },
        InvalidCase { name: "keepassxc_csv_missing_password", source: SourceFormat::KeePassXcCsv, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/keepass/keepassxc_missing_required_columns.csv")), expected_fragment: "missing required" },
        InvalidCase { name: "keepassxc_csv_malformed_quote", source: SourceFormat::KeePassXcCsv, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/keepass/keepassxc_malformed_unclosed_quote.csv")), expected_fragment: "malformed" },
        InvalidCase { name: "keepass_classic_csv_missing_password", source: SourceFormat::KeePassClassicCsv, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/keepass/keepass_classic_missing_required_columns.csv")), expected_fragment: "missing required" },
        InvalidCase { name: "keepass_classic_csv_malformed_quote", source: SourceFormat::KeePassClassicCsv, payload: FixturePayload::Text(include_str!("../test_data/fixtures/password_import_sources/keepass/keepass_classic_malformed_unclosed_quote.csv")), expected_fragment: "malformed" },
    ]
}

fn assert_secret_free(surface: &str) {
    for marker in ["SYNTH-", "SYNTHOTP"] {
        assert!(
            !surface.contains(marker),
            "preview/error text leaked a synthetic password marker"
        );
    }
}

#[test]
fn password_import_sources_valid_rows_from_public_specs_are_previewed() {
    let mut failures = Vec::new();
    for case in valid_cases() {
        let bytes = case.payload.materialize();
        match parse_password_import_source_preview(case.source, &bytes) {
            Ok(preview) => {
                assert_secret_free(&preview.surface_text());
                if preview.imported != case.expected.imported {
                    failures.push(format!("{} imported count mismatch", case.name));
                }
            }
            Err(err) => failures.push(format!(
                "{} ({}) failed before preview: {err}",
                case.name,
                case.source.label()
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "source-specific parser dispatch/implementation is missing or incomplete:\n{}",
        failures.join("\n")
    );
}

#[test]
fn password_import_sources_report_skips_duplicates_and_blank_passwords() {
    let mut failures = Vec::new();
    for case in valid_cases() {
        let bytes = case.payload.materialize();
        match parse_password_import_source_preview(case.source, &bytes) {
            Ok(preview) => {
                let expected = case.expected;
                let got = (
                    preview.skipped,
                    preview.duplicates,
                    preview.blank_passwords,
                    preview.unsupported_fields,
                );
                let want = (
                    expected.skipped,
                    expected.duplicates,
                    expected.blank_passwords,
                    expected.unsupported_fields,
                );
                if got != want {
                    failures.push(format!("{} skip/duplicate/blank/unsupported counts mismatch: got {got:?}, want {want:?}", case.name));
                }
            }
            Err(err) => failures.push(format!(
                "{} ({}) failed before skip accounting: {err}",
                case.name,
                case.source.label()
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "source-specific preview accounting is missing or incomplete:\n{}",
        failures.join("\n")
    );
}

#[test]
fn password_import_sources_reject_malformed_and_missing_required_inputs() {
    let mut failures = Vec::new();
    for case in invalid_cases() {
        let bytes = case.payload.materialize();
        match parse_password_import_source_preview(case.source, &bytes) {
            Ok(_) => failures.push(format!(
                "{} ({}) unexpectedly previewed",
                case.name,
                case.source.label()
            )),
            Err(err) => {
                let message = err.to_string().to_ascii_lowercase();
                assert_secret_free(&message);
                if !message.contains(case.expected_fragment) {
                    failures.push(format!(
                        "{} expected source-specific error containing {:?}, got {message:?}",
                        case.name, case.expected_fragment
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "source-specific invalid-input errors are missing or incomplete:\n{}",
        failures.join("\n")
    );
}

#[test]
fn password_import_sources_preview_and_error_text_do_not_leak_raw_passwords() {
    for case in valid_cases() {
        let bytes = case.payload.materialize();
        let surface = parse_password_import_source_preview(case.source, &bytes)
            .map(|preview| preview.surface_text())
            .unwrap_or_else(|err| err.to_string());
        assert_secret_free(&surface);
    }
    for case in invalid_cases() {
        let bytes = case.payload.materialize();
        let surface = parse_password_import_source_preview(case.source, &bytes)
            .map(|preview| preview.surface_text())
            .unwrap_or_else(|err| err.to_string());
        assert_secret_free(&surface);
    }
}
