//! Bounded owner metadata streams, scoped to a machine event socket.
use crate::daemon::{
    protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION},
    session_service::DaemonSessionService,
};
use crate::remote::machine_protocol::{RemoteTerminalTarget, Session};
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct OwnerMetadata {
    session: Session,
    revision: crate::scoped_contracts::Epoch,
}

struct StreamCount(tokio::sync::watch::Sender<usize>);
impl StreamCount {
    fn new(sender: &tokio::sync::watch::Sender<usize>) -> Self {
        sender.send_modify(|count| *count += 1);
        Self(sender.clone())
    }
}
impl Drop for StreamCount {
    fn drop(&mut self) {
        self.0.send_modify(|count| *count -= 1);
    }
}

impl DaemonSessionService {
    pub(crate) async fn serve_metadata<S: AsyncRead + AsyncWrite + Unpin>(
        &self,
        stream: S,
        target: RemoteTerminalTarget,
    ) -> Result<(), String> {
        let _count = StreamCount::new(&self.workspace_service.machine_events.owner_streams);
        let mut events = self.workspace_service.machine_events.subscribe();
        let (read, mut write) = tokio::io::split(stream);
        let mut reader = BufReader::new(read);
        let mut line = String::new();
        loop {
            let record = self
                .workspace_service
                .journal
                .session(&target.session_id)?
                .ok_or("SESSION_NOT_FOUND")?;
            if record.session.target != target {
                return Err("STALE_EPOCH".into());
            }
            let frame = OwnerMetadata {
                session: record.session,
                revision: self.workspace_service.journal.session_revision()?,
            };
            let mut bytes = serde_json::to_vec(&frame).map_err(|_| "INVALID_METADATA")?;
            bytes.push(b'\n');
            tokio::time::timeout(Duration::from_secs(10), write.write_all(&bytes))
                .await
                .map_err(|_| "TIMEOUT")?
                .map_err(|_| "HOST_UNAVAILABLE")?;
            loop {
                tokio::select! {
                    _ = reader.read_line(&mut line) => return Ok(()),
                    event = events.recv() => match event {
                        Ok(event) if event["sessionId"] == target.session_id => break,
                        Ok(_) => {},
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => break,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
                    }
                }
            }
        }
    }

    pub(crate) async fn forward_metadata(
        self: &Arc<Self>,
        target: RemoteTerminalTarget,
        status: tokio::sync::watch::Sender<bool>,
    ) -> Result<(), String> {
        let _count = StreamCount::new(&self.workspace_service.machine_events.forwarders);
        let peer = self
            .session_router
            .find_legacy_peer_for_session(&target.session_id)
            .ok_or("SESSION_NOT_FOUND")?;
        let socket_path = peer.socket_path().to_owned();
        crate::ipc::run_blocking(move || {
            crate::daemon::server::validate_runtime_socket_path(&socket_path)
                .map_err(crate::ipc::IpcError::internal)
        })
        .await
        .map_err(|_| "HOST_UNAVAILABLE")?;
        #[cfg(unix)]
        let stream = tokio::net::UnixStream::connect(peer.socket_path())
            .await
            .map_err(|_| "HOST_UNAVAILABLE")?;
        #[cfg(not(unix))]
        let stream = {
            let port = tokio::fs::read_to_string(peer.socket_path())
                .await
                .map_err(|_| "HOST_UNAVAILABLE")?;
            let port = port.trim().parse::<u16>().map_err(|_| "HOST_UNAVAILABLE")?;
            tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .map_err(|_| "HOST_UNAVAILABLE")?
        };
        let (read, mut write) = tokio::io::split(stream);
        let mut reader = BufReader::new(read);
        let mut handshake = serde_json::to_vec(&DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
        })
        .map_err(|_| "INVALID_METADATA")?;
        handshake.push(b'\n');
        tokio::time::timeout(Duration::from_secs(10), write.write_all(&handshake))
            .await
            .map_err(|_| "TIMEOUT")?
            .map_err(|_| "HOST_UNAVAILABLE")?;
        let mut line = String::new();
        use tokio::io::AsyncReadExt;
        let handshake_bytes = tokio::time::timeout(
            Duration::from_secs(10),
            (&mut reader).take(65537).read_line(&mut line),
        )
        .await
        .map_err(|_| "TIMEOUT")?
        .map_err(|_| "HOST_UNAVAILABLE")?;
        if handshake_bytes > 65536 {
            return Err("INVALID_METADATA".into());
        }
        if !matches!(
            serde_json::from_str::<DaemonResponse>(&line),
            Ok(DaemonResponse::HandshakeOk { .. })
        ) {
            return Err("MACHINE_OWNER_UNSUPPORTED".into());
        }
        let mut request = serde_json::to_vec(&DaemonRequest::MachineMetadataSubscribe {
            target: target.clone(),
        })
        .map_err(|_| "INVALID_METADATA")?;
        request.push(b'\n');
        tokio::time::timeout(Duration::from_secs(10), write.write_all(&request))
            .await
            .map_err(|_| "TIMEOUT")?
            .map_err(|_| "HOST_UNAVAILABLE")?;
        loop {
            line.clear();
            let count = (&mut reader)
                .take(65537)
                .read_line(&mut line)
                .await
                .map_err(|_| "HOST_UNAVAILABLE")?;
            if count == 0 || count > 65536 {
                return Err("HOST_UNAVAILABLE".into());
            }
            let frame: OwnerMetadata =
                serde_json::from_str(&line).map_err(|_| "INVALID_METADATA")?;
            if frame.session.target != target {
                return Err("STALE_EPOCH".into());
            }
            status.send_if_modified(|connected| {
                if *connected {
                    false
                } else {
                    *connected = true;
                    true
                }
            });
            self.workspace_service.machine_events.publish_revision(
                frame.revision.0,
                "sessionMetadataChanged",
                Some(&frame.session.workspace_id),
                Some(&target.session_id),
                serde_json::json!(frame.session),
            );
        }
    }
}
