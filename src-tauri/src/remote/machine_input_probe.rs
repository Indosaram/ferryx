//! Test-only observations; the production future is polled unchanged.
use std::{collections::HashMap, sync::{Arc, LazyLock, Mutex}, task::Poll};
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Progress {
    pub pending: bool,
    pub dropped: bool,
    pub queue_full: bool,
}
static OBSERVERS: LazyLock<Mutex<HashMap<String, watch::Sender<Progress>>>> = LazyLock::new(Mutex::default);

pub(super) struct Observation(pub watch::Receiver<Progress>, String);
impl Observation {
    pub fn register(id: &str) -> Self {
        let (sender, receiver) = watch::channel(Progress::default());
        OBSERVERS.lock().unwrap().insert(id.into(), sender);
        Self(receiver, id.into())
    }
}
impl Drop for Observation {
    fn drop(&mut self) { OBSERVERS.lock().unwrap().remove(&self.1); }
}
pub(super) fn queue_full(id: &str) {
    if let Some(sender) = OBSERVERS.lock().unwrap().get(id) {
        sender.send_modify(|state| state.queue_full = true);
    }
}
pub(super) async fn observe<F: std::future::Future>(id: &str, future: F) -> F::Output {
    struct Pending(Option<Arc<watch::Sender<Progress>>>);
    impl Drop for Pending {
        fn drop(&mut self) {
            if let Some(sender) = &self.0 { sender.send_modify(|state| state.dropped = true); }
        }
    }
    let sender = OBSERVERS.lock().unwrap().get(id).cloned().map(Arc::new);
    let pending = Pending(sender);
    tokio::pin!(future);
    std::future::poll_fn(|cx| {
        let result = future.as_mut().poll(cx);
        if matches!(result, Poll::Pending) {
            if let Some(sender) = &pending.0 { sender.send_modify(|state| state.pending = true); }
        }
        result
    }).await
}
