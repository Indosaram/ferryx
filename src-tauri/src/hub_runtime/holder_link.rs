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

use crate::paired_host::client::{ClientError, Operation, OperationRequest, OperationResult};
use crate::paired_host::inventory::{AuthStatus, GrantScope, HostView};
use crate::paired_host::service::{RedeemGrantRequest, Secret};

/// How long a failed pairing attempt is not repeated.
const PAIRING_RETRY: Duration = Duration::from_secs(300);

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
}

/// The daemon's one link.
pub(crate) static LINK: HolderLink = HolderLink::new();

fn usable(host: &HostView) -> bool {
    host.grant_scope == GrantScope::Machine
        && host.auth_status == AuthStatus::Paired
        && host.unavailable_reason.is_none()
}

fn is_holder(host: &HostView, holder: &Holder) -> bool {
    holder
        .machine_id
        .as_deref()
        .is_some_and(|id| host.machine_id == id || host.host_id == id)
        || holder.label.as_deref().is_some_and(|label| host.display_label == label)
}

impl HolderLink {
    pub(crate) const fn new() -> Self {
        Self {
            state: RwLock::new(Pairing::NoHolder),
            last_attempt: std::sync::Mutex::new(None),
        }
    }

    pub(crate) fn pairing(&self) -> Pairing {
        self.state.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set(&self, pairing: Pairing) {
        *self.state.write().unwrap_or_else(|e| e.into_inner()) = pairing;
    }

    /// True when this machine holds a usable grant on the holder.
    pub(crate) fn holder_paired(&self) -> bool {
        self.pairing() == Pairing::Paired
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
        let listed = hosts.list().await.unwrap_or_default();
        if let Some(host) = listed.into_iter().find(|host| usable(host) && is_holder(host, holder)) {
            self.set(Pairing::Paired);
            return Some(host);
        }
        {
            let mut last = self.last_attempt.lock().unwrap_or_else(|e| e.into_inner());
            let now = tokio::time::Instant::now();
            if self.pairing() == Pairing::NotPaired
                && last.is_some_and(|at| now.duration_since(at) < PAIRING_RETRY)
            {
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
        match redeemed {
            Ok(host) if usable(&host) => {
                tracing::info!(host_id = %host.host_id, "paired to the supervisor holder through the account");
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

    /// Sends `operation` to the paired holder; SUPERVISOR_STANDBY naming the holder when this
    /// machine is not paired to it.
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
        hosts.send(&host, operation).await.map_err(failure_of)
    }
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

/// Grants on the holder from the owner's account, with the account session the existing
/// redeem flow takes (`FERRYX_ACCOUNT_SESSION_TOKEN` for this machine's account origin, see
/// [`crate::cli::billing::resolve_session_token`]). No session: not paired to the holder.
pub(crate) struct AccountService;

#[async_trait]
impl AccountGrants for AccountService {
    async fn grant_on(&self, holder_machine_id: &str) -> Result<RedeemGrantRequest, String> {
        use crate::remote::account_protocol::{AccountGrantRequest, AccountGrantResponse, AccountGrantScope};
        let record = crate::ipc::run_blocking(|| Ok(crate::account::enroll_client::load_enrollment_record()))
            .await
            .ok()
            .flatten()
            .ok_or("this machine is not enrolled in an account")?;
        let origin = record.account_origin.trim_end_matches('/').to_owned();
        let token = crate::cli::billing::resolve_session_token(&origin).map_err(|error| error.to_string())?;
        let identity = crate::remote::auth::canonical_identity_dir()
            .and_then(|dir| crate::remote::auth::load_or_generate_machine_identity(&dir))?;
        let attach_public_key =
            crate::paired_host::secure_http::client_attach_public_key().map_err(|error| error.code)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoped_contracts::Epoch;

    fn host(machine_id: &str, status: AuthStatus) -> HostView {
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

    /// Paired hosts that start empty; a redeem adds the holder. Sends are recorded.
    #[derive(Default)]
    pub(crate) struct FakeHosts {
        pub hosts: parking_lot::Mutex<Vec<HostView>>,
        pub redeemed: parking_lot::Mutex<Vec<String>>,
        pub sent: parking_lot::Mutex<Vec<(String, Value)>>,
    }

    #[async_trait]
    impl PairedHosts for FakeHosts {
        async fn list(&self) -> Result<Vec<HostView>, String> {
            Ok(self.hosts.lock().clone())
        }
        async fn redeem(&self, request: RedeemGrantRequest) -> Result<HostView, String> {
            self.redeemed.lock().push(request.machine_id.clone());
            let paired = host(&request.machine_id, AuthStatus::Paired);
            self.hosts.lock().push(paired.clone());
            Ok(paired)
        }
        async fn send(&self, host: &HostView, operation: Operation) -> Result<Value, ClientError> {
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
}
