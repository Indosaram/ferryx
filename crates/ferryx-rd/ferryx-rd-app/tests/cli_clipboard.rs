use std::{net::UdpSocket, time::Duration};

use ferryx_rd_net::{DatagramCipher, Direction, PskIdentity, TlsPskServer};
use ferryx_rd_proto::{
    Capabilities, ClipboardSyncDirection, ClipboardSyncOrigin, ClipboardSyncUpdate, ControlMessage,
    GuestPermissions, Handshake, PacketHeader, PacketType, SessionEndReason, SessionEnded,
    WireCodec,
};

const DEADLINE: Duration = Duration::from_secs(60);

#[path = "support/client_exe.rs"]
mod client_exe;
const PROBE: &str = "maho-c2h-cli-probe";
const HOST_TEXT: &str = "host-to-client";

fn control(message: ControlMessage) -> Vec<u8> {
    let mut packet = PacketHeader::new(PacketType::Control, 0, 0, 0)
        .encode()
        .unwrap();
    packet.extend_from_slice(&message.encode().unwrap());
    packet
}

/// Runs the real ferryx-rd-client executable against a mock host that grants (or
/// denies) clipboard, then returns what the host received plus client stdout.
async fn execute(grant_clipboard: bool) -> (Option<ClipboardSyncUpdate>, String) {
    let key = [0x41; 32];
    let server = TlsPskServer::new([PskIdentity::pairing("cli-clip", &key).unwrap()]).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tcp_port = listener.local_addr().unwrap().port();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    udp.set_read_timeout(Some(DEADLINE)).unwrap();
    let udp_port = udp.local_addr().unwrap().port();
    let peer = tokio::spawn(async move {
        let (socket, _) = tokio::time::timeout(DEADLINE, listener.accept())
            .await
            .unwrap()
            .unwrap();
        let socket = socket.into_std().unwrap();
        tokio::task::spawn_blocking(move || {
            let mut stream = server
                .accept_stream_until(socket, std::time::Instant::now() + DEADLINE)
                .unwrap();
            stream
                .ssl_stream()
                .get_ref()
                .set_read_timeout(Some(DEADLINE))
                .unwrap();
            let frame = stream.read_frame().unwrap();
            let mut handshake = Handshake::decode(&frame[PacketHeader::SIZE..]).unwrap();
            handshake.capabilities = Capabilities::AUTHENTICATED_UDP_REGISTRATION
                | Capabilities::TEXT_CLIPBOARD_SYNC
                | Capabilities::GUEST_PERMISSIONS
                | Capabilities::SESSION_ENDED;
            let mut ack = PacketHeader::new(PacketType::HandshakeAck, 0, 0, 0)
                .encode()
                .unwrap();
            ack.extend_from_slice(&handshake.encode().unwrap());
            stream.write_frame(&ack).unwrap();
            let mut probe = [0; 1024];
            let (probe_len, _) = udp.recv_from(&mut probe).unwrap();
            let mut c2h =
                DatagramCipher::derive(&key, &handshake.session_salt, Direction::ClientToHost)
                    .unwrap();
            assert_eq!(
                c2h.open_datagram(&probe[..probe_len])
                    .unwrap()
                    .0
                    .packet_type,
                PacketType::Ping
            );
            stream
                .write_frame(&control(ControlMessage::GuestPermissions(
                    GuestPermissions {
                        keyboard: true,
                        mouse: true,
                        gamepad: false,
                        clipboard: grant_clipboard,
                        audio: false,
                    },
                )))
                .unwrap();
            let mut received = None;
            if grant_clipboard {
                stream
                    .write_frame(&control(ControlMessage::ClipboardSyncUpdate(
                        ClipboardSyncUpdate {
                            request_id: 0,
                            direction: ClipboardSyncDirection::HostToClient,
                            origin: ClipboardSyncOrigin::LocalPasteboard,
                            text: HOST_TEXT.to_owned(),
                        },
                    )))
                    .unwrap();
                while received.is_none() {
                    let frame = stream.read_frame().unwrap();
                    let header = PacketHeader::decode(&frame[..PacketHeader::SIZE]).unwrap();
                    if header.packet_type != PacketType::Control {
                        continue;
                    }
                    if let ControlMessage::ClipboardSyncUpdate(update) =
                        ControlMessage::decode(&frame[PacketHeader::SIZE..]).unwrap()
                    {
                        received = Some(update);
                    }
                }
            }
            stream
                .write_frame(&control(ControlMessage::SessionEnded(SessionEnded {
                    reason: SessionEndReason::HostStopped,
                })))
                .unwrap();
            // Read until the client closes its end so any clipboard push it
            // makes after the policy or SessionEnded is still observed.
            while let Ok(frame) = stream.read_frame() {
                let header = PacketHeader::decode(&frame[..PacketHeader::SIZE]).unwrap();
                if header.packet_type != PacketType::Control {
                    continue;
                }
                if let ControlMessage::ClipboardSyncUpdate(update) =
                    ControlMessage::decode(&frame[PacketHeader::SIZE..]).unwrap()
                {
                    received.get_or_insert(update);
                }
            }
            received
        })
        .await
        .unwrap()
    });
    let temporary = tempfile::tempdir().unwrap();
    let mut command = tokio::process::Command::new(client_exe::client_exe());
    command
        .args([
            "--host",
            "127.0.0.1",
            "--tcp-port",
            &tcp_port.to_string(),
            "--udp-port",
            &udp_port.to_string(),
            "--pairing-id",
            "cli-clip",
            "--psk-hex",
            &"41".repeat(32),
            "--timeout-secs",
            "45",
            "--clipboard-send",
            PROBE,
        ])
        .arg("--pairing-store")
        .arg(temporary.path().join("pairings.json"))
        .env("RUST_LOG", "info")
        .kill_on_drop(true);
    let output = tokio::time::timeout(DEADLINE, command.output())
        .await
        .expect("executable exit deadline")
        .unwrap();
    let received = tokio::time::timeout(DEADLINE, peer)
        .await
        .expect("peer deadline")
        .unwrap();
    let mut logs = String::from_utf8_lossy(&output.stdout).into_owned();
    logs.push_str(&String::from_utf8_lossy(&output.stderr));
    (received, logs)
}

#[tokio::test]
async fn cli_clipboard_send_reaches_host_and_inbound_text_is_reported() {
    let (received, logs) = execute(true).await;
    let update = received.expect("host never received the clipboard push");
    assert_eq!(update.text, PROBE);
    assert_eq!(update.direction, ClipboardSyncDirection::ClientToHost);
    assert_eq!(update.origin, ClipboardSyncOrigin::LocalPasteboard);
    assert!(logs.contains(&format!("clipboard sent: {} bytes", PROBE.len())));
    assert!(
        logs.contains(&format!("clipboard received: {} bytes", HOST_TEXT.len())),
        "{logs}"
    );
    assert!(
        !logs.contains(HOST_TEXT),
        "clipboard content must not be logged"
    );
}

#[tokio::test]
async fn cli_clipboard_send_is_refused_when_host_denies_clipboard() {
    let (received, logs) = execute(false).await;
    assert!(received.is_none(), "host received a denied clipboard push");
    assert!(logs.contains("clipboard send refused:"), "{logs}");
    assert!(!logs.contains("clipboard sent:"), "{logs}");
}
