use super::*;
use crate::terminal::TerminalService;
use std::sync::Arc;

struct MockTailscaleRunner {
    installed: bool,
    running: bool,
    self_dns: Option<String>,
}

impl CommandRunner for MockTailscaleRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
        if program != "tailscale" {
            return Err("not tailscale".into());
        }
        if !self.installed {
            return Err("command not found".into());
        }
        if args.contains(&"status") {
            let json = serde_json::json!({
                "BackendState": if self.running { "Running" } else { "Stopped" },
                "CurrentTailnet": { "Name": "test-tailnet" },
                "Self": { "DNSName": self.self_dns.as_deref().unwrap_or("my-mac.test.ts.net.") }
            });
            Ok(json.to_string())
        } else if args.contains(&"serve") {
            Ok(r#"{"TCP":{"43821":{"HTTPS":true}}}"#.into())
        } else {
            Ok("".into())
        }
    }
}

#[test]
fn test_tailscale_status_parsing() {
    let runner = MockTailscaleRunner {
        installed: true,
        running: true,
        self_dns: Some("my-mac.tailnet.ts.net.".into()),
    };
    let status = check_tailscale_status(&runner);
    assert!(status.installed);
    assert!(status.running);
    assert_eq!(status.self_dns, Some("my-mac.tailnet.ts.net".into()));
    assert_eq!(status.tailnet_name, Some("test-tailnet".into()));
}

#[test]
fn test_auth_manager_pairing_and_revocation() {
    let auth = AuthManager::new();
    let code = auth.create_pairing_code(DevicePermission::Control);

    // Invalid code fails
    assert!(auth.exchange_pairing_code("invalid_code", "test-device").is_err());

    // Valid code succeeds
    let (token, device) = auth.exchange_pairing_code(&code, "test-device").expect("exchange success");
    assert_eq!(device.permission, DevicePermission::Control);
    assert!(!device.revoked);

    // Code is single use
    assert!(auth.exchange_pairing_code(&code, "test-device-2").is_err());

    // Validate token
    let validated = auth.validate_token(&token).expect("validate success");
    assert_eq!(validated.id, device.id);

    // Revoke device
    assert!(auth.revoke_device(&device.id));
    assert!(matches!(auth.validate_token(&token), Err(AuthError::RevokedDevice)));
}

#[tokio::test]
async fn test_remote_server_health_and_lifecycle() {
    let terminal_service = Arc::new(TerminalService::default());
    let state = Arc::new(RemoteGatewayState::new(terminal_service));

    *state.config.write() = RemoteGatewayConfig {
        mode: RemoteNetworkMode::Tailscale,
        port: 0, // OS assigns available port
        allow_control: true,
    };

    let (handle, addr) = start_remote_server(Arc::clone(&state)).await.expect("start server");
    assert!(addr.ip().is_unspecified() || addr.ip().is_loopback());

    // Health endpoint
    let client = reqwest_like_health(&format!("http://{addr}/api/v1/health")).await;
    assert!(client);

    handle.stop();
}

async fn reqwest_like_health(url: &str) -> bool {
    // simple TCP request
    let parsed: std::net::SocketAddr = url.trim_start_matches("http://").trim_end_matches("/api/v1/health").parse().unwrap();
    if let Ok(mut stream) = tokio::net::TcpStream::connect(parsed).await {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let req = b"GET /api/v1/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(req).await;
        let mut buf = [0u8; 1024];
        if let Ok(n) = stream.read(&mut buf).await {
            let res = String::from_utf8_lossy(&buf[..n]);
            return res.contains("200 OK") && res.contains(r#""status":"ok""#);
        }
    }
    false
}
