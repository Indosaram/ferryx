//! Best-effort disk snapshots of terminal ring buffers so a restarted daemon
//! can replay restored sessions' previous output. Persistence errors must
//! never fail the attach/replay path: callers treat any error as "no history".

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const DEFAULT_MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
pub const DEFAULT_MAX_FILES: usize = 256;

/// `session:<uuid>` ids contain `:`, which is unsafe on some filesystems.
/// Keeps `[A-Za-z0-9._-]`, replaces everything else with `_`.
pub fn sanitize_session_id(session_id: &str) -> String {
    let sanitized: String = session_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' })
        .collect();
    let trimmed = sanitized.trim_matches('_');
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

#[derive(Debug, Clone)]
pub struct HistoryStore {
    dir: PathBuf,
}

impl HistoryStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn file_path(&self, session_id: &str) -> PathBuf {
        self.dir
            .join(format!("sess-{}.snap", sanitize_session_id(session_id)))
    }

    pub fn save(&self, session_id: &str, bytes: &[u8]) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let path = self.file_path(session_id);
        let tmp = self.dir.join(format!(
            ".tmp-{}-{}",
            std::process::id(),
            path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "snap".to_string())
        ));
        fs::write(&tmp, bytes)?;
        match fs::rename(&tmp, &path) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                Err(e)
            }
        }
    }

    /// A corrupted snapshot is deleted so it cannot repeatedly fail
    /// resurrection on every daemon start.
    pub fn load(&self, session_id: &str) -> io::Result<Option<Vec<u8>>> {
        let path = self.file_path(session_id);
        if !path.exists() {
            return Ok(None);
        }
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(_) => {
                let _ = fs::remove_file(&path);
                let _ = fs::remove_dir_all(&path);
                Ok(None)
            }
        }
    }

    pub fn delete(&self, session_id: &str) -> io::Result<()> {
        let path = self.file_path(session_id);
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }

    pub fn prune(&self, max_total_bytes: u64, max_files: usize) -> usize {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return 0;
        };
        let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("snap") {
                    return None;
                }
                let meta = entry.metadata().ok()?;
                if !meta.is_file() {
                    return None;
                }
                let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                Some((path, meta.len(), modified))
            })
            .collect();

        let mut removed = 0usize;
        let mut total: u64 = files.iter().map(|(_, len, _)| *len).sum();

        files.sort_by_key(|(_, _, modified)| *modified);
        while (total > max_total_bytes || files.len() > max_files) && !files.is_empty() {
            let (path, len, _) = files.remove(0);
            if fs::remove_file(&path).is_ok() {
                removed += 1;
                total = total.saturating_sub(len);
            }
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ferryx-history-store-test-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sanitizes_session_ids_and_maps_back_consistently() {
        assert_eq!(sanitize_session_id("session:080b8666-2d32-4eb0-a7ef-668869429009"), "session_080b8666-2d32-4eb0-a7ef-668869429009");
        assert_eq!(sanitize_session_id(":::"), "unknown");
        assert_eq!(sanitize_session_id(""), "unknown");
        let a = sanitize_session_id("session:abc");
        let b = sanitize_session_id("session:abc");
        assert_eq!(a, b);
    }

    #[test]
    fn save_load_round_trip_preserves_bytes() {
        let dir = temp_dir("roundtrip");
        let store = HistoryStore::new(dir.clone());
        let bytes = vec![0x1bu8, 0x5b, 0x41, 0x00, 0xff];
        store.save("session:test-roundtrip", &bytes).unwrap();
        let loaded = store.load("session:test-roundtrip").unwrap().expect("snapshot exists");
        assert_eq!(loaded, bytes);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_snapshot_returns_none() {
        let dir = temp_dir("missing");
        let store = HistoryStore::new(dir.clone());
        assert!(store.load("session:never-existed").unwrap().is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupted_snapshot_is_deleted_and_returns_none() {
        let dir = temp_dir("corrupt");
        let store = HistoryStore::new(dir.clone());
        let path = store.file_path("session:corrupt-me");
        // A directory where a snapshot file is expected is unreadable as bytes.
        fs::create_dir_all(&path).unwrap();
        assert!(store.load("session:corrupt-me").unwrap().is_none());
        assert!(!path.exists(), "corrupted snapshot should be removed");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_removes_snapshot() {
        let dir = temp_dir("delete");
        let store = HistoryStore::new(dir.clone());
        store.save("session:doomed", b"payload").unwrap();
        store.delete("session:doomed").unwrap();
        assert!(store.load("session:doomed").unwrap().is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn prune_removes_oldest_beyond_caps() {
        let dir = temp_dir("prune");
        let store = HistoryStore::new(dir.clone());
        for i in 0..5 {
            store.save(&format!("session:prune-{i}"), &[i; 512]).unwrap();
            // Ensure distinct mtimes even on coarse filesystems.
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let removed = store.prune(64 * 1024, 3);
        assert_eq!(removed, 2);
        let remaining = fs::read_dir(&dir).unwrap().filter(|e| e.as_ref().is_ok()).count() - 0;
        assert_eq!(remaining, 3);
        fs::remove_dir_all(&dir).ok();
    }
}
