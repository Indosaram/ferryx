use super::*;
use crate::ssh::{SshAuthMethod, SshHostSource};

const KEY_A: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIHN/xU5oToJg6KAiB2Jpff7Vlf244K0USyyxReXdGMTQ";
/// `ssh-keygen -lf` output for KEY_A.
const FINGERPRINT_A: &str = "SHA256:QC8NK7dHes+PsoxIbE/WpRKLdTd8DwAUUbcIg2HU4pg";
const KEY_B: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIILuvDE2XOi43O+bFOV7VIj+oOYjtNZzl+BV5FPSM1bk";
const FINGERPRINT_B: &str = "SHA256:5wCd5F1DRG+7lrYwg8ZI4vy1yfKsc5/4QMrBteles4M";

fn host() -> SshHost {
    SshHost {
        id: "h".into(),
        label: "l".into(),
        hostname: "example.test".into(),
        username: Some("dev".into()),
        port: Some(2222),
        identity_file: None,
        jump_host: Some("bastion".into()),
        source: SshHostSource::Manual,
        auth_method: SshAuthMethod::Agent,
        disabled: None,
    }
}

fn scanned(key: &str) -> Vec<ScannedHostKey> {
    parse_scan(&format!("[example.test]:2222 ssh-ed25519 {key}\n"))
}

#[test]
fn scan_reports_openssh_fingerprints_and_the_exact_line() {
    assert_eq!(
        scanned(KEY_A),
        vec![ScannedHostKey {
            key_type: "ssh-ed25519".into(),
            fingerprint_sha256: FINGERPRINT_A.into(),
            line: format!("[example.test]:2222 ssh-ed25519 {KEY_A}"),
        }]
    );
    assert_eq!(scanned(KEY_B)[0].fingerprint_sha256, FINGERPRINT_B);
    assert!(
        parse_scan("# comment\n|1|abc|def ssh-ed25519 AAAA\nhost ssh-ed25519 !!notbase64\n")
            .is_empty()
    );
}

#[test]
fn trust_refuses_a_mismatched_fingerprint_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let known_hosts = dir.path().join(".ssh").join("known_hosts");
    let error = trust_into(&known_hosts, &scanned(KEY_B), FINGERPRINT_A).unwrap_err();
    assert_eq!(error.code, IpcErrorCode::HostKeyChanged);
    assert_eq!(error.details.unwrap()["stage"], "host_key");
    assert!(!known_hosts.exists());
}

#[test]
fn trust_appends_the_confirmed_line_once() {
    let dir = tempfile::tempdir().unwrap();
    let known_hosts = dir.path().join("known_hosts");
    std::fs::write(&known_hosts, "other.test ssh-ed25519 AAAA").unwrap();
    let line = trust_into(&known_hosts, &scanned(KEY_A), FINGERPRINT_A).unwrap();
    assert_eq!(
        std::fs::read_to_string(&known_hosts).unwrap(),
        format!("other.test ssh-ed25519 AAAA\n{line}\n")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let fresh = dir.path().join("fresh");
        trust_into(&fresh, &scanned(KEY_A), FINGERPRINT_A).unwrap();
        assert_eq!(
            std::fs::metadata(&fresh).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    // A second trust finds the entry it wrote and refuses to stack another key next to it.
    let error = trust_into(&known_hosts, &scanned(KEY_A), FINGERPRINT_A).unwrap_err();
    assert_eq!(error.code, IpcErrorCode::HostKeyChanged);
}

#[test]
fn trust_never_adds_a_key_beside_an_existing_entry() {
    let existing = format!("[example.test]:2222 ssh-ed25519 {KEY_B}\n");
    assert!(line_to_trust(&scanned(KEY_A), FINGERPRINT_A, &existing).is_err());
    let revoked = format!("@revoked [example.test]:2222 ssh-ed25519 {KEY_A}\n");
    assert!(line_to_trust(&scanned(KEY_A), FINGERPRINT_A, &revoked).is_err());
    // `ssh-keygen -H` form of "[example.test]:2222" with salt 0x00..00.
    use hmac::{Hmac, Mac};
    let salt = [0u8; 20];
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(&salt).unwrap();
    mac.update(b"[example.test]:2222");
    let hashed = format!(
        "|1|{}|{} ssh-ed25519 {KEY_B}\n",
        STANDARD.encode(salt),
        STANDARD.encode(mac.finalize().into_bytes())
    );
    assert!(line_to_trust(&scanned(KEY_A), FINGERPRINT_A, &hashed).is_err());
    assert!(line_to_trust(
        &scanned(KEY_A),
        FINGERPRINT_A,
        "example.test ssh-ed25519 AAAA\n"
    )
    .is_ok());
}

#[test]
fn scan_plan_accepts_new_keys_only_into_the_private_file() {
    let file = std::path::Path::new(if cfg!(windows) {
        r"C:\tmp dir\known_hosts"
    } else {
        "/tmp dir/known_hosts"
    });
    let plan = scan_plan(&host(), file).unwrap();
    let expected = if cfg!(windows) {
        "\"C:/tmp dir/known_hosts\""
    } else {
        "\"/tmp dir/known_hosts\""
    };
    assert_eq!(
        plan.args[..2],
        ["-o".to_string(), format!("UserKnownHostsFile={expected}")]
    );
    assert!(plan
        .args
        .windows(2)
        .any(|a| a == ["-o", "StrictHostKeyChecking=accept-new"]));
    assert!(!plan.args.iter().any(|a| a == "StrictHostKeyChecking=yes"));
    assert!(plan.args.windows(2).any(|a| a == ["-J", "bastion"]));
    assert_eq!(plan.args.last().map(String::as_str), Some("exit 0"));
    // Ordinary sessions stay strict.
    let ordinary = super::super::direct::ssh_plan(&host(), "true".into(), false).unwrap();
    assert!(ordinary
        .args
        .iter()
        .any(|a| a == "StrictHostKeyChecking=yes"));
    assert!(!ordinary.args.iter().any(|a| a.contains("accept-new")));
}

/// Asserts `option` is passed as `-o option` before any other value of the same OpenSSH option, so
/// it wins over the plan's own options and over ssh_config.
fn assert_first_value(args: &[String], option: &str) {
    let name = option.split('=').next().unwrap();
    let first = args
        .iter()
        .position(|arg| arg.split('=').next() == Some(name))
        .unwrap_or_else(|| panic!("{name} missing from {args:?}"));
    assert_eq!(args[first], option, "{args:?}");
    assert_eq!(args[first - 1], "-o", "{args:?}");
}

#[test]
fn scan_and_probe_never_authenticate_forward_or_reach_the_stored_password() {
    let password_host = SshHost {
        hostname: "scan-password.example.test".into(),
        auth_method: SshAuthMethod::Password,
        ..host()
    };
    crate::ssh::password::set(
        &password_host,
        crate::ssh::password::Password::new("secret".into()),
    )
    .unwrap();
    let file = std::path::Path::new("/tmp/known_hosts");
    for target in [host(), password_host.clone()] {
        let ordinary = super::super::direct::ssh_plan(&target, "exit 0".into(), false).unwrap();
        let plans = [
            scan_plan(&target, file).unwrap(),
            unauthenticated_plan(&target).unwrap(),
        ];
        for plan in plans {
            for option in UNAUTHENTICATED {
                assert_first_value(&plan.args, option);
            }
            assert!(
                !plan
                    .args
                    .iter()
                    .any(|arg| PASSWORD_ONLY.contains(&arg.as_str())),
                "{:?}",
                plan.args
            );
            assert!(crate::ssh::password::environment(&plan.args)
                .unwrap()
                .is_empty());
            assert!(plan.args.windows(2).any(|a| a == ["-J", "bastion"]));
            assert_eq!(plan.args.last().map(String::as_str), Some("exit 0"));
        }
        if target.auth_method == SshAuthMethod::Password {
            // The ordinary plan for the same host would hand the askpass helper to ssh.
            assert!(!crate::ssh::password::environment(&ordinary.args)
                .unwrap()
                .is_empty());
        }
    }
    // The probe keeps OpenSSH's strict check: it only reports, it never records a key.
    let probe = unauthenticated_plan(&password_host).unwrap();
    assert_first_value(&probe.args, "StrictHostKeyChecking=yes");
    assert!(!probe
        .args
        .iter()
        .any(|arg| arg.contains("accept-new") || arg.contains("KnownHostsFile")));
    crate::ssh::password::clear(&password_host).unwrap();
}

#[test]
fn only_openssh_new_host_line_lets_trust_proceed() {
    let failed = |stderr: &str| -> Result<Vec<u8>, IpcError> {
        Err(IpcError::new(IpcErrorCode::IoError, "ssh failed")
            .with_details(serde_json::json!({ "exitCode": 255, "stderr": stderr })))
    };
    assert!(probe_reports_new_host(&failed(
        "No ED25519 host key is known for [127.0.0.1]:2222 and you have requested strict checking.\r\nHost key verification failed.\r\n"
    )));
    // A changed key (user, global or wildcard entry), the bare LogLevel FATAL line, an auth failure
    // after a matching key, a session that opened, or a probe without stderr: never new.
    assert!(!probe_reports_new_host(&failed(
        "@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\nHost key for [127.0.0.1]:2222 has changed and you have requested strict checking.\nHost key verification failed.\n"
    )));
    assert!(!probe_reports_new_host(&failed(
        "Host key verification failed.\r\n"
    )));
    assert!(!probe_reports_new_host(&failed(
        "dev@127.0.0.1: Permission denied (publickey,password).\r\n"
    )));
    assert!(!probe_reports_new_host(&Ok(Vec::new())));
    assert!(!probe_reports_new_host(&Err(IpcError::new(
        IpcErrorCode::IoError,
        "spawn failed"
    ))));
}

#[test]
fn trust_target_is_the_first_user_known_hosts_file_ssh_reports() {
    let none = |_: &str| false;
    let posix = "user dev\nuserknownhostsfile /home/dev/.ssh/known_hosts /home/dev/.ssh/known_hosts2\nloglevel INFO\n";
    assert_eq!(
        first_user_known_hosts(posix, none).as_deref(),
        Some("/home/dev/.ssh/known_hosts")
    );
    let custom = "userknownhostsfile /srv/pins/dev /home/dev/.ssh/known_hosts\n";
    assert_eq!(
        first_user_known_hosts(custom, none).as_deref(),
        Some("/srv/pins/dev")
    );
    // `ssh -G` does not quote; a Windows profile with a space is rebuilt from its words.
    let windows = "userknownhostsfile C:\\Users\\Jane Doe/.ssh/known_hosts C:\\Users\\Jane Doe/.ssh/known_hosts2\r\n";
    assert_eq!(
        first_user_known_hosts(windows, none).as_deref(),
        Some("C:\\Users\\Jane Doe/.ssh/known_hosts")
    );
    let spaced = "userknownhostsfile /home/dev/my pins/kh /home/dev/c\n";
    let exists = |path: &str| path == "/home/dev/my pins/kh";
    assert_eq!(
        first_user_known_hosts(spaced, exists).as_deref(),
        Some("/home/dev/my pins/kh")
    );
    // A custom first file that does not exist yet is never joined with the entry after it.
    let missing = "userknownhostsfile /srv/my pins/dev /home/dev/.ssh/known_hosts\n";
    assert_eq!(
        first_user_known_hosts(missing, none).as_deref(),
        Some("/srv/my pins/dev")
    );
    assert_eq!(
        first_user_known_hosts("userknownhostsfile none\n", none),
        None
    );
    assert_eq!(first_user_known_hosts("loglevel INFO\n", none), None);
}

fn qa_env(name: &str) -> Option<String> {
    std::env::var(format!("FERRYX_HOSTKEY_QA_{name}"))
        .ok()
        .filter(|value| !value.is_empty())
}

/// Live first-contact flow against a real sshd. The caller wraps `ssh` with a private `-F` config
/// (its "user ssh_config", which names the scratch known_hosts files) and sets FERRYX_HOSTKEY_QA_*:
/// PORT, IDENTITY, KNOWN_HOSTS (the file that config's UserKnownHostsFile names first), optional
/// PASSWORD (password-auth host) and EXPECT:
/// `trust` (new host: wrong fingerprint refused, confirmed key pinned, second test reachable),
/// `scan` (first test + scan only; the caller checks the sshd log for authentication attempts),
/// `refuse` (first test must report CODE; trust of the scanned key must be refused).
#[tokio::test]
#[ignore = "requires a live sshd on 127.0.0.1 and a private ssh_config; see FERRYX_HOSTKEY_QA_*"]
async fn live_first_contact_scan_and_trust() {
    let port: u16 = qa_env("PORT").unwrap().parse().unwrap();
    let password = qa_env("PASSWORD");
    let host = SshHost {
        id: "hostkey-qa".into(),
        label: "hostkey-qa".into(),
        hostname: "127.0.0.1".into(),
        username: None,
        port: Some(port),
        identity_file: qa_env("IDENTITY"),
        jump_host: None,
        source: SshHostSource::Manual,
        auth_method: if password.is_some() {
            SshAuthMethod::Password
        } else {
            SshAuthMethod::Key
        },
        disabled: None,
    };
    if let Some(password) = password {
        crate::ssh::password::set(&host, crate::ssh::password::Password::new(password)).unwrap();
    }
    let known_hosts = std::path::PathBuf::from(qa_env("KNOWN_HOSTS").unwrap());
    let first = crate::ipc::ssh::ssh_test_connection_summary(host.clone()).await;
    let diagnostic = first.diagnostic.expect("first contact must fail");
    println!(
        "QA first test: code={} details={}",
        diagnostic.code,
        diagnostic.details.clone().unwrap_or_default()
    );
    let scanned = scan(&host).await.unwrap();
    println!("QA scan: {}", serde_json::to_string(&scanned).unwrap());
    match qa_env("EXPECT").unwrap().as_str() {
        "trust" => {
            assert_eq!(diagnostic.code, IpcErrorCode::HostKeyUnknown);
            println!(
                "QA trust target: {}",
                known_hosts_target(&host).await.unwrap().display()
            );
            let wrong = trust(
                &host,
                "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
            )
            .await
            .unwrap_err();
            println!("QA mismatched trust rejected: code={}", wrong.code);
            assert_eq!(wrong.code, IpcErrorCode::HostKeyChanged);
            assert!(!known_hosts.exists());
            trust(&host, scanned.keys[0].fingerprint_sha256.clone())
                .await
                .unwrap();
            println!(
                "QA known_hosts after trust: {}",
                std::fs::read_to_string(&known_hosts).unwrap().trim()
            );
            let second = crate::ipc::ssh::ssh_test_connection_summary(host).await;
            println!(
                "QA second test: reachable={} environment={:?}",
                second.reachable, second.environment
            );
            assert!(second.reachable, "{:?}", second.diagnostic);
        }
        "scan" => assert_eq!(diagnostic.code, IpcErrorCode::HostKeyUnknown),
        "refuse" => {
            assert_eq!(diagnostic.code.to_string(), qa_env("CODE").unwrap());
            let before = std::fs::read(&known_hosts).ok();
            let refused = trust(&host, scanned.keys[0].fingerprint_sha256.clone())
                .await
                .unwrap_err();
            println!(
                "QA trust refused: code={} message={}",
                refused.code, refused.message
            );
            assert_eq!(refused.code, IpcErrorCode::HostKeyChanged);
            assert_eq!(std::fs::read(&known_hosts).ok(), before);
        }
        other => panic!("unknown FERRYX_HOSTKEY_QA_EXPECT {other}"),
    }
}
