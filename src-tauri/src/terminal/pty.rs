use crate::terminal::{session::PtySessionConfig, PtyError, PtySession};
use parking_lot::{Mutex, RwLock};
use portable_pty::{native_pty_system, CommandBuilder, PtySize, PtySystem};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Clone)]
pub struct PtyManager {
    sessions: Arc<RwLock<HashMap<String, Arc<PtySession>>>>,
    pty_system: Arc<Mutex<Box<dyn PtySystem + Send>>>,
}

impl Default for PtyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            pty_system: Arc::new(Mutex::new(native_pty_system())),
        }
    }

    pub fn spawn(
        &self,
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
    ) -> Result<(String, mpsc::Receiver<Vec<u8>>), PtyError> {
        let session_id = Uuid::new_v4().to_string();
        let rx = self.spawn_with_id(session_id.clone(), cmd, cols, rows)?;
        Ok((session_id, rx))
    }

    pub fn spawn_shell(
        &self,
        cols: u16,
        rows: u16,
    ) -> Result<(String, mpsc::Receiver<Vec<u8>>), PtyError> {
        let cmd = CommandBuilder::new_default_prog();
        self.spawn(cmd, cols, rows)
    }

    pub fn spawn_with_id(
        &self,
        session_id: impl Into<String>,
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
    ) -> Result<mpsc::Receiver<Vec<u8>>, PtyError> {
        let session_id = session_id.into();
        let pty_size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = {
            let pty_system = self.pty_system.lock();
            pty_system
                .openpty(pty_size)
                .map_err(|e| PtyError::PtyCreationError(e.to_string()))?
        };

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| PtyError::IoError(format!("Failed to clone reader: {}", e)))?;

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| PtyError::IoError(format!("Failed to take writer: {}", e)))?;

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| PtyError::SpawnError(format!("Failed to spawn command: {}", e)))?;

        // Drop slave in parent process
        drop(pair.slave);

        let (tx, rx) = mpsc::channel::<Vec<u8>>(1024);

        let session = Arc::new(PtySession::new(PtySessionConfig {
            id: session_id.clone(),
            master: pair.master,
            child,
            writer,
            reader,
            cols,
            rows,
            tx,
        }));

        self.sessions.write().insert(session_id, session);
        Ok(rx)
    }

    pub fn write_input(&self, session_id: &str, data: &[u8]) -> Result<(), PtyError> {
        let session = self.get_session(session_id).ok_or_else(|| {
            PtyError::SessionNotFound(session_id.to_string())
        })?;
        session.write_input(data)
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), PtyError> {
        let session = self.get_session(session_id).ok_or_else(|| {
            PtyError::SessionNotFound(session_id.to_string())
        })?;
        session.resize(cols, rows)
    }

    pub fn kill(&self, session_id: &str) -> Result<(), PtyError> {
        let session = self.get_session(session_id).ok_or_else(|| {
            PtyError::SessionNotFound(session_id.to_string())
        })?;
        session.kill()
    }

    pub fn signal(&self, session_id: &str, sig: i32) -> Result<(), PtyError> {
        let session = self.get_session(session_id).ok_or_else(|| {
            PtyError::SessionNotFound(session_id.to_string())
        })?;
        session.signal(sig)
    }

    pub fn get_session(&self, session_id: &str) -> Option<Arc<PtySession>> {
        self.sessions.read().get(session_id).cloned()
    }

    pub fn has_session(&self, session_id: &str) -> bool {
        self.sessions.read().contains_key(session_id)
    }

    pub fn remove_session(&self, session_id: &str) -> Option<Arc<PtySession>> {
        self.sessions.write().remove(session_id)
    }

    pub fn list_sessions(&self) -> Vec<String> {
        self.sessions.read().keys().cloned().collect()
    }

    pub fn session_count(&self) -> usize {
        self.sessions.read().len()
    }

    pub fn is_alive(&self, session_id: &str) -> Result<bool, PtyError> {
        let session = self.get_session(session_id).ok_or_else(|| {
            PtyError::SessionNotFound(session_id.to_string())
        })?;
        Ok(session.is_alive())
    }
}
