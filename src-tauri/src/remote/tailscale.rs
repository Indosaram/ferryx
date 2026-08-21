use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TailscaleStatus {
    pub installed: bool,
    pub running: bool,
    pub tailnet_name: Option<String>,
    pub self_dns: Option<String>,
    pub serve_active: bool,
}

pub trait CommandRunner: Send + Sync {
    fn run(&self, program: &str, args: &[&str]) -> Result<String, String>;
}

pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|e| e.to_string())?;

        if output.status.success() {
            String::from_utf8(output.stdout).map_err(|e| e.to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }
}

pub fn check_tailscale_status(runner: &dyn CommandRunner) -> TailscaleStatus {
    let status_res = runner.run("tailscale", &["status", "--json"]);
    let Ok(json_str) = status_res else {
        return TailscaleStatus {
            installed: false,
            running: false,
            tailnet_name: None,
            self_dns: None,
            serve_active: false,
        };
    };

    let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) else {
        return TailscaleStatus {
            installed: true,
            running: false,
            tailnet_name: None,
            self_dns: None,
            serve_active: false,
        };
    };

    let running = val.get("BackendState").and_then(|v| v.as_str()) == Some("Running");
    let self_dns = val
        .get("Self")
        .and_then(|s| s.get("DNSName"))
        .and_then(|d| d.as_str())
        .map(|s| s.trim_end_matches('.').to_string());
    let tailnet_name = val
        .get("CurrentTailnet")
        .and_then(|t| t.get("Name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());

    let serve_res = runner.run("tailscale", &["serve", "status", "--json"]);
    let serve_active = serve_res.is_ok() && !serve_res.unwrap().trim().is_empty();

    TailscaleStatus {
        installed: true,
        running,
        tailnet_name,
        self_dns,
        serve_active,
    }
}
