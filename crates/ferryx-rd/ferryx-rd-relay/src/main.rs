use std::net::SocketAddr;

use anyhow::{Context, Result};
use ferryx_rd_relay::{config_from_env, router};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = config_from_env().context("RELAY_AUTH_SECRET")?;
    let bind = std::env::var("RELAY_BIND").unwrap_or_else(|_| "0.0.0.0:8080".to_owned());
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .with_context(|| format!("bind {bind}"))?;
    tracing::info!(%bind, "ferryx-rd-relay listening");
    let app = router(config).into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, app).await.context("serve")?;
    Ok(())
}
