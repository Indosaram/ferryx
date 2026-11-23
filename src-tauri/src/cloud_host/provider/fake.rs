//! An in-memory [`SandboxProvider`] for unit tests. Nothing is provisioned and no network
//! call is made; state lives in one process.
//!
//! Semantics mirror what a real provider must guarantee, so a test that passes here is a
//! statement about the contract and not only about the fake:
//!
//! - `create` is idempotent per key: the same request returns the same id, and a different
//!   request under a used key is a `Conflict`.
//! - a fresh machine is `Provisioning` and turns `Ready` on its first `get`; `stop` passes
//!   through `Stopping` and settles on `Stopped` at the next `get`. [`FakeProvider::with_stopped_on_create`]
//!   flips that first state to `Stopped` (the Fly Sprites `cold`/`warm` shape) without changing
//!   anything else: `get` still never wakes a machine there, and `exec` is the wake.
//! - `get` and `list` never wake a machine.
//! - `delete` leaves `Deleted`, which `list` omits and every other call reports `NotFound`.
//!
//! `write_file` and `ssh_target` are deliberately left at their trait defaults (both report
//! `Config`), because a fake serves one transport at a time and a test that needs a file API
//! must say so with its own double instead of getting one by accident.
//!
//! Every call is appended to a log in the order it arrived, so a test can assert that a
//! lifecycle path did not wake anything (`no call was Resume`) or that `create` ran before
//! `exec`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicI64, Ordering};

use parking_lot::Mutex;

use super::{
    Capabilities, CreateSandbox, ErrorKind, ExecResult, ListedSandbox, SandboxError, SandboxInfo,
    SandboxProvider, SandboxState, Transport,
};

/// One recorded call, in arrival order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeCall {
    Create { idempotency_key: String, name: String, ttl_seconds: Option<u64> },
    Get { sandbox_id: String },
    Stop { sandbox_id: String },
    Resume { sandbox_id: String, ttl_seconds: Option<u64> },
    Delete { sandbox_id: String },
    ExtendTtl { sandbox_id: String, ttl_seconds: Option<u64> },
    Exec { sandbox_id: String, command: String },
    List,
}

impl FakeCall {
    /// Method name, so a test can assert on the sequence without matching payloads.
    pub fn method(&self) -> &'static str {
        match self {
            FakeCall::Create { .. } => "create",
            FakeCall::Get { .. } => "get",
            FakeCall::Stop { .. } => "stop",
            FakeCall::Resume { .. } => "resume",
            FakeCall::Delete { .. } => "delete",
            FakeCall::ExtendTtl { .. } => "extend_ttl",
            FakeCall::Exec { .. } => "exec",
            FakeCall::List => "list",
        }
    }
}

#[derive(Debug, Clone)]
struct FakeMachine {
    id: String,
    name: String,
    state: SandboxState,
    created_at: i64,
    ttl_seconds: Option<u64>,
    env: BTreeMap<String, String>,
    execs: Vec<String>,
}

#[derive(Default)]
struct Inner {
    machines: BTreeMap<String, FakeMachine>,
    /// idempotency key -> the machine it created, plus the request fingerprint it created it
    /// from (a reused key with a different request is the caller's bug, not a second machine).
    keys: BTreeMap<String, (String, String)>,
    calls: Vec<FakeCall>,
    /// One-shot injections, drained one per call.
    fail_next: Vec<ErrorKind>,
}

pub struct FakeProvider {
    capabilities: Capabilities,
    inner: Mutex<Inner>,
    /// Deterministic millisecond clock: every machine is one second newer than the last, so
    /// `created_at` orders without a wall clock and a test never races a real clock.
    clock: AtomicI64,
    next_id: AtomicI64,
    /// The Fly Sprites shape: a machine created here starts `Stopped` and is woken by `exec`,
    /// not by `get`. `false` for every provider whose `Stopped` means "asleep until `resume`".
    stopped_on_create: bool,
}

impl Default for FakeProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Epoch base for the deterministic clock (2025-01-01T00:00:00Z).
const CLOCK_BASE_MS: i64 = 1_735_689_600_000;

impl FakeProvider {
    /// A relay-transport provider with no TTL requirement and no native auto-stop: the
    /// weakest shape, so a caller that works here must have supplied its own guarantees.
    pub fn new() -> Self {
        Self::with_capabilities(Capabilities {
            ttl_required: false,
            max_ttl_seconds: None,
            transport: Transport::Relay,
            native_auto_stop: false,
        })
    }

    /// A T1 provider: SSH endpoint, provider-native auto-stop, optional TTL.
    pub fn ssh() -> Self {
        Self::with_capabilities(Capabilities {
            ttl_required: false,
            max_ttl_seconds: Some(6 * 60 * 60),
            transport: Transport::Ssh,
            native_auto_stop: true,
        })
    }

    pub fn with_capabilities(capabilities: Capabilities) -> Self {
        Self {
            capabilities,
            inner: Mutex::new(Inner::default()),
            clock: AtomicI64::new(CLOCK_BASE_MS),
            next_id: AtomicI64::new(1),
            stopped_on_create: false,
        }
    }

    /// Behave like Fly Sprites for a newly created machine: it answers `Stopped` from the moment
    /// it exists (a `cold`/`warm` Sprite), `get` keeps reporting that, and `exec` is what wakes it.
    ///
    /// This is the one shape where [`SandboxProvider::accepts_stopped_as_ready_on_create`] is true,
    /// so a create path that insists on `Ready` can be proven to spend its budget here.
    pub fn with_stopped_on_create(mut self) -> Self {
        self.stopped_on_create = true;
        self
    }

    /// Make the next call fail with `kind` (no state changes, nothing recorded).
    pub fn fail_next(&self, kind: ErrorKind) {
        self.fail_next_times(kind, 1);
    }

    /// Make the next `n` calls fail with `kind`.
    pub fn fail_next_times(&self, kind: ErrorKind, n: usize) {
        let mut inner = self.inner.lock();
        for _ in 0..n {
            inner.fail_next.push(kind);
        }
    }

    /// Every call so far, in arrival order.
    pub fn calls(&self) -> Vec<FakeCall> {
        self.inner.lock().calls.clone()
    }

    /// Method names of every call so far, in arrival order.
    pub fn call_methods(&self) -> Vec<&'static str> {
        self.inner.lock().calls.iter().map(FakeCall::method).collect()
    }

    /// Commands a machine ran, in order.
    pub fn execs_of(&self, sandbox_id: &str) -> Vec<String> {
        self.inner
            .lock()
            .machines
            .get(sandbox_id)
            .map(|machine| machine.execs.clone())
            .unwrap_or_default()
    }

    /// The `env` a machine was created with.
    pub fn env_of(&self, sandbox_id: &str) -> BTreeMap<String, String> {
        self.inner
            .lock()
            .machines
            .get(sandbox_id)
            .map(|machine| machine.env.clone())
            .unwrap_or_default()
    }

    /// The auto-stop a machine currently carries, as last set by create/resume/extend_ttl.
    pub fn ttl_of(&self, sandbox_id: &str) -> Option<Option<u64>> {
        self.inner.lock().machines.get(sandbox_id).map(|machine| machine.ttl_seconds)
    }

    /// The id a machine was created under, for a key.
    pub fn id_for_key(&self, idempotency_key: &str) -> Option<String> {
        self.inner.lock().keys.get(idempotency_key).map(|(id, _)| id.clone())
    }

    /// Ids of every machine the provider still knows, in creation order.
    pub fn live_ids(&self) -> BTreeSet<String> {
        self.inner
            .lock()
            .machines
            .values()
            .filter(|machine| machine.state != SandboxState::Deleted)
            .map(|machine| machine.id.clone())
            .collect()
    }

    fn injected(&self) -> Option<SandboxError> {
        let mut inner = self.inner.lock();
        if inner.fail_next.is_empty() {
            return None;
        }
        let kind = inner.fail_next.remove(0);
        Some(SandboxError::new(kind, "fake", "injected", format!("injected {kind} failure")))
    }

    fn record(&self, call: FakeCall) {
        self.inner.lock().calls.push(call);
    }

    fn not_found(sandbox_id: &str) -> SandboxError {
        SandboxError::new(
            ErrorKind::NotFound,
            "fake",
            "sandbox_not_found",
            format!("no sandbox {sandbox_id}"),
        )
    }

    /// Resolve a live machine, refusing a deleted or unknown id.
    fn find(inner: &Inner, sandbox_id: &str) -> Result<FakeMachine, SandboxError> {
        match inner.machines.get(sandbox_id) {
            Some(machine) if machine.state != SandboxState::Deleted => Ok(machine.clone()),
            _ => Err(Self::not_found(sandbox_id)),
        }
    }

    fn tick(&self) -> i64 {
        self.clock.fetch_add(1_000, Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl SandboxProvider for FakeProvider {
    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    fn accepts_stopped_as_ready_on_create(&self) -> bool {
        self.stopped_on_create
    }

    async fn create(&self, request: CreateSandbox) -> Result<String, SandboxError> {
        if let Some(error) = self.injected() {
            return Err(error);
        }
        if self.capabilities.ttl_required && request.ttl_seconds.is_none() {
            return Err(SandboxError::new(
                ErrorKind::Config,
                "fake",
                "ttl_required",
                "this provider cannot disable auto-stop; pass a TTL",
            ));
        }
        let fingerprint = format!("{}|{:?}|{:?}", request.name, request.ttl_seconds, request.env);
        self.record(FakeCall::Create {
            idempotency_key: request.idempotency_key.clone(),
            name: request.name.clone(),
            ttl_seconds: request.ttl_seconds,
        });

        let mut inner = self.inner.lock();
        if let Some((id, prior)) = inner.keys.get(&request.idempotency_key) {
            if prior != &fingerprint {
                return Err(SandboxError::new(
                    ErrorKind::Conflict,
                    "fake",
                    "idempotency_key_reused",
                    format!("idempotency key {} was used with a different request", request.idempotency_key),
                ));
            }
            let id = id.clone();
            // A repeat create of a live machine is a no-op that returns its handle.
            Self::find(&inner, &id)?;
            return Ok(id);
        }

        let n = self.next_id.fetch_add(1, Ordering::SeqCst);
        let id = format!("fake-{n}");
        let created_at = self.tick();
        // A cold/warm Sprite reports `Stopped` immediately; everything else is still coming up.
        let state = if self.stopped_on_create {
            SandboxState::Stopped
        } else {
            SandboxState::Provisioning
        };
        inner.machines.insert(
            id.clone(),
            FakeMachine {
                id: id.clone(),
                name: request.name,
                state,
                created_at,
                ttl_seconds: request.ttl_seconds,
                env: request.env,
                execs: Vec::new(),
            },
        );
        inner.keys.insert(request.idempotency_key, (id.clone(), fingerprint));
        Ok(id)
    }

    async fn get(&self, sandbox_id: &str) -> Result<SandboxInfo, SandboxError> {
        if let Some(error) = self.injected() {
            return Err(error);
        }
        self.record(FakeCall::Get { sandbox_id: sandbox_id.to_string() });
        let mut inner = self.inner.lock();
        let machine = Self::find(&inner, sandbox_id)?;
        // Promotion only: a status check reports the transition a wake already started, and
        // never starts one.
        let settled = match machine.state {
            SandboxState::Provisioning => Some(SandboxState::Ready),
            SandboxState::Stopping => Some(SandboxState::Stopped),
            other => Some(other),
        };
        let state = settled.expect("every state settles");
        if state != machine.state {
            if let Some(stored) = inner.machines.get_mut(sandbox_id) {
                stored.state = state;
            }
        }
        Ok(SandboxInfo { state, error: None })
    }

    async fn stop(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        if let Some(error) = self.injected() {
            return Err(error);
        }
        self.record(FakeCall::Stop { sandbox_id: sandbox_id.to_string() });
        let mut inner = self.inner.lock();
        let machine = Self::find(&inner, sandbox_id)?;
        match machine.state {
            SandboxState::Stopped | SandboxState::Stopping => Ok(()),
            SandboxState::Provisioning | SandboxState::Ready | SandboxState::Error => {
                if let Some(stored) = inner.machines.get_mut(sandbox_id) {
                    stored.state = SandboxState::Stopping;
                }
                Ok(())
            }
            SandboxState::Deleted => Err(Self::not_found(sandbox_id)),
        }
    }

    async fn resume(&self, sandbox_id: &str, ttl_seconds: Option<u64>) -> Result<(), SandboxError> {
        if let Some(error) = self.injected() {
            return Err(error);
        }
        self.record(FakeCall::Resume {
            sandbox_id: sandbox_id.to_string(),
            ttl_seconds,
        });
        let mut inner = self.inner.lock();
        let machine = Self::find(&inner, sandbox_id)?;
        if machine.state != SandboxState::Stopped {
            return Err(SandboxError::new(
                ErrorKind::Conflict,
                "fake",
                "not_stopped",
                format!("cannot resume a {} sandbox", machine.state),
            ));
        }
        if let Some(stored) = inner.machines.get_mut(sandbox_id) {
            stored.state = SandboxState::Provisioning;
            stored.ttl_seconds = ttl_seconds;
        }
        Ok(())
    }

    async fn delete(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        if let Some(error) = self.injected() {
            return Err(error);
        }
        self.record(FakeCall::Delete { sandbox_id: sandbox_id.to_string() });
        let mut inner = self.inner.lock();
        Self::find(&inner, sandbox_id)?;
        if let Some(stored) = inner.machines.get_mut(sandbox_id) {
            stored.state = SandboxState::Deleted;
        }
        Ok(())
    }

    async fn extend_ttl(
        &self,
        sandbox_id: &str,
        ttl_seconds: Option<u64>,
    ) -> Result<(), SandboxError> {
        if let Some(error) = self.injected() {
            return Err(error);
        }
        self.record(FakeCall::ExtendTtl {
            sandbox_id: sandbox_id.to_string(),
            ttl_seconds,
        });
        let mut inner = self.inner.lock();
        Self::find(&inner, sandbox_id)?;
        if let Some(stored) = inner.machines.get_mut(sandbox_id) {
            stored.ttl_seconds = ttl_seconds;
        }
        Ok(())
    }

    async fn exec(
        &self,
        sandbox_id: &str,
        command: &str,
        timeout_seconds: u64,
    ) -> Result<ExecResult, SandboxError> {
        let _ = timeout_seconds;
        if let Some(error) = self.injected() {
            return Err(error);
        }
        self.record(FakeCall::Exec { sandbox_id: sandbox_id.to_string(), command: command.to_string() });
        let mut inner = self.inner.lock();
        let machine = Self::find(&inner, sandbox_id)?;
        if machine.state != SandboxState::Ready {
            // A cold/warm Sprite is woken by the request itself, which is exactly why a create
            // that accepts `Stopped` as ready can still run its bootstrap. No other `Stopped`
            // machine is wakeable without `resume`, so it stays a `Conflict`.
            if !(self.stopped_on_create && machine.state == SandboxState::Stopped) {
                return Err(SandboxError::new(
                    ErrorKind::Conflict,
                    "fake",
                    "not_ready",
                    format!("sandbox is {}", machine.state),
                ));
            }
            if let Some(stored) = inner.machines.get_mut(sandbox_id) {
                stored.state = SandboxState::Ready;
            }
        }
        if let Some(stored) = inner.machines.get_mut(sandbox_id) {
            stored.execs.push(command.to_string());
        }
        Ok(ExecResult { exit_code: Some(0), stdout: String::new(), stderr: String::new(), timed_out: false })
    }

    async fn list(&self) -> Result<Vec<ListedSandbox>, SandboxError> {
        if let Some(error) = self.injected() {
            return Err(error);
        }
        self.record(FakeCall::List);
        let inner = self.inner.lock();
        Ok(inner
            .machines
            .values()
            .filter(|machine| machine.state != SandboxState::Deleted)
            .map(|machine| ListedSandbox {
                sandbox_id: machine.id.clone(),
                state: machine.state,
                created_at: Some(machine.created_at),
                name: Some(machine.name.clone()),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(key: &str) -> CreateSandbox {
        CreateSandbox {
            idempotency_key: key.to_string(),
            name: key.to_string(),
            ttl_seconds: Some(1_800),
            env: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn create_is_idempotent_per_key() {
        let provider = FakeProvider::new();
        let first = provider.create(request("fx-a-1")).await.expect("first create");
        let second = provider.create(request("fx-a-1")).await.expect("repeat create");

        assert_eq!(first, second);
        assert_eq!(provider.live_ids(), BTreeSet::from([first.clone()]));
        assert_eq!(provider.id_for_key("fx-a-1").as_deref(), Some(first.as_str()));

        // A second key is a second machine; that is what the key is for.
        let other = provider.create(request("fx-b-1")).await.expect("second key");
        assert_ne!(other, first);
    }

    #[tokio::test]
    async fn reusing_a_key_with_a_different_request_conflicts() {
        let provider = FakeProvider::new();
        provider.create(request("fx-a-1")).await.expect("first create");

        let mut changed = request("fx-a-1");
        changed.ttl_seconds = Some(600);
        let error = provider.create(changed).await.expect_err("different request under a used key");
        assert_eq!(error.kind, ErrorKind::Conflict);
        assert_eq!(error.provider_code, "idempotency_key_reused");
        assert_eq!(provider.live_ids().len(), 1);
    }

    #[tokio::test]
    async fn injected_not_found_surfaces_with_kind_not_found() {
        let provider = FakeProvider::new();
        let id = provider.create(request("fx-a-1")).await.expect("create");

        provider.fail_next(ErrorKind::NotFound);
        let error = provider.get(&id).await.expect_err("injected NotFound");
        assert_eq!(error.kind, ErrorKind::NotFound);
        assert!(error.is_not_found());
        assert_eq!(error.provider, "fake");

        // The injection is one-shot: the same call succeeds afterwards.
        assert_eq!(provider.get(&id).await.expect("get after injection").state, SandboxState::Ready);
    }

    #[tokio::test]
    async fn an_unknown_id_is_not_found() {
        let provider = FakeProvider::new();
        for result in [
            provider.get("fake-404").await.map(|_| ()),
            provider.stop("fake-404").await,
            provider.resume("fake-404", None).await,
            provider.delete("fake-404").await,
            provider.extend_ttl("fake-404", None).await,
            provider.exec("fake-404", "true", 30).await.map(|_| ()),
        ] {
            assert_eq!(result.expect_err("unknown id").kind, ErrorKind::NotFound);
        }
    }

    #[tokio::test]
    async fn list_returns_only_created_not_deleted_ids() {
        let provider = FakeProvider::new();
        let keep = provider.create(request("fx-keep-1")).await.expect("create keep");
        let gone = provider.create(request("fx-gone-1")).await.expect("create gone");
        provider.delete(&gone).await.expect("delete");

        let listed: Vec<String> = provider
            .list()
            .await
            .expect("list")
            .into_iter()
            .map(|row| row.sandbox_id)
            .collect();
        assert_eq!(listed, vec![keep.clone()]);
        assert!(!listed.contains(&gone));
        assert_eq!(provider.live_ids(), BTreeSet::from([keep]));
    }

    #[tokio::test]
    async fn deleted_machine_reports_not_found_everywhere() {
        let provider = FakeProvider::new();
        let id = provider.create(request("fx-a-1")).await.expect("create");
        provider.delete(&id).await.expect("delete");

        assert_eq!(provider.get(&id).await.expect_err("deleted get").kind, ErrorKind::NotFound);
        assert_eq!(provider.delete(&id).await.expect_err("double delete").kind, ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn get_settles_provisioning_and_stop_settles_to_stopped_without_resuming() {
        let provider = FakeProvider::new();
        let id = provider.create(request("fx-a-1")).await.expect("create");

        assert_eq!(provider.get(&id).await.unwrap().state, SandboxState::Ready);
        provider.stop(&id).await.expect("stop");
        assert_eq!(provider.get(&id).await.unwrap().state, SandboxState::Stopped);
        assert_eq!(provider.get(&id).await.unwrap().state, SandboxState::Stopped);

        // Stopping twice is a no-op, and neither `get` nor `stop` woke anything.
        provider.stop(&id).await.expect("stop again");
        assert!(!provider.call_methods().contains(&"resume"), "no status path may resume");

        provider.resume(&id, Some(600)).await.expect("resume");
        assert_eq!(provider.get(&id).await.unwrap().state, SandboxState::Ready);
        assert_eq!(provider.ttl_of(&id), Some(Some(600)));
    }

    #[tokio::test]
    async fn resume_of_a_ready_machine_conflicts() {
        let provider = FakeProvider::new();
        let id = provider.create(request("fx-a-1")).await.expect("create");
        provider.get(&id).await.expect("promote");

        let error = provider.resume(&id, None).await.expect_err("resume a ready machine");
        assert_eq!(error.kind, ErrorKind::Conflict);
        assert_eq!(error.provider_code, "not_stopped");
    }

    #[tokio::test]
    async fn ttl_required_provider_refuses_a_missing_ttl() {
        let provider = FakeProvider::with_capabilities(Capabilities {
            ttl_required: true,
            max_ttl_seconds: Some(3_600),
            transport: Transport::Relay,
            native_auto_stop: true,
        });
        let mut none = request("fx-a-1");
        none.ttl_seconds = None;
        let error = provider.create(none).await.expect_err("ttl is required");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "ttl_required");
        assert!(provider.live_ids().is_empty());
    }

    #[tokio::test]
    async fn every_call_is_recorded_in_order() {
        let provider = FakeProvider::new();
        let id = provider.create(request("fx-a-1")).await.expect("create");
        provider.get(&id).await.expect("get");
        provider.exec(&id, "uname -m", 30).await.expect("exec");
        provider.extend_ttl(&id, Some(900)).await.expect("extend");
        provider.delete(&id).await.expect("delete");
        provider.list().await.expect("list");

        assert_eq!(
            provider.call_methods(),
            vec!["create", "get", "exec", "extend_ttl", "delete", "list"]
        );
        assert_eq!(provider.execs_of(&id), vec!["uname -m".to_string()]);
        assert_eq!(provider.ttl_of(&id), Some(Some(900)));
    }

    #[tokio::test]
    async fn exec_refuses_a_machine_that_is_not_ready() {
        let provider = FakeProvider::new();
        let id = provider.create(request("fx-a-1")).await.expect("create");

        // Still Provisioning: the first `get` is what makes a fake machine Ready.
        let error = provider.exec(&id, "true", 30).await.expect_err("not ready yet");
        assert_eq!(error.kind, ErrorKind::Conflict);
        assert_eq!(error.provider_code, "not_ready");
        assert!(provider.execs_of(&id).is_empty());
    }

    /// Fly Sprites as it really is on create: `cold`/`warm`, i.e. `Stopped`, and the first request
    /// for the machine is what starts it. A status read is still not a wake.
    #[tokio::test]
    async fn a_stopped_on_create_machine_starts_stopped_and_is_woken_by_exec() {
        let provider = FakeProvider::new().with_stopped_on_create();
        assert!(provider.accepts_stopped_as_ready_on_create());
        let id = provider.create(request("fx-a-1")).await.expect("create");

        assert_eq!(provider.get(&id).await.unwrap().state, SandboxState::Stopped);
        assert_eq!(
            provider.get(&id).await.unwrap().state,
            SandboxState::Stopped,
            "a status read must not wake it"
        );
        provider.exec(&id, "true", 30).await.expect("the first command wakes it");
        assert_eq!(provider.get(&id).await.unwrap().state, SandboxState::Ready);
        assert_eq!(provider.execs_of(&id), vec!["true".to_string()]);
    }

    /// The flag is opt-in, and opt-in only where it is true: every other provider's `Stopped` means
    /// asleep until `resume`, so `exec` must keep refusing it.
    #[tokio::test]
    async fn stopped_counts_as_ready_on_create_only_when_asked_for() {
        assert!(!FakeProvider::new().accepts_stopped_as_ready_on_create());
        assert!(!FakeProvider::ssh().accepts_stopped_as_ready_on_create());
        assert!(FakeProvider::new().with_stopped_on_create().accepts_stopped_as_ready_on_create());

        let provider = FakeProvider::new();
        let id = provider.create(request("fx-a-1")).await.expect("create");
        provider.get(&id).await.expect("promote");
        provider.stop(&id).await.expect("stop");
        // Settle `Stopping` into `Stopped`: the state a paused machine really reports.
        assert_eq!(provider.get(&id).await.expect("settle").state, SandboxState::Stopped);
        let error = provider.exec(&id, "true", 30).await.expect_err("asleep is not workable");
        assert_eq!(error.provider_code, "not_ready");
    }

    #[test]
    fn created_at_orders_without_a_wall_clock() {
        let provider = FakeProvider::new();
        let first = provider.inner.lock().machines.len();
        assert_eq!(first, 0);
        assert_eq!(provider.tick(), CLOCK_BASE_MS);
        assert_eq!(provider.tick(), CLOCK_BASE_MS + 1_000);
    }
}
