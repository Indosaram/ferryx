//! Todo 26 spike probe (feature `iroh-direct`): measures attach keystroke echo round trips over
//! the current relay hop and over iroh with only the self-hosted relay, on the same two hosts.
//! Reached only through the hidden `ferryx-cli iroh-probe` entry of an `iroh-direct` build and
//! driven by `scripts/qa/iroh-rtt.mjs`; every line on stdout is one JSON object.
//!
//! ```text
//! ferryx-cli iroh-probe keygen
//! ferryx-cli iroh-probe iroh-serve  --relay URL --machine-private B64 --device-key B64 [--bind ADDR]
//! ferryx-cli iroh-probe iroh-dial   --relay URL --peer ID [--direct ADDR]... --machine-key B64 --device-private B64 [--count N]
//! ferryx-cli iroh-probe relay-serve --relay-ws WS --token T --machine-private B64 --device-key B64
//! ferryx-cli iroh-probe relay-dial  --relay-ws WS --session ID --machine-key B64 --device-private B64 [--count N]
//! ```
//!
//! The serve roles echo every frame until the peer leaves; the dial roles print
//! `{"transport", "count", "p50Ms", "p95Ms", "direct", "maxRssKb"}`.
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use crate::remote::attach_crypto::{AttachInitiator, WebSocketByteStream};
use crate::remote::attach_identity::AttachIdentity;
use crate::remote::iroh_direct;
use crate::remote::session_transport::{establish_session, SessionTransport};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::tungstenite::Message;

const MACHINE_ID: &str = "iroh-spike";
const EPOCH: &str = "1";

struct Args(Vec<String>);

impl Args {
    fn one(&self, name: &str) -> Result<String> {
        self.optional(name).ok_or_else(|| anyhow!("missing {name}"))
    }

    fn optional(&self, name: &str) -> Option<String> {
        self.0
            .iter()
            .position(|arg| arg == name)
            .and_then(|index| self.0.get(index + 1).cloned())
    }

    fn all(&self, name: &str) -> Vec<String> {
        self.0
            .windows(2)
            .filter(|pair| pair[0] == name)
            .map(|pair| pair[1].clone())
            .collect()
    }

    fn count(&self) -> Result<usize> {
        match self.optional("--count") {
            None => Ok(1000),
            Some(value) => match value.parse::<usize>() {
                Ok(count) if count > 0 => Ok(count),
                _ => bail!("--count must be a positive integer, got {value:?}"),
            },
        }
    }
}

fn key32(value: &str, name: &str) -> Result<[u8; 32]> {
    STANDARD
        .decode(value)
        .ok()
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .ok_or_else(|| anyhow!("{name} must be 32 bytes of base64"))
}

fn machine_identity(args: &Args) -> Result<AttachIdentity> {
    let private = key32(&args.one("--machine-private")?, "--machine-private")?;
    let public = x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(private));
    Ok(AttachIdentity {
        public_key: STANDARD.encode(public.as_bytes()),
        private_key: STANDARD.encode(private),
    })
}

#[cfg(unix)]
fn max_rss_kb() -> Option<i64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: getrusage writes a full rusage into the provided, correctly sized buffer.
    let usage = unsafe {
        if libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) != 0 {
            return None;
        }
        usage.assume_init()
    };
    // Linux reports kilobytes, macOS bytes.
    let rss = usage.ru_maxrss;
    Some(if cfg!(target_os = "macos") { rss / 1024 } else { rss })
}

#[cfg(not(unix))]
fn max_rss_kb() -> Option<i64> {
    None
}

async fn serve<S>(stream: S, args: &Args, session_id: &str) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let machine = machine_identity(args)?;
    let device = key32(&args.one("--device-key")?, "--device-key")?;
    let transport = establish_session(stream, true, Some(&machine), MACHINE_ID, session_id, EPOCH, {
        move |key: &[u8; 32]| *key == device
    })
    .await
    .map_err(|error| anyhow!("attach handshake: {error}"))?;
    let SessionTransport::Attached(secure) = transport else {
        bail!("session was not attached");
    };
    let echoed = iroh_direct::serve_echo(secure)
        .await
        .map_err(|error| anyhow!("echo: {error}"))?;
    println!("{}", json!({"event": "served", "echoed": echoed, "maxRssKb": max_rss_kb()}));
    Ok(())
}

async fn dial<S>(stream: S, args: &Args, session_id: &str, transport: &str, direct: Option<bool>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let device = key32(&args.one("--device-private")?, "--device-private")?;
    let initiator = AttachInitiator::new(&device, &args.one("--machine-key")?, MACHINE_ID, session_id, EPOCH)
        .map_err(|error| anyhow!("initiator: {error}"))?;
    let (mut secure, _) = initiator
        .connect(stream)
        .await
        .map_err(|error| anyhow!("attach handshake: {error}"))?;
    let count = args.count()?;
    let samples = iroh_direct::measure_echo_rtt(&mut secure, count)
        .await
        .map_err(|error| anyhow!("echo: {error}"))?;
    println!(
        "{}",
        json!({
            "transport": transport,
            "count": samples.len(),
            "p50Ms": iroh_direct::percentile_ms(&samples, 50.0),
            "p95Ms": iroh_direct::percentile_ms(&samples, 95.0),
            "direct": direct,
            "maxRssKb": max_rss_kb(),
        })
    );
    Ok(())
}

async fn iroh_serve(args: &Args) -> Result<()> {
    let relay = args.one("--relay")?;
    let endpoint = iroh_direct::bind_endpoint(&relay, args.optional("--bind").as_deref())
        .await
        .map_err(|error| anyhow!(error))?;
    iroh_direct::assert_self_hosted_only(&endpoint, &relay)
        .await
        .map_err(|error| anyhow!(error))?;
    println!(
        "{}",
        json!({
            "event": "listening",
            "peer": endpoint.id().to_string(),
            "direct": endpoint.bound_sockets().iter().map(ToString::to_string).collect::<Vec<_>>(),
        })
    );
    while let Some(opened) = iroh_direct::accept(&endpoint).await {
        let (connection, stream) = opened.map_err(|error| anyhow!(error))?;
        let result = serve(stream, args, "iroh").await;
        println!(
            "{}",
            json!({"event": "closed", "direct": iroh_direct::uses_direct_path(&connection), "error": result.err().map(|e| e.to_string())})
        );
    }
    Ok(())
}

async fn iroh_dial(args: &Args) -> Result<()> {
    let relay = args.one("--relay")?;
    let peer = args
        .one("--peer")?
        .parse::<iroh::EndpointId>()
        .map_err(|error| anyhow!("--peer: {error}"))?;
    let direct = args
        .all("--direct")
        .iter()
        .map(|addr| addr.parse().with_context(|| format!("--direct {addr}")))
        .collect::<Result<Vec<std::net::SocketAddr>>>()?;
    let endpoint = iroh_direct::bind_endpoint(&relay, None)
        .await
        .map_err(|error| anyhow!(error))?;
    iroh_direct::assert_self_hosted_only(&endpoint, &relay)
        .await
        .map_err(|error| anyhow!(error))?;
    let target = iroh_direct::peer_addr(peer, &relay, direct).map_err(|error| anyhow!(error))?;
    let (connection, stream) = iroh_direct::dial(&endpoint, target)
        .await
        .map_err(|error| anyhow!(error))?;
    // Path state after the run, so a hole-punch upgrade during warm-up is reflected.
    let outcome = dial(stream, args, "iroh", "iroh", None).await;
    println!(
        "{}",
        json!({"event": "path", "direct": iroh_direct::uses_direct_path(&connection)})
    );
    connection.close(0u32.into(), b"done");
    endpoint.close().await;
    outcome
}

async fn relay_serve(args: &Args) -> Result<()> {
    let base = args.one("--relay-ws")?;
    let token = args.one("--token")?;
    let (mut control, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/control?token={token}"))
        .await
        .context("relay control connect")?;
    control
        .send(Message::Text(json!({"type": "AllocateSession", "opaque": true}).to_string().into()))
        .await
        .context("allocate session")?;
    let session_id = loop {
        match control.next().await {
            Some(Ok(Message::Text(text))) => {
                let value: serde_json::Value = serde_json::from_str(&text).context("control frame")?;
                if let Some(id) = value.get("session_id").and_then(|id| id.as_str()) {
                    break id.to_string();
                }
                if value.get("type").and_then(|kind| kind.as_str()) == Some("AllocateSessionRejected") {
                    bail!("relay refused the allocation: {text}");
                }
            }
            Some(Ok(_)) => continue,
            Some(Err(error)) => bail!("relay control: {error}"),
            None => bail!("relay closed the control channel"),
        }
    };
    println!("{}", json!({"event": "listening", "session": session_id}));
    let (data, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/data/{session_id}"))
        .await
        .context("relay data connect")?;
    let result = serve(WebSocketByteStream::new(data), args, &session_id).await;
    drop(control);
    result
}

async fn relay_dial(args: &Args) -> Result<()> {
    let base = args.one("--relay-ws")?;
    let session_id = args.one("--session")?;
    let (client, _) = tokio_tungstenite::connect_async(format!("{base}/tunnel/opaque/{session_id}"))
        .await
        .context("relay opaque connect")?;
    dial(WebSocketByteStream::new(client), args, &session_id, "relay", Some(false)).await
}

/// Runs one probe role; `args` are the words after `iroh-probe`. Returns the exit code.
pub fn run_iroh_probe_cli(args: &[String]) -> i32 {
    let args = Args(args.to_vec());
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            println!("{}", json!({"event": "error", "error": format!("tokio runtime: {error}")}));
            return 1;
        }
    };
    let outcome = runtime.block_on(async {
        match args.0.first().map(String::as_str) {
            Some("keygen") => {
                let secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
                println!(
                    "{}",
                    json!({
                        "public": STANDARD.encode(x25519_dalek::PublicKey::from(&secret).as_bytes()),
                        "private": STANDARD.encode(secret.to_bytes()),
                    })
                );
                Ok(())
            }
            Some("iroh-serve") => iroh_serve(&args).await,
            Some("iroh-dial") => iroh_dial(&args).await,
            Some("relay-serve") => relay_serve(&args).await,
            Some("relay-dial") => relay_dial(&args).await,
            other => Err(anyhow!("unknown role {other:?}")),
        }
    });
    match outcome {
        Ok(()) => 0,
        Err(error) => {
            println!("{}", json!({"event": "error", "error": format!("{error:#}")}));
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Args {
        Args(line.split_whitespace().map(str::to_string).collect())
    }

    #[test]
    fn probe_arguments_reject_malformed_values() {
        assert_eq!(words("x").count().unwrap(), 1000);
        assert_eq!(words("--count 7").count().unwrap(), 7);
        for bad in ["--count 0", "--count -1", "--count abc", "--count 1.5"] {
            assert!(words(bad).count().is_err(), "{bad} must be refused");
        }
        assert!(words("--relay").one("--relay").is_err());
        assert_eq!(words("--direct a --x y --direct b").all("--direct"), vec!["a", "b"]);
        assert!(key32("not base64", "k").is_err());
        assert!(key32(&STANDARD.encode([0u8; 31]), "k").is_err());
        assert!(key32(&STANDARD.encode([7u8; 32]), "k").is_ok());
        assert_eq!(run_iroh_probe_cli(&["nope".to_string()]), 1);
    }
}
