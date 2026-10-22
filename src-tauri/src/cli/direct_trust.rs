//! `ferryx direct-trust` — explicit, out-of-band provisioning of direct (P2P) peer keys.
//!
//! Machines enrolled with the same account owner normally pin each other automatically from
//! account-signed peer records; manual pins made here always win over those, `revoke` tombstones
//! the id so it is never auto-repinned, and a later manual `provision` clears the tombstone.
//!
//! The relay is untrusted, so each side exchanges `identity` output by hand and runs
//! `provision` with the other machine's values. Never contacts or launches the daemon:
//! it only touches `<data>/remote/{identity,direct-trust}.json`, with that disk IO
//! offloaded through `crate::ipc::run_blocking` per project convention.
use std::io::Write;
use std::path::Path;

use crate::paired_host::direct_trust::DirectTrustStore;

pub const DIRECT_TRUST_USAGE: &str = "expected `ferryx direct-trust <identity|provision|revoke>`\n  identity:  `ferryx direct-trust identity`\n  provision: `ferryx direct-trust provision <machineId> <publicKeyBase64> [--replace]`\n  revoke:    `ferryx direct-trust revoke <machineId>`";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectTrustCliCommand {
    Identity,
    Provision {
        machine_id: String,
        public_key: String,
        replace: bool,
    },
    Revoke {
        machine_id: String,
    },
}

pub fn parse_direct_trust_cli<I, T>(args: I) -> Result<DirectTrustCliCommand, String>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    if args.get(1).is_none_or(|arg| arg != "direct-trust") {
        return Err(DIRECT_TRUST_USAGE.into());
    }
    let rest = &args[2.min(args.len())..];
    let (flags, positional): (Vec<&str>, Vec<&str>) = rest
        .iter()
        .map(String::as_str)
        .partition(|arg| arg.starts_with("--"));
    let replace = flags.contains(&"--replace");
    if let Some(unknown) = flags.iter().find(|flag| **flag != "--replace") {
        return Err(format!("unknown option `{unknown}`\n{DIRECT_TRUST_USAGE}"));
    }
    let command = match positional.as_slice() {
        ["identity"] => DirectTrustCliCommand::Identity,
        ["revoke", machine_id] => DirectTrustCliCommand::Revoke {
            machine_id: (*machine_id).to_string(),
        },
        ["provision", machine_id, public_key] => DirectTrustCliCommand::Provision {
            machine_id: (*machine_id).to_string(),
            public_key: (*public_key).to_string(),
            replace,
        },
        _ => return Err(DIRECT_TRUST_USAGE.into()),
    };
    if replace && !matches!(command, DirectTrustCliCommand::Provision { .. }) {
        return Err(format!("`--replace` only applies to provision\n{DIRECT_TRUST_USAGE}"));
    }
    Ok(command)
}

pub fn run_direct_trust_cli(command: DirectTrustCliCommand) -> Result<(), String> {
    let dir = crate::remote::auth::canonical_identity_dir()?;
    let stdout = std::io::stdout();
    run_direct_trust_cli_in(command, &dir, &mut stdout.lock())
}

/// Runs against an explicit `<data>/remote` directory so tests use a temp fixture.
/// The store at `dir` is the same file `DirectTrustStore::canonical()` resolves.
pub fn run_direct_trust_cli_in(
    command: DirectTrustCliCommand,
    dir: &Path,
    out: &mut impl Write,
) -> Result<(), String> {
    let dir = dir.to_path_buf();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(|e| format!("failed to start CLI runtime: {e}"))?;
    let line = runtime
        .block_on(crate::ipc::run_blocking(move || {
            apply(command, &dir).map_err(crate::ipc::IpcError::internal)
        }))
        .map_err(|e| e.message)?;
    writeln!(out, "{line}").map_err(|e| format!("failed to write output: {e}"))
}

/// Synchronous disk work; only ever invoked on a `run_blocking` worker.
fn apply(command: DirectTrustCliCommand, dir: &Path) -> Result<serde_json::Value, String> {
    let store = DirectTrustStore::at(dir);
    Ok(match command {
        DirectTrustCliCommand::Identity => {
            // Only the public half is printed; the private key never leaves this file.
            let identity = crate::remote::auth::load_or_generate_machine_identity(dir)?;
            serde_json::json!({
                "machineId": identity.machine_id,
                "publicKey": identity.public_key,
            })
        }
        DirectTrustCliCommand::Provision {
            machine_id,
            public_key,
            replace,
        } => {
            store
                .provision(&machine_id, &public_key, replace)
                .map_err(|e| e.to_string())?;
            serde_json::json!({ "provisioned": machine_id })
        }
        DirectTrustCliCommand::Revoke { machine_id } => {
            let removed = store.revoke(&machine_id).map_err(|e| e.to_string())?;
            serde_json::json!({ "machineId": machine_id, "revoked": removed })
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    fn key_b64(seed: u8) -> String {
        let key = ed25519_dalek::SigningKey::from_bytes(&[seed; 32]);
        STANDARD.encode(key.verifying_key().to_bytes())
    }

    fn run(dir: &Path, args: &[&str]) -> Result<serde_json::Value, String> {
        let command = parse_direct_trust_cli(args)?;
        let mut out = Vec::new();
        run_direct_trust_cli_in(command, dir, &mut out)?;
        serde_json::from_slice(&out).map_err(|e| e.to_string())
    }

    #[test]
    fn parses_every_subcommand_and_rejects_bad_shapes() {
        assert_eq!(
            parse_direct_trust_cli(["ferryx", "direct-trust", "identity"]),
            Ok(DirectTrustCliCommand::Identity)
        );
        assert_eq!(
            parse_direct_trust_cli(["ferryx", "direct-trust", "provision", "m-1", "KEY", "--replace"]),
            Ok(DirectTrustCliCommand::Provision {
                machine_id: "m-1".into(),
                public_key: "KEY".into(),
                replace: true,
            })
        );
        assert_eq!(
            parse_direct_trust_cli(["ferryx", "direct-trust", "revoke", "m-1"]),
            Ok(DirectTrustCliCommand::Revoke { machine_id: "m-1".into() })
        );
        assert!(parse_direct_trust_cli(["ferryx", "direct-trust"]).is_err());
        assert!(parse_direct_trust_cli(["ferryx", "direct-trust", "provision", "m-1"]).is_err());
        assert!(parse_direct_trust_cli(["ferryx", "direct-trust", "revoke", "m-1", "--replace"]).is_err());
        assert!(parse_direct_trust_cli(["ferryx", "direct-trust", "list"]).is_err());
        assert!(parse_direct_trust_cli(["ferryx", "direct-trust", "identity", "--bogus"]).is_err());
    }

    #[test]
    fn identity_prints_only_public_fields_and_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let first = run(dir.path(), &["ferryx", "direct-trust", "identity"]).unwrap();
        let object = first.as_object().unwrap();
        assert_eq!(object.len(), 2);
        assert!(object.contains_key("machineId") && object.contains_key("publicKey"));
        let stored = crate::remote::auth::load_or_generate_machine_identity(dir.path()).unwrap();
        assert_eq!(first["publicKey"], stored.public_key);
        let second = run(dir.path(), &["ferryx", "direct-trust", "identity"]).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn provision_pins_conflicts_replaces_and_revokes() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (key_b64(1), key_b64(2));
        run(dir.path(), &["ferryx", "direct-trust", "provision", "peer-1", &a]).unwrap();
        let conflict = run(dir.path(), &["ferryx", "direct-trust", "provision", "peer-1", &b]);
        assert_eq!(
            conflict,
            Err(crate::paired_host::direct_trust::TrustError::PinConflict.to_string())
        );
        run(dir.path(), &["ferryx", "direct-trust", "provision", "peer-1", &b, "--replace"]).unwrap();
        assert_eq!(
            DirectTrustStore::at(dir.path()).list().unwrap(),
            vec![("peer-1".to_string(), b.clone())]
        );
        let revoked = run(dir.path(), &["ferryx", "direct-trust", "revoke", "peer-1"]).unwrap();
        assert_eq!(revoked["revoked"], true);
        let again = run(dir.path(), &["ferryx", "direct-trust", "revoke", "peer-1"]).unwrap();
        assert_eq!(again["revoked"], false);
        assert!(run(dir.path(), &["ferryx", "direct-trust", "provision", "peer-1", "not-a-key"]).is_err());
    }

    #[test]
    fn cli_revoke_tombstones_and_manual_provision_clears_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = DirectTrustStore::at(dir.path());
        store.provision_from_account("peer-2", &key_b64(3), 1).unwrap();
        let revoked = run(dir.path(), &["ferryx", "direct-trust", "revoke", "peer-2"]).unwrap();
        assert_eq!(revoked["revoked"], true);
        assert_eq!(
            store.provision_from_account("peer-2", &key_b64(3), 2),
            Err(crate::paired_host::direct_trust::TrustError::Revoked)
        );
        let out = run(dir.path(), &["ferryx", "direct-trust", "provision", "peer-2", &key_b64(3)]).unwrap();
        assert_eq!(out, serde_json::json!({ "provisioned": "peer-2" }));
        assert!(store.trusted_key("peer-2").unwrap().is_some());
    }
}
