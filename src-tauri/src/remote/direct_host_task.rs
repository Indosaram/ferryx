use crate::paired_host::direct::{
    bridge_quic_connection_to_tcp, create_quic_endpoint, discover_public_endpoint,
    execute_nonce_punch, BridgeError, DirectConnectionOffer, EphemeralCert,
};
use std::net::{SocketAddr, SocketAddrV4};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;
use tokio::time::Instant;

pub(super) async fn discover(socket: &UdpSocket, servers: &[String]) -> Option<SocketAddrV4> {
    let mut resolved = Vec::new();
    for server in servers {
        if let Ok(addrs) = tokio::net::lookup_host(server.as_str()).await {
            resolved.extend(addrs.filter(SocketAddr::is_ipv4));
        }
    }
    discover_public_endpoint(socket, &resolved).await.ok()
}

/// Ties the separately spawned bridge to the manager-owned task's lifetime.
struct AbortOnDrop(JoinHandle<Result<(), BridgeError>>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(super) struct HostJob {
    pub socket: UdpSocket,
    pub cert: EphemeralCert,
    pub local_nonce: [u8; 32],
    pub offer: DirectConnectionOffer,
    pub gateway: SocketAddr,
    pub deadline: Duration,
    pub cancellation: Option<tokio::sync::broadcast::Receiver<()>>,
}

impl HostJob {
    pub async fn run(mut self) {
        let started = Instant::now();
        let peer = self.offer.public_endpoint;
        if let Err(error) =
            execute_nonce_punch(&self.socket, peer, self.local_nonce, self.offer.punch_nonce, self.deadline).await
        {
            tracing::debug!("direct host punch failed; client stays on relay: {error}");
            return;
        }
        let std_socket = match self.socket.into_std() {
            Ok(socket) => socket,
            Err(error) => {
                tracing::debug!("direct host socket handoff failed: {error}");
                return;
            }
        };
        let endpoint = match create_quic_endpoint(std_socket, &self.cert, &self.offer.cert_der, true) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                tracing::debug!("direct host QUIC endpoint failed: {error}");
                return;
            }
        };
        let remaining = self.deadline.saturating_sub(started.elapsed());
        let connection = tokio::time::timeout(remaining, async {
            loop {
                let incoming = endpoint.accept().await?;
                if incoming.remote_address() != SocketAddr::V4(peer) {
                    incoming.refuse();
                    continue;
                }
                return incoming.await.ok();
            }
        })
        .await;
        let Ok(Some(connection)) = connection else {
            tracing::debug!("direct host QUIC accept timed out or failed");
            endpoint.close(0u32.into(), b"accept failed");
            return;
        };
        let mut bridge = AbortOnDrop(bridge_quic_connection_to_tcp(connection, self.gateway));
        match self.cancellation.as_mut() {
            Some(cancel_rx) => {
                tokio::select! {
                    res = &mut bridge.0 => {
                        if let Ok(Err(error)) = res {
                            tracing::debug!("direct host bridge ended: {error}");
                        }
                    }
                    _ = cancel_rx.recv() => {
                        tracing::debug!("direct host bridge cancelled by lease revocation");
                    }
                }
            }
            None => {
                if let Ok(Err(error)) = (&mut bridge.0).await {
                    tracing::debug!("direct host bridge ended: {error}");
                }
            }
        }
        endpoint.close(0u32.into(), b"bridge closed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paired_host::direct::{generate_ephemeral_cert, DirectRole};
    use ed25519_dalek::SigningKey;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;
    use tokio::sync::{broadcast, oneshot};
    use tokio::task::JoinSet;

    struct TaskGuard<T>(Option<JoinHandle<T>>);

    impl<T> TaskGuard<T> {
        fn new(handle: JoinHandle<T>) -> Self {
            Self(Some(handle))
        }

        async fn join(&mut self) -> Result<T, tokio::task::JoinError> {
            let handle = self.0.as_mut().expect("task already joined");
            let res = handle.await;
            self.0.take();
            res
        }
    }

    impl<T> Drop for TaskGuard<T> {
        fn drop(&mut self) {
            if let Some(handle) = self.0.as_ref() {
                handle.abort();
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum EchoServerEvent {
        Accepted { peer: SocketAddr },
        Closed { peer: SocketAddr },
    }

    async fn spawn_guarded_echo_server() -> (
        SocketAddr,
        oneshot::Sender<()>,
        TaskGuard<Result<(), std::io::Error>>,
        broadcast::Receiver<EchoServerEvent>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind echo listener");
        let addr = listener.local_addr().expect("echo addr");
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
        let (event_tx, event_rx) = broadcast::channel(32);

        let server_task = tokio::spawn(async move {
            let mut conn_tasks: JoinSet<Result<(), std::io::Error>> = JoinSet::new();
            loop {
                while let Some(res) = conn_tasks.try_join_next() {
                    res.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))??;
                }

                tokio::select! {
                    accept_res = listener.accept() => {
                        let (mut stream, peer_addr) = accept_res?;
                        let _ = event_tx.send(EchoServerEvent::Accepted { peer: peer_addr });
                        let event_signal = event_tx.clone();
                        conn_tasks.spawn(async move {
                            let (mut reader, mut writer) = stream.split();
                            tokio::io::copy(&mut reader, &mut writer).await?;
                            writer.shutdown().await?;
                            event_signal
                                .send(EchoServerEvent::Closed { peer: peer_addr })
                                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                            Ok(())
                        });
                    }
                    _ = &mut shutdown_rx => {
                        break;
                    }
                }
            }
            while let Some(res) = conn_tasks.join_next().await {
                res.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))??;
            }
            Ok(())
        });

        (addr, shutdown_tx, TaskGuard::new(server_task), event_rx)
    }

    #[tokio::test]
    async fn test_host_job_quic_tcp_bridge_and_billing_cancellation() {
        tokio::time::timeout(Duration::from_secs(15), async {
            let (echo_addr, echo_shutdown, mut echo_server, mut echo_events_rx) =
                spawn_guarded_echo_server().await;

            let host_sock = UdpSocket::bind("127.0.0.1:0").await.expect("bind host_sock");
            let client_sock = UdpSocket::bind("127.0.0.1:0").await.expect("bind client_sock");

            let host_addr = match host_sock.local_addr().expect("host addr") {
                SocketAddr::V4(v4) => v4,
                _ => panic!("expected v4 host"),
            };
            let client_addr = match client_sock.local_addr().expect("client addr") {
                SocketAddr::V4(v4) => v4,
                _ => panic!("expected v4 client"),
            };

            let host_cert = generate_ephemeral_cert().expect("host cert");
            let host_cert_der = host_cert.cert_der.clone();
            let client_cert = generate_ephemeral_cert().expect("client cert");

            let host_nonce = [11u8; 32];
            let client_nonce = [22u8; 32];

            let client_signing_key = SigningKey::from_bytes(&[77u8; 32]);
            let offer = DirectConnectionOffer::new_at(
                "session-test-cancellation".to_string(),
                client_addr,
                client_cert.cert_der.clone(),
                client_nonce,
                DirectRole::Initiator,
                1_800_000_000_000,
                &client_signing_key,
            );

            let (cancel_tx, cancel_rx) = broadcast::channel(16);

            let host_job = HostJob {
                socket: host_sock,
                cert: host_cert,
                local_nonce: host_nonce,
                offer: offer.clone(),
                gateway: echo_addr,
                deadline: Duration::from_secs(5),
                cancellation: Some(cancel_rx),
            };

            let mut host_job_task = TaskGuard::new(tokio::spawn(host_job.run()));

            execute_nonce_punch(
                &client_sock,
                host_addr,
                client_nonce,
                host_nonce,
                Duration::from_secs(4),
            )
            .await
            .expect("client punch ok");

            let client_std_sock = client_sock.into_std().expect("client into_std");
            let client_endpoint = create_quic_endpoint(
                client_std_sock,
                &client_cert,
                &host_cert_der,
                false,
            )
            .expect("client quic endpoint");

            let client_conn = client_endpoint
                .connect(SocketAddr::V4(host_addr), "ferryx-direct")
                .expect("connect call")
                .await
                .expect("quic handshake ok");

            let (mut quic_send, mut quic_recv) = client_conn
                .open_bi()
                .await
                .expect("open bi stream");

            let test_payload = b"FERRYX_REAL_BRIDGE_ROUNDTRIP_BEFORE_CANCEL";
            quic_send
                .write_all(test_payload)
                .await
                .expect("write payload");

            let mut echo_buf = vec![0u8; test_payload.len()];
            quic_recv
                .read_exact(&mut echo_buf)
                .await
                .expect("read echo payload");
            assert_eq!(&echo_buf, test_payload);

            cancel_tx.send(()).expect("send cancel signal");

            tokio::time::timeout(Duration::from_secs(3), host_job_task.join())
                .await
                .expect("host job joined within timeout")
                .expect("host job ok");

            let mut closed_peer = None;
            let receive_close = tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    match echo_events_rx.recv().await {
                        Ok(EchoServerEvent::Closed { peer }) => {
                            closed_peer = Some(peer);
                            break;
                        }
                        Ok(_) => continue,
                        Err(e) => panic!("event rx error: {e}"),
                    }
                }
            })
            .await;
            assert!(receive_close.is_ok(), "TCP bridge forwarder closed on server");
            assert!(closed_peer.is_some());

            let stream_closed = tokio::time::timeout(Duration::from_secs(2), async {
                let mut buf = [0u8; 16];
                loop {
                    match quic_recv.read(&mut buf).await {
                        Ok(Some(0)) | Ok(None) | Err(_) => return true,
                        Ok(Some(_)) => continue,
                    }
                }
            })
            .await;
            assert!(stream_closed.is_ok());

            client_endpoint.close(0u32.into(), b"client done");
            tokio::time::timeout(Duration::from_secs(2), client_endpoint.wait_idle())
                .await
                .expect("client endpoint idle");

            // Recovery test
            let host_sock2 = UdpSocket::bind("127.0.0.1:0").await.expect("bind host_sock2");
            let client_sock2 = UdpSocket::bind("127.0.0.1:0").await.expect("bind client_sock2");

            let host_addr2 = match host_sock2.local_addr().expect("host addr 2") {
                SocketAddr::V4(v4) => v4,
                _ => panic!("expected v4 host 2"),
            };
            let client_addr2 = match client_sock2.local_addr().expect("client addr 2") {
                SocketAddr::V4(v4) => v4,
                _ => panic!("expected v4 client 2"),
            };

            let host_cert2 = generate_ephemeral_cert().expect("host cert 2");
            let host_cert_der2 = host_cert2.cert_der.clone();
            let client_cert2 = generate_ephemeral_cert().expect("client cert 2");

            let host_nonce2 = [33u8; 32];
            let client_nonce2 = [44u8; 32];

            let offer2 = DirectConnectionOffer::new_at(
                "session-test-recovery".to_string(),
                client_addr2,
                client_cert2.cert_der.clone(),
                client_nonce2,
                DirectRole::Initiator,
                1_800_000_000_000,
                &client_signing_key,
            );

            let host_job2 = HostJob {
                socket: host_sock2,
                cert: host_cert2,
                local_nonce: host_nonce2,
                offer: offer2,
                gateway: echo_addr,
                deadline: Duration::from_secs(5),
                cancellation: None,
            };

            let mut host_job_task2 = TaskGuard::new(tokio::spawn(host_job2.run()));

            execute_nonce_punch(
                &client_sock2,
                host_addr2,
                client_nonce2,
                host_nonce2,
                Duration::from_secs(4),
            )
            .await
            .expect("client punch 2 ok");

            let client_std_sock2 = client_sock2.into_std().expect("client std sock 2");
            let client_endpoint2 = create_quic_endpoint(
                client_std_sock2,
                &client_cert2,
                &host_cert_der2,
                false,
            )
            .expect("client endpoint 2");

            let client_conn2 = client_endpoint2
                .connect(SocketAddr::V4(host_addr2), "ferryx-direct")
                .expect("connect 2")
                .await
                .expect("quic connect 2 ok");

            let (mut quic_send2, mut quic_recv2) = client_conn2
                .open_bi()
                .await
                .expect("open bi 2");

            let recovery_payload = b"FERRYX_RECOVERY_ROUNDTRIP";
            quic_send2
                .write_all(recovery_payload)
                .await
                .expect("write payload 2");

            let mut recovery_echo = vec![0u8; recovery_payload.len()];
            quic_recv2
                .read_exact(&mut recovery_echo)
                .await
                .expect("read echo 2");
            assert_eq!(&recovery_echo, recovery_payload);

            quic_send2.finish().expect("finish send 2");
            client_conn2.close(0u32.into(), b"recovery done");
            client_endpoint2.close(0u32.into(), b"recovery done");
            tokio::time::timeout(Duration::from_secs(2), client_endpoint2.wait_idle())
                .await
                .expect("client endpoint 2 idle");

            tokio::time::timeout(Duration::from_secs(3), host_job_task2.join())
                .await
                .expect("host job 2 joined")
                .expect("host job 2 ok");

            echo_shutdown.send(()).expect("echo shutdown");
            tokio::time::timeout(Duration::from_secs(2), echo_server.join())
                .await
                .expect("echo server joined")
                .expect("echo server ok")
                .expect("echo server io ok");
        })
        .await
        .expect("test completed within global bound");
    }

    #[tokio::test]
    async fn test_enrolled_host_direct_route_active_suspended_recovery_lifecycle() {
        use base64::Engine;
        use crate::account::billing::entitlement::GRACE_SECS;
        use crate::account::billing::lemonsqueezy::LemonSqueezyConfig;
        use crate::account::billing::routes::billing_routes;
        use crate::account::enroll_client::AccountEnrollmentRecord;
        use crate::account::mailer::FileMailer;
        use crate::account::origin::DeploymentMode;
        use crate::account::service::AccountState;
        use crate::account::store::{AccountStore, MachineRecord, UserRecord};
        use crate::remote::auth::MachineIdentity;
        use crate::remote::direct_lease::{
            renew_host_lease, spawn_lease_cancellation_watch, HostLeaseContext, HostLeaseState,
            LeaseAttempt,
        };
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::Arc;

        tokio::time::timeout(Duration::from_secs(30), async {
            // 1. Setup real Account service with SQLite store and billing routes
            let tmp = tempfile::tempdir().expect("tempdir");
            let mail_dir = tmp.path().join("mail");
            let mailer = Arc::new(FileMailer::with_dir(mail_dir));
            let account_state = Arc::new(
                AccountState::new(tmp.path(), "http://127.0.0.1", mailer)
                    .with_deployment_mode(DeploymentMode::Commercial),
            );

            let ls_config = LemonSqueezyConfig {
                api_key: "test_key".to_string(),
                store_id: "12345".to_string(),
                webhook_secret: "test_secret".to_string(),
                variant_pro_monthly: "var_pro_m".to_string(),
                variant_pro_annual: "var_pro_a".to_string(),
                variant_team_monthly: "var_team_m".to_string(),
                variant_team_annual: "var_team_a".to_string(),
                variant_team_hostpack_annual: "var_pack_a".to_string(),
                api_base_url: None,
                expect_test_mode: true,
            };
            let router = billing_routes(ls_config).with_state(Arc::clone(&account_state));
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind account");
            let account_addr = listener.local_addr().expect("local addr");
            let account_origin = format!("http://{account_addr}");
            let (server_shutdown_tx, server_shutdown_rx) = oneshot::channel::<()>();
            let mut server_task = TaskGuard::new(tokio::spawn(async move {
                let _ = axum::serve(listener, router)
                    .with_graceful_shutdown(async move {
                        let _ = server_shutdown_rx.await;
                    })
                    .await;
            }));

            // 2. Provision user and two machines in SQLite store.
            // On default Free plan (limit = 1 machine), 2 registered machines means
            // machines_used (2) > machine_limit (1). This constitutes an OverLimit violation.
            let machine_signing_key = SigningKey::from_bytes(&[11u8; 32]);
            let machine_public_key = base64::engine::general_purpose::STANDARD
                .encode(machine_signing_key.verifying_key().as_bytes());
            let machine_private_key = base64::engine::general_purpose::STANDARD
                .encode(machine_signing_key.as_bytes());
            let machine_id = "m-test-direct-enrolled-01".to_string();
            let machine_id_2 = "m-test-direct-enrolled-02".to_string();
            let owner_user_id = "user_direct_test_01".to_string();

            let account_signing_key = account_state
                .signing_key()
                .expect("account signing key available");
            let account_public_key_b64 = base64::engine::general_purpose::STANDARD
                .encode(account_signing_key.verifying_key().as_bytes());

            // Realistic timestamp near current epoch to satisfy lease clock skew tolerance
            let base_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_secs();
            let current_time = Arc::new(AtomicU64::new(base_time));
            let now_fn: Arc<dyn Fn() -> u64 + Send + Sync> = {
                let current_time = Arc::clone(&current_time);
                Arc::new(move || current_time.load(Ordering::Acquire))
            };

            {
                let db_path = tmp.path().to_path_buf();
                let user_id = owner_user_id.clone();
                let m1 = machine_id.clone();
                let m2 = machine_id_2.clone();
                let m_pub = machine_public_key.clone();
                tokio::task::spawn_blocking(move || {
                    let mut store = AccountStore::load(&db_path).expect("load store");
                    store.users.insert(
                        user_id.clone(),
                        UserRecord {
                            user_id: user_id.clone(),
                            email: "user@test.org".to_string(),
                            created_at: base_time,
                        },
                    );
                    store.machines.insert(
                        format!("rec-{m1}"),
                        MachineRecord {
                            machine_record_id: format!("rec-{m1}"),
                            owner_user_id: user_id.clone(),
                            machine_id: m1,
                            display_name: "Test Machine 1".to_string(),
                            public_key: m_pub.clone(),
                            attach_public_key: String::new(),
                            relay_origin: "https://relay.test".to_string(),
                            platform: "test".to_string(),
                            enrollment_epoch: 1,
                            enrolled_at: base_time,
                            last_seen_at: base_time,
                        },
                    );
                    store.machines.insert(
                        format!("rec-{m2}"),
                        MachineRecord {
                            machine_record_id: format!("rec-{m2}"),
                            owner_user_id: user_id,
                            machine_id: m2,
                            display_name: "Test Machine 2".to_string(),
                            public_key: m_pub,
                            attach_public_key: String::new(),
                            relay_origin: "https://relay.test".to_string(),
                            platform: "test".to_string(),
                            enrollment_epoch: 1,
                            enrolled_at: base_time,
                            last_seen_at: base_time,
                        },
                    );
                    store.save(&db_path).expect("save store");
                })
                .await
                .expect("spawn_blocking store init");
            }

            // 3. Host identity & HostLeaseContext setup
            let machine_identity = MachineIdentity {
                machine_id: machine_id.clone(),
                display_name: "Test Machine 1".to_string(),
                public_key: machine_public_key.clone(),
                private_key: machine_private_key,
            };
            let enrollment = AccountEnrollmentRecord {
                account_id: owner_user_id.clone(),
                machine_record_id: format!("rec-{machine_id}"),
                account_origin: account_origin.clone(),
                relay_origin: "https://relay.test".to_string(),
                enrollment_epoch: "1".to_string(),
                enrolled_at: base_time,
            };

            let host_lease_state = Arc::new(HostLeaseState::new());
            let lease_context = HostLeaseContext {
                enrollment: Some(enrollment),
                machine: Some(machine_identity),
                account_public_key: Some(account_public_key_b64.clone()),
                http: reqwest::Client::new(),
                now_fn: Arc::clone(&now_fn),
            };

            // Initial renewal succeeds because violation just started (within grace window)
            let first_renewal = renew_host_lease(&host_lease_state, &lease_context).await;
            assert_eq!(first_renewal, LeaseAttempt::Renewed);
            let valid_lease = host_lease_state
                .get_valid_lease(now_fn())
                .expect("valid lease exists");
            // Direct integrated verification: initial OverLimit violation ensures lease TTL is capped
            // by the actual grace deadline (base_time + GRACE_SECS).
            let expected_grace_deadline = base_time + GRACE_SECS;
            assert!(
                valid_lease.expires_at <= expected_grace_deadline,
                "first HTTP lease claims.expires_at ({}) must be <= actual grace deadline ({})",
                valid_lease.expires_at,
                expected_grace_deadline
            );

            // 4. Wire real cancellation watch to a cancellation broadcast
            let (cancel_tx, mut cancel_rx) = broadcast::channel::<()>(4);
            let cancel_sender = cancel_tx.clone();
            let mut watch_task = TaskGuard::new(spawn_lease_cancellation_watch(
                Arc::clone(&host_lease_state),
                Arc::clone(&now_fn),
                Arc::new(move || {
                    let _ = cancel_sender.send(());
                }),
            ));

            // 5. Spawn guarded TCP echo server (simulating terminal gateway endpoint)
            let (echo_addr, echo_shutdown, mut echo_server, mut echo_events_rx) =
                spawn_guarded_echo_server().await;

            // 6. Establish a separate concurrently running NON-ACCOUNT bridge.
            // Non-account bridges (LAN / static token / direct) have cancellation: None,
            // proving that account billing lease revocation only cuts off account routes
            // while non-account traffic survives unimpeded.
            let non_account_host_sock = UdpSocket::bind("127.0.0.1:0").await.expect("bind non-acct host_sock");
            let non_account_client_sock = UdpSocket::bind("127.0.0.1:0").await.expect("bind non-acct client_sock");
            let non_account_host_addr = match non_account_host_sock.local_addr().expect("non-acct host addr") {
                SocketAddr::V4(v4) => v4,
                _ => panic!("expected v4 non-acct host"),
            };
            let non_account_client_addr = match non_account_client_sock.local_addr().expect("non-acct client addr") {
                SocketAddr::V4(v4) => v4,
                _ => panic!("expected v4 non-acct client"),
            };
            let non_account_host_cert = generate_ephemeral_cert().expect("non-acct host cert");
            let non_account_host_cert_der = non_account_host_cert.cert_der.clone();
            let non_account_client_cert = generate_ephemeral_cert().expect("non-acct client cert");
            let non_account_client_signing_key = SigningKey::from_bytes(&[88u8; 32]);
            let non_account_client_nonce = [61u8; 32];
            let non_account_host_nonce = [62u8; 32];
            let non_account_offer = DirectConnectionOffer::new_at(
                "test-direct-non-account-session".to_string(),
                non_account_client_addr,
                non_account_client_cert.cert_der.clone(),
                non_account_client_nonce,
                DirectRole::Initiator,
                now_fn() * 1000,
                &non_account_client_signing_key,
            );
            let non_account_host_job = HostJob {
                socket: non_account_host_sock,
                cert: non_account_host_cert,
                local_nonce: non_account_host_nonce,
                offer: non_account_offer,
                gateway: echo_addr,
                deadline: Duration::from_secs(5),
                cancellation: None, // No account cancellation watch wired
            };
            let mut non_account_host_job_task = TaskGuard::new(tokio::spawn(non_account_host_job.run()));
            tokio::time::timeout(
                Duration::from_secs(4),
                execute_nonce_punch(
                    &non_account_client_sock,
                    non_account_host_addr,
                    non_account_client_nonce,
                    non_account_host_nonce,
                    Duration::from_secs(4),
                ),
            )
            .await
            .expect("non-acct punch timeout bounded")
            .expect("non-acct client punch ok");
            let non_account_client_std = non_account_client_sock.into_std().expect("non-acct into_std");
            let non_account_client_endpoint = create_quic_endpoint(
                non_account_client_std,
                &non_account_client_cert,
                &non_account_host_cert_der,
                false,
            )
            .expect("non-acct client endpoint");
            let non_account_client_conn = tokio::time::timeout(
                Duration::from_secs(4),
                non_account_client_endpoint
                    .connect(SocketAddr::V4(non_account_host_addr), "ferryx-direct")
                    .expect("non-acct connect call"),
            )
            .await
            .expect("non-acct connect timeout bounded")
            .expect("non-acct quic connect ok");
            let (mut non_account_quic_send, mut non_account_quic_recv) = tokio::time::timeout(
                Duration::from_secs(3),
                non_account_client_conn.open_bi(),
            )
            .await
            .expect("non-acct open_bi timeout bounded")
            .expect("non-acct open bi");
            let non_account_payload_1 = b"NON_ACCOUNT_PRE_REVOCATION_TRAFFIC\n";
            tokio::time::timeout(
                Duration::from_secs(3),
                non_account_quic_send.write_all(non_account_payload_1),
            )
            .await
            .expect("non-acct write timeout bounded")
            .expect("non-acct write 1");
            let mut non_account_echo_buf = vec![0u8; non_account_payload_1.len()];
            tokio::time::timeout(
                Duration::from_secs(3),
                non_account_quic_recv.read_exact(&mut non_account_echo_buf),
            )
            .await
            .expect("non-acct read timeout bounded")
            .expect("non-acct read 1");
            // Capture the exact accepted peer for the non-account bridge
            let non_account_tcp_peer = tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    match echo_events_rx.recv().await {
                        Ok(EchoServerEvent::Accepted { peer }) => return peer,
                        Ok(_) => continue,
                        Err(e) => panic!("event rx error: {e}"),
                    }
                }
            })
            .await
            .expect("receive non-acct accepted peer");

            // 7. Establish real UDP punch & QUIC cert pinned bridge for the ENROLLED host (Account bridge)
            let host_sock = UdpSocket::bind("127.0.0.1:0").await.expect("bind host_sock");
            let client_sock = UdpSocket::bind("127.0.0.1:0").await.expect("bind client_sock");

            let host_addr = match host_sock.local_addr().expect("host addr") {
                SocketAddr::V4(v4) => v4,
                _ => panic!("expected v4 host"),
            };
            let client_addr = match client_sock.local_addr().expect("client addr") {
                SocketAddr::V4(v4) => v4,
                _ => panic!("expected v4 client"),
            };

            let host_cert = generate_ephemeral_cert().expect("host cert");
            let client_cert = generate_ephemeral_cert().expect("client cert");
            let host_cert_der = host_cert.cert_der.clone();

            let client_nonce = [42u8; 32];
            let host_nonce = [43u8; 32];

            let client_signing_key = SigningKey::from_bytes(&[77u8; 32]);
            let offer = DirectConnectionOffer::new_at(
                "test-direct-lifecycle-session-1".to_string(),
                client_addr,
                client_cert.cert_der.clone(),
                client_nonce,
                DirectRole::Initiator,
                now_fn() * 1000,
                &client_signing_key,
            );

            let host_job = HostJob {
                socket: host_sock,
                cert: host_cert,
                local_nonce: host_nonce,
                offer: offer.clone(),
                gateway: echo_addr,
                deadline: Duration::from_secs(5),
                cancellation: Some(cancel_tx.subscribe()),
            };

            let mut host_job_task = TaskGuard::new(tokio::spawn(host_job.run()));

            tokio::time::timeout(
                Duration::from_secs(4),
                execute_nonce_punch(
                    &client_sock,
                    host_addr,
                    client_nonce,
                    host_nonce,
                    Duration::from_secs(4),
                ),
            )
            .await
            .expect("client punch timeout bounded")
            .expect("client punch ok");

            let client_std_sock = client_sock.into_std().expect("client into_std");
            let client_endpoint = create_quic_endpoint(
                client_std_sock,
                &client_cert,
                &host_cert_der,
                false,
            )
            .expect("client endpoint");

            let client_conn = tokio::time::timeout(
                Duration::from_secs(4),
                client_endpoint
                    .connect(SocketAddr::V4(host_addr), "ferryx-direct")
                    .expect("connect call"),
            )
            .await
            .expect("connect timeout bounded")
            .expect("quic connect ok");

            let (mut quic_send, mut quic_recv) = tokio::time::timeout(
                Duration::from_secs(3),
                client_conn.open_bi(),
            )
            .await
            .expect("open_bi timeout bounded")
            .expect("open bi");

            // Verify payload transit through active bridge
            // Note: This exercises the real QUIC-to-TCP forwarder and host job stream loop,
            // proving network data transit and lease-driven cutoff/recovery. Natural 7-day
            // lease expiry and full PTY session attachment remain separately verified in
            // daemon persistence and PTY contracts.
            let payload = b"STAGE1_ACTIVE_PAYLOAD\n";
            tokio::time::timeout(
                Duration::from_secs(3),
                quic_send.write_all(payload),
            )
            .await
            .expect("write payload timeout bounded")
            .expect("write payload");
            let mut echo_buf = vec![0u8; payload.len()];
            tokio::time::timeout(
                Duration::from_secs(3),
                quic_recv.read_exact(&mut echo_buf),
            )
            .await
            .expect("read echo timeout bounded")
            .expect("read echo");
            assert_eq!(&echo_buf, payload);

            // Capture the exact accepted peer for the account bridge
            let account_tcp_peer = tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    match echo_events_rx.recv().await {
                        Ok(EchoServerEvent::Accepted { peer }) => return peer,
                        Ok(_) => continue,
                        Err(e) => panic!("event rx error: {e}"),
                    }
                }
            })
            .await
            .expect("receive acct accepted peer");
            assert_ne!(account_tcp_peer, non_account_tcp_peer, "distinct TCP peers for distinct bridges");

            // 7. Fast-forward grace expiration in SQLite store via spawn_blocking (per AGENTS)
            {
                let db_path = tmp.path().to_path_buf();
                let user_id = owner_user_id.clone();
                let now = now_fn();
                let grace_started = now.saturating_sub(GRACE_SECS + 3600);
                tokio::task::spawn_blocking(move || {
                    let mut store = AccountStore::load(&db_path).expect("load store");
                    store.billing_states.insert(
                        user_id.clone(),
                        crate::account::store::BillingStateRecord {
                            owner_key: user_id,
                            grace_started_at: Some(grace_started),
                            stopped_at: Some(now.saturating_sub(3600)),
                            last_notice: Some("grace_expired".to_string()),
                        },
                    );
                    store.save(&db_path).expect("save store");
                })
                .await
                .expect("spawn_blocking fast-forward grace");
            }

            // 8. Execute real production renew_host_lease -> receives HTTP 402 REMOTE_SUSPENDED -> LeaseAttempt::Revoked
            let revoked_renewal = renew_host_lease(&host_lease_state, &lease_context).await;
            assert_eq!(revoked_renewal, LeaseAttempt::Revoked);

            // Verify host lease state is cleared
            assert!(host_lease_state.get_valid_lease(now_fn()).is_none());

            // 9. Verify cancellation watch fired and closed bridge
            tokio::time::timeout(Duration::from_secs(3), cancel_rx.recv())
                .await
                .expect("cancellation receiver signaled")
                .expect("broadcast ok");

            // Verify host job joins and TCP bridge forwarder closes specifically for the account bridge
            tokio::time::timeout(Duration::from_secs(3), host_job_task.join())
                .await
                .expect("host job joined within timeout")
                .expect("host job ok");

            let closed_peer = tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    match echo_events_rx.recv().await {
                        Ok(EchoServerEvent::Closed { peer }) => return peer,
                        Ok(_) => continue,
                        Err(e) => panic!("event rx error: {e}"),
                    }
                }
            })
            .await
            .expect("TCP bridge forwarder closed on server");
            // The echo server reports the exact client peer address of the closed connection.
            // Assert that the closed connection is EXACTLY the account bridge connection,
            // while the non-account bridge remains untouched!
            assert_eq!(
                closed_peer,
                account_tcp_peer,
                "tcp close event delivered exact account gateway connection peer"
            );

            // Verify QUIC stream reads EOF
            let stream_closed = tokio::time::timeout(Duration::from_secs(2), async {
                let mut buf = [0u8; 16];
                loop {
                    match quic_recv.read(&mut buf).await {
                        Ok(Some(0)) | Ok(None) | Err(_) => return true,
                        Ok(Some(_)) => continue,
                    }
                }
            })
            .await
            .expect("quic recv read loop bounded");
            assert!(stream_closed, "QUIC stream was closed on cancellation");

            client_conn.close(0u32.into(), b"suspension verified");
            client_endpoint.close(0u32.into(), b"suspension verified");
            tokio::time::timeout(Duration::from_secs(2), client_endpoint.wait_idle())
                .await
                .expect("client endpoint idle");

            // 10. Verify NON-ACCOUNT traffic survives while account route is suspended!
            let non_account_payload_2 = b"NON_ACCOUNT_POST_REVOCATION_SURVIVAL_PAYLOAD\n";
            tokio::time::timeout(
                Duration::from_secs(3),
                non_account_quic_send.write_all(non_account_payload_2),
            )
            .await
            .expect("non-acct write 2 timeout bounded")
            .expect("non-acct write 2");
            let mut non_account_echo_buf_2 = vec![0u8; non_account_payload_2.len()];
            tokio::time::timeout(
                Duration::from_secs(3),
                non_account_quic_recv.read_exact(&mut non_account_echo_buf_2),
            )
            .await
            .expect("non-acct read 2 timeout bounded")
            .expect("non-acct read 2");
            assert_eq!(&non_account_echo_buf_2, non_account_payload_2);

            // Cleanly close non-account client and wait for its host job to finish
            non_account_quic_send.finish().expect("finish non-acct send");
            non_account_client_conn.close(0u32.into(), b"non-acct done");
            non_account_client_endpoint.close(0u32.into(), b"non-acct done");
            tokio::time::timeout(Duration::from_secs(2), non_account_client_endpoint.wait_idle())
                .await
                .expect("non-acct client endpoint idle");
            tokio::time::timeout(Duration::from_secs(3), non_account_host_job_task.join())
                .await
                .expect("non-account host job joined within timeout")
                .expect("non-account host job ok");

            let non_account_closed_peer = tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    match echo_events_rx.recv().await {
                        Ok(EchoServerEvent::Closed { peer }) => return peer,
                        Ok(_) => continue,
                        Err(e) => panic!("event rx error: {e}"),
                    }
                }
            })
            .await
            .expect("non-acct tcp close event");
            assert_eq!(
                non_account_closed_peer,
                non_account_tcp_peer,
                "non-acct closed peer matches"
            );

            // 11. Recovery: Remove second machine so machines_used (1) <= limit (1) and clear billing_states
            {
                let db_path = tmp.path().to_path_buf();
                let user_id = owner_user_id.clone();
                let m2 = machine_id_2.clone();
                tokio::task::spawn_blocking(move || {
                    let mut store = AccountStore::load(&db_path).expect("load store");
                    store.machines.remove(&format!("rec-{m2}"));
                    store.billing_states.remove(&user_id);
                    store.save(&db_path).expect("save store");
                })
                .await
                .expect("spawn_blocking recovery");
            }

            // 12. Execute renew_host_lease -> receives HTTP 200 -> LeaseAttempt::Renewed on same host
            let restored_renewal = renew_host_lease(&host_lease_state, &lease_context).await;
            assert_eq!(restored_renewal, LeaseAttempt::Renewed);
            assert!(host_lease_state.get_valid_lease(now_fn()).is_some());

            // 13. Fresh bridge payload transit on same host
            let host_sock2 = UdpSocket::bind("127.0.0.1:0").await.expect("bind host_sock 2");
            let client_sock2 = UdpSocket::bind("127.0.0.1:0").await.expect("bind client_sock 2");

            let host_addr2 = match host_sock2.local_addr().expect("host addr 2") {
                SocketAddr::V4(v4) => v4,
                _ => panic!("expected v4 host 2"),
            };
            let client_addr2 = match client_sock2.local_addr().expect("client addr 2") {
                SocketAddr::V4(v4) => v4,
                _ => panic!("expected v4 client 2"),
            };

            let host_cert2 = generate_ephemeral_cert().expect("host cert 2");
            let client_cert2 = generate_ephemeral_cert().expect("client cert 2");
            let host_cert_der2 = host_cert2.cert_der.clone();

            let client_nonce2 = [52u8; 32];
            let host_nonce2 = [53u8; 32];

            let offer2 = DirectConnectionOffer::new_at(
                "test-direct-lifecycle-session-2".to_string(),
                client_addr2,
                client_cert2.cert_der.clone(),
                client_nonce2,
                DirectRole::Initiator,
                now_fn() * 1000,
                &client_signing_key,
            );

            let host_job2 = HostJob {
                socket: host_sock2,
                cert: host_cert2,
                local_nonce: host_nonce2,
                offer: offer2.clone(),
                gateway: echo_addr,
                deadline: Duration::from_secs(5),
                cancellation: Some(cancel_tx.subscribe()),
            };

            let mut host_job_task2 = TaskGuard::new(tokio::spawn(host_job2.run()));

            tokio::time::timeout(
                Duration::from_secs(4),
                execute_nonce_punch(
                    &client_sock2,
                    host_addr2,
                    client_nonce2,
                    host_nonce2,
                    Duration::from_secs(4),
                ),
            )
            .await
            .expect("client punch 2 timeout bounded")
            .expect("client punch 2 ok");

            let client_std_sock2 = client_sock2.into_std().expect("client into_std 2");
            let client_endpoint2 = create_quic_endpoint(
                client_std_sock2,
                &client_cert2,
                &host_cert_der2,
                false,
            )
            .expect("client endpoint 2");

            let client_conn2 = tokio::time::timeout(
                Duration::from_secs(4),
                client_endpoint2
                    .connect(SocketAddr::V4(host_addr2), "ferryx-direct")
                    .expect("connect 2 call"),
            )
            .await
            .expect("connect 2 timeout bounded")
            .expect("quic connect 2 ok");

            let (mut quic_send2, mut quic_recv2) = tokio::time::timeout(
                Duration::from_secs(3),
                client_conn2.open_bi(),
            )
            .await
            .expect("open_bi 2 timeout bounded")
            .expect("open bi 2");

            let recovery_payload = b"STAGE4_RECOVERED_PAYLOAD\n";
            tokio::time::timeout(
                Duration::from_secs(3),
                quic_send2.write_all(recovery_payload),
            )
            .await
            .expect("write recovery timeout bounded")
            .expect("write recovery");
            let mut recovery_echo = vec![0u8; recovery_payload.len()];
            tokio::time::timeout(
                Duration::from_secs(3),
                quic_recv2.read_exact(&mut recovery_echo),
            )
            .await
            .expect("read echo 2 timeout bounded")
            .expect("read echo 2");
            assert_eq!(&recovery_echo, recovery_payload);

            quic_send2.finish().expect("finish send 2");
            client_conn2.close(0u32.into(), b"recovery done");
            client_endpoint2.close(0u32.into(), b"recovery done");
            tokio::time::timeout(Duration::from_secs(2), client_endpoint2.wait_idle())
                .await
                .expect("client endpoint 2 idle");

            tokio::time::timeout(Duration::from_secs(3), host_job_task2.join())
                .await
                .expect("host job 2 joined")
                .expect("host job 2 ok");

            // Clean shutdown of mock echo server and background account server
            echo_shutdown.send(()).expect("echo shutdown");
            tokio::time::timeout(Duration::from_secs(2), echo_server.join())
                .await
                .expect("echo server joined")
                .expect("echo server ok")
                .expect("echo server io ok");

            let _ = server_shutdown_tx.send(());
            let _ = tokio::time::timeout(Duration::from_secs(2), server_task.join()).await;
            drop(watch_task);
        })
        .await
        .expect("test completed within global bound");
    }
}
