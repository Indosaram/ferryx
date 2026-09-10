use crate::ipc::error::{IpcError, IpcErrorCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HelperTarget {
    #[serde(rename = "x86_64-unknown-linux-gnu")]
    LinuxX86_64,
    #[serde(rename = "aarch64-unknown-linux-gnu")]
    LinuxAarch64,
    #[serde(rename = "x86_64-pc-windows-msvc")]
    WindowsX64,
}

impl HelperTarget {
    pub fn triple(&self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "x86_64-unknown-linux-gnu",
            Self::LinuxAarch64 => "aarch64-unknown-linux-gnu",
            Self::WindowsX64 => "x86_64-pc-windows-msvc",
        }
    }

    pub fn filename(&self) -> &'static str {
        match self {
            Self::LinuxX86_64 | Self::LinuxAarch64 => "ferryx-remote-helper",
            Self::WindowsX64 => "ferryx-remote-helper.exe",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperAssetEntry {
    pub target: HelperTarget,
    pub filename: String,
    pub sha256: String,
    pub byte_length: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperAssetManifest {
    pub schema_version: u32,
    pub helper_version: String,
    pub protocol_version: u32,
    pub artifacts: Vec<HelperAssetEntry>,
}

impl HelperAssetManifest {
    pub fn find_artifact(&self, target: HelperTarget) -> Option<&HelperAssetEntry> {
        self.artifacts.iter().find(|a| a.target == target)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedHelperAsset {
    pub target: HelperTarget,
    pub binary_path: PathBuf,
    pub sha256: String,
    pub byte_length: u64,
}

pub fn compute_file_sha256(path: &Path) -> Result<String, std::io::Error> {
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn resolve_target_from_probe(
    platform: &str,
    arch: &str,
) -> Result<HelperTarget, IpcError> {
    let norm_platform = platform.trim().to_lowercase();
    let norm_arch = arch.trim().to_lowercase();

    match norm_platform.as_str() {
        "posix" | "linux" => match norm_arch.as_str() {
            "x86_64" | "amd64" => Ok(HelperTarget::LinuxX86_64),
            "aarch64" | "arm64" => Ok(HelperTarget::LinuxAarch64),
            _ => Err(IpcError::new(
                IpcErrorCode::Unsupported,
                format!("Unsupported remote CPU architecture for Linux: '{arch}'"),
            )),
        },
        "windows" => match norm_arch.as_str() {
            "x86_64" | "amd64" => Ok(HelperTarget::WindowsX64),
            _ => Err(IpcError::new(
                IpcErrorCode::Unsupported,
                format!("Unsupported remote CPU architecture for Windows: '{arch}'"),
            )),
        },
        _ => Err(IpcError::new(
            IpcErrorCode::Unsupported,
            format!("Unsupported remote operating system platform: '{platform}'"),
        )),
    }
}

pub fn resolve_helper_asset(
    base_dir: &Path,
    target: HelperTarget,
) -> Result<ResolvedHelperAsset, IpcError> {
    let manifest_path = base_dir.join("manifest.json");
    if !manifest_path.exists() {
        return Err(IpcError::new(
            IpcErrorCode::CliExecutableNotFound,
            format!("Helper manifest not found at '{}'", manifest_path.display()),
        )
        .with_details(serde_json::json!({
            "stage": "helper_asset_manifest_missing",
            "path": manifest_path.to_string_lossy(),
            "target": target.triple(),
        })));
    }

    let manifest_bytes = std::fs::read(&manifest_path).map_err(|e| {
        IpcError::new(
            IpcErrorCode::IoError,
            format!("Failed to read helper manifest: {e}"),
        )
    })?;

    let manifest: HelperAssetManifest = serde_json::from_slice(&manifest_bytes).map_err(|e| {
        IpcError::new(
            IpcErrorCode::ParseError,
            format!("Failed to parse helper manifest: {e}"),
        )
    })?;

    let entry = manifest.find_artifact(target).ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::CliExecutableNotFound,
            format!(
                "No bundled helper artifact found for target '{}'",
                target.triple()
            ),
        )
        .with_details(serde_json::json!({
            "stage": "helper_asset_target_missing",
            "target": target.triple(),
        }))
    })?;

    let binary_path = base_dir.join(target.triple()).join(&entry.filename);
    if !binary_path.exists() {
        return Err(IpcError::new(
            IpcErrorCode::CliExecutableNotFound,
            format!(
                "Bundled helper binary missing at '{}'",
                binary_path.display()
            ),
        )
        .with_details(serde_json::json!({
            "stage": "helper_asset_binary_missing",
            "path": binary_path.to_string_lossy(),
            "target": target.triple(),
        })));
    }

    let actual_sha = compute_file_sha256(&binary_path).map_err(|e| {
        IpcError::new(
            IpcErrorCode::IoError,
            format!("Failed to hash helper binary: {e}"),
        )
    })?;

    if !actual_sha.eq_ignore_ascii_case(&entry.sha256) {
        return Err(IpcError::new(
            IpcErrorCode::InvalidArgument,
            format!(
                "Helper binary checksum mismatch for '{}': expected {}, got {}",
                target.triple(),
                entry.sha256,
                actual_sha
            ),
        )
        .with_details(serde_json::json!({
            "stage": "helper_asset_checksum_mismatch",
            "expected": entry.sha256,
            "actual": actual_sha,
            "target": target.triple(),
        })));
    }

    Ok(ResolvedHelperAsset {
        target,
        binary_path,
        sha256: actual_sha,
        byte_length: entry.byte_length,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn resolve_target_normalizes_linux_x86_64() {
        assert_eq!(
            resolve_target_from_probe("posix", "x86_64").unwrap(),
            HelperTarget::LinuxX86_64
        );
        assert_eq!(
            resolve_target_from_probe("posix", "amd64").unwrap(),
            HelperTarget::LinuxX86_64
        );
    }

    #[test]
    fn resolve_target_normalizes_linux_aarch64() {
        assert_eq!(
            resolve_target_from_probe("posix", "aarch64").unwrap(),
            HelperTarget::LinuxAarch64
        );
        assert_eq!(
            resolve_target_from_probe("posix", "arm64").unwrap(),
            HelperTarget::LinuxAarch64
        );
    }

    #[test]
    fn resolve_target_normalizes_windows_x64() {
        assert_eq!(
            resolve_target_from_probe("windows", "x86_64").unwrap(),
            HelperTarget::WindowsX64
        );
        assert_eq!(
            resolve_target_from_probe("windows", "amd64").unwrap(),
            HelperTarget::WindowsX64
        );
        assert_eq!(
            resolve_target_from_probe("windows", "AMD64").unwrap(),
            HelperTarget::WindowsX64
        );
    }

    #[test]
    fn resolve_target_rejects_unsupported_architectures() {
        for (platform, arch) in [
            ("posix", "mips"),
            ("posix", "armv7l"),
            ("posix", "i686"),
            ("darwin", "arm64"),
            ("freebsd", "x86_64"),
            ("windows", "arm64"),
            ("windows", "x86"),
        ] {
            let res = resolve_target_from_probe(platform, arch);
            assert!(res.is_err());
            assert_eq!(res.unwrap_err().code, IpcErrorCode::Unsupported);
        }
    }

    #[test]
    fn manifest_serialization_and_lookup() {
        let json = r#"{
            "schemaVersion": 1,
            "helperVersion": "2026.908.1",
            "protocolVersion": 1,
            "artifacts": [
                {
                    "target": "x86_64-unknown-linux-gnu",
                    "filename": "ferryx-remote-helper",
                    "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                    "byteLength": 123456
                },
                {
                    "target": "x86_64-pc-windows-msvc",
                    "filename": "ferryx-remote-helper.exe",
                    "sha256": "01ba4719c80b6fe911b091a7c05124b64eeece964e09c058ef8f9805daca546b",
                    "byteLength": 234567
                }
            ]
        }"#;

        let manifest: HelperAssetManifest = serde_json::from_str(json).expect("valid manifest");
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.helper_version, "2026.908.1");
        assert_eq!(manifest.protocol_version, 1);

        let linux = manifest.find_artifact(HelperTarget::LinuxX86_64).expect("linux asset");
        assert_eq!(linux.filename, "ferryx-remote-helper");
        assert_eq!(linux.byte_length, 123456);

        assert!(manifest.find_artifact(HelperTarget::LinuxAarch64).is_none());
    }

    #[test]
    fn resolve_helper_asset_verifies_checksum_and_structure() {
        let temp = TempDir::new().unwrap();
        let base = temp.path();

        let target = HelperTarget::WindowsX64;
        let target_dir = base.join(target.triple());
        fs::create_dir_all(&target_dir).unwrap();

        let binary_path = target_dir.join(target.filename());
        let payload = b"fake binary payload for unit testing";
        fs::write(&binary_path, payload).unwrap();

        let mut hasher = Sha256::new();
        hasher.update(payload);
        let valid_sha = format!("{:x}", hasher.finalize());

        let manifest = HelperAssetManifest {
            schema_version: 1,
            helper_version: "2026.908.1".into(),
            protocol_version: 1,
            artifacts: vec![HelperAssetEntry {
                target,
                filename: target.filename().into(),
                sha256: valid_sha.clone(),
                byte_length: payload.len() as u64,
            }],
        };
        fs::write(
            base.join("manifest.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();

        let resolved = resolve_helper_asset(base, target).expect("should resolve valid asset");
        assert_eq!(resolved.target, target);
        assert_eq!(resolved.sha256, valid_sha);
        assert_eq!(resolved.byte_length, payload.len() as u64);
        assert_eq!(resolved.binary_path, binary_path);
    }

    #[test]
    fn resolve_helper_asset_rejects_checksum_mismatch() {
        let temp = TempDir::new().unwrap();
        let base = temp.path();

        let target = HelperTarget::LinuxX86_64;
        let target_dir = base.join(target.triple());
        fs::create_dir_all(&target_dir).unwrap();

        let binary_path = target_dir.join(target.filename());
        fs::write(&binary_path, b"actual content").unwrap();

        let manifest = HelperAssetManifest {
            schema_version: 1,
            helper_version: "2026.908.1".into(),
            protocol_version: 1,
            artifacts: vec![HelperAssetEntry {
                target,
                filename: target.filename().into(),
                sha256: "0000000000000000000000000000000000000000000000000000000000000000".into(),
                byte_length: 14,
            }],
        };
        fs::write(
            base.join("manifest.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();

        let err = resolve_helper_asset(base, target).unwrap_err();
        assert_eq!(err.code, IpcErrorCode::InvalidArgument);
        assert_eq!(
            err.details.unwrap().get("stage").unwrap().as_str(),
            Some("helper_asset_checksum_mismatch")
        );
    }

    #[test]
    fn resolve_helper_asset_reports_missing_manifest_or_target() {
        let temp = TempDir::new().unwrap();
        let base = temp.path();

        let err = resolve_helper_asset(base, HelperTarget::LinuxAarch64).unwrap_err();
        assert_eq!(err.code, IpcErrorCode::CliExecutableNotFound);
        assert_eq!(
            err.details.unwrap().get("stage").unwrap().as_str(),
            Some("helper_asset_manifest_missing")
        );
    }
}
