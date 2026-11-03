//! A standby's link to the machine holding the supervisor lease. Supervisor requests from the
//! owner's CLI/MCP and observations of this machine's own panes are forwarded to the holder
//! over the paired-host channel. Forwarding needs an agent-control grant on the holder: when
//! this machine has none, it redeems one through the account (same account machines only).
//! Without an account session the state is "not paired to holder" and nothing is forwarded.
use std::sync::RwLock;
use std::time::Duration;

use async_trait::async_trait;
use ferryx_hub::supervisor::request::RequestFailure;
use serde_json::Value;

use crate::paired_host::client::{ClientError, MachineClient, Operation, OperationRequest, OperationResult};
use crate::paired_host::inventory::{AuthStatus, GrantScope, HostView};
use crate::paired_host::service::{PairedHostService, RedeemGrantRequest, Secret};

/// How long a failed pairing attempt is not repeated.
pub(crate) const PAIRING_RETRY: Duration = Duration::from_secs(300);

/// The lease holder as a standby knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Holder {
    pub machine_id: Option<String>,
    pub label: Option<String>,
}

/// This machine's pairing to the holder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Pairing {
    NoHolder,
    Paired,
    NotPaired,
}

/// This machine's paired hosts: the daemon's inventory in production, a fake in tests.
#[async_trait]
pub(crate) trait PairedHosts: Send + Sync {
    async fn list(&self) -> Result<Vec<HostView>, String>;
    async fn redeem(&self, request: RedeemGrantRequest) -> Result<HostView, String>;
    async fn send(&self, host: &HostView, operation: Operation) -> Result<Value, ClientError>;
}

/// Issues an account grant on the holder for this machine.
#[async_trait]
pub(crate) trait AccountGrants: Send + Sync {
    async fn grant_on(&self, holder_machine_id: &str) -> Result<RedeemGrantRequest, String>;
}

pub(crate) struct HolderLink {
    state: RwLock<Pairing>,
    last_attempt: std::sync::Mutex<Option<tokio::time::Instant>>,
    /// The holder's machine id as this daemon's role or a pairing attempt last named it.
    holder_id: std::sync::Mutex<Option<String>>,
    /// One pairing attempt at a time: a concurrent caller waits and then finds the host the
    /// first one redeemed, instead of reading that attempt as a failure inside the backoff.
    attempt: tokio::sync::Mutex<()>,
    /// The paired holder host whose grant the holder refused (403 FORBIDDEN on a machine-peer
    /// route): a grant redeemed before the account vouched for machines, e.g. one the desktop
    /// paired. It is not used for supervisor peer calls until a fresh grant replaces it (a
    /// re-pair keeps the host id and moves its generation).
    refused_host: std::sync::Mutex<Option<(String, crate::scoped_contracts::Epoch)>>,
    /// The last pairing attempt failed for want of an account session.
    needs_login: std::sync::atomic::AtomicBool,
}

/// The daemon's one link.
pub(crate) static LINK: HolderLink = HolderLink::new();

fn usable(host: &HostView) -> bool {
    host.grant_scope == GrantScope::Machine
        && host.auth_status == AuthStatus::Paired
        && host.unavailable_reason.is_none()
}

/// A paired host is the holder only by machine id: a display label is the owner's free text
/// and two machines may share one.
fn is_holder(host: &HostView, holder: &Holder) -> bool {
    holder.machine_id.as_deref().is_some_and(|id| host.machine_id == id)
}

impl HolderLink {
    pub(crate) const fn new() -> Self {
        Self {
            state: RwLock::new(Pairing::NoHolder),
            last_attempt: std::sync::Mutex::new(None),
            holder_id: std::sync::Mutex::new(None),
            attempt: tokio::sync::Mutex::const_new(()),
            refused_host: std::sync::Mutex::new(None),
            needs_login: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn refused_host(&self) -> Option<(String, crate::scoped_contracts::Epoch)> {
        self.refused_host.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set_refused_host(&self, host: Option<&HostView>) {
        *self.refused_host.lock().unwrap_or_else(|e| e.into_inner()) =
            host.map(|host| (host.host_id.clone(), host.generation));
    }

    pub(crate) fn pairing(&self) -> Pairing {
        self.state.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set(&self, pairing: Pairing) {
        *self.state.write().unwrap_or_else(|e| e.into_inner()) = pairing;
    }

    /// Records `id` as the holder; true when it differs from the one known, whose pairing
    /// backoff then no longer applies.
    fn follow(&self, id: Option<String>) -> bool {
        let mut known = self.holder_id.lock().unwrap_or_else(|e| e.into_inner());
        if *known == id {
            return false;
        }
        *known = id;
        *self.last_attempt.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.set_refused_host(None);
        self.needs_login.store(false, std::sync::atomic::Ordering::Relaxed);
        true
    }

    /// Follows this daemon's role: without a known holder machine the state is "no holder";
    /// a newly named holder is "not paired to holder" until a pairing attempt says otherwise,
    /// and is attempted at once. The holder a pairing attempt already followed keeps its state.
    pub(crate) fn note_holder(&self, holder: Option<&Holder>) {
        let id = holder.and_then(|holder| holder.machine_id.clone());
        let known = id.is_some();
        if self.follow(id) && known {
            self.set(Pairing::NotPaired);
        } else if !known {
            self.set(Pairing::NoHolder);
        }
    }

    /// True when this machine holds a usable grant on the holder.
    pub(crate) fn holder_paired(&self) -> bool {
        self.pairing() == Pairing::Paired
    }

    /// Why this machine is not paired to the holder, when the fix is the owner's: no account
    /// session ([`NEEDS_LOGIN_REASON`]). `None` otherwise.
    pub(crate) fn pairing_reason(&self) -> Option<&'static str> {
        (self.pairing() == Pairing::NotPaired && self.needs_login.load(std::sync::atomic::Ordering::Relaxed))
            .then_some(NEEDS_LOGIN_REASON)
    }

    /// `"paired"`, `"not paired to holder"` or `"no holder"`, for status.
    pub(crate) fn pairing_state(&self) -> &'static str {
        match self.pairing() {
            Pairing::Paired => "paired",
            Pairing::NotPaired => "not paired to holder",
            Pairing::NoHolder => "no holder",
        }
    }

    /// The holder's paired host, redeeming an account grant first when this machine has none.
    /// A failed attempt is not repeated within [`PAIRING_RETRY`].
    pub(crate) async fn holder_host(
        &self,
        holder: &Holder,
        hosts: &dyn PairedHosts,
        account: &dyn AccountGrants,
    ) -> Option<HostView> {
        let _serial = self.attempt.lock().await;
        // A label-only holder (machine unknown) leaves the followed holder as it is.
        if holder.machine_id.is_some() {
            self.follow(holder.machine_id.clone());
        }
        let listed = hosts.list().await.unwrap_or_default();
        let refused = self.refused_host();
        if let Some(host) = listed.into_iter().find(|host| {
            usable(host)
                && is_holder(host, holder)
                && refused.as_ref().is_none_or(|(id, generation)| (id, generation) != (&host.host_id, &host.generation))
        }) {
            self.set(Pairing::Paired);
            return Some(host);
        }
        {
            let mut last = self.last_attempt.lock().unwrap_or_else(|e| e.into_inner());
            let now = tokio::time::Instant::now();
            // A refused grant is re-redeemed at most once per backoff, so a holder that keeps
            // refusing never makes this machine loop on account grants.
            if (self.pairing() == Pairing::NotPaired || refused.is_some())
                && last.is_some_and(|at| now.duration_since(at) < PAIRING_RETRY)
            {
                if refused.is_some() {
                    self.set(Pairing::NotPaired);
                }
                return None;
            }
            *last = Some(now);
        }
        let Some(machine_id) = holder.machine_id.as_deref() else {
            tracing::warn!("not paired to the supervisor holder: its machine is unknown");
            self.set(Pairing::NotPaired);
            return None;
        };
        let redeemed = match account.grant_on(machine_id).await {
            Ok(request) => hosts.redeem(request).await,
            Err(reason) => Err(reason),
        };
        self.needs_login.store(
            redeemed.as_ref().is_err_and(|reason| reason.starts_with(crate::cli::billing::ACCOUNT_SESSION_REQUIRED)),
            std::sync::atomic::Ordering::Relaxed,
        );
        match redeemed {
            Ok(host) if usable(&host) => {
                tracing::info!(host_id = %host.host_id, "paired to the supervisor holder through the account");
                self.set_refused_host(None);
                self.set(Pairing::Paired);
                Some(host)
            }
            Ok(_) => {
                tracing::warn!("not paired to the supervisor holder: the grant is not machine control");
                self.set(Pairing::NotPaired);
                None
            }
            Err(reason) => {
                tracing::warn!(%reason, "not paired to the supervisor holder");
                self.set(Pairing::NotPaired);
                None
            }
        }
    }

    /// The holder refused `refused`'s grant on a machine-peer route with the agent-control
    /// refusal (403 marked `details.refusal = agentControl`): a grant
    /// without the account's same-machine marker. Sets it aside and redeems a fresh account
    /// grant, which carries this machine's proof; `None` when that is not possible now (no
    /// session, refused, or already tried within [`PAIRING_RETRY`]).
    pub(crate) async fn repair_refused(
        &self,
        holder: &Holder,
        hosts: &dyn PairedHosts,
        account: &dyn AccountGrants,
        refused: &HostView,
    ) -> Option<HostView> {
        tracing::info!(host_id = %refused.host_id, "the holder refused this machine's grant; redeeming a fresh one");
        self.set_refused_host(Some(refused));
        self.holder_host(holder, hosts, account).await
    }

    /// Sends `operation` to the paired holder; SUPERVISOR_STANDBY naming the holder when this
    /// machine is not paired to it. The agent-control refusal (403 marked
    /// `details.refusal = agentControl`) is answered by one fresh grant and one retry
    /// ([`Self::repair_refused`]); any other refusal is reported as it is.
    pub(crate) async fn forward(
        &self,
        holder: &Holder,
        hosts: &dyn PairedHosts,
        account: &dyn AccountGrants,
        operation: Operation,
    ) -> Result<Value, RequestFailure> {
        let Some(host) = self.holder_host(holder, hosts, account).await else {
            return Err(ferryx_hub::holder::standby_failure(
                holder.machine_id.as_deref(),
                holder.label.as_deref(),
            ));
        };
        match hosts.send(&host, operation.clone()).await {
            Err(error) if is_forbidden(&error) => match self.repair_refused(holder, hosts, account, &host).await {
                Some(repaired) => hosts.send(&repaired, operation).await.map_err(failure_of),
                None => Err(failure_of(error)),
            },
            sent => sent.map_err(failure_of),
        }
    }
}

/// The pairing reason a standby reports when it cannot pair for want of an account session.
pub(crate) const NEEDS_LOGIN_REASON: &str =
    "no account session on this machine: run `ferryx account login` once";

/// The holder refused this machine's device with the agent-control access refusal (403
/// FORBIDDEN marked by [`crate::agent_control::policy::is_agent_control_refusal`]): its grant
/// lacks the account's same-machine mark. Any other FORBIDDEN, such as a goal of another owner,
/// is the supervisor's answer and is reported as it is.
fn is_forbidden(error: &ClientError) -> bool {
    error.machine_error.as_ref().is_some_and(|machine| {
        machine.code == "FORBIDDEN" && crate::agent_control::policy::is_agent_control_refusal(&machine.details)
    })
}

/// The holder's refusal as this machine reports it; a transport failure is unavailable.
fn failure_of(error: ClientError) -> RequestFailure {
    let Some(machine) = error.machine_error else {
        return RequestFailure::new(
            "SUPERVISOR_UNAVAILABLE",
            format!("the supervisor holder could not be reached: {}", error.code),
        );
    };
    let mut failure = RequestFailure::new(&machine.code, machine.message.clone());
    if machine.details.contains_key("holderMachineId") || machine.details.contains_key("holderLabel") {
        let text = |key: &str| machine.details.get(key).and_then(Value::as_str).map(str::to_owned);
        failure.holder = Some((text("holderMachineId"), text("holderLabel")));
    }
    failure
}

/// The daemon's own paired-host inventory, over its control socket.
pub(crate) struct DaemonPairedHosts(pub crate::daemon::client::DaemonClient);

#[async_trait]
impl PairedHosts for DaemonPairedHosts {
    async fn list(&self) -> Result<Vec<HostView>, String> {
        self.0.paired_host_list().await.map_err(|error| error.message)
    }

    async fn redeem(&self, request: RedeemGrantRequest) -> Result<HostView, String> {
        self.0
            .paired_host_redeem_grant(request)
            .await
            .map_err(|error| format!("{}: {}", error.code, error.message))
    }

    async fn send(&self, host: &HostView, operation: Operation) -> Result<Value, ClientError> {
        let request = OperationRequest { host_id: host.host_id.clone(), generation: host.generation, operation };
        match self.0.paired_host_operation(request).await?.result {
            OperationResult::Supervisor(value) => Ok(value),
            _ => Err(ClientError::local("PAIRED_HOST_INVALID_RESPONSE")),
        }
    }
}

/// The daemon's paired-host inventory in process (the daemon's own service, as its control
/// socket serves it to [`DaemonPairedHosts`]).
pub(crate) struct ServicePairedHosts(pub PairedHostService);

#[async_trait]
impl PairedHosts for ServicePairedHosts {
    async fn list(&self) -> Result<Vec<HostView>, String> {
        self.0.list().await.map_err(|error| error.message)
    }

    async fn redeem(&self, request: RedeemGrantRequest) -> Result<HostView, String> {
        self.0
            .redeem_grant(request)
            .await
            .map_err(|error| format!("{}: {}", error.code, error.message))
    }

    async fn send(&self, host: &HostView, operation: Operation) -> Result<Value, ClientError> {
        let request = OperationRequest { host_id: host.host_id.clone(), generation: host.generation, operation };
        match MachineClient::new().execute(&self.0, request).await?.result {
            OperationResult::Supervisor(value) => Ok(value),
            _ => Err(ClientError::local("PAIRED_HOST_INVALID_RESPONSE")),
        }
    }
}

/// Grants on the holder from the owner's account, with the account session the existing
/// redeem flow takes: `FERRYX_ACCOUNT_SESSION_TOKEN` for this machine's account origin when
/// set, else the session saved at `ferryx account login` / desktop link
/// ([`crate::cli::billing::resolve_session_token_or_stored`]). No session: not paired to the
/// holder. The request carries this machine's enrolled-key proof, so the account vouches for
/// it as a same-account machine in the grant the holder applies.
pub(crate) struct AccountService;

#[async_trait]
impl AccountGrants for AccountService {
    async fn grant_on(&self, holder_machine_id: &str) -> Result<RedeemGrantRequest, String> {
        use crate::remote::account_protocol::{AccountGrantRequest, AccountGrantResponse, AccountGrantScope};
        // Reads the enrollment record and the saved session: off the async threads.
        let (origin, token) = crate::ipc::run_blocking(|| {
            Ok(crate::remote::auth::canonical_remote_dir()
                .ok_or_else(|| "cannot resolve the identity directory".to_owned())
                .and_then(|dir| grant_bearer_in(&dir, |key| std::env::var(key).ok())))
        })
        .await
        .map_err(|error| error.to_string())??;
        // Both read (or create) files: off the async threads.
        let (identity, attach_public_key) = crate::ipc::run_blocking(|| {
            Ok((
                crate::remote::auth::canonical_identity_dir()
                    .and_then(|dir| crate::remote::auth::load_or_generate_machine_identity(&dir)),
                crate::paired_host::secure_http::client_attach_public_key(),
            ))
        })
        .await
        .map_err(|error| error.to_string())?;
        let identity = identity?;
        let attach_public_key = attach_public_key.map_err(|error| error.code)?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|error| error.to_string())?;
        let machines: Vec<Value> = client
            .get(format!("{origin}/api/account/v1/machines"))
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|error| error.to_string())?
            .json()
            .await
            .map_err(|error| error.to_string())?;
        let holder = machines
            .iter()
            .find(|machine| machine["machineId"].as_str() == Some(holder_machine_id))
            .ok_or("the supervisor holder is not a machine of this account")?;
        let text = |value: &Value| match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        let record_id = text(&holder["machineRecordId"]);
        let enrollment_epoch = text(&holder["enrollmentEpoch"]);
        let request = AccountGrantRequest {
            machine_record_id: record_id.clone(),
            enrollment_epoch: enrollment_epoch.clone(),
            device_label: identity.display_name.clone(),
            installation_id: identity.machine_id.clone(),
            grant_scope: AccountGrantScope::Machine,
            requester_machine: Some(requester_proof(
                &identity,
                &record_id,
                &attach_public_key,
                crate::account::store::now_secs(),
            )?),
            attach_public_key,
        };
        let response = client
            .post(format!("{origin}/api/account/v1/machines/{record_id}/grants"))
            .bearer_auth(&token)
            .json(&request)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            return Err(format!("the account refused a grant on the holder: {}", response.status()));
        }
        let grant: AccountGrantResponse = response.json().await.map_err(|error| error.to_string())?;
        Ok(RedeemGrantRequest {
            relay_origin: grant.relay_origin,
            machine_id: grant.machine_id,
            display_label: text(&holder["displayName"]),
            pairing_token: Secret(grant.pairing_token),
            machine_attach_public_key: grant.machine_attach_public_key,
            enrollment_epoch,
            account_session_token: Secret(token),
        })
    }
}

/// The account origin and bearer [`AccountService::grant_on`] uses, from the identity directory
/// `dir`: this machine's enrollment record names the origin; the bearer is the environment's
/// session (`lookup`), else the session saved in `dir`. `Err` ("not paired to holder") without
/// an enrollment or without either session. Blocking: reads files.
pub(crate) fn grant_bearer_in<F>(dir: &std::path::Path, lookup: F) -> Result<(String, String), String>
where
    F: Fn(&str) -> Option<String>,
{
    let record: crate::account::enroll_client::AccountEnrollmentRecord =
        std::fs::read(dir.join(crate::account::enroll_client::ACCOUNT_ENROLLMENT_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or("this machine is not enrolled in an account")?;
    let origin = record.account_origin.trim_end_matches('/').to_owned();
    let stored = crate::account::enroll_client::load_account_session_at(
        &dir.join(crate::account::enroll_client::ACCOUNT_SESSION_FILE),
    );
    let token = session_bearer(&origin, lookup, stored.as_ref())?;
    Ok((origin, token))
}

/// The bearer [`grant_bearer_in`] picks: the environment's session, else the saved one.
/// `Err` ("not paired to holder") with neither.
pub(crate) fn session_bearer<F>(
    origin: &str,
    lookup: F,
    stored: Option<&crate::account::enroll_client::StoredAccountSession>,
) -> Result<String, String>
where
    F: Fn(&str) -> Option<String>,
{
    crate::cli::billing::resolve_session_token_or_stored(origin, lookup, stored).map_err(|error| error.to_string())
}

/// This machine's proof, by its enrolled machine key, that it asks for the grant on
/// `target_machine_record_id` for its own device key `attach_public_key`
/// ([`crate::remote::account_protocol::grant_requester_nonce`]).
pub(crate) fn requester_proof(
    identity: &crate::remote::auth::MachineIdentity,
    target_machine_record_id: &str,
    attach_public_key: &str,
    now: u64,
) -> Result<crate::remote::account_protocol::GrantRequesterProof, String> {
    let nonce = crate::remote::account_protocol::grant_requester_nonce(
        &identity.machine_id,
        target_machine_record_id,
        attach_public_key,
    );
    Ok(crate::remote::account_protocol::GrantRequesterProof {
        machine_id: identity.machine_id.clone(),
        timestamp: now,
        signature: crate::remote::auth::sign_challenge(identity, &nonce, now)?,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::scoped_contracts::Epoch;

    /// The holder's agent-control access refusal as the paired-host client reports it.
    pub(crate) fn forbidden_error() -> ClientError {
        let mut details = serde_json::Map::new();
        details.insert(
            crate::agent_control::policy::REFUSAL_DETAIL.into(),
            crate::agent_control::policy::AGENT_CONTROL_REFUSAL.into(),
        );
        service_forbidden(details)
    }

    /// A 403 FORBIDDEN with `details`; without the marker it is the supervisor's own answer.
    pub(crate) fn service_forbidden(details: serde_json::Map<String, Value>) -> ClientError {
        ClientError {
            code: "FORBIDDEN".into(),
            machine_error: Some(Box::new(crate::remote::machine_protocol::MachineError {
                code: "FORBIDDEN".into(),
                message: "The paired host could not complete the request.".into(),
                retryable: false,
                request_id: "r1".into(),
                details,
            })),
            request_id: None,
            ambiguous: false,
        }
    }

    pub(crate) fn host(machine_id: &str, status: AuthStatus) -> HostView {
        HostView {
            host_id: format!("host-{machine_id}"),
            relay_origin: "https://relay.test".into(),
            machine_id: machine_id.into(),
            display_label: "Laptop B".into(),
            grant_scope: GrantScope::Machine,
            generation: Epoch(1),
            auth_status: status,
            online: true,
            ssh_host_id: None,
            device_id: None,
            unavailable_reason: None,
        }
    }

    fn holder() -> Holder {
        Holder { machine_id: Some("mach-b".into()), label: Some("Laptop B".into()) }
    }

    /// Paired hosts that start empty; a redeem adds the holder, or re-pairs it in the next
    /// generation as the inventory does. Sends are recorded.
    /// With `refuse_redeem` every redeem is recorded and then refused.
    #[derive(Default)]
    pub(crate) struct FakeHosts {
        pub hosts: parking_lot::Mutex<Vec<HostView>>,
        pub redeemed: parking_lot::Mutex<Vec<String>>,
        pub sent: parking_lot::Mutex<Vec<(String, Value)>>,
        pub refuse_redeem: bool,
        /// How many next sends the holder answers 403 FORBIDDEN (an unmarked grant).
        pub forbid_sends: std::sync::atomic::AtomicUsize,
        /// How many next sends the holder's supervisor refuses with an ordinary FORBIDDEN.
        pub refuse_sends: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl PairedHosts for FakeHosts {
        async fn list(&self) -> Result<Vec<HostView>, String> {
            Ok(self.hosts.lock().clone())
        }
        async fn redeem(&self, request: RedeemGrantRequest) -> Result<HostView, String> {
            self.redeemed.lock().push(request.machine_id.clone());
            if self.refuse_redeem {
                return Err("PAIRING_REFUSED: the relay refused the grant".into());
            }
            let mut paired = host(&request.machine_id, AuthStatus::Paired);
            let mut listed = self.hosts.lock();
            if let Some(previous) = listed.iter().position(|known| known.host_id == paired.host_id) {
                paired.generation = crate::scoped_contracts::Epoch(listed[previous].generation.0 + 1);
                listed.remove(previous);
            }
            listed.push(paired.clone());
            Ok(paired)
        }
        async fn send(&self, host: &HostView, operation: Operation) -> Result<Value, ClientError> {
            let forbid = &self.forbid_sends;
            if forbid.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                forbid.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                return Err(forbidden_error());
            }
            let refuse = &self.refuse_sends;
            if refuse.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                refuse.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                return Err(service_forbidden(Default::default()));
            }
            let body = match operation {
                Operation::SupervisorObserve { observation } => observation,
                Operation::SupervisorRequest { envelope } => envelope,
                _ => Value::Null,
            };
            self.sent.lock().push((host.host_id.clone(), body));
            Ok(serde_json::json!({"ok": true}))
        }
    }

    /// The account service stand-in: grants on any holder, or has no session.
    pub(crate) struct FakeAccount {
        pub session: bool,
        pub grants: parking_lot::Mutex<Vec<String>>,
    }

    #[async_trait]
    impl AccountGrants for FakeAccount {
        async fn grant_on(&self, holder_machine_id: &str) -> Result<RedeemGrantRequest, String> {
            if !self.session {
                return Err("ACCOUNT_SESSION_REQUIRED".into());
            }
            self.grants.lock().push(holder_machine_id.to_owned());
            Ok(RedeemGrantRequest {
                relay_origin: "https://relay.test".into(),
                machine_id: holder_machine_id.into(),
                display_label: "Laptop B".into(),
                pairing_token: Secret("pt".into()),
                machine_attach_public_key: "k".into(),
                enrollment_epoch: "1".into(),
                account_session_token: Secret("s".into()),
            })
        }
    }

    #[tokio::test]
    async fn a_standby_without_a_grant_on_the_holder_redeems_one_and_reports_paired() {
        let link = HolderLink::new();
        let hosts = FakeHosts::default();
        let account = FakeAccount { session: true, grants: parking_lot::Mutex::new(Vec::new()) };
        assert_eq!(link.pairing_state(), "no holder");
        let observation = serde_json::json!({"kind": "keystrokes", "sessionId": "pane-1", "nowMs": 1});
        let answer = link
            .forward(&holder(), &hosts, &account, Operation::SupervisorObserve { observation: observation.clone() })
            .await
            .unwrap();
        assert_eq!(answer["ok"], true);
        assert_eq!(*account.grants.lock(), vec!["mach-b".to_owned()], "one account grant on the holder");
        assert_eq!(*hosts.redeemed.lock(), vec!["mach-b".to_owned()]);
        assert_eq!(*hosts.sent.lock(), vec![("host-mach-b".to_owned(), observation)]);
        assert!(link.holder_paired());
        assert_eq!(link.pairing_state(), "paired");
        // Already paired: no second grant.
        link.forward(&holder(), &hosts, &account, Operation::SupervisorObserve { observation: Value::Null })
            .await
            .unwrap();
        assert_eq!(account.grants.lock().len(), 1);
    }

    /// A paired host is the holder by machine id only: another machine that shares the
    /// holder's display label is never forwarded to.
    #[tokio::test]
    async fn a_host_sharing_the_holders_label_is_not_the_holder() {
        let link = HolderLink::new();
        let hosts = FakeHosts::default();
        hosts.hosts.lock().push(host("mach-x", AuthStatus::Paired));
        assert_eq!(hosts.hosts.lock()[0].display_label, holder().label.unwrap());
        let account = FakeAccount { session: false, grants: parking_lot::Mutex::new(Vec::new()) };
        let refused = link
            .forward(&holder(), &hosts, &account, Operation::SupervisorObserve { observation: Value::Null })
            .await
            .unwrap_err();
        assert_eq!(refused.code, ferryx_hub::holder::SUPERVISOR_STANDBY);
        assert!(hosts.sent.lock().is_empty(), "nothing went to the look-alike host");
        assert_eq!(link.pairing_state(), "not paired to holder");
    }

    #[tokio::test]
    async fn without_an_account_session_the_standby_is_not_paired_and_refuses_with_the_holder() {
        let link = HolderLink::new();
        let hosts = FakeHosts::default();
        let account = FakeAccount { session: false, grants: parking_lot::Mutex::new(Vec::new()) };
        let refused = link
            .forward(&holder(), &hosts, &account, Operation::SupervisorRequest { envelope: Value::Null })
            .await
            .unwrap_err();
        assert_eq!(refused.code, ferryx_hub::holder::SUPERVISOR_STANDBY);
        assert_eq!(refused.holder, Some((Some("mach-b".into()), Some("Laptop B".into()))));
        assert!(!link.holder_paired());
        assert_eq!(link.pairing_state(), "not paired to holder");
        assert!(hosts.sent.lock().is_empty(), "nothing forwarded");
    }

    /// Status follows the role: a known holder is "not paired to holder" before any attempt,
    /// stays "paired" while the same holder is named, and a new holder starts over.
    #[tokio::test]
    async fn the_pairing_state_follows_the_holder_the_role_names() {
        let link = HolderLink::new();
        link.note_holder(Some(&holder()));
        assert_eq!(link.pairing_state(), "not paired to holder");
        let hosts = FakeHosts::default();
        hosts.hosts.lock().push(host("mach-b", AuthStatus::Paired));
        let account = FakeAccount { session: false, grants: parking_lot::Mutex::new(Vec::new()) };
        assert!(link.holder_host(&holder(), &hosts, &account).await.is_some());
        link.note_holder(Some(&holder()));
        assert_eq!(link.pairing_state(), "paired", "the same holder keeps its pairing");
        let other = Holder { machine_id: Some("mach-c".into()), label: None };
        link.note_holder(Some(&other));
        assert_eq!(link.pairing_state(), "not paired to holder");
        link.note_holder(None);
        assert_eq!(link.pairing_state(), "no holder");
    }

    const ORIGIN: &str = "https://account.test";

    /// The daemon carries no account env: the session saved owner-only at login is the grant's
    /// bearer. The saved file is mode 0600 on Unix.
    #[test]
    fn with_the_env_unset_the_grant_uses_the_saved_session_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(crate::account::enroll_client::ACCOUNT_SESSION_FILE);
        crate::account::enroll_client::save_account_session_at(&path, ORIGIN, "file-session").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let stored = crate::account::enroll_client::load_account_session_at(&path);
        assert_eq!(session_bearer(ORIGIN, |_| None, stored.as_ref()).unwrap(), "file-session");
    }

    /// The environment still wins over the saved session, and the saved one is never sent to
    /// another account origin.
    #[test]
    fn the_env_session_wins_over_the_saved_file_and_the_file_stays_with_its_origin() {
        let stored = crate::account::enroll_client::StoredAccountSession {
            account_origin: ORIGIN.into(),
            session_token: "file-session".into(),
        };
        let env = |key: &str| match key {
            crate::cli::billing::SESSION_TOKEN_ENV => Some("env-session".to_string()),
            crate::cli::billing::SESSION_ORIGIN_ENV => Some(ORIGIN.to_string()),
            _ => None,
        };
        assert_eq!(session_bearer(ORIGIN, env, Some(&stored)).unwrap(), "env-session");
        assert!(session_bearer("https://other.test", |_| None, Some(&stored)).is_err());
    }

    /// The bearer `AccountService::grant_on` resolves from an identity directory: the
    /// enrollment names the origin; with neither env nor saved session it fails with
    /// ACCOUNT_SESSION_REQUIRED (the standby's "run ferryx account login" reason); the session
    /// saved there is used once written; the environment still wins over it.
    #[test]
    fn the_grant_bearer_comes_from_the_saved_session_in_the_identity_dir_unless_the_env_names_one() {
        let dir = tempfile::tempdir().unwrap();
        assert!(grant_bearer_in(dir.path(), |_| None).unwrap_err().contains("not enrolled"));
        let record = crate::account::enroll_client::AccountEnrollmentRecord {
            account_id: "acct-1".into(),
            machine_record_id: "rec-1".into(),
            account_origin: format!("{ORIGIN}/"),
            relay_origin: "https://relay.test".into(),
            enrollment_epoch: "1".into(),
            enrolled_at: 1,
        };
        crate::remote::auth::write_private_json(
            &dir.path().join(crate::account::enroll_client::ACCOUNT_ENROLLMENT_FILE),
            &record,
        )
        .unwrap();

        let missing = grant_bearer_in(dir.path(), |_| None).unwrap_err();
        assert!(missing.starts_with(crate::cli::billing::ACCOUNT_SESSION_REQUIRED), "{missing}");

        crate::account::enroll_client::save_account_session_at(
            &dir.path().join(crate::account::enroll_client::ACCOUNT_SESSION_FILE),
            ORIGIN,
            "file-session",
        )
        .unwrap();
        assert_eq!(grant_bearer_in(dir.path(), |_| None).unwrap(), (ORIGIN.to_owned(), "file-session".to_owned()));

        let env = |key: &str| match key {
            crate::cli::billing::SESSION_TOKEN_ENV => Some("env-session".to_string()),
            crate::cli::billing::SESSION_ORIGIN_ENV => Some(ORIGIN.to_string()),
            _ => None,
        };
        assert_eq!(grant_bearer_in(dir.path(), env).unwrap().1, "env-session");
    }

    /// MAJOR-A: a standby paired before the account vouched for machines (e.g. by the desktop)
    /// holds a grant the holder refuses with 403 when agent control is off. The first 403
    /// redeems exactly one fresh account grant and the forward is retried once on it.
    #[tokio::test]
    async fn a_forbidden_holder_grant_is_replaced_by_one_account_grant_and_the_forward_retried() {
        let link = HolderLink::new();
        let hosts = FakeHosts { forbid_sends: std::sync::atomic::AtomicUsize::new(1), ..FakeHosts::default() };
        hosts.hosts.lock().push(host("mach-b", AuthStatus::Paired));
        let account = FakeAccount { session: true, grants: parking_lot::Mutex::new(Vec::new()) };

        let sent = link
            .forward(&holder(), &hosts, &account, Operation::SupervisorObserve { observation: serde_json::json!({"n": 1}) })
            .await
            .unwrap();

        assert_eq!(sent["ok"], true);
        assert_eq!(*account.grants.lock(), vec!["mach-b".to_owned()], "exactly one account grant");
        assert_eq!(*hosts.redeemed.lock(), vec!["mach-b".to_owned()], "exactly one redeem");
        let listed = hosts.hosts.lock().clone();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].generation, crate::scoped_contracts::Epoch(2), "the grant was re-paired");
        assert_eq!(*hosts.sent.lock(), vec![("host-mach-b".to_owned(), serde_json::json!({"n": 1}))]);
        assert_eq!(link.pairing_state(), "paired");
    }

    /// A holder that keeps answering 403 after the fresh grant does not make the standby loop:
    /// one redeem, the refusal is reported, and the next forward inside the pairing backoff
    /// redeems nothing more.
    #[tokio::test(start_paused = true)]
    async fn a_second_forbidden_answer_does_not_redeem_again_inside_the_backoff() {
        let link = HolderLink::new();
        let hosts = FakeHosts { forbid_sends: std::sync::atomic::AtomicUsize::new(usize::MAX), ..FakeHosts::default() };
        hosts.hosts.lock().push(host("mach-b", AuthStatus::Paired));
        let account = FakeAccount { session: true, grants: parking_lot::Mutex::new(Vec::new()) };
        let request = || Operation::SupervisorRequest { envelope: Value::Null };

        let refused = link.forward(&holder(), &hosts, &account, request()).await.unwrap_err();
        assert_eq!(refused.code, "FORBIDDEN");
        assert_eq!(account.grants.lock().len(), 1);
        assert_eq!(hosts.redeemed.lock().len(), 1);

        tokio::time::advance(PAIRING_RETRY - Duration::from_secs(1)).await;
        let refused = link.forward(&holder(), &hosts, &account, request()).await.unwrap_err();
        assert_eq!(refused.code, "FORBIDDEN");
        assert_eq!(account.grants.lock().len(), 1, "no second grant inside the backoff");
        assert_eq!(hosts.redeemed.lock().len(), 1, "no second redeem inside the backoff");
        assert_eq!(link.pairing_state(), "not paired to holder");

        // Still inside the backoff the refused grant is not used at all.
        let standby = link.forward(&holder(), &hosts, &account, request()).await.unwrap_err();
        assert_eq!(standby.code, ferryx_hub::holder::SUPERVISOR_STANDBY);
        assert_eq!(hosts.redeemed.lock().len(), 1);
        assert!(hosts.sent.lock().is_empty());
    }

    /// F4 MINOR-1: an ordinary FORBIDDEN from the holder's supervisor (a goal of another owner)
    /// is reported as it is: no grant is redeemed and the grant is not set aside.
    #[tokio::test]
    async fn an_ordinary_forbidden_is_reported_without_redeeming_a_grant() {
        let link = HolderLink::new();
        let hosts = FakeHosts { refuse_sends: std::sync::atomic::AtomicUsize::new(2), ..FakeHosts::default() };
        hosts.hosts.lock().push(host("mach-b", AuthStatus::Paired));
        let account = FakeAccount { session: true, grants: parking_lot::Mutex::new(Vec::new()) };
        for _ in 0..2 {
            let refused = link
                .forward(&holder(), &hosts, &account, Operation::SupervisorRequest { envelope: Value::Null })
                .await
                .unwrap_err();
            assert_eq!(refused.code, "FORBIDDEN");
        }
        assert!(account.grants.lock().is_empty(), "no account grant");
        assert!(hosts.redeemed.lock().is_empty(), "no redeem");
        assert_eq!(link.pairing_state(), "paired", "the grant stays in use");
        link.forward(&holder(), &hosts, &account, Operation::SupervisorRequest { envelope: Value::Null })
            .await
            .unwrap();
        assert_eq!(hosts.sent.lock().len(), 1, "the next request goes out on the same grant");
    }

    /// Without an account session the not-paired state carries the fix: `ferryx account login`.
    #[tokio::test]
    async fn without_a_session_the_pairing_reason_names_account_login() {
        let link = HolderLink::new();
        let hosts = FakeHosts::default();
        let account = FakeAccount { session: false, grants: parking_lot::Mutex::new(Vec::new()) };
        assert_eq!(link.pairing_reason(), None);
        assert!(link.holder_host(&holder(), &hosts, &account).await.is_none());
        assert_eq!(link.pairing_state(), "not paired to holder");
        assert_eq!(link.pairing_reason(), Some(NEEDS_LOGIN_REASON));

        let link = HolderLink::new();
        let refusing = FakeHosts { refuse_redeem: true, ..FakeHosts::default() };
        let account = FakeAccount { session: true, grants: parking_lot::Mutex::new(Vec::new()) };
        assert!(link.holder_host(&holder(), &refusing, &account).await.is_none());
        assert_eq!(link.pairing_reason(), None, "a refused redeem is not a missing session");
    }
}
