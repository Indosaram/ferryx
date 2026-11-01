use serde::Deserialize;

use crate::{ImportError, ImportResult};

use super::{CredentialCandidate, PasswordImportParsedFile, PreviewBuilder};

#[derive(Deserialize)]
struct BitwardenExport {
    #[serde(default)]
    encrypted: bool,
    #[serde(default)]
    items: Vec<BitwardenItem>,
}

#[derive(Deserialize)]
struct BitwardenItem {
    #[serde(default, rename = "type")]
    item_type: Option<u8>,
    #[serde(default)]
    login: Option<BitwardenLogin>,
    #[serde(default)]
    fields: Vec<BitwardenField>,
    #[serde(default)]
    card: Option<BitwardenCard>,
}

#[derive(Deserialize)]
struct BitwardenLogin {
    #[serde(default)]
    uris: Vec<BitwardenUri>,
    #[serde(default)]
    username: String,
    password: Option<String>,
}

#[derive(Deserialize)]
struct BitwardenUri {
    #[serde(default)]
    uri: String,
}

#[derive(Deserialize)]
struct BitwardenField {}

#[derive(Deserialize)]
struct BitwardenCard {}

pub(super) fn parse_bitwarden_json(contents: &[u8]) -> ImportResult<PasswordImportParsedFile> {
    let export: BitwardenExport = serde_json::from_slice(contents)
        .map_err(|_| ImportError::Parse("malformed Bitwarden JSON export".into()))?;

    if export.encrypted {
        return Err(ImportError::Parse(
            "unsupported encrypted Bitwarden JSON export".into(),
        ));
    }

    let mut builder = PreviewBuilder::new();
    for item in export.items {
        if !item.fields.is_empty() {
            builder.record_unsupported_field("fields");
        }
        if item.card.is_some() {
            builder.skip_unsupported_item("card item");
            continue;
        }

        match item.item_type {
            Some(1) => import_login_item(item.login, &mut builder)?,
            Some(_) | std::option::Option::None => builder.skip_unsupported_item("non-login item"),
        }
    }

    Ok(builder.finish())
}

fn import_login_item(
    login: Option<BitwardenLogin>,
    builder: &mut PreviewBuilder,
) -> ImportResult<()> {
    let Some(login) = login else {
        return Err(ImportError::Parse(
            "missing required Bitwarden JSON login object".into(),
        ));
    };
    let Some(password) = login.password else {
        return Err(ImportError::Parse(
            "missing required Bitwarden JSON login.password".into(),
        ));
    };
    let origin_url = login
        .uris
        .iter()
        .find_map(|uri| {
            let trimmed = uri.uri.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .unwrap_or_default();

    builder.import_credential(CredentialCandidate {
        origin_url,
        username: login.username.trim().to_string(),
        password,
    });
    Ok(())
}
