//! Live omo turns by session, so `/omo-stop` can interrupt one and `/omo-steer` can inject
//! guidance into it (omo-gateway's `active_turns`). A turn registers for as long as it runs;
//! the durable session id and hub-local turn id arrive after mhc opens the session.
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
    #[cfg(test)]
    registry_changes().send_modify(|version| *version += 1);
}

#[cfg(test)]
fn registry_changes() -> &'static watch::Sender<u64> {
    static CHANGES: OnceLock<watch::Sender<u64>> = OnceLock::new();
    CHANGES.get_or_init(|| watch::channel(0).0)
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

    /// The executable mhc peer acknowledges prompt but leaves the turn running.
    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn e2e_a_live_omo_turn_is_steered_then_stopped() {
        use crate::meta::omo_client::{run_turn, steer, OmoClientConfig, TurnRequest, STOPPED};
        use crate::meta::rpc_mock::{serve, Script};

        let (binary, log) = serve(Script {
            messages: Vec::new(),
            approval: false,
            live: true,
        })
        .await;
        let config = OmoClientConfig::new(binary);
        let turn_config = config.clone();
        let guard = begin("k-e2e", "C-e2e");
        let mut ready = registry_changes().subscribe();
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
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while ids("k-e2e").is_none() {
                ready.changed().await.unwrap();
            }
        })
        .await
        .expect("turn ready");
        let (thread_id, turn_id) = ids("k-e2e").expect("engine ids registered");
        assert_eq!(thread_id, log.session_file.to_string_lossy());
        assert!(!turn_id.is_empty());
        assert!(
            steer(&config, &thread_id, &turn_id, "[Steering] use staging")
                .await
                .unwrap()
        );
        assert!(stop("k-e2e"));
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), turn)
            .await
            .expect("stopped turn ends")
            .unwrap();
        assert!(result.unwrap_err().to_string().contains(STOPPED));
        drop(guard);
        log.wait_for(|frames| frames.iter().any(|f| f["type"] == "steer"))
            .await;
        let frames = log.frames.lock().unwrap();
        let steer = frames
            .iter()
            .find(|f| f["type"] == "steer")
            .expect("steer sent");
        assert_eq!(steer["message"], "[Steering] use staging");
        // Each turn owns a single-session mhc RPC child, so steer frames carry no session id.
        assert!(steer.get("sessionId").is_none());
    }
}
