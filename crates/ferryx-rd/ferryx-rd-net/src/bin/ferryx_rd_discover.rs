use ferryx_rd_net::discovery::LanDiscovery;
use std::{env, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut timeout_secs = 3u64;

    let args: Vec<String> = env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--timeout-secs" => {
                if i + 1 < args.len() {
                    timeout_secs = args[i + 1].parse()?;
                    i += 2;
                } else {
                    return Err("missing value for --timeout-secs".into());
                }
            }
            "--help" | "-h" => {
                println!("Usage: ferryx-rd-discover [--timeout-secs <seconds>]");
                return Ok(());
            }
            unknown => {
                return Err(format!(
                    "unknown argument: '{unknown}'. Usage: ferryx-rd-discover [--timeout-secs <seconds>]"
                )
                .into());
            }
        }
    }

    let browser = LanDiscovery::new()?;
    tokio::time::sleep(Duration::from_secs(timeout_secs)).await;
    let hosts = browser.snapshot()?;
    println!("{}", serde_json::to_string_pretty(&hosts)?);

    Ok(())
}
