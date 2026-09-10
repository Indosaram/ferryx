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

use ferryx_lib::remote::relay_server::{relay_router, spawn_session_reaper, RelayState};

struct RelayConfig {
    port: u16,
    machine_tokens: Vec<String>,
}

fn parse_args(args: &[String]) -> Result<RelayConfig, String> {
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
            other => {
                return Err(format!("unrecognized argument: {other}"));
            }
        }
        i += 1;
    }

    Ok(RelayConfig {
        port,
        machine_tokens,
    })
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

    let state = RelayState::new(config.machine_tokens);
    spawn_session_reaper(state.clone());
    let router = relay_router(state);

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
