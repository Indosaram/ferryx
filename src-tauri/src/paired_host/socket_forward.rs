//! The desktop end of a cloud host's socket forward: a local unix socket that behaves exactly
//! like a connected host, except every connection is carried to the host's own RPC socket over
//! the paired relay's end-to-end encrypted session.
//!
//! `ferryx-cli cloud socket <name>` prints this path and the caller (`omo host session ...`,
//! with `--socket`) cannot tell it apart from a forwarded SSH socket. That is the point: task
//! 1 selected `MODE=A`, so the RPC verbs run on this Mac against a socket, and the transport
//! under it is the relay rather than SSH.
//!
//! The path is `$TMPDIR/ferryx-cloud-<uid>/<name>.sock`, created 0600 inside a 0700 directory.
//! A socket is not a secret, but its directory listing names the machines this desktop manages,
//! and the mode keeps another local account from connecting to it.

use crate::paired_host::client::{ClientError, MachineClient, PairedSocketForward};
use crate::paired_host::service::PairedHostService;
use crate::scoped_contracts::Epoch;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long the readiness probe waits for the host's `get_protocol_info` reply.
///
/// The same bound task 6 uses for the SSH forward: an `ExitOnForwardFailure`/bind that succeeds
/// proves nothing about the far end, so the only honest proof is one answered RPC line.
pub const PROBE_DEADLINE: Duration = Duration::from_secs(10);

/// One RPC line that any host daemon answers without side effects.
pub const PROBE_REQUEST: &str = "{\"type\":\"get_protocol_info\"}\n";

/// Directory holding this user's forwarded sockets.
fn socket_dir() -> PathBuf {
    let uid = {
        #[cfg(unix)]
        // SAFETY: `getuid` has no preconditions and cannot fail.
        unsafe {
            libc::getuid()
        }
        #[cfg(not(unix))]
        0u32
    };
    std::env::temp_dir().join(format!("ferryx-cloud-{uid}"))
}

/// The socket a caller for `name` will be told to use.
///
/// `name` is a catalog machine name, so it is held to the same bare-token rule the SSH forward
/// applies to a file stem: it becomes a path segment and must not be able to leave the
/// directory.
pub fn socket_path(name: &str) -> Result<PathBuf, ClientError> {
    let valid = !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if !valid || name == "." || name == ".." {
        return Err(ClientError::local("INVALID_REQUEST"));
    }
    Ok(socket_dir().join(format!("{name}.sock")))
}

/// The file identity of the socket name a forward published.
///
/// `Drop` must remove this forward's own name and nothing else. A second forward for the same
/// name re-binds the path to a fresh inode, so removing by name alone would delete the name the
/// newer forward published and leave its listener alive on an inode no caller can reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PublishedName {
    dev: u64,
    ino: u64,
}

#[cfg(unix)]
fn published_name(path: &Path) -> Option<PublishedName> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::symlink_metadata(path).ok()?;
    Some(PublishedName {
        dev: meta.dev(),
        ino: meta.ino(),
    })
}

#[cfg(not(unix))]
fn published_name(_path: &Path) -> Option<PublishedName> {
    None
}

/// A listening local socket, removed when this value drops.
///
/// The listener task is aborted on drop, so a caller that abandons the forward does not leave a
/// live path behind that silently accepts and then fails every connection. The path is removed
/// only while it still names the socket this forward bound: a newer forward for the same name
/// owns the path by then, and it keeps its own entry.
pub struct SocketForward {
    path: PathBuf,
    published: Option<PublishedName>,
    task: tokio::task::JoinHandle<()>,
}

impl SocketForward {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SocketForward {
    fn drop(&mut self) {
        self.task.abort();
        // A newer forward for the same name has re-bound the path by now, and that listener is
        // the one a caller reaches. Removing by name would orphan it, so the entry goes only if
        // it still resolves to the socket bound here.
        let still_published_here = match self.published {
            Some(published) => published_name(&self.path) == Some(published),
            None => true,
        };
        if still_published_here {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Publishes `path` and accepts connections, forwarding each one over its own encrypted session.
#[cfg(unix)]
pub async fn listen(
    client: MachineClient,
    service: PairedHostService,
    host_id: String,
    generation: Epoch,
    name: &str,
) -> Result<SocketForward, ClientError> {
    use std::os::unix::fs::PermissionsExt;

    let path = socket_path(name)?;
    let dir = path
        .parent()
        .ok_or_else(|| ClientError::local("INVALID_REQUEST"))?;
    std::fs::create_dir_all(dir).map_err(|_| ClientError::local("SOCKET_LISTEN_FAILED"))?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| ClientError::local("SOCKET_LISTEN_FAILED"))?;
    // A stale socket from a crashed run would make `bind` fail forever; unlinking first is
    // safe because a live sibling listener holds its own inode, not the name.
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path)
        .map_err(|_| ClientError::local("SOCKET_LISTEN_FAILED"))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .map_err(|_| ClientError::local("SOCKET_LISTEN_FAILED"))?;
    // Recorded while the name is certainly this forward's own, so `Drop` can tell its entry
    // from one a later forward for the same name has since published.
    let published = published_name(&path);

    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let client = client.clone();
            let service = service.clone();
            let host_id = host_id.clone();
            tokio::spawn(async move {
                // Every accepted connection gets its own authenticated session: one wedged
                // caller must not stall the next, and a revocation cancels all of them at once.
                let Ok(forward) = client
                    .attach_socket_forward(&service, host_id, generation)
                    .await
                else {
                    return;
                };
                let _ = carry(stream, forward).await;
            });
        }
    });
    Ok(SocketForward {
        path,
        published,
        task,
    })
}

/// Copies bytes between one accepted connection and the host's socket.
///
/// The encrypted session is already established by `attach_socket_forward`, so this is a plain
/// byte copy in both directions and ends when either side closes.
#[cfg(unix)]
async fn carry(
    local: tokio::net::UnixStream,
    forward: PairedSocketForward,
) -> std::io::Result<()> {
    use crate::remote::attach_crypto::WebSocketByteStream;
    let mut local = local;
    let mut remote = WebSocketByteStream::new(forward.socket);
    tokio::io::copy_bidirectional(&mut local, &mut remote).await?;
    Ok(())
}

/// Proves the forwarded socket answers RPC, not just that a socket exists.
///
/// A forward whose remote end is missing still yields a bound local socket, and the failure then
/// surfaces on the caller's first verb with no indication which hop was wrong. One answered
/// line turns that into a bound, attributable error.
pub async fn probe(path: &Path) -> Result<String, ClientError> {
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(ClientError::local("SOCKET_PROBE_FAILED"))
    }
    #[cfg(unix)]
    {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let Ok(connected) =
            tokio::time::timeout(PROBE_DEADLINE, tokio::net::UnixStream::connect(path)).await
        else {
            return Err(ClientError::local("SOCKET_PROBE_TIMEOUT"));
        };
        let Ok(mut stream) = connected else {
            return Err(ClientError::local("SOCKET_PROBE_FAILED"));
        };
        stream
            .write_all(PROBE_REQUEST.as_bytes())
            .await
            .map_err(|_| ClientError::local("SOCKET_PROBE_FAILED"))?;
        let mut line = String::new();
        let mut reader = BufReader::new(stream);
        let read = reader.read_line(&mut line);
        tokio::time::timeout(PROBE_DEADLINE, read)
            .await
            .map_err(|_| ClientError::local("SOCKET_PROBE_TIMEOUT"))?
            .map_err(|_| ClientError::local("SOCKET_PROBE_FAILED"))?;
        // The reply must parse: a daemon that answers with a diagnostic line is not ready.
        serde_json::from_str::<serde_json::Value>(line.trim())
            .map(|_| line)
            .map_err(|_| ClientError::local("SOCKET_PROBE_NOT_JSON"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_machine_name_cannot_leave_the_socket_directory() {
        assert!(socket_path("../etc/passwd").is_err());
        assert!(socket_path("a/b").is_err());
        assert!(socket_path("").is_err());
        assert!(socket_path("..").is_err());
        assert!(socket_path("fx-e2e-daytona").is_ok());
    }

    #[test]
    fn the_socket_lives_under_the_per_user_temporary_directory() {
        let path = socket_path("fx-a").expect("valid name");
        assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("fx-a.sock"));
        let dir = path.parent().expect("parent").file_name().unwrap();
        assert!(dir.to_string_lossy().starts_with("ferryx-cloud-"));
    }

    #[tokio::test]
    async fn a_dead_socket_path_is_an_error_not_a_hang() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("absent.sock");
        let outcome = probe(&missing).await;
        assert!(outcome.is_err(), "an absent socket must not report ready");
        assert_eq!(outcome.unwrap_err().code, "SOCKET_PROBE_FAILED");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_listener_that_never_answers_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("silent.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        // Accept and hold the connection without answering, the shape of a wedged daemon.
        let hold = tokio::spawn(async move {
            let _held = listener.accept().await;
            tokio::time::sleep(Duration::from_secs(60)).await;
        });
        let outcome = probe(&path).await;
        assert_eq!(outcome.unwrap_err().code, "SOCKET_PROBE_TIMEOUT");
        hold.abort();
    }

    /// One forward for `name`, wired to a service holding no paired hosts.
    ///
    /// The accept task then fails every attach, which the tests below do not look at: their
    /// subject is which file-system entry the published name resolves to, not a stream.
    #[cfg(unix)]
    async fn forward_for(name: &str, dir: &Path) -> SocketForward {
        listen(
            MachineClient::new(),
            PairedHostService::open(dir.to_path_buf()),
            "fx-fix-dup-host".to_owned(),
            Epoch(1),
            name,
        )
        .await
        .expect("listen")
    }

    #[cfg(unix)]
    fn inode(path: &Path) -> (u64, u64) {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::symlink_metadata(path).expect("the socket name must exist");
        (meta.dev(), meta.ino())
    }

    /// Two forwards for one `name` cannot both own it, but each must only ever remove the name
    /// it published itself: dropping the older one has to leave the newer one's path alone.
    #[cfg(unix)]
    #[tokio::test]
    async fn two_forwards_for_one_name_do_not_share_a_name_safely() {
        let dir = tempfile::tempdir().unwrap();
        let name = format!("fx-fix-dup-{}", std::process::id());
        let older = forward_for(&name, dir.path()).await;
        let path = older.path().to_path_buf();
        let newer = forward_for(&name, dir.path()).await;
        assert_eq!(newer.path(), path.as_path(), "both forwards share one name");
        let owned_by_newer = inode(&path);
        drop(older);
        assert_eq!(
            inode(&path),
            owned_by_newer,
            "dropping the older forward must not remove the name the newer one published"
        );
        drop(newer);
        assert!(
            std::fs::symlink_metadata(&path).is_err(),
            "the forward that owns the name removes it on drop"
        );
    }

    /// The shape a caller hits on a retry or from two terminals: the second forward for a name
    /// re-binds it, and the first one's drop must not leave the second listening on a path no
    /// one can name (a listener that is alive but unreachable).
    #[cfg(unix)]
    #[tokio::test]
    async fn a_dropped_older_forward_orphans_the_newer_listener() {
        let dir = tempfile::tempdir().unwrap();
        let name = format!("fx-fix-orphan-{}", std::process::id());
        let older = forward_for(&name, dir.path()).await;
        let path = older.path().to_path_buf();
        let newer = forward_for(&name, dir.path()).await;
        // While both are alive the name is served by the newer listener.
        assert!(tokio::net::UnixStream::connect(&path).await.is_ok());
        drop(older);
        assert!(
            tokio::net::UnixStream::connect(&path).await.is_ok(),
            "the newer listener must stay reachable through the name once the older one drops"
        );
        drop(newer);
        assert!(
            tokio::net::UnixStream::connect(&path).await.is_err(),
            "after the last owner drops the name is gone"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_json_reply_proves_the_far_end() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ready.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            assert_eq!(line, PROBE_REQUEST);
            reader
                .into_inner()
                .write_all(b"{\"protocol\":1}\n")
                .await
                .unwrap();
        });
        let reply = probe(&path).await.expect("a JSON reply is ready");
        assert!(reply.contains("protocol"));
        server.await.unwrap();
    }
}
