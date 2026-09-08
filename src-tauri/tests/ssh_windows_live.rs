//! Opt-in integration against a provisioned Windows OpenSSH endpoint.
use ferryx_lib::ipc::ssh::cmd_ssh_test_connection;
use ferryx_lib::ssh::{direct, operations, runtime};
use ferryx_lib::ssh::{SshAuthMethod, SshHost, SshHostSource};
use std::time::Duration;

fn host() -> SshHost {
    SshHost {
        id: "windows-live-qa".into(),
        label: "Windows live QA".into(),
        hostname: std::env::var("FERRYX_SSH_WINDOWS_HOST").expect("explicit Windows QA host"),
        username: None,
        port: None,
        identity_file: None,
        jump_host: None,
        source: SshHostSource::Manual,
        auth_method: SshAuthMethod::Agent,
        disabled: None,
    }
}

#[tokio::test]
#[ignore = "requires FERRYX_SSH_WINDOWS_HOST and a trusted Windows OpenSSH endpoint"]
async fn windows_connection_test_accepts_the_remote_shell() {
    let summary = cmd_ssh_test_connection(host()).await.expect("SSH test");
    assert!(summary.reachable, "{:?}", summary.last_error);
    assert_eq!(
        summary.environment.unwrap().platform,
        runtime::RemotePlatform::Windows
    );
}

async fn script(host: &SshHost, environment: &runtime::RemoteEnvironment, text: &str) -> Vec<u8> {
    let plan = direct::ssh_plan(host, environment.executor.command(text), false).unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(20),
        tokio::process::Command::new(plan.program)
            .args(plan.args)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[tokio::test]
#[ignore = "requires FERRYX_SSH_WINDOWS_HOST; starts a session-owned loopback state listener"]
async fn windows_state_bridge_delivers_authenticated_reports() {
    let host = host();
    let environment = runtime::detect(&host).await.unwrap();
    let mut bridge = ferryx_lib::ssh::state_bridge::StateBridge::start(&host, &environment)
        .await
        .unwrap();
    let payload = serde_json::json!({
        "type": "agentState", "sessionId": "qa-session", "token": bridge.endpoint.token, "state": "working",
    }).to_string() + "\n";
    let command = format!(
        "$c=New-Object Net.Sockets.TcpClient; try {{ $c.Connect('127.0.0.1',{}); \
         $b=[Text.Encoding]::UTF8.GetBytes({}); $c.GetStream().Write($b,0,$b.Length) }} finally {{ $c.Close() }}",
        bridge.endpoint.port, runtime::powershell_data(&payload),
    );
    let process_id = bridge.endpoint.process_id;
    let receive = tokio::time::timeout(Duration::from_secs(8), bridge.next_report("qa-session"));
    let (report, _) = tokio::join!(receive, script(&host, &environment, &command));
    bridge.close().await;
    script(&host, &environment, &format!(
        "try {{ $p=[Diagnostics.Process]::GetProcessById({process_id}) }} \
         catch [ArgumentException] {{ exit 0 }}; \
         if (!$p.WaitForExit(5000)) {{ throw 'SSH state relay survived its connection' }}"
    )).await;
    let report: serde_json::Value = serde_json::from_str(&report.unwrap().unwrap()).unwrap();
    assert_eq!(report["state"], "working");
    assert!(report.get("token").is_none());
    println!("WINDOWS_SSH_STATE_BRIDGE_OK");
}

#[tokio::test]
#[ignore = "requires FERRYX_SSH_WINDOWS_HOST; creates and cleans a private QA directory"]
async fn windows_directory_git_upload_and_pty_use_the_same_runtime() {
    let host = host();
    let environment = runtime::detect(&host).await.unwrap();
    let root = format!(
        "{}ferryx-qa-{} space '$ & (한글)",
        environment.temp,
        uuid::Uuid::new_v4()
    );
    let path_data = runtime::powershell_data(&root);
    script(
        &host,
        &environment,
        &format!("[void][IO.Directory]::CreateDirectory({path_data})"),
    )
    .await;
    let outcome = exercise_windows(&host, &environment, &root).await;
    script(
        &host,
        &environment,
        &format!("Remove-Item -LiteralPath {path_data} -Recurse -Force"),
    )
    .await;
    outcome.unwrap();
}

async fn exercise_windows(
    host: &SshHost,
    environment: &runtime::RemoteEnvironment,
    root: &str,
) -> Result<(), ferryx_lib::ipc::IpcError> {
    let found = operations::probe(host, environment, root).await?;
    assert_eq!(found.0, root);
    assert_eq!(found.1, None);
    operations::git(host, environment, root, &["init"]).await?;
    operations::git(
        host,
        environment,
        root,
        &["remote", "add", "origin", "https://example.test/qa.git"],
    )
    .await?;
    let found = operations::probe(host, environment, root).await?;
    assert_eq!(found.1.as_deref(), Some(root.replace('\\', "/").as_str()));
    assert_eq!(found.2.as_deref(), Some("https://example.test/qa.git"));

    let state = tempfile::tempdir().unwrap();
    let store = state.path().join("ssh_hosts.json");
    std::fs::write(
        &store,
        serde_json::to_vec(&ferryx_lib::ipc::ssh::SshHostStore {
            hosts: vec![host.clone()],
            tombstones: vec![],
        })
        .unwrap(),
    )
    .unwrap();
    let registered = ferryx_lib::ipc::project_remote::register_remote_project(
        store.clone(),
        ferryx_lib::ipc::project_remote::RegisterRemoteProjectRequest {
            workspace_id: "windows-qa".into(),
            host_id: host.id.clone(),
            repo_path: root.into(),
        },
    )
    .await?;
    assert_eq!(registered.repo_root, root);
    let persisted = ferryx_lib::ssh::projects::resolve(&store, &registered.workspace_id)?;
    assert_eq!(persisted.0.platform, Some(runtime::RemotePlatform::Windows));

    let mut isolated = environment.clone();
    isolated.home = root.into();
    operations::prepare_integration(host, &isolated).await?;
    operations::prepare_integration(host, &isolated).await?;
    let installed = script(host, environment, &format!(
        "[Console]::Write([IO.File]::ReadAllText((Join-Path {} '.omo/agent/extensions/ferryx-agent-state.ts')))",
        runtime::powershell_data(root)
    )).await;
    assert_eq!(
        installed,
        ferryx_lib::daemon::agent_extension::EXTENSION_SOURCE.as_bytes()
    );
    let mut core = environment.clone();
    core.executor = runtime::RemoteExecutor::Pwsh;
    assert_eq!(operations::probe(host, &core, root).await?.0, root);

    let payload: Vec<u8> = (0..65536).map(|i| (i % 256) as u8).collect();
    let file = operations::upload(
        host,
        environment,
        &format!("{}.png", uuid::Uuid::new_v4()),
        payload.clone(),
    )
    .await?;
    let encoded = script(host, environment, &format!(
        "$p={}; try {{ [Console]::Write([Convert]::ToBase64String([IO.File]::ReadAllBytes($p))) }} finally {{ Remove-Item -LiteralPath $p -Force }}",
        runtime::powershell_data(&file)
    )).await;
    use base64::Engine as _;
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap(),
        payload
    );

    let service = ferryx_lib::terminal::TerminalService::default();
    let clone = service.clone();
    let host_copy = host.clone();
    let env_copy = environment.clone();
    let root_copy = root.to_string();
    let (id, _) = tokio::task::spawn_blocking(move || {
        clone
            .spawn_ssh(&host_copy, &env_copy, &root_copy, 512, 30, None)
            .map_err(ferryx_lib::ipc::IpcError::from)
    })
    .await
    .unwrap()?;
    let (mut history, mut events) = service.attach(&id).unwrap();
    let expected = format!("FXQA{root}");
    let mut answered = 0;
    let mut sent = false;
    let result = tokio::time::timeout(Duration::from_secs(20), async {
        while !String::from_utf8_lossy(&history).contains(&expected) {
            let requests = history.windows(4).filter(|bytes| *bytes == b"\x1b[6n").count();
            while answered < requests {
                service.write_input(&id, b"\x1b[1;1R").unwrap();
                answered += 1;
            }
            if !sent && String::from_utf8_lossy(&history).contains(&format!("{root}>")) {
                service.write_input(&id, b"[Console]::WriteLine(([char]70+[string][char]88+[char]81+[char]65)+$PWD.Path)\r").unwrap();
                sent = true;
            }
            history.extend(events.recv().await.unwrap());
        }
    }).await;
    service.close_session(&id).await.unwrap();
    assert!(
        result.is_ok(),
        "PTY did not return its real CWD: {:?}",
        String::from_utf8_lossy(&history)
    );
    println!("WINDOWS_SSH_DIRECTORY_GIT_UPLOAD_PTY_OK");
    Ok(())
}
