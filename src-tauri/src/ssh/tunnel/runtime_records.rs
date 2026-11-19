//! Crash-visible ownership of live tunnel children.
//!
//! Reaping must not depend on Rust code running: an `ssh -N -L` child does not
//! die with its parent, so a SIGKILLed daemon leaves a process holding
//! `127.0.0.1:<port>` that no later start can reuse. This module persists one
//! record per runtime generation to `<data>/ssh_tunnel_runtimes.json` and
//! reclaims the records of dead daemons.
//!
//! Two record kinds, and the difference is load-bearing:
//! - a **pid record** (`intent: running`) names a child pid, its start time and
//!   its process group, so it is killable after a start-time check;
//! - an **intent record** (`intent: spawning`) exists for the window between the
//!   decision to spawn and the pid write. It has no identity, so the sweep only
//!   releases its claim and never kills "whoever holds the port".
//!
//! Accepted V1 residual: a crash inside the one syscall between a successful
//! `spawn()` and the pid write leaves a live child the sweep cannot prove is
//! ours. That child keeps the port; the later start fails `port_in_use` and the
//! operator kills the recorded child or changes the port.
//!
//! Liveness is defined, never assumed. The owning daemon (the **owner**, never
//! the ssh child) holds `<data>/tunnel-owned-<instance>.lock` for its whole
//! life. The instance id is minted fresh per process; the lock is opened
//! `O_CLOEXEC` because `ssh -N -L` children inherit open descriptors.
//!
//! There is exactly ONE reap rule, and it is a disjunction:
//! reap a record **iff (its owner's instance lock opens without `O_CREAT` and is
//! free) OR (that lock is `ENOENT` AND the owner identity — `owner_pid` +
//! `owner_start_time` + `owner_boot_id` — is provably dead)**, and never
//! evaluate the sweep's own `owner_instance_id`. The `ENOENT` half exists
//! because macOS `periodic` unlinks files in `/tmp` after three days without
//! atime updates and `flock` never touches atime, so a long-lived daemon's lock
//! file can vanish; reading the fresh inode that `O_CREAT` would create as
//! "free" is how a sweep kills live tunnels.
//!
//! Lock order: definitions sidecar -> registry -> runtimes sidecar. This module
//! only ever takes the runtimes sidecar, non-blocking with a bounded retry, and
//! never while the registry mutex is held across an await.
use super::registry::{Signal, TunnelSpawner};
use super::TunnelRuntimeId;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::{self, Write as _},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Path, PathBuf},
    process::Command,
    sync::{atomic::{AtomicU64, Ordering}, Arc, Mutex},
    time::Duration,
};

pub const RUNTIMES_FILE: &str = "ssh_tunnel_runtimes.json";
pub const RUNTIMES_LOCK: &str = "ssh_tunnel_runtimes.lock";
pub const INSTANCE_LOCK_PREFIX: &str = "tunnel-owned-";

/// The daemon that wrote a record. Not the ssh child: the child's pid and start
/// time are on the record too, and reading the disjunction against the child
/// would leave a live orphan unreaped forever.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerIdentity {
    pub owner_instance_id: String,
    pub owner_pid: u32,
    pub owner_start_time: String,
    pub owner_boot_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordIntent {
    Spawning,
    Running,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRecord {
    pub runtime_id: String,
    pub generation: u64,
    pub intent: RecordIntent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_start_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pgid: Option<i32>,
    pub local_port: u16,
    pub owner: OwnerIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweptRecord {
    pub runtime_id: String,
    pub generation: u64,
    pub pid: Option<u32>,
    pub local_port: u16,
    /// False for an intent record: the claim was released, nothing was signalled.
    pub killed: bool,
}

/// Identity of a process, by start time. Never by name, pattern or port bind.
pub trait ProcessIdentity: Send + Sync {
    /// `Some("")` when the lookup ran and no such process exists; `None` when it
    /// could not be determined, which is read as "not provably dead".
    fn start_time(&self, pid: u32) -> Option<String>;
    fn boot_id(&self) -> String;
}

/// `ps -o lstart=` is the portable read on macOS and Linux. One-second
/// resolution is enough: the record is compared against itself, never against a
/// wall-clock expectation.
pub struct PsProcessIdentity;

impl ProcessIdentity for PsProcessIdentity {
    fn start_time(&self, pid: u32) -> Option<String> {
        let output = match Command::new("ps")
            .args(["-o", "lstart=", "-p", &pid.to_string()])
            .output()
        {
            Ok(output) => output,
            Err(error) => {
                tracing::warn!("Cannot read tunnel child start time for pid {pid}: {error}");
                return None;
            }
        };
        if !output.status.success() {
            // `ps` exits non-zero precisely when the pid does not exist.
            return Some(String::new());
        }
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn boot_id(&self) -> String {
        if let Ok(bytes) = fs::read("/proc/sys/kernel/random/boot_id") {
            return String::from_utf8_lossy(&bytes).trim().to_string();
        }
        match Command::new("sysctl").args(["-n", "kern.boottime"]).output() {
            Ok(output) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            }
            Ok(output) => String::from_utf8_lossy(&output.stderr).trim().to_string(),
            Err(error) => {
                tracing::warn!("Cannot read boot identity: {error}");
                String::new()
            }
        }
    }
}

/// A different boot proves a different process even for a reused pid. Empty on
/// either side means "unknown", and unknown is never death.
fn provably_dead(owner: &OwnerIdentity, source: &dyn ProcessIdentity) -> bool {
    let boot = source.boot_id();
    if !boot.is_empty() && !owner.owner_boot_id.is_empty() && boot != owner.owner_boot_id {
        return true;
    }
    if owner.owner_start_time.is_empty() {
        return false;
    }
    match source.start_time(owner.owner_pid) {
        Some(observed) if observed.is_empty() => true,
        Some(observed) => observed != owner.owner_start_time,
        None => false,
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordsEnvelope {
    version: u64,
    records: HashMap<String, RuntimeRecord>,
}

/// The daemon's own view of the sidecar. One instance per daemon process; the
/// lock file it holds is what makes its records skippable by a successor.
pub struct RuntimeRecords {
    dir: PathBuf,
    path: PathBuf,
    owner: OwnerIdentity,
    source: Arc<dyn ProcessIdentity>,
    /// Held for the process lifetime, so the instance lock stays taken.
    _instance: fs::File,
    gate: Mutex<()>,
    sequence: AtomicU64,
}

impl RuntimeRecords {
    pub fn open(dir: PathBuf, source: Arc<dyn ProcessIdentity>) -> io::Result<Self> {
        let pid = std::process::id();
        let owner = OwnerIdentity {
            owner_instance_id: uuid::Uuid::new_v4().to_string(),
            owner_pid: pid,
            owner_start_time: source.start_time(pid).unwrap_or_default(),
            owner_boot_id: source.boot_id(),
        };
        Self::open_with_owner(dir, source, owner)
    }

    /// Test seam: whether an owner is reachable is decided by comparing its
    /// recorded identity against the live source, and open always records THIS
    /// process, so a case that needs a genuinely dead owner must pin the
    /// identity. Nothing here is reachable from the daemon's own paths.
    #[cfg(all(test, unix))]
    pub fn open_as_owner(
        dir: PathBuf,
        source: Arc<dyn ProcessIdentity>,
        owner: OwnerIdentity,
    ) -> io::Result<Self> {
        Self::open_with_owner(dir, source, owner)
    }

    fn open_with_owner(dir: PathBuf, source: Arc<dyn ProcessIdentity>, owner: OwnerIdentity) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        let instance = instance_lock_path(&dir, &owner.owner_instance_id);
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC)
            .open(&instance)?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            path: dir.join(RUNTIMES_FILE),
            dir,
            owner,
            source,
            _instance: file,
            gate: Mutex::new(()),
            sequence: AtomicU64::new(0),
        })
    }

    pub fn owner(&self) -> &OwnerIdentity {
        &self.owner
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Before `spawn()`: a crash in the window still names the port to reclaim.
    pub fn record_intent(&self, id: &TunnelRuntimeId, generation: u64, local_port: u16) -> io::Result<()> {
        self.mutate(|records| {
            records.insert(
                id.0.clone(),
                RuntimeRecord {
                    runtime_id: id.0.clone(),
                    generation,
                    intent: RecordIntent::Spawning,
                    pid: None,
                    process_start_time: None,
                    pgid: None,
                    local_port,
                    owner: self.owner.clone(),
                },
            );
        })
    }

    /// After `spawn()`, before the reaper exists: the reaper is what makes an
    /// entry live, so a crash before it leaves a killable record.
    pub fn record_pid(
        &self,
        id: &TunnelRuntimeId,
        generation: u64,
        local_port: u16,
        pid: u32,
        pgid: i32,
    ) -> io::Result<()> {
        let start_time = self.source.start_time(pid).unwrap_or_default();
        self.mutate(|records| {
            records.insert(
                id.0.clone(),
                RuntimeRecord {
                    runtime_id: id.0.clone(),
                    generation,
                    intent: RecordIntent::Running,
                    pid: Some(pid),
                    process_start_time: Some(start_time.clone()),
                    pgid: Some(pgid),
                    local_port,
                    owner: self.owner.clone(),
                },
            );
        })
    }

    /// Only after the reaper published `Some(status)`, and only for the exact
    /// `(runtime_id, generation)` the reaper owned: an old generation's late
    /// removal must never delete the live child's newer record.
    pub fn remove(&self, id: &TunnelRuntimeId, generation: u64) -> io::Result<()> {
        self.mutate(|records| {
            if records
                .get(&id.0)
                .is_some_and(|record| record.generation == generation)
            {
                records.remove(&id.0);
            }
        })
    }

    pub fn load(&self) -> io::Result<HashMap<String, RuntimeRecord>> {
        let _gate = self.gate.lock().unwrap_or_else(|error| error.into_inner());
        self.read()
    }

    /// The single reap rule. Returns what it processed, then drops those records.
    pub fn sweep(&self, spawner: &dyn TunnelSpawner) -> Vec<SweptRecord> {
        let mut swept = Vec::new();
        let result = self.mutate(|records| {
            let mut remove = Vec::new();
            for record in records.values() {
                // Never evaluate our own instance: we hold our own lock, and a
                // second rule for "owned by this instance" invites a shared sweep
                // that kills the daemon's own live tunnels on the first tick.
                if record.owner.owner_instance_id == self.owner.owner_instance_id {
                    continue;
                }
                if !self.owner_provably_free(&record.owner) {
                    continue;
                }
                let killed = match (record.pid, record.pgid, record.process_start_time.as_deref()) {
                    (Some(pid), Some(pgid), Some(start))
                        if self.source.start_time(pid).as_deref() == Some(start) =>
                    {
                        spawner.kill_group(pgid, Signal::Kill);
                        true
                    }
                    // No identity, or a pid whose start time no longer matches:
                    // release the claim only. Never kill whoever holds the port.
                    _ => false,
                };
                swept.push(SweptRecord {
                    runtime_id: record.runtime_id.clone(),
                    generation: record.generation,
                    pid: record.pid,
                    local_port: record.local_port,
                    killed,
                });
                remove.push(record.runtime_id.clone());
            }
            for key in &remove {
                records.remove(key);
            }
        });
        if let Err(error) = result {
            tracing::warn!("Cannot sweep tunnel runtime records: {error}");
        }
        if !swept.is_empty() {
            tracing::warn!(
                records = swept.len(),
                "Reclaimed tunnel runtime records of dead daemon instances"
            );
        }
        swept
    }

    /// Shared half of the disjunction, split out so both locks are taken the
    /// same way: no `O_CREAT`, so a vanished lock file is never read as free.
    fn owner_provably_free(&self, owner: &OwnerIdentity) -> bool {
        let lock = instance_lock_path(&self.dir, &owner.owner_instance_id);
        match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_CLOEXEC)
            .open(&lock)
        {
            Ok(file) => {
                if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
                    // Names are instance-unique, so nobody else can be waiting on
                    // this inode; otherwise one stale file accumulates per boot.
                    let _ = fs::remove_file(&lock);
                    true
                } else {
                    false
                }
            }
            // `ENOENT` is decided by the owner-identity half, never by a fresh
            // inode that `O_CREAT` would have handed us.
            Err(error) if error.kind() == io::ErrorKind::NotFound => provably_dead(owner, &*self.source),
            Err(error) => {
                tracing::warn!("Cannot inspect tunnel instance lock {lock:?}: {error}");
                false
            }
        }
    }

    fn mutate<T>(&self, f: impl FnOnce(&mut HashMap<String, RuntimeRecord>) -> T) -> io::Result<T> {
        let _gate = self.gate.lock().unwrap_or_else(|error| error.into_inner());
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC)
            .open(self.dir.join(RUNTIMES_LOCK))?;
        // Non-blocking with a bounded retry: a blocking `flock` here would stall
        // reapers and `shutdown()` on a current-thread runtime.
        let mut acquired = false;
        for _ in 0..200 {
            if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                acquired = true;
                break;
            }
            let error = io::Error::last_os_error();
            match error.raw_os_error() {
                Some(libc::EWOULDBLOCK) | Some(libc::EINTR) => std::thread::sleep(Duration::from_millis(1)),
                _ => return Err(error),
            }
        }
        if !acquired {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "tunnel runtime sidecar lock unavailable",
            ));
        }
        let mut records = self.read()?;
        let out = f(&mut records);
        let written = self.write(&records);
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_UN) };
        written?;
        Ok(out)
    }

    fn read(&self) -> io::Result<HashMap<String, RuntimeRecord>> {
        match fs::read(&self.path) {
            Ok(bytes) => match serde_json::from_slice::<RecordsEnvelope>(&bytes) {
                Ok(envelope) => Ok(envelope.records),
                Err(error) => {
                    tracing::warn!("Corrupt tunnel runtime sidecar {path:?}: {error}", path = self.path);
                    Ok(HashMap::new())
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(HashMap::new()),
            Err(error) => Err(error),
        }
    }

    fn write(&self, records: &HashMap<String, RuntimeRecord>) -> io::Result<()> {
        let envelope = RecordsEnvelope { version: 1, records: records.clone() };
        let bytes = serde_json::to_vec(&envelope)
            .map_err(|error| io::Error::other(error.to_string()))?;
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst);
        let temp = self
            .dir
            .join(format!(".ssh-tunnel-runtimes.{}.{sequence}", std::process::id()));
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_CLOEXEC)
                .open(&temp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temp, &self.path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

impl Drop for RuntimeRecords {
    fn drop(&mut self) {
        // Closing the descriptor releases the instance lock; a successor may then
        // reap whatever we left behind.
        let instance = instance_lock_path(&self.dir, &self.owner.owner_instance_id);
        let _ = fs::remove_file(instance);
    }
}

fn instance_lock_path(dir: &Path, instance_id: &str) -> PathBuf {
    dir.join(format!("{INSTANCE_LOCK_PREFIX}{instance_id}.lock"))
}
