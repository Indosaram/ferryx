//! Live omo turns by session, so `/omo-stop` can interrupt one and `/omo-steer` can inject
//! guidance into it (omo-gateway's `active_turns`). A turn registers for as long as it runs;
//! the engine ids arrive once `turn/start` is answered.
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use tokio::sync::watch;

#[derive(Debug)]
struct Entry {
    channel: String,
    thread_id: Option<String>,
    turn_id: Option<String>,
    cancel: watch::Sender<bool>,
    generation: u64,
}

fn turns() -> &'static Mutex<(u64, HashMap<String, Entry>)> {
    static TURNS: OnceLock<Mutex<(u64, HashMap<String, Entry>)>> = OnceLock::new();
    TURNS.get_or_init(|| Mutex::new((0, HashMap::new())))
}

tokio::task_local! {
    static CURRENT: String;
}

/// Removes the registration when the turn ends, unless a newer turn of the session replaced it.
#[derive(Debug)]
pub struct TurnGuard {
    key: String,
    generation: u64,
}

impl Drop for TurnGuard {
    fn drop(&mut self) {
        let mut guard = turns().lock().expect("turn registry");
        if guard
            .1
            .get(&self.key)
            .is_some_and(|entry| entry.generation == self.generation)
        {
            guard.1.remove(&self.key);
        }
    }
}

pub fn begin(key: &str, channel: &str) -> TurnGuard {
    let (cancel, _) = watch::channel(false);
    let mut guard = turns().lock().expect("turn registry");
    guard.0 += 1;
    let generation = guard.0;
    guard.1.insert(
        key.to_owned(),
        Entry {
            channel: channel.to_owned(),
            thread_id: None,
            turn_id: None,
            cancel,
            generation,
        },
    );
    TurnGuard {
        key: key.to_owned(),
        generation,
    }
}

/// Runs `future` (one turn) with `key` visible to the engine client.
pub async fn scope<F: std::future::Future>(key: String, future: F) -> F::Output {
    CURRENT.scope(key, future).await
}

pub fn current_key() -> Option<String> {
    CURRENT.try_with(Clone::clone).ok()
}

/// The engine accepted the turn: these ids are what `turn/interrupt` and `turn/steer` name.
pub fn started(key: &str, thread_id: &str, turn_id: &str) {
    if let Some(entry) = turns().lock().expect("turn registry").1.get_mut(key) {
        entry.thread_id = Some(thread_id.to_owned());
        entry.turn_id = Some(turn_id.to_owned()).filter(|id| !id.is_empty());
    }
}

/// Fires once `/omo-stop` asks for this session's turn to end.
pub fn cancelled(key: &str) -> Option<watch::Receiver<bool>> {
    turns()
        .lock()
        .expect("turn registry")
        .1
        .get(key)
        .map(|entry| entry.cancel.subscribe())
}

/// Asks the running turn of `key` to stop. False when nothing runs there.
pub fn stop(key: &str) -> bool {
    match turns().lock().expect("turn registry").1.get(key) {
        Some(entry) => {
            entry.cancel.send_replace(true);
            true
        }
        None => false,
    }
}

pub fn is_active(key: &str) -> bool {
    turns().lock().expect("turn registry").1.contains_key(key)
}

/// `(thread_id, turn_id)` of the running turn, once the engine named them.
pub fn ids(key: &str) -> Option<(String, String)> {
    let guard = turns().lock().expect("turn registry");
    let entry = guard.1.get(key)?;
    Some((entry.thread_id.clone()?, entry.turn_id.clone()?))
}

/// Sessions with a running turn in `channel`, for a slash command typed at the channel's top level.
pub fn active_in_channel(channel: &str) -> Vec<String> {
    let mut keys: Vec<String> = turns()
        .lock()
        .expect("turn registry")
        .1
        .iter()
        .filter(|(_, entry)| entry.channel == channel)
        .map(|(key, _)| key.clone())
        .collect();
    keys.sort();
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_turn_registers_reports_ids_and_can_be_stopped() {
        let guard = begin("k-test-1", "C-test-1");
        assert!(is_active("k-test-1"));
        assert_eq!(active_in_channel("C-test-1"), vec!["k-test-1".to_owned()]);
        assert_eq!(ids("k-test-1"), None);
        started("k-test-1", "th", "tu");
        assert_eq!(ids("k-test-1"), Some(("th".into(), "tu".into())));
        let mut rx = cancelled("k-test-1").unwrap();
        assert!(stop("k-test-1"));
        rx.changed().await.unwrap();
        assert!(*rx.borrow());
        drop(guard);
        assert!(!is_active("k-test-1"));
        assert!(!stop("k-test-1"));
        let seen = scope("k-test-2".into(), async { current_key() }).await;
        assert_eq!(seen.as_deref(), Some("k-test-2"));
        assert_eq!(current_key(), None);
    }

    #[test]
    fn a_newer_turn_is_not_removed_by_an_older_guard() {
        let old = begin("k-test-3", "C");
        let new = begin("k-test-3", "C");
        drop(old);
        assert!(is_active("k-test-3"));
        drop(new);
        assert!(!is_active("k-test-3"));
    }

    /// e2e against a mock omo app-server whose turn never ends on its own: the turn is
    /// steered (`turn/steer` with the live turn id) and then stopped (`turn/interrupt`).
    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn e2e_a_live_omo_turn_is_steered_then_stopped() {
        use crate::meta::omo_client::{run_turn, steer, OmoClientConfig, TurnRequest, STOPPED};
        use futures_util::{SinkExt, StreamExt};
        use serde_json::{json, Value};
        use std::sync::{Arc, Mutex as StdMutex};
        use tokio_tungstenite::tungstenite::Message;

        let frames: Arc<StdMutex<Vec<Value>>> = Arc::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let log = frames.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let log = log.clone();
                tokio::spawn(async move {
                    let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
                    while let Some(Ok(Message::Text(text))) = ws.next().await {
                        let frame: Value = serde_json::from_str(&text).unwrap();
                        log.lock().unwrap().push(frame.clone());
                        let id = frame.get("id").cloned().unwrap_or(Value::Null);
                        let reply = match frame.get("method").and_then(Value::as_str) {
                            Some("initialize") => json!({"jsonrpc":"2.0","id":id,"result":{}}),
                            Some("thread/start") => {
                                json!({"jsonrpc":"2.0","id":id,"result":{"thread":{"id":"th-live"}}})
                            }
                            Some("turn/start") => {
                                json!({"jsonrpc":"2.0","id":id,"result":{"turn":{"id":"tu-live"}}})
                            }
                            Some("turn/steer") => json!({"jsonrpc":"2.0","id":id,"result":{}}),
                            _ => continue,
                        };
                        ws.send(Message::text(reply.to_string())).await.unwrap();
                    }
                });
            }
        });

        let config = OmoClientConfig::new(url);
        let turn_config = config.clone();
        let guard = begin("k-e2e", "C-e2e");
        let turn = tokio::spawn(scope("k-e2e".into(), async move {
            run_turn(
                &turn_config,
                &TurnRequest {
                    text: "long job".into(),
                    ..TurnRequest::default()
                },
                None,
            )
            .await
        }));
        let mut ids_seen = None;
        for _ in 0..200 {
            ids_seen = ids("k-e2e");
            if ids_seen.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let (thread_id, turn_id) = ids_seen.expect("the engine ids are registered");
        assert_eq!(
            (thread_id.as_str(), turn_id.as_str()),
            ("th-live", "tu-live")
        );

        assert!(
            steer(&config, &thread_id, &turn_id, "[Steering] use staging")
                .await
                .unwrap()
        );
        assert!(stop("k-e2e"));
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), turn)
            .await
            .expect("the stopped turn ends")
            .unwrap();
        assert!(result.unwrap_err().to_string().contains(STOPPED));
        drop(guard);

        // The interrupt is written just before the client closes; give the mock a moment.
        for _ in 0..200 {
            if frames
                .lock()
                .unwrap()
                .iter()
                .any(|frame| frame["method"] == "turn/interrupt")
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let frames = frames.lock().unwrap().clone();
        let steer_frame = frames
            .iter()
            .find(|frame| frame["method"] == "turn/steer")
            .expect("turn/steer sent");
        assert_eq!(steer_frame["params"]["expectedTurnId"], "tu-live");
        assert_eq!(
            steer_frame["params"]["input"][0]["text"],
            "[Steering] use staging"
        );
        let interrupt = frames
            .iter()
            .find(|frame| frame["method"] == "turn/interrupt")
            .expect("turn/interrupt sent");
        assert_eq!(interrupt["params"]["turnId"], "tu-live");
        assert_eq!(interrupt["params"]["threadId"], "th-live");
    }
}
