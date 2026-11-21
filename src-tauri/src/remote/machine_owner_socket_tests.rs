use super::*;
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt, DuplexStream};

const TIMEOUT: Duration = Duration::from_secs(3);

async fn write_owner_reply(mut io: DuplexStream, response: &[u8]) {
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
        request.push(io.read_u8().await.unwrap());
    }
    io.write_all(response).await.unwrap();
}

#[tokio::test]
async fn rejection_body_is_not_limited_to_the_upgrade_header_read() {
    let body = b"CONTROL_CONFLICT\nOther phone";
    for chunked in [false, true] {
        let mut wire = b"HTTP/1.1 409 Conflict\r\nContent-Type: text/plain\r\n".to_vec();
        if chunked {
            wire.extend_from_slice(b"Transfer-Encoding: chunked\r\n\r\n");
            for part in body.chunks(7) {
                wire.extend_from_slice(format!("{:x}\r\n", part.len()).as_bytes());
                wire.extend_from_slice(part);
                wire.extend_from_slice(b"\r\n");
            }
            wire.extend_from_slice(b"0\r\n\r\n");
        } else {
            wire.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
            wire.extend_from_slice(body);
        }
        // A one-byte transport forces the final header byte to be read without any body.
        let (client, owner) = duplex(1);
        let request = "ws://localhost/terminal".into_client_request().unwrap();
        let (reply, ()) = tokio::time::timeout(TIMEOUT, async {
            tokio::join!(owner_handshake(client, request, true), write_owner_reply(owner, &wire))
        }).await.unwrap();
        let Err(response) = reply.unwrap() else { panic!("owner refused the upgrade"); };
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(to_bytes(response.into_body(), 1024).await.unwrap().as_ref(), body);
    }
}

#[tokio::test]
async fn approved_upgrade_retains_the_first_frame_read_with_its_headers() {
    let request = "ws://localhost/terminal".into_client_request().unwrap();
    let accept = derive_accept_key(request.headers()[header::SEC_WEBSOCKET_KEY].as_bytes());
    let mut wire = format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").into_bytes();
    wire.extend_from_slice(b"\x82\x02OK"); // One complete binary frame in the same write.
    let (client, owner) = duplex(1024);
    let (bytes, ()) = tokio::time::timeout(TIMEOUT, async {
        tokio::join!(async {
            let Ok(mut socket) = owner_handshake(client, request, false).await.unwrap() else {
                panic!("expected a validated owner upgrade");
            };
            socket.next().await.unwrap().unwrap().into_data()
        }, write_owner_reply(owner, &wire))
    }).await.unwrap();
    assert_eq!(bytes.as_ref(), b"OK");
}
