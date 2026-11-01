//! Installs the Chromium engine on demand, so Ferryx ships without it and users who never
//! open a Chromium tab never download it.
//!
//! The engine is published as one zip per platform with a manifest beside it, and the manifest
//! carries a detached ed25519 signature (`manifest.json.sig`, base64 of the 64 signature bytes)
//! made with a key whose public half is compiled in here. Installing:
//!
//! 1. downloads the manifest and its signature and refuses an unsigned or wrongly signed one;
//! 2. streams this platform's zip into `<root>/.download-*.part`, checking size and SHA-256
//!    against the signed manifest (a short, cut off, oversized or changed download is refused
//!    and its part file removed);
//! 3. unpacks it into `<root>/.staging-*` and renames that to `<root>/<version>_<sha12>`;
//! 4. points `<root>/active` at the new directory (written to a temp file and renamed over the
//!    old pointer, so a crash leaves either the old or the new engine active, never neither),
//!    remembers the one it replaced in `<root>/previous` for rollback, and removes older ones.
//!
//! `<root>` is `<data>/browser-engine`. An install made before the pointer existed
//! (`<root>/<version>`) is still found.

use ed25519_dalek::{Signature, VerifyingKey};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;

/// The engine version this build drives. The engine's switches and window handling change with
/// the Ferryx side, so the app only installs the build made for it.
pub const ENGINE_VERSION: &str = "156.2.0-ferryx.1";

/// Engine versions this build can run: the one it installs first, then older builds that speak
/// the same protocol and may stay active through a rollback.
pub const SUPPORTED_ENGINE_VERSIONS: &[&str] = &[ENGINE_VERSION];

/// Where the per-version manifest is published.
pub const MANIFEST_URL: &str =
    "https://github.com/Indosaram/ferryx/releases/download/browser-engine-156.2.0-ferryx.1/manifest.json";

/// Public keys (base64 of the 32 key bytes) whose manifest signatures this build accepts.
/// `scripts/browser-engine/release-engine.mjs keygen` prints the line to add here; the private
/// key never enters the repository. Keep the previous key listed for one release when rotating.
pub const TRUSTED_MANIFEST_KEYS: &[&str] = &[];

/// A debug build may trust one more key from this variable, so QA can install an engine signed
/// with a throwaway key. Release builds ignore it.
pub const DEBUG_PUBLIC_KEY_ENV: &str = "FERRYX_BROWSER_ENGINE_PUBLIC_KEY";

const ACTIVE_POINTER: &str = "active";
const PREVIOUS_POINTER: &str = "previous";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: String,
    pub platforms: HashMap<String, Artifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state", rename_all_fields = "camelCase")]
pub enum EngineStatus {
    /// No engine for this platform.
    Unsupported,
    NotInstalled,
    /// `previous` is the version a rollback returns to, when one is kept and runnable.
    Installed {
        version: String,
        previous: Option<String>,
    },
}

/// Why an install, rollback or removal failed. Serialized (tag `reason`) into the IPC error's
/// `details`, so the settings page names the cause instead of parsing the message.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "reason", rename_all_fields = "camelCase")]
pub enum InstallError {
    #[error("no engine is published for {platform}")]
    Unsupported { platform: String },
    #[error("engine manifest is for {found}, this build needs {expected}")]
    VersionMismatch { expected: String, found: String },
    #[error("the engine manifest is not signed")]
    Unsigned,
    #[error("the engine manifest signature is not valid for this build")]
    BadSignature,
    #[error("engine manifest is invalid: {detail}")]
    BadManifest { detail: String },
    #[error("engine download stopped after {received} of {expected} bytes")]
    Partial { expected: u64, received: u64 },
    #[error("engine download is larger than the {expected} bytes the manifest says")]
    Oversized { expected: u64 },
    #[error("engine download does not match its checksum")]
    ChecksumMismatch,
    #[error("not enough disk space for the engine: {detail}")]
    DiskFull { detail: String },
    #[error("engine archive is invalid: {detail}")]
    BadArchive { detail: String },
    #[error("engine archive has no {executable}")]
    MissingExecutable { executable: String },
    #[error("an engine install is already running")]
    Busy,
    #[error("no earlier engine is kept to roll back to")]
    NoRollback,
    #[error("{detail}")]
    Io { detail: String },
    #[error("engine download failed: {detail}")]
    Network { detail: String },
}

impl From<std::io::Error> for InstallError {
    fn from(error: std::io::Error) -> Self {
        if is_disk_full(&error) {
            Self::DiskFull { detail: error.to_string() }
        } else {
            Self::Io { detail: error.to_string() }
        }
    }
}

/// Whether an I/O error means the volume is full: `StorageFull`, or the raw OS codes for it
/// where std does not map them (ENOSPC/EDQUOT, ERROR_DISK_FULL/ERROR_HANDLE_DISK_FULL).
pub fn is_disk_full(error: &std::io::Error) -> bool {
    if matches!(
        error.kind(),
        std::io::ErrorKind::StorageFull | std::io::ErrorKind::QuotaExceeded
    ) {
        return true;
    }
    match error.raw_os_error() {
        #[cfg(unix)]
        Some(code) => code == libc::ENOSPC || code == libc::EDQUOT,
        #[cfg(windows)]
        Some(code) => code == 39 || code == 112,
        _ => false,
    }
}

/// The manifest key for the running platform, or `None` where no engine is built.
pub fn platform_key() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("windows-x86_64"),
        ("macos", "aarch64") => Some("macos-aarch64"),
        _ => None,
    }
}

pub fn engine_root(data_dir: &Path) -> PathBuf {
    data_dir.join("browser-engine")
}

/// The directory of the active engine: the one `active` names, else an install made before
/// the pointer existed. A pointer to a version this build cannot run counts as none.
pub fn active_dir(data_dir: &Path) -> Option<PathBuf> {
    let root = engine_root(data_dir);
    match read_pointer(&root, ACTIVE_POINTER) {
        Some(name) => is_supported_dir(&name).then(|| root.join(name)),
        None => {
            let legacy = root.join(ENGINE_VERSION);
            legacy.is_dir().then_some(legacy)
        }
    }
}

/// Where the engine runs from: the active install, else where a pre-pointer install lived.
pub fn install_dir(data_dir: &Path) -> PathBuf {
    active_dir(data_dir).unwrap_or_else(|| engine_root(data_dir).join(ENGINE_VERSION))
}

pub fn status(data_dir: &Path, executable: &str) -> EngineStatus {
    if platform_key().is_none() {
        return EngineStatus::Unsupported;
    }
    let Some(active) = active_dir(data_dir).filter(|dir| dir.join(executable).is_file()) else {
        return EngineStatus::NotInstalled;
    };
    EngineStatus::Installed {
        version: dir_version(&active),
        previous: rollback_target(data_dir, executable).map(|dir| dir_version(&dir)),
    }
}

fn dir_version(dir: &Path) -> String {
    let name = dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    version_of(&name).to_string()
}

/// `156.2.0-ferryx.1_0123456789ab` -> `156.2.0-ferryx.1`; a pre-pointer dir is just the version.
fn version_of(dir_name: &str) -> &str {
    dir_name.rsplit_once('_').map_or(dir_name, |(version, _)| version)
}

fn is_supported_dir(name: &str) -> bool {
    is_single_component(name) && SUPPORTED_ENGINE_VERSIONS.contains(&version_of(name))
}

fn is_single_component(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn read_pointer(root: &Path, pointer: &str) -> Option<String> {
    let name = std::fs::read_to_string(root.join(pointer)).ok()?;
    let name = name.trim();
    is_single_component(name).then(|| name.to_string())
}

/// Replaces a pointer file in one rename, so readers see the old or the new name.
fn write_pointer(root: &Path, pointer: &str, name: &str) -> Result<(), InstallError> {
    let temp = root.join(format!(".{pointer}-{}", uuid::Uuid::new_v4()));
    let result = std::fs::write(&temp, name).and_then(|()| std::fs::rename(&temp, root.join(pointer)));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    Ok(result?)
}

/// Picks this platform's artifact from a manifest made for this build.
pub fn select_artifact(manifest: &Manifest, platform: &str) -> Result<Artifact, InstallError> {
    if manifest.version != ENGINE_VERSION {
        return Err(InstallError::VersionMismatch {
            expected: ENGINE_VERSION.to_string(),
            found: manifest.version.clone(),
        });
    }
    manifest
        .platforms
        .get(platform)
        .cloned()
        .ok_or_else(|| InstallError::Unsupported { platform: platform.to_string() })
}

/// The keys this build trusts: the compiled-in list, plus the debug-only override.
pub fn trusted_keys() -> Vec<VerifyingKey> {
    let mut keys: Vec<VerifyingKey> = TRUSTED_MANIFEST_KEYS.iter().filter_map(|key| parse_public_key(key)).collect();
    if cfg!(debug_assertions) {
        if let Some(key) = std::env::var(DEBUG_PUBLIC_KEY_ENV).ok().and_then(|key| parse_public_key(&key)) {
            keys.push(key);
        }
    }
    keys
}

/// A base64 (standard alphabet) ed25519 public key.
pub fn parse_public_key(encoded: &str) -> Option<VerifyingKey> {
    use base64::Engine as _;
    let bytes: [u8; 32] = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?
        .try_into()
        .ok()?;
    VerifyingKey::from_bytes(&bytes).ok()
}

/// Checks the manifest's detached signature against `keys` and parses it. The signature covers
/// the manifest bytes exactly as served.
pub fn verify_manifest(manifest: &[u8], signature: &str, keys: &[VerifyingKey]) -> Result<Manifest, InstallError> {
    use base64::Engine as _;
    let signature = signature.trim();
    if signature.is_empty() {
        return Err(InstallError::Unsigned);
    }
    let signature = base64::engine::general_purpose::STANDARD
        .decode(signature)
        .ok()
        .and_then(|bytes| Signature::from_slice(&bytes).ok())
        .ok_or(InstallError::BadSignature)?;
    if !keys.iter().any(|key| key.verify_strict(manifest, &signature).is_ok()) {
        return Err(InstallError::BadSignature);
    }
    serde_json::from_slice(manifest).map_err(|error| InstallError::BadManifest { detail: error.to_string() })
}

/// Checks a download as it arrives: never more bytes than the manifest says, and on `finish`
/// exactly that many with the manifest's SHA-256.
pub struct DownloadCheck {
    hasher: Sha256,
    received: u64,
    expected_size: u64,
    expected_sha256: String,
}

impl DownloadCheck {
    pub fn new(artifact: &Artifact) -> Self {
        Self {
            hasher: Sha256::new(),
            received: 0,
            expected_size: artifact.size,
            expected_sha256: artifact.sha256.trim().to_ascii_lowercase(),
        }
    }

    pub fn update(&mut self, chunk: &[u8]) -> Result<(), InstallError> {
        self.received += chunk.len() as u64;
        if self.received > self.expected_size {
            return Err(InstallError::Oversized { expected: self.expected_size });
        }
        self.hasher.update(chunk);
        Ok(())
    }

    pub fn finish(self) -> Result<(), InstallError> {
        if self.received != self.expected_size {
            return Err(InstallError::Partial {
                expected: self.expected_size,
                received: self.received,
            });
        }
        let hex: String = self.hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
        if hex == self.expected_sha256 {
            Ok(())
        } else {
            Err(InstallError::ChecksumMismatch)
        }
    }
}

/// Checks a complete archive in memory against its manifest entry.
pub fn verify(bytes: &[u8], artifact: &Artifact) -> Result<(), InstallError> {
    let mut check = DownloadCheck::new(artifact);
    check.update(bytes)?;
    check.finish()
}

/// Checks an archive on disk against its manifest entry, reading it in chunks.
pub fn verify_file(path: &Path, artifact: &Artifact) -> Result<(), InstallError> {
    let mut file = std::fs::File::open(path)?;
    let mut check = DownloadCheck::new(artifact);
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        check.update(&buffer[..read])?;
    }
    check.finish()
}

/// Writes `chunks` to `writer` while checking them, flushing at the end. A chunk error is the
/// download breaking off; a write error is the disk (full, or otherwise failing).
pub async fn copy_checked<S, E, W>(chunks: S, writer: &mut W, artifact: &Artifact) -> Result<(), InstallError>
where
    S: futures_util::Stream<Item = Result<bytes::Bytes, E>>,
    E: std::fmt::Display,
    W: tokio::io::AsyncWrite + Unpin,
{
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;
    let mut chunks = std::pin::pin!(chunks);
    let mut check = DownloadCheck::new(artifact);
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|error| {
            tracing::warn!(received = check.received, "engine download broke off: {error}");
            InstallError::Partial {
                expected: check.expected_size,
                received: check.received,
            }
        })?;
        check.update(&chunk)?;
        writer.write_all(&chunk).await?;
    }
    writer.flush().await?;
    check.finish()
}

/// Removes what an interrupted install left: part files, staging dirs and pointer temp files.
pub fn clean_leftovers(data_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(engine_root(data_dir)) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let removed = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        if let Err(error) = removed {
            tracing::warn!(path = %path.display(), "could not remove engine install leftover: {error}");
        }
    }
}

/// A fresh part file path for a download.
pub fn part_path(data_dir: &Path) -> PathBuf {
    engine_root(data_dir).join(format!(".download-{}.part", uuid::Uuid::new_v4()))
}

/// Unpacks a verified archive file into its version directory and makes it the active engine.
pub fn install_archive(
    archive: &Path,
    data_dir: &Path,
    version: &str,
    sha256: &str,
    executable: &str,
) -> Result<PathBuf, InstallError> {
    let root = engine_root(data_dir);
    std::fs::create_dir_all(&root)?;
    let name = format!("{version}_{}", sha256.trim().get(..12).unwrap_or(sha256).to_ascii_lowercase());
    if !is_single_component(&name) {
        return Err(InstallError::BadManifest { detail: format!("bad version {version}") });
    }
    let target = root.join(&name);
    if !target.join(executable).is_file() {
        let file = std::fs::File::open(archive)?;
        let staging = root.join(format!(".staging-{}", uuid::Uuid::new_v4()));
        let result = unpack_into(file, &staging, executable).and_then(|()| {
            if target.exists() {
                std::fs::remove_dir_all(&target)?;
            }
            Ok(std::fs::rename(&staging, &target)?)
        });
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&staging);
        }
        result?;
    }
    activate(data_dir, &name)?;
    Ok(target)
}

/// Unpacks an in-memory archive (tests, local tools) the same way as [`install_archive`].
pub fn unpack(bytes: &[u8], data_dir: &Path, executable: &str) -> Result<PathBuf, InstallError> {
    let root = engine_root(data_dir);
    std::fs::create_dir_all(&root)?;
    let archive = part_path(data_dir);
    std::fs::write(&archive, bytes)?;
    let sha: String = Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect();
    let result = install_archive(&archive, data_dir, ENGINE_VERSION, &sha, executable);
    let _ = std::fs::remove_file(&archive);
    result
}

/// Points `active` at `name`, keeps the engine it replaces as `previous`, removes the rest.
fn activate(data_dir: &Path, name: &str) -> Result<(), InstallError> {
    let root = engine_root(data_dir);
    let current = read_pointer(&root, ACTIVE_POINTER)
        .or_else(|| root.join(ENGINE_VERSION).is_dir().then(|| ENGINE_VERSION.to_string()));
    write_pointer(&root, ACTIVE_POINTER, name)?;
    let previous = match current {
        Some(current) if current != name => {
            write_pointer(&root, PREVIOUS_POINTER, &current)?;
            Some(current)
        }
        _ => read_pointer(&root, PREVIOUS_POINTER).filter(|previous| previous != name),
    };
    prune(&root, &[Some(name), previous.as_deref()]);
    Ok(())
}

/// Removes engine directories other than `keep`. One still in use (an open engine on Windows)
/// stays until the next install.
fn prune(root: &Path, keep: &[Option<&str>]) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !entry.path().is_dir() || keep.contains(&Some(name.as_str())) {
            continue;
        }
        if let Err(error) = std::fs::remove_dir_all(entry.path()) {
            tracing::warn!(dir = %entry.path().display(), "could not remove an older engine: {error}");
        }
    }
}

/// The kept earlier engine, when it is complete and this build can run it.
fn rollback_target(data_dir: &Path, executable: &str) -> Option<PathBuf> {
    let root = engine_root(data_dir);
    let name = read_pointer(&root, PREVIOUS_POINTER)?;
    let dir = root.join(&name);
    (is_supported_dir(&name) && dir.join(executable).is_file()).then_some(dir)
}

/// Makes the kept earlier engine active again; the one it replaces becomes the kept one.
pub fn rollback(data_dir: &Path, executable: &str) -> Result<EngineStatus, InstallError> {
    let root = engine_root(data_dir);
    let target = rollback_target(data_dir, executable).ok_or(InstallError::NoRollback)?;
    let name = target.file_name().map(|name| name.to_string_lossy().into_owned()).ok_or(InstallError::NoRollback)?;
    let current = read_pointer(&root, ACTIVE_POINTER);
    write_pointer(&root, ACTIVE_POINTER, &name)?;
    match current {
        Some(current) => write_pointer(&root, PREVIOUS_POINTER, &current)?,
        None => std::fs::remove_file(root.join(PREVIOUS_POINTER))?,
    }
    Ok(status(data_dir, executable))
}

/// Removes every installed engine and the pointers.
pub fn remove_all(data_dir: &Path) -> Result<(), InstallError> {
    let root = engine_root(data_dir);
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    Ok(())
}

fn unpack_into(reader: impl Read + std::io::Seek, staging: &Path, executable: &str) -> Result<(), InstallError> {
    let bad = |error: zip::result::ZipError| InstallError::BadArchive { detail: error.to_string() };
    let mut archive = zip::ZipArchive::new(reader).map_err(bad)?;
    std::fs::create_dir_all(staging)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(bad)?;
        let relative = safe_relative(entry.name()).ok_or_else(|| InstallError::BadArchive {
            detail: format!("unsafe entry {}", entry.name()),
        })?;
        let path = staging.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&path)?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&path)?;
        std::io::copy(&mut entry, &mut out)?;
        // The engine and its helper apps must stay executable on Unix.
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode & 0o777))?;
        }
    }
    if !staging.join(executable).is_file() {
        return Err(InstallError::MissingExecutable { executable: executable.to_string() });
    }
    Ok(())
}

fn safe_relative(name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// The manifest location; `FERRYX_BROWSER_ENGINE_MANIFEST_URL` points a test or staging build
/// at another server. The signature is at the same URL plus `.sig`.
pub fn manifest_url() -> String {
    std::env::var("FERRYX_BROWSER_ENGINE_MANIFEST_URL").unwrap_or_else(|_| MANIFEST_URL.to_string())
}

/// One install at a time: a second request while one runs is refused, not interleaved.
static INSTALLING: AtomicBool = AtomicBool::new(false);

pub struct InstallGuard(());

impl InstallGuard {
    pub fn acquire() -> Result<Self, InstallError> {
        INSTALLING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self(()))
            .map_err(|_| InstallError::Busy)
    }
}

impl Drop for InstallGuard {
    fn drop(&mut self) {
        INSTALLING.store(false, Ordering::Release);
    }
}

/// A verified archive on disk, ready for [`install_archive`]. The caller removes `path`.
pub struct Downloaded {
    pub path: PathBuf,
    pub version: String,
    pub sha256: String,
}

/// Downloads and verifies the signed manifest, then streams this platform's archive into a part
/// file under the engine root, verifying it on the way. A failed download leaves no part file.
pub async fn download(client: &reqwest::Client, data_dir: &Path) -> Result<Downloaded, InstallError> {
    let platform = platform_key().ok_or_else(|| InstallError::Unsupported {
        platform: std::env::consts::OS.to_string(),
    })?;
    let network = |error: reqwest::Error| InstallError::Network { detail: error.to_string() };
    let url = manifest_url();
    let manifest = client
        .get(&url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(network)?
        .bytes()
        .await
        .map_err(network)?;
    let signature = client.get(format!("{url}.sig")).send().await.map_err(network)?;
    if signature.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(InstallError::Unsigned);
    }
    let signature = signature.error_for_status().map_err(network)?.text().await.map_err(network)?;
    let manifest = verify_manifest(&manifest, &signature, &trusted_keys())?;
    let artifact = select_artifact(&manifest, platform)?;
    let response = client
        .get(&artifact.url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(network)?;
    tokio::fs::create_dir_all(engine_root(data_dir)).await?;
    let path = part_path(data_dir);
    let result = async {
        let mut file = tokio::fs::File::create(&path).await?;
        copy_checked(response.bytes_stream(), &mut file, &artifact).await
    }
    .await;
    if let Err(error) = result {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(error);
    }
    Ok(Downloaded {
        path,
        version: manifest.version,
        sha256: artifact.sha256,
    })
}

/// Why a tab that should have used Chromium opened in the system webview instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineFallbackNotice {
    pub browser_id: String,
    pub message: String,
}

static FALLBACKS: LazyLock<Mutex<HashMap<String, EngineFallbackNotice>>> = LazyLock::new(Default::default);

/// Remembers that `browser_id` fell back, until its pane takes the notice.
pub fn record_fallback(browser_id: &str, message: &str) -> EngineFallbackNotice {
    let notice = EngineFallbackNotice {
        browser_id: browser_id.to_string(),
        message: message.to_string(),
    };
    FALLBACKS.lock().insert(browser_id.to_string(), notice.clone());
    notice
}

pub fn take_fallback(browser_id: &str) -> Option<EngineFallbackNotice> {
    FALLBACKS.lock().remove(browser_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::io::Write;

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut out);
        for (name, body) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("start entry");
            writer.write_all(body).expect("write entry");
        }
        writer.finish().expect("finish zip");
        out.into_inner()
    }

    fn sha_of(bytes: &[u8]) -> String {
        Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
    }

    fn artifact_for(bytes: &[u8]) -> Artifact {
        Artifact {
            url: "https://example.invalid/engine.zip".into(),
            sha256: sha_of(bytes),
            size: bytes.len() as u64,
        }
    }

    fn signed(manifest: &[u8], key: &SigningKey) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(key.sign(manifest).to_bytes())
    }

    fn manifest_json() -> Vec<u8> {
        format!(
            r#"{{"version":"{ENGINE_VERSION}","platforms":{{"macos-aarch64":{{"url":"https://example.invalid/e.zip","sha256":"{}","size":1}}}}}}"#,
            "a".repeat(64)
        )
        .into_bytes()
    }

    fn leftovers(data: &Path) -> Vec<String> {
        std::fs::read_dir(engine_root(data))
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .filter(|name| name.starts_with('.'))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn a_manifest_signed_by_a_trusted_key_is_accepted() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let manifest = manifest_json();
        let parsed = verify_manifest(&manifest, &signed(&manifest, &key), &[key.verifying_key()]).expect("verified");
        assert_eq!(parsed.version, ENGINE_VERSION);
        assert!(parsed.platforms.contains_key("macos-aarch64"));
    }

    #[test]
    fn bad_signatures_and_unsigned_manifests_are_refused() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let other = SigningKey::from_bytes(&[8; 32]);
        let manifest = manifest_json();
        let signature = signed(&manifest, &key);
        let trusted = [key.verifying_key()];
        assert_eq!(verify_manifest(&manifest, "", &trusted), Err(InstallError::Unsigned));
        assert_eq!(verify_manifest(&manifest, "  \n", &trusted), Err(InstallError::Unsigned));
        assert_eq!(verify_manifest(&manifest, "not base64!", &trusted), Err(InstallError::BadSignature));
        assert_eq!(
            verify_manifest(&manifest, &signed(&manifest, &other), &trusted),
            Err(InstallError::BadSignature)
        );
        let mut tampered = manifest.clone();
        let at = tampered.len() - 3;
        tampered[at] = b'2';
        assert_eq!(verify_manifest(&tampered, &signature, &trusted), Err(InstallError::BadSignature));
        assert_eq!(verify_manifest(&manifest, &signature, &[]), Err(InstallError::BadSignature));
    }

    #[test]
    fn public_keys_parse_from_base64_only_at_full_length() {
        use base64::Engine as _;
        let key = SigningKey::from_bytes(&[9; 32]).verifying_key();
        let encoded = base64::engine::general_purpose::STANDARD.encode(key.as_bytes());
        assert_eq!(parse_public_key(&format!(" {encoded}\n")), Some(key));
        assert_eq!(parse_public_key(&encoded[..20]), None);
        assert_eq!(parse_public_key("@@@"), None);
    }

    #[test]
    fn verify_accepts_matching_and_rejects_changed_archives() {
        let bytes = zip_of(&[("engine.exe", b"x")]);
        let artifact = artifact_for(&bytes);
        assert_eq!(verify(&bytes, &artifact), Ok(()));
        let mut flipped = bytes.clone();
        flipped[0] ^= 1;
        assert_eq!(verify(&flipped, &artifact), Err(InstallError::ChecksumMismatch));
        assert!(matches!(verify(&bytes[1..], &artifact), Err(InstallError::Partial { .. })));
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(matches!(verify(&longer, &artifact), Err(InstallError::Oversized { .. })));
    }

    #[test]
    fn verify_file_reads_the_archive_from_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bytes = vec![5u8; 200_000];
        let path = dir.path().join("engine.zip");
        std::fs::write(&path, &bytes).expect("write");
        assert_eq!(verify_file(&path, &artifact_for(&bytes)), Ok(()));
        let mut other = bytes.clone();
        other[150_000] = 6;
        assert_eq!(verify_file(&path, &artifact_for(&other)), Err(InstallError::ChecksumMismatch));
    }

    #[test]
    fn manifest_for_another_version_is_refused() {
        let manifest = Manifest {
            version: "1.0.0".into(),
            platforms: [("windows-x86_64".to_string(), artifact_for(b"z"))].into(),
        };
        assert!(matches!(
            select_artifact(&manifest, "windows-x86_64"),
            Err(InstallError::VersionMismatch { .. })
        ));
        let current = Manifest {
            version: ENGINE_VERSION.into(),
            ..manifest
        };
        assert!(select_artifact(&current, "windows-x86_64").is_ok());
        assert_eq!(
            select_artifact(&current, "linux-x86_64"),
            Err(InstallError::Unsupported { platform: "linux-x86_64".into() })
        );
    }

    type Chunks = futures_util::stream::Iter<std::vec::IntoIter<Result<bytes::Bytes, String>>>;

    fn chunks(parts: Vec<Result<&'static str, &'static str>>) -> Chunks {
        futures_util::stream::iter(
            parts
                .into_iter()
                .map(|part| part.map(|text| bytes::Bytes::from_static(text.as_bytes())).map_err(str::to_string))
                .collect::<Vec<_>>(),
        )
    }

    #[tokio::test]
    async fn a_complete_download_is_written_and_verified() {
        let artifact = artifact_for(b"hello world");
        let mut out = Vec::new();
        copy_checked(chunks(vec![Ok("hello "), Ok("world")]), &mut out, &artifact)
            .await
            .expect("copied");
        assert_eq!(out, b"hello world");
    }

    #[tokio::test]
    async fn a_download_that_breaks_off_or_ends_early_is_partial() {
        let artifact = artifact_for(b"hello world");
        let mut out = Vec::new();
        assert_eq!(
            copy_checked(chunks(vec![Ok("hello "), Err("connection reset")]), &mut out, &artifact).await,
            Err(InstallError::Partial { expected: 11, received: 6 })
        );
        let mut out = Vec::new();
        assert_eq!(
            copy_checked(chunks(vec![Ok("hello")]), &mut out, &artifact).await,
            Err(InstallError::Partial { expected: 11, received: 5 })
        );
    }

    #[tokio::test]
    async fn a_changed_or_oversized_download_is_refused() {
        let artifact = artifact_for(b"hello world");
        let mut out = Vec::new();
        assert_eq!(
            copy_checked(chunks(vec![Ok("hello World")]), &mut out, &artifact).await,
            Err(InstallError::ChecksumMismatch)
        );
        let mut out = Vec::new();
        assert_eq!(
            copy_checked(chunks(vec![Ok("hello world"), Ok("!")]), &mut out, &artifact).await,
            Err(InstallError::Oversized { expected: 11 })
        );
    }

    /// A writer on a full volume.
    struct FullDisk;

    impl tokio::io::AsyncWrite for FullDisk {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            _: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            std::task::Poll::Ready(Err(std::io::Error::from(std::io::ErrorKind::StorageFull)))
        }
        fn poll_flush(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn a_full_disk_is_reported_as_disk_full() {
        let artifact = artifact_for(b"hello world");
        assert!(matches!(
            copy_checked(chunks(vec![Ok("hello world")]), &mut FullDisk, &artifact).await,
            Err(InstallError::DiskFull { .. })
        ));
        #[cfg(unix)]
        assert!(is_disk_full(&std::io::Error::from_raw_os_error(libc::ENOSPC)));
        #[cfg(windows)]
        assert!(is_disk_full(&std::io::Error::from_raw_os_error(112)));
        assert!(!is_disk_full(&std::io::Error::from(std::io::ErrorKind::PermissionDenied)));
    }

    #[test]
    fn install_errors_serialize_with_a_reason_tag() {
        let value = serde_json::to_value(InstallError::Partial { expected: 9, received: 3 }).expect("json");
        assert_eq!(value, serde_json::json!({ "reason": "partial", "expected": 9, "received": 3 }));
        let value = serde_json::to_value(InstallError::BadSignature).expect("json");
        assert_eq!(value, serde_json::json!({ "reason": "badSignature" }));
    }

    #[test]
    fn unpack_installs_into_a_version_dir_and_leaves_no_staging() {
        let data = tempfile::tempdir().expect("tempdir");
        let bytes = zip_of(&[("engine.exe", b"bin"), ("locales/en-US.pak", b"pak")]);
        let dir = unpack(&bytes, data.path(), "engine.exe").expect("unpack");
        assert_eq!(dir, install_dir(data.path()));
        assert_eq!(std::fs::read(dir.join("locales/en-US.pak")).expect("pak"), b"pak");
        assert!(leftovers(data.path()).is_empty(), "{:?}", leftovers(data.path()));
        assert_eq!(
            status_for_any_platform(data.path(), "engine.exe"),
            Some((ENGINE_VERSION.to_string(), None))
        );
    }

    /// `status` reports `Unsupported` on platforms without a build; tests check the install.
    fn status_for_any_platform(data: &Path, executable: &str) -> Option<(String, Option<String>)> {
        let active = active_dir(data).filter(|dir| dir.join(executable).is_file())?;
        Some((dir_version(&active), rollback_target(data, executable).map(|dir| dir_version(&dir))))
    }

    #[test]
    fn unpack_rejects_escaping_entries_and_missing_executable() {
        let data = tempfile::tempdir().expect("tempdir");
        let escaping = zip_of(&[("../evil.exe", b"x"), ("engine.exe", b"bin")]);
        assert!(matches!(
            unpack(&escaping, data.path(), "engine.exe"),
            Err(InstallError::BadArchive { .. })
        ));
        assert!(!data.path().join("evil.exe").exists());
        let incomplete = zip_of(&[("readme.txt", b"x")]);
        assert_eq!(
            unpack(&incomplete, data.path(), "engine.exe"),
            Err(InstallError::MissingExecutable { executable: "engine.exe".into() })
        );
        assert_eq!(active_dir(data.path()), None);
        assert!(leftovers(data.path()).is_empty(), "{:?}", leftovers(data.path()));
    }

    #[test]
    fn a_failed_install_keeps_the_active_engine() {
        let data = tempfile::tempdir().expect("tempdir");
        let good = unpack(&zip_of(&[("engine.exe", b"v1")]), data.path(), "engine.exe").expect("first");
        assert!(unpack(b"not a zip", data.path(), "engine.exe").is_err());
        assert_eq!(active_dir(data.path()), Some(good.clone()));
        assert_eq!(std::fs::read(good.join("engine.exe")).expect("exe"), b"v1");
    }

    #[test]
    fn installs_keep_one_previous_engine_and_roll_back_to_it() {
        let data = tempfile::tempdir().expect("tempdir");
        let first = unpack(&zip_of(&[("engine.exe", b"v1")]), data.path(), "engine.exe").expect("v1");
        let second = unpack(&zip_of(&[("engine.exe", b"v2")]), data.path(), "engine.exe").expect("v2");
        assert_ne!(first, second);
        assert_eq!(active_dir(data.path()), Some(second.clone()));
        assert_eq!(rollback_target(data.path(), "engine.exe"), Some(first.clone()));
        let third = unpack(&zip_of(&[("engine.exe", b"v3")]), data.path(), "engine.exe").expect("v3");
        assert!(!first.exists(), "the engine two installs back is removed");
        assert!(second.exists() && third.exists());

        rollback(data.path(), "engine.exe").expect("rollback");
        assert_eq!(active_dir(data.path()), Some(second.clone()));
        assert_eq!(rollback_target(data.path(), "engine.exe"), Some(third.clone()));
        assert_eq!(std::fs::read(install_dir(data.path()).join("engine.exe")).expect("exe"), b"v2");
    }

    #[test]
    fn rollback_needs_a_complete_supported_earlier_engine() {
        let data = tempfile::tempdir().expect("tempdir");
        assert_eq!(rollback(data.path(), "engine.exe"), Err(InstallError::NoRollback));
        unpack(&zip_of(&[("engine.exe", b"v1")]), data.path(), "engine.exe").expect("v1");
        assert_eq!(rollback(data.path(), "engine.exe"), Err(InstallError::NoRollback));
        let root = engine_root(data.path());
        std::fs::create_dir_all(root.join("1.0.0_aaaaaaaaaaaa")).expect("old dir");
        std::fs::write(root.join("1.0.0_aaaaaaaaaaaa/engine.exe"), b"old").expect("old exe");
        std::fs::write(root.join(PREVIOUS_POINTER), "1.0.0_aaaaaaaaaaaa").expect("pointer");
        assert_eq!(rollback(data.path(), "engine.exe"), Err(InstallError::NoRollback));
        std::fs::write(root.join(PREVIOUS_POINTER), "../escape").expect("pointer");
        assert_eq!(rollback(data.path(), "engine.exe"), Err(InstallError::NoRollback));
    }

    #[test]
    fn an_install_from_before_the_pointer_is_found_and_kept_as_previous() {
        let data = tempfile::tempdir().expect("tempdir");
        let legacy = engine_root(data.path()).join(ENGINE_VERSION);
        std::fs::create_dir_all(&legacy).expect("legacy");
        std::fs::write(legacy.join("engine.exe"), b"legacy").expect("exe");
        assert_eq!(active_dir(data.path()), Some(legacy.clone()));
        let new = unpack(&zip_of(&[("engine.exe", b"new")]), data.path(), "engine.exe").expect("new");
        assert_eq!(active_dir(data.path()), Some(new));
        assert_eq!(rollback_target(data.path(), "engine.exe"), Some(legacy));
    }

    #[test]
    fn an_active_pointer_to_an_unsupported_version_counts_as_not_installed() {
        let data = tempfile::tempdir().expect("tempdir");
        let root = engine_root(data.path());
        std::fs::create_dir_all(root.join("1.0.0_aaaaaaaaaaaa")).expect("dir");
        std::fs::write(root.join("1.0.0_aaaaaaaaaaaa/engine.exe"), b"old").expect("exe");
        std::fs::write(root.join(ACTIVE_POINTER), "1.0.0_aaaaaaaaaaaa").expect("pointer");
        assert_eq!(active_dir(data.path()), None);
    }

    #[test]
    fn leftovers_of_an_interrupted_install_are_cleaned() {
        let data = tempfile::tempdir().expect("tempdir");
        let installed = unpack(&zip_of(&[("engine.exe", b"v1")]), data.path(), "engine.exe").expect("v1");
        let root = engine_root(data.path());
        std::fs::write(part_path(data.path()), b"half a zip").expect("part");
        std::fs::create_dir_all(root.join(".staging-x/sub")).expect("staging");
        std::fs::write(root.join(".active-x"), b"tmp").expect("tmp pointer");
        clean_leftovers(data.path());
        assert!(leftovers(data.path()).is_empty(), "{:?}", leftovers(data.path()));
        assert_eq!(active_dir(data.path()), Some(installed));
    }

    #[test]
    fn only_one_install_runs_at_a_time() {
        let guard = InstallGuard::acquire().expect("first");
        assert!(matches!(InstallGuard::acquire(), Err(InstallError::Busy)));
        drop(guard);
        assert!(InstallGuard::acquire().is_ok());
    }

    #[test]
    fn a_fallback_notice_is_taken_once() {
        record_fallback("tab-fallback-test", "engine failed");
        assert_eq!(take_fallback("tab-fallback-test").map(|notice| notice.message), Some("engine failed".into()));
        assert_eq!(take_fallback("tab-fallback-test"), None);
    }

    #[cfg(unix)]
    #[test]
    fn unpack_keeps_the_executable_bit() {
        use std::os::unix::fs::PermissionsExt;
        let mut out = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut out);
        writer
            .start_file("engine", zip::write::SimpleFileOptions::default().unix_permissions(0o755))
            .expect("start entry");
        writer.write_all(b"bin").expect("write entry");
        writer
            .start_file("data.pak", zip::write::SimpleFileOptions::default().unix_permissions(0o644))
            .expect("start entry");
        writer.write_all(b"pak").expect("write entry");
        writer.finish().expect("finish zip");
        let data = tempfile::tempdir().expect("tempdir");
        let dir = unpack(&out.into_inner(), data.path(), "engine").expect("unpack");
        let mode = |name: &str| std::fs::metadata(dir.join(name)).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode("engine"), 0o755);
        assert_eq!(mode("data.pak"), 0o644);
    }
}
