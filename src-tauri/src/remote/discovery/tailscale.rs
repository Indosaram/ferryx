//! Tailscale-based peer discovery.
//!
//! Discovers Ferryx hosts reachable over the Tailnet by inspecting the
//! output of `tailscale status --json`. Each online peer's `100.x.y.z`
//! Tailscale IP is combined with the default Ferryx port to produce a
//! [`HostEndpoint`].

use super::{HostAuthStatus, HostEndpoint, TransportType};
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;

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
        let output = Command::new("tailscale")
            .args(["status", "--json"])
            .output()
            .map_err(|e| format!("failed to run tailscale status: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("tailscale status exited with error: {stderr}"));
        }

        let json_str = String::from_utf8_lossy(&output.stdout);
        Self::parse_status_json(&json_str)
    }

    /// Parses `tailscale status --json` output into [`HostEndpoint`]
    /// records, one per peer that has a Tailscale IPv4 address.
    pub fn parse_status_json(json_str: &str) -> Result<Vec<HostEndpoint>, String> {
        let status: TailscaleStatus =
            serde_json::from_str(json_str).map_err(|e| format!("invalid tailscale status json: {e}"))?;

        let mut endpoints = Vec::new();
        for (key, peer) in status.peer.into_iter() {
            let Some(ipv4) = peer
                .tailscale_ips
                .iter()
                .find(|ip| ip.starts_with("100."))
                .cloned()
            else {
                continue;
            };

            let host_id = peer.id.clone().unwrap_or(key);
            let name = peer
                .dns_name
                .as_deref()
                .map(|d| d.trim_end_matches('.').to_string())
                .filter(|d| !d.is_empty())
                .or_else(|| peer.host_name.clone())
                .unwrap_or_else(|| host_id.clone());

            endpoints.push(HostEndpoint {
                host_id,
                name,
                address: format!("{ipv4}:{DEFAULT_FERRYX_PORT}"),
                transport: TransportType::Tailscale,
                latency_ms: None,
                auth_status: HostAuthStatus::Unknown,
                online: peer.online,
            });
        }

        Ok(endpoints)
    }
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
}
