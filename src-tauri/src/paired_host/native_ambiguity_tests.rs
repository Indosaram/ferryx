//! Exercise the real connect-only IPC boundary, without a daemon or remote mutation.
use super::client::{Operation, OperationRequest};
use crate::daemon::{client::DaemonClient, protocol::{DaemonRequest, DAEMON_PROTOCOL_VERSION}};
use crate::scoped_contracts::Epoch;
use serde_json::json;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const ID: &str = "3941b9de-b16d-4d9a-ae0a-118f90fd91f4";

async fn failed_exchange(operation: Operation, response: Option<&[u8]>, timeout: bool) -> super::client::ClientError {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("ipc.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let client = DaemonClient::new_with_socket(socket);
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let peer = async {
        let (stream, _) = listener.accept().await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(matches!(serde_json::from_str::<DaemonRequest>(&line).unwrap(), DaemonRequest::Handshake { .. }));
        writer.write_all(format!("{{\"type\":\"handshakeOk\",\"version\":{DAEMON_PROTOCOL_VERSION},\"pid\":1,\"epoch\":1}}\n").as_bytes()).await.unwrap();
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        assert!(matches!(serde_json::from_str::<DaemonRequest>(&line).unwrap(), DaemonRequest::GetCapabilities));
        writer.write_all(b"{\"type\":\"capabilitiesOk\",\"capabilities\":[\"pairedHostInventoryV1\"]}\n").await.unwrap();
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        assert!(matches!(serde_json::from_str::<DaemonRequest>(&line).unwrap(), DaemonRequest::PairedHostOperation { .. }));
        if timeout {
            // Exact receipt is the signal: the native request is now in flight.
            // Virtual time proves the outer mutation deadline without wall-clock waits.
            tokio::time::pause();
            tokio::time::advance(DaemonClient::PAIRED_MUTATION_OUTER_TIMEOUT).await;
            done_rx.await.unwrap();
            tokio::time::resume();
        } else if let Some(bytes) = response {
            writer.write_all(bytes).await.unwrap();
        }
        // Drop both halves after receipt, not after a sleep or a race with send.
    };
    let action = async {
        let result = client.paired_host_operation(OperationRequest {
            host_id: "https://relay.example/host/a".into(), generation: Epoch(1), operation,
        }).await;
        let _ = done_tx.send(());
        result
    };
    let (_, result) = tokio::time::timeout(Duration::from_secs(200), async { tokio::join!(peer, action) }).await.unwrap();
    drop(listener);
    drop(client);
    let path = root.path().to_owned();
    root.close().unwrap();
    assert!(!path.exists());
    result.unwrap_err()
}

#[tokio::test]
async fn mutation_native_transport_failure_retains_identity() {
    let wt = json!({"wsId":"workspace","slug":"branch"});
    let operations = [
        json!({"kind":"registerProject","request":{"requestId":ID,"repoPath":"/repo"}}),
        json!({"kind":"unregisterProject","workspaceId":"workspace","request":{"requestId":ID,"expectedRevision":"1"}}),
        json!({"kind":"createWorktree","request":{"requestId":ID,"workspaceId":"workspace","worktree":wt}}),
        json!({"kind":"deleteWorktree","request":{"requestId":ID,"workspaceId":"workspace","worktree":wt,"deleteBranch":false,"expectedRevision":"1"}}),
        json!({"kind":"createSession","request":{"requestId":ID,"workspaceId":"workspace","worktree":null,"cols":80,"rows":24,"inheritFromSessionId":null,"cwdRelative":null,"startup":{"kind":"shell"}}}),
        json!({"kind":"closeSession","sessionId":"session","request":{"requestId":ID,"daemonEpoch":"1"}}),
    ];
    for value in operations {
        let operation: Operation = serde_json::from_value(value).unwrap();
        for response in [None, Some(b"not json\n".as_slice()), Some(b"{\"type\":\"pong\"}\n".as_slice())] {
            let error = failed_exchange(operation.clone(), response, false).await;
            eprintln!("A14 mutation error={error:?} cleanup=true");
            assert_eq!(error.code, "PAIRED_HOST_UNAVAILABLE");
            assert!(error.ambiguous, "mutation receipt followed by transport failure: {error:?}");
            assert_eq!(error.request_id.as_deref(), Some(ID));
        }
    }
}

#[tokio::test]
async fn read_only_native_transport_failure_is_not_ambiguous() {
    for value in [
        json!({"kind":"capabilities"}), json!({"kind":"projects"}),
        json!({"kind":"directories","includeHidden":false}),
        json!({"kind":"worktrees","workspaceId":"workspace"}),
        json!({"kind":"worktreeStatus","workspaceId":"workspace","worktree":{"wsId":"workspace","slug":"branch"}}),
        json!({"kind":"sessions"}),
        json!({"kind":"session","sessionId":"session","daemonEpoch":"1"}),
        json!({"kind":"operation","requestId":ID}),
    ] {
        let error = failed_exchange(serde_json::from_value(value).unwrap(), None, false).await;
        assert_eq!(error.code, "PAIRED_HOST_UNAVAILABLE");
        assert!(!error.ambiguous);
        assert_eq!(error.request_id, None);
    }
    eprintln!("A14 all_read_only_non_ambiguous=true lookup_id_not_mutation=true cleanup=true");
}

#[tokio::test]
async fn native_35s_deadline_retains_mutation_reconciliation() {
    let operation = serde_json::from_value(json!({"kind":"registerProject","request":{"requestId":ID,"repoPath":"/repo"}})).unwrap();
    let error = failed_exchange(operation, None, true).await;
    eprintln!("A14 native_35s_deadline error={error:?} peer_still_in_flight=true cleanup=true");
    assert!(matches!(error.code.as_str(), "PAIRED_HOST_UNAVAILABLE" | "TIMEOUT"));
    assert!(error.ambiguous);
    assert_eq!(error.request_id.as_deref(), Some(ID));
}
