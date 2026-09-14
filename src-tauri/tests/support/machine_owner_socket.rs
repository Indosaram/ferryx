use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};
type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
const DEADLINE: Duration = Duration::from_secs(15);

pub async fn attach(base: &str, target: &Value) -> (Socket, Value) {
    let client = reqwest::Client::builder().no_proxy().timeout(DEADLINE).build().expect("ticket client");
    let response = client.post(format!("{base}/api/v1/socket-ticket")).bearer_auth("owner-token")
        .header("content-type", "application/json").body(json!({"target":format!("/api/v1/terminal/{}", target["sessionId"].as_str().expect("ID"))}).to_string()).send().await.expect("ticket response");
    assert_eq!(response.status(), 200);
    let ticket: Value = serde_json::from_str(&response.text().await.expect("ticket body")).expect("ticket JSON");
    let url = format!("{}/api/v1/terminal/{}?ticket={}&daemonEpoch={}", base.replacen("http", "ws", 1), target["sessionId"].as_str().expect("ID"), ticket["ticket"].as_str().expect("ticket"), target["daemonEpoch"].as_str().expect("epoch"));
    let (mut socket, _) = tokio::time::timeout(DEADLINE, connect_async(url)).await.expect("bounded attach").expect("owner attach");
    let Message::Text(text) = next(&mut socket).await else { panic!("attached boundary") };
    let boundary: Value = serde_json::from_str(&text).expect("boundary JSON");
    assert_eq!(boundary["target"], *target);
    (socket, boundary)
}
async fn next(socket: &mut Socket) -> Message {
    tokio::time::timeout(DEADLINE, socket.next()).await.expect("bounded socket read").expect("socket open").expect("socket message")
}
pub async fn proof(socket: &mut Socket) -> String {
    socket.send(Message::Binary(b"printf '\\nA10_%s:%s:%s:END\\n' HANDOVER \"$$\" \"$PWD\"\r".to_vec().into())).await.expect("owner input");
    tokio::time::timeout(DEADLINE, async {
        let mut bytes = Vec::new();
        loop {
            if let Message::Binary(frame) = next(socket).await {
                bytes.extend_from_slice(ferryx_lib::remote::terminal_wire::decode_frame(&frame).expect("wire frame").terminal_bytes);
            }
            let text = String::from_utf8_lossy(&bytes);
            if let Some((_, tail)) = text.split_once("A10_HANDOVER:") {
                if let Some((proof, _)) = tail.split_once(":END") { return proof.to_owned(); }
            }
        }
    }).await.expect("PTY marker")
}
pub async fn resize(socket: &mut Socket, generation: &Value) {
    socket.send(Message::Text(json!({"type":"resize","generation":generation,"cols":103,"rows":37}).to_string().into())).await.expect("resize");
    socket.send(Message::Text(json!({"type":"ping"}).to_string().into())).await.expect("barrier");
    loop { if let Message::Text(text) = next(socket).await { if serde_json::from_str::<Value>(&text).expect("control")["type"] == "pong" { break; } } }
}
pub async fn denied(base: &str, target: &Value, token: &str, status: u16) {
    let client = reqwest::Client::builder().no_proxy().timeout(DEADLINE).build().expect("ticket client");
    let response = client.post(format!("{base}/api/v1/socket-ticket")).bearer_auth(token)
        .header("content-type", "application/json").body(json!({"target":format!("/api/v1/terminal/{}", target["sessionId"].as_str().expect("ID"))}).to_string()).send().await.expect("ticket");
    assert_eq!(response.status(), 200);
    let ticket: Value = serde_json::from_str(&response.text().await.expect("body")).expect("JSON");
    let url = format!("{}/api/v1/terminal/{}?ticket={}&daemonEpoch={}", base.replacen("http", "ws", 1), target["sessionId"].as_str().expect("ID"), ticket["ticket"].as_str().expect("ticket"), target["daemonEpoch"].as_str().expect("epoch"));
    let error = tokio::time::timeout(DEADLINE, connect_async(url)).await.expect("bounded denial").expect_err("socket denied");
    assert!(matches!(error, tokio_tungstenite::tungstenite::Error::Http(ref response) if response.status().as_u16() == status), "{error}");
}
pub async fn stale_resize(socket: &mut Socket, generation: &Value) {
    socket.send(Message::Text(json!({"type":"resize","generation":generation,"cols":31,"rows":12}).to_string().into())).await.expect("stale resize");
    loop { if let Message::Text(text) = next(socket).await { if serde_json::from_str::<Value>(&text).expect("control")["type"] == "error" { break; } } }
}
pub async fn closed(socket: &mut Socket) {
    tokio::time::timeout(DEADLINE, async {
        loop { match socket.next().await { None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break, _ => {} } }
    }).await.expect("fenced socket closes");
}
