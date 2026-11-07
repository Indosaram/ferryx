use serde::Deserialize;

use crate::{ImportError, ImportResult};

use super::{CredentialCandidate, PasswordImportParsedFile, PreviewBuilder};

const LOCAL_FILE_HEADER_SIGNATURE: &[u8; 4] = b"PK\x03\x04";
const CENTRAL_DIRECTORY_SIGNATURE: &[u8; 4] = b"PK\x01\x02";
const END_OF_CENTRAL_DIRECTORY_SIGNATURE: &[u8; 4] = b"PK\x05\x06";
const LOCAL_FILE_HEADER_LEN: usize = 30;

#[derive(Deserialize)]
struct OnePuxExport {
    #[serde(default)]
    accounts: Vec<OnePuxAccount>,
}

#[derive(Deserialize)]
struct OnePuxAccount {
    #[serde(default)]
    vaults: Vec<OnePuxVault>,
}

#[derive(Deserialize)]
struct OnePuxVault {
    #[serde(default)]
    items: Vec<OnePuxItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OnePuxItem {
    #[serde(default)]
    category_uuid: String,
    #[serde(default)]
    overview: OnePuxOverview,
    #[serde(default)]
    details: OnePuxDetails,
}

#[derive(Default, Deserialize)]
struct OnePuxOverview {
    #[serde(default)]
    url: String,
    #[serde(default)]
    urls: Vec<OnePuxUrl>,
}

#[derive(Deserialize)]
struct OnePuxUrl {
    #[serde(default)]
    url: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OnePuxDetails {
    #[serde(default)]
    login_fields: Vec<OnePuxLoginField>,
    #[serde(default)]
    sections: Vec<OnePuxSection>,
}

#[derive(Deserialize)]
struct OnePuxLoginField {
    #[serde(default)]
    designation: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    value: String,
}

#[derive(Deserialize)]
struct OnePuxSection {}

pub(super) fn parse_onepux(contents: &[u8]) -> ImportResult<PasswordImportParsedFile> {
    let export_data = read_export_data(contents)?;
    let export: OnePuxExport = serde_json::from_slice(&export_data)
        .map_err(|_| ImportError::Parse("malformed 1pux export.data JSON".into()))?;

    let mut builder = PreviewBuilder::new();
    for account in export.accounts {
        for vault in account.vaults {
            for item in vault.items {
                import_item(item, &mut builder);
            }
        }
    }

    Ok(builder.finish())
}

fn import_item(item: OnePuxItem, builder: &mut PreviewBuilder) {
    if item.category_uuid != "001" {
        builder.skip_unsupported_item("secure note item");
        return;
    }
    if !item.details.sections.is_empty() {
        builder.record_unsupported_field("sections");
    }

    let username = login_field_value(&item.details.login_fields, "username");
    let password = login_field_value(&item.details.login_fields, "password");
    builder.import_credential(CredentialCandidate {
        origin_url: item.overview.origin_url(),
        username,
        password,
    });
}

impl OnePuxOverview {
    fn origin_url(&self) -> String {
        if !self.url.trim().is_empty() {
            return self.url.trim().to_string();
        }
        self.urls
            .iter()
            .find_map(|url| {
                let trimmed = url.url.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            })
            .unwrap_or_default()
    }
}

fn login_field_value(fields: &[OnePuxLoginField], expected: &str) -> String {
    fields
        .iter()
        .find_map(|field| {
            let designation = field.designation.trim();
            let name = field.name.trim();
            (designation.eq_ignore_ascii_case(expected) || name.eq_ignore_ascii_case(expected))
                .then(|| field.value.clone())
        })
        .unwrap_or_default()
}

fn read_export_data(contents: &[u8]) -> ImportResult<Vec<u8>> {
    let mut offset = 0usize;
    while offset < contents.len() {
        let Some(signature) = contents.get(offset..offset.saturating_add(4)) else {
            return corrupt_1pux();
        };
        if signature == CENTRAL_DIRECTORY_SIGNATURE
            || signature == END_OF_CENTRAL_DIRECTORY_SIGNATURE
        {
            break;
        }
        if signature != LOCAL_FILE_HEADER_SIGNATURE {
            return corrupt_1pux();
        }

        let header_end = checked_add(offset, LOCAL_FILE_HEADER_LEN)?;
        if header_end > contents.len() {
            return corrupt_1pux();
        }

        let method = read_u16(contents, checked_add(offset, 8)?)?;
        let compressed_size = u32_to_usize(read_u32(contents, checked_add(offset, 18)?)?)?;
        let uncompressed_size = u32_to_usize(read_u32(contents, checked_add(offset, 22)?)?)?;
        let file_name_len = usize::from(read_u16(contents, checked_add(offset, 26)?)?);
        let extra_len = usize::from(read_u16(contents, checked_add(offset, 28)?)?);
        let name_start = header_end;
        let name_end = checked_add(name_start, file_name_len)?;
        let data_start = checked_add(name_end, extra_len)?;
        let data_end = checked_add(data_start, compressed_size)?;
        if data_end > contents.len() {
            return corrupt_1pux();
        }

        let name_bytes = contents
            .get(name_start..name_end)
            .ok_or_else(corrupt_error)?;
        let name = std::str::from_utf8(name_bytes).map_err(|_| corrupt_error())?;
        if name == "export.data" {
            if method != 0 || compressed_size != uncompressed_size {
                return Err(ImportError::Parse(
                    "unsupported 1pux compression method".into(),
                ));
            }
            return contents
                .get(data_start..data_end)
                .map(|data| data.to_vec())
                .ok_or_else(corrupt_error);
        }

        offset = data_end;
    }

    Err(corrupt_error())
}

fn read_u16(contents: &[u8], offset: usize) -> ImportResult<u16> {
    let bytes = contents
        .get(offset..checked_add(offset, 2)?)
        .and_then(|slice| <[u8; 2]>::try_from(slice).ok())
        .ok_or_else(corrupt_error)?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32(contents: &[u8], offset: usize) -> ImportResult<u32> {
    let bytes = contents
        .get(offset..checked_add(offset, 4)?)
        .and_then(|slice| <[u8; 4]>::try_from(slice).ok())
        .ok_or_else(corrupt_error)?;
    Ok(u32::from_le_bytes(bytes))
}

fn u32_to_usize(value: u32) -> ImportResult<usize> {
    usize::try_from(value).map_err(|_| corrupt_error())
}

fn checked_add(lhs: usize, rhs: usize) -> ImportResult<usize> {
    lhs.checked_add(rhs).ok_or_else(corrupt_error)
}

fn corrupt_1pux<T>() -> ImportResult<T> {
    Err(corrupt_error())
}

fn corrupt_error() -> ImportError {
    ImportError::Parse("corrupt 1pux archive".into())
}
