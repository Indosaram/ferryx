use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::{agent_input::InputStateTracker, agent_server::AgentServerBackend};

pub fn list_mcp_tools() -> Value {
    json!([
        {
            "name": "remote_mouse_click",
            "description": "Clicks at specific coordinates on the remote desktop screen.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "x": { "type": "number", "description": "X coordinate (pixels or normalized 0.0-1.0)" },
                    "y": { "type": "number", "description": "Y coordinate (pixels or normalized 0.0-1.0)" },
                    "button": { "type": "string", "enum": ["left", "right", "middle"], "default": "left" },
                    "count": { "type": "integer", "default": 1, "description": "Click count (1=single, 2=double, 3=triple)" },
                    "normalized": { "type": "boolean", "default": false, "description": "Whether coordinates are normalized in [0.0, 1.0]" }
                },
                "required": ["x", "y"]
            }
        },
        {
            "name": "remote_mouse_move",
            "description": "Moves the cursor to the target coordinates.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "x": { "type": "number", "description": "X coordinate" },
                    "y": { "type": "number", "description": "Y coordinate" },
                    "normalized": { "type": "boolean", "default": false }
                },
                "required": ["x", "y"]
            }
        },
        {
            "name": "remote_mouse_drag",
            "description": "Drags mouse pointer from start to end coordinates with held button.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "start_x": { "type": "number" },
                    "start_y": { "type": "number" },
                    "end_x": { "type": "number" },
                    "end_y": { "type": "number" },
                    "button": { "type": "string", "enum": ["left", "right", "middle"], "default": "left" },
                    "steps": { "type": "integer", "default": 10 },
                    "normalized": { "type": "boolean", "default": false }
                },
                "required": ["start_x", "start_y", "end_x", "end_y"]
            }
        },
        {
            "name": "remote_mouse_scroll",
            "description": "Scrolls mouse wheel horizontally (dx) and vertically (dy).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dx": { "type": "number", "default": 0.0 },
                    "dy": { "type": "number", "default": 0.0 },
                    "x": { "type": "number" },
                    "y": { "type": "number" },
                    "normalized": { "type": "boolean", "default": false }
                },
                "required": ["dx", "dy"]
            }
        },
        {
            "name": "remote_key_press",
            "description": "Presses and releases a single key (e.g. 'Enter', 'Escape', 'F5', 'Tab', 'Space').",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Key name (e.g. 'Enter', 'Space', 'Backspace', 'F5')" },
                    "hold_ms": { "type": "integer", "default": 50 }
                },
                "required": ["key"]
            }
        },
        {
            "name": "remote_hotkey",
            "description": "Executes a combination of modifier keys and target key (e.g. ['Control', 'Alt', 't'], ['Command', 'Space']).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "keys": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of keys (e.g. ['Control', 'Shift', 't'])"
                    }
                },
                "required": ["keys"]
            }
        },
        {
            "name": "remote_type_text",
            "description": "Types a text string onto the remote host.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Text to type" },
                    "delay_ms": { "type": "integer", "default": 20 }
                },
                "required": ["text"]
            }
        },
        {
            "name": "remote_release_all",
            "description": "Emergency release for all active keys and mouse buttons.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "remote_get_screen_info",
            "description": "Gets current remote screen info including physical dimensions (width, height in pixels), DPI scale factor, logical resolution (logical_width, logical_height), multi-monitor enumeration (monitors array with per-monitor id, name, coordinates, scale, primary flag), and connected host name.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "remote_take_screenshot",
            "description": "Captures the active remote desktop screen as a base64 PNG image.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "format": { "type": "string", "enum": ["png", "jpeg"], "default": "png" }
                }
            }
        },
        {
            "name": "remote_wait_for_screen_change",
            "description": "Waits for a new video frame to arrive on the remote screen, enabling reactive perception loops without polling delays.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "last_frame_id": { "type": "integer", "description": "Frame ID to compare against (optional)" },
                    "timeout_ms": { "type": "integer", "description": "Maximum wait time in milliseconds (optional, default 5000, max 30000)" }
                }
            }
        }
    ])
}

#[path = "mcp_dispatch.rs"]
mod dispatch;
pub use dispatch::handle_mcp_message;

#[path = "mcp_stdio.rs"]
mod stdio;
pub use stdio::run as run_mcp_stdio_until;

pub async fn run_mcp_stdio(backend: Arc<dyn AgentServerBackend>) -> std::io::Result<()> {
    let (_stop, receiver) = tokio::sync::watch::channel(false);
    run_mcp_stdio_until(backend, receiver).await
}

pub async fn run_mcp_io(
    backend: Arc<dyn AgentServerBackend>,
    stdin: impl tokio::io::AsyncRead + Unpin,
    stdout: impl tokio::io::AsyncWrite + Unpin,
) -> std::io::Result<()> {
    let (_stop, receiver) = tokio::sync::watch::channel(false);
    run_mcp_io_until(backend, (stdin, stdout), receiver).await
}

/// Upper bound on a single newline-delimited MCP record, so a peer cannot grow the read
/// buffer without limit in a long-lived process.
const MAX_MCP_RECORD_BYTES: usize = 1024 * 1024;

/// Reads one newline-delimited record into `buf`, capped at [`MAX_MCP_RECORD_BYTES`].
///
/// An oversized record is discarded through its terminating newline, the retained buffer is
/// shrunk back down, and an explicit error is returned.
async fn read_bounded_record<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
) -> std::io::Result<usize> {
    buf.clear();
    let read = (&mut *reader)
        .take(MAX_MCP_RECORD_BYTES as u64)
        .read_until(b'\n', buf)
        .await?;
    if read == MAX_MCP_RECORD_BYTES && buf.last() != Some(&b'\n') {
        let mut discard = Vec::new();
        loop {
            discard.clear();
            let skipped = (&mut *reader)
                .take(MAX_MCP_RECORD_BYTES as u64)
                .read_until(b'\n', &mut discard)
                .await?;
            if skipped == 0 || discard.last() == Some(&b'\n') {
                break;
            }
        }
        *buf = Vec::new();
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("MCP record exceeds the {MAX_MCP_RECORD_BYTES}-byte limit"),
        ));
    }
    Ok(read)
}

pub async fn run_mcp_io_until(
    backend: Arc<dyn AgentServerBackend>,
    (stdin, mut stdout): (
        impl tokio::io::AsyncRead + Unpin,
        impl tokio::io::AsyncWrite + Unpin,
    ),
    mut stop: tokio::sync::watch::Receiver<bool>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stdin);
    let mut line: Vec<u8> = Vec::new();

    let mut tracker = InputStateTracker::default();
    let mut current_pos = (0.5, 0.5);

    let work = async {
        loop {
            let bytes_read = read_bounded_record(&mut reader, &mut line).await?;
            if bytes_read == 0 {
                break;
            }

            let text = std::str::from_utf8(&line)
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Some(resp) = dispatch::handle_mcp_message_cancellable(
                trimmed,
                backend.clone(),
                &mut tracker,
                &mut current_pos,
            )
            .await
            {
                stdout.write_all(resp.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
        }

        Ok::<(), std::io::Error>(())
    };
    let result = tokio::select! {
        biased;
        _ = stop.wait_for(|stopped| *stopped) => Ok(()),
        result = work => result,
    };
    let cleanup = dispatch::release(backend.as_ref(), &mut tracker, current_pos)
        .map_err(std::io::Error::other);
    match (result, cleanup) {
        (Ok(()), result) => result.map(|_| ()),
        (result, Ok(_)) => result,
        (Err(error), Err(cleanup)) => Err(std::io::Error::other(format!(
            "{error}; cleanup: {cleanup}"
        ))),
    }
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;

#[cfg(test)]
mod bounded_io_tests {
    use super::*;
    use crate::agent_input::{FrameMetadata, ScreenInfo};

    struct CountingBackend {
        sent: std::sync::atomic::AtomicUsize,
        polled: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        frame: Option<FrameMetadata>,
    }

    impl CountingBackend {
        fn new() -> Self {
            Self {
                sent: std::sync::atomic::AtomicUsize::new(0),
                polled: std::sync::Mutex::new(None),
                frame: None,
            }
        }
    }

    impl AgentServerBackend for CountingBackend {
        fn send_input_event(&self, _event: ferryx_rd_proto::InputEvent) -> Result<(), String> {
            self.sent.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
        fn get_screen_info(&self) -> ScreenInfo {
            ScreenInfo {
                width: 1920,
                height: 1080,
                scale: 1.0,
                logical_width: Some(1920),
                logical_height: Some(1080),
                monitors: vec![],
                connected_host: "bounded-io".to_string(),
            }
        }
        fn get_latest_frame_nv12(&self) -> Option<(u32, u32, Arc<Vec<u8>>)> {
            None
        }
        fn get_latest_frame_metadata(&self) -> Option<FrameMetadata> {
            if let Some(polled) = self.polled.lock().unwrap().take() {
                let _ = polled.send(());
            }
            self.frame
        }
    }

    #[tokio::test]
    async fn oversized_record_is_rejected_without_unbounded_buffering() {
        // Given a peer that sends more than the per-record limit before any newline.
        let backend = Arc::new(CountingBackend::new());
        let mut oversized = vec![b'x'; MAX_MCP_RECORD_BYTES * 2];
        oversized.push(b'\n');
        // When the stdio server reads it.
        let error = run_mcp_io(backend.clone(), &oversized[..], tokio::io::sink())
            .await
            .expect_err("oversized record must fail explicitly");
        // Then the record is refused explicitly and held input is still released.
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(backend.sent.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn screen_change_wait_observes_shutdown_while_waiting() {
        use tokio::io::AsyncWriteExt;
        // Given a backend whose frame never changes, so the wait runs its full 30s timeout.
        let (polled_tx, polled_rx) = tokio::sync::oneshot::channel();
        let backend = Arc::new(CountingBackend {
            polled: std::sync::Mutex::new(Some(polled_tx)),
            frame: Some(FrameMetadata {
                frame_id: 7,
                timestamp_ms: 0,
                age_ms: 0,
            }),
            ..CountingBackend::new()
        });
        let (stop, receiver) = tokio::sync::watch::channel(false);
        let (mut peer, reader) = tokio::io::duplex(4096);
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "remote_wait_for_screen_change",
                "arguments": {"last_frame_id": 7, "timeout_ms": 30000},
            }
        })
        .to_string();
        peer.write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        let server = tokio::spawn(run_mcp_io_until(
            backend,
            (reader, tokio::io::sink()),
            receiver,
        ));
        // When shutdown is signalled after the wait loop has started polling.
        polled_rx.await.unwrap();
        stop.send(true).unwrap();
        // Then the server stops instead of blocking for the remaining timeout.
        tokio::time::timeout(std::time::Duration::from_secs(5), server)
            .await
            .expect("shutdown must preempt the screen-change wait")
            .unwrap()
            .unwrap();
    }
}
