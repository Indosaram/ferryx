use super::*;
use crate::ssh::runtime::{RemoteEnvironment, RemoteExecutor, RemotePlatform};
use crate::ssh::{SshAuthMethod, SshHost, SshHostSource};
use std::path::Path;

fn sample_host(id: &str) -> SshHost {
    SshHost {
        id: id.to_string(),
        label: "test-label".to_string(),
        hostname: "127.0.0.1".to_string(),
        username: None,
        port: None,
        identity_file: None,
        jump_host: None,
        source: SshHostSource::Config,
        auth_method: SshAuthMethod::Agent,
        disabled: None,
    }
}

fn sample_posix_env(home: &str) -> RemoteEnvironment {
    RemoteEnvironment {
        platform: RemotePlatform::Posix,
        executor: RemoteExecutor::Sh,
        version: "Linux 6.1".to_string(),
        home: home.to_string(),
        temp: "/tmp".to_string(),
        git: true,
    }
}

fn sample_windows_env(home: &str) -> RemoteEnvironment {
    RemoteEnvironment {
        platform: RemotePlatform::Windows,
        executor: RemoteExecutor::Powershell,
        version: "Windows 10.0.19045".to_string(),
        home: home.to_string(),
        temp: "C:\\Temp".to_string(),
        git: true,
    }
}

#[test]
fn ssh_helper_setup_location_serde_round_trip() {
    let location = HelperLocation {
        executable: "/home/testuser/.ferryx/bin/ferryx-remote-helper".into(),
        root: "/home/testuser/.ferryx/helper/qa-host".into(),
    };
    let json = serde_json::to_string(&location).expect("serialize");
    let back: HelperLocation = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(location, back);
    assert!(json.contains("\"executable\":"));
    assert!(json.contains("\"root\":"));
}

#[test]
fn ssh_helper_setup_default_location_posix() {
    let host = sample_host("qa-host-1");
    let env = sample_posix_env("/home/testuser");
    let location = default_location(&host, &env).expect("default_location");

    assert!(location.executable.starts_with("/home/testuser/.ferryx/bin/"));
    assert!(location.root.starts_with("/home/testuser/.ferryx/helper/"));

    // Trailing slashes in home should be normalized
    let env_trailing = sample_posix_env("/home/testuser/");
    let location_trailing = default_location(&host, &env_trailing).expect("default_location");
    assert_eq!(location, location_trailing);

    // Platform validation checks
    assert!(env.platform.validate_path(&location.executable).is_ok());
    assert!(env.platform.validate_path(&location.root).is_ok());
}

#[test]
fn ssh_helper_setup_default_location_windows() {
    let host = sample_host("win-box");
    let env = sample_windows_env("C:\\Users\\testuser");
    let location = default_location(&host, &env).expect("default_location");

    assert!(location.executable.starts_with("C:\\Users\\testuser\\.ferryx\\bin\\"));
    assert!(location.root.starts_with("C:\\Users\\testuser\\.ferryx\\helper\\"));

    // Trailing slashes in home should be normalized
    let env_trailing = sample_windows_env("C:\\Users\\testuser\\");
    let location_trailing = default_location(&host, &env_trailing).expect("default_location");
    assert_eq!(location, location_trailing);

    assert!(env.platform.validate_path(&location.executable).is_ok());
    assert!(env.platform.validate_path(&location.root).is_ok());
}

#[test]
fn ssh_helper_setup_default_location_rejects_invalid_home() {
    let host = sample_host("qa-host");
    let invalid_posix = sample_posix_env("relative/home");
    assert!(default_location(&host, &invalid_posix).is_err());

    let invalid_windows = sample_windows_env("relative\\path");
    assert!(default_location(&host, &invalid_windows).is_err());
}

#[test]
fn ssh_helper_setup_host_id_collision_resistance() {
    let env = sample_posix_env("/home/testuser");
    let host1 = sample_host("a:b");
    let host2 = sample_host("a/b");
    let host3 = sample_host("a_b");
    let loc1 = default_location(&host1, &env).unwrap();
    let loc2 = default_location(&host2, &env).unwrap();
    let loc3 = default_location(&host3, &env).unwrap();

    assert_ne!(
        loc1.root, loc2.root,
        "host roots for 'a:b' and 'a/b' must not collide"
    );
    assert_ne!(
        loc1.root, loc3.root,
        "host roots for 'a:b' and 'a_b' must not collide"
    );
    assert_ne!(
        loc2.root, loc3.root,
        "host roots for 'a/b' and 'a_b' must not collide"
    );
}

#[tokio::test]
async fn ssh_helper_setup_install_rejects_missing_local_binary() {
    let host = sample_host("qa-host");
    let env = sample_posix_env("/home/testuser");
    let location = HelperLocation {
        executable: "/home/testuser/.ferryx/bin/ferryx-remote-helper".into(),
        root: "/home/testuser/.ferryx/helper/qa-host".into(),
    };
    let non_existent = Path::new("/nonexistent/binary/path/ferryx-helper");
    let result = install(&host, &env, &location, non_existent).await;
    assert!(result.is_err(), "installing missing local binary must fail");
    let err = result.unwrap_err();
    assert!(
        matches!(err.code, IpcErrorCode::IoError | IpcErrorCode::InvalidPath),
        "expected IoError or InvalidPath for missing binary, got {:?}",
        err.code
    );
}

#[tokio::test]
async fn ssh_helper_setup_install_rejects_empty_local_binary() {
    let temp_file = tempfile::NamedTempFile::new().expect("create temp file");
    let host = sample_host("qa-host");
    let env = sample_posix_env("/home/testuser");
    let location = HelperLocation {
        executable: "/home/testuser/.ferryx/bin/ferryx-remote-helper".into(),
        root: "/home/testuser/.ferryx/helper/qa-host".into(),
    };
    let result = install(&host, &env, &location, temp_file.path()).await;
    assert!(result.is_err(), "installing empty binary must fail");
}

#[tokio::test]
async fn ssh_helper_setup_ensure_started_rejects_invalid_paths() {
    let host = sample_host("qa-host");
    let env = sample_posix_env("/home/testuser");
    let location = HelperLocation {
        executable: "relative/executable".into(),
        root: "/home/testuser/.ferryx/helper/qa-host".into(),
    };
    let result = ensure_started(&host, &env, &location).await;
    assert!(result.is_err());
}

#[test]
fn ssh_helper_setup_readiness_parser_valid_ready() {
    let output = b"{\"event\":\"ready\",\"protocol\":1}\n";
    assert!(parse_ready_output(output).is_ok());

    let output_with_banners = b"Warning: banner text\n{\"event\":\"ready\",\"protocol\":1}\n";
    assert!(parse_ready_output(output_with_banners).is_ok());
}

#[test]
fn ssh_helper_setup_readiness_parser_rejects_malformed_json() {
    let output = b"just some text saying event ready protocol 1";
    let res = parse_ready_output(output);
    assert!(res.is_err());
    assert_eq!(res.unwrap_err().code, IpcErrorCode::IoError);
}

#[test]
fn ssh_helper_setup_readiness_parser_rejects_missing_ready_event() {
    let output = b"{\"event\":\"started\",\"protocol\":1}\n";
    let res = parse_ready_output(output);
    assert!(res.is_err());
    assert_eq!(res.unwrap_err().code, IpcErrorCode::IoError);
}

#[test]
fn ssh_helper_setup_ensure_started_exit_code_127_maps_to_missing_helper() {
    let location = HelperLocation {
        executable: "/home/u/.ferryx/bin/helper".into(),
        root: "/home/u/.ferryx/helper/qa".into(),
    };
    let raw_err = IpcError::new(IpcErrorCode::IoError, "exit 127").with_details(serde_json::json!({
        "stage": "execution",
        "exitCode": 127,
        "stderr": "sh: helper: not found",
    }));
    let mapped = map_ensure_started_error(raw_err, &location);
    assert_eq!(mapped.code, IpcErrorCode::CliExecutableNotFound);
    let details = mapped.details.as_ref().expect("details");
    assert_eq!(
        details.get("stage").and_then(serde_json::Value::as_str),
        Some("helper_missing")
    );
}

#[test]
fn ssh_helper_setup_ensure_started_exit_code_126_maps_to_unsupported_permissions() {
    let location = HelperLocation {
        executable: "/home/u/.ferryx/bin/helper".into(),
        root: "/home/u/.ferryx/helper/qa".into(),
    };
    let raw_err = IpcError::new(IpcErrorCode::IoError, "exit 126").with_details(serde_json::json!({
        "stage": "execution",
        "exitCode": 126,
        "stderr": "sh: helper: Permission denied",
    }));
    let mapped = map_ensure_started_error(raw_err, &location);
    assert_eq!(mapped.code, IpcErrorCode::Unsupported);
    let details = mapped.details.as_ref().expect("details");
    assert_eq!(
        details.get("stage").and_then(serde_json::Value::as_str),
        Some("helper_permissions")
    );
}

#[test]
fn ssh_helper_setup_ensure_started_windows_exit_1_with_sentinel_maps_to_missing_helper() {
    let location = HelperLocation {
        executable: r"C:\Users\u\.ferryx\bin\ferryx-remote-helper.exe".into(),
        root: r"C:\Users\u\.ferryx\helper\qa".into(),
    };
    // Windows OpenSSH normalizes non-zero exit codes to 1, but captures stderr sentinel
    let raw_err = IpcError::new(
        IpcErrorCode::IoError,
        "SSH command failed (exit 1): FERRYX_ERR_HELPER_MISSING",
    )
    .with_details(serde_json::json!({
        "stage": "execution",
        "exitCode": 1,
        "stderr": "FERRYX_ERR_HELPER_MISSING\r\n",
    }));
    let mapped = map_ensure_started_error(raw_err, &location);
    assert_eq!(mapped.code, IpcErrorCode::CliExecutableNotFound);
    let details = mapped.details.as_ref().expect("details");
    assert_eq!(
        details.get("stage").and_then(serde_json::Value::as_str),
        Some("helper_missing")
    );
    assert!(mapped.message.contains("not installed"));
}

#[test]
fn ssh_helper_setup_ensure_started_windows_exit_1_with_sentinel_maps_to_unsupported_permissions() {
    let location = HelperLocation {
        executable: r"C:\Users\u\.ferryx\bin\ferryx-remote-helper.exe".into(),
        root: r"C:\Users\u\.ferryx\helper\qa".into(),
    };
    let raw_err = IpcError::new(
        IpcErrorCode::IoError,
        "SSH command failed (exit 1): FERRYX_ERR_HELPER_NOT_EXECUTABLE",
    )
    .with_details(serde_json::json!({
        "stage": "execution",
        "exitCode": 1,
        "stderr": "FERRYX_ERR_HELPER_NOT_EXECUTABLE\r\n",
    }));
    let mapped = map_ensure_started_error(raw_err, &location);
    assert_eq!(mapped.code, IpcErrorCode::Unsupported);
    let details = mapped.details.as_ref().expect("details");
    assert_eq!(
        details.get("stage").and_then(serde_json::Value::as_str),
        Some("helper_permissions")
    );
}

#[test]
fn ssh_helper_setup_map_error_preserves_exit_255_transport_even_with_marker() {
    let location = HelperLocation {
        executable: "/home/u/.ferryx/bin/ferryx-remote-helper".into(),
        root: "/home/u/.ferryx/helper/qa".into(),
    };
    let raw_err = IpcError::new(
        IpcErrorCode::IoError,
        "SSH connection error: host unreachable FERRYX_ERR_HELPER_MISSING",
    )
    .with_details(serde_json::json!({
        "stage": "transport",
        "exitCode": 255,
        "stderr": "ssh: connect to host 1.2.3.4 port 22: Connection refused\nFERRYX_ERR_HELPER_MISSING",
    }));
    let mapped = map_ensure_started_error(raw_err.clone(), &location);
    assert_eq!(mapped.code, IpcErrorCode::IoError);
    assert_eq!(mapped.message, raw_err.message);
}

#[test]
fn ssh_helper_setup_map_error_rejects_contradictory_markers() {
    let location = HelperLocation {
        executable: "/home/u/.ferryx/bin/ferryx-remote-helper".into(),
        root: "/home/u/.ferryx/helper/qa".into(),
    };
    let raw_err = IpcError::new(IpcErrorCode::IoError, "exit 1 with both markers").with_details(
        serde_json::json!({
            "stage": "execution",
            "exitCode": 1,
            "stderr": "FERRYX_ERR_HELPER_MISSING\nFERRYX_ERR_HELPER_NOT_EXECUTABLE\n",
        }),
    );
    let mapped = map_ensure_started_error(raw_err.clone(), &location);
    assert_eq!(mapped.code, IpcErrorCode::IoError);

    let raw_err2 = IpcError::new(IpcErrorCode::IoError, "exit 126 with missing marker").with_details(
        serde_json::json!({
            "stage": "execution",
            "exitCode": 126,
            "stderr": "FERRYX_ERR_HELPER_MISSING\n",
        }),
    );
    let mapped2 = map_ensure_started_error(raw_err2.clone(), &location);
    assert_eq!(mapped2.code, IpcErrorCode::IoError);
}

#[test]
fn ssh_helper_setup_map_error_retains_diagnostic_cause() {
    let location = HelperLocation {
        executable: r"C:\Users\u\.ferryx\bin\ferryx-remote-helper.exe".into(),
        root: r"C:\Users\u\.ferryx\helper\qa".into(),
    };
    let raw_err = IpcError::new(
        IpcErrorCode::IoError,
        "SSH command failed (exit 1)",
    )
    .with_details(serde_json::json!({
        "stage": "execution",
        "exitCode": 1,
        "stderr": "FERRYX_ERR_HELPER_MISSING\r\n",
    }));
    let mapped = map_ensure_started_error(raw_err, &location);
    assert_eq!(mapped.code, IpcErrorCode::CliExecutableNotFound);
    let details = mapped.details.as_ref().expect("details");
    let cause = details.get("cause").expect("cause must be preserved");
    assert_eq!(cause.get("exitCode").and_then(serde_json::Value::as_i64), Some(1));
    assert!(cause.get("stderr").and_then(serde_json::Value::as_str).unwrap().contains("FERRYX_ERR_HELPER_MISSING"));
}

#[cfg(unix)]
#[test]
fn ssh_helper_setup_process_start_rejects_symlink_root_without_writing_log() {
    let temp = tempfile::tempdir().expect("tempdir");
    let sentinel = temp.path().join("sentinel.txt");
    std::fs::write(&sentinel, b"ORIGINAL_SENTINEL_DATA").expect("write sentinel");

    let symlink_root = temp.path().join("symlink_root");
    std::os::unix::fs::symlink(&sentinel, &symlink_root).expect("create symlink");

    // Attempt start on symlink root
    let result = crate::ssh::helper_runtime::process::start(symlink_root, "qa-host".into());
    assert!(result.is_err(), "start must fail on symlink root");

    // Sentinel must be completely untouched (not truncated or overwritten)
    let content = std::fs::read(&sentinel).expect("read sentinel");
    assert_eq!(content, b"ORIGINAL_SENTINEL_DATA");
}
