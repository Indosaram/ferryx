use super::*;
use crate::ssh::{SshAuthMethod, SshHostSource};

fn host() -> SshHost {
    SshHost {
        id: "host-one".into(),
        label: "Test host".into(),
        hostname: "example.test".into(),
        username: Some("user".into()),
        port: Some(2200),
        identity_file: Some("/keys/key with 'quote'".into()),
        jump_host: Some("jump-user@bastion:2222".into()),
        source: SshHostSource::Manual,
        auth_method: SshAuthMethod::Key,
        disabled: None,
    }
}

#[test]
fn startup_plan_honors_saved_options_and_quotes_remote_root() {
    let plan = shell_plan(&host(), "/srv/project's space").expect("plan");
    assert_eq!(plan.program, "ssh");
    assert!(plan.args.iter().any(|v| v == "-tt"));
    for pair in [
        ["-p", "2200"],
        ["-i", "/keys/key with 'quote'"],
        ["-J", "jump-user@bastion:2222"],
    ] {
        assert!(plan.args.windows(2).any(|args| args == pair));
    }
    assert_eq!(plan.args[plan.args.len() - 2], "user@example.test");
    assert_eq!(
        plan.args.last().unwrap(),
        "cd '/srv/project'\\''s space' && exec \"${SHELL:-/bin/sh}\" -l"
    );
}

#[test]
fn unsafe_connection_tokens_and_paths_are_rejected() {
    for hostname in [
        "-oProxyCommand=touch",
        "host;touch",
        "host name",
        "host\n",
        "$(whoami)",
    ] {
        let mut h = host();
        h.hostname = hostname.into();
        assert!(shell_plan(&h, "/srv").is_err(), "{hostname}");
    }
    for key in ["-option", "", "key\n", "relative-key"] {
        let mut h = host();
        h.identity_file = Some(key.into());
        assert!(validate_host(&h).is_err());
    }
    let mut h = host();
    h.port = Some(0);
    assert!(validate_host(&h).is_err());
    h = host();
    h.jump_host = Some("-oProxyCommand=evil".into());
    assert!(validate_host(&h).is_err());
    h = host();
    h.username = Some("user;evil".into());
    assert!(validate_host(&h).is_err());
    for path in [
        "",
        "-option",
        "relative",
        "~/repo",
        "/path\nnext",
        "/path\0",
    ] {
        assert!(probe_command(path).is_err(), "{path:?}");
    }
}

#[cfg(unix)]
#[test]
fn remote_shell_probe_canonicalizes_quoted_directory_and_reports_plain_or_git() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("space ' apostrophe -option");
    std::fs::create_dir(&root).unwrap();
    let command = probe_command(root.to_str().unwrap()).unwrap();
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", &command])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        parse_probe(&output.stdout).unwrap(),
        (root.canonicalize().unwrap().to_str().unwrap().into(), None)
    );
    assert!(std::process::Command::new("git")
        .arg("init")
        .arg(&root)
        .output()
        .unwrap()
        .status
        .success());
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", &command])
        .output()
        .unwrap();
    let (actual, git) = parse_probe(&output.stdout).unwrap();
    assert_eq!(git, Some(actual));
}

#[test]
fn malformed_probe_output_cannot_become_a_local_project() {
    for output in [
        b"/local/path".as_slice(),
        b"FERRYX_REMOTE_V1\0relative\0\0",
        b"FERRYX_REMOTE_V1\0/remote\0\0extra",
    ] {
        assert!(parse_probe(output).is_err());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn failed_probe_reports_exit_and_caps_output() {
    let plan = ShellCommandPlan {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "printf 'denied' >&2; exit 7".into()],
    };
    assert_eq!(
        bounded_output(&plan, Duration::from_secs(2))
            .await
            .unwrap_err()
            .code,
        IpcErrorCode::InvalidPath
    );
    let plan = ShellCommandPlan {
        program: "/usr/bin/yes".into(),
        args: vec![],
    };
    assert_eq!(
        bounded_output(&plan, Duration::from_secs(2))
            .await
            .unwrap_err()
            .code,
        IpcErrorCode::IoError
    );
}

#[cfg(unix)]
#[tokio::test]
async fn deadline_terminates_and_reaps_probe() {
    // Time itself is under test. PID ownership is known before starting the deadline.
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("blocked");
    assert!(std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    let child = tokio::process::Command::new("/bin/sh")
        .args([
            "-c",
            &format!("read line < {}", quote_posix(fifo.to_str().unwrap())),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let pid = child.id().unwrap().to_string();
    assert_eq!(
        collect_output(child, Duration::from_millis(20))
            .await
            .unwrap_err()
            .code,
        IpcErrorCode::InvalidPath
    );
    let output = std::process::Command::new("ps")
        .args(["-p", &pid, "-o", "pid="])
        .output()
        .unwrap();
    assert!(!output.status.success(), "probe process survived timeout");
}
