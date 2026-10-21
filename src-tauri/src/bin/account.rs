use std::sync::Arc;

use ferryx_lib::account::mailer::FileMailer;
use ferryx_lib::account::origin::{
    account_data_dir, account_origin, deployment_mode, missing_lemon_squeezy_env_vars,
    DeploymentMode,
};
use ferryx_lib::account::service::{serve, AccountState};

fn required<T>(value: Option<T>, message: &str) -> Result<T, String> {
    value.ok_or_else(|| message.to_string())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ferryx-account: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let origin = account_origin().map_err(|error| error.to_string())?;
    let mode = deployment_mode().map_err(|error| error.message)?;
    let data_dir = required(account_data_dir(), "ACCOUNT_DATA_DIR_UNRESOLVED")?;
    let bind = std::env::var("FERRYX_ACCOUNT_BIND")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "127.0.0.1:43822".to_string());
    let per_hour = std::env::var("FERRYX_ACCOUNT_LOGIN_PER_HOUR")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(ferryx_lib::account::store::DEFAULT_LOGIN_REQUESTS_PER_HOUR);
    let max_body = std::env::var("FERRYX_ACCOUNT_MAX_BODY_BYTES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(ferryx_lib::account::store::DEFAULT_MAX_BODY_BYTES);

    match mode {
        DeploymentMode::SelfHost => {
            println!("FERRYX_ACCOUNT_MODE selfhost: billing, lease and team routes are not served")
        }
        DeploymentMode::Commercial => {
            println!("FERRYX_ACCOUNT_MODE commercial: billing routes enabled");
            let missing = missing_lemon_squeezy_env_vars();
            if !missing.is_empty() {
                eprintln!(
                    "ferryx-account: WARNING: commercial mode without complete Lemon Squeezy configuration; \
                     missing variables: {}. Checkout, quantity and webhook fail closed with \
                     503 BILLING_UNCONFIGURED; Free accounts keep working",
                    missing.join(", ")
                );
            }
        }
    }

    let relay_origin = std::env::var("FERRYX_ACCOUNT_RELAY_ORIGIN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| origin.clone());

    let state = AccountState::new(data_dir, origin, Arc::new(FileMailer::new()))
        .with_relay_origin(relay_origin)
        .with_limits(per_hour, max_body)
        .with_deployment_mode(mode);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("runtime: {error}"))?;
    let listener = runtime.block_on(async {
        tokio::net::TcpListener::bind(&bind)
            .await
            .map_err(|error| format!("bind {bind}: {error}"))
    })?;
    println!(
        "FERRYX_ACCOUNT_READY {}",
        listener
            .local_addr()
            .map(|addr| addr.to_string())
            .unwrap_or(bind)
    );
    runtime.block_on(serve(listener, Arc::new(state)))
}
