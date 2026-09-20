//! Repeated owner issuance through the real CLI, UDS dispatcher, and relay.
use super::{owner_cli, Exchange};
use anyhow::ensure;
use std::path::Path;
use std::time::Duration;

pub(super) async fn repeat_after_redemption(
    binary: &Path,
    root: &Path,
    relay_url: &str,
) -> anyhow::Result<()> {
    // Given: the shared daemon coordinator has already issued a redeemed PIN.
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(8))
        .build()?;
    let endpoint = format!("{relay_url}/api/v1/pair/exchange");
    for issuance in 1..=2 {
        // When: the owner issues machine access again without changing coordinator state.
        let issued = owner_cli(binary, root, true).await?;
        let response = client
            .post(&endpoint)
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&serde_json::json!({
                "pin": issued.pin, "deviceName": "A13 repeated owner CLI",
                "permission": "view", "accessScope": "mirror"
            }))?)
            .send()
            .await?;
        // Then: issuer-approved machine Control survives forged client preferences.
        ensure!(
            response.status().is_success(),
            "repeat relay exchange refused"
        );
        let exchange: Exchange = serde_json::from_slice(&response.bytes().await?)?;
        ensure!(
            exchange.device.access_scope == crate::remote::auth::DeviceAccessScope::Machine,
            "repeated machine scope lost"
        );
        ensure!(
            exchange.device.permission == crate::remote::auth::DevicePermission::Control,
            "repeated machine permission lost"
        );
        let stale = client
            .post(&endpoint)
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&serde_json::json!({
                "pin": issued.pin, "deviceName": "A13 stale redemption"
            }))?)
            .send()
            .await?;
        ensure!(!stale.status().is_success(), "previous PIN redeemed twice");
        println!("A13 REPEAT issuance={issuance} owner_cli=0 exchange=200 scope=Machine permission=Control stale_pin_refused=true");
    }
    Ok(())
}
