use super::*;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;

#[tokio::test]
async fn test_fake_stun_event_driven() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let server_sock = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind STUN server");
        let server_addr = server_sock.local_addr().expect("STUN local addr");

        let client_sock = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind STUN client");

        let server_task = tokio::spawn(async move {
            let mut buf = [0u8; 128];
            let (len, client_addr) = server_sock.recv_from(&mut buf).await.expect("recv STUN req");
            assert_eq!(len, 20);

            let tx_id = &buf[8..20];
            let mut resp = Vec::new();
            resp.extend_from_slice(&STUN_BINDING_RESPONSE.to_be_bytes());

            let client_v4 = match client_addr {
                SocketAddr::V4(v4) => v4,
                _ => panic!("expected IPv4"),
            };

            let mut attr = Vec::new();
            attr.push(0x00);
            attr.push(0x01);
            let xport = client_v4.port() ^ STUN_MAGIC_COOKIE_UPPER_16;
            attr.extend_from_slice(&xport.to_be_bytes());
            let xip = u32::from(*client_v4.ip()) ^ STUN_MAGIC_COOKIE;
            attr.extend_from_slice(&xip.to_be_bytes());

            let attr_len = attr.len() as u16;
            let body_len = 4 + attr_len;

            resp.extend_from_slice(&body_len.to_be_bytes());
            resp.extend_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
            resp.extend_from_slice(tx_id);
            resp.extend_from_slice(&STUN_ATTR_XOR_MAPPED_ADDRESS.to_be_bytes());
            resp.extend_from_slice(&attr_len.to_be_bytes());
            resp.extend_from_slice(&attr);

            server_sock
                .send_to(&resp, client_addr)
                .await
                .expect("send STUN resp");
        });

        let discovered = discover_public_endpoint(&client_sock, &[server_addr])
            .await
            .expect("STUN discovery should succeed");

        let client_local_v4 = match client_sock.local_addr().expect("client addr") {
            SocketAddr::V4(v4) => v4,
            _ => panic!("expected IPv4"),
        };
        assert_eq!(discovered, client_local_v4);
        server_task.await.expect("server task finish");
    })
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn test_nonce_punch_mutual_confirmation() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let sock_a = UdpSocket::bind("127.0.0.1:0").await.expect("bind sock_a");
        let sock_b = UdpSocket::bind("127.0.0.1:0").await.expect("bind sock_b");

        let addr_a = match sock_a.local_addr().expect("addr a") {
            SocketAddr::V4(v4) => v4,
            _ => panic!("expected v4"),
        };
        let addr_b = match sock_b.local_addr().expect("addr b") {
            SocketAddr::V4(v4) => v4,
            _ => panic!("expected v4"),
        };

        let nonce_a = [1u8; 32];
        let nonce_b = [2u8; 32];

        let punch_a = execute_nonce_punch(&sock_a, addr_b, nonce_a, nonce_b, Duration::from_secs(3));
        let punch_b = execute_nonce_punch(&sock_b, addr_a, nonce_b, nonce_a, Duration::from_secs(3));

        let (res_a, res_b) = tokio::join!(punch_a, punch_b);
        res_a.expect("punch a failed");
        res_b.expect("punch b failed");
    })
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn test_nonce_punch_unreachable_timeout() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let sock = UdpSocket::bind("127.0.0.1:0").await.expect("bind sock");
        let silent_peer = UdpSocket::bind("127.0.0.1:0").await.expect("bind silent peer");
        let silent_addr = match silent_peer.local_addr().expect("silent addr") {
            SocketAddr::V4(v4) => v4,
            _ => panic!("expected ipv4"),
        };

        let res = execute_nonce_punch(
            &sock,
            silent_addr,
            [1u8; 32],
            [2u8; 32],
            Duration::from_millis(100),
        )
        .await;

        match res {
            Err(PunchError::Timeout) => {}
            other => panic!("expected Timeout error, got {other:?}"),
        }

        drop(silent_peer);
    })
    .await
    .expect("test timeout");
}

#[tokio::test]
async fn test_oversized_non_punch_packet_transition() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let sock_a = UdpSocket::bind("127.0.0.1:0").await.expect("bind sock_a");
        let sock_b = UdpSocket::bind("127.0.0.1:0").await.expect("bind sock_b");

        let addr_a = match sock_a.local_addr().expect("addr a") {
            SocketAddr::V4(v4) => v4,
            _ => panic!("expected v4"),
        };
        let addr_b = match sock_b.local_addr().expect("addr b") {
            SocketAddr::V4(v4) => v4,
            _ => panic!("expected v4"),
        };

        let nonce_a = [10u8; 32];
        let nonce_b = [20u8; 32];

        let task_b = tokio::spawn(async move {
            execute_nonce_punch(&sock_b, addr_a, nonce_b, nonce_a, Duration::from_secs(3)).await?;
            Ok::<_, PunchError>(sock_b)
        });

        let probe = create_punch_packet(PUNCH_MSG_PROBE, &nonce_a, &nonce_b);
        sock_a
            .send_to(&probe, SocketAddr::V4(addr_b))
            .await
            .expect("send probe");

        let mut recv_buf = [0u8; 256];
        loop {
            let (len, from) = sock_a.recv_from(&mut recv_buf).await.expect("recv from sock_a");
            if from == SocketAddr::V4(addr_b)
                && len == PUNCH_PACKET_LEN
                && recv_buf[0..8] == PUNCH_MAGIC
                && recv_buf[8] == PUNCH_MSG_ACK
                && recv_buf[9..41] == nonce_b
                && recv_buf[41..73] == nonce_a
            {
                break;
            }
        }

        let simulated_quic_packet = vec![0x80u8; 1200];
        sock_a
            .send_to(&simulated_quic_packet, SocketAddr::V4(addr_b))
            .await
            .expect("send simulated quic");

        let sock_b = task_b
            .await
            .expect("task b join")
            .expect("task b punch");

        let mut quic_buf = vec![0u8; 2048];
        let (recv_len, from) = sock_b
            .recv_from(&mut quic_buf)
            .await
            .expect("recv queued quic packet");
        assert_eq!(from, SocketAddr::V4(addr_a));
        assert_eq!(recv_len, 1200);
        assert_eq!(&quic_buf[..1200], &simulated_quic_packet[..]);
    })
    .await
    .expect("test timeout");
}
