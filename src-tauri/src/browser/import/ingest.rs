//! Moves staged import records (see [`super::staging`]) into Ferryx's stores and deletes the
//! staged files.
//!
//! - Bookmarks go under `Imported from <source>` on the bookmark bar, keeping their folders. A
//!   url already in its target folder is a duplicate.
//! - History: a visit whose url already has a visit at the same time is a duplicate.
//! - Passwords go into the vault through its normal save path, keyed by origin and username: the
//!   same password is a duplicate, a new password updates the saved login.
//!
//! Every staged file is overwritten with zeros and deleted once it was read, whether or not its
//! records were stored, so plaintext passwords never outlive the import that wrote them. A
//! `.partial` file left by a crashed process is deleted the same way. All functions block.

use super::staging::{StagedRecord, PARTIAL_SUFFIX};
use super::{
    ImportDataType, ImportTypeError, ImportTypeResult, ImportedBookmark, ImportedHistoryEntry,
    ImportedPassword,
};
use crate::browser::library::bookmarks::{ImportedBookmark as LibraryBookmark, BAR_ROOT};
use crate::browser::library::history::{is_recordable_url, ImportedVisit};
use crate::browser::library::BrowserLibrary;
use crate::browser::vault::store::SaveKind;
use crate::browser::vault::{origin_of_page, CredentialOrigin, VaultError, VaultStore};
use std::io::Write;
use std::path::{Path, PathBuf};
use zeroize::{Zeroize, Zeroizing};

/// One ingest at a time, so two imports never read the same file.
static INGEST: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

/// Access to the password vault (the app's shared store, or a test store).
pub trait PasswordVault {
    /// Runs `f` on the open vault; `Err` when the vault cannot be opened.
    fn with_vault(
        &self,
        f: &mut dyn FnMut(&mut VaultStore) -> Result<(), VaultError>,
    ) -> Result<(), VaultError>;
}

/// What the stores kept of one data type.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeIngest {
    pub data_type: ImportDataType,
    /// Records added (or, for passwords, saved logins updated).
    pub stored: usize,
    /// Records the store already had.
    pub duplicates: usize,
    /// Records the store cannot hold (an unsupported url, an empty password, a torn line).
    pub skipped: usize,
    pub error: Option<ImportTypeError>,
}

impl TypeIngest {
    fn new(data_type: ImportDataType) -> Self {
        Self { data_type, stored: 0, duplicates: 0, skipped: 0, error: None }
    }

    fn fail(&mut self, error: ImportTypeError) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }

    fn merge(&mut self, other: Self) {
        self.stored += other.stored;
        self.duplicates += other.duplicates;
        self.skipped += other.skipped;
        if let Some(error) = other.error {
            self.fail(error);
        }
    }
}

fn staged_type(name: &str) -> Option<ImportDataType> {
    let kind = name.split('-').next()?;
    [ImportDataType::Bookmarks, ImportDataType::History, ImportDataType::Passwords]
        .into_iter()
        .find(|data_type| data_type.as_str() == kind)
}

/// The writer's pid of a `<type>-<millis>-<run>-<pid>.jsonl.partial` name.
fn partial_pid(name: &str) -> Option<u32> {
    name.strip_suffix(&format!(".jsonl{PARTIAL_SUFFIX}"))?.rsplit('-').next()?.parse().ok()
}

fn wipe(path: &Path) -> std::io::Result<()> {
    let len = std::fs::metadata(path)?.len();
    let mut file = std::fs::OpenOptions::new().write(true).open(path)?;
    let zeros = [0u8; 8192];
    let mut left = len;
    while left > 0 {
        let n = left.min(zeros.len() as u64) as usize;
        file.write_all(&zeros[..n])?;
        left -= n as u64;
    }
    file.sync_all()
}

/// Overwrites `path` with zeros, then deletes it.
pub(super) fn shred(path: &Path) -> std::io::Result<()> {
    if let Err(error) = wipe(path) {
        tracing::warn!(path = %path.display(), %error, "could not overwrite a staged import file");
    }
    std::fs::remove_file(path)
}

fn records<T: serde::de::DeserializeOwned>(text: &str, ingest: &mut TypeIngest) -> Vec<StagedRecord<T>> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| match serde_json::from_str(line) {
            Ok(record) => Some(record),
            Err(_) => {
                ingest.skipped += 1;
                None
            }
        })
        .collect()
}

fn store_failed(error: impl std::fmt::Display) -> ImportTypeError {
    ImportTypeError::new("store-failed", error.to_string())
}

fn ingest_bookmarks(text: &str, library: &BrowserLibrary) -> TypeIngest {
    let mut ingest = TypeIngest::new(ImportDataType::Bookmarks);
    let entries: Vec<LibraryBookmark> = records::<ImportedBookmark>(text, &mut ingest)
        .into_iter()
        .filter_map(|staged| {
            if !is_recordable_url(&staged.record.url) {
                ingest.skipped += 1;
                return None;
            }
            let mut folder_path = vec![format!("Imported from {}", staged.source.trim())];
            folder_path.extend(staged.record.folder_path);
            Some(LibraryBookmark { title: staged.record.title, url: staged.record.url, folder_path })
        })
        .collect();
    let added = library.with_bookmarks(|store| {
        let added = store.import(BAR_ROOT, &entries)?;
        store.save()?;
        Ok(added)
    });
    match added {
        Ok(added) => {
            ingest.stored = added;
            ingest.duplicates = entries.len().saturating_sub(added);
        }
        Err(error) => ingest.fail(store_failed(error)),
    }
    ingest
}

fn ingest_history(text: &str, library: &BrowserLibrary) -> TypeIngest {
    let mut ingest = TypeIngest::new(ImportDataType::History);
    let visits: Vec<ImportedVisit> = records::<ImportedHistoryEntry>(text, &mut ingest)
        .into_iter()
        .filter_map(|staged| {
            let entry = staged.record;
            let visited_at = (entry.visited_at_unix * 1000.0).round() as i64;
            if !is_recordable_url(&entry.url) || visited_at <= 0 {
                ingest.skipped += 1;
                return None;
            }
            let title = Some(entry.title).filter(|title| !title.trim().is_empty());
            Some(ImportedVisit { url: entry.url, title, visited_at, visit_count: entry.visit_count })
        })
        .collect();
    match library.with_history(|history| history.import_new(&visits)) {
        Ok((inserted, duplicates)) => {
            ingest.stored = inserted;
            ingest.duplicates = duplicates;
        }
        Err(error) => ingest.fail(store_failed(error)),
    }
    ingest
}

struct Login {
    origin: CredentialOrigin,
    username: String,
    password: Zeroizing<String>,
}

fn vault_failed(error: &VaultError) -> ImportTypeError {
    match error {
        VaultError::Unavailable(_) | VaultError::Keystore(_) => {
            ImportTypeError::new("vault-unavailable", error.to_string())
        }
        _ => store_failed(error),
    }
}

fn ingest_passwords(text: &str, vault: &dyn PasswordVault) -> TypeIngest {
    let mut ingest = TypeIngest::new(ImportDataType::Passwords);
    let logins: Vec<Login> = records::<ImportedPassword>(text, &mut ingest)
        .into_iter()
        .filter_map(|staged| {
            let mut record = staged.record;
            let password = Zeroizing::new(std::mem::take(&mut record.password));
            record.action_url.zeroize();
            match origin_of_page(&record.origin_url) {
                Ok(origin) if !password.is_empty() => {
                    Some(Login { origin, username: record.username, password })
                }
                _ => {
                    ingest.skipped += 1;
                    None
                }
            }
        })
        .collect();
    if logins.is_empty() {
        return ingest;
    }
    let (mut stored, mut duplicates, mut skipped) = (0, 0, 0);
    let saved = vault.with_vault(&mut |store: &mut VaultStore| {
        for login in &logins {
            if store.classify(&login.origin, &login.username, &login.password)? == SaveKind::Unchanged {
                duplicates += 1;
                continue;
            }
            match store.save(login.origin.as_str(), &login.username, &login.password) {
                Ok(_) => stored += 1,
                Err(VaultError::InvalidArgument(_)) => skipped += 1,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    });
    ingest.stored = stored;
    ingest.duplicates = duplicates;
    ingest.skipped += skipped;
    if let Err(error) = saved {
        ingest.fail(vault_failed(&error));
    }
    ingest
}

fn ingest_file(
    data_type: ImportDataType,
    path: &Path,
    library: &BrowserLibrary,
    vault: &dyn PasswordVault,
) -> TypeIngest {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => Zeroizing::new(text),
        Err(error) => {
            let mut ingest = TypeIngest::new(data_type);
            ingest.fail(store_failed(format!("{}: {error}", path.display())));
            return ingest;
        }
    };
    match data_type {
        ImportDataType::Bookmarks => ingest_bookmarks(&text, library),
        ImportDataType::History => ingest_history(&text, library),
        _ => ingest_passwords(&text, vault),
    }
}

/// Stores every staged file in `dir` and deletes it; one entry per data type that had a file.
pub fn ingest_pending(dir: &Path, library: &BrowserLibrary, vault: &dyn PasswordVault) -> Vec<TypeIngest> {
    let _one_at_a_time = INGEST.lock();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(dir = %dir.display(), %error, "could not list staged import files");
            }
            return Vec::new();
        }
    };
    let mut staged: Vec<(String, ImportDataType, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if partial_pid(&name).is_some_and(|pid| pid != std::process::id()) {
            if let Err(error) = shred(&path) {
                tracing::warn!(path = %path.display(), %error, "could not delete a stale staging file");
            }
        } else if let Some(data_type) = staged_type(&name).filter(|_| name.ends_with(".jsonl")) {
            staged.push((name, data_type, path));
        }
    }
    staged.sort_by(|a, b| a.0.cmp(&b.0));

    let mut outcomes: Vec<TypeIngest> = Vec::new();
    for (_, data_type, path) in staged {
        let mut ingest = ingest_file(data_type, &path, library, vault);
        if let Err(error) = shred(&path) {
            ingest.fail(ImportTypeError::new(
                "cleanup-failed",
                format!("could not delete {}: {error}", path.display()),
            ));
        }
        match outcomes.iter_mut().find(|outcome| outcome.data_type == data_type) {
            Some(outcome) => outcome.merge(ingest),
            None => outcomes.push(ingest),
        }
    }
    outcomes.sort_by_key(|outcome| ImportDataType::ALL.iter().position(|kind| *kind == outcome.data_type));
    outcomes
}

/// Replaces each type's staged count with what the stores kept. A type that was not part of
/// this import (a file an earlier, interrupted import left) is appended.
pub fn apply(results: &mut Vec<ImportTypeResult>, ingested: Vec<TypeIngest>) {
    for ingest in ingested {
        match results.iter_mut().find(|result| result.data_type == ingest.data_type) {
            Some(result) => {
                result.imported = ingest.stored;
                result.duplicates = ingest.duplicates;
                result.skipped += ingest.skipped;
                if result.error.is_none() {
                    result.error = ingest.error;
                }
            }
            None => results.push(ImportTypeResult {
                data_type: ingest.data_type,
                imported: ingest.stored,
                duplicates: ingest.duplicates,
                skipped: ingest.skipped,
                notes: Vec::new(),
                error: ingest.error,
            }),
        }
    }
}

#[cfg(test)]
#[path = "ingest_tests.rs"]
mod tests;
