//! SSH port-forwarded tunnel fallback provider.
//!
//! When neither Tailscale nor mDNS discovery succeeds, Ferryx can fall back
//! to a manually configured SSH port-forwarded tunnel: an ephemeral local
//! port is forwarded to the remote host's Ferryx gateway port over `ssh -L`.

use serde::Deserialize;

/// Subset of the `ferryx remote status --json` schema that we care about.
#[derive(Debug, Deserialize)]
struct RemoteStatusJson {
    port: u16,
}

/// Provides SSH tunnel bootstrap helpers: allocating a free local port,
/// building the `ssh -N -L` command, and parsing the remote host's status.
pub struct SshTunnelProvider;

impl SshTunnelProvider {
    /// Binds an ephemeral TCP port on localhost, then releases it so it can
    /// be handed to the `ssh -L` forward.
    pub fn allocate_ephemeral_local_port() -> Result<u16, String> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|error| format!("failed to bind ephemeral port: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("failed to read local address: {error}"))?
            .port();
        drop(listener);
        Ok(port)
    }

    /// Builds the `ssh -N -L <local_port>:127.0.0.1:<remote_port> <ssh_host>`
    /// command args used to establish the tunnel.
    pub fn build_ssh_tunnel_command(ssh_host: &str, local_port: u16, remote_port: u16) -> Vec<String> {
        vec![
            "ssh".to_string(),
            "-N".to_string(),
            "-L".to_string(),
            format!("{local_port}:127.0.0.1:{remote_port}"),
            ssh_host.to_string(),
        ]
    }

    /// Parses the remote host's `ferryx remote status --json` output and
    /// extracts the remote gateway port.
    pub fn parse_remote_status_json(output: &str) -> Result<u16, String> {
        let status: RemoteStatusJson = serde_json::from_str(output)
            .map_err(|error| format!("failed to parse remote status json: {error}"))?;
        Ok(status.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_tunnel_bootstrap() {
        let port = SshTunnelProvider::allocate_ephemeral_local_port()
            .expect("should allocate an ephemeral port");
        assert_ne!(port, 0);

        let parsed = SshTunnelProvider::parse_remote_status_json(
            r#"{"status":"ok","port":43821,"mode":"localNetwork"}"#,
        )
        .expect("should parse remote status json");
        assert_eq!(parsed, 43821);

        let command = SshTunnelProvider::build_ssh_tunnel_command("workbox", 54321, 43821);
        assert_eq!(
            command,
            vec!["ssh", "-N", "-L", "54321:127.0.0.1:43821", "workbox"]
        );
    }
}
