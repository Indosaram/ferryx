use std::time::Duration;

pub async fn run(test_name: &str) -> bool {
    if std::env::var("FERRYX_PRIVATE_TEST").as_deref() == Ok(test_name) {
        return false;
    }
    // Keep Unix socket paths below the platform sockaddr_un limit.
    #[cfg(unix)]
    let parent = std::path::PathBuf::from("/tmp");
    #[cfg(not(unix))]
    let parent = std::env::temp_dir();
    let root = tempfile::Builder::new()
        .prefix("a10-owner-")
        .tempdir_in(parent)
        .expect("private supervisor root");
    for name in ["home", "data", "runtime", "sessions", "tmp", "config", "cache"] {
        std::fs::create_dir(root.path().join(name)).expect("private directory");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            root.path().join("runtime"),
            std::fs::Permissions::from_mode(0o700),
        )
        .expect("private runtime directory permissions");
    }
    let mut command = tokio::process::Command::new(std::env::current_exe().expect("test executable"));
    command.args(["--exact", test_name, "--nocapture"])
        .env("FERRYX_PRIVATE_TEST", test_name)
        .env("HOME", root.path().join("home"))
        .env("FERRYX_DATA_DIR", root.path().join("data"))
        .env("FERRYX_RUNTIME_DIR", root.path().join("runtime"))
        .env("FERRYX_SESSION_DIR", root.path().join("sessions"))
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("XDG_DATA_HOME", root.path().join("data"))
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .env("TMPDIR", root.path().join("tmp"))
        .env("TEMP", root.path().join("tmp"))
        .env("TMP", root.path().join("tmp"))
        .kill_on_drop(true);
    let mut child = command.spawn().expect("private test child");
    let pid = child.id().expect("recorded child pid");
    let status = tokio::time::timeout(Duration::from_secs(180), child.wait()).await;
    let status = match status {
        Ok(status) => status.expect("child exit"),
        Err(_) => {
            child.kill().await.expect("stop owned test child");
            child.wait().await.expect("reap owned test child");
            panic!("private test {test_name} pid={pid} exceeded deadline");
        }
    };
    root.close().expect("remove private supervisor root");
    assert!(status.success(), "private test {test_name} pid={pid} failed: {status}");
    true
}
