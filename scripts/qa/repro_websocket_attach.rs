use std::path::PathBuf;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use futures_util::{SinkExt as _, StreamExt as _};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CapturedJson {
    machine_id: String,
    session_id: String,
    enrollment_epoch: String,
    frame: Vec<u8>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let result = tokio::time::timeout(Duration::from_secs(5), run_probe()).await;
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => {
            eprintln!("[PROBE_ERROR]: {e}");
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("[PROBE_ERROR]: Probe timed out after 5 seconds");
            std::process::exit(2);
        }
    }
}

async fn run_probe() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let captured_json_path = args.next().ok_or("Arg 1: path to captured-noise-msg1.json required")?;
    let copied_auth_dir = args.next().ok_or("Arg 2: path to copied private auth dir required")?;

    let json_bytes = std::fs::read(&captured_json_path)?;
    let captured: CapturedJson = serde_json::from_slice(&json_bytes)?;

    let auth_dir = PathBuf::from(&copied_auth_dir);
    let auth_path = auth_dir.join("remote-auth.json");

    let attach = ferryx_lib::remote::attach_identity::load_or_generate_attach_identity(&auth_dir)
        .map_err(|e| format!("load attach identity from {}: {e}", auth_dir.display()))?;
    let auth_manager = ferryx_lib::remote::auth::AuthManager::with_persistence(Some(auth_path));

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let captured_frame = captured.frame.clone();
    let client_task = tokio::spawn(async move {
        let url = format!("ws://{}", addr);
        let (mut ws, _) = connect_async(url).await.expect("client connect");
        ws.send(Message::Binary(captured_frame.into())).await.expect("client send");

        let mut received = Vec::new();
        while let Some(msg) = ws.next().await {
            match msg {
                Ok(Message::Binary(bytes)) => {
                    received.extend_from_slice(&bytes);
                    if received.len() >= 4 {
                        let expected_len = u32::from_be_bytes(received[0..4].try_into().unwrap()) as usize;
                        if received.len() >= 4 + expected_len {
                            return Ok::<Vec<u8>, String>(received[4..4 + expected_len].to_vec());
                        }
                    }
                }
                Ok(Message::Close(_)) => break,
                Ok(_) => continue,
                Err(e) => return Err(format!("WS read error: {e}")),
            }
        }
        if received.is_empty() {
            Err("Stream closed without response (EOF/zero frames)".into())
        } else {
            Err(format!("Incomplete frame received: {} bytes", received.len()))
        }
    });

    let (tcp_stream, _peer) = listener.accept().await?;
    let server_ws = accept_async(tcp_stream).await?;
    let byte_stream = ferryx_lib::remote::attach_crypto::WebSocketByteStream::new(server_ws);

    let machine_id = captured.machine_id.clone();
    let session_id = captured.session_id.clone();
    let enrollment_epoch = captured.enrollment_epoch.clone();

    let transport_result = ferryx_lib::remote::session_transport::establish_session(
        byte_stream,
        true,
        Some(&attach),
        &machine_id,
        &session_id,
        &enrollment_epoch,
        move |key: &[u8; 32]| {
            auth_manager.authorizes_attach_key_bytes(key)
        },
    ).await;

    let secure_stream = match transport_result {
        Ok(ferryx_lib::remote::session_transport::SessionTransport::Attached(secure)) => secure,
        Ok(ferryx_lib::remote::session_transport::SessionTransport::Plain(_)) => {
            return Err("Unexpected plain transport returned".into());
        }
        Err(e) => {
            return Err(format!("establish_session failed: {e:?}").into());
        }
    };

    let client_msg2 = client_task.await??;
    if client_msg2.len() != 48 {
        return Err(format!("Expected 48-byte msg2 payload, received {} bytes", client_msg2.len()).into());
    }

    println!("REPRO_RESULT: PASS (Noise msg2 48 bytes successfully received and assembled by client)");
    let _ = secure_stream;
    Ok(())
}
