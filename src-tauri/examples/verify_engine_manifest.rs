//! Checks a browser-engine release directory with the app's own verifier: the manifest
//! signature against a public key, then each listed archive's size and SHA-256.
//!
//!   cargo run --manifest-path src-tauri/Cargo.toml --example verify_engine_manifest -- \
//!       <dir> <base64 public key>
//!
//! Exits 0 when the app would accept the release, 1 with the typed reason when it would not.

use ferryx_lib::browser::engine_install::{parse_public_key, verify_file, verify_manifest, InstallError};
use std::path::Path;

fn check(dir: &Path, key: &ed25519_dalek::VerifyingKey) -> Result<String, InstallError> {
    let manifest = std::fs::read(dir.join("manifest.json"))?;
    let signature = std::fs::read_to_string(dir.join("manifest.json.sig")).unwrap_or_default();
    let manifest = verify_manifest(&manifest, &signature, std::slice::from_ref(key))?;
    for (platform, artifact) in &manifest.platforms {
        let file = artifact.url.rsplit('/').next().unwrap_or_default();
        verify_file(&dir.join(file), artifact)?;
        println!("{platform}: {file} size and sha256 OK");
    }
    Ok(manifest.version)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(dir), Some(key)) = (args.next(), args.next()) else {
        eprintln!("usage: verify_engine_manifest <dir> <base64 public key>");
        std::process::exit(2);
    };
    let dir = std::path::PathBuf::from(dir);
    let Some(key) = parse_public_key(&key) else {
        eprintln!("not a base64 ed25519 public key");
        std::process::exit(2);
    };
    match check(&dir, &key) {
        Ok(version) => println!("ACCEPTED manifest {version}"),
        Err(error) => {
            println!("REJECTED {}", serde_json::to_string(&error).unwrap_or_default());
            std::process::exit(1);
        }
    }
}
