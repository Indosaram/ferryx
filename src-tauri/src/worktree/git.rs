use crate::worktree::model::{DirtyFile, DirtyState, Worktree, WorktreeError};
use std::path::{Path, PathBuf};

#[cfg(unix)]
#[path = "git/unix.rs"]
mod containment;
#[cfg(windows)]
#[path = "git/windows.rs"]
mod containment;

fn escape_git_arg_for_log(arg: &str) -> String {
    let mut escaped = String::with_capacity(arg.len());
    for ch in arg.chars() {
        match ch {
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(&mut escaped, "\\u{{{:x}}}", ch as u32);
            }
            ch => escaped.push(ch),
        }
    }
    escaped
}
fn validate_git_value(value: &str, label: &str) -> Result<(), WorktreeError> {
    if value.is_empty() {
        return Err(WorktreeError::InvalidNamespace {
            reason: format!("{label} cannot be empty"),
        });
    }
    if value.starts_with('-') {
        return Err(WorktreeError::InvalidNamespace {
            reason: format!("{label} cannot start with a dash"),
        });
    }
    if value.chars().any(char::is_control) {
        return Err(WorktreeError::InvalidNamespace {
            reason: format!("{label} cannot contain control characters"),
        });
    }
    Ok(())
}

pub fn strip_verbatim_prefix(path: &str) -> String {
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else if let Some(unc) = path.strip_prefix("//?/UNC/") {
        format!("//{unc}")
    } else if let Some(stripped) = path.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else if let Some(stripped) = path.strip_prefix("//?/") {
        stripped.to_string()
    } else if let Some(stripped) = path.strip_prefix(r"\\.\") {
        stripped.to_string()
    } else {
        path.to_string()
    }
}

pub fn normalize_path_for_git(path: &Path) -> PathBuf {
    if let Some(path_str) = path.to_str() {
        PathBuf::from(strip_verbatim_prefix(path_str))
    } else {
        path.to_path_buf()
    }
}

fn validate_git_path_argument(path: &Path) -> Result<String, WorktreeError> {
    let path_str = path
        .to_str()
        .ok_or_else(|| WorktreeError::ParseError("Invalid UTF-8 in worktree path".into()))?;
    let normalized = strip_verbatim_prefix(path_str);
    if normalized.starts_with('-') {
        return Err(WorktreeError::InvalidPath {
            path: path.to_path_buf(),
            reason: "Git path argument cannot start with a dash".into(),
        });
    }
    if normalized.chars().any(char::is_control) {
        return Err(WorktreeError::InvalidPath {
            path: path.to_path_buf(),
            reason: "Git path argument cannot contain control characters".into(),
        });
    }
    Ok(normalized)
}

fn validate_base_ref(repo_root: &Path, base_ref: &str) -> Result<(), WorktreeError> {
    validate_git_value(base_ref, "Base ref")?;
    let commit_ref = format!("{base_ref}^{{commit}}");
    run_git(
        repo_root,
        &["rev-parse", "--verify", "--quiet", commit_ref.as_str()],
    )?;
    Ok(())
}

/// Request-scoped child budget, installed only on a blocking domain worker.
#[derive(Clone)]
pub(crate) struct GitBudget {
    pub deadline: std::time::Instant,
    pub revoked: tokio::sync::watch::Receiver<bool>,
    pub cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub cancellation: Option<std::sync::Arc<tokio::sync::Notify>>,
}
thread_local! { static GIT_BUDGET: std::cell::RefCell<Option<GitBudget>> = const { std::cell::RefCell::new(None) }; }
#[cfg(all(test, unix))]
thread_local! { pub(crate) static CHILD_STARTED: std::cell::RefCell<Option<tokio::sync::oneshot::Sender<u32>>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
thread_local! { static EXCEPTIONAL_CLEANUP: std::cell::RefCell<Option<tokio::sync::oneshot::Sender<bool>>> = const { std::cell::RefCell::new(None) }; }
/// Local IPC mutations have no remote grant. Retain the sender for the whole
/// transaction so a closed watch channel cannot masquerade as revocation.
pub(crate) fn with_local_worktree_budget<T>(work: impl FnOnce() -> T) -> T {
    if GIT_BUDGET.with(|slot| slot.borrow().is_some()) {
        return work();
    }
    let (_grant, revoked) = tokio::sync::watch::channel(false);
    with_git_budget(
        GitBudget {
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(40),
            revoked,
            cancelled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cancellation: None,
        },
        work,
    )
}
pub(crate) fn with_git_budget<T>(budget: GitBudget, work: impl FnOnce() -> T) -> T {
    struct Restore(Option<GitBudget>);
    impl Drop for Restore {
        fn drop(&mut self) {
            GIT_BUDGET.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(GIT_BUDGET.with(|slot| slot.replace(Some(budget))));
    work()
}
fn bounded_output(
    cwd: &Path,
    args: &[&str],
    budget: GitBudget,
) -> Result<std::process::Output, WorktreeError> {
    let fail = |code: &str| WorktreeError::ParseError(code.into());
    if *budget.revoked.borrow() {
        return Err(fail("UNAUTHORIZED"));
    }
    if budget.cancelled.load(std::sync::atomic::Ordering::Acquire)
        || std::time::Instant::now() >= budget.deadline
    {
        return Err(fail("TIMEOUT"));
    }
    tokio::runtime::Handle::current().block_on(async {
        use tokio::io::AsyncReadExt;
        let mut command = crate::util::no_window_tokio_command("git");
        let owner = containment::Owner::prepare(&mut command)?;
        let mut child = command.args(args).current_dir(cwd)
            .env("GIT_TERMINAL_PROMPT", "0").env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped())
            .kill_on_drop(true).spawn()?;
        let owner = match owner.attach(&child) {
            Ok(owner) => owner,
            Err(error) => {
                let killed = child.start_kill();
                let waited = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;
                let mut details = vec![error.to_string()];
                if let Err(error) = killed { details.push(error.to_string()); }
                match waited {
                    Ok(Ok(_)) => {},
                    Ok(Err(error)) => details.push(error.to_string()),
                    Err(_) => details.push("Git child reap deadline".into()),
                }
                return Err(fail(&details.join("; ")));
            }
        };
        let exit = owner.exit_observer();
        let mut exited = tokio::task::spawn_blocking(move || exit.wait());
        let mut exit_observed = false;
        #[cfg(all(test, unix))]
        CHILD_STARTED.with(|signal| {
            if let Some(signal) = signal.borrow_mut().take() { signal.send(child.id().expect("spawned child PID")).expect("child observer"); }
        });

        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        async fn read<R: tokio::io::AsyncRead + Unpin>(reader: R) -> Result<Vec<u8>, WorktreeError> {
            let mut bytes = Vec::new();
            reader.take(262145).read_to_end(&mut bytes).await?;
            if bytes.len() > 262144 { return Err(WorktreeError::ParseError("OUTPUT_LIMIT_EXCEEDED".into())); }
            Ok(bytes)
        }
        let mut revoked = budget.revoked;
        let deadline = budget.deadline.min(std::time::Instant::now() + std::time::Duration::from_secs(30));
        let result = tokio::select! {
            biased;
            _ = async {
                if let Some(signal) = &budget.cancellation { signal.notified().await; }
                else { std::future::pending::<()>().await; }
            } => Err(fail("TIMEOUT")),
            _ = revoked.wait_for(|v| *v) => Err(fail("UNAUTHORIZED")),
            result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), async {
                let (stdout, stderr, ()) = tokio::try_join!(read(stdout), read(stderr), async {
                    let observation = (&mut exited).await;
                    exit_observed = true;
                    observation.map_err(|e| fail(&format!("Git exit observer: {e}")))?.map_err(WorktreeError::Io)
                })?;
                Ok((stdout, stderr))
            }) => result.unwrap_or_else(|_| Err(fail("TIMEOUT"))),
        };
        // The leader is still unreaped: its identity cannot be recycled while
        // signaling the owned group/job, even when descendants closed the pipes.
        #[cfg(test)]
        let cleanup_receipt = EXCEPTIONAL_CLEANUP.with(|slot| slot.borrow_mut().take());
        #[cfg(test)]
        let inject_panic = cleanup_receipt.is_some();
        let cleanup = tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            if inject_panic { panic!("A08_DRAIN_WORKER_FAILURE"); }
            owner.terminate()
        }).await
            .map_err(|e| fail(&format!("Git group drain: {e}")))
            .and_then(|result| result.map_err(WorktreeError::Io));
        // Even a containment failure must not abandon the immediate child.
        let fallback = if cleanup.is_err() { child.start_kill() } else { Ok(()) };
        let observation = if !exit_observed {
            tokio::time::timeout(std::time::Duration::from_secs(5), &mut exited).await
                .map_err(|_| fail("Git exit observer deadline"))
                .and_then(|joined| joined.map_err(|e| fail(&format!("Git exit observer: {e}"))))
                .and_then(|result| result.map_err(WorktreeError::Io))
        } else { Ok(()) };
        let status = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await
            .map_err(|_| fail("Git child reap deadline"))
            .and_then(|result| result.map_err(WorktreeError::Io));
        #[cfg(test)]
        if let Some(receipt) = cleanup_receipt {
            eprintln!("A08 exceptional cleanup cwd={cwd:?} explicit_wait={status:?}");
            let _ = receipt.send(status.is_ok());
        }
        // Preserve every cleanup failure; none may bypass the explicit wait.
        let errors: Vec<_> = [cleanup.err(), fallback.err().map(WorktreeError::Io), observation.err()]
            .into_iter().flatten().collect();
        if !errors.is_empty() {
            let mut details: Vec<_> = errors.into_iter().map(|error| error.to_string()).collect();
            if let Err(error) = &status { details.push(error.to_string()); }
            return Err(fail(&details.join("; ")));
        }
        let status = status?;
        result.map(|(stdout, stderr)| std::process::Output { stdout, stderr, status })
    })
}

/// Executes a git command in the specified directory.
#[cfg(test)]
mod local_budget_tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a08_git_native_byte_fidelity() {
        use std::os::unix::ffi::OsStringExt;
        let root = tempfile::tempdir().unwrap();
        run_git(root.path(), &["init", "--quiet"]).unwrap();
        let valid = " leading \u{fffd} trailing ";
        std::fs::write(root.path().join(valid), b"valid").unwrap();
        let manager = crate::worktree::WorktreeManager::new(root.path());
        assert_eq!(
            manager.check_dirty(root.path()).unwrap().files[0].path,
            valid
        );
        let invalid = std::ffi::OsString::from_vec(b" leading \xff trailing ".to_vec());
        #[cfg(target_os = "linux")]
        std::fs::write(root.path().join(&invalid), b"invalid").unwrap();
        let object = run_git(root.path(), &["hash-object", "-w", valid]).unwrap();
        let mut index = crate::util::no_window_command("git")
            .args([
                "update-index",
                "--add",
                "--cacheinfo",
                "100644",
                object.trim(),
            ])
            .arg(&invalid)
            .current_dir(root.path())
            .spawn()
            .unwrap();
        assert!(index.wait().unwrap().success());
        let result = run_git(root.path(), &["ls-files", "-z"]);
        eprintln!("A08 byte fixture cwd={:?} result={result:?}", root.path());
        root.close().unwrap();
        assert!(
            matches!(result, Err(WorktreeError::ParseError(_))),
            "invalid native bytes aliased: {result:?}"
        );
    }

    #[test]
    fn a08_git_legacy_path_fidelity() {
        let rows = parse_worktree_list_porcelain("worktree  leading trailing \nHEAD abc\n\nworktree \"/tmp/quote\\\" \\357\\277\\275 \\t\"\nHEAD def\n\n").unwrap();
        assert_eq!(rows[0].path, PathBuf::from(" leading trailing "));
        assert_eq!(rows[1].path, PathBuf::from("/tmp/quote\" \u{fffd} \t"));
        assert!(parse_worktree_list_porcelain("worktree \"/tmp/\\377\"\n").is_err());
    }

    #[test]
    fn a08_git_real_legacy_worktree_path() {
        let root = tempfile::tempdir().unwrap();
        run_git(root.path(), &["init", "--quiet"]).unwrap();
        run_git(
            root.path(),
            &[
                "-c",
                "user.name=A08",
                "-c",
                "user.email=a08@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "base",
            ],
        )
        .unwrap();
        let path = root.path().join(if cfg!(windows) {
            " leading '\u{fffd} trailing"
        } else {
            " leading \"\u{fffd} trailing "
        });
        git_worktree_add(root.path(), &path, "feature", None).unwrap();
        let expected = std::fs::canonicalize(&path).unwrap();
        let rows = git_worktree_list(root.path()).unwrap();
        root.close().unwrap();
        assert!(rows.iter().any(|row| row.path == expected), "{rows:?}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a08_git_exceptional_cleanup_waits_before_error() {
        let (sent, receipt) = tokio::sync::oneshot::channel();
        let result = tokio::task::spawn_blocking(move || {
            let root = tempfile::tempdir().unwrap();
            let (_grant, revoked) = tokio::sync::watch::channel(false);
            let (started, observed) = tokio::sync::oneshot::channel();
            CHILD_STARTED.with(|slot| *slot.borrow_mut() = Some(started));
            EXCEPTIONAL_CLEANUP.with(|slot| *slot.borrow_mut() = Some(sent));
            let result = bounded_output(
                root.path(),
                &["--version"],
                GitBudget {
                    deadline: std::time::Instant::now() + std::time::Duration::from_secs(5),
                    revoked,
                    cancelled: Default::default(),
                    cancellation: None,
                },
            );
            root.close().unwrap();
            let pid = observed.blocking_recv().unwrap();
            eprintln!("A08 exceptional Git pid={pid} root_removed=true result={result:?}");
            assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
            result
        })
        .await
        .unwrap();
        assert!(result.is_err());
        assert_eq!(
            receipt.await,
            Ok(true),
            "drain JoinError bypassed explicit child wait"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn git_hook_descendant_containment() {
        use futures_util::FutureExt;
        use std::os::unix::fs::PermissionsExt;
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
        for mode in [
            "cancel", "revoke", "deadline", "output", "success", "failure", "injected",
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let root = tokio::task::spawn_blocking(move || {
                let root = tempfile::tempdir().unwrap();
                run_git(root.path(), &["init", "--quiet"]).unwrap();
                let hook = root.path().join(".git/hooks/pre-commit");
                std::fs::write(&hook, format!(r#"#!/usr/bin/env python3
import os, socket, sys
ancestors = [os.getpid()]
for generation in range(2):
    pid = os.fork()
    if pid:
        _, status = os.waitpid(pid, 0)
        os._exit(os.waitstatus_to_exitcode(status) if os.WIFEXITED(status) else 1)
    ancestors.append(os.getpid())
s = socket.create_connection(('127.0.0.1', {port}))
s.sendall(('%d %d %d %d %d\n' % (os.getpid(), os.getpgrp(), os.getuid(), ancestors[0], ancestors[1])).encode())
action = s.recv(1)
if action == b'o':
    os.write(1, b'x' * 262145)
    s.recv(1)
elif action == b'f':
    sys.exit(1)
"#)).unwrap();
                std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o700)).unwrap();
                root
            }).await.unwrap();
            let cwd = root.path().to_owned();
            let (grant, revoked) = tokio::sync::watch::channel(false);
            let (started, observed) = tokio::sync::oneshot::channel();
            let cancellation = std::sync::Arc::new(tokio::sync::Notify::new());
            let notify = cancellation.clone();
            let mut worker = tokio::task::spawn_blocking(move || {
                CHILD_STARTED.with(|signal| *signal.borrow_mut() = Some(started));
                bounded_output(
                    &cwd,
                    &[
                        "-c",
                        "user.name=A08",
                        "-c",
                        "user.email=a08@example.invalid",
                        "commit",
                        "--allow-empty",
                        "-m",
                        "fixture",
                    ],
                    GitBudget {
                        deadline: std::time::Instant::now()
                            + std::time::Duration::from_secs(if mode == "deadline" {
                                3
                            } else {
                                30
                            }),
                        revoked,
                        cancelled: Default::default(),
                        cancellation: Some(cancellation),
                    },
                )
            });
            let mut socket = None;
            let mut sibling: Option<tokio::process::Child> = None;
            #[cfg(target_os = "macos")]
            let mut descendant_exited = None;
            let mut worker_joined = false;
            let outcome = std::panic::AssertUnwindSafe(async {
            let git_pid = tokio::time::timeout(std::time::Duration::from_secs(5), observed).await.unwrap().unwrap();
            let (accepted, _) = tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept()).await.unwrap().unwrap();
            socket = Some(tokio::io::BufReader::new(accepted));
            let socket = socket.as_mut().unwrap();
            let mut ready = String::new();
            tokio::time::timeout(std::time::Duration::from_secs(5), socket.read_line(&mut ready)).await.unwrap().unwrap();
            let ids: Vec<i32> = ready.split_whitespace().map(|v| v.parse().unwrap()).collect();
            eprintln!("A08 hook mode={mode} git={git_pid} leaf={} pgid={} uid={} hook={} intermediate={} test_pgid={}", ids[0], ids[1], ids[2], ids[3], ids[4], unsafe { libc::getpgrp() });
            assert_eq!(ids[1], git_pid as i32);
            assert_ne!(ids[1], unsafe { libc::getpgrp() });
            #[cfg(target_os = "macos")]
            { descendant_exited = Some({
                use std::os::fd::FromRawFd;
                let fd = unsafe { libc::kqueue() };
                assert!(fd >= 0);
                let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) };
                let event = libc::kevent { ident: ids[0] as usize, filter: libc::EVFILT_PROC,
                    flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_ONESHOT, fflags: libc::NOTE_EXIT,
                    data: 0, udata: std::ptr::null_mut() };
                use std::os::fd::AsRawFd;
                assert_eq!(unsafe { libc::kevent(fd.as_raw_fd(), &event, 1, std::ptr::null_mut(), 0, std::ptr::null()) }, 0);
                tokio::task::spawn_blocking(move || {
                    let mut event: libc::kevent = unsafe { std::mem::zeroed() };
                    let timeout = libc::timespec { tv_sec: 10, tv_nsec: 0 };
                    let count = unsafe { libc::kevent(fd.as_raw_fd(), std::ptr::null(), 0, &mut event, 1, &timeout) };
                    assert_eq!(count, 1, "descendant exit notification");
                    assert_ne!(event.fflags & libc::NOTE_EXIT, 0);
                })
            }); }
            sibling = Some( crate::util::no_window_tokio_command("python3")
                .args(["-c", "import sys; print('ready', flush=True); sys.stdin.read()"])
                .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped())
                .kill_on_drop(true).spawn().unwrap());
            let sibling = sibling.as_mut().unwrap();
            let mut sibling_ready = String::new();
            tokio::time::timeout(std::time::Duration::from_secs(5), tokio::io::BufReader::new(sibling.stdout.take().unwrap()).read_line(&mut sibling_ready)).await.unwrap().unwrap();
            assert_eq!(sibling_ready.trim(), "ready");
            if mode == "injected" { std::panic::panic_any("A08_INJECTED_READY_FAILURE"); }
            match mode {
                "cancel" => notify.notify_one(),
                "revoke" => grant.send(true).unwrap(),
                "output" => socket.write_all(b"o").await.unwrap(),
                "success" => socket.write_all(b"s").await.unwrap(),
                "failure" => socket.write_all(b"f").await.unwrap(),
                _ => {},
            }
            let joined = tokio::time::timeout(std::time::Duration::from_secs(5), &mut worker).await.unwrap();
            worker_joined = true;
            let result = joined.unwrap();
            let mut tail = Vec::new();
            let exited = tokio::time::timeout(std::time::Duration::from_secs(1), socket.read_to_end(&mut tail)).await.is_ok();
            assert!(exited, "owned hook survived {mode}: {result:?}");
            assert!(sibling.try_wait().unwrap().is_none(), "unrelated child killed");
            match mode {
                "success" => assert!(result.unwrap().status.success()),
                "failure" => assert!(!result.unwrap().status.success()),
                _ => assert!(matches!(result, Err(WorktreeError::ParseError(ref code)) if code == match mode { "revoke" => "UNAUTHORIZED", "output" => "OUTPUT_LIMIT_EXCEEDED", _ => "TIMEOUT" }), "{result:?}"),
            }
            assert_eq!(unsafe { libc::kill(git_pid as i32, 0) }, -1);
            assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
            }).catch_unwind().await;

            // Keep every owned handle until cleanup completes, even on assertion failure.
            let mut cleanup_errors = Vec::new();
            if let Some(socket) = socket.as_mut() {
                if let Err(error) = socket.shutdown().await {
                    if !matches!(
                        error.kind(),
                        std::io::ErrorKind::BrokenPipe
                            | std::io::ErrorKind::NotConnected
                            | std::io::ErrorKind::ConnectionReset
                    ) {
                        cleanup_errors.push(format!("socket release: {error}"));
                    }
                }
            }
            notify.notify_one();
            if !worker_joined {
                match tokio::time::timeout(std::time::Duration::from_secs(40), &mut worker).await {
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => cleanup_errors.push(format!("Git worker: {error}")),
                    Err(error) => {
                        cleanup_errors.push(format!("Git worker timeout: {error}"));
                        // Blocking workers cannot be aborted. Join before deleting their root.
                        if let Err(error) = worker.await {
                            cleanup_errors.push(format!("Git join: {error}"));
                        }
                    }
                }
            }
            if let Some(socket) = socket.as_mut() {
                let mut tail = Vec::new();
                match tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    socket.read_to_end(&mut tail),
                )
                .await
                {
                    Ok(Ok(_)) => {}
                    result => cleanup_errors.push(format!("socket EOF: {result:?}")),
                }
            }
            drop(socket);
            #[cfg(target_os = "macos")]
            if let Some(mut descendant) = descendant_exited {
                match tokio::time::timeout(std::time::Duration::from_secs(12), &mut descendant)
                    .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => cleanup_errors.push(format!("descendant exit: {error}")),
                    Err(error) => {
                        cleanup_errors.push(format!("descendant timeout: {error}"));
                        if let Err(error) = descendant.await {
                            cleanup_errors.push(format!("descendant join: {error}"));
                        }
                    }
                }
            }
            if let Some(mut sibling) = sibling {
                drop(sibling.stdin.take());
                match tokio::time::timeout(std::time::Duration::from_secs(5), sibling.wait()).await
                {
                    Ok(Ok(status)) if status.success() => {}
                    Ok(result) => cleanup_errors.push(format!("sibling exit: {result:?}")),
                    Err(error) => {
                        cleanup_errors.push(format!("sibling timeout: {error}"));
                        if let Err(error) = sibling.start_kill() {
                            cleanup_errors.push(format!("sibling cleanup kill: {error}"));
                        }
                        if let Err(error) = sibling.wait().await {
                            cleanup_errors.push(format!("sibling reap: {error}"));
                        }
                    }
                }
            }
            drop(listener);
            let closed = tokio::task::spawn_blocking(move || root.close()).await;
            if !matches!(closed, Ok(Ok(()))) {
                cleanup_errors.push(format!("root close: {closed:?}"));
            }
            eprintln!("A08 mode={mode} cleanup worker_joined=true descendant_joined=true sibling_waited=true root_removed={} errors={cleanup_errors:?}", matches!(closed, Ok(Ok(()))));
            assert!(cleanup_errors.is_empty(), "{cleanup_errors:?}");
            if mode == "injected" {
                assert_eq!(
                    outcome.unwrap_err().downcast_ref::<&str>(),
                    Some(&"A08_INJECTED_READY_FAILURE")
                );
            } else if let Err(panic) = outcome {
                std::panic::resume_unwind(panic);
            }
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn running_git_revocation_and_deadline_kill_and_reap() {
        for mode in ["revoke", "deadline", "cancel"] {
            let revoke = mode == "revoke";
            let root = tokio::task::spawn_blocking(|| {
                let root = tempfile::tempdir().unwrap();
                let fifo =
                    std::ffi::CString::new(root.path().join("blocked").to_str().unwrap()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
                root
            })
            .await
            .unwrap();
            let cwd = root.path().to_owned();
            let (grant, revoked) = tokio::sync::watch::channel(false);
            let (started, observed) = tokio::sync::oneshot::channel();
            let cancellation = std::sync::Arc::new(tokio::sync::Notify::new());
            let notify = cancellation.clone();
            let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let flag = cancelled.clone();
            let worker = tokio::task::spawn_blocking(move || {
                CHILD_STARTED.with(|signal| *signal.borrow_mut() = Some(started));
                let budget = GitBudget {
                    deadline: std::time::Instant::now()
                        + std::time::Duration::from_millis(if mode == "deadline" {
                            500
                        } else {
                            30000
                        }),
                    revoked,
                    cancelled,
                    cancellation: Some(cancellation),
                };
                bounded_output(&cwd, &["hash-object", "blocked"], budget)
            });
            let pid = tokio::time::timeout(std::time::Duration::from_secs(5), observed)
                .await
                .unwrap()
                .unwrap();
            if revoke {
                grant.send(true).unwrap();
            }
            if mode == "cancel" {
                flag.store(true, std::sync::atomic::Ordering::Release);
                notify.notify_one();
            }
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), worker)
                .await
                .unwrap()
                .unwrap();
            let mut status = 0;
            let waited = unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) };
            let wait_error = std::io::Error::last_os_error().raw_os_error();
            let alive = unsafe { libc::kill(pid as libc::pid_t, 0) };
            let alive_error = std::io::Error::last_os_error().raw_os_error();
            tokio::task::spawn_blocking(move || root.close().unwrap())
                .await
                .unwrap();
            assert!(
                matches!(result, Err(WorktreeError::ParseError(ref code)) if code == if revoke { "UNAUTHORIZED" } else { "TIMEOUT" })
            );
            assert_eq!((waited, wait_error), (-1, Some(libc::ECHILD)));
            assert_eq!((alive, alive_error), (-1, Some(libc::ESRCH)));
            eprintln!(
                "A08 Git pid={pid} mode={mode} killed=true reaped=true private_root_removed=true"
            );
        }
    }

    #[test]
    fn local_mutation_budget_is_scoped_and_preserves_remote_deadline() {
        assert!(GIT_BUDGET.with(|slot| slot.borrow().is_none()));
        with_local_worktree_budget(|| {
            let local = GIT_BUDGET.with(|slot| slot.borrow().clone().unwrap());
            assert!(!*local.revoked.borrow());
            assert!(local.deadline > std::time::Instant::now());
            with_local_worktree_budget(|| {
                assert_eq!(
                    GIT_BUDGET.with(|slot| slot.borrow().as_ref().unwrap().deadline),
                    local.deadline
                );
            });
        });
        assert!(GIT_BUDGET.with(|slot| slot.borrow().is_none()));
        let (_grant, revoked) = tokio::sync::watch::channel(false);
        let deadline = std::time::Instant::now();
        with_git_budget(
            GitBudget {
                deadline,
                revoked,
                cancelled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                cancellation: None,
            },
            || {
                with_local_worktree_budget(|| {
                    assert_eq!(
                        GIT_BUDGET.with(|slot| slot.borrow().as_ref().unwrap().deadline),
                        deadline
                    )
                });
            },
        );
        assert!(GIT_BUDGET.with(|slot| slot.borrow().is_none()));
    }

    #[tokio::test]
    async fn local_budget_runs_existing_manager_create_delete() {
        crate::ipc::run_blocking(|| {
            let root = tempfile::tempdir().unwrap();
            run_git(root.path(), &["init", "--quiet"]).unwrap();
            run_git(
                root.path(),
                &[
                    "-c",
                    "user.name=A08",
                    "-c",
                    "user.email=a08@example.invalid",
                    "commit",
                    "--allow-empty",
                    "-m",
                    "base",
                ],
            )
            .unwrap();
            let manager = crate::worktree::WorktreeManager::new(root.path());
            let path = manager
                .worktree_path_for("local-budget", "feature")
                .unwrap();
            with_local_worktree_budget(|| {
                manager
                    .create_worktree(crate::worktree::CreateWorktreeOptions::new(
                        "local-budget",
                        "feature",
                        &path,
                    ))
                    .unwrap();
                manager.delete_worktree_and_branch(&path, true).unwrap();
            });
            assert!(!path.exists());
            assert!(GIT_BUDGET.with(|slot| slot.borrow().is_none()));
            root.close().unwrap();
            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn bounded_git_rejects_large_output_and_expired_or_revoked_admission() {
        crate::ipc::run_blocking(|| {
            let root = tempfile::tempdir().unwrap();
            run_git(root.path(), &["init", "--quiet"]).unwrap();
            std::fs::write(root.path().join("large"), vec![b'x'; 262145]).unwrap();
            let object = run_git(root.path(), &["hash-object", "-w", "large"]).unwrap();
            let (grant, revoked) = tokio::sync::watch::channel(false);
            let mut budget = GitBudget { deadline: std::time::Instant::now() + std::time::Duration::from_secs(10), revoked, cancelled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)), cancellation: None };
            let result = bounded_output(root.path(), &["cat-file", "blob", object.trim()], budget.clone());
            assert!(matches!(result, Err(WorktreeError::ParseError(ref code)) if code == "OUTPUT_LIMIT_EXCEEDED"), "{result:?}");
            budget.deadline = std::time::Instant::now();
            assert!(matches!(bounded_output(root.path(), &["status"], budget.clone()), Err(WorktreeError::ParseError(ref code)) if code == "TIMEOUT"));
            grant.send(true).unwrap();
            assert!(matches!(bounded_output(root.path(), &["status"], budget), Err(WorktreeError::ParseError(ref code)) if code == "UNAUTHORIZED"));
            root.close().unwrap();
            Ok(())
        }).await.unwrap();
    }
}

pub fn run_git<P: AsRef<Path>, S: AsRef<str>>(cwd: P, args: &[S]) -> Result<String, WorktreeError> {
    let cwd_path = cwd.as_ref();
    let normalized_cwd = normalize_path_for_git(cwd_path);
    let arg_strs: Vec<&str> = args.iter().map(|s| s.as_ref()).collect();
    let command_str = format!(
        "git {}",
        arg_strs
            .iter()
            .map(|arg| escape_git_arg_for_log(arg))
            .collect::<Vec<_>>()
            .join(" ")
    );

    let budget = GIT_BUDGET.with(|slot| slot.borrow().clone());
    let output = if let Some(budget) = budget {
        bounded_output(&normalized_cwd, &arg_strs, budget)?
    } else {
        crate::util::no_window_command("git")
            .args(&arg_strs)
            .current_dir(&normalized_cwd)
            .output()
            .map_err(WorktreeError::Io)?
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(WorktreeError::GitError {
            command: command_str,
            stderr: stderr.trim().to_string(),
            stdout: stdout.trim().to_string(),
            code: output.status.code(),
        });
    }

    String::from_utf8(output.stdout)
        .map_err(|_| WorktreeError::ParseError("Invalid UTF-8 in Git stdout".into()))
}

fn decode_git_quoted_path(path: &str) -> Result<String, WorktreeError> {
    if !path.starts_with('"') {
        return Ok(path.to_owned());
    }
    let invalid = || WorktreeError::ParseError("Invalid Git quoted path".into());
    let body = path
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(invalid)?;
    let mut input = body.bytes();
    let mut bytes = Vec::new();
    while let Some(byte) = input.next() {
        if byte != b'\\' {
            bytes.push(byte);
            continue;
        }
        let escaped = input.next().ok_or_else(invalid)?;
        bytes.push(match escaped {
            b'a' => 7,
            b'b' => 8,
            b't' => 9,
            b'n' => 10,
            b'v' => 11,
            b'f' => 12,
            b'r' => 13,
            b'\\' | b'"' => escaped,
            b'0'..=b'3' => {
                let second = input
                    .next()
                    .filter(|b| (b'0'..=b'7').contains(b))
                    .ok_or_else(invalid)?;
                let third = input
                    .next()
                    .filter(|b| (b'0'..=b'7').contains(b))
                    .ok_or_else(invalid)?;
                (escaped - b'0') * 64 + (second - b'0') * 8 + (third - b'0')
            }
            _ => return Err(invalid()),
        });
    }
    String::from_utf8(bytes).map_err(|_| invalid())
}

pub(crate) fn native_git_path(path: &str) -> PathBuf {
    #[cfg(windows)]
    if matches!(Path::new(path).components().next(), Some(std::path::Component::Prefix(p)) if matches!(p.kind(), std::path::Prefix::Disk(_)))
    {
        return PathBuf::from(format!(r"\\?\{}", path.replace('/', r"\")));
    }
    PathBuf::from(path)
}

/// Parses the output of `git worktree list --porcelain`.
pub fn parse_worktree_list_porcelain(output: &str) -> Result<Vec<Worktree>, WorktreeError> {
    let mut worktrees = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_head = String::new();
    let mut current_branch: Option<String> = None;
    let mut is_bare = false;
    let mut is_detached = false;
    let mut locked_reason: Option<String> = None;
    let mut prunable_reason: Option<String> = None;

    let flush = |worktrees: &mut Vec<Worktree>,
                 path: &mut Option<PathBuf>,
                 head: &mut String,
                 branch: &mut Option<String>,
                 bare: &mut bool,
                 detached: &mut bool,
                 locked: &mut Option<String>,
                 prunable: &mut Option<String>| {
        if let Some(p) = path.take() {
            worktrees.push(Worktree {
                path: p,
                head: std::mem::take(head),
                branch: branch.take(),
                bare: *bare,
                detached: *detached,
                locked: locked.take(),
                prunable: prunable.take(),
            });
            *bare = false;
            *detached = false;
        }
    };

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush(
                &mut worktrees,
                &mut current_path,
                &mut current_head,
                &mut current_branch,
                &mut is_bare,
                &mut is_detached,
                &mut locked_reason,
                &mut prunable_reason,
            );
            continue;
        }

        if let Some(path_str) = line.strip_prefix("worktree ") {
            if current_path.is_some() {
                flush(
                    &mut worktrees,
                    &mut current_path,
                    &mut current_head,
                    &mut current_branch,
                    &mut is_bare,
                    &mut is_detached,
                    &mut locked_reason,
                    &mut prunable_reason,
                );
            }
            current_path = Some(native_git_path(&decode_git_quoted_path(path_str)?));
        } else if let Some(head_str) = trimmed.strip_prefix("HEAD ") {
            current_head = head_str.trim().to_string();
        } else if let Some(branch_str) = trimmed.strip_prefix("branch ") {
            current_branch = Some(branch_str.trim().to_string());
        } else if trimmed == "bare" {
            is_bare = true;
        } else if trimmed == "detached" {
            is_detached = true;
        } else if let Some(reason) = trimmed.strip_prefix("locked") {
            let reason_str = reason.trim();
            locked_reason = Some(if reason_str.is_empty() {
                "locked".to_string()
            } else {
                reason_str.to_string()
            });
        } else if let Some(reason) = trimmed.strip_prefix("prunable") {
            let reason_str = reason.trim();
            prunable_reason = Some(if reason_str.is_empty() {
                "prunable".to_string()
            } else {
                reason_str.to_string()
            });
        }
    }

    flush(
        &mut worktrees,
        &mut current_path,
        &mut current_head,
        &mut current_branch,
        &mut is_bare,
        &mut is_detached,
        &mut locked_reason,
        &mut prunable_reason,
    );

    Ok(worktrees)
}

/// Parses output from `git status --porcelain` or `git status --porcelain=v2`.
pub fn parse_status_porcelain(output: &str) -> DirtyState {
    let mut files = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("1 ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 8 {
                files.push(DirtyFile {
                    status_code: parts[0].to_string(),
                    path: parts[7..].join(" "),
                });
                continue;
            }
        }

        if let Some(rest) = trimmed.strip_prefix("2 ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 9 {
                files.push(DirtyFile {
                    status_code: parts[0].to_string(),
                    path: parts[8..].join(" "),
                });
                continue;
            }
        }

        if let Some(rest) = trimmed.strip_prefix("u ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 10 {
                files.push(DirtyFile {
                    status_code: parts[0].to_string(),
                    path: parts[9..].join(" "),
                });
                continue;
            }
        }

        if let Some(rest) = trimmed.strip_prefix("? ") {
            files.push(DirtyFile {
                status_code: "?".to_string(),
                path: rest.trim().to_string(),
            });
            continue;
        }

        if trimmed.starts_with("! ") {
            continue;
        }

        if line.len() >= 3 {
            let status_code = line[..2].to_string();
            let path = line[3..].trim().to_string();
            if !path.is_empty() {
                files.push(DirtyFile { status_code, path });
            }
        }
    }

    if files.is_empty() {
        DirtyState::clean()
    } else {
        DirtyState::dirty(files)
    }
}

pub fn git_worktree_add(
    repo_root: &Path,
    worktree_path: &Path,
    branch_name: &str,
    base_ref: Option<&str>,
) -> Result<(), WorktreeError> {
    validate_git_value(branch_name, "Branch name")?;
    let path_str = validate_git_path_argument(worktree_path)?;
    if let Some(base) = base_ref {
        validate_base_ref(repo_root, base)?;
    }

    let mut args = vec![
        "worktree",
        "add",
        "-b",
        branch_name,
        "--",
        path_str.as_str(),
    ];
    if let Some(base) = base_ref {
        args.push(base);
    }
    run_git(repo_root, &args)?;
    Ok(())
}

pub fn git_worktree_list(repo_root: &Path) -> Result<Vec<Worktree>, WorktreeError> {
    let output = run_git(repo_root, &["worktree", "list", "--porcelain"])?;
    parse_worktree_list_porcelain(&output)
}

/// Inspect a single worktree directory without listing every worktree in the repo.
pub fn inspect_worktree(path: &Path) -> Result<Worktree, WorktreeError> {
    let head = run_git(path, &["rev-parse", "HEAD"])?.trim().to_string();
    let branch = run_git(path, &["symbolic-ref", "-q", "HEAD"])
        .ok()
        .map(|output| output.trim().to_string())
        .filter(|value| !value.is_empty());
    let detached = branch.is_none();
    Ok(Worktree {
        path: path.to_path_buf(),
        head,
        branch,
        bare: false,
        detached,
        locked: None,
        prunable: None,
    })
}

/// Returns true when `ancestor` is an ancestor of `descendant` via `git merge-base --is-ancestor`.
pub fn git_merge_base_is_ancestor(
    repo_root: &Path,
    ancestor: &str,
    descendant: &str,
) -> Result<bool, WorktreeError> {
    validate_git_value(ancestor, "Ancestor ref")?;
    validate_git_value(descendant, "Descendant ref")?;
    match run_git(
        repo_root,
        &["merge-base", "--is-ancestor", ancestor, descendant],
    ) {
        Ok(_) => Ok(true),
        Err(WorktreeError::GitError { code: Some(1), .. }) => Ok(false),
        Err(error) => Err(error),
    }
}

/// Returns true when `branch` is an ancestor of HEAD (merged).
pub fn git_branch_is_ancestor_of_head(
    repo_root: &Path,
    branch: &str,
) -> Result<bool, WorktreeError> {
    git_merge_base_is_ancestor(repo_root, branch, "HEAD")
}

pub fn git_worktree_remove(
    repo_root: &Path,
    worktree_path: &Path,
    force: bool,
) -> Result<(), WorktreeError> {
    let path_str = validate_git_path_argument(worktree_path)?;
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.extend(["--", path_str.as_str()]);
    run_git(repo_root, &args)?;
    Ok(())
}

pub fn git_worktree_prune(repo_root: &Path) -> Result<(), WorktreeError> {
    run_git(repo_root, &["worktree", "prune"])?;
    Ok(())
}

pub fn git_status_porcelain(worktree_path: &Path) -> Result<DirtyState, WorktreeError> {
    let output = run_git(worktree_path, &["status", "--porcelain"])?;
    Ok(parse_status_porcelain(&output))
}

pub fn git_branch_delete(
    repo_root: &Path,
    branch_name: &str,
    force: bool,
) -> Result<(), WorktreeError> {
    validate_git_value(branch_name, "Branch name")?;
    let flag = if force { "-D" } else { "-d" };
    run_git(repo_root, &["branch", flag, "--", branch_name])?;
    Ok(())
}

pub fn git_remote_origin_url(repo_root: &Path) -> Option<String> {
    select_project_remote(&run_git(repo_root, &["remote", "-v"]).ok()?)
}

pub fn select_project_remote(remotes: &str) -> Option<String> {
    remotes
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?;
            let url = fields.next()?;
            if fields.next()? != "(fetch)" || !is_hosted_git_remote(url) {
                return None;
            }
            let priority = match name {
                "upstream" => 0,
                "origin" => 1,
                _ => 2,
            };
            Some((priority, name, url))
        })
        .min()
        .map(|(_, _, url)| url.to_string())
}

fn is_hosted_git_remote(remote: &str) -> bool {
    if remote.contains("://") {
        return tauri::Url::parse(remote).is_ok_and(|url| {
            matches!(url.scheme(), "ssh" | "git" | "https" | "http")
                && url.host_str().is_some()
                && !url.path().trim_matches('/').is_empty()
        });
    }
    let Some((host, path)) = remote.split_once(':') else {
        return false;
    };
    !host.is_empty()
        && !host.contains(['/', '\\'])
        && !(host.len() == 1 && path.starts_with(['/', '\\']))
        && !path.is_empty()
}

pub fn git_common_dir(repo_root: &Path) -> Option<PathBuf> {
    let common = run_git(repo_root, &["rev-parse", "--git-common-dir"]).ok()?;
    let common = Path::new(common.trim_end_matches(['\r', '\n']));
    std::fs::canonicalize(repo_root.join(common)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_path_argument_normalization_strips_windows_verbatim_prefixes() {
        assert_eq!(
            validate_git_path_argument(Path::new(
                r"\\?\C:\Users\orca\repo\.orca-worktrees\ws\task"
            ))
            .unwrap(),
            r"C:\Users\orca\repo\.orca-worktrees\ws\task"
        );
        assert_eq!(
            validate_git_path_argument(Path::new("//?/C:/Users/orca/repo/.orca-worktrees/ws/task"))
                .unwrap(),
            "C:/Users/orca/repo/.orca-worktrees/ws/task"
        );
        assert_eq!(
            validate_git_path_argument(Path::new(
                r"\\?\UNC\server\share\repo\.orca-worktrees\ws\task"
            ))
            .unwrap(),
            r"\\server\share\repo\.orca-worktrees\ws\task"
        );
        assert_eq!(
            validate_git_path_argument(Path::new(
                "//?/UNC/server/share/repo/.orca-worktrees/ws/task"
            ))
            .unwrap(),
            "//server/share/repo/.orca-worktrees/ws/task"
        );
        assert_eq!(
            validate_git_path_argument(Path::new(
                r"\\.\C:\Users\orca\repo\.orca-worktrees\ws\task"
            ))
            .unwrap(),
            r"C:\Users\orca\repo\.orca-worktrees\ws\task"
        );
        assert_eq!(
            validate_git_path_argument(Path::new(r"C:\Users\orca\repo\.orca-worktrees\ws\task"))
                .unwrap(),
            r"C:\Users\orca\repo\.orca-worktrees\ws\task"
        );
        assert_eq!(
            validate_git_path_argument(Path::new("/Users/orca/repo/.orca-worktrees/ws/task"))
                .unwrap(),
            "/Users/orca/repo/.orca-worktrees/ws/task"
        );
        assert!(validate_git_path_argument(Path::new("-bad-path")).is_err());
        assert!(validate_git_path_argument(Path::new("bad\npath")).is_err());
    }
}
