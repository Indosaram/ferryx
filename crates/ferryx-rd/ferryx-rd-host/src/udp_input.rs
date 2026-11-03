use super::*;

/// Session-owned input state for authenticated UDP `InputBatch` datagrams.
pub(super) struct UdpInputSink<'a> {
    pub(super) order: &'a mut ferryx_rd_proto::InputOrder,
    pub(super) input: &'a InputDispatch,
    pub(super) echo: bool,
    pub(super) permissions: GuestPermissions,
}

impl HostServer {
    /// The only consumer of a registered peer's datagrams. Socket reads are
    /// owned by `UdpRecoveryRoutes::drain`; each packet is authenticated once
    /// with the session c2h cipher and dispatched by type, so recovery NACKs
    /// and input batches never consume or desynchronize each other.
    pub(super) fn receive_udp_datagrams(
        &self,
        peer: SocketAddr,
        cipher: &mut DatagramCipher,
        nacks: Option<&mpsc::SyncSender<ferryx_rd_proto::FrameNack>>,
        mut input: Option<UdpInputSink<'_>>,
    ) -> Result<(), SessionError> {
        let packets = self
            .udp_recovery
            .lock()
            .expect("UDP recovery routes poisoned")
            .receive(&self.udp_socket, peer)?;
        for packet in packets {
            let Ok((header, payload)) = cipher.open_datagram(&packet) else {
                continue;
            };
            match header.packet_type {
                PacketType::FrameNack => {
                    let Some(tx) = nacks else { continue };
                    let Ok(nack) = ferryx_rd_proto::FrameNack::decode(&payload) else {
                        continue;
                    };
                    // A full queue drops feedback; a closed sender only ends recovery.
                    let _ = tx.try_send(nack);
                }
                PacketType::InputBatch => {
                    let Some(sink) = input.as_mut() else { continue };
                    let Ok(batch) = ferryx_rd_proto::InputBatch::decode(&payload) else {
                        continue;
                    };
                    let mut events = Vec::new();
                    for mut event in sink.order.accept(&batch, false) {
                        if !sink.permissions.mouse {
                            continue;
                        }
                        if !sink.permissions.keyboard {
                            event.modifiers = ferryx_rd_proto::Modifiers::empty();
                        }
                        events.push(event);
                    }
                    if sink.echo {
                        sink.input
                            .inject_batch(header.sequence, batch.sequence, events)?;
                    } else {
                        for event in events {
                            sink.input.inject(header.sequence, event)?;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    fn input(kind: ferryx_rd_proto::InputEventType) -> InputEvent {
        let mut event = InputEvent::decode(&[0; InputEvent::SIZE]).unwrap();
        event.event_type = kind;
        event
    }

    /// FrameNack and InputBatch from the same registered endpoint, interleaved
    /// on one sequence space, are authenticated once by the single session
    /// cipher and both delivered; the socket stays owned by the route drain.
    #[test]
    fn interleaved_nack_and_input_on_same_socket_are_both_delivered() {
        let directory = tempfile::tempdir().unwrap();
        let (consent, _rx) = mpsc::channel();
        let server = HostServer::bind(HostConfig {
            tcp_addr: "127.0.0.1:0".parse().unwrap(),
            udp_addr: "127.0.0.1:0".parse().unwrap(),
            bootstrap_pin: Some("12345678".into()),
            pairing_window: Duration::from_secs(5),
            pairing_store: PairingStore::new(directory.path().join("pairing.json")),
            host_name: "interleave".into(),
            display: DisplayInfo {
                desktop_x: 0,
                desktop_y: 0,
                logical_width: 100,
                logical_height: 100,
                pixel_width: 100,
                pixel_height: 100,
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
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        let peer = client.local_addr().unwrap();
        let foreign = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut send = DatagramCipher::derive(&[3; 32], &[4; 16], Direction::ClientToHost).unwrap();
        let mut receive =
            DatagramCipher::derive(&[3; 32], &[4; 16], Direction::ClientToHost).unwrap();
        let nack = |frame_id| ferryx_rd_proto::FrameNack {
            frame_id,
            loss_permille: 10,
            rtt_ms: 5,
            missing: vec![0],
        };
        let mut journal = ferryx_rd_proto::InputJournal::default();
        let down = journal.append(&[input(ferryx_rd_proto::InputEventType::MouseMove)]);
        let up = journal.append(&[input(ferryx_rd_proto::InputEventType::RelativeMove)]);
        let mut seal = |sequence, kind, payload: Vec<u8>| {
            send.seal_datagram(&PacketHeader::new(kind, sequence, 0, 0), &payload)
                .unwrap()
        };
        // One sequence space shared by both lanes, strictly interleaved.
        let packets = [
            seal(1, PacketType::FrameNack, nack(5).encode().unwrap()),
            seal(2, PacketType::InputBatch, down.encode().unwrap()),
            seal(3, PacketType::FrameNack, nack(6).encode().unwrap()),
            seal(4, PacketType::InputBatch, up.encode().unwrap()),
        ];
        {
            // Register both lanes on the same socket-owned route.
            let mut routes = server.udp_recovery.lock().unwrap();
            routes.register(peer);
            routes
                .admission
                .push_back((foreign.local_addr().unwrap(), vec![9]));
        }
        for packet in packets {
            client.send_to(&packet, server.udp_addr().unwrap()).unwrap();
            // Await this datagram's exact readiness before draining it. No
            // elapsed-time assumption or competing socket read is involved.
            server.udp_socket.set_nonblocking(false).unwrap();
            server
                .udp_socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut buffer = [0; 2048];
            server.udp_socket.peek_from(&mut buffer).unwrap();
            server.udp_socket.set_nonblocking(true).unwrap();
            server
                .udp_recovery
                .lock()
                .unwrap()
                .drain(&server.udp_socket)
                .unwrap();
        }

        let worker = InputDispatch::start(|| Ok(InputInjector::new(100.0, 100.0))).unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        worker
            .dispatch(move |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            })
            .unwrap();
        // The worker is parked inside the job, so injected events stay queued.
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();

        let (nack_tx, nack_rx) = mpsc::sync_channel(32);
        let mut order = ferryx_rd_proto::InputOrder::default();
        let permissions = GuestPermissions {
            mouse: true,
            keyboard: true,
            gamepad: false,
            clipboard: false,
            audio: false,
        };
        server
            .receive_udp_datagrams(
                peer,
                &mut receive,
                Some(&nack_tx),
                Some(UdpInputSink {
                    order: &mut order,
                    input: &worker,
                    echo: false,
                    permissions,
                }),
            )
            .unwrap();

        assert_eq!(nack_rx.try_recv().unwrap(), nack(5));
        assert_eq!(nack_rx.try_recv().unwrap(), nack(6));
        assert_eq!(nack_rx.try_recv().unwrap_err(), mpsc::TryRecvError::Empty);
        {
            let queue = worker.queue.0.lock().unwrap();
            let delivered: Vec<_> = queue
                .pending
                .iter()
                .map(|job| match job {
                    QueuedInput::Event(sequence, event) => (*sequence, event.event_type),
                    QueuedInput::Job(_) => panic!("only input events were dispatched"),
                })
                .collect();
            assert_eq!(
                delivered,
                [
                    (2, ferryx_rd_proto::InputEventType::MouseMove),
                    (4, ferryx_rd_proto::InputEventType::RelativeMove),
                ]
            );
        }
        let mut routes = server.udp_recovery.lock().unwrap();
        assert!(routes.peers[&peer].is_empty());
        // The foreign endpoint's datagram stays in admission, untouched.
        assert_eq!(
            routes.discovery(&server.udp_socket, peer.ip()).unwrap(),
            Some((vec![9], foreign.local_addr().unwrap()))
        );
        routes.remove(peer);
        drop(routes);
        release_tx.send(()).unwrap();
        worker.stop();
    }
}
