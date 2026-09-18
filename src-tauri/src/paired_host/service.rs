//! Native pairing orchestration. Only the daemon constructs this authority.
#[cfg(test)]
mod native_transport_tests {
    use super::*;
    #[tokio::test]
    async fn redirect_never_forwards_credential_and_failed_migration_preserves_source() {
        let root = tempfile::tempdir().unwrap();
        let sink = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let destination = format!("http://{}/stolen", sink.local_addr().unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
        let router = axum::Router::new().route("/host/a/api/v1/capabilities", axum::routing::get(move |headers: axum::http::HeaderMap| async move {
            assert_eq!(headers.get("authorization").unwrap(), "Bearer private-fixture-secret");
            axum::response::Redirect::temporary(&destination)
        }));
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move { axum::serve(listener, router).await });
        // Subscribe before the migration; sink accepts are checked without timing windows.
        tasks.spawn(async move { let accepted = sink.accept().await; let _ = seen_tx.send(()); accepted.map(|_| ()) });
        let service = PairedHostService::open_test_loopback(root.path().join("data"));
        let source = root.path().join("legacy");
        std::fs::write(&source, b"private-fixture-secret").unwrap();
        let outcome = service.migrate_legacy(MigrationRequest { relay_origin: origin, machine_id: "a".into(), display_label: "a".into(), device_token: Secret("private-fixture-secret".into()) }).await;
        tasks.shutdown().await;
        assert_eq!(outcome.unwrap_err().code, "PAIRED_HOST_MIGRATION_PENDING");
        assert!(seen_rx.await.is_err(), "redirect was followed");
        assert_eq!(std::fs::read(&source).unwrap(), b"private-fixture-secret");
        assert!(service.list().await.unwrap().is_empty());
        let receipt = root.path().to_owned(); root.close().unwrap();
        eprintln!("A13 redirect_rejected=true no_token_forwarding=true migration_source_retained=true cleanup={}", !receipt.exists());
    }
    #[tokio::test]
    async fn paused_authenticated_response_cannot_adopt_after_repair_or_forget() {
        use axum::{routing::{get, post}, Json};
        use serde_json::json;
        let root = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let origin = format!("http://{address}");
        // Each delayed HTTP request publishes its release gate before awaiting it.
        let (arrived, mut arrivals) = tokio::sync::mpsc::channel::<tokio::sync::oneshot::Sender<()>>(2);
        let router = axum::Router::new()
            .route("/api/v1/pair/exchange", post(|Json(input): Json<serde_json::Value>| async move {
                let pin = input["pin"].as_str().unwrap();
                Json(json!({"token": pin, "machineId": if pin == "other" { "b" } else { "a" },
                    "device": {"id":"fixture", "name":"fixture", "permission":"control", "accessScope":"machine", "createdAt":1, "lastSeenAt":1}}))
            }))
            .route("/host/{machine}/api/v1/capabilities", get(move |axum::extract::Path(machine): axum::extract::Path<String>, headers: axum::http::HeaderMap| {
                let arrived = arrived.clone();
                async move {
                    let authorization = headers.get("authorization").unwrap().to_str().unwrap();
                    assert!(authorization.starts_with("Bearer "));
                    if authorization == "Bearer delayed" {
                        let (release, gate) = tokio::sync::oneshot::channel();
                        arrived.send(release).await.unwrap();
                        tokio::time::timeout(Duration::from_secs(5), gate).await.unwrap().unwrap();
                    }
                    Json(json!({"apiVersion":1,"machineId":machine,"daemonEpoch":"1","platform":"macos","accessScope":"machine","permission":"control","capabilities":[],"limits":{"directoryEntries":10,"terminalSessions":10}}))
                }
            }));
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move { axum::serve(listener, router).await });
        let service = PairedHostService::open_test_loopback(root.path().join("data"));
        let pair = |pin: &str| PairRequest { relay_origin: origin.clone(), pin: Secret(pin.into()), display_label: "fixture".into() };
        let result = tokio::time::timeout(Duration::from_secs(20), async {
            let other = service.pair(pair("other")).await.unwrap();
            let initial = service.pair(pair("initial")).await.unwrap();
            for forget in [false, true] {
                let pending = service.pair(pair("delayed"));
                let mutation = async {
                    let release = arrivals.recv().await.unwrap();
                    let replacement = service.pair(pair("replacement")).await.unwrap();
                    if forget { service.forget(replacement.host_id.clone(), replacement.generation).await.unwrap(); }
                    release.send(()).unwrap();
                    replacement
                };
                let (pending, replacement) = tokio::join!(pending, mutation);
                assert_eq!(pending.unwrap_err().code, "PAIRED_HOST_STALE_GENERATION");
                assert_eq!(service.read(MigrationReceipt { host_id: other.host_id.clone(), generation: other.generation }).await.unwrap(), other);
                let rows = service.list().await.unwrap();
                if forget { assert_eq!(rows, vec![other.clone()]); }
                else { assert!(rows.contains(&replacement)); assert_eq!(replacement.generation.0, initial.generation.0 + 1); }
            }
        }).await;
        tasks.shutdown().await;
        let refused = tokio::net::TcpStream::connect(address).await.is_err();
        let path = root.path().to_owned(); root.close().unwrap();
        eprintln!("A13 paused_authenticated_http=true subscribed_gate=true stale_repair_rejected=true forgotten_tombstone_rejected=true other_host_preserved=true listener_refused={refused} root_removed={}", !path.exists());
        assert!(refused); result.unwrap();
    }
    #[test]
    fn secret_debug_and_production_origin_policy() {
        let request = PairRequest { relay_origin: "https://relay.example".into(), pin: Secret("secret-pin".into()), display_label: "host".into() };
        assert!(!format!("{:?}", crate::daemon::protocol::DaemonRequest::PairedHostPair { request }).contains("secret-pin"));
        for origin in ["http://127.0.0.1:1234", "http://public.example", "https://user@relay.example", "https://relay.example/?token=secret", "https://relay.example/host/a"] {
            assert!(inventory::normalize_origin(origin).is_err());
        }
    }

    #[test]
    fn r5_n1_multibyte_error_message_truncates_on_char_boundary() {
        let message: String = "あ".repeat(200);
        let body = format!(r#"{{"message":"{message}"}}"#);
        let error = map_http_error(reqwest::StatusCode::BAD_REQUEST, body.as_bytes());
        assert_eq!(error.message.chars().count(), 170);
        assert_eq!(error.message.len(), 510);
    }

    #[test]
    fn r5_n1_mixed_boundary_message_stays_valid_utf8() {
        let ascii: String = "a".repeat(170);
        let multibyte: String = "あ".repeat(171);
        let body = format!(r#"{{"message":"{ascii}{multibyte}"}}"#);
        let error = map_http_error(reqwest::StatusCode::BAD_REQUEST, body.as_bytes());
        assert!(error.message.ends_with('あ'));
    }

    #[test]
    fn r5_n2_error_details_projection_is_allowlisted_and_sanitized() {
        let body = r#"{"code":"UNAUTHORIZED","message":"denied","details":{"hint":"check relay","deviceToken":"0123456789abcdef0123456789abcdef"},"request_id":"req-1","machine_id":"m","nested":{"token":"t"}}"#;
        let error = map_http_error(reqwest::StatusCode::UNAUTHORIZED, body.as_bytes());
        assert_eq!(error.code, "UNAUTHORIZED");
        let details = error.details.expect("safe details retained");
        assert_eq!(details.get("hint").and_then(|v| v.as_str()), Some("check relay"));
        assert_eq!(details.get("request_id").and_then(|v| v.as_str()), Some("req-1"));
        assert!(details.get("deviceToken").is_none());
        assert!(details.get("machine_id").is_none());
        assert!(details.get("nested").is_none());
        assert!(!serde_json::to_string(&details).unwrap().contains("0123456789abcdef"));
    }
}

use super::inventory::{self, GrantScope, HostView, Inventory, InventoryError, LegacyCredential, MigrationReceipt, Pairing};
use crate::scoped_contracts::Epoch;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{path::PathBuf, sync::{Arc, Mutex}, time::Duration};

#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(pub String);
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("[REDACTED]") }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairRequest {
    pub relay_origin: String,
    pub pin: Secret,
    pub display_label: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrationRequest {
    pub relay_origin: String,
    pub machine_id: String,
    pub display_label: String,
    pub device_token: Secret,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
}
impl ServiceError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
            retryable: None,
        }
    }
    pub fn with_details(mut self, details: Option<serde_json::Value>) -> Self {
        self.details = details;
        self
    }
    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = Some(retryable);
        self
    }
    pub fn unavailable() -> Self { InventoryError::Unavailable.into() }
    fn invalid() -> Self { InventoryError::InvalidInput.into() }
    fn migration() -> Self { InventoryError::MigrationPending.into() }
}
impl From<InventoryError> for ServiceError {
    fn from(error: InventoryError) -> Self {
        let (code, retryable) = match error {
            InventoryError::InvalidInput => ("PAIRED_HOST_INVALID_INPUT", false),
            InventoryError::StaleGeneration => ("PAIRED_HOST_STALE_GENERATION", true),
            InventoryError::Unauthorized => ("PAIRED_HOST_UNAUTHORIZED", false),
            InventoryError::MigrationPending => ("PAIRED_HOST_MIGRATION_PENDING", false),
            _ => ("PAIRED_HOST_UNAVAILABLE", true),
        };
        Self {
            code: code.into(),
            message: error.to_string(),
            details: None,
            retryable: Some(retryable),
        }
    }
}
pub type Result<T> = std::result::Result<T, ServiceError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryChangeEvent {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<HostView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
}

pub type InventoryEventSink = Arc<dyn Fn(InventoryChangeEvent) + Send + Sync>;

struct Owner {
    directory: PathBuf,
    inventory: Option<std::result::Result<Inventory, InventoryError>>,
    #[cfg(test)]
    loopback_http: bool,
}
pub struct PairedHostService {
    inventory: Arc<Mutex<Owner>>,
    http: reqwest::Client,
    pub(crate) event_sink: Option<InventoryEventSink>,
    #[cfg(test)]
    loopback_http: bool,
}
impl PairedHostService {
    pub fn open(directory: PathBuf) -> Self {
        // Loading is deferred to the first off-thread gated operation. Constructing
        // the synchronous daemon never joins a disk worker on an executor thread.
        Self {
            inventory: Arc::new(Mutex::new(Owner { directory, inventory: None,
                #[cfg(test)]
                loopback_http: false,
            })), 
            http: reqwest::Client::builder().no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(12)).build().expect("native HTTP client"),
            event_sink: None,
            #[cfg(test)]
            loopback_http: false,
        }
    }
    pub fn set_event_sink(&mut self, sink: InventoryEventSink) {
        self.event_sink = Some(sink);
    }
    /// Test/audit hook: whether a production event sink was installed.
    pub fn has_event_sink(&self) -> bool {
        self.event_sink.is_some()
    }
    #[cfg(test)]
    pub(crate) fn open_test_loopback(directory: PathBuf) -> Self {
        let mut service = Self::open(directory.clone());
        service.inventory.lock().unwrap().loopback_http = true;
        service.loopback_http = true;
        service
    }
    #[cfg(test)]
    pub(crate) async fn test_capture(&self, host: String, generation: Epoch) -> Result<inventory::CredentialLease> {
        self.run(move |store| store.capture(&host, generation)).await
    }
    pub(crate) async fn capture_operation(&self, host: String, generation: Epoch) -> Result<(HostView, inventory::CredentialLease)> {
        self.run(move |store| Ok((store.read_verified(&host, generation)?, store.capture(&host, generation)?))).await
    }
    pub(crate) async fn current_generation(&self, host: String, generation: Epoch) -> Result<()> {
        self.run(move |store| store.read_verified(&host, generation).map(|_| ())).await
    }
    pub async fn available(&self) -> bool { self.run(|store| store.generation_snapshot().map(|_| ())).await.is_ok() }
    async fn run<T: Send + 'static>(&self, operation: impl FnOnce(&mut Inventory) -> std::result::Result<T, InventoryError> + Send + 'static) -> Result<T> {
        let inventory = Arc::clone(&self.inventory);
        crate::ipc::run_blocking(move || {
            let result = (|| {
                let mut owner = inventory.lock().map_err(|_| InventoryError::Unavailable)?;
                if owner.inventory.is_none() {
                    #[cfg(test)]
                    let loaded = if owner.loopback_http { Inventory::open_test_loopback(&owner.directory) }
                        else { Inventory::open(&owner.directory) };
                    #[cfg(not(test))]
                    let loaded = Inventory::open(&owner.directory);
                    owner.inventory = Some(loaded);
                }
                operation(owner.inventory.as_mut().expect("loaded under gate").as_mut().map_err(|error| *error)?)
            })();
            Ok(result)
        }).await.map_err(|_| ServiceError::unavailable())?.map_err(Into::into)
    }
    fn origin(&self, value: &str) -> Result<String> {
        #[cfg(test)]
        if self.loopback_http { return inventory::normalize_test_loopback_origin(value).map_err(Into::into); }
        inventory::normalize_origin(value).map_err(Into::into)
    }
    pub async fn list(&self) -> Result<Vec<HostView>> { self.run(|store| Ok(store.list())).await }
    pub async fn read(&self, request: MigrationReceipt) -> Result<HostView> {
        self.run(move |store| store.read_verified(&request.host_id, request.generation)).await
    }
    pub async fn forget(&self, host_id: String, expected: Epoch) -> Result<()> {
        let host_id_clone = host_id.clone();
        self.run(move |store| store.forget(&host_id, expected)).await?;
        if let Some(sink) = &self.event_sink {
            sink(InventoryChangeEvent {
                r#type: "forget".into(),
                host: None,
                host_id: Some(host_id_clone),
                generation: Some(expected.0.to_string()),
            });
        }
        Ok(())
    }

    pub async fn revoke_on_auth_failure(&self, host_id: String, generation: Epoch) {
        let view = {
            let id = host_id.clone();
            self.run(move |store| {
                store.mark_auth_unavailable(&id, generation, true)?;
                store
                    .list()
                    .into_iter()
                    .find(|v| v.host_id == id)
                    .ok_or(InventoryError::Unavailable)
            })
            .await
        };
        if let Ok(view) = view {
            let generation = view.generation.0.to_string();
            if let Some(sink) = &self.event_sink {
                sink(InventoryChangeEvent {
                    r#type: "revoke".into(),
                    host: Some(view),
                    host_id: Some(host_id),
                    generation: Some(generation),
                });
            }
        }
    }
    async fn json<T: DeserializeOwned>(&self, request: reqwest::RequestBuilder) -> Result<T> {
        let response = match request.send().await {
            Ok(resp) => resp,
            Err(e) => {
                if e.is_timeout() {
                    return Err(ServiceError {
                        code: "TIMEOUT".into(),
                        message: "Request timed out".into(),
                        details: None,
                        retryable: Some(true),
                    });
                }
                return Err(ServiceError {
                    code: "TRANSPORT".into(),
                    message: "Transport connection failed".into(),
                    details: None,
                    retryable: Some(true),
                });
            }
        };

        let status = response.status();
        const LIMIT: usize = 64 * 1024;
        if response.content_length().is_some_and(|size| size > LIMIT as u64) {
            if !status.is_success() {
                return Err(map_http_error(status, &[]));
            }
            return Err(ServiceError {
                code: "MALFORMED_RESPONSE".into(),
                message: "Response payload exceeds maximum allowed size".into(),
                details: None,
                retryable: Some(false),
            });
        }

        let mut bytes = Vec::new();
        let mut stream = response;
        while let Some(chunk) = stream.chunk().await.map_err(|e| {
            if e.is_timeout() {
                ServiceError {
                    code: "TIMEOUT".into(),
                    message: "Response stream timed out".into(),
                    details: None,
                    retryable: Some(true),
                }
            } else {
                ServiceError {
                    code: "TRANSPORT".into(),
                    message: "Transport stream error".into(),
                    details: None,
                    retryable: Some(true),
                }
            }
        })? {
            if bytes.len() + chunk.len() > LIMIT {
                if !status.is_success() {
                    return Err(map_http_error(status, &bytes));
                }
                return Err(ServiceError {
                    code: "MALFORMED_RESPONSE".into(),
                    message: "Response payload exceeds maximum allowed size".into(),
                    details: None,
                    retryable: Some(false),
                });
            }
            bytes.extend_from_slice(&chunk);
        }

        if !status.is_success() {
            return Err(map_http_error(status, &bytes));
        }

        serde_json::from_slice(&bytes).map_err(|e| {
            ServiceError {
                code: "MALFORMED_RESPONSE".into(),
                message: format!("Failed to parse response JSON: {e}"),
                details: None,
                retryable: Some(false),
            }
        })
    }
    async fn authenticate(&self, origin: &str, machine: &str, token: &str) -> Result<GrantScope> {
        if token.is_empty() || token.len() > 8192 || !token.bytes().all(|b| (33..=126).contains(&b)) { return Err(ServiceError::invalid()); }
        let host_id = inventory::host_key(origin, machine)?;
        let caps: crate::remote::machine_protocol::Capabilities = self.json(self.http.get(format!("{host_id}/api/v1/capabilities")).bearer_auth(token)).await?;
        if caps.machine_id != machine || (caps.access_scope == crate::remote::machine_protocol::AccessScope::Machine
            && caps.permission != crate::remote::machine_protocol::Permission::Control) { return Err(InventoryError::Unauthorized.into()); }
        Ok(match caps.access_scope {
            crate::remote::machine_protocol::AccessScope::Machine => GrantScope::Machine,
            crate::remote::machine_protocol::AccessScope::Mirror => GrantScope::Mirror,
        })
    }
    pub async fn pair(&self, request: PairRequest) -> Result<HostView> {
        let host = tokio::time::timeout(Duration::from_secs(30), self.pair_inner(request))
            .await
            .map_err(|_| ServiceError {
                code: "TIMEOUT".into(),
                message: "Pairing operation timed out after 30 seconds".into(),
                details: None,
                retryable: Some(true),
            })??;
        if let Some(sink) = &self.event_sink {
            sink(InventoryChangeEvent {
                r#type: "pair".into(),
                host: Some(host.clone()),
                host_id: Some(host.host_id.clone()),
                generation: Some(host.generation.0.to_string()),
            });
        }
        Ok(host)
    }
    async fn pair_inner(&self, request: PairRequest) -> Result<HostView> {
        let origin = self.origin(&request.relay_origin)?;
        for value in [&request.display_label] {
            if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) { return Err(ServiceError::invalid()); }
        }
        if request.pin.0.is_empty() || request.pin.0.len() > 256 || !request.pin.0.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') { return Err(ServiceError::invalid()); }
        let snapshot = self.run(|store| store.generation_snapshot()).await?;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Exchange { token: Secret, machine_id: String, device: crate::remote::auth::DeviceInfo }
        let exchanged: Exchange = self.json(self.http.post(format!("{origin}/api/v1/pair/exchange"))
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&serde_json::json!({"pin": request.pin.0, "deviceName": "Ferryx Desktop"})).map_err(|_| ServiceError::invalid())?)).await?;
        let grant_scope = self.authenticate(&origin, &exchanged.machine_id, &exchanged.token.0).await?;
        let issued_scope = match exchanged.device.access_scope {
            crate::remote::auth::DeviceAccessScope::Machine => GrantScope::Machine,
            crate::remote::auth::DeviceAccessScope::Mirror => GrantScope::Mirror,
        };
        if issued_scope != grant_scope { return Err(InventoryError::Unauthorized.into()); }
        let host_id = inventory::host_key(&origin, &exchanged.machine_id)?;
        let pairing = Pairing { relay_origin: origin, machine_id: exchanged.machine_id,
            display_label: request.display_label, grant_scope, device_token: exchanged.token.0 };
        self.run(move |store| {
            if store.generation_snapshot()?.get(&host_id) != snapshot.get(&host_id) { return Err(InventoryError::StaleGeneration); }
            store.pair(&pairing)
        }).await
    }
    pub async fn migrate_legacy(&self, request: MigrationRequest) -> Result<MigrationReceipt> {
        let (receipt, view) = tokio::time::timeout(Duration::from_secs(20), self.migrate_inner(request))
            .await
            .map_err(|_| ServiceError::migration())?
            .map_err(|_| ServiceError::migration())?;
        let generation = receipt.generation.0.to_string();
        if let Some(sink) = &self.event_sink {
            sink(InventoryChangeEvent {
                r#type: "migrate".into(),
                host: Some(view),
                host_id: Some(receipt.host_id.clone()),
                generation: Some(generation),
            });
        }
        Ok(receipt)
    }
    async fn migrate_inner(&self, request: MigrationRequest) -> Result<(MigrationReceipt, HostView)> {
        let origin = self.origin(&request.relay_origin)?;
        let host_id = inventory::host_key(&origin, &request.machine_id)?;
        let snapshot = self.run(|store| store.generation_snapshot()).await?;
        let grant_scope = self.authenticate(&origin, &request.machine_id, &request.device_token.0).await?;
        let legacy = LegacyCredential { host_id, pairing: Pairing {
            relay_origin: origin, machine_id: request.machine_id, display_label: request.display_label,
            grant_scope, device_token: request.device_token.0,
        }};
        self.run(move |store| {
            if store.generation_snapshot()?.get(&legacy.host_id) != snapshot.get(&legacy.host_id) { return Err(InventoryError::MigrationPending); }
            let receipt = store.migrate_copy(&legacy)?;
            let view = store
                .list()
                .into_iter()
                .find(|v| v.host_id == receipt.host_id)
                .ok_or(InventoryError::MigrationPending)?;
            Ok((receipt, view))
        }).await
    }
}

fn map_http_error(status: reqwest::StatusCode, bytes: &[u8]) -> ServiceError {
    let json_obj = serde_json::from_slice::<serde_json::Value>(bytes).ok();

    // Check for explicit code in JSON response
    let explicit_code = json_obj.as_ref().and_then(|v| {
        v.get("code")
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                v.get("error").and_then(|e| {
                    if let Some(s) = e.as_str() {
                        if s.chars().all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()) {
                            Some(s)
                        } else {
                            None
                        }
                    } else if let Some(err_obj) = e.as_object() {
                        err_obj.get("code").and_then(serde_json::Value::as_str)
                    } else {
                        None
                    }
                })
            })
    });

    // Extract message if present
    let raw_message = json_obj.as_ref().and_then(|v| {
        v.get("message")
            .and_then(serde_json::Value::as_str)
            .or_else(|| v.get("error").and_then(serde_json::Value::as_str))
            .or_else(|| {
                v.get("error")
                    .and_then(serde_json::Value::as_object)
                    .and_then(|err_obj| err_obj.get("message").and_then(serde_json::Value::as_str))
            })
    }).or_else(|| {
        if !bytes.is_empty() && bytes.len() <= 2048 {
            std::str::from_utf8(bytes).ok()
        } else {
            None
        }
    });

    // Map to canonical code and retryable flag
    let (code, retryable) = if let Some(code) = explicit_code {
        match code {
            "PIN_EXPIRED" | "EXPIRED_PIN" => ("PIN_EXPIRED", false),
            "INVALID_PIN" | "INVALID_CODE" => ("INVALID_PIN", false),
            "PIN_NOT_FOUND" => ("PIN_NOT_FOUND", false),
            "pairing_rate_limited" | "RATE_LIMITED" => ("RATE_LIMITED", true),
            "WRONG_RELAY" | "INVALID_RELAY_ORIGIN" => ("WRONG_RELAY", false),
            "MACHINE_GRANT_REQUIRED" => ("MACHINE_GRANT_REQUIRED", false),
            "UNAUTHORIZED" | "PAIRED_HOST_UNAUTHORIZED" => ("UNAUTHORIZED", false),
            "SERVICE_UNAVAILABLE" | "MACHINE_SERVICE_UNAVAILABLE" => ("SERVICE_UNAVAILABLE", true),
            "TIMEOUT" => ("TIMEOUT", true),
            other => (other, is_retryable_http_status(status.as_u16())),
        }
    } else {
        match status.as_u16() {
            400 => {
                if raw_message.is_some_and(|m| {
                    let lower = m.to_ascii_lowercase();
                    lower.contains("pin") || lower.contains("code")
                }) {
                    ("INVALID_PIN", false)
                } else {
                    ("PAIRED_HOST_INVALID_INPUT", false)
                }
            }
            401 | 403 => {
                if raw_message.is_some_and(|m| m.to_ascii_lowercase().contains("expired")) {
                    ("PIN_EXPIRED", false)
                } else {
                    ("UNAUTHORIZED", false)
                }
            }
            404 => ("PIN_NOT_FOUND", false),
            429 => ("RATE_LIMITED", true),
            502 | 503 => ("SERVICE_UNAVAILABLE", true),
            504 => ("TIMEOUT", true),
            _ => ("PAIRED_HOST_UNAVAILABLE", true),
        }
    };

    let message = if let Some(msg) = raw_message {
        sanitize_error_text(truncate_utf8_boundary(msg.trim(), 512))
    } else {
        match code {
            "PIN_NOT_FOUND" => "The pairing PIN was not found or is invalid".into(),
            "RATE_LIMITED" => "Pairing rate limited, please wait and retry".into(),
            "SERVICE_UNAVAILABLE" => "Pairing relay or host service unavailable".into(),
            "UNAUTHORIZED" => "Pairing authorization was rejected".into(),
            "PIN_EXPIRED" => "The pairing PIN has expired".into(),
            "INVALID_PIN" => "Invalid pairing PIN".into(),
            "TIMEOUT" => "The pairing request timed out".into(),
            "TRANSPORT" => "Transport error connecting to relay or host".into(),
            _ => format!("Pairing failed with status {}", status.as_u16()),
        }
    };

    let details = project_safe_details(json_obj.as_ref());

    ServiceError {
        code: code.into(),
        message,
        details,
        retryable: Some(retryable),
    }
}

fn is_retryable_http_status(status: u16) -> bool {
    matches!(status, 429 | 502 | 503 | 504)
}

fn truncate_utf8_boundary(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn project_safe_details(json_obj: Option<&serde_json::Value>) -> Option<serde_json::Value> {
    const SAFE_DETAIL_KEYS: [&str; 4] = ["request_id", "reason", "hint", "detail"];
    let obj = json_obj?.as_object()?;
    let mut projected = serde_json::Map::new();
    let sources = [obj.get("details").and_then(|d| d.as_object()), Some(obj)];
    for source in sources.into_iter().flatten() {
        for key in SAFE_DETAIL_KEYS {
            if projected.contains_key(key) {
                continue;
            }
            if let Some(value) = source.get(key).and_then(serde_json::Value::as_str) {
                let sanitized = sanitize_error_text(truncate_utf8_boundary(value.trim(), 512));
                if !sanitized.is_empty() {
                    projected.insert(key.to_string(), serde_json::Value::String(sanitized));
                }
            }
        }
    }
    if projected.is_empty() { None } else { Some(serde_json::Value::Object(projected)) }
}

fn sanitize_error_text(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut word = String::new();

    let flush_word = |word: &str, result: &mut String| {
        if word.len() >= 32 && word.chars().all(|c| c.is_ascii_hexdigit()) {
            result.push_str("[REDACTED]");
        } else if word.len() == 6 && word.chars().all(|c| c.is_ascii_digit()) {
            result.push_str("[REDACTED]");
        } else {
            result.push_str(word);
        }
    };

    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            word.push(c);
        } else {
            if !word.is_empty() {
                flush_word(&word, &mut result);
                word.clear();
            }
            result.push(c);
        }
    }
    if !word.is_empty() {
        flush_word(&word, &mut result);
    }
    result
}

