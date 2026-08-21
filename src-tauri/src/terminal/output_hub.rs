use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::broadcast;

const DEFAULT_BUFFER_CAPACITY: usize = 512 * 1024; // 512 KiB
const BROADCAST_CAPACITY: usize = 1024;

pub struct BoundedBuffer {
    capacity: usize,
    chunks: VecDeque<Vec<u8>>,
    current_size: usize,
}

impl BoundedBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            chunks: VecDeque::new(),
            current_size: 0,
        }
    }

    pub fn push(&mut self, chunk: Vec<u8>) {
        if chunk.is_empty() {
            return;
        }
        self.current_size += chunk.len();
        self.chunks.push_back(chunk);

        while self.current_size > self.capacity && !self.chunks.is_empty() {
            if let Some(front) = self.chunks.pop_front() {
                self.current_size -= front.len();
            }
        }
    }

    pub fn snapshot(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.current_size);
        for chunk in &self.chunks {
            out.extend_from_slice(chunk);
        }
        out
    }
}

struct SessionHub {
    buffer: BoundedBuffer,
    sender: broadcast::Sender<Vec<u8>>,
}

#[derive(Clone)]
pub struct TerminalOutputHub {
    sessions: Arc<RwLock<HashMap<String, Arc<RwLock<SessionHub>>>>>,
    capacity: usize,
}

impl Default for TerminalOutputHub {
    fn default() -> Self {
        Self::new(DEFAULT_BUFFER_CAPACITY)
    }
}

impl TerminalOutputHub {
    pub fn new(capacity: usize) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            capacity,
        }
    }

    pub fn register_session(&self, session_id: &str) -> broadcast::Receiver<Vec<u8>> {
        let (tx, rx) = broadcast::channel(BROADCAST_CAPACITY);
        let hub = SessionHub {
            buffer: BoundedBuffer::new(self.capacity),
            sender: tx,
        };
        self.sessions
            .write()
            .insert(session_id.to_string(), Arc::new(RwLock::new(hub)));
        rx
    }

    pub fn publish(&self, session_id: &str, chunk: Vec<u8>) {
        let session_hub = {
            let sessions = self.sessions.read();
            sessions.get(session_id).cloned()
        };

        if let Some(hub) = session_hub {
            let mut hub = hub.write();
            hub.buffer.push(chunk.clone());
            // It is okay if there are no active receivers; send returns Err in that case
            let _ = hub.sender.send(chunk);
        }
    }

    pub fn subscribe(&self, session_id: &str) -> Option<(Vec<u8>, broadcast::Receiver<Vec<u8>>)> {
        let session_hub = {
            let sessions = self.sessions.read();
            sessions.get(session_id).cloned()
        }?;

        let hub = session_hub.read();
        let history = hub.buffer.snapshot();
        let rx = hub.sender.subscribe();
        Some((history, rx))
    }

    pub fn remove_session(&self, session_id: &str) {
        self.sessions.write().remove(session_id);
    }

    pub fn has_session(&self, session_id: &str) -> bool {
        self.sessions.read().contains_key(session_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_output_hub_replay_and_broadcast() {
        let hub = TerminalOutputHub::new(1024);
        let session_id = "test-session";
        let _initial_rx = hub.register_session(session_id);

        hub.publish(session_id, b"hello ".to_vec());
        hub.publish(session_id, b"world\n".to_vec());

        let (history, mut rx) = hub.subscribe(session_id).expect("session exists");
        assert_eq!(history, b"hello world\n");

        hub.publish(session_id, b"live chunk".to_vec());
        let received = rx.recv().await.expect("received live message");
        assert_eq!(received, b"live chunk");
    }

    #[tokio::test]
    async fn test_bounded_buffer_overflow() {
        let mut buffer = BoundedBuffer::new(10);
        buffer.push(b"12345".to_vec());
        buffer.push(b"67890".to_vec());
        buffer.push(b"abcdef".to_vec());

        let snap = buffer.snapshot();
        assert!(snap.len() <= 16); // pops chunks until under capacity
        assert!(snap.ends_with(b"abcdef"));
    }
}
