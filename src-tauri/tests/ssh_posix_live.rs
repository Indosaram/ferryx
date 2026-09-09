use ferryx_lib::ssh::{operations, runtime, SshAuthMethod, SshHost, SshHostSource};
use std::time::Duration;

#[tokio::test]
#[ignore = "requires FERRYX_SSH_POSIX_HOST and a trusted POSIX SSH endpoint"]
async fn posix_environment_directory_and_real_pty() {
    let host = SshHost {
        id: "posix-live-qa".into(),
        label: "POSIX live QA".into(),
        hostname: std::env::var("FERRYX_SSH_POSIX_HOST").expect("explicit POSIX QA host"),
        username: None,
        port: None,
        identity_file: None,
        jump_host: None,
        source: SshHostSource::Manual,
        auth_method: SshAuthMethod::Agent,
        disabled: None,
    };
    let environment = runtime::detect(&host).await.unwrap();
    assert_eq!(environment.platform, runtime::RemotePlatform::Posix);
    let root = operations::probe(&host, &environment, &environment.home).await.unwrap().repo_root;
    let service = ferryx_lib::terminal::TerminalService::default();
    let clone = service.clone();
    let expected = format!("FXQA{root}");
    let (id, _) = tokio::task::spawn_blocking(move || {
        clone.spawn_ssh(&host, &environment, &root, 160, 30, None)
    }).await.unwrap().unwrap();
    let (mut history, mut events) = service.attach(&id).unwrap();
    service.write_input(&id, b"printf '\\106\\130\\121\\101'; pwd -P\n").unwrap();
    let observed = tokio::time::timeout(Duration::from_secs(10), async {
        while !String::from_utf8_lossy(&history).contains(&expected) {
            history.extend(events.recv().await.unwrap());
        }
    }).await;
    service.close_session(&id).await.unwrap();
    observed.expect("exact CWD from real POSIX shell output");
    println!("POSIX_SSH_ENVIRONMENT_DIRECTORY_PTY_OK");
}
