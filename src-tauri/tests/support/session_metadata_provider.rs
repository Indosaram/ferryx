//! Real descendant process/environment and owner transcript discovery fixture.
use ferryx_lib::daemon::{
    protocol::{AgentProviderSession, AgentProviderSessionKey, AgentStateReport},
    server::DaemonServer,
};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

pub async fn publish_discovered(owner: &DaemonServer, root: &Path, session: &Value) {
    let id = session["target"]["sessionId"].as_str().expect("session ID");
    let provider_id = uuid::Uuid::new_v4().to_string();
    let home = std::path::PathBuf::from(std::env::var_os("HOME").expect("isolated HOME"));
    let transcript = home
        .join(".omo/sessions/project")
        .join(format!("fixture_{provider_id}.jsonl"));
    let script = root.join("omo.js");
    let cwd = std::fs::canonicalize(root.join("project/child")).expect("provider CWD");
    std::fs::create_dir_all(transcript.parent().expect("transcript parent"))
        .expect("provider directory");
    std::fs::write(
        &transcript,
        json!({"type":"session","id":provider_id,"cwd":cwd}).to_string(),
    )
    .expect("owner transcript");
    // The user-installed shell keeps a descendant with /omo.js in argv, exactly the
    // production discovery matcher; no mock discovery return value is injected.
    std::fs::write(&script, "test -n \"$PI_SESSION_FILE\" || exit 1\nprintf '\\nA12_%s\\n' PROVIDER_READY\nread answer\n").expect("provider executable script");
    let (_, mut receiver) = owner
        .terminal_service()
        .output_hub()
        .subscribe(id)
        .expect("subscribe provider readiness");
    let executable = std::env::var("A12_PROVIDER_FIXTURE_SHELL")
        .expect("runner supplies an inspectable user executable");
    let command = format!(
        "PI_SESSION_FILE='{}' '{}' '{}'\n",
        transcript.display(),
        executable,
        script.display()
    );
    owner
        .terminal_service()
        .write_input(id, command.as_bytes())
        .expect("start provider descendant");
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut bytes = Vec::new();
        loop {
            bytes.extend(receiver.recv().await.expect("provider PTY output"));
            if bytes
                .windows(b"A12_PROVIDER_READY".len())
                .any(|part| part == b"A12_PROVIDER_READY")
            {
                break;
            }
        }
    })
    .await
    .expect("provider process ready");
    let mut events = owner
        .remote_state()
        .machine_services
        .as_ref()
        .expect("authority")
        .workspaces
        .machine_events
        .subscribe();
    let hint = AgentStateReport {
        session_id: id.into(),
        state: "idle".into(),
        agent: Some("omo".into()),
        provider_session: Some(AgentProviderSession {
            key: AgentProviderSessionKey::SessionId,
            id: provider_id.clone(),
            transcript_path: None,
        }),
    };
    use tokio::io::AsyncWriteExt;
    let path = ferryx_lib::daemon::server::agent_state_socket_path();
    let mut socket = tokio::net::UnixStream::connect(path)
        .await
        .expect("canonical agent report socket");
    let mut frame = serde_json::to_vec(&hint).expect("typed report");
    frame.push(b'\n');
    socket
        .write_all(&frame)
        .await
        .expect("canonical report write");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = events.recv().await.expect("owner metadata event");
            if event["payload"]["providerSession"]["id"] == provider_id {
                assert_eq!(event["payload"]["target"], session["target"]);
                break;
            }
        }
    })
    .await
    .expect("canonical socket validated provider");
    socket.shutdown().await.expect("report socket cleanup");
    owner
        .terminal_service()
        .write_input(id, b"done\n")
        .expect("release provider descendant");
    eprintln!("A12 provider accepted only after real descendant environment discovery and exact owner transcript/CWD resolution");
}
