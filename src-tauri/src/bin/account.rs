use std::sync::Arc;

use ferryx_lib::account::mailer::FileMailer;
use ferryx_lib::account::origin::{account_data_dir, account_origin};
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

    let state = AccountState::new(data_dir, origin, Arc::new(FileMailer::new()))
        .with_limits(per_hour, max_body);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("runtime: {error}"))?;
    let listener = runtime.block_on(async {
        tokio::net::TcpListener::bind(&bind)
            .await
            .map_err(|error| format!("bind {bind}: {error}"))
    })?;
    println!("FERRYX_ACCOUNT_READY {}", listener.local_addr().map(|addr| addr.to_string()).unwrap_or(bind));
    runtime.block_on(serve(listener, Arc::new(state)))
}
