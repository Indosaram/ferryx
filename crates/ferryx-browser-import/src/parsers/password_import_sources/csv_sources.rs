use crate::parsers::csv::parse_csv_records;
use crate::{ImportError, ImportResult};

use super::{CredentialCandidate, PasswordImportParsedFile, PreviewBuilder};

struct CsvRequiredColumns {
    origin_url: &'static str,
    username: &'static str,
    password: &'static str,
}

struct CsvSourceSpec {
    label: &'static str,
    required: CsvRequiredColumns,
    unsupported_headers: &'static [&'static str],
    type_column: Option<&'static str>,
    supported_type: Option<&'static str>,
}

struct CsvColumns {
    origin_url: usize,
    username: usize,
    password: usize,
    type_column: Option<usize>,
}

pub(super) fn parse_onepassword_csv(contents: &[u8]) -> ImportResult<PasswordImportParsedFile> {
    parse_csv_source(
        contents,
        CsvSourceSpec {
            label: "1Password CSV",
            required: CsvRequiredColumns {
                origin_url: "Website",
                username: "Username",
                password: "Password",
            },
            unsupported_headers: &["Favorite status", "Archived status", "Tags", "Notes"],
            type_column: None,
            supported_type: None,
        },
    )
}

pub(super) fn parse_bitwarden_individual_csv(
    contents: &[u8],
) -> ImportResult<PasswordImportParsedFile> {
    parse_csv_source(
        contents,
        CsvSourceSpec {
            label: "Bitwarden individual CSV",
            required: CsvRequiredColumns {
                origin_url: "login_uri",
                username: "login_username",
                password: "login_password",
            },
            unsupported_headers: &["fields"],
            type_column: Some("type"),
            supported_type: Some("login"),
        },
    )
}

pub(super) fn parse_bitwarden_organization_csv(
    contents: &[u8],
) -> ImportResult<PasswordImportParsedFile> {
    parse_csv_source(
        contents,
        CsvSourceSpec {
            label: "Bitwarden organization CSV",
            required: CsvRequiredColumns {
                origin_url: "login_uri",
                username: "login_username",
                password: "login_password",
            },
            unsupported_headers: &["fields"],
            type_column: Some("type"),
            supported_type: Some("login"),
        },
    )
}

pub(super) fn parse_apple_csv(contents: &[u8]) -> ImportResult<PasswordImportParsedFile> {
    parse_csv_source(
        contents,
        CsvSourceSpec {
            label: "Apple Passwords CSV",
            required: CsvRequiredColumns {
                origin_url: "URL",
                username: "Username",
                password: "Password",
            },
            unsupported_headers: &["Notes"],
            type_column: None,
            supported_type: None,
        },
    )
}

pub(super) fn parse_keepassxc_csv(contents: &[u8]) -> ImportResult<PasswordImportParsedFile> {
    parse_csv_source(
        contents,
        CsvSourceSpec {
            label: "KeePassXC CSV",
            required: CsvRequiredColumns {
                origin_url: "URL",
                username: "Username",
                password: "Password",
            },
            unsupported_headers: &["Notes"],
            type_column: None,
            supported_type: None,
        },
    )
}

pub(super) fn parse_keepass_classic_csv(contents: &[u8]) -> ImportResult<PasswordImportParsedFile> {
    parse_csv_source(
        contents,
        CsvSourceSpec {
            label: "KeePass classic CSV",
            required: CsvRequiredColumns {
                origin_url: "Web Site",
                username: "Login Name",
                password: "Password",
            },
            unsupported_headers: &["Comments"],
            type_column: None,
            supported_type: None,
        },
    )
}

fn parse_csv_source(
    contents: &[u8],
    spec: CsvSourceSpec,
) -> ImportResult<PasswordImportParsedFile> {
    let content = std::str::from_utf8(contents)
        .map_err(|_| ImportError::Parse(format!("malformed {}: input is not UTF-8", spec.label)))?;
    let records = parse_csv_records(content)?;
    let Some((header, rows)) = records.split_first() else {
        return Err(ImportError::Parse(format!("{} is empty", spec.label)));
    };

    let columns = CsvColumns::from_header(header, &spec)?;
    let mut builder = PreviewBuilder::new();

    for header_name in spec.unsupported_headers {
        if find_header(header, header_name).is_some() {
            builder.record_unsupported_field(header_name);
        }
    }

    for row in rows {
        if row.iter().all(|field| field.is_empty()) {
            continue;
        }
        if let Some(type_column) = columns.type_column {
            let supported_type = spec.supported_type.unwrap_or("");
            if !field(row, type_column).trim().eq_ignore_ascii_case(supported_type) {
                builder.skip_unsupported_item("non-login CSV item");
                continue;
            }
        }

        builder.import_credential(CredentialCandidate {
            origin_url: field(row, columns.origin_url).trim().to_string(),
            username: field(row, columns.username).to_string(),
            password: field(row, columns.password).to_string(),
        });
    }

    Ok(builder.finish())
}

impl CsvColumns {
    fn from_header(header: &[String], spec: &CsvSourceSpec) -> ImportResult<Self> {
        let Some(origin_url) = find_header(header, spec.required.origin_url) else {
            return Err(missing_required(spec.label, spec.required.origin_url));
        };
        let Some(username) = find_header(header, spec.required.username) else {
            return Err(missing_required(spec.label, spec.required.username));
        };
        let Some(password) = find_header(header, spec.required.password) else {
            return Err(missing_required(spec.label, spec.required.password));
        };

        Ok(Self {
            origin_url,
            username,
            password,
            type_column: spec.type_column.and_then(|name| find_header(header, name)),
        })
    }
}

fn find_header(header: &[String], name: &str) -> Option<usize> {
    header.iter().position(|header_name| {
        header_name
            .trim_start_matches('\u{feff}')
            .trim()
            .eq_ignore_ascii_case(name)
    })
}

fn field(row: &[String], idx: usize) -> &str {
    row.get(idx).map(String::as_str).unwrap_or("")
}

fn missing_required(label: &str, column: &str) -> ImportError {
    ImportError::Parse(format!("missing required {label} column: {column}"))
}

#[cfg(test)]
mod review_tests {
    use super::*;

    #[test]
    fn review_csv_credentials_preserve_significant_whitespace() {
        for (parser, header) in [
            (parse_onepassword_csv as fn(&[u8]) -> ImportResult<PasswordImportParsedFile>, "Website,Username,Password"),
            (parse_apple_csv, "URL,Username,Password"),
            (parse_keepassxc_csv, "URL,Username,Password"),
            (parse_keepass_classic_csv, "Web Site,Login Name,Password"),
            (parse_bitwarden_individual_csv, "login_uri,login_username,login_password"),
            (parse_bitwarden_organization_csv, "login_uri,login_username,login_password"),
        ] {
            let input = format!("{header}\nhttps://example.com,\" user \",\" secret \"\nhttps://example.org,user,\"   \"\n");
            let parsed = parser(input.as_bytes()).unwrap();
            assert_eq!(parsed.credentials.len(), 2, "whitespace-only password must survive");
            assert_eq!(parsed.credentials[0].username, " user ");
            assert_eq!(parsed.credentials[0].password.as_str(), " secret ");
            assert_eq!(parsed.credentials[1].password.as_str(), "   ");
        }
    }
}
