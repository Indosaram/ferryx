use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc::UnboundedSender, Notify};

use crate::session::Event;

pub struct SnapItem {
    pub subscription_id: u64,
    pub reservation_id: u64,
    pub snapshot_id: u64,
    pub session: UnboundedSender<Event>,
    pub bytes: Vec<u8>,
    pub last: bool,
}

pub enum Item {
    Frame(Vec<u8>),
    Snap(SnapItem),
}

impl Item {
    fn len(&self) -> usize {
        match self {
            Item::Frame(b) => b.len(),
            Item::Snap(s) => s.bytes.len(),
        }
    }
}

#[derive(Default)]
pub struct Outbox {
    queue: Mutex<VecDeque<Item>>,
    bytes: AtomicUsize,
    notify: Notify,
    closed: AtomicBool,
}

#[derive(Clone)]
pub struct ConnHandle {
    pub id: u64,
    pub outbox: Arc<Outbox>,
}

impl Outbox {
    pub fn push_frame(&self, bytes: Vec<u8>) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        self.bytes.fetch_add(bytes.len(), Ordering::AcqRel);
        self.queue.lock().expect("outbox").push_back(Item::Frame(bytes));
        self.notify.notify_one();
    }

    pub fn push_snap(&self, item: SnapItem) {
        if self.closed.load(Ordering::Acquire) {
            release(&item);
            return;
        }
        self.bytes.fetch_add(item.bytes.len(), Ordering::AcqRel);
        self.queue.lock().expect("outbox").push_back(Item::Snap(item));
        self.notify.notify_one();
    }

    pub fn queued_bytes(&self) -> usize {
        self.bytes.load(Ordering::Acquire)
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Removes the not-yet-started frames of one snapshot. When any frame is
    /// removed, the buffer release is reported here; otherwise the writer
    /// reports it after the last frame.
    pub fn drop_pending(&self, subscription_id: u64, snapshot_id: u64) {
        let mut q = self.queue.lock().expect("outbox");
        let mut removed: Option<SnapItem> = None;
        let mut kept = VecDeque::with_capacity(q.len());
        for item in q.drain(..) {
            match item {
                Item::Snap(s) if s.subscription_id == subscription_id && s.snapshot_id == snapshot_id => {
                    self.bytes.fetch_sub(s.bytes.len(), Ordering::AcqRel);
                    removed = Some(s);
                }
                other => kept.push_back(other),
            }
        }
        *q = kept;
        drop(q);
        if let Some(s) = removed {
            release(&s);
        }
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.notify.notify_one();
    }

    fn pop(&self) -> Option<Item> {
        let item = self.queue.lock().expect("outbox").pop_front()?;
        self.bytes.fetch_sub(item.len(), Ordering::AcqRel);
        Some(item)
    }

    fn drain_snapshots(&self) {
        let items: Vec<Item> = self.queue.lock().expect("outbox").drain(..).collect();
        self.bytes.store(0, Ordering::Release);
        let mut seen: Vec<(u64, u64, u64)> = Vec::new();
        for item in items {
            if let Item::Snap(s) = item {
                let key = (s.subscription_id, s.reservation_id, s.snapshot_id);
                if !seen.contains(&key) {
                    seen.push(key);
                    release(&s);
                }
            }
        }
    }
}

fn release(s: &SnapItem) {
    let _ = s.session.send(Event::Released { subscription_id: s.subscription_id, reservation_id: s.reservation_id, snapshot_id: s.snapshot_id });
}

pub async fn run_writer<W: AsyncWrite + Unpin>(outbox: Arc<Outbox>, mut w: W) {
    loop {
        let Some(item) = outbox.pop() else {
            if outbox.is_closed() {
                break;
            }
            outbox.notify.notified().await;
            continue;
        };
        match &item {
            Item::Frame(b) => {
                if w.write_all(b).await.is_err() {
                    break;
                }
            }
            Item::Snap(s) => {
                let ok = w.write_all(&s.bytes).await.is_ok();
                if !ok {
                    release(s);
                    break;
                }
                let _ = s.session.send(Event::FrameWritten {
                    subscription_id: s.subscription_id,
                    reservation_id: s.reservation_id,
                    snapshot_id: s.snapshot_id,
                    last: s.last,
                });
                if s.last {
                    release(s);
                }
            }
        }
    }
    outbox.close();
    outbox.drain_snapshots();
    let _ = w.shutdown().await;
}
