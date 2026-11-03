use std::{env, fs, process::Command, sync::mpsc, thread, time::Duration};

use ferryx_rd_host::{
    DisplayInfo, HostConfig, HostServer, PairingRecord as HostPairingRecord,
    PairingStore as HostPairingStore,
};
use ferryx_rd_net::{PskIdentity, TlsPskClient};
use ferryx_rd_proto::WireCodec;

/// Application-data directory the client store resolves to when `HOME`/`XDG_DATA_HOME`/`APPDATA`
/// all point at `data_dir` (macOS keeps it under `Library/Application Support`).
fn app_data_dir(data_dir: &std::path::Path) -> std::path::PathBuf {
    #[cfg(target_os = "macos")]
    let base = data_dir.join("Library").join("Application Support");
    #[cfg(not(target_os = "macos"))]
    let base = data_dir.to_path_buf();
    base.join("MahoRD")
}

fn run_isolated_subprocess(case: &str, test_name: &str, setup_fn: impl FnOnce(&std::path::Path)) {
    let temp_dir = tempfile::tempdir().unwrap();
    let data_dir = temp_dir.path().to_path_buf();
    setup_fn(&data_dir);

    let mut child = Command::new(env::current_exe().unwrap());
    child
        .args(["--exact", test_name, "--nocapture"])
        .env("MAHO_PAIRING_ISOLATION_CASE", case)
        .env("XDG_DATA_HOME", &data_dir)
        .env("HOME", &data_dir);
    #[cfg(windows)]
    child.env("APPDATA", &data_dir);

    let exit_status = child
        .status()
        .expect("isolated child process must execute and exit");
    assert!(
        exit_status.success(),
        "isolated subprocess failed for case: {case} with status: {exit_status}"
    );
}

#[test]
fn test_outbound_client_pairing_rejected_as_host_authorization() {
    if env::var("MAHO_PAIRING_ISOLATION_CASE").as_deref() == Ok("outbound-rejected-and-isolation") {
        // Step 1: Functional trust isolation ahead of filename strings.
        // Client saves an outbound credential via open_default().
        let client_store = ferryx_rd_app::PairingStore::open_default().unwrap();
        let client_key = vec![0x42; 32];
        let client_record = ferryx_rd_app::PairingRecord {
            id: "client-outbound-id-1".into(),
            name: "RemoteHostName".into(),
            key: client_key.clone(),
            added_at_unix_ms: 1700000000000,
            last_endpoint: None,
            endpoint_aliases: Vec::new(),
            relay_url: None,
            relay_host_id: None,
            trust: ferryx_rd_proto::PairingTrust::Owner,
        };
        client_store.save(client_record.clone()).unwrap();

        // Host default store MUST NOT load client's outbound credential (trust mixing assertion)
        let host_store = HostPairingStore::host_default().unwrap();
        let host_records = host_store.load_all().unwrap();
        assert!(
            host_records.is_empty(),
            "Host authorization store must be empty and must not contain client outbound keys (trust mixing detected: {host_records:?})"
        );
        assert!(
            host_store.load("client-outbound-id-1").unwrap().is_none(),
            "Host must not load client-outbound-id-1"
        );

        // Step 2: Real host TLS loopback rejects client outbound key
        let config = HostConfig {
            tcp_addr: "127.0.0.1:0".parse().unwrap(),
            udp_addr: "127.0.0.1:0".parse().unwrap(),
            bootstrap_pin: None,
            pairing_window: Duration::from_secs(0),
            pairing_store: host_store.clone(),
            host_name: "test-host".into(),
            display: DisplayInfo {
                desktop_x: 0,
                desktop_y: 0,
                logical_width: 640,
                logical_height: 360,
                pixel_width: 640,
                pixel_height: 360,
                scale_factor_milli: 1000,
            },
            frames_per_second: 60,
            bitrate: 12_000_000,
            capture_audio: false,
            output_name: None,
            consent_sender: None,
            allow_input: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            allow_clipboard: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
        };
        let server = HostServer::bind(config).unwrap();
        let tcp_addr = server.tcp_addr().unwrap();

        let (server_done_tx, server_done_rx) = mpsc::channel();
        let server_thread = thread::spawn(move || {
            let res = server.serve_n(1);
            let _ = server_done_tx.send(res);
        });

        let tls_client =
            TlsPskClient::new(PskIdentity::pairing(&client_record.id, &client_record.key).unwrap())
                .unwrap();
        let connect_res = tls_client.connect(tcp_addr);
        assert!(
            connect_res.is_err(),
            "Host TLS loopback must reject outbound client key with identity unknown"
        );
        if let Ok(rejected_stream) = connect_res {
            drop(rejected_stream);
        }

        let server_res = server_done_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("server must finish serving within timeout after rejected TLS");
        assert!(
            server_res.is_err(),
            "server must record error on rejected TLS handshake, got: {server_res:?}"
        );
        server_thread
            .join()
            .expect("server thread must join cleanly");

        // Step 3: Separately approved inbound key works
        let host_inbound_key = [0x99; 32];
        let host_record = HostPairingRecord {
            id: "host-inbound-id-1".into(),
            name: "AllowedPeer".into(),
            key: host_inbound_key,
            added_at_unix_ms: 1700000000000,
        };
        host_store.save(host_record.clone()).unwrap();

        let config2 = HostConfig {
            tcp_addr: "127.0.0.1:0".parse().unwrap(),
            udp_addr: "127.0.0.1:0".parse().unwrap(),
            bootstrap_pin: None,
            pairing_window: Duration::from_secs(0),
            pairing_store: host_store.clone(),
            host_name: "test-host".into(),
            display: DisplayInfo {
                desktop_x: 0,
                desktop_y: 0,
                logical_width: 640,
                logical_height: 360,
                pixel_width: 640,
                pixel_height: 360,
                scale_factor_milli: 1000,
            },
            frames_per_second: 60,
            bitrate: 12_000_000,
            capture_audio: false,
            output_name: None,
            consent_sender: None,
            allow_input: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            allow_clipboard: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
        };
        let server2 = HostServer::bind(config2).unwrap();
        let tcp_addr2 = server2.tcp_addr().unwrap();

        let (server2_done_tx, server2_done_rx) = mpsc::channel();
        let server2_thread = thread::spawn(move || {
            let res = server2.serve_n(1);
            let _ = server2_done_tx.send(res);
        });

        let tls_client2 =
            TlsPskClient::new(PskIdentity::pairing(&host_record.id, &host_record.key).unwrap())
                .unwrap();
        let connect_res2 = tls_client2.connect(tcp_addr2);
        assert!(
            connect_res2.is_ok(),
            "Separately approved inbound key must be accepted by host TLS"
        );

        // Explicitly close the connection with a clean disconnect control packet
        // and drop the TLS stream BEFORE awaiting server done and join,
        // so the server cleanly terminates the connection without hanging or erroring.
        let mut stream2 = connect_res2.unwrap();
        let disconnect_payload = ferryx_rd_proto::ControlMessage::Disconnect.encode().unwrap();
        let mut disconnect_packet =
            ferryx_rd_proto::PacketHeader::new(ferryx_rd_proto::PacketType::Control, 0, 0, 0)
                .encode()
                .unwrap();
        disconnect_packet.extend_from_slice(&disconnect_payload);
        let _ = stream2.write_frame(&disconnect_packet);
        drop(stream2);

        let server2_res = server2_done_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("server2 must finish serving within timeout after client disconnect");
        assert!(
            server2_res.is_ok(),
            "server2 must exit cleanly after client disconnect: {server2_res:?}"
        );
        server2_thread
            .join()
            .expect("server2 thread must join cleanly");

        // Step 4: Deletion isolation
        client_store.delete("client-outbound-id-1").unwrap();
        assert!(client_store.load("client-outbound-id-1").unwrap().is_none());

        // Host still has inbound record intact
        assert!(host_store.load("host-inbound-id-1").unwrap().is_some());
        let host_all = host_store.load_all().unwrap();
        assert_eq!(host_all.len(), 1);
        assert_eq!(host_all[0].id, "host-inbound-id-1");

        // Host revoking does not affect client store
        assert!(host_store.revoke("host-inbound-id-1").unwrap());
        assert!(host_store.load("host-inbound-id-1").unwrap().is_none());

        // Step 5: Assert distinct default filenames and separate parent files
        let client_default_path = ferryx_rd_app::PairingStore::default_path().unwrap();
        assert_eq!(
            client_default_path.file_name().unwrap(),
            "client-pairings.json",
            "Client default store filename must be client-pairings.json"
        );
        assert_eq!(
            host_store.path().file_name().unwrap(),
            "host-authorizations.json",
            "Host default store filename must be host-authorizations.json"
        );
        return;
    }

    run_isolated_subprocess(
        "outbound-rejected-and-isolation",
        "test_outbound_client_pairing_rejected_as_host_authorization",
        |_| {},
    );
}

#[test]
fn test_legacy_pairing_keys_not_auto_imported_by_host_and_migrated_by_client() {
    if env::var("MAHO_PAIRING_ISOLATION_CASE").as_deref()
        == Ok("legacy-migration-and-host-isolation")
    {
        // Step 1: Observable host non-auto-import isolation ahead of filename checks
        let host_store = HostPairingStore::host_default().unwrap();
        let host_records = host_store.load_all().unwrap();
        assert!(
            host_records.is_empty(),
            "Host must NEVER auto-import records from legacy pairing-keys.json (trust mixing: {host_records:?})"
        );

        // Step 2: Client migrates legacy pairing-keys.json
        let client_store = ferryx_rd_app::PairingStore::open_default().unwrap();
        let client_records = client_store.load_all().unwrap();
        assert_eq!(
            client_records.len(),
            1,
            "Client must migrate legacy pairing-keys.json"
        );
        assert_eq!(client_records[0].id, "legacy-client-1");
        assert_eq!(client_records[0].name, "LegacyDevice");

        let client_file = client_store.path();
        assert!(
            client_file.exists(),
            "client-pairings.json must exist after migration"
        );

        // Step 3: Verify legacy file is preserved intact and unmodified
        let parent = client_file.parent().unwrap();
        let legacy_file = parent.join("pairing-keys.json");
        assert!(
            legacy_file.exists(),
            "Legacy pairing-keys.json must be preserved"
        );
        let legacy_data: serde_json::Value =
            serde_json::from_slice(&fs::read(&legacy_file).unwrap()).unwrap();
        assert_eq!(legacy_data[0]["id"], "legacy-client-1");

        // Step 4: Deleting from client store does not delete or touch legacy file
        client_store.delete("legacy-client-1").unwrap();
        assert!(client_store.load_all().unwrap().is_empty());
        assert!(
            legacy_file.exists(),
            "Legacy pairing-keys.json must remain after client store deletion"
        );
        return;
    }

    run_isolated_subprocess(
        "legacy-migration-and-host-isolation",
        "test_legacy_pairing_keys_not_auto_imported_by_host_and_migrated_by_client",
        |data_dir| {
            let maho_dir = app_data_dir(data_dir);
            fs::create_dir_all(&maho_dir).unwrap();
            let legacy_file = maho_dir.join("pairing-keys.json");
            let legacy_content = serde_json::json!([
                {
                    "id": "legacy-client-1",
                    "name": "LegacyDevice",
                    "key": "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=",
                    "addedAt": 0.0
                }
            ]);
            let legacy_bytes = serde_json::to_vec_pretty(&legacy_content).unwrap();
            fs::write(&legacy_file, &legacy_bytes).unwrap();
        },
    );
}
