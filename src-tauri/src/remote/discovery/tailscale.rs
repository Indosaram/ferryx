//! Tailscale-based peer discovery.
//!
//! Discovers Ferryx hosts reachable over the Tailnet by inspecting the
//! output of `tailscale status --json`. Each online peer's `100.x.y.z`
//! Tailscale IP is combined with the default Ferryx port to produce a
//! [`HostEndpoint`].

use super::{HostAuthStatus, HostEndpoint, TransportType};
use serde::Deserialize;
use std::net::Ipv4Addr;
use std::collections::HashMap;

/// Default TCP port Ferryx listens on.
pub const DEFAULT_FERRYX_PORT: u16 = 43821;

/// Subset of the `tailscale status --json` schema that we care about.
#[derive(Debug, Deserialize)]
struct TailscaleStatus {
    #[serde(rename = "Self", default)]
    #[allow(dead_code)]
    myself: Option<TailscalePeer>,
    #[serde(rename = "Peer", default)]
    peer: HashMap<String, TailscalePeer>,
    #[serde(rename = "BackendState", default)]
    backend_state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TailscalePeer {
    #[serde(rename = "ID", default)]
    id: Option<String>,
    #[serde(rename = "HostName", default)]
    host_name: Option<String>,
    #[serde(rename = "DNSName", default)]
    dns_name: Option<String>,
    #[serde(rename = "TailscaleIPs", default)]
    tailscale_ips: Vec<String>,
    #[serde(rename = "Online", default)]
    online: bool,
    #[serde(rename = "OS", default)]
    os: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailscaleSshPeer {
    pub node_id: String,
    pub hostname: String,
    pub dns_name: Option<String>,
    pub ipv4: String,
    pub online: bool,
    pub os: Option<String>,
}

#[derive(Debug)]
pub enum TailscaleDiscoveryError {
    Timeout,
    Command(String),
    Parse(String),
}

impl std::fmt::Display for TailscaleDiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { Self::Timeout => write!(f, "tailscale status command timed out"), Self::Command(message) | Self::Parse(message) => f.write_str(message) }
    }
}

/// Discovers Ferryx-capable peers on the Tailnet via the `tailscale` CLI.
#[derive(Debug, Default, Clone, Copy)]
pub struct TailscaleDiscoveryProvider;

impl TailscaleDiscoveryProvider {
    pub fn new() -> Self {
        Self
    }

    /// Runs `tailscale status --json` and parses the resulting peers into
    /// [`HostEndpoint`] records.
    pub fn discover(&self) -> Result<Vec<HostEndpoint>, String> {
        Ok(self.discover_ssh_peers()?.into_iter().map(|peer| {
            let name = peer.dns_name.clone().unwrap_or_else(|| peer.hostname.clone());
            HostEndpoint { host_id: peer.node_id, name, address: format!("{}:{DEFAULT_FERRYX_PORT}", peer.ipv4), transport: TransportType::Tailscale, latency_ms: None, auth_status: HostAuthStatus::Unknown, online: peer.online }
        }).collect())
    }

    /// Parses `tailscale status --json` output into [`HostEndpoint`]
    /// records, one per peer that has a Tailscale IPv4 address.
    pub fn parse_status_json(json_str: &str) -> Result<Vec<HostEndpoint>, String> {
        Ok(Self::parse_ssh_peers(json_str)?.into_iter().map(|peer| {
            let name = peer.dns_name.clone().unwrap_or_else(|| peer.hostname.clone());
            HostEndpoint { host_id: peer.node_id, name, address: format!("{}:{DEFAULT_FERRYX_PORT}", peer.ipv4), transport: TransportType::Tailscale, latency_ms: None, auth_status: HostAuthStatus::Unknown, online: peer.online }
        }).collect())
    }

    pub fn parse_ssh_peers(json_str: &str) -> Result<Vec<TailscaleSshPeer>, String> {
        let status: TailscaleStatus = serde_json::from_str(json_str).map_err(|e| format!("invalid tailscale status json: {e}"))?;
        if status.backend_state.as_deref().is_some_and(|state| state != "Running") {
            return Err(format!("tailscale backend is not running: {}", status.backend_state.as_deref().unwrap_or("unknown")));
        }
        Ok(status.peer.into_iter().filter_map(|(key, peer)| {
            let ipv4 = peer.tailscale_ips.iter().find_map(|value| {
                let ip = value.parse::<Ipv4Addr>().ok()?;
                let [a,b,_,_] = ip.octets();
                (a == 100 && (64..=127).contains(&b)).then(|| ip.to_string())
            })?;
            let node_id = peer.id.filter(|id| !id.is_empty()).unwrap_or(key);
            let hostname = peer.host_name.filter(|name| !name.is_empty()).unwrap_or_else(|| node_id.clone());
            let dns_name = peer.dns_name.map(|name| name.trim_end_matches('.').to_owned()).filter(|name| !name.is_empty());
            let online = peer.online;
            Some(TailscaleSshPeer { node_id, hostname, dns_name, ipv4, online, os: peer.os })
        }).collect())
    }

    pub fn discover_ssh_peers(&self) -> Result<Vec<TailscaleSshPeer>, String> {
        let output = std::process::Command::new(tailscale_executable())
            .args(["status", "--json"])
            .output()
            .map_err(|e| format!("failed to run tailscale status: {e}"))?;
        if !output.status.success() {
            return Err(format!("tailscale status exited with error: {}", String::from_utf8_lossy(&output.stderr)));
        }
        Self::parse_ssh_peers(&String::from_utf8_lossy(&output.stdout))
    }

    pub async fn discover_ssh_peers_async(&self) -> Result<Vec<TailscaleSshPeer>, TailscaleDiscoveryError> {
        let executable = crate::ipc::run_blocking(|| Ok(tailscale_executable())).await
            .map_err(|error| TailscaleDiscoveryError::Command(error.to_string()))?;
        let result = tokio::time::timeout(std::time::Duration::from_secs(6), async move {
            let output = tokio::process::Command::new(executable)
                .args(["status", "--json"])
                .kill_on_drop(true)
                .output().await.map_err(|e| TailscaleDiscoveryError::Command(format!("failed to run tailscale status: {e}")))?;
            if !output.status.success() { return Err(TailscaleDiscoveryError::Command(format!("tailscale status exited with error: {}", String::from_utf8_lossy(&output.stderr)))); }
            Self::parse_ssh_peers(&String::from_utf8_lossy(&output.stdout)).map_err(TailscaleDiscoveryError::Parse)
        }).await;
        result.map_err(|_| TailscaleDiscoveryError::Timeout)?
    }
}

fn tailscale_executable() -> std::path::PathBuf {
    #[cfg(target_os = "macos")]
    {
        for candidate in ["/Applications/Tailscale.app/Contents/MacOS/Tailscale", "/usr/local/bin/tailscale", "/opt/homebrew/bin/tailscale"] {
            let candidate = std::path::PathBuf::from(candidate);
            if candidate.is_file() { return candidate; }
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let bundled = dir.join("../Resources/tailscale");
                if bundled.is_file() { return bundled; }
            }
        }
    }
    std::path::PathBuf::from("tailscale")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_STATUS_JSON: &str = r#"
    {
        "Self": {
            "ID": "self-node",
            "HostName": "my-laptop",
            "DNSName": "my-laptop.tailnet-1234.ts.net.",
            "TailscaleIPs": ["100.64.0.1"],
            "Online": true
        },
        "Peer": {
            "peer1key": {
                "ID": "peer-1",
                "HostName": "mac-mini",
                "DNSName": "mac-mini.tailnet-1234.ts.net.",
                "TailscaleIPs": ["100.64.0.2", "fd7a:115c:a1e0::1"],
                "Online": true
            },
            "peer2key": {
                "ID": "peer-2",
                "HostName": "workstation",
                "DNSName": "workstation.tailnet-1234.ts.net.",
                "TailscaleIPs": ["100.64.0.3"],
                "Online": false
            }
        }
    }
    "#;

    #[test]
    fn test_tailscale_peer_discovery() {
        let endpoints = TailscaleDiscoveryProvider::parse_status_json(SAMPLE_STATUS_JSON)
            .expect("should parse sample tailscale status json");

        assert_eq!(endpoints.len(), 2);

        for endpoint in &endpoints {
            assert_eq!(endpoint.transport, TransportType::Tailscale);
        }

        let mac_mini = endpoints
            .iter()
            .find(|e| e.host_id == "peer-1")
            .expect("mac-mini peer present");
        assert_eq!(mac_mini.name, "mac-mini.tailnet-1234.ts.net");
        assert_eq!(mac_mini.address, "100.64.0.2:43821");
        assert!(mac_mini.online);

        let workstation = endpoints
            .iter()
            .find(|e| e.host_id == "peer-2")
            .expect("workstation peer present");
        assert_eq!(workstation.name, "workstation.tailnet-1234.ts.net");
        assert_eq!(workstation.address, "100.64.0.3:43821");
        assert!(!workstation.online);
    }

    #[test]
    fn test_parse_status_json_invalid_input_errors() {
        let result = TailscaleDiscoveryProvider::parse_status_json("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_status_json_skips_peers_without_ipv4() {
        let json = r#"
        {
            "Peer": {
                "peer1key": {
                    "ID": "peer-1",
                    "HostName": "ipv6-only",
                    "TailscaleIPs": ["fd7a:115c:a1e0::1"],
                    "Online": true
                }
            }
        }
        "#;
        let endpoints =
            TailscaleDiscoveryProvider::parse_status_json(json).expect("should parse json");
        assert!(endpoints.is_empty());
    }

    #[test]
    fn ssh_peer_parser_rejects_invalid_cgnat() {
        let json = r#"{"Peer":{"n1":{"ID":"node-1","HostName":"box","TailscaleIPs":["100.200.0.1","100.64.0.8"],"Online":true,"BackendState":"Stopped","OS":"linux"},"n2":{"ID":"node-2","HostName":"bad","TailscaleIPs":["100.200.0.1"],"Online":true}}}"#;
        let peers = TailscaleDiscoveryProvider::parse_ssh_peers(json).expect("fixture parse");
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].ipv4, "100.64.0.8");
        assert!(peers[0].online);
        assert_eq!(peers[0].os.as_deref(), Some("linux"));
    }

    #[test]
    fn stopped_local_backend_is_a_discovery_error() {
        let json = r#"{"BackendState":"Stopped","Peer":{}}"#;
        assert!(TailscaleDiscoveryProvider::parse_ssh_peers(json).unwrap_err().contains("not running"));
    }
}
