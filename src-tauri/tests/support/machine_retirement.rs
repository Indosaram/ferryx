//! Separate-process final machine-close retirement regression.
use super::*;
use serde_json::{json, Value};

async fn http(socket: &Path, pid: u32, method: &str, path: &str, body: &Value) -> (u16, Value) {
    let mut client = TestDaemonClient::connect(socket, pid).await.expect("owned handshake");
    let response = client.send_request(&DaemonRequest::MachineGateway).await.expect("gateway negotiation");
    assert!(matches!(response, DaemonResponse::MachineGatewayOk));
    let body = serde_json::to_vec(body).expect("request JSON");
    let head = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer retirement-token\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    client.writer.write_all(head.as_bytes()).await.expect("HTTP headers");
    client.writer.write_all(&body).await.expect("HTTP body");
    let mut bytes = Vec::new();
    use tokio::io::AsyncReadExt;
    timeout(Duration::from_secs(15), client.reader.read_to_end(&mut bytes)).await.expect("bounded HTTP").expect("HTTP read");
    let text = String::from_utf8(bytes).expect("HTTP UTF8");
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or_else(|| panic!("owner retired before HTTP response: {text:?}"));
    let status = head.split_whitespace().nth(1).expect("status").parse().expect("numeric status");
    (status, if body.is_empty() { Value::Null } else { serde_json::from_str(body).expect("response JSON") })
}

#[tokio::test]
async fn final_machine_close_is_durable_before_predecessor_retires() {
    // Given: two owned subprocesses use the existing private handover harness.
    let mut daemons = PrivateDaemons::new();
    let remote = daemons.root.path().join("data/remote");
    std::fs::create_dir_all(&remote).expect("private remote data");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock").as_secs();
    std::fs::write(remote.join("remote-auth.json"), json!({"devices":{"retirement":{"id":"retirement","name":"fixture","permission":"control","accessScope":"machine","createdAt":now,"lastSeenAt":now}},"tokens":{"retirement-token":"retirement"}}).to_string()).expect("private grant");
    let d1 = daemons.launch(None).await;
    let repo = daemons.repo();
    let (status, project) = http(&daemons.socket(), daemons.pid(d1), "POST", "/api/v1/workspace/projects", &json!({"requestId":uuid::Uuid::new_v4().to_string(),"repoPath":repo})).await;
    assert_eq!(status, 201, "{project}");
    let (status, session) = http(&daemons.socket(), daemons.pid(d1), "POST", "/api/v1/sessions", &json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":project["workspaceId"],"worktree":null,"cols":80,"rows":24,"inheritFromSessionId":null,"cwdRelative":null,"startup":{"kind":"shell"}})).await;
    assert_eq!(status, 201, "{session}");
    let id = session["target"]["sessionId"].as_str().expect("raw session");
    let mut old = TestDaemonClient::connect(&daemons.socket(), daemons.pid(d1)).await.expect("predecessor");
    let shell_pid = capture_shell_pid(&daemons, d1, &mut old, id, "retirement").await;
    let legacy = match old.send_request(&DaemonRequest::PrepareHandover).await.expect("prepare") {
        DaemonResponse::PrepareHandoverOk { legacy_socket_path, .. } => PathBuf::from(legacy_socket_path),
        response => panic!("prepare response: {response:?}"),
    };
    let d2 = daemons.launch(Some(&legacy)).await;
    let request_id = uuid::Uuid::new_v4().to_string();
    let request = json!({"requestId":request_id,"daemonEpoch":session["target"]["daemonEpoch"]});
    let mut exits = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::child()).expect("subscribe child exit before close");
    // When: replacement forwards the final machine close to the draining owner.
    let (status, body) = http(&daemons.socket(), daemons.pid(d2), "DELETE", &format!("/api/v1/sessions/{id}"), &request).await;
    // Then: the response and durable operation exist before clean retirement.
    assert_eq!(status, 204, "final close must finish before retirement: {body}");
    let exit = timeout(Duration::from_secs(15), async {
        loop {
            if let Some(status) = daemons.children[d1].try_wait().expect("predecessor wait") { break status; }
            exits.recv().await.expect("child exit signal");
        }
    }).await.expect("bounded predecessor retirement");
    assert!(exit.success());
    let journal = std::fs::read_to_string(remote.join("machine-operations.v1.json")).expect("durable operation journal");
    let journal: Value = serde_json::from_str(&journal).expect("journal JSON");
    let key = serde_json::to_string(&("retirement", &request_id)).expect("journal key");
    let record = &journal["records"][&key];
    assert_eq!(record["status"], 204);
    assert_eq!(record["operation"]["state"], "completed");
    assert_eq!(record["operation"]["outcome"]["kind"], "noContent");
    assert_eq!(journal["sessions"][id]["session"]["running"], false);
    eprintln!("RETIREMENT durable close request={request_id} status204=true state=completed outcome=noContent owner={} shell={shell_pid}", daemons.pid(d1));
    let (status, operation) = http(&daemons.socket(), daemons.pid(d2), "GET", &format!("/api/v1/workspace/operations/{request_id}"), &Value::Null).await;
    assert_eq!(status, 200, "{operation}");
    assert_eq!(operation["state"], "completed", "{operation}");
    assert_eq!(operation["outcome"]["kind"], "noContent");
    let (replayed, body) = http(&daemons.socket(), daemons.pid(d2), "DELETE", &format!("/api/v1/sessions/{id}"), &request).await;
    assert_eq!(replayed, 204, "durable close replay after retirement: {body}");
    eprintln!("RETIREMENT final assertion predecessor={} replacement={} shell={} close204=true durable_operation={request_id} predecessor_reaped={exit}", daemons.pid(d1), daemons.pid(d2), shell_pid);
    assert!(daemons.cleanup().is_empty());
    let root = daemons.root.path().to_owned();
    drop(daemons);
    eprintln!("RETIREMENT cleanup children_reaped=true drains_joined=true listeners_closed=true root_removed={}", root.display());
}
