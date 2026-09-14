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
pub struct ServiceError { pub code: String, pub message: String }
impl ServiceError {
    pub fn unavailable() -> Self { InventoryError::Unavailable.into() }
    fn invalid() -> Self { InventoryError::InvalidInput.into() }
    fn migration() -> Self { InventoryError::MigrationPending.into() }
}
impl From<InventoryError> for ServiceError {
    fn from(error: InventoryError) -> Self {
        let code = match error {
            InventoryError::InvalidInput => "PAIRED_HOST_INVALID_INPUT",
            InventoryError::StaleGeneration => "PAIRED_HOST_STALE_GENERATION",
            InventoryError::Unauthorized => "PAIRED_HOST_UNAUTHORIZED",
            InventoryError::MigrationPending => "PAIRED_HOST_MIGRATION_PENDING",
            _ => "PAIRED_HOST_UNAVAILABLE",
        };
        Self { code: code.into(), message: error.to_string() }
    }
}
pub type Result<T> = std::result::Result<T, ServiceError>;

struct Owner {
    directory: PathBuf,
    inventory: Option<std::result::Result<Inventory, InventoryError>>,
    #[cfg(test)]
    loopback_http: bool,
}
pub struct PairedHostService {
    inventory: Arc<Mutex<Owner>>,
    http: reqwest::Client,
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
            #[cfg(test)]
            loopback_http: false,
        }
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
        self.run(move |store| store.forget(&host_id, expected)).await
    }
    async fn json<T: DeserializeOwned>(&self, request: reqwest::RequestBuilder) -> Result<T> {
        let mut response = request.send().await.map_err(|_| ServiceError::unavailable())?;
        if matches!(response.status().as_u16(), 401 | 403) { return Err(InventoryError::Unauthorized.into()); }
        if !response.status().is_success() { return Err(ServiceError::unavailable()); }
        const LIMIT: usize = 64 * 1024;
        if response.content_length().is_some_and(|size| size > LIMIT as u64) { return Err(ServiceError::unavailable()); }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| ServiceError::unavailable())? {
            if bytes.len() + chunk.len() > LIMIT { return Err(ServiceError::unavailable()); }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| ServiceError::unavailable())
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
        tokio::time::timeout(Duration::from_secs(30), self.pair_inner(request)).await.map_err(|_| ServiceError::unavailable())?
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
        tokio::time::timeout(Duration::from_secs(20), self.migrate_inner(request)).await.map_err(|_| ServiceError::migration())?
            .map_err(|_| ServiceError::migration())
    }
    async fn migrate_inner(&self, request: MigrationRequest) -> Result<MigrationReceipt> {
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
            store.migrate_copy(&legacy)
        }).await
    }
}
