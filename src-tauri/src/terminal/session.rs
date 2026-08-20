use crate::terminal::PtyError;
use parking_lot::Mutex;
use portable_pty::{Child, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

pub struct PtySessionConfig {
    pub id: String,
    pub master: Box<dyn MasterPty + Send>,
    pub child: Box<dyn Child + Send + Sync>,
    pub writer: Box<dyn Write + Send>,
    pub reader: Box<dyn Read + Send>,
    pub cols: u16,
    pub rows: u16,
    pub tx: mpsc::Sender<Vec<u8>>,
}

pub struct PtySession {
    pub id: String,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    reader_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    cols: Arc<Mutex<u16>>,
    rows: Arc<Mutex<u16>>,
}

impl PtySession {
    pub fn new(config: PtySessionConfig) -> Self {
        let tx = config.tx;
        let reader = config.reader;
        let reader_task = tokio::task::spawn_blocking(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        // EOF
                        break;
                    }
                    Ok(n) => {
                        let chunk = buf[..n].to_vec();
                        if tx.blocking_send(chunk).is_err() {
                            // Receiver dropped
                            break;
                        }
                    }
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::Interrupted {
                            continue;
                        }
                        // Broken pipe / EIO on terminal close
                        break;
                    }
                }
            }
        });

        Self {
            id: config.id,
            master: Arc::new(Mutex::new(config.master)),
            writer: Arc::new(Mutex::new(config.writer)),
            child: Arc::new(Mutex::new(config.child)),
            reader_task: Arc::new(Mutex::new(Some(reader_task))),
            cols: Arc::new(Mutex::new(config.cols)),
            rows: Arc::new(Mutex::new(config.rows)),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.lock().process_id()
    }

    pub fn write_input(&self, data: &[u8]) -> Result<(), PtyError> {
        let mut writer = self.writer.lock();
        writer
            .write_all(data)
            .map_err(|e| PtyError::IoError(format!("Write failed: {}", e)))?;
        writer
            .flush()
            .map_err(|e| PtyError::IoError(format!("Flush failed: {}", e)))?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), PtyError> {
        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        self.master
            .lock()
            .resize(size)
            .map_err(|e| PtyError::ResizeError(format!("Resize failed: {}", e)))?;
        *self.cols.lock() = cols;
        *self.rows.lock() = rows;
        Ok(())
    }

    pub fn get_size(&self) -> (u16, u16) {
        (*self.cols.lock(), *self.rows.lock())
    }

    pub fn kill(&self) -> Result<(), PtyError> {
        self.child
            .lock()
            .kill()
            .map_err(|e| PtyError::KillError(format!("Kill failed: {}", e)))?;
        Ok(())
    }

    #[cfg(unix)]
    pub fn signal(&self, sig: i32) -> Result<(), PtyError> {
        if let Some(pid) = self.pid() {
            let res = unsafe { libc::kill(pid as i32, sig) };
            if res != 0 {
                let err = std::io::Error::last_os_error();
                return Err(PtyError::KillError(format!(
                    "Failed to send signal {} to pid {}: {}",
                    sig, pid, err
                )));
            }
            return Ok(());
        }
        Err(PtyError::KillError("PID not available for signal".to_string()))
    }

    #[cfg(not(unix))]
    pub fn signal(&self, _sig: i32) -> Result<(), PtyError> {
        self.kill()
    }

    pub fn try_wait(&self) -> Result<Option<portable_pty::ExitStatus>, PtyError> {
        self.child
            .lock()
            .try_wait()
            .map_err(|e| PtyError::Other(format!("try_wait failed: {}", e)))
    }

    pub fn is_alive(&self) -> bool {
        matches!(self.try_wait(), Ok(None))
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        if let Some(handle) = self.reader_task.lock().take() {
            handle.abort();
        }
    }
}
