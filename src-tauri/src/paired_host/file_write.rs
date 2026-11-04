//! Paired-host file write: one regular file at a caller-chosen path.
//!
//! Each chunk is at most 32 KiB raw so the base64 JSON body stays under the relay's
//! 64 KiB forwarded-body cap. A retried chunk overwrites the same offset. The chunk
//! that finishes the declared length renames a staging file over the destination.
use crate::remote::machine_protocol as m;
use base64::Engine as _;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{Duration, SystemTime};

pub const PAIRED_FILE_WRITE_CAPABILITY: &str = "pairedFileWriteV1";
/// Advertised by a local daemon whose protocol carries [crate::paired_host::client::Operation::FileWrite].
pub const PAIRED_FILE_WRITE_PROXY_CAPABILITY: &str = "pairedFileWriteProxyV1";

const STALE_STAGING: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostWriteError {
    InvalidRequest,
    InvalidPath,
    NotFound,
    NotRegularFile,
    TooLarge,
    Denied,
    Unavailable,
    StagingLost,
}

impl HostWriteError {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "INVALID_REQUEST",
            Self::InvalidPath => "INVALID_PATH",
            Self::NotFound => "NOT_FOUND",
            Self::NotRegularFile => "UNSUPPORTED_PATH",
            Self::TooLarge => "PAYLOAD_TOO_LARGE",
            Self::Denied => "PERMISSION_DENIED",
            Self::Unavailable => "MACHINE_SERVICE_UNAVAILABLE",
            Self::StagingLost => "STAGING_LOST",
        }
    }
    pub fn status(self) -> u16 {
        match self {
            Self::InvalidRequest | Self::InvalidPath | Self::NotRegularFile => 400,
            Self::NotFound | Self::StagingLost => 404,
            Self::TooLarge => 413,
            Self::Denied => 403,
            Self::Unavailable => 503,
        }
    }
}

fn io_error(error: std::io::Error) -> HostWriteError {
    match error.kind() {
        std::io::ErrorKind::NotFound => HostWriteError::NotFound,
        std::io::ErrorKind::PermissionDenied => HostWriteError::Denied,
        _ => HostWriteError::Unavailable,
    }
}

fn staging_name(upload_id: &str, suffix: &str) -> String {
    format!(".{upload_id}.ferryx-{suffix}")
}

/// True for a staging name this module writes (`.<uuid>.ferryx-partial` or `.<uuid>.ferryx-ok`).
pub(super) fn is_staging_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix('.') else {
        return false;
    };
    ["partial", "ok"].iter().any(|suffix| {
        rest.strip_suffix(&format!(".ferryx-{suffix}")).is_some_and(m::is_hyphenated_uuid)
    })
}

/// Writes one chunk. Call only on a blocking worker.
pub fn serve_host_write(
    request: &m::FileWriteRequest,
    home: Option<&Path>,
) -> Result<m::FileWriteResult, HostWriteError> {
    if !m::is_hyphenated_uuid(&request.upload_id) || !m::is_hyphenated_uuid(&request.request_id) {
        return Err(HostWriteError::InvalidRequest);
    }
    if !super::file_read::is_home_or_absolute(&request.path) {
        return Err(HostWriteError::InvalidPath);
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(request.data.trim())
        .map_err(|_| HostWriteError::InvalidRequest)?;
    if bytes.len() > m::FILE_WRITE_CHUNK_MAX_BYTES as usize {
        return Err(HostWriteError::TooLarge);
    }
    let end = request
        .offset
        .checked_add(bytes.len() as u64)
        .ok_or(HostWriteError::InvalidRequest)?;
    if end > request.total_bytes || (request.total_bytes == 0 && !bytes.is_empty()) {
        return Err(HostWriteError::InvalidRequest);
    }
    let resolved = super::file_read::resolve_host_path(&request.path, None, home, None)
        .map_err(|_| HostWriteError::InvalidPath)?;
    let parent = resolved.parent().ok_or(HostWriteError::InvalidPath)?;
    // Follow a symlink in the parent, the way scp opens the destination directory.
    // The final component is still refused when it is itself a symlink.
    let parent_meta = std::fs::metadata(parent).map_err(io_error)?;
    if !parent_meta.is_dir() {
        return Err(HostWriteError::NotFound);
    }
    if request.offset == 0 {
        sweep_stale(parent);
    }
    refuse_existing_destination(&resolved)?;
    let partial = parent.join(staging_name(&request.upload_id, "partial"));
    let receipt = parent.join(staging_name(&request.upload_id, "ok"));
    // A completing chunk with a non-zero offset is a later page. If its staging file is
    // already gone, the publish either finished (receipt) or was lost.
    if completing(request, &bytes) && request.offset > 0 && !partial.exists() {
        if receipt_matches(&receipt, request.total_bytes) && dest_len(&resolved)? == Some(request.total_bytes) {
            return Ok(done(&resolved, request.offset));
        }
        return Err(HostWriteError::StagingLost);
    }
    write_at(&partial, request.offset, &bytes)?;
    if !completing(request, &bytes) {
        return Ok(m::FileWriteResult {
            path: resolved.to_string_lossy().into_owned(),
            offset: request.offset,
            complete: false,
        });
    }
    let written = std::fs::symlink_metadata(&partial).map_err(io_error)?.len();
    if written != request.total_bytes {
        return Err(HostWriteError::InvalidRequest);
    }
    replace_file(&partial, &resolved)?;
    std::fs::write(&receipt, request.total_bytes.to_string()).map_err(io_error)?;
    Ok(done(&resolved, request.offset))
}

fn completing(request: &m::FileWriteRequest, bytes: &[u8]) -> bool {
    request.offset + bytes.len() as u64 == request.total_bytes
}

fn done(path: &Path, offset: u64) -> m::FileWriteResult {
    m::FileWriteResult {
        path: path.to_string_lossy().into_owned(),
        offset,
        complete: true,
    }
}

fn dest_len(path: &Path) -> Result<Option<u64>, HostWriteError> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Ok(Some(meta.len())),
        Ok(_) => Err(HostWriteError::NotRegularFile),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(error)),
    }
}

fn receipt_matches(path: &Path, total: u64) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
        == Some(total)
}

fn refuse_existing_destination(path: &Path) -> Result<(), HostWriteError> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
        Ok(meta) if meta.file_type().is_symlink() || meta.is_dir() || !meta.is_file() => {
            Err(HostWriteError::NotRegularFile)
        }
        Ok(_) => Ok(()),
    }
}

fn write_at(path: &Path, offset: u64, bytes: &[u8]) -> Result<(), HostWriteError> {
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(HostWriteError::NotRegularFile);
        }
    }
    let mut file = open_partial(path)?;
    if offset == 0 {
        file.set_len(0).map_err(io_error)?;
    }
    file.seek(SeekFrom::Start(offset)).map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    Ok(())
}

fn open_partial(path: &Path) -> Result<std::fs::File, HostWriteError> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true).read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path).map_err(io_error)
}

fn replace_file(from: &Path, to: &Path) -> Result<(), HostWriteError> {
    // Replaces an existing file on every target (Windows: MoveFileExW REPLACE_EXISTING).
    std::fs::rename(from, to).map_err(io_error)
}

fn sweep_stale(parent: &Path) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    let Some(cutoff) = SystemTime::now().checked_sub(STALE_STAGING) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !name.ends_with(".ferryx-partial") && !name.ends_with(".ferryx-ok") {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            continue;
        }
        if meta.modified().ok().is_some_and(|modified| modified < cutoff) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// True when a max-size write chunk, plus a longest legal path, still fits the relay body cap.
pub fn max_chunk_fits_relay_body() -> bool {
    let request = m::FileWriteRequest {
        request_id: "3941b9de-b16d-4d9a-ae0a-118f90fd91f4".into(),
        path: format!("/{}", "a".repeat(4095)),
        upload_id: "3941b9de-b16d-4d9a-ae0a-118f90fd91f4".into(),
        offset: 0,
        total_bytes: m::FILE_WRITE_CHUNK_MAX_BYTES as u64,
        data: base64::engine::general_purpose::STANDARD
            .encode(vec![b'x'; m::FILE_WRITE_CHUNK_MAX_BYTES as usize]),
    };
    serde_json::to_vec(&request)
        .map(|bytes| bytes.len() < m::MACHINE_JSON_MAX_BYTES)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paired_host::file_read::serve_host_read;

    fn request(path: &str, offset: u64, total: u64, bytes: &[u8]) -> m::FileWriteRequest {
        m::FileWriteRequest {
            request_id: "3941b9de-b16d-4d9a-ae0a-118f90fd91f4".into(),
            path: path.into(),
            upload_id: "8f5c1c2a-0d3e-4b6a-9c7d-1e2f3a4b5c6d".into(),
            offset,
            total_bytes: total,
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        }
    }

    #[test]
    fn a_max_write_chunk_stays_under_the_relay_body_cap() {
        assert!(max_chunk_fits_relay_body());
    }

    #[test]
    fn write_refuses_a_relative_path_and_a_climb_above_root() {
        let dir = std::env::temp_dir();
        assert_eq!(
            serve_host_write(&request("notes.txt", 0, 1, b"a"), Some(&dir)).unwrap_err(),
            HostWriteError::InvalidPath
        );
        assert_eq!(
            serve_host_write(&request("/../secret", 0, 1, b"a"), Some(&dir)).unwrap_err(),
            HostWriteError::InvalidPath
        );
    }

    #[test]
    fn write_then_read_round_trips_and_a_lost_final_response_retries() {
        let dir = std::env::temp_dir().join(format!("ferryx-write-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("note.txt");
        let spec = path.to_str().unwrap();
        let first = serve_host_write(&request(spec, 0, 5, b"hello"), Some(&dir)).unwrap();
        assert!(first.complete);
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
        let again = serve_host_write(&request(spec, 0, 5, b"hello"), Some(&dir)).unwrap();
        assert!(again.complete);
        let page_one = serve_host_write(&request(spec, 0, 5, b"hel"), Some(&dir)).unwrap();
        assert!(!page_one.complete);
        let page_two = request(spec, 3, 5, b"lo");
        assert!(serve_host_write(&page_two, Some(&dir)).unwrap().complete);
        assert!(serve_host_write(&page_two, Some(&dir)).unwrap().complete);
        let page = serve_host_read(
            &m::FileReadRequest {
                request_id: "3941b9de-b16d-4d9a-ae0a-118f90fd91f4".into(),
                session_id: None,
                cwd: None,
                path: spec.into(),
                offset: 0,
                length: 16,
                copy: false,
            },
            Some(&dir),
            None,
        )
        .unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD.decode(page.data).unwrap(),
            b"hello"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_oversized_chunk_and_a_directory_are_refused() {
        let dir = std::env::temp_dir().join(format!("ferryx-write-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let big = vec![0u8; m::FILE_WRITE_CHUNK_MAX_BYTES as usize + 1];
        let file = dir.join("big.bin");
        assert_eq!(
            serve_host_write(
                &request(file.to_str().unwrap(), 0, big.len() as u64, &big),
                Some(&dir)
            )
            .unwrap_err(),
            HostWriteError::TooLarge
        );
        assert_eq!(
            serve_host_write(&request(dir.to_str().unwrap(), 0, 1, b"a"), Some(&dir)).unwrap_err(),
            HostWriteError::NotRegularFile
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_destination_is_refused() {
        let dir = std::env::temp_dir().join(format!("ferryx-write-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let target = dir.join("real.txt");
        std::fs::write(&target, b"keep").unwrap();
        let link = dir.join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(
            serve_host_write(&request(link.to_str().unwrap(), 0, 1, b"x"), Some(&dir)).unwrap_err(),
            HostWriteError::NotRegularFile
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
