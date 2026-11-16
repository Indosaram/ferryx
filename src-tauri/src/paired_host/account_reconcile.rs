//! Pairs every machine of the signed-in account with no registration or Connect step.
//!
//! Signing in *is* the connection. This lives in the daemon, not a UI hook, because the daemon
//! already owns the account credential, the generation-fenced inventory, and single-process
//! serialization; a hook would make a second owner and would redeem twice with two windows open.
//!
//! Nothing here polls or sleeps: a retryable failure waits for the next observed event, and a
//! terminal one parks that machine for the daemon's lifetime.

use std::collections::HashSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use super::service::{PairedHostService, RedeemGrantRequest, Secret};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountMachine {
    pub machine_id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub machine_record_id: Option<String>,
    /// The account service serializes this as a JSON number while the paired-host inventory
    /// stores it as text, so it is read as either and normalized by [`AccountMachine::epoch`].
    #[serde(default)]
    pub enrollment_epoch: Option<serde_json::Value>,
}

impl AccountMachine {
    fn epoch(&self) -> Option<String> {
        match self.enrollment_epoch.as_ref()? {
            serde_json::Value::String(text) => Some(text.clone()),
            other => Some(other.to_string()),
        }
        .filter(|text| !text.is_empty())
    }
}

fn machines_of(body: &serde_json::Value) -> Vec<AccountMachine> {
    body.as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| serde_json::from_value::<AccountMachine>(row.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileSkip {
    pub machine_id: String,
    pub code: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileReport {
    pub paired: Vec<String>,
    pub already_usable: Vec<String>,
    pub skipped: Vec<ReconcileSkip>,
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Retryable,
    Terminal,
}

fn classify(code: &str) -> Class {
    match code {
        // A machine that is not usable yet is retried on the next observed event: it becomes
        // reachable, its epoch moves, or the account finishes provisioning it.
        "RATE_LIMITED" | "SERVICE_UNAVAILABLE" | "TIMEOUT" | "HOST_UNAVAILABLE" | "UNAUTHORIZED"
        | "ENROLLMENT_EPOCH_STALE" | "MACHINE_NOT_FOUND" => Class::Retryable,
        _ => Class::Terminal,
    }
}

#[derive(Default, Clone)]
pub struct ReconcileState {
    parked: Arc<Mutex<HashSet<String>>>,
    in_flight: Arc<Mutex<HashSet<String>>>,
}

impl ReconcileState {
    async fn is_parked(&self, machine_id: &str) -> bool {
        self.parked.lock().await.contains(machine_id)
    }

    async fn park(&self, machine_id: &str) {
        self.parked.lock().await.insert(machine_id.to_owned());
    }

    async fn claim(&self, machine_id: &str) -> Option<InFlight<'_>> {
        if !self.in_flight.lock().await.insert(machine_id.to_owned()) {
            return None;
        }
        Some(InFlight {
            state: self,
            machine_id: machine_id.to_owned(),
        })
    }
}

struct InFlight<'a> {
    state: &'a ReconcileState,
    machine_id: String,
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        let machine = std::mem::take(&mut self.machine_id);
        let parked = Arc::clone(&self.state.in_flight);
        // Drop cannot await. A leaked claim would block this machine for the daemon's life, so
        // the release is scheduled instead of skipped.
        tokio::spawn(async move {
            parked.lock().await.remove(&machine);
        });
    }
}

struct Credential {
    origin: String,
    token: String,
}

impl Credential {
    fn read() -> Option<Self> {
        let session = crate::account::enroll_client::load_account_session()?;
        let origin = session.account_origin.trim_end_matches('/').to_owned();
        if origin.is_empty() || session.session_token.trim().is_empty() {
            return None;
        }
        Some(Self {
            origin,
            token: session.session_token,
        })
    }
}

fn attach_public_key() -> Option<String> {
    crate::paired_host::secure_http::client_attach_public_key().ok()
}

fn installation_id() -> Option<String> {
    let dir = crate::remote::auth::canonical_identity_dir().ok()?;
    crate::remote::auth::load_or_generate_machine_identity(&dir)
        .ok()
        .map(|identity| identity.machine_id)
}

fn transport_code(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "TIMEOUT".into()
    } else {
        "SERVICE_UNAVAILABLE".into()
    }
}

impl PairedHostService {
    /// Idempotent: an already-usable host costs one inventory read and no network call.
    pub async fn reconcile_account_machines(&self) -> ReconcileReport {
        let mut report = ReconcileReport::default();
        let Some(credential) = crate::ipc::run_blocking(|| Ok(Credential::read()))
            .await
            .ok()
            .flatten()
        else {
            report.blocked = Some("NOT_SIGNED_IN".into());
            return report;
        };
        let machines = match self.account_machines(&credential).await {
            Ok(machines) => machines,
            Err(code) => {
                report.blocked = Some(code);
                return report;
            }
        };
        let paired = match self.paired_machine_epochs().await {
            Ok(epochs) => epochs,
            Err(_) => {
                report.blocked = Some("INVENTORY_UNAVAILABLE".into());
                return report;
            }
        };
        let local = crate::ipc::run_blocking(|| Ok(installation_id()))
            .await
            .ok()
            .flatten();
        for machine in machines {
            if Some(machine.machine_id.as_str()) == local.as_deref() {
                continue;
            }
            if self.reconcile_state.is_parked(&machine.machine_id).await {
                continue;
            }
            let current = machine.epoch();
            if current.is_some() && paired.get(&machine.machine_id) == current.as_ref() {
                report.already_usable.push(machine.machine_id);
                continue;
            }
            let Some(claim) = self.reconcile_state.claim(&machine.machine_id).await else {
                // Another pass is pairing this machine right now.
                continue;
            };
            let result = self.redeem_machine(&credential, &machine).await;
            drop(claim);
            match result {
                Ok(host_id) => report.paired.push(host_id),
                Err(code) => {
                    if classify(&code) == Class::Terminal {
                        self.reconcile_state.park(&machine.machine_id).await;
                    }
                    report.skipped.push(ReconcileSkip {
                        machine_id: machine.machine_id,
                        code,
                    });
                }
            }
        }
        report
    }

    /// Signing in *is* the registration: this writes the daemon's own account session, enrolls
    /// this machine silently when the account has no record for it yet, then pairs every machine
    /// of the account. Signing out passes `None` and only forgets the credential.
    pub async fn adopt_account_session(
        &self,
        origin: String,
        session_token: Option<String>,
    ) -> ReconcileReport {
        let origin = origin.trim_end_matches('/').to_owned();
        match session_token {
            Some(token) if !token.trim().is_empty() => {
                let saved = {
                    let origin = origin.clone();
                    let token = token.trim().to_owned();
                    crate::ipc::run_blocking(move || {
                        crate::account::enroll_client::save_account_session(&origin, &token)
                            .map_err(|error| {
                                crate::ipc::error::IpcError::internal(format!(
                                    "account session was not saved: {error}"
                                ))
                            })
                    })
                    .await
                };
                if let Err(error) = saved {
                    tracing::warn!(%error, "the account session was not saved for the daemon");
                    return ReconcileReport {
                        blocked: Some("SESSION_NOT_SAVED".into()),
                        ..ReconcileReport::default()
                    };
                }
                if let Err(code) = self.ensure_enrollment(&origin, token.trim()).await {
                    tracing::warn!(%code, "silent enrollment of this machine did not complete");
                    return ReconcileReport {
                        blocked: Some(code),
                        ..ReconcileReport::default()
                    };
                }
            }
            _ => {
                let cleared = crate::ipc::run_blocking(|| {
                    crate::account::enroll_client::clear_account_session()
                        .map_err(|error| {
                            crate::ipc::error::IpcError::internal(format!(
                                "account session was not cleared: {error}"
                            ))
                        })
                })
                .await;
                if let Err(error) = cleared {
                    tracing::warn!(%error, "the account session was not cleared");
                }
                return ReconcileReport {
                    blocked: Some("NOT_SIGNED_IN".into()),
                    ..ReconcileReport::default()
                };
            }
        }
        self.reconcile_account_machines().await
    }

    /// Enrolls this machine into `origin` when the account does not already hold a record for
    /// it. The enrollment code is minted from the signed-in session, so the user never sees one.
    async fn ensure_enrollment(&self, origin: &str, token: &str) -> Result<(), String> {
        if crate::ipc::run_blocking(|| Ok(crate::account::enroll_client::load_enrollment_record()))
            .await
            .ok()
            .flatten()
            .is_some_and(|record| record.account_origin.trim_end_matches('/') == origin)
        {
            return Ok(());
        }
        let code = self.mint_enrollment_code(origin, token).await?;
        let origin_owned = origin.to_owned();
        let enrolled = crate::ipc::run_blocking(move || {
            let handle = tokio::runtime::Handle::current();
            Ok(handle
                .block_on(crate::account::enroll_client::enroll_machine(&origin_owned, &code))
                .map(|_| ())
                .map_err(|error| error.code))
        })
        .await;
        match enrolled {
            Ok(Ok(())) => Ok(()),
            Ok(Err(code)) => Err(code),
            Err(_) => Err("ENROLLMENT_UNAVAILABLE".to_owned()),
        }
    }

    async fn mint_enrollment_code(&self, origin: &str, token: &str) -> Result<String, String> {
        let response = self
            .http
            .post(format!("{origin}/api/account/v1/enrollment-codes"))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|error| transport_code(&error))?;
        if !response.status().is_success() {
            return Err(match response.status().as_u16() {
                401 | 403 => "UNAUTHORIZED",
                status if status >= 500 => "SERVICE_UNAVAILABLE",
                _ => "ENROLLMENT_CODE_REFUSED",
            }
            .into());
        }
        let body: serde_json::Value = response.json().await.map_err(|_| "BAD_RESPONSE")?;
        body.get("code")
            .and_then(serde_json::Value::as_str)
            .filter(|code| !code.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| "BAD_RESPONSE".to_owned())
    }

    async fn account_machines(&self, credential: &Credential) -> Result<Vec<AccountMachine>, String> {
        let response = self
            .http
            .get(format!("{}/api/account/v1/machines", credential.origin))
            .bearer_auth(&credential.token)
            .send()
            .await
            .map_err(|error| transport_code(&error))?;
        if !response.status().is_success() {
            return Err(match response.status().as_u16() {
                401 | 403 => "UNAUTHORIZED",
                status if status >= 500 => "SERVICE_UNAVAILABLE",
                _ => "ACCOUNT_REFUSED",
            }
            .into());
        }
        let body: serde_json::Value = response.json().await.map_err(|_| "BAD_RESPONSE")?;
        Ok(machines_of(&body))
    }

    async fn redeem_machine(
        &self,
        credential: &Credential,
        machine: &AccountMachine,
    ) -> Result<String, String> {
        use crate::remote::account_protocol::AccountGrantScope;
        let Some(record_id) = machine.machine_record_id.as_deref().filter(|id| !id.is_empty()) else {
            return Err("MACHINE_RECORD_MISSING".into());
        };
        let Some(epoch) = machine.epoch() else {
            return Err("ENROLLMENT_EPOCH_MISSING".into());
        };
        let key = crate::ipc::run_blocking(|| Ok(attach_public_key()))
            .await
            .ok()
            .flatten()
            .ok_or("ATTACH_IDENTITY_UNAVAILABLE")?;
        let installation = crate::ipc::run_blocking(|| Ok(installation_id()))
            .await
            .ok()
            .flatten()
            .ok_or("IDENTITY_UNAVAILABLE")?;
        let label = machine
            .display_name
            .clone()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| machine.machine_id.clone());
        let request = crate::remote::account_protocol::AccountGrantRequest {
            machine_record_id: record_id.to_owned(),
            enrollment_epoch: epoch.clone(),
            device_label: label.clone(),
            installation_id: installation,
            grant_scope: AccountGrantScope::Machine,
            attach_public_key: key,
            // Omitted on purpose: the same-machine proof would additionally open the peer
            // routes, which a plain machine-scope pairing must not reach.
            requester_machine: None,
        };
        let response = self
            .http
            .post(format!(
                "{}/api/account/v1/machines/{record_id}/grants",
                credential.origin
            ))
            .bearer_auth(&credential.token)
            .json(&request)
            .send()
            .await
            .map_err(|error| transport_code(&error))?;
        if !response.status().is_success() {
            return Err(match response.status().as_u16() {
                401 | 403 => "UNAUTHORIZED",
                404 => "MACHINE_NOT_FOUND",
                409 => "ENROLLMENT_EPOCH_STALE",
                429 => "RATE_LIMITED",
                status if status >= 500 => "SERVICE_UNAVAILABLE",
                _ => "GRANT_REFUSED",
            }
            .into());
        }
        let grant: crate::remote::account_protocol::AccountGrantResponse =
            response.json().await.map_err(|_| "BAD_RESPONSE")?;
        let host = self
            .redeem_grant(RedeemGrantRequest {
                relay_origin: grant.relay_origin,
                machine_id: grant.machine_id,
                display_label: label,
                pairing_token: Secret(grant.pairing_token),
                machine_attach_public_key: grant.machine_attach_public_key,
                enrollment_epoch: epoch,
                account_session_token: Secret(credential.token.clone()),
            })
            .await
            .map_err(|error| error.code)?;
        Ok(host.host_id)
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_wire_shape_matches_the_ui_mirror() {
        let report = ReconcileReport {
            paired: vec!["host-a".into()],
            already_usable: vec!["host-b".into()],
            skipped: vec![ReconcileSkip {
                machine_id: "m3".into(),
                code: "MACHINE_RECORD_MISSING".into(),
            }],
            blocked: Some("SESSION_NOT_SAVED".into()),
        };
        assert_eq!(
            serde_json::to_value(&report).unwrap(),
            serde_json::json!({
                "paired": ["host-a"],
                "alreadyUsable": ["host-b"],
                "skipped": [{ "machineId": "m3", "code": "MACHINE_RECORD_MISSING" }],
                "blocked": "SESSION_NOT_SAVED",
            })
        );
        // The daemon always sends all four keys, so the renderer never has to guard for absence.
        assert_eq!(
            serde_json::to_value(ReconcileReport::default()).unwrap(),
            serde_json::json!({ "paired": [], "alreadyUsable": [], "skipped": [], "blocked": null })
        );
        assert_eq!(serde_json::from_value::<ReconcileReport>(serde_json::to_value(&report).unwrap()).unwrap(), report);
    }

    #[test]
    fn epoch_reads_numeric_and_text_forms_identically() {
        let numeric: AccountMachine = serde_json::from_value(serde_json::json!({
            "machineId": "m1",
            "enrollmentEpoch": 7,
        }))
        .expect("numeric epoch");
        let text: AccountMachine = serde_json::from_value(serde_json::json!({
            "machineId": "m1",
            "enrollmentEpoch": "7",
        }))
        .expect("text epoch");
        assert_eq!(numeric.epoch().as_deref(), Some("7"));
        assert_eq!(numeric.epoch(), text.epoch());
    }

    #[test]
    fn epoch_absent_or_empty_is_none() {
        let absent: AccountMachine =
            serde_json::from_value(serde_json::json!({ "machineId": "m1" })).expect("absent");
        let empty: AccountMachine = serde_json::from_value(serde_json::json!({
            "machineId": "m1",
            "enrollmentEpoch": "",
        }))
        .expect("empty");
        assert_eq!(absent.epoch(), None);
        assert_eq!(empty.epoch(), None);
    }

    #[test]
    fn machines_of_keeps_well_formed_rows_and_drops_the_rest() {
        let body = serde_json::json!([
            { "machineId": "a", "displayName": "Alpha", "machineRecordId": "rec-a", "enrollmentEpoch": 1 },
            { "displayName": "no id" },
            "not an object",
        ]);
        let machines = machines_of(&body);
        assert_eq!(machines.len(), 1);
        assert_eq!(machines[0].machine_id, "a");
        assert_eq!(machines[0].display_name.as_deref(), Some("Alpha"));
    }

    #[test]
    fn machines_of_treats_a_non_array_body_as_empty() {
        assert!(machines_of(&serde_json::json!({ "machines": [] })).is_empty());
    }

    #[test]
    fn transient_codes_are_retried_and_unknown_codes_park_the_machine() {
        for code in [
            "RATE_LIMITED",
            "SERVICE_UNAVAILABLE",
            "TIMEOUT",
            "HOST_UNAVAILABLE",
            "UNAUTHORIZED",
            "ENROLLMENT_EPOCH_STALE",
            "MACHINE_NOT_FOUND",
        ] {
            assert!(matches!(classify(code), Class::Retryable), "{code}");
        }
        assert!(matches!(classify("MACHINE_RECORD_MISSING"), Class::Terminal));
        assert!(matches!(classify("ACCOUNT_REFUSED"), Class::Terminal));
    }

    #[tokio::test]
    async fn a_second_pass_cannot_claim_a_machine_already_in_flight() {
        let state = ReconcileState::default();
        let claim = state.claim("m1").await.expect("first claim");
        assert!(state.claim("m1").await.is_none());
        // A different machine is independent of the first.
        assert!(state.claim("m2").await.is_some());
        drop(claim);
    }

    #[tokio::test]
    async fn dropping_a_claim_releases_the_machine() {
        let state = ReconcileState::default();
        let claim = state.claim("m1").await.expect("first claim");
        drop(claim);
        // The release is scheduled on a spawned task because Drop cannot await, so yield until
        // the pool has run it instead of assuming a fixed delay.
        for _ in 0..64 {
            if state.claim("m1").await.is_some() {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("claim was never released");
    }
}

