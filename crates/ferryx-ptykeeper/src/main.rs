//! `ferryx-ptykeeper`: the process that owns PTY masters across a daemon crash.
//!
//! It is started by the supervisor (Phase 2) and by launchd/systemd, serves the framed UDS at
//! `/tmp/ferryx-{uid}/keeper.sock`, and exits when its listener fails or SIGTERM arrives. It is
//! FFI-free: it never links or calls libghostty-vt and never parses terminal output.

use std::fs;
use std::io;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ferryx_ptykeeper::budget::MemoryBudget;
use ferryx_ptykeeper::endpoint::{self, Endpoint};
use ferryx_ptykeeper::keeper::Keeper;
use ferryx_ptykeeper::server::{self, ServerState};

const USAGE: &str = "usage: ferryx-ptykeeper [--socket <path>] [--print-path] [--version]

  --socket <path>   bind this path instead of /tmp/ferryx-<uid>/keeper.sock
  --print-path      print the endpoint path and exit
  --version         print the keeper version and exit
";

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut socket_override: Option<String> = None;
    let mut print_path = false;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--version" => {
                println!("{}", ferryx_ptykeeper::KEEPER_VERSION);
                return std::process::ExitCode::SUCCESS;
            }
            "--print-path" => {
                print_path = true;
                index += 1;
            }
            "--socket" => match args.get(index + 1) {
                Some(path) => {
                    socket_override = Some(path.clone());
                    index += 2;
                }
                None => {
                    eprintln!("--socket needs a path");
                    return std::process::ExitCode::from(2);
                }
            },
            "--help" | "-h" => {
                print!("{USAGE}");
                return std::process::ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument: {other}");
                eprint!("{USAGE}");
                return std::process::ExitCode::from(2);
            }
        }
    }

    let uid = unsafe { libc::getuid() };

    // A `--socket` override names the socket, so its parent directory is where the lock lives;
    // otherwise the endpoint is the plan's per-uid runtime directory. Building it through `Endpoint`
    // is what keeps the path, the lock and the peer-uid check derived from one place.
    let endpoint = match &socket_override {
        None => match Endpoint::for_runtime() {
            Ok(endpoint) => endpoint,
            Err(err) => {
                eprintln!("ferryx-ptykeeper: {err}");
                return std::process::ExitCode::FAILURE;
            }
        },
        Some(path) => {
            let socket = PathBuf::from(path.as_str());
            if socket.as_os_str().len() > endpoint::MAX_SOCKET_PATH_BYTES {
                eprintln!(
                    "ferryx-ptykeeper: --socket {} is {} bytes, longer than the {} byte unix socket limit",
                    socket.display(),
                    socket.as_os_str().len(),
                    endpoint::MAX_SOCKET_PATH_BYTES
                );
                return std::process::ExitCode::from(2);
            }
            let parent = socket.parent().filter(|dir| !dir.as_os_str().is_empty());
            let runtime_dir = parent.unwrap_or_else(|| Path::new("."));
            match Endpoint::new(runtime_dir, uid) {
                Ok(endpoint) => endpoint,
                Err(err) => {
                    eprintln!("ferryx-ptykeeper: {err}");
                    return std::process::ExitCode::FAILURE;
                }
            }
        }
    };

    let socket_path = socket_override
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| endpoint.socket_path().to_path_buf());

    if print_path {
        println!("{}", socket_path.display());
        return std::process::ExitCode::SUCCESS;
    }

    // An overridden socket may live in a directory this keeper does not own (`/tmp`, a test
    // sandbox), so the plan's `0700` directory policy is applied only to the endpoint we own.
    if socket_override.is_none() {
        if let Err(err) = endpoint.prepare_dir() {
            eprintln!("ferryx-ptykeeper: {err}");
            return std::process::ExitCode::FAILURE;
        }
    } else if let Err(err) = require_private_parent(&socket_path, uid) {
        // Checked before the lock is taken, so a refused override never creates a lock file in a
        // directory the keeper should not be writing to at all. `--print-path` returns above this
        // point on purpose: naming a path is not the same as agreeing to serve from it.
        eprintln!("ferryx-ptykeeper: {err}");
        return std::process::ExitCode::from(2);
    }

    // Non-blocking on purpose: a second keeper is not an error condition worth a crash loop. It
    // exits cleanly and the daemon keeps talking to the one that already holds the lock.
    let _instance = match endpoint.try_lock() {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            eprintln!(
                "ferryx-ptykeeper: another keeper already holds {}; exiting",
                endpoint.lock_path().display()
            );
            return std::process::ExitCode::SUCCESS;
        }
        // Only `Ok(None)` means "another keeper owns the instance", which is a clean exit. A failure
        // to even attempt the lock is not evidence that the instance is served: reporting success
        // would tell a supervisor that this keeper is running when nothing is listening.
        Err(err) => {
            eprintln!("ferryx-ptykeeper: {err}");
            return std::process::ExitCode::FAILURE;
        }
    };

    // Safe now that the lock is held: any socket on this path is a dead keeper's leftover.
    let cleared = if socket_override.is_none() {
        endpoint.clear_stale_socket().map(|_| ()).map_err(|err| err.to_string())
    } else {
        clear_path(&socket_path, uid).map_err(|err| err.to_string())
    };
    if let Err(err) = cleared {
        eprintln!("ferryx-ptykeeper: {err}");
        return std::process::ExitCode::FAILURE;
    }

    let listener = if socket_override.is_none() {
        match endpoint.bind() {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("ferryx-ptykeeper: {err}");
                return std::process::ExitCode::FAILURE;
            }
        }
    } else {
        match bind_owner_only(&socket_path, uid) {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("ferryx-ptykeeper: cannot bind {}: {err}", socket_path.display());
                return std::process::ExitCode::FAILURE;
            }
        }
    };

    eprintln!(
        "ferryx-ptykeeper {} listening on {}",
        ferryx_ptykeeper::KEEPER_VERSION,
        socket_path.display()
    );

    let budget = Arc::new(std::sync::Mutex::new(MemoryBudget::default()));
    let keeper = Arc::new(Keeper::new(env!("CARGO_PKG_VERSION"), budget));
    let state = Arc::new(ServerState::new(keeper, uid));

    let outcome = server::serve(listener, state, endpoint.clone());
    // The socket goes before the lock so a successor can bind. An overridden socket is removed by
    // path; the endpoint's own socket is removed by the endpoint, which knows which file it bound.
    if socket_override.is_some() {
        let _ = fs::remove_file(&socket_path);
    } else {
        endpoint.clear_socket();
    }

    match outcome {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("ferryx-ptykeeper: accept loop failed: {err}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Refuses a `--socket` parent directory another user could act in.
///
/// A socket is only as private as the directory holding it: in a group- or world-writable parent,
/// anyone may unlink the bound socket and put their own in its place, so the daemon would connect
/// to an impostor that speaks the keeper protocol. The parent must therefore already exist, be a
/// directory, be owned by this uid, and not be writable by group or other.
fn require_private_parent(path: &Path, uid: u32) -> io::Result<()> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let metadata = fs::metadata(parent).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "--socket parent {} must already exist and be usable: {error}",
                parent.display()
            ),
        )
    })?;
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("--socket parent {} is not a directory", parent.display()),
        ));
    }
    if metadata.uid() != uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "--socket parent {} is owned by uid {}, not {uid}",
                parent.display(),
                metadata.uid()
            ),
        ));
    }
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o022 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "--socket parent {} is mode {mode:o}, which lets group or other replace the socket",
                parent.display()
            ),
        ));
    }
    Ok(())
}

/// Removes a socket left behind at `path` by a keeper that is gone. Only called while holding the
/// lock, which is what makes "gone" true rather than a guess.
fn clear_path(path: &Path, uid: u32) -> io::Result<()> {
    use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};

    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                format!("cannot inspect the stale keeper socket {}: {error}", path.display()),
            ))
        }
    };
    if !metadata.file_type().is_socket() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} exists and is not a socket; it is not removed", path.display()),
        ));
    }
    if metadata.uid() != uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} is owned by uid {}, not {uid}; it is not removed",
                path.display(),
                metadata.uid()
            ),
        ));
    }
    fs::remove_file(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot remove the stale keeper socket {}: {error}", path.display()),
        )
    })
}

/// Binds `path` and narrows it to `uid`, mirroring what [`Endpoint::bind`] guarantees for the
/// endpoint it owns. Used only for a `--socket` override, which `Endpoint` cannot express.
fn bind_owner_only(path: &Path, uid: u32) -> io::Result<UnixListener> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(endpoint::SOCKET_MODE))?;
    let owner = fs::symlink_metadata(path)?.uid();
    if owner != uid {
        let _ = fs::remove_file(path);
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("bound socket {} is owned by uid {owner}, not {uid}", path.display()),
        ));
    }
    Ok(listener)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    fn uid() -> u32 {
        unsafe { libc::getuid() }
    }

    #[test]
    fn a_group_or_world_writable_socket_parent_is_refused() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).expect("chmod 0700");
        let target = temp.path().join("keeper.sock");
        require_private_parent(&target, uid()).expect("a 0700 tempdir is private enough");
        for mode in [0o770, 0o757, 0o777] {
            fs::set_permissions(temp.path(), fs::Permissions::from_mode(mode)).expect("chmod");
            let error = require_private_parent(&target, uid())
                .expect_err("a parent others may write to lets them replace the socket");
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "mode {mode:o}");
        }
    }

    #[test]
    fn a_socket_parent_that_does_not_exist_is_refused_rather_than_created() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("missing").join("keeper.sock");
        let error = require_private_parent(&target, uid()).expect_err("must refuse");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(!temp.path().join("missing").exists(), "the keeper does not invent a parent");
    }

    #[test]
    fn clearing_a_path_removes_only_a_stale_socket_of_this_uid() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("keeper.sock");

        clear_path(&path, uid()).expect("an absent socket is already clear");

        fs::write(&path, b"a regular file someone planted").expect("plant");
        let error = clear_path(&path, uid()).expect_err("must refuse a non-socket");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(path.exists(), "a non-socket is never deleted");
        fs::remove_file(&path).expect("cleanup");

        let listener = UnixListener::bind(&path).expect("bind a stale socket");
        drop(listener);
        clear_path(&path, uid()).expect("a stale socket is cleared");
        assert!(!path.exists());
    }
}
