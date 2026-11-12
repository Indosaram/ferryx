use super::*;

pub struct Bitwarden;

pub async fn detect() -> Result<ProviderAvailability, ProviderError> {
    crate::ipc::run_blocking(|| probe("bw", EffectivePasswordProvider::Bitwarden))
        .await.map_err(|error| ProviderError::Discovery(error.to_string()))
}

#[async_trait::async_trait]
impl CredentialSource for Bitwarden {
    async fn list_for_origin(&self, _origin: &str) -> Result<Vec<VaultCredentialHandle>, ProviderError> {
        Err(refusal(detect().await?))
    }
    async fn fill_script(&self, _handle: &str, _origin: &str) -> Result<Zeroizing<String>, ProviderError> {
        Err(refusal(detect().await?))
    }
}
