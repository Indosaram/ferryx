//! Background rescan that surfaces externally created, removed, or moved
//! worktrees in the project list.
//!
//! In-app CRUD already emits `worktree_changed` from the create/remove
//! commands, but worktrees made by running agents or other processes only
//! appeared after the user opened the project again. This module closes the
//! gap the same way Orca's `scanAdmin` does: a periodic sweep per registered
//! git workspace, gated by a cheap, subprocess-free fingerprint of git's
//! worktree admin state (`$GIT_COMMON_DIR/worktrees`, HEADs, loose refs,
//! packed-refs) so unchanged repositories cost no `git` subprocesses. Any
//! sweep the fingerprint cannot prove unchanged falls back to a real
//! `list_worktrees()` scan; the diff against the previously observed list is
//! emitted as the same `worktree_changed` events CRUD already sends.
//!
//! Scope note: detection lists whatever `git worktree list` reports for the
//! registered repository root (main checkout + linked worktrees). Sibling
//! worktrees of OTHER checkouts outside the root are intentionally not part
//! of a workspace, consistent with the path jail.

use crate::ipc::run_blocking;
use crate::ipc::worktree::{emit_worktree_changed, WorktreeChangeKind};
use crate::worktree::registry::WorkspaceRegistry;
use crate::worktree::{Worktree, WorktreeIdentity, WorktreeManager};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Sweep cadence: the maximum discovery latency for externally created,
/// removed, or moved worktrees (mirrors Orca's 30s scan TTL contract).
pub const WORKTREE_RESCAN_INTERVAL: Duration = Duration::from_secs(30);

/// Bounded reconciliation: force a real scan this often even when the
/// fingerprint proves nothing changed, so anything the probe cannot observe
/// still converges (mirrors Orca's
/// `WORKTREE_SCAN_ADMIN_RECONCILE_INTERVAL_MS`).
const RECONCILE_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Budget for one fingerprint probe. The probe only reads a handful of small
/// admin files; exceeding this means the filesystem is wedged.
const FINGERPRINT_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Workspace ID assigned to event identities whose branch does not follow the
/// managed `orca/<ws-id>/<slug>` namespace (external worktrees created by
/// other tools).
const DETECTED_WORKSPACE_ID: &str = "detected";

/// Component separator for the fingerprint string. Git ref names and file
/// paths cannot contain NUL, so content can never be confused with framing.
const FINGERPRINT_SEP: char = '\u{0}';

/// One externally-detected worktree change ready to be emitted as
/// `worktree_changed` (same payload shape the CRUD commands emit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedWorktreeChange {
    pub workspace_id: String,
    pub identity: WorktreeIdentity,
    pub kind: WorktreeChangeKind,
}

#[derive(Debug, Default)]
pub struct WorktreeRescanCache {
    entries: Mutex<HashMap<String, RescanEntry>>,
    /// Workspaces whose probe wedged at least once. Permanent until the app
    /// restarts: these never probe again and always reconcile with a real
    /// scan. Tracked independently of the baseline entry so a wedged first
    /// sweep still takes effect even when that sweep's real scan also fails.
    wedged: Mutex<HashSet<String>>,
}

#[derive(Debug)]
struct RescanEntry {
    /// Admin fingerprint captured immediately before `worktrees` was scanned.
    /// `None` means the last probe failed; the gate then always opens.
    fingerprint: Option<String>,
    worktrees: Vec<Worktree>,
    /// Time of the last REAL scan. The reconcile bound is measured from this
    /// and ONLY real scans update it — gate extensions must never touch it,
    /// or 30s extensions postpone reconciliation forever.
    last_scan: Instant,
}

enum ProbeOutcome {
    /// The probe finished; `None` means an unexpected admin-store read error.
    Done(Option<String>),
    /// The probe did not finish within the budget (wedged mount). Mirrors
    /// Orca's in-flight probe guard: the gate stays disabled for the
    /// workspace and every sweep reconciles with a real scan.
    TimedOut,
}

/// Reads a cheap, subprocess-free fingerprint of the repository's worktree
/// admin state. It only reads files inside the git directory tree:
///
/// - `<common>/worktrees` entry names (creation, removal, prune)
/// - per entry: HEAD contents, `gitdir` target content and existence,
///   `locked` presence, and the checkout's branch tip via the loose ref
/// - main checkout HEAD + branch tip, plus the HEAD of the checkout the
///   registered root points at when it is a linked worktree
/// - existence of the registered root itself
/// - `packed-refs` / reftable mtime+size (branch tips moved while refs are
///   packed away)
///
/// `None` means "cannot prove unchanged" (not a git checkout, unexpected IO
/// error); the sweep then always performs a real scan. Failing open is the
/// correct direction: a redundant scan costs one `git` subprocess, a missed
/// change costs a stale project list.
pub fn read_worktree_admin_fingerprint(repo_root: &Path) -> Option<String> {
    let git_dir = resolve_git_dir(repo_root)?;
    let common_dir = resolve_common_dir(&git_dir)?;
    let mut parts: Vec<String> = Vec::new();

    // Linked worktree admin entries. A linked worktree's HEAD lives in its
    // admin entry, so checkout/branch switches inside that worktree are
    // observed here; its branch tip is a loose ref under the common store.
    let worktrees_dir = common_dir.join("worktrees");
    match std::fs::read_dir(&worktrees_dir) {
        Ok(entries) => {
            let mut names: Vec<String> = entries
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| {
                    if entry.file_type().map(|file_type| file_type.is_dir()).unwrap_or(false) {
                        entry.file_name().into_string().ok()
                    } else {
                        None
                    }
                })
                .collect();
            names.sort();
            parts.push(format!("entries:{}", names.join(&FINGERPRINT_SEP.to_string())));
            for name in &names {
                let entry_dir = worktrees_dir.join(name);
                let head = read_file_stable(&entry_dir.join("HEAD"))?.trim().to_string();
                parts.push(format!("head:{name}:{head}"));
                let gitdir = read_file_stable(&entry_dir.join("gitdir"))?
                    .trim()
                    .to_string();
                parts.push(format!("gitdir:{name}:{gitdir}"));
                let checkout_exists = if gitdir.is_empty() {
                    false
                } else {
                    PathBuf::from(&gitdir).exists()
                };
                parts.push(format!("checkout-exists:{name}:{checkout_exists}"));
                parts.push(format!(
                    "locked:{name}:{}",
                    entry_dir.join("locked").exists()
                ));
                if !head.is_empty() {
                    let tip = ref_fingerprint(&common_dir, &head)?;
                    parts.push(format!("ref:{name}:{tip}"));
                }
            }
        }
        // No linked worktrees yet is a stable recorded value.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            parts.push("entries:none".to_string());
        }
        Err(_) => return None,
    }

    // Main checkout HEAD and its branch tip.
    let main_head = read_file_stable(&common_dir.join("HEAD"))?;
    parts.push(format!("main-head:{}", main_head.trim()));
    if !main_head.is_empty() {
        let tip = ref_fingerprint(&common_dir, &main_head)?;
        parts.push(format!("main-ref:{tip}"));
    }

    // HEAD of the checkout the registered root points at, when that checkout
    // is itself a linked worktree rather than the main checkout. (Identical
    // to `main-head` for the common case, so it is only recorded when the
    // paths differ.)
    if git_dir != common_dir {
        let root_head = read_file_stable(&git_dir.join("HEAD"))?;
        parts.push(format!("root-head:{}", root_head.trim()));
        if !root_head.is_empty() {
            let tip = ref_fingerprint(&common_dir, &root_head)?;
            parts.push(format!("root-ref:{tip}"));
        }
    }

    // Presence of the registered checkout itself (removed/moved project).
    parts.push(format!("root-exists:{}", repo_root.exists()));

    // Packed ref storage: branch tips moved while loose refs are packed away.
    parts.push(store_entry_fingerprint(&common_dir.join("packed-refs"), "packed-refs")?);
    parts.push(store_entry_fingerprint(&common_dir.join("reftable"), "reftable")?);

    Some(parts.join(&FINGERPRINT_SEP.to_string()))
}

/// Runs the fingerprint probe on a dedicated thread with a hard deadline so a
/// wedged mount cannot stall the sweep (or leak unbounded threads).
fn probe_fingerprint(repo_root: &Path) -> ProbeOutcome {
    let (sender, receiver) = std::sync::mpsc::channel::<Option<String>>();
    let root = repo_root.to_path_buf();
    let worker = std::thread::Builder::new()
        .name("worktree-fingerprint".to_string())
        .spawn(move || {
            let _ = sender.send(read_worktree_admin_fingerprint(&root));
        });
    if worker.is_err() {
        return ProbeOutcome::Done(None);
    }
    match receiver.recv_timeout(FINGERPRINT_PROBE_TIMEOUT) {
        Ok(result) => ProbeOutcome::Done(result),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => ProbeOutcome::TimedOut,
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => ProbeOutcome::Done(None),
    }
}

fn read_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)?;
    Ok(String::from_utf8_lossy(&contents).into_owned())
}

/// Reads a file whose absence is a meaningful, stable value; unexpected read
/// failures yield `None` and fail the whole probe.
fn read_file_stable(path: &Path) -> Option<String> {
    match read_file(path) {
        Ok(contents) => Some(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(String::new()),
        Err(_) => None,
    }
}

fn resolve_git_dir(repo_root: &Path) -> Option<PathBuf> {
    let dot_git = repo_root.join(".git");
    match std::fs::metadata(&dot_git) {
        Ok(meta) if meta.is_dir() => Some(dot_git),
        Ok(_) => {
            // `.git` file (`gitdir: <path>`): repo_root is itself a linked
            // worktree of some other repository.
            let contents = read_file(&dot_git).ok()?;
            let target = contents.lines().next()?.trim();
            let target = target.strip_prefix("gitdir:")?.trim();
            let path = PathBuf::from(target);
            Some(if path.is_absolute() { path } else { repo_root.join(path) })
        }
        Err(_) => None,
    }
}

/// Resolves the common git dir of a checkout. The main checkout's admin dir
/// is the common dir itself; linked worktrees point back via `commondir`.
fn resolve_common_dir(git_dir: &Path) -> Option<PathBuf> {
    match read_file_stable(&git_dir.join("commondir")) {
        None => None,
        Some(contents) if !contents.is_empty() => {
            let target = PathBuf::from(contents.trim());
            Some(if target.is_absolute() {
                target
            } else {
                git_dir.join(target)
            })
        }
        Some(_) => Some(git_dir.to_path_buf()),
    }
}

/// Resolves a HEAD value (`ref: <name>` or an oid) to its fingerprint: the
/// loose ref's content, or a stable marker when the ref is packed away
/// (packed-refs/reftable metadata covers moves of the packed copy). Only
/// relative POSIX refs under `refs/` are followed, so a hand-edited HEAD
/// cannot steer the probe outside the ref store — backslashes, colons, and
/// absolute paths are rejected too because the Win32 filesystem layer would
/// otherwise honor them as separators/drive specs. Symlinked refs are out
/// of scope: the admin store is git-owned and git itself never creates
/// them; anyone able to plant one already owns the repository.
fn ref_fingerprint(common_dir: &Path, head_value: &str) -> Option<String> {
    let reference = match head_value.strip_prefix("ref:") {
        Some(reference) => reference.trim(),
        None => return Some(head_value.trim().to_string()),
    };
    let escapes_store = !reference.starts_with("refs/")
        || reference.split('/').any(|part| part == "..")
        || reference.contains('\\')
        || reference.contains(':')
        || reference.starts_with('/');
    if escapes_store {
        return Some(format!("unfollowed:{reference}"));
    }
    match read_file(&common_dir.join(reference)) {
        Ok(content) => Some(content.trim().to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Some(format!("packed:{reference}"))
        }
        Err(_) => None,
    }
}

fn store_entry_fingerprint(path: &Path, label: &str) -> Option<String> {
    match std::fs::metadata(path) {
        Ok(meta) => Some(format!(
            "{label}:{:?}:{}",
            meta.modified().ok(),
            meta.len()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Some(format!("{label}:absent"))
        }
        Err(_) => None,
    }
}

/// Diffs two real-scan snapshots into `worktree_changed` events, keyed by
/// checkout path.
fn diff_worktrees(
    old: &[Worktree],
    new: &[Worktree],
    workspace_id: &str,
) -> Vec<DetectedWorktreeChange> {
    let old_by_path: HashMap<&Path, &Worktree> = old
        .iter()
        .map(|worktree| (worktree.path.as_path(), worktree))
        .collect();
    let new_by_path: HashMap<&Path, &Worktree> = new
        .iter()
        .map(|worktree| (worktree.path.as_path(), worktree))
        .collect();

    let mut changes = Vec::new();
    for worktree in new {
        if !old_by_path.contains_key(worktree.path.as_path()) {
            changes.push(DetectedWorktreeChange {
                workspace_id: workspace_id.to_string(),
                identity: detected_identity(worktree),
                kind: WorktreeChangeKind::Created,
            });
        }
    }
    for worktree in old {
        if !new_by_path.contains_key(worktree.path.as_path()) {
            changes.push(DetectedWorktreeChange {
                workspace_id: workspace_id.to_string(),
                identity: detected_identity(worktree),
                kind: WorktreeChangeKind::Pruned,
            });
        }
    }
    for worktree in new {
        if let Some(previous) = old_by_path.get(worktree.path.as_path()) {
            if !worktree_equivalent(previous, worktree) {
                changes.push(DetectedWorktreeChange {
                    workspace_id: workspace_id.to_string(),
                    identity: detected_identity(worktree),
                    kind: WorktreeChangeKind::Updated,
                });
            }
        }
    }
    changes
}

fn worktree_equivalent(previous: &Worktree, current: &Worktree) -> bool {
    previous.head == current.head
        && previous.branch == current.branch
        && previous.bare == current.bare
        && previous.detached == current.detached
        && previous.locked == current.locked
        && previous.prunable == current.prunable
}

/// Identity for events about externally managed worktrees: parses the managed
/// `orca/<ws-id>/<slug>` branch namespace when possible, else falls back to a
/// stable `detected` workspace with the checkout's directory name.
fn detected_identity(worktree: &Worktree) -> WorktreeIdentity {
    if let Some(info) = worktree.orca_info() {
        return WorktreeIdentity {
            ws_id: info.ws_id,
            slug: info.slug,
        };
    }
    WorktreeIdentity {
        ws_id: DETECTED_WORKSPACE_ID.to_string(),
        slug: worktree
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "root".to_string()),
    }
}

/// Runs one sweep over every registered git workspace and returns the
/// detected changes. The first observation of a workspace is a silent
/// baseline: existing worktrees are recorded without emitting events, so
/// startup cannot flood the UI with events for worktrees that already
/// existed.
pub fn sweep_registry_once(
    cache: &WorktreeRescanCache,
    registry: &WorkspaceRegistry,
    now: Instant,
) -> Vec<DetectedWorktreeChange> {
    sweep_with_probe(cache, registry, now, probe_fingerprint)
}

fn sweep_with_probe<F>(
    cache: &WorktreeRescanCache,
    registry: &WorkspaceRegistry,
    now: Instant,
    probe: F,
) -> Vec<DetectedWorktreeChange>
where
    F: Fn(&Path) -> ProbeOutcome,
{
    let mut changes = Vec::new();
    for (workspace_id, manager) in registry.list() {
        changes.extend(sweep_workspace_with_probe(
            cache,
            &workspace_id,
            &manager,
            now,
            &probe,
        ));
    }
    changes
}

/// Sweeps one workspace: gate by fingerprint, else real scan + diff. All
/// filesystem and git-subprocess work happens here with no cache lock held.
fn sweep_workspace_with_probe<F>(
    cache: &WorktreeRescanCache,
    workspace_id: &str,
    manager: &WorktreeManager,
    now: Instant,
    probe: &F,
) -> Vec<DetectedWorktreeChange>
where
    F: Fn(&Path) -> ProbeOutcome,
{
    if !manager.is_git_backed() {
        return Vec::new();
    }
    let repo_root = manager.repo_root().to_path_buf();
    let prior_worktrees = {
        let entries = cache.entries.lock();
        entries.get(workspace_id).map(|entry| entry.worktrees.clone())
    };

    // Read the cached gate state without holding the lock across IO.
    let (stored_fingerprint, reconcile_due) = {
        let entries = cache.entries.lock();
        match entries.get(workspace_id) {
            Some(entry) => (
                entry.fingerprint.clone(),
                now.duration_since(entry.last_scan) >= RECONCILE_INTERVAL,
            ),
            None => (None, false),
        }
    };
    let probe_wedged = cache.wedged.lock().contains(workspace_id);

    let captured = if probe_wedged {
        None
    } else {
        match probe(&repo_root) {
            ProbeOutcome::TimedOut => {
                // Persist the wedge before anything else so a failed scan
                // cannot re-arm the gate: wedged workspaces never probe
                // again; every sweep reconciles with a real scan.
                cache.wedged.lock().insert(workspace_id.to_string());
                None
            }
            ProbeOutcome::Done(fingerprint) => fingerprint,
        }
    };

    // Gate: skip the real scan when the fresh probe proves the admin state
    // unchanged and the last real scan is still fresh. Gate extensions must
    // NOT update `last_scan` — that timestamp belongs to real scans only,
    // or 30s extensions postpone reconciliation forever.
    if let Some(fingerprint) = &captured {
        if !reconcile_due && stored_fingerprint.as_deref() == Some(fingerprint.as_str()) {
            return Vec::new();
        }
    }

    let scanned = match manager.list_worktrees() {
        Ok(worktrees) => worktrees,
        Err(error) => {
            tracing::warn!(
                workspace_id = %workspace_id,
                error = %error,
                "worktree rescan failed"
            );
            return Vec::new();
        }
    };

    let mut changes = Vec::new();
    if let Some(prior_worktrees) = prior_worktrees {
        changes.extend(diff_worktrees(&prior_worktrees, &scanned, workspace_id));
    }

    let mut entries = cache.entries.lock();
    match entries.get_mut(workspace_id) {
        Some(entry) => {
            entry.fingerprint = captured;
            entry.worktrees = scanned;
            entry.last_scan = now;
        }
        None => {
            entries.insert(
                workspace_id.to_string(),
                RescanEntry {
                    fingerprint: captured,
                    worktrees: scanned,
                    last_scan: now,
                },
            );
        }
    }
    changes
}

/// Per-workspace scan budget. A wedged filesystem can make `list_worktrees`
/// hang forever; bounding each workspace keeps one bad mount from stalling
/// every other workspace's sweep (and from withholding already-detected
/// events). Generous relative to git subprocess cost; overflow is absorbed
/// by `MissedTickBehavior::Delay`.
const WORKSPACE_SCAN_TIMEOUT: Duration = Duration::from_secs(30);

/// Spawns the GUI-process rescan task: every 30s, sweep all registered git
/// workspaces and emit `worktree_changed` for externally changed worktrees.
/// Each workspace is scanned in its own bounded `run_blocking` job and its
/// events are emitted immediately, so a hung scan for one workspace delays
/// (bounded by the timeout) but never blocks the others. The interval's
/// first tick establishes the baseline silently.
pub fn spawn_worktree_rescan_task<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    registry: WorkspaceRegistry,
) {
    tauri::async_runtime::spawn(async move {
        let cache = Arc::new(WorktreeRescanCache::default());
        let mut ticker = tokio::time::interval(WORKTREE_RESCAN_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let now = Instant::now();
            for (workspace_id, manager) in registry.list() {
                let workspace_cache = Arc::clone(&cache);
                // Owned copy for the blocking closure; the original stays for
                // logging in the match arms below.
                let closure_workspace_id = workspace_id.clone();
                let scan = tokio::time::timeout(
                    WORKSPACE_SCAN_TIMEOUT,
                    run_blocking(move || {
                        Ok(sweep_workspace_with_probe(
                            &workspace_cache,
                            &closure_workspace_id,
                            &manager,
                            now,
                            &probe_fingerprint,
                        ))
                    }),
                )
                .await;
                match scan {
                    Ok(Ok(changes)) => {
                        for change in changes {
                            tracing::debug!(
                                workspace_id = %change.workspace_id,
                                ws_id = %change.identity.ws_id,
                                slug = %change.identity.slug,
                                kind = ?change.kind,
                                "worktree rescan detected an external change"
                            );
                            if let Err(error) = emit_worktree_changed(
                                &app,
                                change.workspace_id,
                                change.identity,
                                change.kind,
                            ) {
                                tracing::warn!(
                                    error = %error,
                                    "failed to emit worktree rescan change"
                                );
                            }
                        }
                    }
                    Ok(Err(error)) => {
                        tracing::warn!(
                            workspace_id = %workspace_id,
                            error = %error,
                            "worktree rescan sweep failed"
                        );
                    }
                    Err(_elapsed) => {
                        tracing::warn!(
                            workspace_id = %workspace_id,
                            "worktree rescan scan exceeded its budget; skipping this tick"
                        );
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::git;
    use crate::worktree::tests::setup_test_repo;
    use crate::worktree::{CreateWorktreeOptions, WorktreeManager};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn register_git_workspace() -> (tempfile::TempDir, WorktreeManager, WorkspaceRegistry) {
        let (temp, manager) = setup_test_repo();
        let registry = WorkspaceRegistry::new();
        registry.register("ws-a", manager.repo_root()).unwrap();
        (temp, manager, registry)
    }

    fn worktree_from(path: &Path, head: &str, branch: Option<&str>) -> Worktree {
        Worktree {
            path: path.to_path_buf(),
            head: head.to_string(),
            branch: branch.map(|b| b.to_string()),
            bare: false,
            detached: false,
            locked: None,
            prunable: None,
        }
    }

    #[test]
    fn fingerprint_is_stable_and_tracks_worktree_admin_changes() {
        let (_temp, manager) = setup_test_repo();
        let root = manager.repo_root().to_path_buf();

        let before = read_worktree_admin_fingerprint(&root).expect("git repo has a fingerprint");
        let again = read_worktree_admin_fingerprint(&root).expect("git repo has a fingerprint");
        assert_eq!(before, again, "unchanged repo must not change fingerprint");

        let wt_path = manager.worktree_path_for("ws-a", "task-1").unwrap();
        manager
            .create_worktree(CreateWorktreeOptions::new("ws-a", "task-1", &wt_path))
            .unwrap();
        let with_wt = read_worktree_admin_fingerprint(&root).expect("fingerprint");
        assert_ne!(before, with_wt, "worktree add must change fingerprint");

        std::fs::write(wt_path.join("file.txt"), "x").unwrap();
        git::run_git(&wt_path, &["add", "."]).unwrap();
        git::run_git(&wt_path, &["commit", "-m", "wip"]).unwrap();
        let after_commit = read_worktree_admin_fingerprint(&root).expect("fingerprint");
        assert_ne!(with_wt, after_commit, "branch tip movement must change fingerprint");

        git::run_git(&wt_path, &["checkout", "-b", "renamed-task"]).unwrap();
        let after_switch = read_worktree_admin_fingerprint(&root).expect("fingerprint");
        assert_ne!(after_commit, after_switch, "checkout must change fingerprint");

        git::run_git(&root, &["worktree", "lock", wt_path.to_str().unwrap()])
            .unwrap();
        let after_lock = read_worktree_admin_fingerprint(&root).expect("fingerprint");
        assert_ne!(after_switch, after_lock, "locking must change fingerprint");

        git::run_git(&root, &["worktree", "unlock", wt_path.to_str().unwrap()])
            .unwrap();
        let after_unlock = read_worktree_admin_fingerprint(&root).expect("fingerprint");
        assert_ne!(after_lock, after_unlock, "unlocking must change fingerprint");
    }

    #[test]
    fn fingerprint_probes_from_linked_worktree_root_are_stable_and_track_changes() {
        let (_temp, manager) = setup_test_repo();
        let root = manager.repo_root().to_path_buf();
        let wt_path = manager.worktree_path_for("ws-a", "linked").unwrap();
        manager
            .create_worktree(CreateWorktreeOptions::new("ws-a", "linked", &wt_path))
            .unwrap();

        let before = read_worktree_admin_fingerprint(&wt_path).expect("linked root fingerprint");
        let again = read_worktree_admin_fingerprint(&wt_path).expect("linked root fingerprint");
        assert_eq!(before, again);

        let other = manager.worktree_path_for("ws-a", "task-2").unwrap();
        manager
            .create_worktree(CreateWorktreeOptions::new("ws-a", "task-2", &other))
            .unwrap();
        let after = read_worktree_admin_fingerprint(&wt_path).expect("linked root fingerprint");
        assert_ne!(before, after, "admin store growth must be visible from a linked root");

        manager.safe_delete(&other).unwrap();
        let after_delete = read_worktree_admin_fingerprint(&wt_path).expect("linked root fingerprint");
        assert_ne!(after, after_delete, "admin store shrink must be visible from a linked root");

        let _ = root;
    }

    #[test]
    fn fingerprint_is_none_for_non_git_directories() {
        let plain = tempfile::TempDir::new().unwrap();
        assert_eq!(read_worktree_admin_fingerprint(plain.path()), None);
        assert_eq!(read_worktree_admin_fingerprint(&plain.path().join("missing")), None);
    }

    #[test]
    fn diff_worktrees_reports_created_removed_updated_and_external_identities() {
        let kept = worktree_from(Path::new("/repo/wt-kept"), "aaa", Some("refs/heads/orca/ws/kept"));
        let removed = worktree_from(Path::new("/repo/wt-removed"), "bbb", Some("refs/heads/orca/ws/removed"));
        let updated_old = worktree_from(Path::new("/repo/wt-moved"), "ccc", Some("refs/heads/orca/ws/moved"));
        let updated_new = worktree_from(Path::new("/repo/wt-moved"), "ddd", Some("refs/heads/orca/ws/moved"));
        let created = worktree_from(Path::new("/repo/wt-created"), "eee", Some("refs/heads/orca/ws/created"));
        let external = worktree_from(Path::new("/repo/wt-ext"), "fff", Some("refs/heads/feature-x"));
        let external_new = worktree_from(Path::new("/repo/wt-ext"), "ggg", Some("refs/heads/feature-x"));

        let old = vec![kept.clone(), removed.clone(), updated_old, external.clone()];
        let new = vec![kept, updated_new, created, external_new];
        let changes = diff_worktrees(&old, &new, "ws-a");

        assert!(changes.contains(&DetectedWorktreeChange {
            workspace_id: "ws-a".into(),
            identity: WorktreeIdentity { ws_id: "ws".into(), slug: "removed".into() },
            kind: WorktreeChangeKind::Pruned,
        }));
        assert!(changes.contains(&DetectedWorktreeChange {
            workspace_id: "ws-a".into(),
            identity: WorktreeIdentity { ws_id: "ws".into(), slug: "created".into() },
            kind: WorktreeChangeKind::Created,
        }));
        assert!(changes.contains(&DetectedWorktreeChange {
            workspace_id: "ws-a".into(),
            identity: WorktreeIdentity { ws_id: "ws".into(), slug: "moved".into() },
            kind: WorktreeChangeKind::Updated,
        }));
        assert!(changes.contains(&DetectedWorktreeChange {
            workspace_id: "ws-a".into(),
            identity: WorktreeIdentity { ws_id: DETECTED_WORKSPACE_ID.into(), slug: "wt-ext".into() },
            kind: WorktreeChangeKind::Updated,
        }));
        assert_eq!(changes.len(), 4);
    }

    #[test]
    fn sweep_baseline_is_silent_then_emits_created_and_settles() {
        let (_temp, manager, registry) = register_git_workspace();
        let cache = WorktreeRescanCache::default();
        let now = Instant::now();

        let plain = tempfile::TempDir::new().unwrap();
        registry.register("plain", plain.path()).unwrap();

        let changes = sweep_registry_once(&cache, &registry, now);
        assert!(changes.is_empty(), "baseline sweep must not emit events");
        {
            let entries = cache.entries.lock();
            assert!(entries.contains_key("ws-a"));
            assert!(!entries.contains_key("plain"), "non-git workspaces are never scanned");
        }

        let wt_path = manager.worktree_path_for("ws-a", "ext-task").unwrap();
        manager
            .create_worktree(CreateWorktreeOptions::new("ws-a", "ext-task", &wt_path))
            .unwrap();

        let changes = sweep_registry_once(&cache, &registry, now + Duration::from_secs(30));
        assert_eq!(
            changes,
            vec![DetectedWorktreeChange {
                workspace_id: "ws-a".into(),
                identity: WorktreeIdentity { ws_id: "ws-a".into(), slug: "ext-task".into() },
                kind: WorktreeChangeKind::Created,
            }]
        );

        let changes = sweep_registry_once(&cache, &registry, now + Duration::from_secs(60));
        assert!(changes.is_empty(), "steady state must not re-emit");
    }

    #[test]
    fn sweep_emits_updated_when_a_worktree_head_moves() {
        let (_temp, manager, registry) = register_git_workspace();
        let wt_path = manager.worktree_path_for("ws-a", "moving").unwrap();
        manager
            .create_worktree(CreateWorktreeOptions::new("ws-a", "moving", &wt_path))
            .unwrap();

        let cache = WorktreeRescanCache::default();
        let now = Instant::now();
        assert!(sweep_registry_once(&cache, &registry, now).is_empty());

        std::fs::write(wt_path.join("file.txt"), "data").unwrap();
        git::run_git(&wt_path, &["add", "."]).unwrap();
        git::run_git(&wt_path, &["commit", "-m", "move head"]).unwrap();

        let changes = sweep_registry_once(&cache, &registry, now + Duration::from_secs(30));
        assert_eq!(
            changes,
            vec![DetectedWorktreeChange {
                workspace_id: "ws-a".into(),
                identity: WorktreeIdentity { ws_id: "ws-a".into(), slug: "moving".into() },
                kind: WorktreeChangeKind::Updated,
            }]
        );
    }

    #[test]
    fn sweep_emits_pruned_when_a_worktree_disappears() {
        let (_temp, manager, registry) = register_git_workspace();
        let wt_path = manager.worktree_path_for("ws-a", "doomed").unwrap();
        manager
            .create_worktree(CreateWorktreeOptions::new("ws-a", "doomed", &wt_path))
            .unwrap();

        let cache = WorktreeRescanCache::default();
        let now = Instant::now();
        assert!(sweep_registry_once(&cache, &registry, now).is_empty());

        manager.safe_delete(&wt_path).unwrap();

        let changes = sweep_registry_once(&cache, &registry, now + Duration::from_secs(30));
        assert_eq!(
            changes,
            vec![DetectedWorktreeChange {
                workspace_id: "ws-a".into(),
                identity: WorktreeIdentity { ws_id: "ws-a".into(), slug: "doomed".into() },
                kind: WorktreeChangeKind::Pruned,
            }]
        );
    }

    #[test]
    fn equal_fingerprint_suppresses_scans_until_reconcile_interval() {
        let (_temp, manager, registry) = register_git_workspace();
        let wt_path = manager.worktree_path_for("ws-a", "pre-existing").unwrap();
        manager
            .create_worktree(CreateWorktreeOptions::new("ws-a", "pre-existing", &wt_path))
            .unwrap();

        let cache = WorktreeRescanCache::default();
        let now = Instant::now();
        let probe_calls = Arc::new(AtomicUsize::new(0));
        let probe = {
            let calls = Arc::clone(&probe_calls);
            move |_root: &Path| {
                calls.fetch_add(1, Ordering::SeqCst);
                ProbeOutcome::Done(Some("static-claim-unchanged".to_string()))
            }
        };

        assert!(sweep_with_probe(&cache, &registry, now, &probe).is_empty());
        assert_eq!(probe_calls.load(Ordering::SeqCst), 1, "baseline always scans");

        // The probe lies (claims unchanged) while the real admin state changes;
        // the gate must trust it and skip the scan.
        manager.safe_delete(&wt_path).unwrap();
        let changes = sweep_with_probe(&cache, &registry, now + Duration::from_secs(30), &probe);
        assert!(changes.is_empty(), "gate must suppress while the fingerprint claims unchanged");

        // Past the reconcile interval the real scan runs anyway and surfaces
        // the removal the fingerprint failed to report.
        let changes = sweep_with_probe(
            &cache,
            &registry,
            now + RECONCILE_INTERVAL + Duration::from_secs(30),
            &probe,
        );
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, WorktreeChangeKind::Pruned);
    }

    #[test]
    fn gate_extensions_never_postpone_reconciliation() {
        // Continuous 30s sweeps with a fingerprint-blind change: the gate may
        // suppress every intermediate tick, but bounded reconciliation MUST
        // fire a real scan at the 5-minute mark (Orca's
        // WORKTREE_SCAN_ADMIN_RECONCILE_INTERVAL_MS contract).
        let (_temp, manager, registry) = register_git_workspace();
        let wt_path = manager.worktree_path_for("ws-a", "blind-spot").unwrap();
        manager
            .create_worktree(CreateWorktreeOptions::new("ws-a", "blind-spot", &wt_path))
            .unwrap();

        let cache = WorktreeRescanCache::default();
        let now = Instant::now();
        let probe =
            move |_root: &Path| ProbeOutcome::Done(Some("static-claim-unchanged".to_string()));

        assert!(sweep_with_probe(&cache, &registry, now, &probe).is_empty());
        manager.safe_delete(&wt_path).unwrap();

        for seconds in (30..RECONCILE_INTERVAL.as_secs()).step_by(30) {
            let changes =
                sweep_with_probe(&cache, &registry, now + Duration::from_secs(seconds), &probe);
            assert!(
                changes.is_empty(),
                "suppression must hold before the reconcile bound (t=+{seconds}s)"
            );
        }

        let changes =
            sweep_with_probe(&cache, &registry, now + RECONCILE_INTERVAL, &probe);
        assert_eq!(changes.len(), 1, "reconciliation must fire despite gate extensions");
        assert_eq!(changes[0].kind, WorktreeChangeKind::Pruned);

        // The next sweep is a fresh 5-minute window; suppression resumes.
        let changes = sweep_with_probe(
            &cache,
            &registry,
            now + RECONCILE_INTERVAL + Duration::from_secs(30),
            &probe,
        );
        assert!(changes.is_empty());
    }

    #[test]
    fn probe_timeout_permanently_falls_back_to_real_scans() {
        let (_temp, manager, registry) = register_git_workspace();
        let cache = WorktreeRescanCache::default();
        let now = Instant::now();
        let probe_calls = Arc::new(AtomicUsize::new(0));
        let probe = {
            let calls = Arc::clone(&probe_calls);
            move |_root: &Path| {
                calls.fetch_add(1, Ordering::SeqCst);
                ProbeOutcome::TimedOut
            }
        };

        assert!(sweep_with_probe(&cache, &registry, now, &probe).is_empty());
        assert_eq!(probe_calls.load(Ordering::SeqCst), 1);

        let wt_path = manager.worktree_path_for("ws-a", "after-wedge").unwrap();
        manager
            .create_worktree(CreateWorktreeOptions::new("ws-a", "after-wedge", &wt_path))
            .unwrap();

        let changes = sweep_with_probe(&cache, &registry, now + Duration::from_secs(30), &probe);
        assert_eq!(probe_calls.load(Ordering::SeqCst), 1, "wedged workspaces never probe again");
        assert_eq!(changes.len(), 1, "wedged workspaces always reconcile with a real scan");
        assert_eq!(changes[0].kind, WorktreeChangeKind::Created);
        assert!(cache.wedged.lock().contains("ws-a"));

        // A wedged first sweep persists even though that sweep's baseline had
        // not been recorded when the probe timed out.
        let fresh_cache = WorktreeRescanCache::default();
        assert!(sweep_with_probe(&fresh_cache, &registry, now, &probe).is_empty());
        assert!(fresh_cache.wedged.lock().contains("ws-a"));
    }

    #[test]
    fn worktree_change_kind_wire_names_cover_updated() {
        assert_eq!(
            serde_json::to_value(WorktreeChangeKind::Updated).unwrap(),
            serde_json::json!("updated")
        );
        assert_eq!(
            serde_json::to_value(WorktreeChangeKind::DirtyChanged).unwrap(),
            serde_json::json!("dirtyChanged")
        );
    }
}
