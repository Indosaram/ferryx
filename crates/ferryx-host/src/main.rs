use std::sync::Arc;

use ferryx_host::transport::{default_endpoint, serve, uuid_hex};
use ferryx_host::{HostInfo, HostShared};
use fxsh::types::Supervisor;
use fxsh::Uuid;

fn arg(name: &str) -> Option<String> {
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        if a == name {
            return it.next();
        }
    }
    None
}

fn parse_instance(s: &str) -> Option<Uuid> {
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(Uuid(out))
}

fn supervisor() -> Supervisor {
    if cfg!(target_os = "macos") {
        Supervisor::Launchd
    } else if cfg!(windows) {
        Supervisor::TaskScheduler
    } else {
        Supervisor::Systemd
    }
}

#[tokio::main]
async fn main() {
    let instance = match arg("--instance") {
        Some(s) => match parse_instance(&s) {
            Some(i) => i,
            None => {
                eprintln!("ferryx-host: --instance must be a UUID");
                std::process::exit(2);
            }
        },
        None => Uuid(*uuid::Uuid::new_v4().as_bytes()),
    };
    let endpoint = match arg("--endpoint").map(Ok).unwrap_or_else(|| default_endpoint(&instance)) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("ferryx-host: cannot determine endpoint: {e}");
            std::process::exit(2);
        }
    };
    let info = Arc::new(HostInfo { supervisor: supervisor(), pid: std::process::id(), process_start_time: 0, version: env!("CARGO_PKG_VERSION").to_owned() });
    let host = HostShared::new(instance);
    let printed = endpoint.clone();
    let result = serve(host, info, &endpoint, move || {
        println!("FERRYX_HOST_READY instance={} endpoint={}", uuid_hex(&instance), printed);
    })
    .await;
    if let Err(e) = result {
        eprintln!("ferryx-host: {e}");
        std::process::exit(1);
    }
}
