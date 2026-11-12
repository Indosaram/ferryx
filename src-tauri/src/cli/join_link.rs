//! `ferryx join` — enroll a headless machine from one link, without a magic link.
//!
//! A join link carries everything a machine needs to attach to an account: the account origin
//! and a short-lived, single-use install key (`fxk_<64 hex>`). The owner creates it from a
//! machine that is already signed in (`ferryx join --create`), then pastes it on the target.
//!
//! ```text
//! ferryx join --create [--label <name>] [--ttl <secs>] [--origin <url>]
//! ferryx join 'ferryx://join/<origin-host>/<key>'
//! ```
//!
//! Joining never authenticates as the owner. It redeems the install key through the same
//! `up --key` path the one-line installer uses, so the target machine only ever proves
//! possession of the link. The link is therefore a bearer credential: single-use, TTL-bounded,
//! revocable, and never logged.

use std::time::Duration;

use crate::account::install_keys::{
    clamp_install_key_ttl, IssueInstallKeyResponse, INSTALL_KEY_PREFIX,
};

/// 15s like every other account CLI call; a join is interactive, so it must not hang.
fn join_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| format!("cannot build the account client: {error}"))
}

/// Scheme and host of a join link. The account origin follows in the first path segment so a
/// self-hosted account service works without extra syntax.
pub const JOIN_SCHEME: &str = "ferryx";
pub const JOIN_HOST: &str = "join";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinCliCommand {
    /// Owner side: mint an install key and render the link.
    Create {
        label: Option<String>,
        ttl_secs: Option<u64>,
        origin: Option<String>,
    },
    /// Target side: redeem a link by enrolling through the install-key path.
    Redeem { link: String, name: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinLink {
    /// Account origin without a trailing slash, e.g. `https://ferryx.dev`.
    pub origin: String,
    /// The install key. Secret: never print it outside the link the owner asked for.
    pub key: String,
}

impl JoinLink {
    pub fn parse(link: &str) -> Result<Self, String> {
        let trimmed = link.trim();
        let rest = trimmed
            .strip_prefix(JOIN_SCHEME)
            .and_then(|rest| rest.strip_prefix("://"))
            .ok_or_else(|| format!("not a ferryx join link: expected {JOIN_SCHEME}://{JOIN_HOST}/..."))?;
        let rest = rest
            .strip_prefix(JOIN_HOST)
            .and_then(|rest| rest.strip_prefix('/'))
            .ok_or_else(|| format!("not a ferryx join link: expected {JOIN_SCHEME}://{JOIN_HOST}/..."))?;
        let (origin_host, key) = rest
            .rsplit_once('/')
            .ok_or_else(|| "join link is missing the install key".to_string())?;
        let origin_host = origin_host.trim();
        if origin_host.is_empty() {
            return Err("join link is missing the account origin".to_string());
        }
        let key = key.trim();
        if !key.starts_with(INSTALL_KEY_PREFIX) {
            return Err(format!("join link key must start with {INSTALL_KEY_PREFIX}"));
        }
        let scheme = if origin_host.starts_with("localhost") || origin_host.starts_with("127.0.0.1") {
            "http"
        } else {
            "https"
        };
        Ok(Self {
            origin: format!("{scheme}://{origin_host}"),
            key: key.to_string(),
        })
    }

    /// Renders the link the owner hands to the target machine.
    pub fn render(&self) -> String {
        let host = self
            .origin
            .strip_prefix("https://")
            .or_else(|| self.origin.strip_prefix("http://"))
            .unwrap_or(&self.origin)
            .trim_end_matches('/');
        format!("{JOIN_SCHEME}://{JOIN_HOST}/{host}/{}", self.key)
    }
}

/// `POST /api/account/v1/install-keys` as the signed-in owner.
pub async fn issue_join_key(
    origin: &str,
    session_token: &str,
    label: Option<&str>,
    ttl_secs: Option<u64>,
) -> Result<IssueInstallKeyResponse, String> {
    let origin = origin.trim_end_matches('/');
    let body = serde_json::json!({
        "label": label,
        "ttlSecs": clamp_install_key_ttl(ttl_secs),
    });
    let client = join_http_client()?;
    let response = client
        .post(format!("{origin}/api/account/v1/install-keys"))
        .bearer_auth(session_token)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("cannot reach {origin}: {error}"))?;
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(format!("install key was rejected ({status}): {text}"));
    }
    serde_json::from_str(&text).map_err(|error| format!("account service sent an unreadable response: {error}"))
}

pub const JOIN_USAGE: &str = "expected `ferryx join <link>` or `ferryx join --create`\n  create: `ferryx join --create [--label <text>] [--ttl <secs>] [--origin <url>]`\n  redeem: `ferryx join 'ferryx://join/<host>/<key>' [--name <text>]`";

pub fn parse_join_cli<I, T>(args: I) -> Result<JoinCliCommand, String>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args: Vec<String> = args.into_iter().map(|arg| arg.as_ref().to_string()).collect();
    // Same shape as `direct-trust`: the first element is the program, the second the group.
    if args.get(1).is_none_or(|arg| arg != "join") {
        return Err(JOIN_USAGE.to_string());
    }
    let args = args[2.min(args.len())..].to_vec();
    let create = args.iter().any(|arg| arg == "--create");
    let mut label = None;
    let mut ttl_secs = None;
    let mut origin = None;
    let mut name = None;
    let mut positional: Vec<String> = Vec::new();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        let mut take = |flag: &str| -> Result<String, String> {
            iter.next().ok_or_else(|| format!("`{flag}` needs a value\n{JOIN_USAGE}"))
        };
        match arg.as_str() {
            "--create" => {}
            "--label" => label = Some(take("--label")?),
            "--ttl" => {
                let raw = take("--ttl")?;
                let parsed = raw
                    .parse::<u64>()
                    .map_err(|_| format!("`--ttl` needs whole seconds, got `{raw}`"))?;
                ttl_secs = Some(parsed);
            }
            "--origin" => origin = Some(take("--origin")?),
            "--name" => name = Some(take("--name")?),
            other if other.starts_with("--") => {
                return Err(format!("unknown option `{other}`\n{JOIN_USAGE}"));
            }
            other => positional.push(other.to_string()),
        }
    }
    match (create, positional.as_slice()) {
        (true, []) => {
            if name.is_some() {
                return Err(format!("`--name` applies when redeeming a link, not creating one\n{JOIN_USAGE}"));
            }
            Ok(JoinCliCommand::Create { label, ttl_secs, origin })
        }
        (false, [link]) => {
            if label.is_some() || ttl_secs.is_some() || origin.is_some() {
                return Err(format!("`--label`, `--ttl` and `--origin` apply to `--create` only\n{JOIN_USAGE}"));
            }
            Ok(JoinCliCommand::Redeem { link: link.clone(), name })
        }
        (true, _) => Err(format!("`--create` takes no link\n{JOIN_USAGE}")),
        (false, []) => Err(JOIN_USAGE.to_string()),
        (false, _) => Err(format!("`ferryx join` takes exactly one link\n{JOIN_USAGE}")),
    }
}


/// Runs `ferryx join`: mint a link as the owner, or redeem one on the target machine.
pub fn run_join_cli(command: JoinCliCommand, json: bool) -> Result<(), String> {
    match command {
        JoinCliCommand::Create { label, ttl_secs, origin } => {
            let origin = match origin {
                Some(value) => crate::account::origin::normalize_account_origin(&value)
                    .map_err(|error| error.to_string())?,
                None => crate::account::origin::account_origin().map_err(|error| error.to_string())?,
            };
            let stored = crate::account::enroll_client::load_account_session();
            let session_token = crate::cli::billing::resolve_session_token_or_stored(
                &origin,
                |key| std::env::var(key).ok(),
                stored.as_ref(),
            )
            .map_err(|error| error.message)?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| format!("cannot start the runtime: {error}"))?;
            let issued =
                runtime.block_on(issue_join_key(&origin, &session_token, label.as_deref(), ttl_secs))?;
            let link = JoinLink { origin: origin.clone(), key: issued.key.clone() };
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "link": link.render(),
                        "keyId": issued.key_id,
                        "expiresAt": issued.expires_at,
                        "installCommand": issued.install_command,
                    })
                );
            } else {
                println!("{}", link.render());
                eprintln!(
                    "Single use, expires at {} (epoch seconds).\nPaste the link on the target machine and run:\n  ferryx join '<link>'",
                    issued.expires_at
                );
            }
            Ok(())
        }
        JoinCliCommand::Redeem { link, name } => {
            let parsed = JoinLink::parse(&link)?;
            let origin = parsed.origin.clone();
            let key = parsed.key.clone();
            let summary = crate::cli::run_up_cli_with_key(&key, Some(origin), name)
                .map_err(|error| error.message)?;
            if !json {
                match summary.enrolled.as_ref() {
                    Some(enrolled) => eprintln!(
                        "Joined as machine record {}.\nThe background service installed={} (backend {}, supervised {}).\nremote access is {}.",
                        enrolled.machine_record_id,
                        summary.service.installed,
                        summary.service.backend,
                        summary.service.supervised,
                        enrolled.relay_activation
                    ),
                    None => eprintln!("Joined, but the account summary carried no machine record."),
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_round_trips_through_render_and_parse() {
        let link = JoinLink { origin: "https://relay.ferryx.dev".into(), key: format!("{INSTALL_KEY_PREFIX}abc123") };
        let rendered = link.render();
        assert_eq!(rendered, "ferryx://join/relay.ferryx.dev/fxk_abc123");
        assert_eq!(JoinLink::parse(&rendered).expect("parses"), link);
    }

    #[test]
    fn localhost_origin_stays_plain_http() {
        let parsed = JoinLink::parse("ferryx://join/localhost:8787/fxk_deadbeef").expect("parses");
        assert_eq!(parsed.origin, "http://localhost:8787");
        assert_eq!(parsed.key, "fxk_deadbeef");
    }

    #[test]
    fn a_key_must_carry_the_install_prefix() {
        let error = JoinLink::parse("ferryx://join/example.com/not-a-key").expect_err("rejects");
        assert!(error.contains(INSTALL_KEY_PREFIX), "{error}");
    }

    #[test]
    fn a_foreign_scheme_is_not_a_join_link() {
        assert!(JoinLink::parse("https://example.com/fxk_abc").is_err());
        assert!(JoinLink::parse("ferryx://pair/example.com/fxk_abc").is_err());
    }

    #[test]
    fn usage_errors_never_echo_the_key() {
        let error = parse_join_cli(["ferryx", "join", "fxk_secret", "extra"]).expect_err("rejects");
        assert!(!error.contains("fxk_secret"), "{error}");
    }

    #[test]
    fn create_rejects_a_link_and_redeem_rejects_create_flags() {
        assert!(parse_join_cli(["ferryx", "join", "--create", "ferryx://join/h/fxk_a"]).is_err());
        assert!(parse_join_cli(["ferryx", "join", "ferryx://join/h/fxk_a", "--ttl", "60"]).is_err());
        assert!(parse_join_cli(["ferryx", "join"]).is_err());
    }

    #[test]
    fn create_parses_its_options() {
        match parse_join_cli(["ferryx", "join", "--create", "--label", "ci", "--ttl", "900"])
            .expect("parses")
        {
            JoinCliCommand::Create { label, ttl_secs, origin } => {
                assert_eq!(label.as_deref(), Some("ci"));
                assert_eq!(ttl_secs, Some(900));
                assert!(origin.is_none());
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn redeem_parses_a_link_and_optional_name() {
        match parse_join_cli(["ferryx", "join", "ferryx://join/h.example/fxk_a", "--name", "ci-1"])
            .expect("parses")
        {
            JoinCliCommand::Redeem { link, name } => {
                assert_eq!(link, "ferryx://join/h.example/fxk_a");
                assert_eq!(name.as_deref(), Some("ci-1"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn a_missing_group_is_a_usage_error() {
        assert!(parse_join_cli(["ferryx", "not-join", "ferryx://join/h/fxk_a"]).is_err());
        assert!(parse_join_cli(Vec::<String>::new()).is_err());
    }
}
