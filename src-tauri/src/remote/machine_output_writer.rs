//! Machine writer admission; guards at the caller survive send AND flush.
use axum::extract::ws::Message;
use futures_util::{Sink, SinkExt};
use tokio::sync::watch;
use crate::terminal::output_hub::machine_output::{MachineOutputError, MACHINE_FRAME_OVERHEAD};
use std::time::Duration;

const WS_HEADER_BYTES: usize = 10;
const CONTROL_SLOT_BYTES: usize = 1024;

pub(super) fn machine_control(message: Message) -> Result<Message, ()> {
    let bytes = match &message {
        Message::Text(text) => text.len(),
        Message::Binary(bytes) | Message::Ping(bytes) | Message::Pong(bytes) => bytes.len(),
        Message::Close(frame) => frame.as_ref().map_or(0, |frame| 2 + frame.reason.len()),
    };
    if bytes + WS_HEADER_BYTES > CONTROL_SLOT_BYTES { return Err(()); }
    Ok(message)
}

pub(super) fn machine_frame(frame: Vec<u8>, payload: usize) -> Result<Message, ()> {
    if frame.len().saturating_sub(payload) + WS_HEADER_BYTES > MACHINE_FRAME_OVERHEAD { return Err(()); }
    Ok(Message::Binary(frame.into()))
}

pub(super) async fn machine_send<S>(
    sender: &mut S, message: Message, termination: &mut watch::Receiver<Option<MachineOutputError>>,
) -> Result<(), ()>
where S: Sink<Message> + Unpin {
    tokio::select! {
        biased;
        _ = termination.wait_for(|state| state.is_some()) => Err(()),
        result = tokio::time::timeout(Duration::from_secs(10), async {
            sender.send(message).await.map_err(|_| ())?;
            sender.flush().await.map_err(|_| ())
        }) => result.map_err(|_| ())?,
    }
}

#[cfg(test)]
#[path = "machine_output_writer_tests.rs"]
mod tests;
