//! Usage:
//!   cargo run --example metrics_driver -- list
//!   cargo run --example metrics_driver -- write <session_id> <command>

use std::error::Error;

use ferryx_lib::daemon::client::DaemonClient;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("list");
    let client = DaemonClient::new();

    match mode {
        "list" => {
            let sessions = client
                .list_sessions()
                .await
                .map_err(|e| format!("list failed: {e:?}"))?;
            println!("sessions: {sessions:#?}");
        }
        "write" => {
            let session_id = args
                .get(2)
                .ok_or("usage: metrics_driver write <session_id> <command>")?;
            let command = args
                .get(3)
                .ok_or("usage: metrics_driver write <session_id> <command>")?;
            client
                .write_terminal(session_id, format!("{command}\n").into_bytes())
                .await
                .map_err(|e| format!("write failed: {e:?}"))?;
            println!("written to {session_id}: {command}");
        }
        other => return Err(format!("unknown mode: {other}").into()),
    }
    Ok(())
}
