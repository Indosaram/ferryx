//! Installs the Chromium engine on demand, so Ferryx ships without it and users who never
//! open a Chromium tab never download it.
//!
//! The engine is published as one zip per platform with a manifest beside it. Installing
//! downloads the zip, checks its SHA-256 against the manifest, unpacks it into a staging
//! directory under the app data dir and renames that into place, so a half-finished install
//! is never picked up. The installed engine lives in `<data>/browser-engine/<version>`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// The engine version this build drives. The engine's switches and window handling change with
/// the Ferryx side, so the app only installs and runs the build made for it.
pub const ENGINE_VERSION: &str = "156.2.0-ferryx.1";

/// Where the per-version manifest is published.
pub const MANIFEST_URL: &str =
    "https://github.com/Indosaram/ferryx/releases/download/browser-engine-156.2.0-ferryx.1/manifest.json";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: String,
    pub platforms: std::collections::HashMap<String, Artifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum EngineStatus {
    /// No engine for this platform.
    Unsupported,
    NotInstalled,
    Installed { version: String },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InstallError {
    #[error("no engine is published for {0}")]
    Unsupported(String),
    #[error("engine manifest is for {found}, this build needs {expected}")]
    VersionMismatch { expected: String, found: String },
    #[error("engine download is {found} bytes, the manifest says {expected}")]
    SizeMismatch { expected: u64, found: u64 },
    #[error("engine download does not match its checksum")]
    ChecksumMismatch,
    #[error("engine archive is invalid: {0}")]
    BadArchive(String),
    #[error("engine archive has no {0}")]
    MissingExecutable(String),
    #[error("{0}")]
    Io(String),
    #[error("engine download failed: {0}")]
    Network(String),
}

impl From<std::io::Error> for InstallError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
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

pub fn install_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("browser-engine").join(ENGINE_VERSION)
}

pub fn status(data_dir: &Path, executable: &str) -> EngineStatus {
    if platform_key().is_none() {
        return EngineStatus::Unsupported;
    }
    if install_dir(data_dir).join(executable).is_file() {
        EngineStatus::Installed {
            version: ENGINE_VERSION.to_string(),
        }
    } else {
        EngineStatus::NotInstalled
    }
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
        .ok_or_else(|| InstallError::Unsupported(platform.to_string()))
}

/// Checks a downloaded archive against its manifest entry.
pub fn verify(bytes: &[u8], artifact: &Artifact) -> Result<(), InstallError> {
    if bytes.len() as u64 != artifact.size {
        return Err(InstallError::SizeMismatch {
            expected: artifact.size,
            found: bytes.len() as u64,
        });
    }
    let digest = Sha256::digest(bytes);
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    if hex.eq_ignore_ascii_case(artifact.sha256.trim()) {
        Ok(())
    } else {
        Err(InstallError::ChecksumMismatch)
    }
}

/// Unpacks a verified archive into `<data>/browser-engine/<version>`. Entries that would land
/// outside the target (absolute paths, `..`) reject the whole archive.
pub fn unpack(bytes: &[u8], data_dir: &Path, executable: &str) -> Result<PathBuf, InstallError> {
    let root = data_dir.join("browser-engine");
    std::fs::create_dir_all(&root)?;
    let staging = root.join(format!(".staging-{}", uuid::Uuid::new_v4()));
    let result = unpack_into(bytes, &staging, executable).and_then(|()| {
        let target = install_dir(data_dir);
        if target.exists() {
            std::fs::remove_dir_all(&target)?;
        }
        std::fs::rename(&staging, &target)?;
        Ok(target)
    });
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

fn unpack_into(bytes: &[u8], staging: &Path, executable: &str) -> Result<(), InstallError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|error| InstallError::BadArchive(error.to_string()))?;
    std::fs::create_dir_all(staging)?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| InstallError::BadArchive(error.to_string()))?;
        let relative = safe_relative(entry.name())
            .ok_or_else(|| InstallError::BadArchive(format!("unsafe entry {}", entry.name())))?;
        let path = staging.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&path)?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut contents = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut contents)?;
        std::fs::write(&path, contents)?;
        // The engine and its helper apps must stay executable on Unix.
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode & 0o777))?;
        }
    }
    if !staging.join(executable).is_file() {
        return Err(InstallError::MissingExecutable(executable.to_string()));
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
/// at another server.
pub fn manifest_url() -> String {
    std::env::var("FERRYX_BROWSER_ENGINE_MANIFEST_URL").unwrap_or_else(|_| MANIFEST_URL.to_string())
}

/// Downloads the manifest and this platform's archive.
pub async fn fetch(client: &reqwest::Client) -> Result<Vec<u8>, InstallError> {
    let platform = platform_key().ok_or_else(|| InstallError::Unsupported(std::env::consts::OS.to_string()))?;
    let network = |error: reqwest::Error| InstallError::Network(error.to_string());
    let manifest: Manifest = client
        .get(manifest_url())
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(network)?
        .json()
        .await
        .map_err(network)?;
    let artifact = select_artifact(&manifest, platform)?;
    let bytes = client
        .get(&artifact.url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(network)?
        .bytes()
        .await
        .map_err(network)?;
    verify(&bytes, &artifact)?;
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn artifact_for(bytes: &[u8]) -> Artifact {
        let hex: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
        Artifact {
            url: "https://example.invalid/engine.zip".into(),
            sha256: hex,
            size: bytes.len() as u64,
        }
    }

    #[test]
    fn verify_accepts_matching_and_rejects_changed_archives() {
        let bytes = zip_of(&[("engine.exe", b"x")]);
        let artifact = artifact_for(&bytes);
        assert_eq!(verify(&bytes, &artifact), Ok(()));
        let mut flipped = bytes.clone();
        flipped[0] ^= 1;
        assert_eq!(verify(&flipped, &artifact), Err(InstallError::ChecksumMismatch));
        assert!(matches!(
            verify(&bytes[1..], &artifact),
            Err(InstallError::SizeMismatch { .. })
        ));
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
            Err(InstallError::Unsupported("linux-x86_64".into()))
        );
    }

    #[test]
    fn unpack_installs_into_the_version_dir_and_leaves_no_staging() {
        let data = tempfile::tempdir().expect("tempdir");
        let bytes = zip_of(&[("engine.exe", b"bin"), ("locales/en-US.pak", b"pak")]);
        let dir = unpack(&bytes, data.path(), "engine.exe").expect("unpack");
        assert_eq!(dir, install_dir(data.path()));
        assert_eq!(std::fs::read(dir.join("locales/en-US.pak")).expect("pak"), b"pak");
        let leftovers: Vec<_> = std::fs::read_dir(data.path().join("browser-engine"))
            .expect("engine root")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".staging"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn unpack_rejects_escaping_entries_and_missing_executable() {
        let data = tempfile::tempdir().expect("tempdir");
        let escaping = zip_of(&[("../evil.exe", b"x"), ("engine.exe", b"bin")]);
        assert!(matches!(
            unpack(&escaping, data.path(), "engine.exe"),
            Err(InstallError::BadArchive(_))
        ));
        assert!(!data.path().join("evil.exe").exists());
        let incomplete = zip_of(&[("readme.txt", b"x")]);
        assert_eq!(
            unpack(&incomplete, data.path(), "engine.exe"),
            Err(InstallError::MissingExecutable("engine.exe".into()))
        );
        assert!(!install_dir(data.path()).exists());
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

