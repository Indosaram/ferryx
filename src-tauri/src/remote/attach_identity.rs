use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use x25519_dalek::{PublicKey, StaticSecret};

use super::auth::{canonical_remote_dir, write_private_json};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachIdentity {
    pub public_key: String,
    pub private_key: String,
}

pub fn load_or_generate_attach_identity(base_dir: &Path) -> Result<AttachIdentity, String> {
    let _guard = AttachLock::acquire(base_dir)?;
    let path = base_dir.join("attach-identity.json");
    match std::fs::read(&path) {
        Ok(bytes) => parse_attach_identity(&bytes).map_err(|error| {
            format!("ATTACH_IDENTITY_CORRUPT: {error}")
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let secret = StaticSecret::random_from_rng(OsRng);
            let public = PublicKey::from(&secret);
            let identity = AttachIdentity {
                public_key: STANDARD.encode(public.as_bytes()),
                private_key: STANDARD.encode(secret.to_bytes()),
            };
            write_private_json(&path, &identity)
                .map_err(|error| format!("Failed to persist attach identity: {error}"))?;
            Ok(identity)
        }
        Err(error) => Err(format!("Failed to read attach identity: {error}")),
    }
}

pub fn load_or_generate_canonical_attach_identity() -> Result<AttachIdentity, String> {
    let dir = canonical_remote_dir().ok_or("Cannot resolve machine identity directory")?;
    load_or_generate_attach_identity(&dir)
}

fn parse_attach_identity(bytes: &[u8]) -> Result<AttachIdentity, String> {
    let identity: AttachIdentity =
        serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let secret_bytes = decode_key(&identity.private_key)?;
    let public_bytes = decode_key(&identity.public_key)?;
    let secret = StaticSecret::from(secret_bytes);
    let derived = PublicKey::from(&secret);
    if derived.as_bytes() != &public_bytes {
        return Err("public key does not match private key".into());
    }
    Ok(identity)
}

fn decode_key(value: &str) -> Result<[u8; 32], String> {
    let bytes = STANDARD
        .decode(value)
        .map_err(|error| format!("invalid key encoding: {error}"))?;
    bytes
        .try_into()
        .map_err(|_| "key must be 32 bytes".to_string())
}

struct AttachLock {
    _file: File,
}

impl AttachLock {
    fn acquire(base_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(base_dir)
            .map_err(|error| format!("Failed to create attach identity directory: {error}"))?;
        let path = lock_path(base_dir);
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|error| format!("Failed to open attach identity lock: {error}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
            if rc != 0 {
                return Err(format!(
                    "Failed to lock attach identity: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }
        Ok(Self { _file: file })
    }
}

#[cfg(unix)]
impl Drop for AttachLock {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        unsafe { libc::flock(self._file.as_raw_fd(), libc::LOCK_UN) };
    }
}

fn lock_path(base_dir: &Path) -> PathBuf {
    base_dir.join("attach-identity.tx.lock")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn attach_identity_does_not_rewrite_machine_key() {
        let dir = tempfile::tempdir().expect("temp");
        let identity_path = dir.path().join("identity.json");
        std::fs::write(&identity_path, b"{\"machineId\":\"keep\"}").expect("seed");
        let first = load_or_generate_attach_identity(dir.path()).expect("generate");
        assert_eq!(std::fs::read(&identity_path).expect("read"), b"{\"machineId\":\"keep\"}");
        let mode = std::fs::metadata(dir.path().join("attach-identity.json"))
            .expect("meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let second = load_or_generate_attach_identity(dir.path()).expect("reload");
        assert_eq!(first, second);
        std::fs::remove_file(dir.path().join("attach-identity.json")).expect("remove");
        let third = load_or_generate_attach_identity(dir.path()).expect("regen");
        assert_ne!(first.private_key, third.private_key);
        assert_eq!(std::fs::read(&identity_path).expect("still"), b"{\"machineId\":\"keep\"}");

        std::fs::write(dir.path().join("attach-identity.json"), b"{not-json").expect("corrupt");
        let error = load_or_generate_attach_identity(dir.path()).expect_err("corrupt");
        assert!(error.contains("ATTACH_IDENTITY_CORRUPT"), "{error}");
        assert_eq!(
            std::fs::read(dir.path().join("attach-identity.json")).expect("untouched"),
            b"{not-json"
        );
    }
}
