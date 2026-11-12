//! Provider discovery never logs in, starts a service, or retrieves a secret.
// Vendor adapters are unverified: no shipped path may reach them until a real store is
// measured end to end. Gated to tests, so a production reference fails to compile instead of
// silently exposing a half-implemented credential path. Drop the gate (and add the measured
// test) only when a real adapter is proven.
#[cfg(test)]
pub mod bitwarden;
#[cfg(test)]
pub mod onepassword;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;
use super::{fill_for_page, CredentialOrigin, VaultStore};
use crate::ipc::browser_cli::VaultCredentialHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EffectivePasswordProvider { Disabled, FerryxNative, Bitwarden, OnePassword }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EffectivePasswordProviderDisabledReason {
    NotInstalled,
    Locked,
    Unsupported { detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAvailability {
    pub provider: EffectivePasswordProvider,
    pub installed: bool,
    pub disabled_reason: Option<EffectivePasswordProviderDisabledReason>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("password provider unavailable: {0:?}")]
    Unavailable(EffectivePasswordProviderDisabledReason),
    #[error("{detail}")]
    Unsupported { detail: String },
    #[error(transparent)]
    Vault(#[from] super::VaultError),
    #[error("provider discovery failed: {0}")]
    Discovery(String),
}

#[cfg(test)]
pub(crate) fn probe(binary: &str, provider: EffectivePasswordProvider) -> Result<ProviderAvailability, crate::ipc::error::IpcError> {
    let status = std::process::Command::new(binary).arg("--version")
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .status();
    match status {
        Ok(status) if status.success() => Ok(availability(provider, true)),
        Ok(status) => Err(crate::ipc::error::IpcError::internal(format!("{binary} --version exited with {status}"))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(availability(provider, false)),
        Err(error) => Err(crate::ipc::error::IpcError::internal(format!("could not probe {binary}: {error}"))),
    }
}

#[cfg(test)]
fn availability(provider: EffectivePasswordProvider, installed: bool) -> ProviderAvailability {
    ProviderAvailability {
        provider, installed,
        disabled_reason: Some(if installed {
            EffectivePasswordProviderDisabledReason::Unsupported { detail: "credential CLI integration is not implemented; use Ferryx native vault".into() }
        } else { EffectivePasswordProviderDisabledReason::NotInstalled }),
    }
}

#[cfg(test)]
pub(crate) fn refusal(availability: ProviderAvailability) -> ProviderError {
    if availability.installed {
        ProviderError::Unsupported { detail: "credential CLI integration is not implemented; use Ferryx native vault".into() }
    } else { ProviderError::Unavailable(EffectivePasswordProviderDisabledReason::NotInstalled) }
}

#[async_trait::async_trait]
pub trait CredentialSource: Send + Sync {
    async fn list_for_origin(&self, origin: &str) -> Result<Vec<VaultCredentialHandle>, ProviderError>;
    async fn fill_script(&self, handle: &str, origin: &str) -> Result<Zeroizing<String>, ProviderError>;
}

/// Separate from the agent service: callers still need a human-approved capability before
/// executing any script. Store access stays on blocking threads, just like the app vault.
pub struct NativeCredentialSource {
    store: std::sync::Arc<parking_lot::Mutex<VaultStore>>,
    handles: std::sync::Arc<super::grants::CredentialGrants>,
}

impl NativeCredentialSource {
    pub fn new(store: VaultStore) -> Self {
        Self { store: std::sync::Arc::new(parking_lot::Mutex::new(store)), handles: Default::default() }
    }
}

#[async_trait::async_trait]
impl CredentialSource for NativeCredentialSource {
    async fn list_for_origin(&self, origin: &str) -> Result<Vec<VaultCredentialHandle>, ProviderError> {
        let origin: CredentialOrigin = origin.to_string().try_into()
            .map_err(|error: super::origin::InvalidOrigin| ProviderError::Vault(super::VaultError::InvalidArgument(error.to_string())))?;
        let store = self.store.clone();
        let handles = self.handles.clone();
        crate::ipc::run_blocking(move || {
            Ok(store.lock().matches(origin.as_str()).into_iter().map(|item| VaultCredentialHandle {
                handle: handles.issue("", &item.id, origin.clone(), crate::browser::agent::VaultAction::Fill),
                origin: item.origin, username: item.username,
            }).collect())
        }).await.map_err(|error| ProviderError::Discovery(error.to_string()))
    }

    async fn fill_script(&self, handle: &str, origin: &str) -> Result<Zeroizing<String>, ProviderError> {
        let origin: CredentialOrigin = origin.to_string().try_into()
            .map_err(|error: super::origin::InvalidOrigin| ProviderError::Vault(super::VaultError::InvalidArgument(error.to_string())))?;
        let item = self.handles.take(handle, handle, crate::browser::agent::VaultAction::Fill, &origin)
            .ok_or_else(|| ProviderError::Unsupported { detail: "credential handle expired or origin mismatch".into() })?;
        let store = self.store.clone();
        crate::ipc::run_blocking(move || {
            let error = |error: super::VaultError| crate::ipc::error::IpcError::internal(error.to_string());
            fill_for_page(&store.lock().credential(&item).map_err(error)?, origin.as_str())
                .map_err(error)
        }).await.map_err(|error| ProviderError::Discovery(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detection_shape_is_secret_free() {
        let value = serde_json::to_value(availability(EffectivePasswordProvider::Bitwarden, false)).unwrap();
        assert_eq!(value["provider"], "bitwarden");
        assert_eq!(value["installed"], false);
        assert_eq!(value["disabledReason"]["kind"], "notInstalled");
    }
    #[test]
    fn refusal_mapping_is_honest() {
        assert!(matches!(refusal(availability(EffectivePasswordProvider::OnePassword, false)), ProviderError::Unavailable(EffectivePasswordProviderDisabledReason::NotInstalled)));
        assert!(matches!(refusal(availability(EffectivePasswordProvider::OnePassword, true)), ProviderError::Unsupported { .. }));
    }
}
