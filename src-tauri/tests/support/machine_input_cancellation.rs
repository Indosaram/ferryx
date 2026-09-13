use super::machine_input_probe::Observation;
#[path = "machine_input_fixture.rs"]
mod fixture;
use fixture::{Fixture, Held, DEADLINE};
use futures_util::{FutureExt, SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

#[derive(Clone, Copy, Debug)]
enum Cancellation { Disconnect, Revoke, Replace, SaturatedQueue }

async fn scenario(cancellation: Cancellation) {
    // Given a real machine-owned nonreading PTY, filled to kernel WouldBlock.
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let session = fixture.create().await;
        let id = session["target"]["sessionId"].as_str().unwrap();
        let mut socket = fixture.attach(&session).await;
        let held = Held::start(&fixture, &session, &mut socket).await;
        let mut observation = Observation::register(id);
        socket.send(Message::Binary(b"LATE".to_vec().into())).await.unwrap();
        tokio::time::timeout(DEADLINE, observation.0.wait_for(|p| p.pending)).await.expect("real input future must become pending").unwrap();
        eprintln!("A10 real socket input pending scenario={cancellation:?}");
        // When the socket lifetime ends while production input is pending.
        match cancellation {
            Cancellation::Disconnect => socket.close(None).await.unwrap(),
            Cancellation::Revoke => { fixture.state.auth_manager.revoke_device(&fixture.device); }
            Cancellation::Replace => {
                let replacement = fixture.attach(&session).await;
                let _ = socket.send(Message::Binary(b"Z".to_vec().into())).await;
                drop(replacement);
            }
            Cancellation::SaturatedQueue => {
                socket.send(Message::Binary(b"QUEUED".to_vec().into())).await.unwrap();
                socket.send(Message::Binary(b"QUEUED".to_vec().into())).await.unwrap();
                tokio::time::timeout(DEADLINE, observation.0.wait_for(|p| p.queue_full)).await.unwrap().unwrap();
                socket.close(None).await.unwrap();
            }
        }
        // Then pending production IO is dropped before the child is released.
        tokio::time::timeout(DEADLINE, observation.0.wait_for(|p| p.dropped)).await.expect("cancellation must drop pending input without waiting for PTY timeout").unwrap();
        drop(socket);
        held.drain().await;
    }).catch_unwind().await;
    fixture.cleanup().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn input_is_cancelled_when_socket_disconnects() { scenario(Cancellation::Disconnect).await; }
#[tokio::test]
async fn input_is_cancelled_when_grant_is_revoked() { scenario(Cancellation::Revoke).await; }
#[tokio::test]
async fn input_is_cancelled_when_generation_is_replaced() { scenario(Cancellation::Replace).await; }
#[tokio::test]
async fn input_is_cancelled_when_full_queue_precedes_close() { scenario(Cancellation::SaturatedQueue).await; }

pub(crate) async fn disconnect_and_revoke() {
    scenario(Cancellation::Disconnect).await;
    scenario(Cancellation::Revoke).await;
}

#[tokio::test]
async fn sibling_pty_is_responsive_when_first_input_is_saturated() {
    // Given two real machine sockets and an exactly observed pending first input.
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let session = fixture.create().await;
        let sibling = fixture.create().await;
        let mut one = fixture.attach(&session).await;
        let mut two = fixture.attach(&sibling).await;
        let held = Held::start(&fixture, &session, &mut one).await;
        let mut observation = Observation::register(session["target"]["sessionId"].as_str().unwrap());
        one.send(Message::Binary(b"LATE".to_vec().into())).await.unwrap();
        tokio::time::timeout(DEADLINE, observation.0.wait_for(|p| p.pending)).await.unwrap().unwrap();
        // When input is sent to the independent second PTY over its actual WS.
        two.send(Message::Binary(b"printf '\\nA10_%s:%s:END\\n' SIBLING \"$$\"\r".to_vec().into())).await.unwrap();
        // Then that shell executes before the first PTY is released.
        let pid = fixture.owner.terminal_service().get_session(sibling["target"]["sessionId"].as_str().unwrap()).unwrap().pid().unwrap();
        let marker = format!("A10_SIBLING:{pid}:END");
        tokio::time::timeout(DEADLINE, async {
            let mut output = Vec::new();
            loop {
                if let Message::Binary(bytes) = two.next().await.unwrap().unwrap() {
                    output.extend_from_slice(crate::remote::terminal_wire::decode_frame(&bytes).unwrap().terminal_bytes);
                    if output.windows(marker.len()).any(|w| w == marker.as_bytes()) { break; }
                }
            }
        }).await.expect("sibling PTY must not serialize behind saturated machine input");
        eprintln!("A10 sibling original_pid={pid} executed while first saturated");
        fixture.state.auth_manager.revoke_device(&fixture.device);
        tokio::time::timeout(DEADLINE, observation.0.wait_for(|p| p.dropped)).await.unwrap().unwrap();
        drop(one); drop(two);
        held.drain().await;
    }).catch_unwind().await;
    fixture.cleanup().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}
