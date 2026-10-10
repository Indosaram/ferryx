use std::sync::Arc;

use fxsh::frame::{Frame, FLAG_ERROR, FLAG_RESPONSE, MAJOR, MINOR};
use fxsh::messages::*;
use fxsh::types::*;
use fxsh::{decode_frame, Decoded, ErrorDetail, FatalFrameError, Message, Reason, Uuid, CAP_V1_REQUIRED};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};

use crate::host::HostShared;
use crate::outbox::{run_writer, ConnHandle, Outbox};

pub struct HostInfo {
    pub supervisor: Supervisor,
    pub pid: u32,
    pub process_start_time: u64,
    pub version: String,
}

fn send_error(outbox: &Outbox, session: Uuid, rid: u64, flags: u16, d: ErrorDetail, text: &str) {
    outbox.push_frame(Frame { session_id: session, request_id: rid, flags: flags | FLAG_ERROR, message: Message::Error(d.to_frame(text)) }.encode());
}

pub async fn serve_connection<R, W>(host: Arc<HostShared>, info: Arc<HostInfo>, mut reader: R, writer: W)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let outbox = Arc::new(Outbox::default());
    let conn = ConnHandle { id: host.next_conn_id(), outbox: outbox.clone() };
    let writer_task = tokio::spawn(run_writer(outbox.clone(), writer));
    host.register_conn(conn.clone());

    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    let mut chunk = vec![0u8; 64 * 1024];
    let mut hello_done = false;
    'conn: loop {
        loop {
            match decode_frame(&buf) {
                Ok(Decoded::NeedMore) => break,
                Ok(Decoded::Rejected { consumed, header, error }) => {
                    buf.drain(..consumed);
                    let detail = match error {
                        PayloadError::UnknownCommand(command) => ErrorDetail::UnknownCommand { command },
                        PayloadError::Violation(reason) => ErrorDetail::ProtocolViolation(reason),
                    };
                    let fatal = matches!(error, PayloadError::Violation(_));
                    send_error(&outbox, header.session_id, header.request_id, FLAG_RESPONSE, detail, "rejected frame");
                    if fatal {
                        break 'conn;
                    }
                }
                Ok(Decoded::Frame { consumed, header, frame }) => {
                    buf.drain(..consumed);
                    if !hello_done {
                        match frame.message {
                            Message::Hello(h) => {
                                if h.major != MAJOR {
                                    send_error(&outbox, Uuid::default(), header.request_id, FLAG_RESPONSE, ErrorDetail::VersionUnsupported { host_major: MAJOR, host_minor: MINOR }, "unsupported major version");
                                    break 'conn;
                                }
                                if let Some(bit) = (0..5u8).find(|b| CAP_V1_REQUIRED & (1 << b) != 0 && h.capabilities & (1 << b) == 0) {
                                    send_error(&outbox, Uuid::default(), header.request_id, FLAG_RESPONSE, ErrorDetail::UnsupportedCapability { capability_bit: bit }, "client lacks a required capability");
                                    break 'conn;
                                }
                                outbox.push_frame(
                                    Frame {
                                        session_id: Uuid::default(),
                                        request_id: header.request_id,
                                        flags: FLAG_RESPONSE,
                                        message: Message::HelloAck(HelloAck { major: MAJOR, minor: MINOR, capabilities: CAP_V1_REQUIRED, host_instance_id: host.instance_id, supervisor: info.supervisor, pid: info.pid, process_start_time: info.process_start_time, host_version: info.version.clone() }),
                                    }
                                    .encode(),
                                );
                                hello_done = true;
                            }
                            _ => {
                                send_error(&outbox, header.session_id, header.request_id, FLAG_RESPONSE, ErrorDetail::ProtocolViolation(Reason::BadOrder), "Hello must be the first frame");
                                break 'conn;
                            }
                        }
                        continue;
                    }
                    if matches!(frame.message, Message::Hello(_)) {
                        send_error(&outbox, header.session_id, header.request_id, FLAG_RESPONSE, ErrorDetail::ProtocolViolation(Reason::BadOrder), "duplicate Hello");
                        break 'conn;
                    }
                    host.handle(&conn, frame);
                }
                Err(FatalFrameError::PayloadTooLarge(_)) | Err(FatalFrameError::BadMagic) | Err(FatalFrameError::UndefinedFlags(_)) | Err(FatalFrameError::Truncated) => break 'conn,
            }
        }
        if outbox.is_closed() {
            break;
        }
        match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    host.conn_closed(conn.id);
    outbox.close();
    let _ = writer_task.await;
}
