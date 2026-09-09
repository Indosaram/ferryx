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
    assert_eq!(std::fs::read(dir.path().join("endpoint.json")).unwrap(), original);
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
    write_frame(&mut input, &json!({"protocol":1,"op":"pty.describe","params":{}})).unwrap();
    let mut output = Vec::new();
    bridge(dir.path(), std::io::Cursor::new(input), &mut output).unwrap();
    let response = read_frame(&mut std::io::Cursor::new(output)).unwrap().unwrap();
    assert_eq!(response["error"].as_str().unwrap().split(':').next(), Some("INVALID_REQUEST"));
    worker.join().unwrap();
    let auth = endpoint(dir.path()).unwrap();
    assert!(runtime.handle(Request {
        protocol: 1, token: auth.token, op: "handshake".into(), params: json!({})
    }).is_ok());
}
