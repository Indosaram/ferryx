use super::*;
use crate::paired_host::inventory::PublishedOriginKind;
use crate::ssh::{SshAuthMethod, SshHostSource};

fn sample_host(id: &str, label: &str, hostname: &str, username: Option<&str>) -> SshHost {
    SshHost {
        id: id.to_owned(),
        label: label.to_owned(),
        hostname: hostname.to_owned(),
        username: username.map(str::to_owned),
        port: None,
        identity_file: None,
        jump_host: None,
        source: SshHostSource::Manual,
        auth_method: SshAuthMethod::Agent,
        disabled: None,
        shell: None,
    }
}

#[test]
fn matches_explicit_then_verified_then_published_ip_then_label_siblings() {
    let ts_pro = sample_host(
        "tailscale:nYr",
        "indoui-macbookpro-3.taila0cef2.ts.net",
        "100.110.225.110",
        None,
    );
    let manual_pro = sample_host("ssh-macbook-pro", "macbook-pro", "100.110.225.110", Some("indo"));
    let other = sample_host("ssh-omarchy", "omarchy", "100.91.254.71", Some("indo"));
    let hosts = vec![ts_pro, manual_pro, other];

    let matched = match_ssh_hosts(&hosts, None, None, "Indoui-MacBookPro-3.local", &[]);
    assert_eq!(matched.len(), 2);
    assert_eq!(matched[0].id, "ssh-macbook-pro");
    assert_eq!(matched[1].id, "tailscale:nYr");

    let explicit = match_ssh_hosts(&hosts, None, Some("ssh-omarchy"), "Indoui-MacBookPro-3", &[]);
    assert_eq!(explicit[0].id, "ssh-omarchy");

    let origins = vec![PublishedOrigin {
        kind: PublishedOriginKind::Tailscale,
        origin: "http://100.91.254.71:43821".into(),
        observed_at: 1,
    }];
    let by_origin = match_ssh_hosts(&hosts, None, None, "Unrelated Name", &origins);
    assert_eq!(by_origin.len(), 1);
    assert_eq!(by_origin[0].id, "ssh-omarchy");
}

#[test]
fn forward_args_include_stdio_target_and_hardening_options() {
    let mut host = sample_host("ssh-pro", "pro", "100.110.225.110", Some("indo"));
    host.port = Some(2222);
    host.identity_file = Some("/keys/id_ed25519".into());
    host.jump_host = Some("bastion".into());
    let args = forward_args(&host, 43821);
    assert_eq!(&args[..2], &["-W", "127.0.0.1:43821"]);
    assert!(args.windows(2).any(|w| w == ["-p", "2222"]));
    assert!(args.windows(2).any(|w| w == ["-i", "/keys/id_ed25519"]));
    assert!(args.windows(2).any(|w| w == ["-J", "bastion"]));
    assert_eq!(args.last().map(String::as_str), Some("indo@100.110.225.110"));
}

#[test]
fn forward_args_reuse_the_master_socket_instead_of_redialing() {
    let host = sample_host("ssh-pro", "pro", "100.110.225.110", Some("indo"));
    let socket = std::path::Path::new("/tmp/ferryx-ssh/0123456789abcdef.sock");
    let args = forward_args_reusing(&host, 43821, Some(socket));
    // Reuse options must lead: OpenSSH keeps the first -o it sees for a keyword, and the
    // hardening block below carries ControlPath-free defaults.
    assert_eq!(&args[..2], &["-o", "ControlMaster=no"]);
    assert_eq!(&args[2..4], &["-o", "ControlPath=/tmp/ferryx-ssh/0123456789abcdef.sock"]);
    assert!(args.iter().any(|arg| arg == "-W"));
    assert_eq!(args.last().map(String::as_str), Some("indo@100.110.225.110"));
    // The cold path must stay byte-identical to what the shipped code sends.
    assert_eq!(forward_args_reusing(&host, 43821, None), forward_args(&host, 43821));
    assert!(!forward_args(&host, 43821).iter().any(|arg| arg.starts_with("ControlPath=")));
}

#[test]
fn normalize_machine_token_strips_local_and_tailscale_suffixes() {
    assert_eq!(
        normalize_machine_token("Indoui-MacBookPro-3.local"),
        "indoui-macbookpro-3"
    );
    assert_eq!(
        normalize_machine_token("indoui-macbookpro-3.taila0cef2.ts.net"),
        "indoui-macbookpro-3"
    );
    assert_eq!(normalize_machine_token("omarchy"), "omarchy");
}

#[tokio::test]
async fn test_forward_bridges_attach_and_beats_slow_relay_in_race() {
    use crate::paired_host::path_select::{select_and_reuse_channel, AttachPath, CandidatePath};

    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_addr = target.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((mut s, _)) = target.accept().await {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut b = [0u8; 4];
            if s.read_exact(&mut b).await.is_ok() {
                let _ = s.write_all(&b).await;
            }
        }
    });

    let (origin, abort) =
        spawn_test_forward(move || async move { tokio::net::TcpStream::connect(target_addr).await })
            .await;

    let mut ssh_cand = CandidatePath::new(AttachPath::SshForward, origin.clone(), None);
    ssh_cand.probe_budget = Some(Duration::from_secs(2));
    let mut relay_cand = CandidatePath::new(AttachPath::Relay, "https://relay.invalid", None);
    relay_cand.probe_budget = Some(Duration::from_secs(5));

    let winner = select_and_reuse_channel(
        &[ssh_cand, relay_cand],
        Duration::from_secs(2),
        |cand| async move {
            let start = std::time::Instant::now();
            if cand.path == AttachPath::Relay {
                tokio::time::sleep(Duration::from_millis(500)).await;
                return Ok(("relay", start.elapsed()));
            }
            let addr = cand.base_origin.strip_prefix("http://").unwrap();
            let mut s = tokio::net::TcpStream::connect(addr).await.map_err(|_| ())?;
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            s.write_all(b"ping").await.map_err(|_| ())?;
            let mut buf = [0u8; 4];
            s.read_exact(&mut buf).await.map_err(|_| ())?;
            if &buf == b"ping" {
                Ok(("ssh", start.elapsed()))
            } else {
                Err(())
            }
        },
    )
    .await
    .expect("fastest path wins");
    assert_eq!(winner.path, AttachPath::SshForward);
    assert_eq!(winner.channel, "ssh");
    abort.abort();
}
