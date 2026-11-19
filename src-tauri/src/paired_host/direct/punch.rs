use std::io::ErrorKind;
use std::net::SocketAddrV4;
use std::time::Duration;
use thiserror::Error;
use tokio::net::UdpSocket;

pub const PUNCH_MAGIC: [u8; 8] = *b"FXPUNCH1";
pub const PUNCH_MSG_PROBE: u8 = 0x01;
pub const PUNCH_MSG_ACK: u8 = 0x02;
pub const PUNCH_PACKET_LEN: usize = 73;

#[derive(Debug, Error)]
pub enum PunchError {
    #[error("I/O error during UDP hole punch: {0}")]
    Io(#[from] std::io::Error),
    #[error("UDP hole punch timed out without mutual confirmation")]
    Timeout,
    #[error("Peer nonce verification failed")]
    InvalidNonce,
}

pub fn create_punch_packet(
    msg_type: u8,
    sender_nonce: &[u8; 32],
    expected_peer_nonce: &[u8; 32],
) -> [u8; PUNCH_PACKET_LEN] {
    let mut packet = [0u8; PUNCH_PACKET_LEN];
    packet[0..8].copy_from_slice(&PUNCH_MAGIC);
    packet[8] = msg_type;
    packet[9..41].copy_from_slice(sender_nonce);
    packet[41..73].copy_from_slice(expected_peer_nonce);
    packet
}

fn is_transient_reset(err: &std::io::Error) -> bool {
    let kind_reset = err.kind() == ErrorKind::ConnectionReset;

    #[cfg(windows)]
    let os_reset = err.raw_os_error() == Some(10054);
    #[cfg(not(windows))]
    let os_reset = false;

    kind_reset || os_reset
}

pub async fn execute_nonce_punch(
    socket: &UdpSocket,
    peer_endpoint: SocketAddrV4,
    local_nonce: [u8; 32],
    peer_nonce: [u8; 32],
    timeout: Duration,
) -> Result<(), PunchError> {
    let peer_addr = std::net::SocketAddr::V4(peer_endpoint);
    let probe_packet = create_punch_packet(PUNCH_MSG_PROBE, &local_nonce, &peer_nonce);
    let ack_packet = create_punch_packet(PUNCH_MSG_ACK, &local_nonce, &peer_nonce);

    let deadline = tokio::time::Instant::now() + timeout;
    let mut send_interval = tokio::time::interval(Duration::from_millis(50));
    send_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut peer_authenticated = false;
    let mut peer_ack_received = false;
    let mut peek_buf = vec![0u8; 65535];
    let mut discard_buf = vec![0u8; 65535];

    while tokio::time::Instant::now() < deadline {
        tokio::select! {
            _ = send_interval.tick() => {
                match socket.send_to(&probe_packet, peer_addr).await {
                    Ok(_) => {}
                    Err(e) if is_transient_reset(&e) => {}
                    Err(e) => return Err(PunchError::Io(e)),
                }
                if peer_authenticated {
                    match socket.send_to(&ack_packet, peer_addr).await {
                        Ok(_) => {}
                        Err(e) if is_transient_reset(&e) => {}
                        Err(e) => return Err(PunchError::Io(e)),
                    }
                }
            }
            peek_res = socket.peek_from(&mut peek_buf) => {
                match peek_res {
                    Ok((len, from)) => {
                        if from == peer_addr {
                            if len >= 8 && peek_buf[0..8] == PUNCH_MAGIC {
                                match socket.recv_from(&mut discard_buf).await {
                                    Ok((recv_len, recv_from)) => {
                                        if recv_from == peer_addr && recv_len == PUNCH_PACKET_LEN {
                                            let msg_type = discard_buf[8];
                                            let incoming_sender_nonce = &discard_buf[9..41];
                                            let incoming_expected_nonce = &discard_buf[41..73];

                                            if incoming_sender_nonce == peer_nonce && incoming_expected_nonce == local_nonce {
                                                peer_authenticated = true;
                                                if msg_type == PUNCH_MSG_PROBE {
                                                    socket.send_to(&ack_packet, peer_addr).await?;
                                                } else if msg_type == PUNCH_MSG_ACK {
                                                    peer_ack_received = true;
                                                }
                                            }
                                        }
                                    }
                                    Err(e) if is_transient_reset(&e) => {}
                                    Err(e) => return Err(PunchError::Io(e)),
                                }
                            } else if peer_authenticated {
                                return Ok(());
                            } else {
                                let _ = socket.recv_from(&mut discard_buf).await;
                            }
                        } else {
                            let _ = socket.recv_from(&mut discard_buf).await;
                        }
                    }
                    Err(e) => {
                        if is_transient_reset(&e) {
                            continue;
                        }
                        return Err(PunchError::Io(e));
                    }
                }
            }
        }

        if peer_authenticated && peer_ack_received {
            for _ in 0..3 {
                let _ = socket.send_to(&ack_packet, peer_addr).await;
            }
            return Ok(());
        }
    }

    Err(PunchError::Timeout)
}
