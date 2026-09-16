//! Machine-only project routes. Blocking transactions share the daemon admission gate.
pub(crate) mod worktrees;
use super::{
    auth::{DeviceAccessScope, DevicePermission},
    backend::RemoteSessionBackend,
    machine_operation_journal::{Begin, Record},
    machine_protocol::*,
    state::RemoteGatewayState,
};
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Arc};
use std::{sync::atomic::{AtomicBool, Ordering}, time::{Duration, Instant}};

struct RequestContext {
    cancelled: Arc<AtomicBool>,
    cancellation: Arc<tokio::sync::Notify>,
    deadline: Instant,
    revoked: tokio::sync::watch::Receiver<bool>,
}
pub(super) struct CancelWork(pub(super) Arc<AtomicBool>, pub(super) Arc<tokio::sync::Notify>);
impl Drop for CancelWork {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
        self.1.notify_one();
    }
}
impl RequestContext {
    fn check(&self) -> Result<(), String> {
        if *self.revoked.borrow() { return Err("UNAUTHORIZED".into()); }
        if self.cancelled.load(Ordering::Acquire) || Instant::now() >= self.deadline { return Err("TIMEOUT".into()); }
        Ok(())
    }
}

pub(super) struct Admission {
    pub(super) deadline: Instant,
    pub(super) revoked: tokio::sync::watch::Receiver<bool>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}
tokio::task_local! { pub(super) static ADMISSION: Arc<Admission>; }
pub(super) static AUTH_SLOTS: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> = std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(16)));

pub(super) async fn admit(state: Arc<RemoteGatewayState>, headers: HeaderMap, mutation: bool, id: &str) -> Result<Arc<Admission>, Response> {
    let deadline = Instant::now() + Duration::from_secs(if mutation { 40 } else { 35 });
    admit_until(state, headers, mutation, id, deadline).await
}

pub(super) async fn admit_until(state: Arc<RemoteGatewayState>, headers: HeaderMap, mutation: bool, id: &str, deadline: Instant) -> Result<Arc<Admission>, Response> {

    let auth_permit = match tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), AUTH_SLOTS.clone().acquire_owned()).await {
        Ok(Ok(p)) => p,
        _ => return Err(failure("TIMEOUT", id)),
    };
    let auth_state = state.clone();
    let auth_headers = headers.clone();
    let authenticated = crate::ipc::run_blocking(move || {
        let _permit = auth_permit;
        #[cfg(test)]
        if auth_state.machine_services.is_some() { probe(&auth_state, "authEntry"); }
        Ok(authorize(&auth_state, &auth_headers).and_then(|device| auth_state.auth_manager.device_revocation(&device).map_err(|_| "UNAUTHORIZED".into())))
    });
    let revoked = match tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), authenticated).await {
        Ok(Ok(Ok(revoked))) => revoked,
        Ok(Ok(Err(e))) => return Err(failure(&e, id)),
        Ok(Err(_)) => return Err(failure("MACHINE_SERVICE_UNAVAILABLE", id)),
        Err(_) => return Err(failure("TIMEOUT", id)),
    };
    let Some(services) = &state.machine_services else { return Err(failure("MACHINE_SERVICE_UNAVAILABLE", id)); };
    let slots = if mutation { &services.workspaces.project_mutations } else { &services.workspaces.project_reads };
    let permit = slots.clone().try_acquire_owned().map_err(|_| failure("CAPACITY_EXCEEDED", id))?;
    Ok(Arc::new(Admission { deadline, revoked, _permit: permit }))
}

async fn execute<F>(state: Arc<RemoteGatewayState>, headers: HeaderMap, mutation: bool, id: String, work: F) -> Response
where F: FnOnce(&RemoteGatewayState, &HeaderMap, &RequestContext) -> Response + Send + 'static {
    let admission = match ADMISSION.try_with(Arc::clone) {
        Ok(admission) => admission,
        Err(_) => match admit(state.clone(), headers.clone(), mutation, &id).await { Ok(a) => a, Err(e) => return e },
    };
    let deadline = admission.deadline;
    let cancel = CancelWork(Arc::new(AtomicBool::new(false)), Arc::new(tokio::sync::Notify::new()));
    let cancelled = cancel.0.clone();
    let cancellation = cancel.1.clone();
    let request = id.clone();
    let (auth_tx, auth_rx) = tokio::sync::oneshot::channel();
    let task = crate::ipc::run_blocking(move || Ok((|| {
        let revoked = admission.revoked.clone();
        let _admission = admission;
        if auth_tx.send(revoked.clone()).is_err() { return failure("TIMEOUT", &request); }
        let context = RequestContext { cancelled, cancellation, deadline, revoked };
        if let Err(e) = context.check() { return failure(&e, &request); }
        let response = work(&state, &headers, &context);
        #[cfg(test)]
        probe(&state, "beforeResponse");
        if let Err(e) = context.check().and_then(|_| authorize(&state, &headers).map(|_| ())) { return failure(&e, &request); }
        response
    })()));
    // run_blocking is lazy: spawn its async wrapper so authentication readiness
    // and the worker are polled concurrently, retaining the shared IO boundary.
    let task = tokio::spawn(task);
    let waiting = async {
        if let Ok(mut revoked) = auth_rx.await {
            tokio::select! {
                biased;
                _ = revoked.wait_for(|v| *v) => Ok(failure("UNAUTHORIZED", &id)),
                result = task => result.map_err(|_| ()).and_then(|r| r.map_err(|_| ())),
            }
        } else { task.await.map_err(|_| ()).and_then(|r| r.map_err(|_| ())) }
    };
    match tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), waiting).await {
        Ok(Ok(response)) => response,
        Ok(Err(_)) => failure("MACHINE_SERVICE_UNAVAILABLE", &id),
        Err(_) => failure("TIMEOUT", &id),
    }
}

#[cfg(test)]
fn probe(state: &RemoteGatewayState, phase: &str) {
    let hook = state.machine_services.as_ref().expect("services").workspaces.transaction_probe.read().clone();
    if let Some(hook) = hook { hook(phase); }
}

fn error(code: &str, request: &str) -> MachineError {
    MachineError {
        code: code.into(),
        message: code.into(),
        retryable: matches!(code, "MACHINE_SERVICE_UNAVAILABLE" | "HOST_UNAVAILABLE" | "TIMEOUT" | "CAPACITY_EXCEEDED" | "RATE_LIMITED"),
        request_id: request.into(),
        details: Default::default(),
    }
}
fn status(code: &str) -> u16 {
    match code {
        "UNAUTHORIZED" => 401,
        "MACHINE_ACCESS_REQUIRED" => 403,
        "DIRECTORY_NOT_FOUND" | "PROJECT_NOT_FOUND" | "WORKTREE_NOT_FOUND" | "OPERATION_NOT_FOUND" => 404,
        "REQUEST_CONFLICT"
        | "PROJECT_BUSY"
        | "WORKTREE_BUSY" | "DIRTY_WORKTREE" | "WORKTREE_LOCKED" | "UNMERGED_BRANCH" | "WORKTREE_EXISTS" | "WORKTREE_REMOVED_BRANCH_RETAINED"
        | "STALE_REVISION"
        | "OPERATION_OUTCOME_UNKNOWN"
        | "OPERATION_RESULT_EXPIRED" => 409,
        "UNSUPPORTED_PATH" | "NOT_A_GIT_REPOSITORY" | "BASE_REF_UNAVAILABLE" => 422,
        "CAPACITY_EXCEEDED" => 429,
        "MACHINE_SERVICE_UNAVAILABLE" => 503,
        "TIMEOUT" => 504,
        "PAYLOAD_TOO_LARGE" => 413,
        "PERMISSION_DENIED" => 403,
        _ => 400,
    }
}
fn response(s: u16, value: impl serde::Serialize) -> Response {
    (
        StatusCode::from_u16(s).expect("HTTP status"),
        [("cache-control", "no-store")],
        Json(value),
    )
        .into_response()
}
fn failure(code: &str, id: &str) -> Response {
    response(
        status(code),
        ErrorEnvelope {
            error: error(code, id),
        },
    )
}
fn authorize(state: &RemoteGatewayState, headers: &HeaderMap) -> Result<String, String> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or("UNAUTHORIZED")?;
    let device = state
        .auth_manager
        .validate_token(token)
        .map_err(|_| "UNAUTHORIZED")?;
    if device.access_scope != DeviceAccessScope::Machine
        || device.permission != DevicePermission::Control
    {
        return Err("MACHINE_ACCESS_REQUIRED".into());
    }
    if state.machine_services.is_none() {
        return Err("MACHINE_SERVICE_UNAVAILABLE".into());
    }
    Ok(device.id)
}
fn replay(r: Record) -> Response {
    match r.operation {
        Operation::Completed {
            outcome: OperationOutcome::Project { project },
            ..
        } => response(r.status, project),
        Operation::Completed { outcome: OperationOutcome::Worktree { worktree }, .. } => response(r.status, worktree),
        Operation::Completed {
            outcome: OperationOutcome::NoContent,
            ..
        } => (StatusCode::NO_CONTENT, [("cache-control", "no-store")]).into_response(),
        Operation::Completed {
            outcome: OperationOutcome::Error { error },
            ..
        } => response(r.status, ErrorEnvelope { error }),
        Operation::OutcomeUnknown { request_id } => {
            failure("OPERATION_OUTCOME_UNKNOWN", &request_id)
        }
        Operation::ResultExpired { request_id } => failure("OPERATION_RESULT_EXPIRED", &request_id),
        operation => response(r.status, operation),
    }
}
fn path(input: &str) -> Result<PathBuf, String> {
    // JSON paths are already decoded. Share browsing's native resolver, not its
    // query decoder; registration alone refuses filesystem roots.
    let home = if input == "~" || input.starts_with("~/") || input.is_empty() {
        PathBuf::from(std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .ok_or("UNSUPPORTED_PATH")?)
    } else {
        PathBuf::new()
    };
    let p = super::filesystem::resolve_directory(input, &home).map_err(|e| match e {
        super::filesystem::FsError::Invalid => "INVALID_PATH",
        super::filesystem::FsError::Missing => "DIRECTORY_NOT_FOUND",
        super::filesystem::FsError::Denied => "PERMISSION_DENIED",
        super::filesystem::FsError::Unsupported => "UNSUPPORTED_PATH",
        super::filesystem::FsError::Timeout => "TIMEOUT",
        super::filesystem::FsError::Unavailable => "MACHINE_SERVICE_UNAVAILABLE",
    })?;
    if p.parent().is_none() {
        return Err("INVALID_PATH".into());
    }
    std::fs::canonicalize(p).map_err(|_| "INVALID_PATH".into())
}
fn git(root: &std::path::Path, args: &[&str], context: &RequestContext) -> Result<Option<String>, String> {
    let mut command = tokio::process::Command::new("git");
    command.arg("-C").arg(root).args(args).env("GIT_TERMINAL_PROMPT", "0").env("GIT_OPTIONAL_LOCKS", "0");
    run_child(command, context)
}

#[cfg(all(test, unix))]
thread_local! { static CHILD_STARTED: std::cell::RefCell<Option<tokio::sync::oneshot::Sender<u32>>> = const { std::cell::RefCell::new(None) }; }

/// Synchronous domain callers may themselves be inside either Tokio runtime
/// flavor. Never nest block_on or depend on that runtime making progress: drive
/// the child supervisor on a scoped worker with its own IO/time driver instead.
/// Async transports must still put the enclosing transaction in run_blocking.
fn run_child(command: tokio::process::Command, context: &RequestContext) -> Result<Option<String>, String> {
    context.check()?;
    #[cfg(all(test, unix))]
    let started = CHILD_STARTED.with(|slot| slot.borrow_mut().take());
    std::thread::scope(|scope| {
        std::thread::Builder::new().name("workspace-child".into()).spawn_scoped(scope, move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()
                .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".to_string())?;
            #[cfg(all(test, unix))]
            CHILD_STARTED.with(|slot| *slot.borrow_mut() = started);
            runtime.block_on(run_child_async(command, context))
        }).map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".to_string())?
            .join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

async fn run_child_async(mut command: tokio::process::Command, context: &RequestContext) -> Result<Option<String>, String> {
        context.check()?;
        use tokio::io::AsyncReadExt;
        command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?;
        #[cfg(all(test, unix))]
        CHILD_STARTED.with(|slot| {
            if let Some(started) = slot.borrow_mut().take() { started.send(child.id().expect("active child")).expect("child observer"); }
        });
        let _stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        async fn bounded<R: tokio::io::AsyncRead + Unpin>(reader: R) -> Result<Vec<u8>, String> {
            let mut bytes = Vec::new();
            reader.take(65537).read_to_end(&mut bytes).await.map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?;
            if bytes.len() > 65536 { return Err("OUTPUT_LIMIT_EXCEEDED".into()); }
            Ok(bytes)
        }
        let mut revoked = context.revoked.clone();
        let deadline = context.deadline.min(Instant::now() + Duration::from_secs(5));
        let result = tokio::select! {
            biased;
            _ = revoked.wait_for(|v| *v) => Err("UNAUTHORIZED".into()),
            _ = context.cancellation.notified() => Err("TIMEOUT".into()),
            result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), async {
                let (out, _, status) = tokio::try_join!(bounded(stdout), bounded(stderr), async {child.wait().await.map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".to_string())})?;
                Ok::<_, String>((out, status))
            }) => result.map_err(|_| "TIMEOUT".to_string()).and_then(|r| r),
        };
        let (stdout, status) = match result {
            Ok(output) => output,
            Err(error) => {
                if child.try_wait().map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?.is_none() { child.start_kill().map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?; }
                child.wait().await.map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?;
                return Err(error);
            }
        };
        context.check()?;
        if !status.success() {
            return Ok(None);
        }
        let text = String::from_utf8(stdout).map_err(|_| "INVALID_PATH")?;
        Ok(Some(text.trim().to_owned()).filter(|s| !s.is_empty()))
}
#[cfg(all(test, unix))]
mod child_tests {
    use super::*;

    fn child_fixture() -> (tokio::sync::watch::Sender<bool>, RequestContext) {
        let (grant, revoked) = tokio::sync::watch::channel(false);
        (grant, RequestContext {
            cancelled: Arc::new(AtomicBool::new(false)),
            cancellation: Arc::new(tokio::sync::Notify::new()),
            deadline: Instant::now() + Duration::from_secs(10), revoked,
        })
    }

    fn assert_sync_child() {
        let (_grant, context) = child_fixture();
        let mut command = tokio::process::Command::new("/bin/echo");
        command.arg("runtime-independent");
        assert_eq!(run_child(command, &context).unwrap().as_deref(), Some("runtime-independent"));
    }

    #[test]
    fn child_without_runtime() { assert_sync_child(); }

    #[tokio::test]
    async fn child_from_current_thread_runtime() { assert_sync_child(); }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn child_from_multi_thread_runtime_and_blocking_worker() {
        assert_sync_child();
        crate::ipc::run_blocking(|| { assert_sync_child(); Ok(()) }).await.unwrap();
    }

    #[tokio::test]
    async fn async_child_cancellation_and_revocation_reap_active_process() {
        for revoke in [false, true] {
            let (grant, context) = child_fixture();
            let guard = CancelWork(context.cancelled.clone(), context.cancellation.clone());
            let (started, observed) = tokio::sync::oneshot::channel();
            let worker = tokio::task::spawn_blocking(move || {
                CHILD_STARTED.with(|slot| *slot.borrow_mut() = Some(started));
                let mut command = tokio::process::Command::new("/bin/cat");
                command.stdin(std::process::Stdio::piped());
                run_child(command, &context)
            });
            let pid = tokio::time::timeout(Duration::from_secs(5), observed).await.unwrap().unwrap();
            if revoke { grant.send(true).unwrap(); } else { drop(guard); }
            let result = tokio::time::timeout(Duration::from_secs(5), worker).await.unwrap().unwrap();
            assert_eq!(result.unwrap_err(), if revoke { "UNAUTHORIZED" } else { "TIMEOUT" });
            let mut status = 0;
            assert_eq!(unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) }, -1);
            assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ECHILD));
        }
    }

    #[tokio::test]
    async fn request_drop_interrupts_active_worktree_git() {
        let root = tokio::task::spawn_blocking(|| {
            let root = tempfile::tempdir().unwrap();
            let fifo = std::ffi::CString::new(root.path().join("blocked").to_str().unwrap()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
            root
        }).await.unwrap();
        let cwd = root.path().to_owned();
        let guard = CancelWork(Arc::new(AtomicBool::new(false)), Arc::new(tokio::sync::Notify::new()));
        let (_grant, revoked) = tokio::sync::watch::channel(false);
        let budget = crate::worktree::git::GitBudget {
            deadline: Instant::now() + Duration::from_secs(30), revoked,
            cancelled: guard.0.clone(), cancellation: Some(guard.1.clone()),
        };
        let (started, observed) = tokio::sync::oneshot::channel();
        let mut worker = tokio::task::spawn_blocking(move || {
            crate::worktree::git::CHILD_STARTED.with(|slot| *slot.borrow_mut() = Some(started));
            crate::worktree::git::with_git_budget(budget, || crate::worktree::git::run_git(cwd, &["hash-object", "blocked"]))
        });
        let observed = tokio::time::timeout(Duration::from_secs(5), observed).await;
        drop(guard);
        // Join even when the start observation failed; the runner's own deadline
        // bounds cleanup, and failure is asserted only after removing the root.
        let completed = tokio::time::timeout(Duration::from_secs(5), &mut worker).await;
        let prompt = completed.is_ok();
        let joined = match completed { Ok(result) => result, Err(_) => worker.await };
        tokio::task::spawn_blocking(move || root.close().unwrap()).await.unwrap();
        assert!(prompt, "request drop must interrupt Git before its 30-second deadline");
        let pid = observed.unwrap().unwrap();
        let result = joined.unwrap();
        assert!(matches!(result, Err(crate::worktree::WorktreeError::ParseError(ref code)) if code == "TIMEOUT"));
        let mut status = 0;
        assert_eq!(unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) }, -1);
        assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ECHILD));
        assert_eq!(unsafe { libc::kill(pid as libc::pid_t, 0) }, -1);
        assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
        eprintln!("A08 request_drop pid={pid} active_child_reaped=true worker_joined=true private_root_removed=true");
    }
    #[test]
    fn r12_child_output_and_deadline() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _enter = runtime.enter();
        let (_tx, revoked) = tokio::sync::watch::channel(false);
        let mut context = RequestContext { cancelled: Arc::new(AtomicBool::new(false)), cancellation: Arc::new(tokio::sync::Notify::new()), deadline: Instant::now() + Duration::from_secs(10), revoked };
        let mut flood = tokio::process::Command::new("/usr/bin/yes");
        flood.arg("bounded-fixture");
        assert_eq!(run_child(flood, &context).unwrap_err(), "OUTPUT_LIMIT_EXCEEDED");
        context.deadline = Instant::now() + Duration::from_millis(50);
        let mut blocked = tokio::process::Command::new("/bin/cat");
        blocked.stdin(std::process::Stdio::piped());
        assert_eq!(run_child(blocked, &context).unwrap_err(), "TIMEOUT");
        eprintln!("R12 child output_limit=65536 timeout=TIMEOUT both_children_waited=true");
    }
}

fn project(id: &str, root: &std::path::Path, revision: crate::scoped_contracts::Epoch, context: &RequestContext) -> Result<Project, String> {
    context.check()?;
    let availability = match std::fs::metadata(root) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Availability::Missing,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            Availability::PermissionDenied
        }
        Err(_) => Availability::Invalid,
        Ok(m) if !m.is_dir() => Availability::Invalid,
        _ => match std::fs::read_dir(root) {
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Availability::PermissionDenied,
            Err(_) => Availability::Invalid,
            Ok(_) => if std::fs::canonicalize(root).ok().as_deref() == Some(root) { Availability::Ready } else { Availability::Invalid },
        },
    };
    let mut p = Project {
        workspace_id: id.into(),
        repo_root: root.to_string_lossy().into(),
        git_root: None,
        git_common_dir: None,
        git_remote: None,
        git_branch: None,
        git_head: None,
        availability,
        revision,
    };
    if p.availability == Availability::Ready {
        let probe = (|| {
            if let Some(top) = git(root, &["rev-parse", "--show-toplevel"], context)? {
                if std::fs::canonicalize(top).ok().as_deref() != Some(root) { p.availability = Availability::Invalid; return Ok(()); }
                p.git_root = Some(p.repo_root.clone());
                p.git_common_dir = git(
                    root,
                    &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                    context,
                )?;
                p.git_branch = git(root, &["symbolic-ref", "--quiet", "--short", "HEAD"], context)?;
                p.git_head = git(root, &["rev-parse", "--verify", "HEAD"], context)?;
                p.git_remote =
                    git(root, &["config", "--get", "remote.origin.url"], context)?.and_then(|s| {
                        if let Ok(mut url) = reqwest::Url::parse(&s) {
                            let _ = url.set_username("");
                            let _ = url.set_password(None);
                            url.set_query(None);
                            url.set_fragment(None);
                            Some(url.to_string())
                        } else if s.contains('@') {
                            s.split_once('@').map(|(_, host)| host.to_owned())
                        } else {
                            Some(s)
                        }
                    });
            } else if root.join(".git").exists() {
                p.availability = Availability::Invalid;
            }
            Ok::<_, String>(())
        })();
        probe?;
    }
    context.check()?;
    Ok(p)
}
/// Reuse the same rich projection and child budget before the durable commit.
/// The subprocess adapter supports synchronous callers with or without Tokio.
pub(crate) fn registration_project(id: &str, root: &std::path::Path) -> Result<Project, String> {
    let (_grant, revoked) = tokio::sync::watch::channel(false);
    let context = RequestContext { cancelled: Arc::new(AtomicBool::new(false)), cancellation: Arc::new(tokio::sync::Notify::new()), deadline: Instant::now() + Duration::from_secs(10), revoked };
    project(id, root, crate::scoped_contracts::Epoch(0), &context)
}

pub(crate) async fn event_projects(service: Arc<crate::daemon::workspace_service::DaemonWorkspaceService>) -> Result<Projects, String> {
    let permit = service.project_reads.clone().try_acquire_owned().map_err(|_| "CAPACITY_EXCEEDED")?;
    let cancel = CancelWork(Arc::new(AtomicBool::new(false)), Arc::new(tokio::sync::Notify::new()));
    let (grant, revoked) = tokio::sync::watch::channel(false);
    let context = RequestContext { cancelled: cancel.0.clone(), cancellation: cancel.1.clone(), deadline: Instant::now() + Duration::from_secs(10), revoked };
    crate::ipc::run_blocking(move || {
        let (_permit, _grant) = (permit, grant);
        Ok(project_inventory(&service, &context))
    }).await.map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".to_owned())?
}

fn project_inventory(service: &crate::daemon::workspace_service::DaemonWorkspaceService, context: &RequestContext) -> Result<Projects, String> {
    let c = service.catalog()?;
    let mut projects = Vec::new();
    let mut completeness = Completeness::Complete;
    let mut failed = std::collections::BTreeSet::new();
    for (id, row) in &c.workspaces {
        match project(id, &row.repo_root, c.revision, context) {
            Ok(p) => projects.push(p),
            Err(error) => {
                context.check()?;
                tracing::warn!(%error, "Project inventory probe incomplete");
                completeness = Completeness::Partial;
                failed.insert(id.clone());
                projects.push(Project { workspace_id: id.clone(), repo_root: row.repo_root.to_str().ok_or("INVALID_PATH")?.into(),
                    git_root: None, git_common_dir: None, git_remote: None, git_branch: None, git_head: None,
                    availability: row.availability.clone(), revision: c.revision });
            }
        }
    }
    #[cfg(test)]
    if let Some(probe) = service.transaction_probe.read().clone() { probe("eventInventoryObserved"); }
    let _gate = service.mutation_gate.try_lock_until(context.deadline).ok_or("TIMEOUT")?;
    context.check()?;
    let mut current = service.catalog()?;
    // Never apply observations to a registration that changed while probing.
    if current.revision != c.revision { return Err("STALE_REVISION".into()); }
    let changed: Vec<_> = projects.iter().filter(|p| current.workspaces[&p.workspace_id].availability != p.availability).cloned().collect();
    if !changed.is_empty() {
        current.revision.0 = current.revision.0.checked_add(1).ok_or("CAPACITY_EXCEEDED")?;
        for p in &changed { current.workspaces.get_mut(&p.workspace_id).expect("observed registration").availability = p.availability.clone(); }
        if super::workspace_catalog::persist(&service.catalog_path, &current).is_err() {
            *service.catalog.lock() = Err("MACHINE_SERVICE_UNAVAILABLE".into());
            return Err("OPERATION_OUTCOME_UNKNOWN".into());
        }
        *service.catalog.lock() = Ok(current.clone());
        for mut p in changed {
            p.revision = current.revision;
            service.machine_events.publish_revision(current.revision.0, "projectAvailabilityChanged", Some(&p.workspace_id), None, serde_json::json!(p));
        }
        for p in &mut projects { p.revision = current.revision; }
    }
    let unavailable_workspace_ids = projects.iter().filter(|p| p.availability != Availability::Ready || failed.contains(&p.workspace_id)).map(|p| p.workspace_id.clone()).collect();
    let inventory = Projects { revision: current.revision, completeness, projects, unavailable_workspace_ids };
    if serde_json::to_vec(&inventory).map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?.len() > 1024 * 1024 { return Err("OUTPUT_LIMIT_EXCEEDED".into()); }
    Ok(inventory)
}

pub async fn list(State(state): State<Arc<RemoteGatewayState>>, headers: HeaderMap) -> Response {
    let id = uuid::Uuid::new_v4().to_string();
    execute(state, headers, false, id.clone(), move |state, headers, context| {
        let services = state
            .machine_services
            .as_ref()
            .expect("authorized services");
        let result = (|| {
            authorize(&state, &headers)?;
            #[cfg(test)]
            probe(state, "listProbe");
            project_inventory(&services.workspaces, context)
        })();
        match result { Ok(p) => response(200, p), Err(e) => failure(&e, &id) }
    }).await
}
pub async fn register(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    mutate(state, headers, body, None).await
}
pub async fn unregister(
    State(state): State<Arc<RemoteGatewayState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    mutate(state, headers, body, Some(id)).await
}
async fn mutate(
    state: Arc<RemoteGatewayState>,
    headers: HeaderMap,
    body: Bytes,
    workspace: Option<String>,
) -> Response {
    let fallback = uuid::Uuid::new_v4().to_string();
    let decoded = if workspace.is_some() {
        decode_json::<UnregisterRequest>(&body, MACHINE_JSON_MAX_BYTES)
            .map(|r| (r.request_id, None, Some(r.expected_revision)))
    } else {
        decode_json::<RegisterRequest>(&body, MACHINE_JSON_MAX_BYTES)
            .map(|r| (r.request_id, Some(r.repo_path), None))
    };
    let operation_id = decoded.as_ref().map(|r| r.0.clone()).unwrap_or_else(|_| fallback.clone());
    execute(state, headers, true, operation_id, move |state, headers, context| {
    let (id, repo, revision) = match decoded {
        Ok(r) => r,
        Err(DecodeError::PayloadTooLarge) => return failure("PAYLOAD_TOO_LARGE", &fallback),
        Err(_) => return failure("INVALID_REQUEST", &fallback),
    };
    let request = id.clone();
    let runtime = tokio::runtime::Handle::current();
        let services = state
            .machine_services
            .as_ref()
            .expect("authorized services");
        let service = &services.workspaces;
        let result = (|| {
            context.check()?;
            let device = authorize(&state, &headers)?;
            let kind = if workspace.is_some() {
                "unregisterProject"
            } else {
                "registerProject"
            };
            let digest = format!(
                "{:x}",
                Sha256::digest(
                    serde_json::to_vec(&(kind, &workspace, &repo, revision))
                        .expect("request tuple")
                )
            );
            let resource = workspace
                .clone()
                .unwrap_or_else(|| format!("project-{}", uuid::Uuid::new_v4().simple()));
            // Replay before probes, then prepare without holding the daemon-wide
            // publication gate. Other roots remain independently readable.
            if let Some(record) = service.journal.reconcile(&device, &request)? {
                if record.digest != digest || record.kind != kind { return Err("REQUEST_CONFLICT".into()); }
                return Ok(record);
            }
            let prepared = if let Some(repo) = &repo {
                #[cfg(test)]
                probe(state, "prepare");
                let root = path(repo)?;
                let root = match git(&root, &["rev-parse", "--show-toplevel"], context)? {
                    Some(top) => path(&top)?, None => root,
                };
                let p = project(&resource, &root, crate::scoped_contracts::Epoch(0), context)?;
                if p.availability == Availability::PermissionDenied { return Err("PERMISSION_DENIED".into()); }
                if p.availability != Availability::Ready { return Err("INVALID_PATH".into()); }
                Some((root, p))
            } else { None };
            // At most eight admitted workers may wait; no unbounded pool queue.
            #[cfg(test)]
            probe(state, "beforeGate");
            let workspace_gate = workspace.as_ref().map(|id| service.worktree_gate(id));
            let _workspace_gate = match &workspace_gate {
                Some(gate) => Some(gate.try_lock_until(context.deadline).ok_or("TIMEOUT")?),
                None => None,
            };
            let _gate = service.mutation_gate.try_lock_until(context.deadline).ok_or("TIMEOUT")?;
            context.check()?;
            authorize(state, headers)?;
            if let Begin::Existing(r) = service
                .journal
                .begin(&device, &request, kind, &digest, &resource)?
            {
                return Ok(r);
            }
            let outcome = (|| {
                let mut c = service
                    .catalog()
                    .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?;
                let (code, outcome) = if let Some(workspace) = workspace {
                    if revision != Some(c.revision) {
                        return Err("STALE_REVISION".into());
                    }
                    if !c.workspaces.contains_key(&workspace) {
                        return Err("PROJECT_NOT_FOUND".into());
                    }
                    runtime.block_on(async {
                        for session in services.sessions.list_sessions().await {
                            let d = services
                                .sessions
                                .describe_session(&session)
                                .await
                                .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?;
                            if d.running && d.workspace_id.as_deref() == Some(&workspace) {
                                return Err("PROJECT_BUSY");
                            }
                        }
                        Ok(())
                    })?;
                    authorize(&state, &headers)?;
                    c.workspaces.remove(&workspace);
                    (204, OperationOutcome::NoContent)
                } else {
                    let (root, prepared_project) = prepared.expect("register projection");
                    if path(&crate::worktree::strip_verbatim_prefix(root.to_str().ok_or("INVALID_PATH")?))? != root { return Err("INVALID_PATH".into()); }
                    // A stable native pathname is not a stable Git identity.
                    // Refresh under admission using the original request budget;
                    // project rejects a root now nested in another repository.
                    let mut p = project(&prepared_project.workspace_id, &root, c.revision, context)?;
                    if p.availability == Availability::PermissionDenied { return Err("PERMISSION_DENIED".into()); }
                    if p.availability != Availability::Ready { return Err("INVALID_PATH".into()); }
                    let root = root.as_path();
                    if root.parent().is_none() {
                        return Err("INVALID_PATH".into());
                    }
                    if let Some((existing, row)) =
                        c.workspaces.iter().find(|(_, r)| r.repo_root == root)
                    {
                        p.workspace_id = existing.clone();
                        p.repo_root = row.repo_root.to_str().ok_or("INVALID_PATH")?.into();
                        p.revision = c.revision;
                        return Ok((
                            200,
                            OperationOutcome::Project {
                                project: p,
                            },
                        ));
                    }
                    authorize(&state, &headers)?;
                    c.workspaces.insert(
                        resource.clone(),
                        super::workspace_catalog::CatalogRow {
                            repo_root: root.to_owned(),
                            mirror_exposed: false,
                            availability: Availability::Ready,
                        },
                    );
                    p.revision = crate::scoped_contracts::Epoch(c.revision.0.checked_add(1).ok_or("CAPACITY_EXCEEDED")?);
                    (201, OperationOutcome::Project { project: p })
                };
                c.revision.0 = c.revision.0.checked_add(1).ok_or("CAPACITY_EXCEEDED")?;
                c.transaction = Some(service.journal.catalog_receipt(&device, &request, code, outcome.clone())?);
                #[cfg(test)]
                probe(state, "beforeCommit");
                context.check()?;
                authorize(state, headers)?;
                if super::workspace_catalog::persist(&service.catalog_path, &c).is_err() {
                    *service.catalog.lock() = Err("MACHINE_SERVICE_UNAVAILABLE".into());
                    return Err("OPERATION_OUTCOME_UNKNOWN".into());
                }
                let revision = c.revision.0;
                *service.catalog.lock() = Ok(c);
                match &outcome {
                    OperationOutcome::Project { project } => service.machine_events.publish_revision(revision, "projectRegistered", Some(&project.workspace_id), None, serde_json::json!(project)),
                    OperationOutcome::NoContent => service.machine_events.publish_revision(revision, "projectRemoved", Some(&resource), None, serde_json::json!({})),
                    _ => {},
                }
                #[cfg(test)]
                probe(state, "afterCatalog");
                if code == 204 {
                    service.registry.unregister(&resource);
                }
                Ok::<_, String>((code, outcome))
            })();
            let (code, outcome) = match outcome {
                Ok(v) => v,
                Err(e) if e == "OPERATION_OUTCOME_UNKNOWN" => return Err(e),
                Err(e) => (
                    status(&e),
                    OperationOutcome::Error {
                        error: error(&e, &request),
                    },
                ),
            };
            let completed = service.journal.complete(&device, &request, code, outcome);
            if completed.is_err() { *service.catalog.lock() = Err("MACHINE_SERVICE_UNAVAILABLE".into()); }
            #[cfg(test)]
            probe(state, "afterJournal");
            completed
        })();
        match result { Ok(r) => replay(r), Err(e) => failure(&e, &id) }
    }).await
}
pub async fn operation(
    State(state): State<Arc<RemoteGatewayState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    execute(state, headers, false, id.clone(), move |state, headers, _context| {
    let device = match authorize(&state, &headers) {
        Ok(d) => d,
        Err(e) => return failure(&e, &uuid::Uuid::new_v4().to_string()),
    };
    if uuid::Uuid::parse_str(&id).is_err() {
        return failure("INVALID_REQUEST", &uuid::Uuid::new_v4().to_string());
    }
    match state
        .machine_services
        .as_ref()
        .expect("services")
        .workspaces
        .journal
        .reconcile(&device, &id)
    {
        Ok(Some(r)) => match r.operation {
            Operation::Pending { .. } => response(202, r.operation),
            Operation::OutcomeUnknown { .. } => failure("OPERATION_OUTCOME_UNKNOWN", &id),
            Operation::ResultExpired { .. } => failure("OPERATION_RESULT_EXPIRED", &id),
            operation => response(200, operation),
        },
        Ok(None) => failure("OPERATION_NOT_FOUND", &id),
        Err(e) => failure(&e, &id),
    }
    }).await
}
