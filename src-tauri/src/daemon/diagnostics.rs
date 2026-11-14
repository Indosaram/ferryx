//! Diagnostics a failed daemon connect or supervisor start carries to the UI and the CLI.
//!
//! Everything here is copied by the user into bug reports, so every free-text value passes
//! through [`redact_secrets`] before it leaves the process.

use regex::{Captures, Regex};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::OnceLock;

const REDACTED: &str = "[redacted]";

struct Redactors {
    pem: Regex,
    install_key: Regex,
    authorization: Regex,
    bearer: Regex,
    pair: Regex,
    long_run: Regex,
}

fn redactors() -> &'static Redactors {
    static REDACTORS: OnceLock<Redactors> = OnceLock::new();
    REDACTORS.get_or_init(|| Redactors {
        pem: Regex::new(r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----")
            .expect("pem pattern"),
        install_key: Regex::new(r"fxk_[A-Za-z0-9]+").expect("install key pattern"),
        authorization: Regex::new(r#"(?i)(authorization"?\s*[:=]\s*"?)[^\r\n"]+"#)
            .expect("authorization pattern"),
        bearer: Regex::new(r"(?i)(bearer\s+)[A-Za-z0-9_.~+/=-]+").expect("bearer pattern"),
        pair: Regex::new(
            r#"(?i)("?[A-Za-z0-9_-]*(?:token|secret|password|passwd|api[_-]?key|auth|cookie|private_key)[A-Za-z0-9_-]*"?\s*[:=]\s*)("[^"]*"|[^\s,;&"']+)"#,
        )
        .expect("pair pattern"),
        long_run: Regex::new(r"[A-Za-z0-9+_=-]{32,}").expect("long run pattern"),
    })
}

/// Replaces anything secret-looking (install keys, bearer tokens, authorization headers,
/// `token=`/`"password":` style pairs, PEM private keys, long token-like runs) with
/// `[redacted]`. Paths and ordinary messages are left as they are.
pub(crate) fn redact_secrets(text: &str) -> String {
    let r = redactors();
    let text = r.pem.replace_all(text, REDACTED);
    let text = r.install_key.replace_all(&text, REDACTED);
    let text = r.authorization.replace_all(&text, format!("${{1}}{REDACTED}").as_str());
    let text = r.bearer.replace_all(&text, format!("${{1}}{REDACTED}").as_str());
    let text = r.pair.replace_all(&text, |caps: &Captures<'_>| {
        let value = if caps[2].starts_with('"') {
            format!("\"{REDACTED}\"")
        } else {
            REDACTED.to_string()
        };
        format!("{}{value}", &caps[1])
    });
    r.long_run
        .replace_all(&text, |caps: &Captures<'_>| {
            let run = &caps[0];
            let letter = run.chars().any(|c| c.is_ascii_alphabetic());
            let digit = run.chars().any(|c| c.is_ascii_digit());
            if letter && digit {
                REDACTED.to_string()
            } else {
                run.to_string()
            }
        })
        .into_owned()
}

/// [`redact_secrets`] for an optional value, as JSON (`null` when absent).
pub(crate) fn redacted_value(text: Option<&str>) -> Value {
    text.map_or(Value::Null, |text| Value::String(redact_secrets(text)))
}

/// Who owns the runtime directory and the socket, and why the client would refuse them.
#[cfg(unix)]
pub(crate) fn runtime_ownership_for_uid(socket_path: &Path, expected_uid: u32) -> Value {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let runtime_dir = socket_path.parent();
    let dir_meta = runtime_dir.and_then(|dir| std::fs::symlink_metadata(dir).ok());
    let socket_meta = std::fs::symlink_metadata(socket_path).ok();
    let problem = crate::daemon::server::validate_runtime_socket_path_for_uid(
        socket_path,
        expected_uid,
    )
    .err();
    json!({
        "runtimeDir": runtime_dir.map(|dir| dir.display().to_string()),
        "exists": dir_meta.is_some(),
        "isSymlink": dir_meta.as_ref().is_some_and(|meta| meta.file_type().is_symlink()),
        "ownerUid": dir_meta.as_ref().map(MetadataExt::uid),
        "expectedUid": expected_uid,
        "mode": dir_meta.as_ref().map(|meta| format!("{:o}", meta.permissions().mode() & 0o777)),
        "socketExists": socket_meta.is_some(),
        "socketOwnerUid": socket_meta.as_ref().map(MetadataExt::uid),
        "problem": problem,
    })
}

/// [`runtime_ownership_for_uid`] for this process's uid.
#[cfg(unix)]
pub(crate) fn runtime_ownership(socket_path: &Path) -> Value {
    // SAFETY: `getuid` takes no arguments and returns the current process UID.
    runtime_ownership_for_uid(socket_path, unsafe { libc::getuid() })
}

#[cfg(not(unix))]
pub(crate) fn runtime_ownership(_socket_path: &Path) -> Value {
    Value::Null
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_redacted_and_paths_survive() {
        let key = format!("fxk_{}", "ab12".repeat(16));
        let cases = [
            (format!("install key {key} failed"), key.clone()),
            ("Authorization: Bearer abc.def.ghi".to_string(), "abc.def.ghi".to_string()),
            ("curl -H 'Bearer tok_42xyz'".to_string(), "tok_42xyz".to_string()),
            (r#"{"token":"s3cr3t-value"}"#.to_string(), "s3cr3t-value".to_string()),
            ("password=hunter2 next".to_string(), "hunter2".to_string()),
            ("api_key: K9zz".to_string(), "K9zz".to_string()),
            (
                "-----BEGIN OPENSSH PRIVATE KEY-----\nAAAAkey\n-----END OPENSSH PRIVATE KEY-----".to_string(),
                "AAAAkey".to_string(),
            ),
            (format!("opaque {}", "Zx9".repeat(12)), "Zx9Zx9Zx9Zx9".to_string()),
        ];
        for (input, secret) in cases {
            let redacted = redact_secrets(&input);
            assert!(!redacted.contains(&secret), "{input:?} leaked into {redacted:?}");
            assert!(redacted.contains(REDACTED), "{input:?} -> {redacted:?}");
        }
        assert_eq!(
            redact_secrets(r#"{"token":"x1"}"#),
            r#"{"token":"[redacted]"}"#,
            "a JSON pair stays valid JSON"
        );
        for plain in [
            "/tmp/ferryx-1000/daemon.sock",
            "/home/u/.ferryx/logs/daemon.log",
            "Daemon connection rejected: transport token missing or invalid",
            "Path /tmp/ferryx-1000 is owned by UID 0 (expected current UID 1000)",
            r#"{"type":"daemonConnectFailure","stage":"spawn"}"#,
        ] {
            assert_eq!(redact_secrets(plain), plain);
        }
    }

    #[cfg(unix)]
    #[test]
    fn ownership_names_the_refusal() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("rt");
        std::fs::create_dir(&runtime).unwrap();
        std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = runtime.join("daemon.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        // SAFETY: `getuid` takes no arguments and returns the current process UID.
        let uid = unsafe { libc::getuid() };

        let own = runtime_ownership_for_uid(&socket, uid);
        assert_eq!(own["problem"], Value::Null);
        assert_eq!(own["mode"], "700");
        assert_eq!(own["ownerUid"], uid);
        assert_eq!(own["socketExists"], true);

        let foreign = runtime_ownership_for_uid(&socket, uid.wrapping_add(1));
        let problem = foreign["problem"].as_str().expect("a refusal");
        assert!(problem.contains(&runtime.display().to_string()), "{problem}");
        assert!(problem.contains("UID"), "{problem}");
    }
}
