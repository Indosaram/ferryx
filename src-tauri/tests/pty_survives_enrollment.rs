use std::sync::Arc;
use std::time::{Duration, Instant};

use ferryx_lib::account::mailer::FileMailer;
use ferryx_lib::account::service::AccountState;
use ferryx_lib::terminal::{PtyManager, TerminalOutputHub, TerminalService};
use ferryx_lib::worktree::WorkspaceRegistry;

async fn serve_accounts(
    data_dir: &std::path::Path,
    mail_dir: &std::path::Path,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let origin = format!("http://{addr}");
    let state = AccountState::new(data_dir, origin.clone(), Arc::new(FileMailer::with_dir(mail_dir)));
    let handle = tokio::spawn(async move {
        let _ = ferryx_lib::account::service::serve(listener, Arc::new(state)).await;
    });
    (origin, handle)
}

async fn post(
    base: &str,
    path: &str,
    body: serde_json::Value,
    bearer: Option<&str>,
) -> (u16, serde_json::Value) {
    let client = reqwest::Client::new();
    let mut request = client.post(format!("{base}{path}")).json(&body);
    if let Some(token) = bearer {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.expect("request");
    let status = response.status().as_u16();
    let text = response.text().await.expect("body");
    (status, serde_json::from_str(&text).unwrap_or(serde_json::Value::Null))
}

fn read_link(mail_dir: &std::path::Path) -> String {
    let entry = std::fs::read_dir(mail_dir)
        .expect("mail dir")
        .filter_map(Result::ok)
        .find(|entry| entry.path().is_file())
        .expect("a mail file");
    std::fs::read_to_string(entry.path()).expect("mail body")
}

async fn read_until(
    rx: &mut tokio::sync::broadcast::Receiver<Vec<u8>>,
    seen: &mut Vec<u8>,
    needle: &str,
    budget: Duration,
) -> bool {
    let deadline = Instant::now() + budget;
    if String::from_utf8_lossy(seen).contains(needle) {
        return true;
    }
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(chunk)) => {
                seen.extend_from_slice(&chunk);
                if String::from_utf8_lossy(seen).contains(needle) {
                    return true;
                }
            }
            Ok(Err(_)) => return false,
            Err(_) => return false,
        }
    }
    false
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_live_pty_survives_machine_enrollment() {
    let account_dir = tempfile::tempdir().expect("account data");
    let mail_dir = tempfile::tempdir().expect("mail");
    let machine_dir = tempfile::tempdir().expect("machine data");
    let workspace = tempfile::tempdir().expect("workspace");

    let (base, server) = serve_accounts(account_dir.path(), mail_dir.path()).await;

    let (status, _) = post(
        &base,
        "/api/account/v1/login/request",
        serde_json::json!({ "email": "owner@b.co" }),
        None,
    )
    .await;
    assert_eq!(status, 202);
    let link = read_link(mail_dir.path());
    let code = link
        .split("code=")
        .nth(1)
        .expect("code param")
        .trim()
        .to_string();
    let (status, body) = post(
        &base,
        "/api/account/v1/login/consume",
        serde_json::json!({ "code": code }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let session = body["token"].as_str().expect("token").to_string();
    let (status, body) = post(
        &base,
        "/api/account/v1/enrollment-codes",
        serde_json::json!({}),
        Some(&session),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let enrollment_code = body["code"].as_str().expect("enrollment code").to_string();

    let service = Arc::new(TerminalService::new(
        Arc::new(PtyManager::new()),
        Arc::new(TerminalOutputHub::default()),
    ));
    let registry = WorkspaceRegistry::new();
    registry
        .register("pty-survival", workspace.path())
        .expect("register workspace");
    let manager = registry.manager("pty-survival").expect("manager");
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    command.cwd(workspace.path());
    let (session_id, _exit_rx) = service
        .spawn_in_worktree(command, 80, 24, &manager, workspace.path())
        .expect("spawn pty");
    let (history, mut rx) = service
        .output_hub()
        .subscribe(&session_id)
        .expect("subscribe to the new session");
    let mut seen = history;

    service
        .write_input(&session_id, b"echo PTY_ALIVE_1\n")
        .expect("write before enrollment");
    assert!(
        read_until(&mut rx, &mut seen, "PTY_ALIVE_1", Duration::from_secs(10)).await,
        "the shell must answer before enrollment: {}",
        String::from_utf8_lossy(&seen)
    );

    let previous = std::env::var_os("FERRYX_DATA_DIR");
    std::env::set_var("FERRYX_DATA_DIR", machine_dir.path());
    let enrolled = ferryx_lib::account::enroll_client::enroll_machine(&base, &enrollment_code).await;
    if let Some(value) = previous {
        std::env::set_var("FERRYX_DATA_DIR", value);
    } else {
        std::env::remove_var("FERRYX_DATA_DIR");
    }
    let record = enrolled.expect("enrollment over the real account service");
    assert!(!record.machine_record_id.is_empty());
    assert!(
        machine_dir
            .path()
            .join("remote")
            .join("account-enrollment.json")
            .is_file(),
        "enrollment writes its record beside the machine identity"
    );

    service
        .write_input(&session_id, b"echo PTY_ALIVE_2\n")
        .expect("write after enrollment");
    assert!(
        read_until(&mut rx, &mut seen, "PTY_ALIVE_2", Duration::from_secs(10)).await,
        "the same session must still run after enrollment: {}",
        String::from_utf8_lossy(&seen)
    );
    assert!(
        service.output_hub().subscribe(&session_id).is_some(),
        "the session must still be registered in the output hub"
    );

    server.abort();
}
