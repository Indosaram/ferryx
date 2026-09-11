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
fn ssh_reconnect_safety_plan_does_not_update_host_keys() {
    for interactive in [false, true] {
        let plan = ssh_plan(&host(), "true".into(), interactive).unwrap();
        assert!(plan.args.windows(2).any(|args| args == ["-o", "UpdateHostKeys=no"]));
    }
}

#[test]
fn automated_commands_disable_tty_even_when_ssh_config_requests_one() {
    let plan = ssh_plan(&host(), "echo probe".into(), false).unwrap();
    assert!(plan.args.iter().any(|arg| arg == "-T"));
    assert!(!plan.args.iter().any(|arg| arg == "-tt"));
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
fn upload_temp_command_rejects_names_that_could_escape_the_scratch_directory() {
    for name in [
        "",
        ".hidden.png",
        "../escape.png",
        "na me.png",
        "name$(id).png",
        "name';touch x;'.png",
        &"a".repeat(129),
    ] {
        assert!(
            upload_temp_command(name).is_err(),
            "expected rejection for {name:?}"
        );
    }
    assert!(upload_temp_command("7f3a-9b.png").is_ok());
}

#[cfg(unix)]
#[test]
fn upload_temp_command_writes_stdin_to_a_private_file_and_prints_its_path() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let scratch = tempfile::tempdir().expect("tempdir");
    let command = upload_temp_command("paste-1.png").expect("command");
    let mut child = std::process::Command::new("sh")
        .arg("-c")
        .arg(&command)
        .env("TMPDIR", scratch.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("run upload command");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(&[0x89, b'P', b'N', b'G', 0x00, 0xff])
        .expect("write payload");
    let output = child.wait_with_output().expect("wait");
    assert!(output.status.success());

    let printed = String::from_utf8(output.stdout).expect("utf-8 path");
    assert_eq!(
        printed,
        format!("{}/ferryx-paste/paste-1.png", scratch.path().display())
    );
    assert!(validate_remote_path(&printed).is_ok());
    assert_eq!(
        std::fs::read(&printed).expect("read uploaded file"),
        vec![0x89, b'P', b'N', b'G', 0x00, 0xff]
    );
    let mode = std::fs::metadata(scratch.path().join("ferryx-paste"))
        .expect("scratch metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
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
        (root.canonicalize().unwrap().to_str().unwrap().into(), None, None)
    );
    assert!(std::process::Command::new("git")
        .arg("init")
        .arg(&root)
        .output()
        .unwrap()
        .status
        .success());
    assert!(std::process::Command::new("git")
        .args(["remote", "add", "origin", "https://github.com/example/test.git"])
        .current_dir(&root)
        .output()
        .unwrap()
        .status
        .success());
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", &command])
        .output()
        .unwrap();
    let (actual, git, remote) = parse_probe(&output.stdout).unwrap();
    assert_eq!(git, Some(actual));
    assert_eq!(remote, Some("https://github.com/example/test.git".to_string()));
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
        IpcErrorCode::IoError
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
        IpcErrorCode::IoError
    );
    let output = std::process::Command::new("ps")
        .args(["-p", &pid, "-o", "pid="])
        .output()
        .unwrap();
    assert!(!output.status.success(), "probe process survived timeout");
}

#[test]
fn shell_plan_with_session_forwards_agent_state_socket_and_exports_env() {
    let plan = shell_plan_with_session(
        &host(),
        "/srv/repo",
        Some("session-123"),
        Some("/tmp/local-agent.sock"),
    )
    .expect("plan");

    assert_eq!(plan.program, "ssh");
    assert!(plan.args.windows(2).any(|pair| pair == ["-o", "StreamLocalBindUnlink=yes"]));
    assert!(plan.args.windows(2).any(|pair| pair == ["-R", "/tmp/ferryx-agent-session-123.sock:/tmp/local-agent.sock"]));

    let remote_cmd = plan.args.last().unwrap();
    assert!(remote_cmd.contains("export FERRYX_SESSION_ID='session-123'"));
    assert!(remote_cmd.contains("FERRYX_AGENT_STATE_SOCKET='/tmp/ferryx-agent-session-123.sock'"));
    assert!(remote_cmd.contains("cd '/srv/repo'"));
}

#[cfg(unix)]
#[test]
fn install_remote_extension_script_creates_and_populates_extension() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let omo_dir = home.join(".omo");
    std::fs::create_dir_all(&omo_dir).unwrap();

    let script = install_remote_extension_script();
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", &script])
        .env("HOME", home)
        .output()
        .unwrap();

    assert!(output.status.success(), "script stderr: {}", String::from_utf8_lossy(&output.stderr));

    let installed_file = home.join(".omo/agent/extensions/ferryx-agent-state.ts");
    assert!(installed_file.is_file(), "extension file must be installed");
    let content = std::fs::read_to_string(&installed_file).unwrap();
    assert_eq!(content, crate::daemon::agent_extension::EXTENSION_SOURCE);
}

#[test]
fn ssh_bridge_plan_options_disable_tty_and_enforce_strict_host_keys() {
    let env = crate::ssh::runtime::RemoteEnvironment {
        platform: crate::ssh::runtime::RemotePlatform::Posix,
        executor: crate::ssh::runtime::RemoteExecutor::Sh,
        version: "Linux 6.1".into(),
        home: "/home/user".into(),
        temp: "/tmp".into(),
        git: true,
    };
    let location = crate::ssh::helper_setup::HelperLocation {
        executable: "/home/user/.ferryx/bin/ferryx-remote-helper".into(),
        root: "/home/user/.ferryx/helper/host-one".into(),
    };
    let plan = bridge_plan(&host(), &env, &location).expect("bridge_plan");
    assert_eq!(plan.program, "ssh");
    assert!(plan.args.iter().any(|v| v == "-T"));
    assert!(!plan.args.iter().any(|v| v == "-tt"));
    assert!(plan.args.windows(2).any(|pair| pair == ["-o", "UpdateHostKeys=no"]));
    assert!(plan.args.windows(2).any(|pair| pair == ["-o", "StrictHostKeyChecking=yes"]));
    assert!(plan.args.windows(2).any(|pair| pair == ["-o", "BatchMode=yes"]));
    assert!(plan.args.windows(2).any(|pair| pair == ["-o", "ClearAllForwardings=yes"]));
    let remote_cmd = plan.args.last().unwrap();
    assert!(remote_cmd.contains("bridge --stdio --root"));
}

#[test]
fn ssh_bridge_plan_windows_uses_raw_child_stdio_forwarding() {
    let env = crate::ssh::runtime::RemoteEnvironment {
        platform: crate::ssh::runtime::RemotePlatform::Windows,
        executor: crate::ssh::runtime::RemoteExecutor::Powershell,
        version: "Windows 10.0.19045".into(),
        home: "C:\\Users\\user".into(),
        temp: "C:\\Temp".into(),
        git: true,
    };
    let location = crate::ssh::helper_setup::HelperLocation {
        executable: "C:\\Users\\user\\.ferryx\\bin\\ferryx-remote-helper.exe".into(),
        root: "C:\\Users\\user\\.ferryx\\helper\\host-win".into(),
    };
    let plan = bridge_plan(&host(), &env, &location).expect("bridge_plan");
    assert_eq!(plan.program, "ssh");
    assert!(plan.args.iter().any(|v| v == "-T"));
    assert!(plan.args.windows(2).any(|pair| pair == ["-o", "UpdateHostKeys=no"]));
    assert!(plan.args.windows(2).any(|pair| pair == ["-o", "StrictHostKeyChecking=yes"]));
    let cmd = bridge_command(&env, &location);
    assert!(cmd.contains("System.Diagnostics.Process"));
    assert!(cmd.contains("RedirectStandardInput = $false"));
    assert!(cmd.contains("RedirectStandardOutput = $false"));
    assert!(cmd.contains("RedirectStandardError = $false"));
    assert!(cmd.contains("bridge --stdio --root"));
}

#[cfg(unix)]
#[tokio::test]
async fn failed_probe_distinguishes_non_utf8_and_empty_stderr() {
    // Non-UTF-8 stderr
    let plan = ShellCommandPlan {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "printf '\\xff\\xfe\\xfd' >&2; exit 1".into()],
    };
    let err = bounded_output(&plan, Duration::from_secs(2)).await.unwrap_err();
    assert!(err.message.contains("contained non-UTF-8 data"), "got: {}", err.message);

    // Empty stderr
    let plan_empty = ShellCommandPlan {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "exit 1".into()],
    };
    let err_empty = bounded_output(&plan_empty, Duration::from_secs(2)).await.unwrap_err();
    assert!(err_empty.message.contains("was empty"), "got: {}", err_empty.message);
}

