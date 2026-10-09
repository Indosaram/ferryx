use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use futures_util::{SinkExt as _, StreamExt as _};
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CapturedJson {
    machine_id: String,
    session_id: String,
    enrollment_epoch: String,
    frame: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ControlChallenge {
    nonce: String,
    timestamp: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    audience: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ControlAuth {
    #[serde(skip_serializing_if = "Option::is_none")]
    enrollment_token: Option<String>,
    machine_id: String,
    display_name: String,
    public_key: String,
    signature: String,
    timestamp: u64,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ControlAuthResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SessionNotice {
    session_id: String,
    opaque: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let result = tokio::time::timeout(Duration::from_secs(10), run_pipeline()).await;
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => {
            eprintln!("[PIPELINE_ERROR]: {e}");
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("[PIPELINE_ERROR]: Pipeline timed out after 10s");
            std::process::exit(2);
        }
    }
}

async fn run_pipeline() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let captured_json_path = args.next().ok_or("Arg 1: path to captured-noise-msg1.json required")?;
    let copied_auth_dir = args.next().ok_or("Arg 2: path to copied private auth dir required")?;

    let json_bytes = std::fs::read(&captured_json_path)?;
    let captured: CapturedJson = serde_json::from_slice(&json_bytes)?;

    let auth_dir = PathBuf::from(&copied_auth_dir);
    let identity = ferryx_lib::remote::auth::load_or_generate_machine_identity(&auth_dir)
        .map_err(|e| format!("load machine identity: {e}"))?;
    let auth_manager = ferryx_lib::remote::auth::AuthManager::with_persistence(Some(auth_dir.join("remote-auth.json")));

    assert_eq!(
        captured.machine_id, identity.machine_id,
        "Captured machineId must match canonical loaded machine identity"
    );

    let enrollment_path = auth_dir.join("account-enrollment.json");
    if enrollment_path.exists() {
        let enrollment_bytes = std::fs::read(&enrollment_path)?;
        let enrollment_json: serde_json::Value = serde_json::from_slice(&enrollment_bytes)?;
        let loaded_epoch = enrollment_json.get("enrollmentEpoch")
            .and_then(|v| v.as_str())
            .unwrap_or("1");
        assert_eq!(
            captured.enrollment_epoch, loaded_epoch,
            "Captured enrollmentEpoch must match canonical enrollment file"
        );
    } else {
        eprintln!("[PIPELINE]: No account-enrollment.json present in copied auth dir, epoch: {}", captured.enrollment_epoch);
    }

    std::env::set_var("FERRYX_DATA_DIR", auth_dir.parent().unwrap_or(&auth_dir));

    let gateway_listener = TcpListener::bind("127.0.0.1:0").await?;
    let gateway_addr = gateway_listener.local_addr()?;
    eprintln!("[PIPELINE]: Mock gateway on 127.0.0.1:{}", gateway_addr.port());

    let (gateway_ready_tx, gateway_ready_rx) = tokio::sync::oneshot::channel();
    let gateway_task = tokio::spawn(async move {
        let _ = gateway_ready_tx.send(());
        if let Ok((mut stream, _)) = gateway_listener.accept().await {
            let mut buf = [0u8; 1024];
            while let Ok(n) = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await {
                if n == 0 { break; }
                eprintln!("[GATEWAY]: Plaintext traffic received: {} bytes", n);
            }
        }
    });
    gateway_ready_rx.await?;

    let relay_listener = TcpListener::bind("127.0.0.1:0").await?;
    let relay_addr = relay_listener.local_addr()?;
    let relay_url = format!("ws://127.0.0.1:{}", relay_addr.port());
    eprintln!("[PIPELINE]: Mock relay listening on {}", relay_url);

    let (control_established_tx, control_established_rx) = tokio::sync::oneshot::channel();
    let (data_channel_tx, data_channel_rx) = tokio::sync::oneshot::channel();

    let session_id_clone = captured.session_id.clone();
    let mock_relay_task = tokio::spawn(async move {
        let (stream1, _) = relay_listener.accept().await.expect("accept control");
        let mut control_ws = accept_async(stream1).await.expect("control ws handshake");

        let now_sec = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let challenge_dto = ControlChallenge {
            nonce: "mock-nonce-1234567890".into(),
            timestamp: now_sec,
            audience: None,
        };
        let challenge_str = serde_json::to_string(&challenge_dto).unwrap();
        control_ws.send(Message::Text(challenge_str.into())).await.expect("send challenge");

        let auth_raw = control_ws.next().await.expect("client auth response").expect("valid msg");
        let auth_text = match auth_raw {
            Message::Text(t) => t.to_string(),
            Message::Binary(b) => String::from_utf8_lossy(&b).to_string(),
            _ => panic!("Unexpected auth msg type"),
        };
        let parsed_auth: ControlAuth = serde_json::from_str(&auth_text).expect("valid ControlAuth DTO");
        eprintln!(
            "[MOCK_RELAY]: ControlAuth parsed successfully: machine_id={}, key_len={}",
            parsed_auth.machine_id,
            parsed_auth.public_key.len()
        );

        let auth_ack = ControlAuthResponse {
            success: true,
            error: None,
        };
        control_ws.send(Message::Text(serde_json::to_string(&auth_ack).unwrap().into()))
            .await
            .expect("send auth ack");

        let _ = control_established_tx.send(());

        let notice_dto = SessionNotice {
            session_id: session_id_clone.clone(),
            opaque: true,
        };
        control_ws.send(Message::Text(serde_json::to_string(&notice_dto).unwrap().into()))
            .await
            .expect("send session notice");

        let (stream2, _) = relay_listener.accept().await.expect("accept daemon data ws");
        let daemon_data_ws = accept_async(stream2).await.expect("daemon data ws handshake");
        let _ = data_channel_tx.send(daemon_data_ws);

        while let Some(msg) = control_ws.next().await {
            if msg.is_err() { break; }
        }
    });

    let client = ferryx_lib::remote::relay_client::RelayClient::with_identity(
        &relay_url,
        identity.clone(),
        format!("127.0.0.1:{}", gateway_addr.port()),
    )
    .with_auth_manager(auth_manager);

    let client_task = tokio::spawn(async move {
        let _ = client.run().await;
    });

    control_established_rx.await?;
    eprintln!("[PIPELINE]: Control channel authenticated");

    let mut daemon_data_ws = data_channel_rx.await?;
    eprintln!("[PIPELINE]: Daemon connected to mock data channel for opaque session {}", captured.session_id);

    eprintln!("[PIPELINE]: Forwarding captured msg1 (len={}) to daemon data WS", captured.frame.len());
    daemon_data_ws.send(Message::Binary(captured.frame.clone().into())).await
        .map_err(|e| format!("Failed to send captured msg1 to daemon: {e}"))?;

    let mut received = Vec::new();
    let mut msg2_payload = None;
    while let Some(msg) = daemon_data_ws.next().await {
        match msg {
            Ok(Message::Binary(bytes)) => {
                received.extend_from_slice(&bytes);
                if received.len() >= 4 {
                    let expected_len = u32::from_be_bytes(received[0..4].try_into().unwrap()) as usize;
                    if received.len() >= 4 + expected_len {
                        msg2_payload = Some(received[4..4 + expected_len].to_vec());
                        break;
                    }
                }
            }
            Ok(Message::Close(frame)) => {
                eprintln!("[PIPELINE]: Daemon closed data channel: {:?}", frame);
                break;
            }
            Ok(_) => continue,
            Err(e) => return Err(format!("Daemon data WS read error: {e}").into()),
        }
    }

    let payload = msg2_payload.ok_or_else(|| {
        format!("Handshake failed: received {} bytes, no complete msg2 frame", received.len())
    })?;

    if payload.len() != 48 {
        return Err(format!("Unexpected msg2 length: {} (expected 48)", payload.len()).into());
    }

    println!("PIPELINE_RESULT: PASS (RelayClient handle_session processed captured frame and returned 48-byte msg2)");

    client_task.abort();
    mock_relay_task.abort();
    gateway_task.abort();
    Ok(())
}
