//! Test-only observations; the production future is polled unchanged.
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    task::Poll,
};
use tokio::sync::watch;

#[derive(Clone, Debug, Default)]
pub(crate) struct Progress {
    pub(crate) started: u64,
    pub(crate) completed: u64,
    pub(crate) succeeded: u64,
    pub(crate) failed: u64,
    pub(crate) last_error: Option<String>,
    pub pending: bool,
    pub dropped: bool,
    pub queue_full: bool,
}
static OBSERVERS: LazyLock<Mutex<HashMap<String, watch::Sender<Progress>>>> =
    LazyLock::new(Mutex::default);

pub(crate) struct Observation(pub watch::Receiver<Progress>, String);
impl Observation {
    pub(crate) fn register(id: &str) -> Self {
        let (sender, receiver) = watch::channel(Progress::default());
        OBSERVERS.lock().unwrap().insert(id.into(), sender);
        Self(receiver, id.into())
    }
}
impl Drop for Observation {
    fn drop(&mut self) {
        OBSERVERS.lock().unwrap().remove(&self.1);
    }
}
pub(super) fn queue_full(id: &str) {
    if let Some(sender) = OBSERVERS.lock().unwrap().get(id) {
        sender.send_modify(|state| state.queue_full = true);
    }
}
pub(super) async fn observe<F, T, E>(id: &str, future: F) -> Result<T, E>
where
    F: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    struct Pending(Option<Arc<watch::Sender<Progress>>>);
    impl Drop for Pending {
        fn drop(&mut self) {
            if let Some(sender) = &self.0 {
                sender.send_modify(|state| state.dropped = true);
            }
        }
    }
    let sender = OBSERVERS.lock().unwrap().get(id).cloned().map(Arc::new);
    let pending = Pending(sender);
    let mut started = false;
    tokio::pin!(future);
    std::future::poll_fn(|cx| {
        if !started {
            if let Some(sender) = &pending.0 {
                sender.send_modify(|state| state.started += 1);
            }
            started = true;
        }
        let result = future.as_mut().poll(cx);
        if let Some(sender) = &pending.0 {
            match &result {
                Poll::Pending => {
                    sender.send_modify(|state| state.pending = true);
                }
                Poll::Ready(Ok(_)) => {
                    sender.send_modify(|state| {
                        state.completed += 1;
                        state.succeeded += 1;
                    });
                }
                Poll::Ready(Err(err)) => {
                    let err_str = err.to_string();
                    sender.send_modify(|state| {
                        state.completed += 1;
                        state.failed += 1;
                        state.last_error = Some(err_str);
                    });
                }
            }
        }
        result
    })
    .await
}
