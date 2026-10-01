use super::*;

fn private_tempdir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    super::super::private_file(dir.path()).unwrap();
    dir
}

#[test]
fn ssh_reconnect_safety_live_runtime_cannot_be_replaced() {
    let dir = private_tempdir();
    let first = bind_runtime(dir.path(), "qa-lock".into()).unwrap();
    let original = std::fs::read(dir.path().join("endpoint.json")).unwrap();

    assert!(matches!(
        bind_runtime(dir.path(), "qa-lock".into()),
        Err(error) if error.starts_with("REMOTE_RUNTIME_CONFLICT:")
    ));
    assert_eq!(
        std::fs::read(dir.path().join("endpoint.json")).unwrap(),
        original
    );
    drop(first);
}

#[test]
fn ssh_process_survival_stale_endpoint_replaced_only_after_lock_release() {
    let dir = private_tempdir();
    let first = bind_runtime(dir.path(), "qa-stale".into()).unwrap();
    let original = endpoint(dir.path()).unwrap().token;
    drop(first);

    let second = bind_runtime(dir.path(), "qa-stale".into()).unwrap();
    assert_ne!(endpoint(dir.path()).unwrap().token, original);
    drop(second);
}

#[cfg(unix)]
#[test]
fn ssh_reconnect_safety_rejects_symlink_root_and_endpoint() {
    use std::os::unix::fs::symlink;
    let dir = private_tempdir();
    let target = private_tempdir();
    let link = dir.path().join("alias");
    symlink(target.path(), &link).unwrap();
    assert!(bind_runtime(&link, "qa-link".into()).is_err());

    let sentinel = target.path().join("sentinel");
    std::fs::write(&sentinel, b"untouched").unwrap();
    symlink(&sentinel, dir.path().join("endpoint.json")).unwrap();
    assert!(bind_runtime(dir.path(), "qa-link".into()).is_err());
    assert!(endpoint(dir.path()).is_err());
    assert_eq!(std::fs::read(sentinel).unwrap(), b"untouched");
}

#[cfg(windows)]
#[test]
fn ssh_reconnect_safety_rejects_untrusted_windows_acl() {
    let dir = private_tempdir();
    let file = dir.path().join("endpoint.json");
    std::fs::write(&file, b"{}").unwrap();
    super::super::private_file(&file).unwrap();
    assert!(validate_private(&file).is_ok());

    let grant = std::process::Command::new("icacls")
        .arg(&file)
        .args(["/grant", "*S-1-1-0:(R)"])
        .output()
        .expect("icacls grant Everyone SID");
    assert!(grant.status.success());

    assert!(matches!(
        validate_private(&file),
        Err(error) if error.starts_with("FORBIDDEN:")
    ));
    assert!(matches!(
        endpoint(dir.path()),
        Err(error) if error.starts_with("FORBIDDEN:")
    ));
}

#[test]
fn ssh_process_survival_bridge_allows_describe_without_stopping_runtime() {
    let dir = private_tempdir();
    let bound = bind_runtime(dir.path(), "qa-bridge".into()).unwrap();
    let runtime = bound.runtime.clone();
    let worker = std::thread::spawn(move || {
        let (stream, _) = bound.listener.accept().unwrap();
        serve(stream, bound.runtime.clone()).unwrap();
    });
    let mut input = Vec::new();
    write_frame(
        &mut input,
        &json!({"protocol":1,"op":"pty.describe","params":{}}),
    )
    .unwrap();
    let mut output = Vec::new();
    bridge(dir.path(), std::io::Cursor::new(input), &mut output).unwrap();
    let response = read_frame(&mut std::io::Cursor::new(output))
        .unwrap()
        .unwrap();
    assert_eq!(
        response["error"].as_str().unwrap().split(':').next(),
        Some("INVALID_REQUEST")
    );
    worker.join().unwrap();
    let auth = endpoint(dir.path()).unwrap();
    assert!(runtime
        .handle(Request {
            protocol: 1,
            token: auth.token,
            op: "handshake".into(),
            params: json!({})
        })
        .is_ok());
}

#[test]
fn ssh_process_survival_agent_state_survives_bridge_eof() {
    use std::net::TcpStream;

    let dir = private_tempdir();
    let bound = bind_runtime(dir.path(), "qa-agent-bridge".into()).unwrap();
    let runtime = bound.runtime.clone();
    let worker = std::thread::spawn(move || {
        let (stream, _) = bound.listener.accept().unwrap();
        serve(stream, bound.runtime.clone()).unwrap();
    });

    let mut input = Vec::new();
    write_frame(
        &mut input,
        &json!({"protocol":1,"op":"handshake","params":{}}),
    )
    .unwrap();
    let mut output = Vec::new();
    bridge(dir.path(), std::io::Cursor::new(input), &mut output).unwrap();
    worker.join().unwrap();

    let server_port = runtime.agent_state_server().unwrap().port();
    let stream = TcpStream::connect(("127.0.0.1", server_port));
    assert!(
        stream.is_ok(),
        "Agent state server must remain alive after bridge EOF"
    );
}

#[test]
fn ssh_helper_unit_launch_args_forward_env_and_drop_connection_vars() {
    use std::ffi::OsString;

    let env = vec![
        (
            OsString::from("FERRYX_HELPER_HYGIENE_SECS"),
            OsString::from("7"),
        ),
        (OsString::from("PATH"), OsString::from("/usr/bin")),
        (
            OsString::from("SSH_CONNECTION"),
            OsString::from("100.78.73.127 5 100.91.254.71 22"),
        ),
        (OsString::from("XDG_RUNTIME_DIR"), OsString::from("")),
        (
            OsString::from("BASH_FUNC_probe%%"),
            OsString::from("() { :; }"),
        ),
    ];
    let args = unit_launch_args(
        std::path::Path::new("/home/u/.ferryx/bin/ferryx-remote-helper"),
        std::path::Path::new("/home/u/.ferryx/helper/ssh-omarchy"),
        "ssh-omarchy",
        &env,
    );
    let text: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();

    assert!(text.contains(&"--setenv=FERRYX_HELPER_HYGIENE_SECS=7".to_string()));
    assert!(text.contains(&"--setenv=PATH=/usr/bin".to_string()));
    assert!(text.contains(&"--user".to_string()));
    assert!(text.contains(&"--collect".to_string()));
    assert!(!text
        .iter()
        .any(|arg| arg.starts_with("--setenv=SSH_CONNECTION=")));
    assert!(!text
        .iter()
        .any(|arg| arg.starts_with("--setenv=XDG_RUNTIME_DIR=")));
    assert!(!text.iter().any(|arg| arg.contains("BASH_FUNC_probe")));
    assert_eq!(
        &text[text.len() - 7..],
        &[
            "--",
            "/home/u/.ferryx/bin/ferryx-remote-helper",
            "daemon",
            "--root",
            "/home/u/.ferryx/helper/ssh-omarchy",
            "--host-id",
            "ssh-omarchy",
        ]
    );
    // Launch and lookup must agree on the unit for the same host and root.
    assert!(text.contains(&format!(
        "--unit={}",
        helper_unit_name("ssh-omarchy", std::path::Path::new("/home/u/.ferryx/helper/ssh-omarchy"))
    )));
}

#[test]
fn ssh_helper_unit_name_is_deterministic_and_scoped_to_the_root() {
    let root = std::path::Path::new("/home/u/.ferryx/r/2026.930.1/aaaaaaaa");
    let other_root = std::path::Path::new("/home/u/.ferryx/r/2026.930.1/bbbbbbbb");
    let other_version = std::path::Path::new("/home/u/.ferryx/r/2026.917.1/aaaaaaaa");

    // Deterministic for the same host and root; never shared across roots.
    assert_eq!(
        helper_unit_name("ssh-omarchy", root),
        helper_unit_name("ssh-omarchy", root)
    );
    assert_ne!(
        helper_unit_name("ssh-omarchy", root),
        helper_unit_name("ssh-omarchy", other_root)
    );
    assert_ne!(
        helper_unit_name("ssh-omarchy", root),
        helper_unit_name("ssh-omarchy", other_version)
    );

    // Host sanitization and truncation are unchanged, with a 16-hex root suffix.
    let long_host = "x".repeat(200);
    for (host, sanitized) in [
        ("ssh-omarchy", "ssh-omarchy".to_string()),
        ("a/b:c d", "a-b-c-d".to_string()),
        ("", "default".to_string()),
        (long_host.as_str(), "x".repeat(64)),
    ] {
        let unit = helper_unit_name(host, root);
        let (prefix, digest) = unit.rsplit_once('-').expect("unit suffix");
        assert_eq!(prefix, format!("ferryx-helper-{sanitized}"));
        assert_eq!(digest.len(), 16);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
