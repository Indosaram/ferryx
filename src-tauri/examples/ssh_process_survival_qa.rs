//! External QA entrypoint: only runs with an exact-owned private fixture.
use ferryx_lib::{daemon::DaemonServer, terminal::remote::RemoteSessionConfig, ssh::bridge::SpawnParams};
use std::{path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() {
    let root = PathBuf::from(std::env::var("FERRYX_SURVIVAL_ROOT").expect("run scripts/qa/ssh-process-survival.mjs; fixture required"));
    assert!(root.is_absolute());
    assert!(root.file_name().unwrap().to_str().unwrap().starts_with("fx-survival-"));
    assert_eq!(std::fs::read_to_string(root.join("owner")).unwrap(), std::env::var("FERRYX_SURVIVAL_OWNER").unwrap());
    assert_eq!(std::env::var_os("FERRYX_RUNTIME_DIR").unwrap(), root.join("runtime"));
    assert_eq!(std::env::var_os("HOME").unwrap(), root.join("home"));
    let daemon = Arc::new(DaemonServer::new_with_paths(Some(root.join("gateway.json")), Some(root.join("auth.json"))));
    if std::env::var("FERRYX_SURVIVAL_MODE").unwrap() == "seed" {
        let config: RemoteSessionConfig = serde_json::from_slice(&std::fs::read(root.join("remote-config.json")).unwrap()).unwrap();
        let params: SpawnParams = serde_json::from_slice(&std::fs::read(root.join("spawn.json")).unwrap()).unwrap();
        let descriptor = daemon.terminal_service().remote().create(config, params, "qa-original-spawn".into()).await.expect("integrated runtime create over OpenSSH");
        let value = serde_json::json!({"version":3,"timestamp":0,"activeWorkspaceId":"","workspaces":{},"remoteSessions":[{"descriptor":descriptor,"metadata":null}]});
        std::fs::write(root.join("remote_sessions.json"), serde_json::to_vec(&value).unwrap()).unwrap();
        println!("QA_SEEDED {}", serde_json::to_string(&descriptor).unwrap());
    } else {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let serving = tokio::spawn(daemon.run_server_with_handover_and_readiness(None, Some(tx)));
        tokio::select! {
            result = serving => panic!("daemon ended before ready: {result:?}"),
            result = rx => { result.unwrap(); println!("QA_DAEMON_READY"); }
        }
        std::future::pending::<()>().await;
    }
}
