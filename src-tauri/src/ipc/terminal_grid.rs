//! Local grid stream for the DOM compatibility renderer.
//!
//! When the native renderer is unavailable (no GPU adapter), the pane renders the same JSON grid
//! frames the remote client consumes (`ui/src/remote/terminalGridProtocol.ts`), produced here by a
//! headless [`RemoteTerminalMirror`] fed from the daemon attachment. The session itself is not
//! touched: detaching a stream only stops this subscriber.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::mpsc;

use crate::daemon::{DaemonClient, DaemonStreamMessage};
use crate::ipc::{IpcError, IpcErrorCode};
use crate::remote::mirror::RemoteTerminalMirror;
use crate::remote::protocol::RemoteGridFrame;

const GRID_MAX_COLS: u16 = 512;
const GRID_MAX_ROWS: u16 = 256;

enum GridCommand {
    Resize(u16, u16),
}

struct GridStream {
    commands: mpsc::UnboundedSender<GridCommand>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for GridStream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Default, Clone)]
pub struct TerminalGridStreams {
    streams: Arc<Mutex<HashMap<String, GridStream>>>,
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn grid_text(frame: &RemoteGridFrame) -> Option<String> {
    serde_json::to_string(frame).ok()
}

fn clamp_geometry(cols: u16, rows: u16) -> (u16, u16) {
    (cols.clamp(1, GRID_MAX_COLS), rows.clamp(1, GRID_MAX_ROWS))
}

fn mirror_error(error: crate::native_terminal::NativeTerminalError) -> IpcError {
    IpcError::internal(error.to_string())
}

/// Subscribes `channel` to grid frames for `session_id` and returns the stream id used to resize
/// and detach. The first frame is a full `grid` snapshot of the replayed history.
#[tauri::command]
pub async fn cmd_terminal_grid_attach(
    daemon_client: State<'_, Arc<DaemonClient>>,
    streams: State<'_, TerminalGridStreams>,
    session_id: String,
    channel: Channel<String>,
) -> Result<String, IpcError> {
    let attachment = daemon_client.attach(&session_id, None).await?;
    let upstream = AbortOnDrop(attachment.stream_task);
    let (cols, rows) = clamp_geometry(
        attachment.pty_cols.unwrap_or(80),
        attachment.pty_rows.unwrap_or(24),
    );
    let mut mirror = RemoteTerminalMirror::new(cols, rows).map_err(mirror_error)?;
    if !attachment.history_segments.is_empty() {
        mirror.feed_segments(&attachment.history_segments).map_err(mirror_error)?;
    } else if !attachment.history.is_empty() {
        mirror.feed(&attachment.history).map_err(mirror_error)?;
    }
    if mirror.dimensions().ok() != Some((cols, rows)) {
        mirror.resize(cols, rows).map_err(mirror_error)?;
    }
    let initial = mirror.full_frame().map_err(mirror_error)?;
    if let Some(text) = grid_text(&initial) {
        channel
            .send(text)
            .map_err(|e| IpcError::new(IpcErrorCode::ChannelError, e.to_string()))?;
    }

    let stream_id = uuid::Uuid::new_v4().to_string();
    let (command_tx, mut command_rx) = mpsc::unbounded_channel();
    let mut messages = attachment.messages;
    let task = tokio::spawn(async move {
        let _upstream = upstream;
        // Idle scrollback compression of the mirror: output restarts the idle delay and a
        // third select arm runs one bounded step when the scheduler's deadline passes. The
        // task owns the mirror, so steps are serialized with every other mirror access.
        mirror.mark_activity(std::time::Instant::now());
        loop {
            let compression_deadline = mirror
                .compression_wakeup(std::time::Instant::now())
                .map(|wait| tokio::time::Instant::now() + wait);
            let frame = tokio::select! {
                command = command_rx.recv() => match command {
                    Some(GridCommand::Resize(cols, rows)) => {
                        mirror.mark_activity(std::time::Instant::now());
                        mirror.resize(cols, rows)
                    }
                    None => break,
                },
                message = messages.recv() => match message {
                    Some(DaemonStreamMessage::Output { data, .. }) => {
                        mirror.mark_activity(std::time::Instant::now());
                        mirror.feed(&data)
                    }
                    Some(DaemonStreamMessage::Lagged { history, segments, .. }) => {
                        // A replay replaces the screen: rebuild the mirror from it at the current size.
                        let Ok((cols, rows)) = mirror.dimensions() else { break };
                        let Ok(mut replacement) = RemoteTerminalMirror::new(cols, rows) else { break };
                        let segments: Vec<_> = segments
                            .into_iter()
                            .map(|s| crate::terminal::output_hub::HistorySegment {
                                cols: s.cols,
                                rows: s.rows,
                                bytes: s.bytes.to_vec(),
                            })
                            .collect();
                        let fed = if segments.is_empty() {
                            replacement.feed(&history).map(|_| ())
                        } else {
                            replacement.feed_segments(&segments)
                        };
                        if fed.is_err() {
                            break;
                        }
                        if replacement.dimensions().ok() != Some((cols, rows))
                            && replacement.resize(cols, rows).is_err()
                        {
                            break;
                        }
                        mirror = replacement;
                        mirror.mark_activity(std::time::Instant::now());
                        mirror.full_frame()
                    }
                    Some(DaemonStreamMessage::Exit { .. }) | None => break,
                    Some(_) => continue,
                },
                _ = async {
                    match compression_deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    let enabled = crate::terminal::cached_terminal_preferences().scrollback_compression;
                    if let Err(error) = mirror.run_due_compression(std::time::Instant::now(), enabled) {
                        tracing::warn!(%error, "grid mirror scrollback compression step failed");
                    }
                    // Compression changes no visible cell, so no frame is sent.
                    continue;
                }
            };
            let Ok(frame) = frame else { break };
            let Some(text) = grid_text(&frame) else { break };
            if channel.send(text).is_err() {
                break;
            }
        }
    });
    streams
        .streams
        .lock()
        .insert(stream_id.clone(), GridStream { commands: command_tx, task });
    Ok(stream_id)
}

/// Resizes the PTY and the stream's mirror so frames match the DOM grid geometry.
#[tauri::command]
pub async fn cmd_terminal_grid_resize(
    daemon_client: State<'_, Arc<DaemonClient>>,
    streams: State<'_, TerminalGridStreams>,
    stream_id: String,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), IpcError> {
    let (cols, rows) = clamp_geometry(cols, rows);
    daemon_client.resize_terminal(&session_id, cols, rows).await?;
    if let Some(stream) = streams.streams.lock().get(&stream_id) {
        let _ = stream.commands.send(GridCommand::Resize(cols, rows));
    }
    Ok(())
}

/// Stops one grid subscriber. The PTY session keeps running.
#[tauri::command]
pub async fn cmd_terminal_grid_detach(
    streams: State<'_, TerminalGridStreams>,
    stream_id: String,
) -> Result<(), IpcError> {
    streams.streams.lock().remove(&stream_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_geometry_is_clamped_to_the_dom_renderer_bounds() {
        assert_eq!(clamp_geometry(0, 0), (1, 1));
        assert_eq!(clamp_geometry(9999, 9999), (GRID_MAX_COLS, GRID_MAX_ROWS));
        assert_eq!(clamp_geometry(80, 24), (80, 24));
    }

    #[test]
    fn grid_frames_use_the_remote_client_wire_shape() {
        let mut mirror = RemoteTerminalMirror::new(12, 2).unwrap();
        let frame = mirror.feed(b"FALLBACK_OK").unwrap();
        let value: serde_json::Value = serde_json::from_str(&grid_text(&frame).unwrap()).unwrap();
        assert_eq!(value["type"], "grid");
        assert_eq!(value["cols"], 12);
        assert_eq!(value["rows"], 2);
    }
}
