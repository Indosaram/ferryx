//! `ferryx-relay` - standalone entrypoint for the public relay server that
//! brokers WebSocket tunnels between desktop daemons and remote clients.
//!
//! Usage:
//!
//! ```text
//! ferryx-relay --port 8787 --machine-token <token> [--machine-token <token> ...]
//! ```
//!
//! The port defaults to `8787` and can also be set via `FERRYX_RELAY_PORT`.
//! Machine Tokens can additionally be supplied via the comma-separated
//! `FERRYX_RELAY_MACHINE_TOKENS` environment variable; tokens from both the
//! environment and `--machine-token` flags are accepted.
//! The relay origin advertised in machine records can be configured via
//! `FERRYX_ACCOUNT_RELAY_ORIGIN`, falling back to the resolved account origin.

use base64::Engine as _;
use ferryx_lib::remote::relay_server::{
    relay_router_with_account, spawn_session_reaper, spawn_suspension_sweeper, RelayState,
};
use ferryx_lib::account::origin::{
    deployment_mode, missing_lemon_squeezy_env_vars, DeploymentMode,
};
use std::path::PathBuf;
use std::sync::Arc;

struct RelayConfig {
    port: u16,
    machine_tokens: Vec<String>,
    account_public_key: Option<String>,
    deployment_mode: DeploymentMode,
}

fn parse_args(args: &[String]) -> Result<RelayConfig, String> {
    let deployment_mode = deployment_mode()
        .map_err(|err| format!("configuration error: {}", err.message))?;

    let mut port: u16 = std::env::var("FERRYX_RELAY_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8787);
    let mut machine_tokens: Vec<String> = std::env::var("FERRYX_RELAY_MACHINE_TOKENS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(|token| token.trim().to_string())
                .filter(|token| !token.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let mut account_public_key = std::env::var("FERRYX_RELAY_ACCOUNT_PUBLIC_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty());

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "--port requires a value".to_string())?;
                port = value
                    .parse()
                    .map_err(|_| format!("invalid --port value: {value}"))?;
            }
            "--machine-token" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "--machine-token requires a value".to_string())?;
                machine_tokens.push(value.clone());
            }
            "--account-public-key" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "--account-public-key requires a value".to_string())?;
                account_public_key = Some(value.clone());
            }
            other => {
                return Err(format!("unrecognized argument: {other}"));
            }
        }
        i += 1;
    }

    Ok(RelayConfig {
        port,
        machine_tokens,
        account_public_key,
        deployment_mode,
    })
}

/// `<home>/.ferryx/account-data`, the directory holding persistent relay account
/// state and the mail spool. There is deliberately no working-directory fallback:
/// a relay started without `HOME` (Windows leaves it unset) must fail fast rather
/// than write durable account data wherever it happened to be launched from.
fn default_account_data_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .or_else(|| std::env::var_os("LOCALAPPDATA"));
    #[cfg(not(windows))]
    let base = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    base.filter(|base| !base.is_empty())
        .map(|base| PathBuf::from(base).join(".ferryx").join("account-data"))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = match parse_args(&args) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("ferryx-relay: {err}");
            std::process::exit(2);
        }
    };

    if config.machine_tokens.is_empty() {
        eprintln!(
            "ferryx-relay: no Machine Tokens configured; set FERRYX_RELAY_MACHINE_TOKENS \
             or pass --machine-token, otherwise no daemon will be able to authenticate"
        );
    }

    match config.deployment_mode {
        DeploymentMode::SelfHost => {
            tracing::info!(
                "ferryx-relay running in selfhost mode: billing disabled, all entitlement checks skipped"
            );
        }
        DeploymentMode::Commercial => {
            let missing = missing_lemon_squeezy_env_vars();
            if !missing.is_empty() {
                eprintln!(
                    "ferryx-relay: WARNING: commercial mode is running without complete Lemon Squeezy configuration; \
                     missing variables: {}. Billing routes and paid surfaces will fail closed with 503 BILLING_UNCONFIGURED",
                    missing.join(", ")
                );
            } else {
                tracing::info!("ferryx-relay running in commercial mode with Lemon Squeezy configured");
            }
        }
    }

    let state = RelayState::new(config.machine_tokens);
    spawn_session_reaper(state.clone());

    let origin = std::env::var("FERRYX_ACCOUNT_ORIGIN")
        .unwrap_or_else(|_| "https://relay.checka.cc".into());
    let relay_origin = std::env::var("FERRYX_ACCOUNT_RELAY_ORIGIN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| origin.clone());
    let data_dir = match std::env::var_os("FERRYX_ACCOUNT_DATA_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => match default_account_data_dir() {
            Some(dir) => dir,
            None => {
                eprintln!(
                    "ferryx-relay: cannot resolve the account data directory; set \
                     FERRYX_ACCOUNT_DATA_DIR or HOME (USERPROFILE on Windows), otherwise \
                     persistent account data would be written relative to the working directory"
                );
                std::process::exit(2);
            }
        },
    };
    let mailer = ferryx_lib::account::mailer::create_production_mailer(Some(data_dir.join("mail")));
    let account_state = Arc::new(
        ferryx_lib::account::service::AccountState::new(&data_dir, &origin, mailer)
            .with_relay_origin(relay_origin)
            .with_deployment_mode(config.deployment_mode),
    );

    let account_public_key = config.account_public_key.or_else(|| {
        account_state.signing_key().ok().map(|k| {
            base64::engine::general_purpose::STANDARD.encode(k.verifying_key().as_bytes())
        })
    });
    let router = relay_router_with_account(
        state.clone(),
        account_public_key,
        Some(account_state.clone()),
    );

    if matches!(config.deployment_mode, DeploymentMode::Commercial) {
        // Only a commercial relay enforces suspension. The task is detached on purpose: it
        // must outlive this scope for the whole process, like the session reaper above.
        let _suspension_sweeper = spawn_suspension_sweeper(state);

        // The grace, suspension and recovery notices are clock-driven and must fire while the
        // entitlement state stays unchanged, so the commercial relay runs one notice sweeper
        // next to the suspension sweeper. Self-host never enables billing and starts neither.
        let _notice_sweeper =
            ferryx_lib::account::billing::notices::spawn_billing_notice_sweeper(account_state);
    }

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("ferryx-relay: failed to bind {addr}: {err}");
            std::process::exit(1);
        }
    };

    tracing::info!("ferryx-relay listening on {addr}");
    if let Err(err) = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    {
        eprintln!("ferryx-relay: server error: {err}");
        std::process::exit(1);
    }
}
