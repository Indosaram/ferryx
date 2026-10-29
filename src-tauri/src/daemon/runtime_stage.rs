//! Immutable, generation-keyed copies of the daemon runtime outside the GUI install.
//!
//! The OS job of each daemon generation runs its own staged copy, never the GUI's executable, so
//! replacing, moving or deleting the app leaves every running daemon's binary untouched. A
//! generation key is `<version>-<first 8 hex of the executable's sha256>`: two builds that share a
//! version still get two keys. A staged generation is written once (temp dir, fsync, hash check,
//! atomic rename) and never overwritten.
//!
//! Layout under [`crate::daemon::server::daemon_runtime_root`]:
//! - macOS: `<root>/<key>/Ferryx Daemon.app`, a byte-for-byte `ditto` copy of the signed helper
//!   bundle the release ships at `<App>.app/Contents/Helpers/Ferryx Daemon.app`. The staged copy
//!   must pass `codesign --verify --strict`; nothing here builds or edits a bundle.
//! - Linux: `<root>/<key>/ferryx` (source: the AppImage path when running from one).
//! - Windows: `<root>\\<key>\\ferryx.exe`.
//!
//! Beside the executable (outside any bundle) every generation also gets copies of the remote
//! helper assets (`<key>/helpers`) and the web UI (`<key>/ui/dist`), so a daemon running from its
//! staged copy needs nothing from the app install.

use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Directory name of the runtime root below the per-platform data directory.
pub const RUNTIME_DIR_NAME: &str = "daemon-runtime";
/// Name of the helper bundle under `<App>.app/Contents/Helpers` (scripts/macos-daemon-helper.mjs).
pub const HELPER_BUNDLE_NAME: &str = "Ferryx Daemon.app";
/// Executable name inside the helper bundle's `Contents/MacOS`.
pub const HELPER_EXECUTABLE: &str = "ferryx";
/// File inside every generation dir (outside any bundle) recording when it was staged, in unix
/// milliseconds; garbage collection keeps the newest generations by this value.
pub const STAGED_AT_FILE: &str = ".staged-at";
/// Dir inside every generation (outside any bundle, so the macOS signature stays sealed) holding a
/// copy of the app's remote helper assets (`manifest.json` plus one binary per target): the staged
/// daemon installs SSH helpers from here, since it no longer runs from the app install.
pub const HELPER_ASSETS_DIR: &str = "helpers";
/// Dir inside every generation (outside any bundle) holding a copy of the app's built web UI, so
/// the remote server of a staged daemon serves the real client instead of the placeholder page.
/// Same relative path as the app bundle resource (`tauri.conf.json` `bundle.resources`).
pub const UI_DIST_DIR: &str = "ui/dist";
/// Generations that garbage collection always keeps, newest first.
pub const KEEP_NEWEST: usize = 2;
const TMP_MARKER: &str = ".tmp-";

#[cfg(windows)]
const DAEMON_FILE_NAME: &str = "ferryx.exe";
#[cfg(not(windows))]
const DAEMON_FILE_NAME: &str = "ferryx";

#[derive(Debug, thiserror::Error)]
pub enum RuntimeStageError {
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },
    #[error("the packaged daemon helper bundle is missing at {}", path.display())]
    HelperMissing { path: PathBuf },
    #[error("the staged daemon runtime at {} failed signature verification: {detail}", path.display())]
    SignatureInvalid { path: PathBuf, detail: String },
    #[error("the staged copy {} has sha256 {actual}, expected {expected}", path.display())]
    HashMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    #[error("daemon runtime generation {key} already exists with different contents; it is never overwritten")]
    ExistingGenerationDiffers { key: String },
}

fn io_error(context: impl Into<String>) -> impl FnOnce(io::Error) -> RuntimeStageError {
    let context = context.into();
    move |source| RuntimeStageError::Io { context, source }
}

/// A staged, immutable daemon runtime generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedRuntime {
    pub key: String,
    pub executable: PathBuf,
}

/// What gets copied into a generation dir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageSource {
    /// A single executable, staged as `<key>/<file_name>`.
    File { executable: PathBuf, file_name: String },
    /// A signed macOS bundle, staged as `<key>/<bundle name>`; `executable` lies inside it and is
    /// what the generation key hashes.
    Bundle { bundle: PathBuf, executable: PathBuf },
}

impl StageSource {
    fn hashed_executable(&self) -> &Path {
        match self {
            Self::File { executable, .. } | Self::Bundle { executable, .. } => executable,
        }
    }

    /// The staged executable's path relative to the generation dir.
    fn staged_executable_relative(&self) -> PathBuf {
        match self {
            Self::File { file_name, .. } => PathBuf::from(file_name),
            Self::Bundle { bundle, executable } => {
                let bundle_name = bundle.file_name().unwrap_or_else(|| OsStr::new(HELPER_BUNDLE_NAME));
                let inside = executable.strip_prefix(bundle).unwrap_or(executable);
                Path::new(bundle_name).join(inside)
            }
        }
    }
}

/// `<version>-<first 8 hex digits of the executable's sha256>`.
pub fn generation_key(version: &str, exe_sha256: &[u8; 32]) -> String {
    let prefix: String = exe_sha256[..4].iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{version}-{prefix}")
}

/// `$FERRYX_DATA_DIR/daemon-runtime` when set (isolated QA), else `<platform base>/daemon-runtime`.
pub fn runtime_root_from(data_dir: Option<&OsStr>, platform_base: Option<PathBuf>) -> Option<PathBuf> {
    data_dir
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or(platform_base)
        .map(|base| base.join(RUNTIME_DIR_NAME))
}

/// The packaged helper bundle next to a macOS GUI executable
/// (`<App>.app/Contents/MacOS/ferryx` -> `<App>.app/Contents/Helpers/Ferryx Daemon.app`).
pub fn macos_helper_source(gui_executable: &Path) -> Result<StageSource, RuntimeStageError> {
    let contents = gui_executable.parent().and_then(Path::parent).unwrap_or(Path::new(""));
    let bundle = contents.join("Helpers").join(HELPER_BUNDLE_NAME);
    let executable = bundle.join("Contents").join("MacOS").join(HELPER_EXECUTABLE);
    if !executable.is_file() {
        return Err(RuntimeStageError::HelperMissing { path: bundle });
    }
    Ok(StageSource::Bundle { bundle, executable })
}

/// The platform's stage source for the running GUI. Linux callers pass
/// `supervisor_linux::daemon_executable` (the AppImage path when running from one).
pub fn platform_source(source_exe: &Path) -> Result<StageSource, RuntimeStageError> {
    if cfg!(target_os = "macos") {
        return macos_helper_source(source_exe);
    }
    Ok(StageSource::File {
        executable: source_exe.to_path_buf(),
        file_name: DAEMON_FILE_NAME.to_string(),
    })
}

/// Stages the runtime for `source_exe` under `root` for this build's version. Blocking: async
/// callers go through [`stage_runtime_blocking`].
pub fn stage_runtime(root: &Path, source_exe: &Path) -> Result<StagedRuntime, RuntimeStageError> {
    let staged = stage_from(
        root,
        &platform_source(source_exe)?,
        env!("CARGO_PKG_VERSION"),
    )?;
    let generation = root.join(&staged.key);
    // The running process is the app being staged (or a staged generation of it); an AppImage
    // `source_exe` sits outside its mounted image, so look next to the live executable first.
    let running_exe = std::env::current_exe().ok();
    let find = |near: fn(&Path) -> Option<PathBuf>| {
        running_exe
            .as_deref()
            .and_then(near)
            .or_else(|| near(source_exe))
    };
    match find(helper_assets_dir_near) {
        Some(assets) => {
            if let Err(error) = ensure_helper_assets(&generation, &assets) {
                tracing::warn!(%error, generation = %staged.key, "Could not stage the remote helper assets; SSH helper installs from this daemon will fail");
            }
        }
        None => {
            tracing::warn!(source = %source_exe.display(), "No remote helper assets next to the daemon source; SSH helper installs from this daemon will fail");
        }
    }
    match find(ui_dist_dir_near) {
        Some(dist) => {
            if let Err(error) = ensure_ui_dist(&generation, &dist) {
                tracing::warn!(%error, generation = %staged.key, "Could not stage the web UI; this daemon's remote server will show its placeholder page");
            }
        }
        None => {
            tracing::warn!(source = %source_exe.display(), "No web UI next to the daemon source; this daemon's remote server will show its placeholder page");
        }
    }
    Ok(staged)
}

/// The remote helper assets dir (one holding `manifest.json`) next to `executable`: an app
/// install (Windows `<install dir>/helpers`, macOS `Contents/Resources/helpers`, Linux deb or
/// AppImage `usr/lib/Ferryx/helpers`) or a staged generation (`<key>/helpers`, which is
/// `../../../helpers` from a macOS staged bundle executable).
pub fn helper_assets_dir_near(executable: &Path) -> Option<PathBuf> {
    let exe_dir = executable.parent()?;
    [
        exe_dir.join(HELPER_ASSETS_DIR),
        exe_dir.join("../Resources").join(HELPER_ASSETS_DIR),
        exe_dir.join("resources").join(HELPER_ASSETS_DIR),
        exe_dir.join("../lib/Ferryx").join(HELPER_ASSETS_DIR),
        exe_dir.join("../../..").join(HELPER_ASSETS_DIR),
    ]
    .into_iter()
    .find(|dir| dir.join("manifest.json").is_file())
}

/// Where the built web UI may sit relative to `executable`, in lookup order: an app install
/// (macOS `Contents/Resources/ui/dist`, Windows `<install dir>/ui/dist` or `resources/ui/dist`,
/// Linux deb or AppImage `usr/lib/Ferryx/ui/dist`) or a staged generation (`<key>/ui/dist`: next
/// to a Linux/Windows staged executable, beside the sealed bundle on macOS).
pub fn ui_dist_candidates(executable: &Path) -> Vec<PathBuf> {
    let Some(exe_dir) = executable.parent() else {
        return Vec::new();
    };
    let mut candidates = vec![
        exe_dir.join("../Resources").join(UI_DIST_DIR),
        exe_dir.join(UI_DIST_DIR),
        exe_dir.join("resources").join(UI_DIST_DIR),
        exe_dir.join("../lib/Ferryx").join(UI_DIST_DIR),
        exe_dir.join("../lib").join(UI_DIST_DIR),
    ];
    // macOS staged generation: `<key>/Ferryx Daemon.app/Contents/MacOS/ferryx` -> `<key>/ui/dist`.
    let generation = exe_dir
        .parent()
        .and_then(Path::parent)
        .filter(|bundle| bundle.file_name() == Some(OsStr::new(HELPER_BUNDLE_NAME)))
        .and_then(Path::parent);
    if let Some(generation) = generation {
        candidates.push(generation.join(UI_DIST_DIR));
    }
    candidates
}

/// The first of [`ui_dist_candidates`] that holds an `index.html`.
pub fn ui_dist_dir_near(executable: &Path) -> Option<PathBuf> {
    ui_dist_candidates(executable)
        .into_iter()
        .find(|dir| dir.join("index.html").is_file())
}

/// Copies `assets` into `<generation>/helpers` unless it is already there. Also backfills
/// generations staged before helper assets were staged. The copy lands complete (temp dir, then
/// rename) and an existing assets dir is never overwritten.
pub fn ensure_helper_assets(generation: &Path, assets: &Path) -> Result<(), RuntimeStageError> {
    ensure_tree_copy(&generation.join(HELPER_ASSETS_DIR), "manifest.json", assets)
}

/// Copies the built web UI `dist` into `<generation>/ui/dist` unless it is already there, with the
/// guarantees of [`ensure_helper_assets`] (it also backfills older generations).
pub fn ensure_ui_dist(generation: &Path, dist: &Path) -> Result<(), RuntimeStageError> {
    ensure_tree_copy(&generation.join(UI_DIST_DIR), "index.html", dist)
}

/// Copies `source` to `destination` unless `destination/<marker>` is already a file: a temp dir
/// next to `destination`, fsync, then rename, so the copy lands complete and is never overwritten.
fn ensure_tree_copy(
    destination: &Path,
    marker: &str,
    source: &Path,
) -> Result<(), RuntimeStageError> {
    if destination.join(marker).is_file() {
        return Ok(());
    }
    let parent = destination.parent().unwrap_or(Path::new(""));
    let name = destination
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    fs::create_dir_all(parent).map_err(io_error(format!("create {}", parent.display())))?;
    static NEXT_TMP: AtomicU64 = AtomicU64::new(0);
    let tmp = parent.join(format!(
        "{name}{TMP_MARKER}{}-{}",
        std::process::id(),
        NEXT_TMP.fetch_add(1, Ordering::Relaxed)
    ));
    let result = copy_tree(source, &tmp)
        .and_then(|()| sync_dir(&tmp).map_err(io_error(format!("sync {}", tmp.display()))))
        .and_then(|()| match fs::rename(&tmp, destination) {
            Ok(()) => sync_dir(parent).map_err(io_error(format!("sync {}", parent.display()))),
            // Another stager won the rename.
            Err(_) if destination.join(marker).is_file() => Ok(()),
            Err(error) => Err(io_error(format!("rename {} to {}", tmp.display(), destination.display()))(error)),
        });
    if tmp.exists() {
        remove_tmp_dir(&tmp);
    }
    result
}

/// Recursive byte copy (`fs::copy` keeps unix permission bits, so helper binaries stay executable).
fn copy_tree(source: &Path, destination: &Path) -> Result<(), RuntimeStageError> {
    fs::create_dir_all(destination).map_err(io_error(format!("create {}", destination.display())))?;
    for entry in fs::read_dir(source).map_err(io_error(format!("read {}", source.display())))? {
        let entry = entry.map_err(io_error(format!("read {}", source.display())))?;
        let target = destination.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).map_err(io_error(format!("copy {}", entry.path().display())))?;
        }
    }
    Ok(())
}

/// The key [`stage_runtime`] gives `source_exe`'s generation, without staging it. Blocking
/// (hashes the stage source).
pub fn source_generation_key(source_exe: &Path) -> Result<String, RuntimeStageError> {
    let source = platform_source(source_exe)?;
    Ok(generation_key(env!("CARGO_PKG_VERSION"), &sha256_file(source.hashed_executable())?))
}

/// [`stage_runtime`] on a blocking thread (`crate::ipc::run_blocking`).
pub async fn stage_runtime_blocking(
    root: PathBuf,
    source_exe: PathBuf,
) -> Result<StagedRuntime, crate::ipc::IpcError> {
    crate::ipc::run_blocking(move || stage_runtime(&root, &source_exe).map_err(crate::ipc::IpcError::internal)).await
}

/// Stages `source` as an immutable generation of `version` under `root`; reuses an existing
/// generation with the same hash and never overwrites one.
pub fn stage_from(root: &Path, source: &StageSource, version: &str) -> Result<StagedRuntime, RuntimeStageError> {
    let source_hash = sha256_file(source.hashed_executable())?;
    let key = generation_key(version, &source_hash);
    let relative = source.staged_executable_relative();
    let final_dir = root.join(&key);
    if final_dir.exists() {
        return reuse_existing(&final_dir, &key, &relative, &source_hash, source);
    }
    fs::create_dir_all(root).map_err(io_error(format!("create {}", root.display())))?;

    static NEXT_TMP: AtomicU64 = AtomicU64::new(0);
    let tmp_dir = root.join(format!(
        "{key}{TMP_MARKER}{}-{}",
        std::process::id(),
        NEXT_TMP.fetch_add(1, Ordering::Relaxed)
    ));
    if let Err(error) = fill_tmp_dir(&tmp_dir, source, &relative, &source_hash) {
        remove_tmp_dir(&tmp_dir);
        return Err(error);
    }
    match fs::rename(&tmp_dir, &final_dir) {
        Ok(()) => {
            sync_dir(root).map_err(io_error(format!("sync {}", root.display())))?;
            Ok(StagedRuntime {
                key,
                executable: final_dir.join(relative),
            })
        }
        // Another stager won the rename: its generation has the same key, so reuse it after the
        // same checks, and drop ours.
        Err(_) if final_dir.exists() => {
            remove_tmp_dir(&tmp_dir);
            reuse_existing(&final_dir, &key, &relative, &source_hash, source)
        }
        Err(error) => {
            remove_tmp_dir(&tmp_dir);
            Err(io_error(format!("rename {} to {}", tmp_dir.display(), final_dir.display()))(error))
        }
    }
}

fn reuse_existing(
    final_dir: &Path,
    key: &str,
    relative: &Path,
    source_hash: &[u8; 32],
    source: &StageSource,
) -> Result<StagedRuntime, RuntimeStageError> {
    let executable = final_dir.join(relative);
    match sha256_file(&executable) {
        Ok(hash) if &hash == source_hash => {}
        _ => {
            return Err(RuntimeStageError::ExistingGenerationDiffers { key: key.to_string() });
        }
    }
    if let StageSource::Bundle { bundle, .. } = source {
        let bundle_name = bundle.file_name().unwrap_or_else(|| OsStr::new(HELPER_BUNDLE_NAME));
        verify_signature(&final_dir.join(bundle_name))?;
    }
    Ok(StagedRuntime {
        key: key.to_string(),
        executable,
    })
}

fn fill_tmp_dir(
    tmp_dir: &Path,
    source: &StageSource,
    relative: &Path,
    source_hash: &[u8; 32],
) -> Result<(), RuntimeStageError> {
    fs::create_dir(tmp_dir).map_err(io_error(format!("create {}", tmp_dir.display())))?;
    let staged_executable = tmp_dir.join(relative);
    match source {
        StageSource::File { executable, .. } => {
            fs::copy(executable, &staged_executable).map_err(io_error(format!(
                "copy {} to {}",
                executable.display(),
                staged_executable.display()
            )))?;
        }
        StageSource::Bundle { bundle, .. } => {
            let bundle_name = bundle.file_name().unwrap_or_else(|| OsStr::new(HELPER_BUNDLE_NAME));
            copy_bundle(bundle, &tmp_dir.join(bundle_name))?;
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged_executable, fs::Permissions::from_mode(0o755))
            .map_err(io_error(format!("chmod {}", staged_executable.display())))?;
    }
    // Windows FlushFileBuffers needs a handle opened for writing.
    fs::OpenOptions::new()
        .write(true)
        .open(&staged_executable)
        .and_then(|file| file.sync_all())
        .map_err(io_error(format!("sync {}", staged_executable.display())))?;

    let staged_hash = sha256_file(&staged_executable)?;
    if &staged_hash != source_hash {
        return Err(RuntimeStageError::HashMismatch {
            path: staged_executable,
            expected: hex(source_hash),
            actual: hex(&staged_hash),
        });
    }
    if let StageSource::Bundle { bundle, .. } = source {
        let bundle_name = bundle.file_name().unwrap_or_else(|| OsStr::new(HELPER_BUNDLE_NAME));
        verify_signature(&tmp_dir.join(bundle_name))?;
    }

    let staged_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0);
    let marker = tmp_dir.join(STAGED_AT_FILE);
    fs::write(&marker, staged_at.to_string()).map_err(io_error(format!("write {}", marker.display())))?;
    if let Some(parent) = staged_executable.parent() {
        sync_dir(parent).map_err(io_error(format!("sync {}", parent.display())))?;
    }
    sync_dir(tmp_dir).map_err(io_error(format!("sync {}", tmp_dir.display())))
}

fn remove_tmp_dir(tmp_dir: &Path) {
    if let Err(error) = fs::remove_dir_all(tmp_dir) {
        if error.kind() != io::ErrorKind::NotFound {
            tracing::warn!(%error, path = %tmp_dir.display(), "Could not remove a daemon runtime staging dir");
        }
    }
}

/// Copies a bundle byte-for-byte, keeping extended attributes and the code signature.
#[cfg(target_os = "macos")]
fn copy_bundle(source: &Path, destination: &Path) -> Result<(), RuntimeStageError> {
    let output = std::process::Command::new("/usr/bin/ditto")
        .arg(source)
        .arg(destination)
        .output()
        .map_err(io_error("run ditto"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(RuntimeStageError::Io {
        context: format!("ditto {} {}", source.display(), destination.display()),
        source: io::Error::other(String::from_utf8_lossy(&output.stderr).trim().to_string()),
    })
}

/// Bundles are a macOS packaging form; elsewhere a plain recursive copy keeps the bytes.
#[cfg(not(target_os = "macos"))]
fn copy_bundle(source: &Path, destination: &Path) -> Result<(), RuntimeStageError> {
    fs::create_dir_all(destination).map_err(io_error(format!("create {}", destination.display())))?;
    for entry in fs::read_dir(source).map_err(io_error(format!("read {}", source.display())))? {
        let entry = entry.map_err(io_error(format!("read {}", source.display())))?;
        let target = destination.join(entry.file_name());
        let file_type = entry.file_type().map_err(io_error(format!("stat {}", entry.path().display())))?;
        if file_type.is_dir() {
            copy_bundle(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).map_err(io_error(format!("copy {}", entry.path().display())))?;
        }
    }
    Ok(())
}

/// `codesign --verify --strict` on a staged bundle; there is no fallback to the GUI executable.
#[cfg(target_os = "macos")]
fn verify_signature(bundle: &Path) -> Result<(), RuntimeStageError> {
    let output = std::process::Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict"])
        .arg(bundle)
        .output()
        .map_err(io_error("run codesign"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(RuntimeStageError::SignatureInvalid {
        path: bundle.to_path_buf(),
        detail: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

/// Only macOS ships the runtime as a signed bundle; the hash check covers the other platforms.
#[cfg(not(target_os = "macos"))]
fn verify_signature(_bundle: &Path) -> Result<(), RuntimeStageError> {
    Ok(())
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::File::open(dir)?.sync_all()
    }
    // Windows opens directories only with FILE_FLAG_BACKUP_SEMANTICS; NTFS journals the rename.
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

fn sha256_file(path: &Path) -> Result<[u8; 32], RuntimeStageError> {
    use std::io::Read;
    let mut file = fs::File::open(path).map_err(io_error(format!("open {}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(io_error(format!("hash {}", path.display())))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// What keeps a staged generation alive during garbage collection.
pub trait RuntimeUsage {
    /// Executable paths of every running process the caller can inspect.
    fn running_executables(&self) -> io::Result<Vec<PathBuf>>;
    /// Executable paths referenced by registered OS jobs (launchd plists, systemd units,
    /// scheduled tasks, including the legacy single job).
    fn registered_job_executables(&self) -> Vec<PathBuf>;
}

/// [`RuntimeUsage`] backed by the OS process table.
pub struct SystemRuntimeUsage {
    pub registered_job_executables: Vec<PathBuf>,
}

impl RuntimeUsage for SystemRuntimeUsage {
    fn running_executables(&self) -> io::Result<Vec<PathBuf>> {
        running_process_executables()
    }

    fn registered_job_executables(&self) -> Vec<PathBuf> {
        self.registered_job_executables.clone()
    }
}

/// Removes generation dirs that no running process executes from and no registered OS job
/// references, always keeping the newest [`KEEP_NEWEST`]. Returns the removed keys. When the
/// process table cannot be read nothing is removed: an unverifiable generation may be in use.
pub fn collect_garbage(root: &Path, usage: &dyn RuntimeUsage) -> Result<Vec<String>, RuntimeStageError> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_error(format!("read {}", root.display()))(error)),
    };
    let mut generations = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io_error(format!("read {}", root.display())))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // In-flight staging dirs belong to their stager.
        if name.contains(TMP_MARKER) || !entry.path().is_dir() {
            continue;
        }
        generations.push((staged_at(&entry.path()), name, entry.path()));
    }
    generations.sort_by_key(|generation| std::cmp::Reverse(generation.0));
    if generations.len() <= KEEP_NEWEST {
        return Ok(Vec::new());
    }

    let running = match usage.running_executables() {
        Ok(running) => running,
        Err(error) => {
            tracing::warn!(%error, "Cannot list running processes; keeping every daemon runtime generation");
            return Ok(Vec::new());
        }
    };
    let in_use: Vec<PathBuf> = running
        .iter()
        .chain(usage.registered_job_executables().iter())
        .map(|path| normalized(path))
        .collect();

    let mut removed = Vec::new();
    for (_, key, dir) in generations.into_iter().skip(KEEP_NEWEST) {
        let dir_normalized = normalized(&dir);
        if in_use.iter().any(|path| path.starts_with(&dir_normalized)) {
            continue;
        }
        match fs::remove_dir_all(&dir) {
            Ok(()) => removed.push(key),
            Err(error) => {
                tracing::warn!(%error, generation = %key, "Could not remove an unused daemon runtime generation");
            }
        }
    }
    Ok(removed)
}

/// Generations among `job_keys` (generations that still have a registered OS job) whose job a
/// supervisor may delete once it lost login autostart, so a job naming an old runtime no longer
/// blocks [`collect_garbage`]. Kept: the [`KEEP_NEWEST`] newest generations by staged-at under
/// `root` (a job whose generation dir is gone counts as oldest), `current` even when it is older
/// (an older app registering never costs a newer generation its job), and every generation a
/// running process executes from. When the process table cannot be read nothing is returned: a
/// job whose generation may be live stays.
pub fn superseded_job_keys(
    root: &Path,
    job_keys: &[String],
    current: &str,
    usage: &dyn RuntimeUsage,
) -> Vec<String> {
    let mut newest_first: Vec<&String> = job_keys.iter().collect();
    newest_first.sort_by_key(|key| std::cmp::Reverse(staged_at(&root.join(key))));
    let older: Vec<&String> = newest_first
        .into_iter()
        .skip(KEEP_NEWEST)
        .filter(|key| key.as_str() != current)
        .collect();
    if older.is_empty() {
        return Vec::new();
    }
    let running: Vec<PathBuf> = match usage.running_executables() {
        Ok(running) => running.iter().map(|path| normalized(path)).collect(),
        Err(error) => {
            tracing::warn!(%error, "Cannot list running processes; keeping every daemon generation job");
            return Vec::new();
        }
    };
    // The root, not the generation dir, is resolved: a removed generation dir has no canonical form.
    let root = normalized(root);
    older
        .into_iter()
        .filter(|key| {
            // Same comparison form as `running`: on Windows the key's case is folded too.
            let dir = comparison_form(&root.join(key), cfg!(windows));
            !running.iter().any(|path| path.starts_with(&dir))
        })
        .cloned()
        .collect()
}

fn staged_at(dir: &Path) -> u128 {
    fs::read_to_string(dir.join(STAGED_AT_FILE))
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

/// Canonical form for prefix comparison: resolves symlinks (`/var` -> `/private/var` on macOS,
/// 8.3 short names on Windows) and, on Windows, drops the verbatim prefix and case.
fn normalized(path: &Path) -> PathBuf {
    let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    comparison_form(&resolved, cfg!(windows))
}

/// The case- and prefix-insensitive form Windows paths are compared in (`windows`), else `path`.
fn comparison_form(path: &Path, windows: bool) -> PathBuf {
    if !windows {
        return path.to_path_buf();
    }
    let text = path.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    PathBuf::from(text.to_lowercase())
}

#[cfg(target_os = "macos")]
fn running_process_executables() -> io::Result<Vec<PathBuf>> {
    // SAFETY: a null buffer with size 0 asks libproc only for the current process count.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Err(io::Error::last_os_error());
    }
    // Headroom for processes started between the two calls.
    let mut pids: Vec<libc::pid_t> = vec![0; count as usize + 64];
    let buffer_size = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
    // SAFETY: the buffer holds exactly `buffer_size` bytes of pid_t slots.
    let listed = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), buffer_size) };
    if listed <= 0 {
        return Err(io::Error::last_os_error());
    }
    pids.truncate(listed as usize);
    let mut executables = Vec::new();
    for pid in pids {
        // PROC_PIDPATHINFO_MAXSIZE (4 * MAXPATHLEN).
        let mut buffer = vec![0u8; 4 * 1024];
        // SAFETY: the buffer is writable for its full length, which is passed as the size.
        let len = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
        // Exited since listing, or another user's process (which cannot run from our data dir).
        if len <= 0 {
            continue;
        }
        buffer.truncate(len as usize);
        if let Ok(path) = String::from_utf8(buffer) {
            executables.push(PathBuf::from(path));
        }
    }
    Ok(executables)
}

#[cfg(target_os = "linux")]
fn running_process_executables() -> io::Result<Vec<PathBuf>> {
    let mut executables = Vec::new();
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        if !entry.file_name().to_string_lossy().bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        // Exited since listing, or another user's process (which cannot run from our data dir).
        let Ok(target) = fs::read_link(entry.path().join("exe")) else {
            continue;
        };
        let text = target.to_string_lossy();
        let text = text.strip_suffix(" (deleted)").unwrap_or(&text);
        executables.push(PathBuf::from(text));
    }
    Ok(executables)
}

#[cfg(windows)]
fn running_process_executables() -> io::Result<Vec<PathBuf>> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStringExt;

    #[link(name = "kernel32")]
    extern "system" {
        fn K32EnumProcesses(process_ids: *mut u32, bytes: u32, bytes_needed: *mut u32) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn QueryFullProcessImageNameW(process: *mut c_void, flags: u32, name: *mut u16, size: *mut u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    let mut pids = vec![0u32; 4096];
    loop {
        let bytes = (pids.len() * std::mem::size_of::<u32>()) as u32;
        let mut needed = 0u32;
        // SAFETY: the buffer holds `bytes` bytes of u32 slots; `needed` is a valid out pointer.
        if unsafe { K32EnumProcesses(pids.as_mut_ptr(), bytes, &mut needed) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // A full buffer may have truncated the list: grow and ask again.
        if needed < bytes {
            pids.truncate(needed as usize / std::mem::size_of::<u32>());
            break;
        }
        pids.resize(pids.len() * 2, 0);
    }
    let mut executables = Vec::new();
    for pid in pids {
        // SAFETY: OpenProcess takes plain values; a null return is handled below.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        // Exited since listing, or a protected or other user's process.
        if handle.is_null() {
            continue;
        }
        let mut buffer = vec![0u16; 32 * 1024];
        let mut size = buffer.len() as u32;
        // SAFETY: `handle` is open; the buffer is writable for `size` UTF-16 units.
        let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size) };
        // SAFETY: `handle` came from OpenProcess above and is closed exactly once.
        unsafe { CloseHandle(handle) };
        if ok != 0 {
            executables.push(PathBuf::from(std::ffi::OsString::from_wide(&buffer[..size as usize])));
        }
    }
    Ok(executables)
}

/// No portable process table elsewhere: report it, so garbage collection removes nothing.
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn running_process_executables() -> io::Result<Vec<PathBuf>> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "process listing is not implemented on this platform"))
}

#[cfg(test)]
#[path = "runtime_stage_tests.rs"]
mod tests;
