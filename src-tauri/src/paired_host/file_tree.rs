//! Paired-host directory listing and creation for a recursive file copy.
//!
//! Listing classifies each child with `symlink_metadata`, so a symlink is reported and never
//! followed. Each page stays under the relay's 64 KiB body cap. `mkdir` creates only the named
//! directory; the caller creates parents one level at a time.
use crate::remote::machine_protocol as m;
use std::path::{Path, PathBuf};

pub const PAIRED_FILE_LIST_CAPABILITY: &str = "pairedFileListV1";
/// Advertised by a local daemon whose protocol carries [crate::paired_host::client::Operation::FileList].
pub const PAIRED_FILE_LIST_PROXY_CAPABILITY: &str = "pairedFileListProxyV1";
pub const PAIRED_FILE_MKDIR_CAPABILITY: &str = "pairedFileMkdirV1";
/// Advertised by a local daemon whose protocol carries [crate::paired_host::client::Operation::FileMkdir].
pub const PAIRED_FILE_MKDIR_PROXY_CAPABILITY: &str = "pairedFileMkdirProxyV1";

const LIST_PAGE_ENTRIES: usize = 64;
const LIST_JSON_BUDGET: usize = 48 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostTreeError {
    InvalidRequest,
    InvalidPath,
    NotFound,
    NotDirectory,
    NotRegularFile,
    TooLarge,
    Denied,
    Unavailable,
}

impl HostTreeError {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "INVALID_REQUEST",
            Self::InvalidPath => "INVALID_PATH",
            Self::NotFound => "NOT_FOUND",
            Self::NotDirectory => "NOT_DIRECTORY",
            Self::NotRegularFile => "UNSUPPORTED_PATH",
            Self::TooLarge => "PAYLOAD_TOO_LARGE",
            Self::Denied => "PERMISSION_DENIED",
            Self::Unavailable => "MACHINE_SERVICE_UNAVAILABLE",
        }
    }

    pub fn status(self) -> u16 {
        match self {
            Self::InvalidRequest | Self::InvalidPath | Self::NotDirectory | Self::NotRegularFile => 400,
            Self::NotFound => 404,
            Self::TooLarge => 413,
            Self::Denied => 403,
            Self::Unavailable => 503,
        }
    }
}

fn io_error(error: std::io::Error) -> HostTreeError {
    match error.kind() {
        std::io::ErrorKind::NotFound => HostTreeError::NotFound,
        std::io::ErrorKind::PermissionDenied => HostTreeError::Denied,
        _ => HostTreeError::Unavailable,
    }
}

fn resolve(path: &str, home: Option<&Path>) -> Result<PathBuf, HostTreeError> {
    if !super::file_read::is_home_or_absolute(path) {
        return Err(HostTreeError::InvalidPath);
    }
    super::file_read::resolve_host_path(path, None, home, None).map_err(|error| match error {
        super::file_read::HostReadError::Unavailable => HostTreeError::Unavailable,
        _ => HostTreeError::InvalidPath,
    })
}

fn child_name(entry: &std::fs::DirEntry) -> Result<String, HostTreeError> {
    let name = entry.file_name().into_string().map_err(|_| HostTreeError::Unavailable)?;
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.chars().any(char::is_control)
    {
        return Err(HostTreeError::InvalidPath);
    }
    Ok(name)
}

fn kind_of(meta: &std::fs::Metadata) -> m::FileKind {
    let file_type = meta.file_type();
    if file_type.is_symlink() {
        m::FileKind::Symlink
    } else if file_type.is_dir() {
        m::FileKind::Dir
    } else if file_type.is_file() {
        m::FileKind::File
    } else {
        m::FileKind::Other
    }
}

/// Lists one page. Call only on a blocking worker.
pub fn serve_host_list(
    request: &m::FileListRequest,
    home: Option<&Path>,
) -> Result<m::FileListResult, HostTreeError> {
    if !m::is_hyphenated_uuid(&request.request_id) {
        return Err(HostTreeError::InvalidRequest);
    }
    if request.cursor.as_deref().is_some_and(|cursor| {
        cursor.is_empty() || cursor.contains('/') || cursor.contains('\\') || cursor.chars().any(char::is_control)
    }) {
        return Err(HostTreeError::InvalidRequest);
    }
    let resolved = resolve(&request.path, home)?;
    let path = resolved.to_str().ok_or(HostTreeError::InvalidPath)?.to_string();
    let own = std::fs::symlink_metadata(&resolved).map_err(io_error)?;
    if own.file_type().is_symlink() {
        return Err(HostTreeError::NotRegularFile);
    }
    if own.is_file() {
        return Err(HostTreeError::NotDirectory);
    }
    if !own.is_dir() {
        return Err(HostTreeError::NotRegularFile);
    }
    let mut classified = Vec::new();
    for child in std::fs::read_dir(&resolved).map_err(io_error)? {
        let child = child.map_err(io_error)?;
        let name = child_name(&child)?;
        // A put's staging and receipt files are not part of the tree.
        if super::file_write::is_staging_name(&name) {
            continue;
        }
        if request.cursor.as_deref().is_some_and(|cursor| name.as_str() <= cursor) {
            continue;
        }
        let meta = std::fs::symlink_metadata(child.path()).map_err(io_error)?;
        classified.push(m::FileListEntry { name, kind: kind_of(&meta) });
    }
    classified.sort_by(|left, right| left.name.cmp(&right.name));
    let mut entries = Vec::new();
    let mut truncated = false;
    for entry in classified {
        let mut trial = entries.clone();
        trial.push(entry.clone());
        let trial_result = m::FileListResult {
            path: path.clone(),
            entries: trial,
            truncated: true,
        };
        let size = serde_json::to_vec(&trial_result).map_err(|_| HostTreeError::Unavailable)?.len();
        if entries.len() >= LIST_PAGE_ENTRIES || size > LIST_JSON_BUDGET {
            if entries.is_empty() {
                return Err(HostTreeError::TooLarge);
            }
            truncated = true;
            break;
        }
        entries.push(entry);
    }
    Ok(m::FileListResult { path, entries, truncated })
}

/// Creates one directory. Call only on a blocking worker.
pub fn serve_host_mkdir(
    request: &m::FileMkdirRequest,
    home: Option<&Path>,
) -> Result<m::FileMkdirResult, HostTreeError> {
    if !m::is_hyphenated_uuid(&request.request_id) {
        return Err(HostTreeError::InvalidRequest);
    }
    let resolved = resolve(&request.path, home)?;
    let path = resolved.to_str().ok_or(HostTreeError::InvalidPath)?.to_string();
    let parent = resolved.parent().ok_or(HostTreeError::InvalidPath)?;
    let parent_meta = std::fs::metadata(parent).map_err(io_error)?;
    if !parent_meta.is_dir() {
        return Err(HostTreeError::NotFound);
    }
    match std::fs::symlink_metadata(&resolved) {
        Ok(meta) if meta.file_type().is_symlink() => return Err(HostTreeError::NotRegularFile),
        Ok(meta) if meta.is_dir() => {
            return Ok(m::FileMkdirResult { path, created: false });
        }
        Ok(_) => return Err(HostTreeError::NotRegularFile),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(error)),
    }
    match std::fs::create_dir(&resolved) {
        Ok(()) => Ok(m::FileMkdirResult { path, created: true }),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let meta = std::fs::symlink_metadata(&resolved).map_err(io_error)?;
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(HostTreeError::NotRegularFile);
            }
            Ok(m::FileMkdirResult { path, created: false })
        }
        Err(error) => Err(io_error(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> String {
        "3941b9de-b16d-4d9a-ae0a-118f90fd91f4".into()
    }

    fn list(path: &str, cursor: Option<&str>) -> m::FileListRequest {
        m::FileListRequest {
            request_id: id(),
            path: path.into(),
            cursor: cursor.map(str::to_string),
        }
    }

    fn mkdir(path: &str) -> m::FileMkdirRequest {
        m::FileMkdirRequest { request_id: id(), path: path.into() }
    }

    #[test]
    fn list_and_mkdir_refuse_a_relative_path_and_a_climb() {
        let home = std::env::temp_dir();
        assert_eq!(serve_host_list(&list("notes", None), Some(&home)).unwrap_err(), HostTreeError::InvalidPath);
        assert_eq!(serve_host_list(&list("/../secret", None), Some(&home)).unwrap_err(), HostTreeError::InvalidPath);
        assert_eq!(serve_host_mkdir(&mkdir("notes"), Some(&home)).unwrap_err(), HostTreeError::InvalidPath);
        assert_eq!(serve_host_mkdir(&mkdir("/../secret"), Some(&home)).unwrap_err(), HostTreeError::InvalidPath);
    }

    #[test]
    fn a_tree_is_created_listed_and_read_back_without_following_a_symlink() {
        let root = std::env::temp_dir().join(format!("ferryx-tree-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let src = root.join("src");
        let dest = root.join("dest");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("a.txt"), b"hello").unwrap();
        std::fs::create_dir(src.join("sub")).unwrap();
        std::fs::write(src.join("sub").join("b.txt"), b"world").unwrap();
        std::fs::create_dir(src.join("sub").join("empty")).unwrap();
        std::fs::write(root.join("secret.txt"), b"secret").unwrap();
        std::fs::write(src.join(".3941b9de-b16d-4d9a-ae0a-118f90fd91f4.ferryx-ok"), b"5").unwrap();
        std::fs::write(src.join(".3941b9de-b16d-4d9a-ae0a-118f90fd91f4.ferryx-partial"), b"he").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("secret.txt"), src.join("escape")).unwrap();

        let created = serve_host_mkdir(&mkdir(dest.to_str().unwrap()), Some(&root)).unwrap();
        assert!(created.created);
        let again = serve_host_mkdir(&mkdir(dest.to_str().unwrap()), Some(&root)).unwrap();
        assert!(!again.created);
        assert!(serve_host_mkdir(&mkdir(dest.join("sub").to_str().unwrap()), Some(&root)).unwrap().created);
        assert!(serve_host_mkdir(&mkdir(dest.join("sub").join("empty").to_str().unwrap()), Some(&root)).unwrap().created);

        let page = serve_host_list(&list(src.to_str().unwrap(), None), Some(&root)).unwrap();
        assert!(!page.truncated);
        let escape = page.entries.iter().find(|entry| entry.name == "escape");
        #[cfg(unix)]
        assert_eq!(escape.map(|entry| entry.kind), Some(m::FileKind::Symlink));
        #[cfg(not(unix))]
        let _ = escape;
        assert!(page.entries.iter().any(|entry| entry.name == "a.txt" && entry.kind == m::FileKind::File));
        assert!(page.entries.iter().any(|entry| entry.name == "sub" && entry.kind == m::FileKind::Dir));
        assert!(!page.entries.iter().any(|entry| entry.name == "secret.txt"));
        assert!(!page.entries.iter().any(|entry| entry.name.contains(".ferryx-")), "receipts are hidden");
        #[cfg(unix)]
        assert_eq!(
            serve_host_list(&list(src.join("escape").to_str().unwrap(), None), Some(&root)).unwrap_err(),
            HostTreeError::NotRegularFile
        );

        let nested = serve_host_list(&list(src.join("sub").to_str().unwrap(), None), Some(&root)).unwrap();
        assert!(nested.entries.iter().any(|entry| entry.name == "b.txt" && entry.kind == m::FileKind::File));
        assert!(nested.entries.iter().any(|entry| entry.name == "empty" && entry.kind == m::FileKind::Dir));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_long_directory_pages_under_the_relay_body_cap() {
        let root = std::env::temp_dir().join(format!("ferryx-page-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        for index in 0..(LIST_PAGE_ENTRIES + 1) {
            std::fs::write(root.join(format!("f{index:04}")), b"x").unwrap();
        }
        let first = serve_host_list(&list(root.to_str().unwrap(), None), Some(&root)).unwrap();
        assert!(first.truncated);
        assert_eq!(first.entries.len(), LIST_PAGE_ENTRIES);
        assert!(serde_json::to_vec(&first).unwrap().len() < m::MACHINE_JSON_MAX_BYTES);
        let cursor = first.entries.last().unwrap().name.clone();
        let second = serve_host_list(&list(root.to_str().unwrap(), Some(&cursor)), Some(&root)).unwrap();
        assert!(!second.truncated);
        assert_eq!(second.entries.len(), 1);
        assert_eq!(second.entries[0].name, format!("f{:04}", LIST_PAGE_ENTRIES));
        let _ = std::fs::remove_dir_all(&root);
    }
}

