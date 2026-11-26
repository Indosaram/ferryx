use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::catalog::STATE_DIR;

pub const POOL_LEASES_DIR: &str = "cloud-pool-leases";
pub const POOL_DIR_ENV: &str = "FERRYX_CLOUD_POOL_DIR";
const LOCK_FILE: &str = ".pool.lock";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolLease {
    pub lease_id: String,
    pub machine: String,
    pub owner_pid: u32,
    pub task_id: Option<String>,
    pub acquired_at: u64,
}

#[derive(Debug, Clone)]
pub struct PoolRegistry {
    root: PathBuf,
}

impl PoolRegistry {
    pub fn default_location() -> Result<Self, String> {
        if let Ok(explicit) = std::env::var(POOL_DIR_ENV) {
            let trimmed = explicit.trim();
            if !trimmed.is_empty() {
                return Ok(Self { root: PathBuf::from(trimmed) });
            }
        }
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .ok_or_else(|| "cannot resolve $HOME for cloud pool leases".to_string())?;
        Ok(Self { root: PathBuf::from(home).join(STATE_DIR).join(POOL_LEASES_DIR) })
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn claim_first_free(
        &self,
        candidates: &[String],
        owner_pid: u32,
        task_id: Option<&str>,
    ) -> Result<Option<PoolLease>, String> {
        let _lock = PoolLock::acquire(&self.root)?;
        self.reap_dead_leases_locked()?;
        let leased = self.active_leased_machines_locked()?;
        for candidate in candidates {
            if !leased.contains(candidate) {
                let lease = self.write_lease_locked(candidate, owner_pid, task_id)?;
                return Ok(Some(lease));
            }
        }
        Ok(None)
    }

    pub fn claim_machine(
        &self,
        machine: &str,
        owner_pid: u32,
        task_id: Option<&str>,
    ) -> Result<PoolLease, String> {
        let _lock = PoolLock::acquire(&self.root)?;
        self.reap_dead_leases_locked()?;
        let path = self.lease_path(machine);
        if path.exists() {
            return Err(format!("cloud machine `{machine}` is already leased"));
        }
        self.write_lease_locked(machine, owner_pid, task_id)
    }

    pub fn release(
        &self,
        machine: &str,
        lease_id: Option<&str>,
        force: bool,
    ) -> Result<bool, String> {
        let _lock = PoolLock::acquire(&self.root)?;
        let path = self.lease_path(machine);
        if !path.exists() {
            return Ok(false);
        }
        if !force {
            if let Some(expected) = lease_id {
                let text = std::fs::read_to_string(&path)
                    .map_err(|error| format!("cannot read lease {}: {error}", path.display()))?;
                let existing: PoolLease = serde_json::from_str(&text)
                    .map_err(|error| format!("corrupt lease {}: {error}", path.display()))?;
                if existing.lease_id != expected {
                    return Err(format!(
                        "lease mismatch for `{machine}`: expected {}, found {}",
                        expected, existing.lease_id
                    ));
                }
            }
        }
        std::fs::remove_file(&path)
            .map_err(|error| format!("cannot remove lease {}: {error}", path.display()))?;
        Ok(true)
    }

    pub fn list_active(&self) -> Result<Vec<PoolLease>, String> {
        let _lock = PoolLock::acquire(&self.root)?;
        self.reap_dead_leases_locked()?;
        let mut out = Vec::new();
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(error) => return Err(format!("cannot read {}: {error}", self.root.display())),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("lease") {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(lease) = serde_json::from_str::<PoolLease>(&text) {
                    out.push(lease);
                }
            }
        }
        out.sort_by(|a, b| a.machine.cmp(&b.machine));
        Ok(out)
    }

    fn lease_path(&self, machine: &str) -> PathBuf {
        self.root.join(format!("{machine}.lease"))
    }

    fn active_leased_machines_locked(&self) -> Result<BTreeSet<String>, String> {
        let mut set = BTreeSet::new();
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(set),
            Err(error) => return Err(format!("cannot read {}: {error}", self.root.display())),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("lease") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                set.insert(stem.to_string());
            }
        }
        Ok(set)
    }

    fn reap_dead_leases_locked(&self) -> Result<(), String> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("cannot read {}: {error}", self.root.display())),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("lease") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(lease) = serde_json::from_str::<PoolLease>(&text) else {
                continue;
            };
            if lease.owner_pid > 0 && !pid_is_alive(lease.owner_pid) {
                let _ = std::fs::remove_file(&path);
            }
        }
        Ok(())
    }

    fn write_lease_locked(
        &self,
        machine: &str,
        owner_pid: u32,
        task_id: Option<&str>,
    ) -> Result<PoolLease, String> {
        let lease = PoolLease {
            lease_id: Uuid::new_v4().to_string(),
            machine: machine.to_string(),
            owner_pid,
            task_id: task_id.map(str::to_string),
            acquired_at: unix_seconds(),
        };
        let bytes = serde_json::to_vec_pretty(&lease)
            .map_err(|error| format!("cannot encode lease: {error}"))?;
        let final_path = self.lease_path(machine);
        let tmp_path = self.root.join(format!(".{machine}.lease.tmp.{}", std::process::id()));
        write_private_bytes(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &final_path).map_err(|error| {
            let _ = std::fs::remove_file(&tmp_path);
            format!("cannot publish lease {}: {error}", final_path.display())
        })?;
        Ok(lease)
    }
}

fn pid_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(raw) = i32::try_from(pid) else {
            return true;
        };
        let rc = unsafe { libc::kill(raw, 0) };
        if rc == 0 {
            return true;
        }
        matches!(std::io::Error::last_os_error().raw_os_error(), Some(code) if code == libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

struct PoolLock {
    #[cfg(unix)]
    file: std::fs::File,
}

impl PoolLock {
    fn acquire(root: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(root)
            .map_err(|error| format!("cannot create {}: {error}", root.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            use std::os::unix::io::AsRawFd;
            let _ = std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700));
            let lock_path = root.join(LOCK_FILE);
            let file = open_private_rw(&lock_path)?;
            loop {
                if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
                    break;
                }
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::Interrupted {
                    return Err(format!("cannot lock {}: {error}", lock_path.display()));
                }
            }
            Ok(Self { file })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {})
        }
    }
}

#[cfg(unix)]
impl Drop for PoolLock {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

#[cfg(unix)]
fn open_private_rw(path: &Path) -> Result<std::fs::File, String> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))
}

fn write_private_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts
        .open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    file.write_all(bytes)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    file.sync_all()
        .map_err(|error| format!("cannot sync {}: {error}", path.display()))?;
    Ok(())
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_skips_leased_machine_and_reuses_after_release() {
        let dir = tempfile::tempdir().unwrap();
        let pool = PoolRegistry::at(dir.path());
        let candidates = vec!["box-a".to_string(), "box-b".to_string()];
        let pid = std::process::id();

        let l1 = pool.claim_first_free(&candidates, pid, Some("st_1")).unwrap().unwrap();
        assert_eq!(l1.machine, "box-a");

        let l2 = pool.claim_first_free(&candidates, pid, Some("st_2")).unwrap().unwrap();
        assert_eq!(l2.machine, "box-b");

        assert!(pool.claim_first_free(&candidates, pid, Some("st_3")).unwrap().is_none());

        assert!(pool.release("box-a", Some("wrong-id"), false).is_err());
        assert!(pool.release("box-a", Some(&l1.lease_id), false).unwrap());

        let l3 = pool.claim_first_free(&candidates, pid, Some("st_3")).unwrap().unwrap();
        assert_eq!(l3.machine, "box-a");
    }

    #[cfg(unix)]
    #[test]
    fn dead_owner_lease_is_automatically_reaped() {
        let dir = tempfile::tempdir().unwrap();
        let pool = PoolRegistry::at(dir.path());
        let _ = pool.claim_machine("box-dead", 999_999, Some("st_dead")).unwrap();

        let reused = pool
            .claim_first_free(&["box-dead".to_string()], std::process::id(), Some("st_new"))
            .unwrap()
            .unwrap();
        assert_eq!(reused.machine, "box-dead");
        assert_eq!(reused.task_id.as_deref(), Some("st_new"));
    }
}
