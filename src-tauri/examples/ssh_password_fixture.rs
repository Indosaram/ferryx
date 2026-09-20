use ferryx_lib::ssh::{self, password::Password};
use std::{io::BufRead, path::PathBuf, sync::Arc, time::Duration};

fn main() {
    if let Some(code) = ssh::password::run_askpass() { std::process::exit(code); }
    if std::env::args().nth(1).as_deref() == Some("--isolated-daemon-wire") {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let path = PathBuf::from(std::env::args().nth(2).unwrap());
            let server = Arc::new(ferryx_lib::daemon::server::DaemonServer::new_with_paths(Some(path.join("gateway.json")), Some(path.join("auth.json"))));
            server.handle_client(tokio::io::join(tokio::io::stdin(), tokio::io::stdout())).await;
        });
        return;
    }
    tokio::runtime::Runtime::new().unwrap().block_on(run());
}

async fn run() {
    let args: Vec<_> = std::env::args().collect();
    let root = PathBuf::from(&args[1]);
    let host: ssh::SshHost = serde_json::from_value(serde_json::json!({"id":"password-fixture","hostname":"127.0.0.1","username":"fixture","port":args[2].parse::<u16>().unwrap(),"authMethod":"password"})).unwrap();
    let mut secret = String::new(); std::io::stdin().lock().read_line(&mut secret).unwrap();
    let secret = secret.trim_end().to_owned();
    use ferryx_lib::daemon::protocol::{DaemonRequest, DaemonResponse};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut daemon = tokio::process::Command::new(std::env::current_exe().unwrap())
        .arg("--isolated-daemon-wire").arg(&root).env("FERRYX_DATA_DIR", &root)
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).kill_on_drop(true).spawn().unwrap();
    let mut input = daemon.stdin.take().unwrap();
    let mut output = BufReader::new(daemon.stdout.take().unwrap());
    for request in [
        DaemonRequest::Handshake { version: ferryx_lib::daemon::protocol::DAEMON_PROTOCOL_VERSION },
        DaemonRequest::SshPassword { host:host.clone(), password:Some(Password::new(secret.clone())) },
        DaemonRequest::SshPassword { host:host.clone(), password:None },
    ] {
        let mut wire=serde_json::to_vec(&request).unwrap(); wire.push(b'\n');
        input.write_all(&wire).await.unwrap();
        let mut response=String::new();
        tokio::time::timeout(Duration::from_secs(10), output.read_line(&mut response)).await.unwrap().unwrap();
        let response:DaemonResponse=serde_json::from_str(&response).unwrap();
        assert!(matches!(response,DaemonResponse::HandshakeOk { .. } | DaemonResponse::Pong));
    }
    drop(input);
    assert!(tokio::time::timeout(Duration::from_secs(10), daemon.wait()).await.unwrap().unwrap().success());
    assert!(ssh::runtime::detect(&host).await.is_err());
    ssh::password::set(&host, Password::new("deliberately-wrong".into())).unwrap();
    assert!(ssh::runtime::detect(&host).await.is_err());
    ssh::password::set(&host, Password::new(secret.clone())).unwrap();
    let environment = ssh::runtime::detect(&host).await.unwrap();
    assert_eq!(environment.platform, ssh::runtime::RemotePlatform::Posix);
    let store = root.join("ssh_hosts.json");
    std::fs::write(&store, serde_json::to_vec(&serde_json::json!({"hosts":[host.clone()]})).unwrap()).unwrap();
    let listing = ssh::browse::list_directories(store.clone(), ssh::browse::ListDirectoriesRequest { host_id:host.id.clone(),path:None }).await.unwrap();
    assert_eq!(listing.home_path, root.to_str().unwrap());
    let probe = ssh::operations::probe(&host, &environment, root.to_str().unwrap()).await.unwrap();
    assert_eq!(probe.repo_root, root.to_str().unwrap());
    let uploaded = ssh::operations::upload(&host, &environment, "fixture.txt", b"password upload".to_vec()).await.unwrap();
    assert_eq!(std::fs::read_to_string(uploaded).unwrap(), "password upload");
    let location = ssh::helper_setup::default_location(&host, &environment).unwrap();
    ssh::helper_setup::install(&host, &environment, &location, &PathBuf::from("remote-helper/target/debug/ferryx-remote-helper")).await.unwrap();
    assert!(PathBuf::from(&location.executable).is_file());

    let mut helper = tokio::process::Command::new(&location.executable)
        .args(["daemon", "--root", &location.root, "--host-id", &host.id])
        .stdout(std::process::Stdio::piped()).kill_on_drop(true).spawn().unwrap();
    let mut ready = String::new();
    tokio::time::timeout(Duration::from_secs(10), BufReader::new(helper.stdout.take().unwrap()).read_line(&mut ready)).await.unwrap().unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&ready).unwrap()["event"], "ready");
    for _ in 0..2 {
        let mut bridge = ssh::bridge::BridgeConnection::spawn(&host, &environment, &location).await.unwrap();
        bridge.handshake().await.unwrap();
        bridge.project_register("password-project", root.to_str().unwrap()).await.unwrap();
        bridge.close().await.unwrap();
    }
    let project = ferryx_lib::ipc::project_remote::register_remote_project(store.clone(), ferryx_lib::ipc::project_remote::RegisterRemoteProjectRequest {
        workspace_id: "password-project".into(), host_id: host.id.clone(), repo_path: root.to_str().unwrap().into(),
    }).await.unwrap();
    let server = Arc::new(ferryx_lib::daemon::server::DaemonServer::new_with_paths(Some(root.join("spawn-gateway.json")), Some(root.join("spawn-auth.json"))));
    let (client, peer) = tokio::io::duplex(65536);
    let serving = tokio::spawn(server.clone().handle_client(peer));
    let (read, mut write) = tokio::io::split(client);
    let mut read = BufReader::new(read);
    let request = DaemonRequest::Spawn { client_request_id:"password-daemon-spawn".into(), workspace_id:project.workspace_id, worktree:None, cwd:None, cols:80, rows:24, shell:None,
        startup:Some(ferryx_lib::daemon::protocol::TerminalStartup::RemoteSsh { host_store_path:store.clone() }) };
    write.write_all(format!("{}\n",serde_json::to_string(&request).unwrap()).as_bytes()).await.unwrap();
    let mut line=String::new();
    tokio::time::timeout(Duration::from_secs(30),read.read_line(&mut line)).await.unwrap().unwrap();
    let sid = match serde_json::from_str::<DaemonResponse>(&line).unwrap() {
        DaemonResponse::SpawnOk { session_id, .. } => session_id,
        other => panic!("daemon spawn failed: {other:?}"),
    };
    let mut states=server.terminal_service().remote().subscribe(&sid).unwrap();
    tokio::time::timeout(Duration::from_secs(30),async {
        loop {
            if states.borrow_and_update().state == ferryx_lib::terminal::remote::RemoteConnectionState::Connected { break; }
            states.changed().await.unwrap();
        }
    }).await.unwrap();
    let (_, mut events)=server.terminal_service().attach(&sid).unwrap();
    let generation=server.terminal_service().remote().details(&sid).unwrap().generation;
    server.terminal_service().write_input_operation(&sid,generation,b"printf 'DAEMON_%s_OK\\n' PASSWORD\n".to_vec()).unwrap().await.unwrap();
    tokio::time::timeout(Duration::from_secs(15),async {
        let mut history=Vec::new();
        while !String::from_utf8_lossy(&history).contains("DAEMON_PASSWORD_OK") { history.extend(events.recv().await.unwrap()); }
    }).await.unwrap();
    // Watch is already subscribed before the fixture drops all SSH transports.
    let plan=ssh::direct::ssh_plan(&host,"FERRYX_FIXTURE_DISCONNECT".into(),false).unwrap();
    let mut breaker=tokio::process::Command::new(&plan.program).args(&plan.args)
        .envs(ssh::password::environment(&plan.args).unwrap()).kill_on_drop(true).spawn().unwrap();
    tokio::time::timeout(Duration::from_secs(30),async {
        loop {
            let details=states.borrow_and_update().clone();
            if details.generation > generation && details.state == ferryx_lib::terminal::remote::RemoteConnectionState::Connected { break; }
            states.changed().await.unwrap();
        }
    }).await.unwrap();
    breaker.wait().await.unwrap();
    let restored=server.terminal_service().remote().details(&sid).unwrap();
    server.terminal_service().write_input_operation(&sid,restored.generation,b"printf 'RETRIED_%s_OK\\n' PASSWORD\n".to_vec()).unwrap().await.unwrap();
    tokio::time::timeout(Duration::from_secs(15),async {
        let mut history=Vec::new();
        while !String::from_utf8_lossy(&history).contains("RETRIED_PASSWORD_OK") { history.extend(events.recv().await.unwrap()); }
    }).await.unwrap();
    println!("daemon forced-disconnect automatic retry preserved session and accepted input");
    write.write_all(format!("{}\n",serde_json::to_string(&DaemonRequest::Close { session_id:sid }).unwrap()).as_bytes()).await.unwrap();
    line.clear(); read.read_line(&mut line).await.unwrap();
    assert!(matches!(serde_json::from_str::<DaemonResponse>(&line).unwrap(),DaemonResponse::CloseOk));
    drop(write); drop(read); serving.await.unwrap();
    helper.kill().await.unwrap();
    let service = Arc::new(ferryx_lib::terminal::service::TerminalService::default());
    let (sid, _lifecycle) = service.spawn_ssh(&host, &environment, root.to_str().unwrap(), 80, 24, None).unwrap();
    let (initial, mut output) = service.attach(&sid).unwrap();
    service.write_input(&sid, b"printf 'PASSWORD_%s_OK\\n' TERMINAL; exit\n").unwrap();
    let bytes = tokio::time::timeout(Duration::from_secs(15), async {
        let mut all=initial;
        while let Ok(bytes)=output.recv().await { all.extend(bytes); }
        all
    }).await.unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("PASSWORD_TERMINAL_OK"));
    assert!(!String::from_utf8_lossy(&bytes).contains(&secret));
    // A new SSH process authenticates with the same transient credential.
    ssh::runtime::detect(&host).await.unwrap();
    ssh::password::clear(&host).unwrap();
    assert!(ssh::runtime::detect(&host).await.is_err());
    assert!(!std::fs::read_to_string(store).unwrap().contains(&secret));
    println!("probe/browse/project/upload/helper-install/terminal/reconnect/clear passed");
}
