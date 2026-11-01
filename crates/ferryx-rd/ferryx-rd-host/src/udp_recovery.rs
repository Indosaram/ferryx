use super::*;
use std::collections::{HashMap, VecDeque};

/// One lock owns reads of the shared socket. Registered endpoints have bounded
/// inboxes; unknown endpoints stay in admission, never consumed by a live peer.
#[derive(Default)]
pub(super) struct UdpRecoveryRoutes {
    pub(super) peers: HashMap<SocketAddr, VecDeque<Vec<u8>>>,
    pub(super) admission: VecDeque<(SocketAddr, Vec<u8>)>,
}
impl UdpRecoveryRoutes {
    pub fn register(&mut self, peer: SocketAddr) {
        self.peers.entry(peer).or_default();
    }
    pub fn remove(&mut self, peer: SocketAddr) {
        self.peers.remove(&peer);
    }
    pub(super) fn drain(&mut self, socket: &UdpSocket) -> io::Result<()> {
        let mut buffer = [0; 2048];
        for _ in 0..MAX_UDP_DISCOVERY_BURST {
            match socket.recv_from(&mut buffer) {
                Ok((length, peer)) => {
                    let packet = buffer[..length].to_vec();
                    if let Some(queue) = self.peers.get_mut(&peer) {
                        if queue.len() == 64 {
                            queue.pop_front();
                        }
                        queue.push_back(packet);
                    } else {
                        if self.admission.len() == 64 {
                            self.admission.pop_front();
                        }
                        self.admission.push_back((peer, packet));
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error)
                    if error.kind() == io::ErrorKind::ConnectionReset
                        || error.raw_os_error() == Some(10054)
                        || error.raw_os_error() == Some(10040) =>
                {
                    continue
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
    pub fn discovery(
        &mut self,
        socket: &UdpSocket,
        ip: std::net::IpAddr,
    ) -> io::Result<Option<(Vec<u8>, SocketAddr)>> {
        self.drain(socket)?;
        let Some(index) = self.admission.iter().position(|(peer, _)| peer.ip() == ip) else {
            return Ok(None);
        };
        let (peer, bytes) = self.admission.remove(index).expect("position exists");
        Ok(Some((bytes, peer)))
    }
    pub(super) fn receive(
        &mut self,
        socket: &UdpSocket,
        peer: SocketAddr,
    ) -> io::Result<VecDeque<Vec<u8>>> {
        self.drain(socket)?;
        Ok(self
            .peers
            .get_mut(&peer)
            .map(std::mem::take)
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routes_preserve_foreign_admission_and_authenticate_nacks() {
        let directory = tempfile::tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing.json"));
        let (consent, _rx) = mpsc::channel();
        let server = HostServer::bind(HostConfig {
            tcp_addr: "127.0.0.1:0".parse().unwrap(),
            udp_addr: "127.0.0.1:0".parse().unwrap(),
            bootstrap_pin: Some("12345678".into()),
            pairing_window: Duration::from_secs(5),
            pairing_store: store,
            host_name: "recovery".into(),
            display: DisplayInfo {
                desktop_x: 0,
                desktop_y: 0,
                logical_width: 2,
                logical_height: 2,
                pixel_width: 2,
                pixel_height: 2,
                scale_factor_milli: 1000,
            },
            frames_per_second: 60,
            bitrate: 12_000_000,
            capture_audio: false,
            output_name: None,
            consent_sender: Some(consent),
            allow_input: Arc::new(AtomicBool::new(true)),
            allow_clipboard: Arc::new(AtomicBool::new(true)),
        })
        .unwrap();
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let other = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = peer.local_addr().unwrap();
        server.udp_recovery.lock().unwrap().register(addr);
        let mut send = DatagramCipher::derive(&[7; 32], &[8; 16], Direction::ClientToHost).unwrap();
        let mut recv = DatagramCipher::derive(&[7; 32], &[8; 16], Direction::ClientToHost).unwrap();
        let nack = ferryx_rd_proto::FrameNack {
            frame_id: 5,
            loss_permille: 20,
            rtt_ms: 10,
            missing: vec![1, 2],
        };
        let packet = send
            .seal_datagram(
                &PacketHeader::new(PacketType::FrameNack, 1, 0, 0),
                &nack.encode().unwrap(),
            )
            .unwrap();
        let (tx, rx) = mpsc::sync_channel(32);
        // Preload bounded route queues to test demux/auth without timing races.
        {
            let mut routes = server.udp_recovery.lock().unwrap();
            routes
                .admission
                .push_back((other.local_addr().unwrap(), vec![9]));
            let q = routes.peers.get_mut(&addr).unwrap();
            let mut tampered = packet.clone();
            *tampered.last_mut().unwrap() ^= 1;
            q.push_back(tampered);
            q.push_back(packet.clone());
            q.push_back(packet);
        }
        server
            .receive_udp_datagrams(addr, &mut recv, Some(&tx), None)
            .unwrap();
        let received_nack = rx.try_recv().unwrap();
        assert_eq!(received_nack, nack);
        assert_eq!(rx.try_recv().unwrap_err(), mpsc::TryRecvError::Empty);
        // Exercise the same scheduler entry point used by the live sender,
        // using feedback authenticated by receive_udp_datagrams above.
        let now = Instant::now();
        let mut sender = UdpSender::default();
        sender.recovery.enabled = true;
        sender.recovery.begin(5, now);
        let chunks = [vec![1, 2], vec![3, 4], vec![5]];
        let mut frames = ferryx_rd_app::FrameAssembler::default();
        frames
            .push_header(
                FrameHeader {
                    frame_id: 5,
                    width: 2,
                    height: 2,
                    is_key_frame: true,
                    total_chunks: 3,
                    total_size: 5,
                },
                42,
                now,
            )
            .unwrap();
        frames
            .push_chunk(
                ferryx_rd_proto::FrameChunk {
                    frame_id: 5,
                    chunk_index: 0,
                    data: chunks[0].clone(),
                },
                now,
            )
            .unwrap();
        for (index, data) in chunks.iter().enumerate().skip(1) {
            sender.recovery.retain(
                index as u16,
                &ferryx_rd_proto::FrameChunk {
                    frame_id: 5,
                    chunk_index: index as u16,
                    data: data.clone(),
                }
                .encode()
                .unwrap(),
            );
        }
        sender.recovery.enqueue(received_nack, now);
        let mut host_send =
            DatagramCipher::derive(&[7; 32], &[8; 16], Direction::HostToClient).unwrap();
        let mut client_receive =
            DatagramCipher::derive(&[7; 32], &[8; 16], Direction::HostToClient).unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        sender
            .send_retransmit_burst(&server.udp_socket, addr, &mut host_send, now)
            .unwrap();
        let mut completed = None;
        for _ in 0..2 {
            let mut buffer = [0; 2048];
            let (length, _) = peer.recv_from(&mut buffer).unwrap();
            let (header, payload) = client_receive.open_datagram(&buffer[..length]).unwrap();
            assert_eq!(header.packet_type, PacketType::FrameChunk);
            completed = frames
                .push_chunk(ferryx_rd_proto::FrameChunk::decode(&payload).unwrap(), now)
                .unwrap()
                .or(completed);
        }
        assert_eq!(completed.unwrap().data, [1, 2, 3, 4, 5]);
        let paced_until = sender.retransmit_at.unwrap();
        sender.recovery.enqueue(nack.clone(), now);
        sender
            .send_retransmit_burst(&server.udp_socket, addr, &mut host_send, now)
            .unwrap();
        peer.set_nonblocking(true).unwrap();
        assert_eq!(
            peer.recv_from(&mut [0; 2048]).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(paced_until > now);
        let mut routes = server.udp_recovery.lock().unwrap();
        assert_eq!(
            routes.discovery(&server.udp_socket, addr.ip()).unwrap(),
            Some((vec![9], other.local_addr().unwrap()))
        );
        routes.remove(addr);
        assert!(routes.peers.is_empty());
    }
}
