//! `ferryx-relay` - standalone entrypoint for the public relay server that
//! brokers WebSocket tunnels between desktop daemons and remote clients.
//!
//! Usage:
//!
//! ```text
//! ferryx-relay --port 8787 --machine-token <token> [--machine-token <token> ...]
//! ```
//!
//! The port defaults to `8787` and can also be set via `FERRYX_RELAY_PORT`.
//! Machine Tokens can additionally be supplied via the comma-separated
//! `FERRYX_RELAY_MACHINE_TOKENS` environment variable; tokens from both the
//! environment and `--machine-token` flags are accepted.
//! The relay origin advertised in machine records can be configured via
//! `FERRYX_ACCOUNT_RELAY_ORIGIN`, falling back to the resolved account origin.
//!
//! - `FERRYX_ACCOUNT_ORIGIN`: public origin of the embedded account service. Required in
//!   selfhost mode (commercial mode defaults to the hosted relay); must be `https`, or `http`
//!   on loopback.
//! - `--bind <ip>` / `FERRYX_RELAY_BIND`: listen address, default `0.0.0.0`; `[::]` listens on
//!   IPv6 and `127.0.0.1` / `::1` keep the relay on loopback behind a local reverse proxy.
//! - Mail: `RESEND_API_KEY` with `FERRYX_MAIL_FROM`, or `FERRYX_MAIL_WEBHOOK_URL`. Without
//!   either the relay refuses to start unless `FERRYX_MAIL_DEV_FILE=1` (development only)
//!   spools magic links to `<account data dir>/mail`.
//! - `FERRYX_DOWNLOADS_DIR`: the directory `/download/<artifact>` serves CLI binaries from.
//!   Unset, commercial mode keeps its `~/.ferryx/downloads` default and selfhost mode
//!   redirects downloads to the GitHub release; `~/bin` is never served.
//!
//! HTTPS (optional; without these flags the relay serves plain HTTP exactly as before):
//!
//! - `--tls-domain <name>` / `FERRYX_RELAY_TLS_DOMAIN` (repeatable, comma-separated): obtain
//!   and renew Let's Encrypt certificates over TLS-ALPN-01 on the listening port.
//! - `--tls-contact <email>` / `FERRYX_RELAY_TLS_CONTACT`: ACME account contact.
//! - `--tls-cache-dir <path>` / `FERRYX_RELAY_TLS_CACHE_DIR`: persisted ACME account key and
//!   certificates (default `<home>/.ferryx/relay-tls`).
//! - `--tls-staging` / `FERRYX_RELAY_TLS_STAGING=1`: use the Let's Encrypt staging directory.
//! - `--tls-cert <pem> --tls-key <pem>` / `FERRYX_RELAY_TLS_CERT`, `FERRYX_RELAY_TLS_KEY`:
//!   serve an operator-provided certificate instead of ACME.
//!
//! With TLS enabled the port defaults to `443` instead of `8787`.
//!
//! Behind a reverse proxy (cloudflared, nginx, ...):
//!
//! - `FERRYX_RELAY_TRUSTED_PROXIES`: comma-separated CIDRs (or bare addresses) of the proxies
//!   whose `X-Forwarded-For` / `CF-Connecting-IP` headers name the real client
//!   (`CF-Connecting-IP` only when no `X-Forwarded-For` is present). Unset, every
//!   forwarding header is ignored and rate limits apply to the socket peer address. A
//!   malformed entry refuses to start.

use base64::Engine as _;
use ferryx_lib::remote::relay_server::{
    parse_trusted_proxies, relay_router_with_account, spawn_session_reaper,
    spawn_suspension_sweeper, RelayState, TrustedProxy, DEFAULT_RELAY_URL,
};
use ferryx_lib::remote::relay_tls::{self, AcmeSettings, TlsAcceptorMode};
use ferryx_lib::account::origin::{
    deployment_mode_with, missing_lemon_squeezy_env_vars, DeploymentMode,
};
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::Arc;

const DEFAULT_HTTP_PORT: u16 = 8787;
const DEFAULT_TLS_PORT: u16 = 443;

#[derive(Debug, PartialEq)]
enum TlsConfig {
    /// Plain HTTP, the historical behavior.
    Off,
    /// Operator-provided PEM certificate chain and private key.
    Manual { cert: PathBuf, key: PathBuf },
    /// Let's Encrypt certificates managed in-process.
    Acme {
        domains: Vec<String>,
        contact: Option<String>,
        /// `None` resolves to `<home>/.ferryx/relay-tls` at startup.
        cache_dir: Option<PathBuf>,
        staging: bool,
    },
}

struct RelayConfig {
    port: u16,
    bind: IpAddr,
    account_origin: String,
    machine_tokens: Vec<String>,
    account_public_key: Option<String>,
    deployment_mode: DeploymentMode,
    tls: TlsConfig,
    trusted_proxies: Vec<TrustedProxy>,
}

fn split_list(value: &str) -> impl Iterator<Item = String> + '_ {
    value
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

fn parse_flag(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

/// A listen address without a port: `0.0.0.0`, `::`, `[::]`, `127.0.0.1`, `[::1]` or
/// `localhost`.
fn parse_bind(value: &str) -> Result<IpAddr, String> {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("localhost") {
        return Ok(IpAddr::V4(Ipv4Addr::LOCALHOST));
    }
    let unbracketed = trimmed
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(trimmed);
    unbracketed.parse().map_err(|_| {
        format!(
            "invalid bind address {value:?}: expected an IP address such as 0.0.0.0, [::] or \
             127.0.0.1, without a port (use --port)"
        )
    })
}

/// Selfhost mode must name its own account origin; only commercial mode keeps the hosted
/// relay as its default. Either way the value is normalized like every other account origin.
fn resolve_account_origin(mode: DeploymentMode, raw: Option<String>) -> Result<String, String> {
    let raw = match non_empty(raw) {
        Some(value) => value,
        None => match mode {
            DeploymentMode::SelfHost => {
                return Err(
                    "selfhost mode requires FERRYX_ACCOUNT_ORIGIN, the public origin \
                     users reach this relay at (https, or http on loopback)"
                        .into(),
                )
            }
            DeploymentMode::Commercial => DEFAULT_RELAY_URL.to_string(),
        },
    };
    ferryx_lib::account::origin::normalize_account_origin(raw.trim())
        .map_err(|err| format!("FERRYX_ACCOUNT_ORIGIN: {err}"))
}

fn parse_args(args: &[String]) -> Result<RelayConfig, String> {
    parse_args_with(args, |name| std::env::var(name).ok())
}

/// `env` looks up environment variables, so tests can supply them without touching the
/// process environment.
fn parse_args_with(
    args: &[String],
    env: impl Fn(&str) -> Option<String>,
) -> Result<RelayConfig, String> {
    let deployment_mode = deployment_mode_with(&env)
        .map_err(|err| format!("configuration error: {}", err.message))?;
    let account_origin = resolve_account_origin(deployment_mode, env("FERRYX_ACCOUNT_ORIGIN"))?;
    // Empty means unset (systemd `Environment=FERRYX_RELAY_BIND=`); whitespace-only is a
    // typo and is rejected by parse_bind rather than silently falling back to 0.0.0.0.
    let mut bind = env("FERRYX_RELAY_BIND")
        .filter(|value| !value.is_empty())
        .map(|value| parse_bind(&value))
        .transpose()
        .map_err(|err| format!("FERRYX_RELAY_BIND: {err}"))?;

    let mut port: Option<u16> = env("FERRYX_RELAY_PORT").and_then(|value| value.parse().ok());
    let mut machine_tokens: Vec<String> = env("FERRYX_RELAY_MACHINE_TOKENS")
        .map(|value| split_list(&value).collect())
        .unwrap_or_default();
    let mut account_public_key = non_empty(env("FERRYX_RELAY_ACCOUNT_PUBLIC_KEY"));
    let mut tls_domains: Vec<String> = env("FERRYX_RELAY_TLS_DOMAIN")
        .map(|value| split_list(&value).collect())
        .unwrap_or_default();
    let mut tls_contact = non_empty(env("FERRYX_RELAY_TLS_CONTACT"));
    let mut tls_cache_dir = non_empty(env("FERRYX_RELAY_TLS_CACHE_DIR")).map(PathBuf::from);
    let mut tls_staging = env("FERRYX_RELAY_TLS_STAGING").is_some_and(|value| parse_flag(&value));
    let mut tls_cert = non_empty(env("FERRYX_RELAY_TLS_CERT")).map(PathBuf::from);
    let mut tls_key = non_empty(env("FERRYX_RELAY_TLS_KEY")).map(PathBuf::from);
    let trusted_proxies = env("FERRYX_RELAY_TRUSTED_PROXIES")
        .map(|value| parse_trusted_proxies(&value))
        .transpose()
        .map_err(|err| format!("FERRYX_RELAY_TRUSTED_PROXIES: {err}"))?
        .unwrap_or_default();

    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value_of = |flag: &str| -> Result<String, String> {
            args.next()
                .cloned()
                .ok_or_else(|| format!("{flag} requires a value"))
        };
        match arg.as_str() {
            "--port" => {
                let value = value_of("--port")?;
                port = Some(
                    value
                        .parse()
                        .map_err(|_| format!("invalid --port value: {value}"))?,
                );
            }
            "--bind" => bind = Some(parse_bind(&value_of("--bind")?)?),
            "--machine-token" => {
                machine_tokens.push(value_of("--machine-token")?);
            }
            "--account-public-key" => {
                account_public_key = Some(value_of("--account-public-key")?);
            }
            "--tls-domain" => tls_domains.extend(split_list(&value_of("--tls-domain")?)),
            "--tls-contact" => tls_contact = Some(value_of("--tls-contact")?),
            "--tls-cache-dir" => tls_cache_dir = Some(PathBuf::from(value_of("--tls-cache-dir")?)),
            "--tls-staging" => tls_staging = true,
            "--tls-cert" => tls_cert = Some(PathBuf::from(value_of("--tls-cert")?)),
            "--tls-key" => tls_key = Some(PathBuf::from(value_of("--tls-key")?)),
            other => {
                return Err(format!("unrecognized argument: {other}"));
            }
        }
    }

    let acme_only_option = tls_contact.is_some() || tls_cache_dir.is_some() || tls_staging;
    let tls = match (tls_cert, tls_key) {
        (Some(cert), Some(key)) => {
            if !tls_domains.is_empty() || acme_only_option {
                return Err(
                    "--tls-cert/--tls-key serve a fixed certificate and cannot be combined with \
                     --tls-domain, --tls-contact, --tls-cache-dir or --tls-staging"
                        .into(),
                );
            }
            TlsConfig::Manual { cert, key }
        }
        (Some(_), None) => return Err("--tls-cert requires --tls-key".into()),
        (None, Some(_)) => return Err("--tls-key requires --tls-cert".into()),
        (None, None) if !tls_domains.is_empty() => TlsConfig::Acme {
            domains: tls_domains,
            contact: tls_contact,
            cache_dir: tls_cache_dir,
            staging: tls_staging,
        },
        (None, None) => {
            if acme_only_option {
                return Err(
                    "--tls-contact, --tls-cache-dir and --tls-staging require --tls-domain".into(),
                );
            }
            TlsConfig::Off
        }
    };
    let default_port = if tls == TlsConfig::Off {
        DEFAULT_HTTP_PORT
    } else {
        DEFAULT_TLS_PORT
    };

    Ok(RelayConfig {
        port: port.unwrap_or(default_port),
        bind: bind.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
        account_origin,
        machine_tokens,
        account_public_key,
        deployment_mode,
        tls,
        trusted_proxies,
    })
}

/// `<home>/.ferryx`. There is deliberately no working-directory fallback: a relay
/// started without `HOME` (Windows leaves it unset) must fail fast rather than write
/// durable data wherever it happened to be launched from.
fn ferryx_home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .or_else(|| std::env::var_os("LOCALAPPDATA"));
    #[cfg(not(windows))]
    let base = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    base.filter(|base| !base.is_empty())
        .map(|base| PathBuf::from(base).join(".ferryx"))
}

/// `<home>/.ferryx/account-data`, the directory holding persistent relay account
/// state and the mail spool.
fn default_account_data_dir() -> Option<PathBuf> {
    ferryx_home_dir().map(|dir| dir.join("account-data"))
}

/// Resolves the TLS configuration into an acceptor. Disk work (reading PEM files,
/// creating the ACME cache) runs on a blocking thread.
async fn prepare_tls(tls: TlsConfig) -> Result<Option<TlsAcceptorMode>, String> {
    match tls {
        TlsConfig::Off => Ok(None),
        TlsConfig::Manual { cert, key } => {
            let config = tokio::task::spawn_blocking(move || {
                relay_tls::load_static_server_config(&cert, &key)
            })
            .await
            .map_err(|err| format!("TLS setup task failed: {err}"))??;
            eprintln!("ferryx-relay: tls: serving the operator-provided certificate");
            Ok(Some(TlsAcceptorMode::Static(config)))
        }
        TlsConfig::Acme {
            domains,
            contact,
            cache_dir,
            staging,
        } => {
            let cache_dir = match cache_dir.or_else(|| ferryx_home_dir().map(|d| d.join("relay-tls"))) {
                Some(dir) => dir,
                None => {
                    return Err("cannot resolve the TLS cache directory; set \
                                FERRYX_RELAY_TLS_CACHE_DIR or pass --tls-cache-dir"
                        .into())
                }
            };
            let probe_dir = cache_dir.clone();
            tokio::task::spawn_blocking(move || relay_tls::prepare_cache_dir(&probe_dir))
                .await
                .map_err(|err| format!("TLS setup task failed: {err}"))??;
            relay_tls::start_acme(AcmeSettings {
                domains,
                contact,
                cache_dir,
                staging,
            })
            .map(Some)
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = match parse_args(&args) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("ferryx-relay: {err}");
            std::process::exit(2);
        }
    };

    if config.machine_tokens.is_empty() {
        eprintln!(
            "ferryx-relay: no Machine Tokens configured; set FERRYX_RELAY_MACHINE_TOKENS \
             or pass --machine-token, otherwise no daemon will be able to authenticate"
        );
    }

    match config.deployment_mode {
        DeploymentMode::SelfHost => {
            tracing::info!(
                "ferryx-relay running in selfhost mode: billing disabled, all entitlement checks skipped"
            );
        }
        DeploymentMode::Commercial => {
            let missing = missing_lemon_squeezy_env_vars();
            if !missing.is_empty() {
                eprintln!(
                    "ferryx-relay: WARNING: commercial mode is running without complete Lemon Squeezy configuration; \
                     missing variables: {}. Billing routes and paid surfaces will fail closed with 503 BILLING_UNCONFIGURED",
                    missing.join(", ")
                );
            } else {
                tracing::info!("ferryx-relay running in commercial mode with Lemon Squeezy configured");
            }
        }
    }

    let state = match RelayState::try_new(config.machine_tokens) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("ferryx-relay: {err}");
            std::process::exit(2);
        }
    };
    state.set_trusted_proxies(config.trusted_proxies);
    spawn_session_reaper(state.clone());

    let origin = config.account_origin;
    let relay_origin = std::env::var("FERRYX_ACCOUNT_RELAY_ORIGIN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| origin.clone());
    let data_dir = match std::env::var_os("FERRYX_ACCOUNT_DATA_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => match default_account_data_dir() {
            Some(dir) => dir,
            None => {
                eprintln!(
                    "ferryx-relay: cannot resolve the account data directory; set \
                     FERRYX_ACCOUNT_DATA_DIR or HOME (USERPROFILE on Windows), otherwise \
                     persistent account data would be written relative to the working directory"
                );
                std::process::exit(2);
            }
        },
    };
    let mailer = match ferryx_lib::account::mailer::create_production_mailer(data_dir.join("mail"))
    {
        Ok(chosen) => {
            if let Some(dir) = &chosen.spool_dir {
                let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.clone());
                eprintln!(
                    "ferryx-relay: WARNING: {}=1: magic links are NOT emailed; they are written \
                     to {} (development only)",
                    ferryx_lib::account::mailer::MAIL_DEV_FILE_ENV,
                    dir.display()
                );
            }
            chosen.mailer
        }
        Err(err) => {
            eprintln!("ferryx-relay: {err}");
            std::process::exit(2);
        }
    };
    let account_state = Arc::new(
        ferryx_lib::account::service::AccountState::new(&data_dir, &origin, mailer)
            .with_relay_origin(relay_origin)
            .with_deployment_mode(config.deployment_mode),
    );

    let account_public_key = config.account_public_key.or_else(|| {
        account_state.signing_key().ok().map(|k| {
            base64::engine::general_purpose::STANDARD.encode(k.verifying_key().as_bytes())
        })
    });
    let router = relay_router_with_account(
        state.clone(),
        account_public_key,
        Some(account_state.clone()),
    );

    // A warm standby's account store is a read-only replica; the write-side sweepers start
    // after a promotion, which removes the marker and restarts the unit.
    let standby = account_state.is_read_only();
    if standby {
        tracing::info!("ferryx-relay running as a read-only warm standby: account writes are refused");
    }
    if matches!(config.deployment_mode, DeploymentMode::Commercial) && !standby {
        // Only a commercial relay enforces suspension. The task is detached on purpose: it
        // must outlive this scope for the whole process, like the session reaper above.
        let _suspension_sweeper = spawn_suspension_sweeper(state);

        // The grace, suspension and recovery notices are clock-driven and must fire while the
        // entitlement state stays unchanged, so the commercial relay runs one notice sweeper
        // next to the suspension sweeper. Self-host never enables billing and starts neither.
        let _notice_sweeper =
            ferryx_lib::account::billing::notices::spawn_billing_notice_sweeper(account_state);
    }

    let addr = std::net::SocketAddr::new(config.bind, config.port);
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("ferryx-relay: failed to bind {addr}: {err}");
            std::process::exit(1);
        }
    };

    // Certificates are prepared only after the port is held, so ACME validation can reach it.
    let tls = match prepare_tls(config.tls).await {
        Ok(tls) => tls,
        Err(err) => {
            eprintln!("ferryx-relay: {err}");
            std::process::exit(2);
        }
    };
    if let Some(mode) = tls {
        tracing::info!("ferryx-relay listening on {addr} (HTTPS)");
        if let Err(err) = relay_tls::serve(listener, mode, router).await {
            eprintln!("ferryx-relay: server error: {err}");
            std::process::exit(1);
        }
        return;
    }

    tracing::info!("ferryx-relay listening on {addr}");
    if let Err(err) = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    {
        eprintln!("ferryx-relay: server error: {err}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn parse(args: &[&str], env: &[(&str, &str)]) -> Result<RelayConfig, String> {
        let mut vars: HashMap<String, String> = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        vars.entry("FERRYX_DEPLOYMENT_MODE".into())
            .or_insert_with(|| "selfhost".into());
        vars.entry("FERRYX_ACCOUNT_ORIGIN".into())
            .or_insert_with(|| "https://relay.example.com".into());
        let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        parse_args_with(&args, |name| vars.get(name).cloned())
    }

    #[test]
    fn no_tls_flags_keep_plain_http_on_8787() {
        let config = parse(&[], &[]).unwrap();
        assert_eq!(config.tls, TlsConfig::Off);
        assert_eq!(config.port, 8787);
        assert_eq!(parse(&["--port", "9000"], &[]).unwrap().port, 9000);
    }

    #[test]
    fn tls_domain_enables_acme_and_defaults_port_to_443() {
        let config = parse(
            &[
                "--tls-domain",
                "relay.example.com, alt.example.com",
                "--tls-domain",
                "third.example.com",
                "--tls-contact",
                "ops@example.com",
                "--tls-cache-dir",
                "/srv/relay-tls",
                "--tls-staging",
            ],
            &[],
        )
        .unwrap();
        assert_eq!(config.port, 443);
        assert_eq!(
            config.tls,
            TlsConfig::Acme {
                domains: vec![
                    "relay.example.com".into(),
                    "alt.example.com".into(),
                    "third.example.com".into()
                ],
                contact: Some("ops@example.com".into()),
                cache_dir: Some(PathBuf::from("/srv/relay-tls")),
                staging: true,
            }
        );
    }

    #[test]
    fn tls_settings_are_read_from_the_environment() {
        let config = parse(
            &[],
            &[
                ("FERRYX_RELAY_TLS_DOMAIN", "relay.example.com,alt.example.com"),
                ("FERRYX_RELAY_TLS_CONTACT", "ops@example.com"),
                ("FERRYX_RELAY_TLS_STAGING", "1"),
            ],
        )
        .unwrap();
        assert_eq!(config.port, 443);
        assert_eq!(
            config.tls,
            TlsConfig::Acme {
                domains: vec!["relay.example.com".into(), "alt.example.com".into()],
                contact: Some("ops@example.com".into()),
                cache_dir: None,
                staging: true,
            }
        );
        // An explicit port still wins over the TLS default.
        let config = parse(&["--port", "8443"], &[("FERRYX_RELAY_TLS_DOMAIN", "relay.example.com")])
            .unwrap();
        assert_eq!(config.port, 8443);
        let config = parse(
            &[],
            &[("FERRYX_RELAY_TLS_DOMAIN", "relay.example.com"), ("FERRYX_RELAY_PORT", "8443")],
        )
        .unwrap();
        assert_eq!(config.port, 8443);
        let config = parse(
            &[],
            &[("FERRYX_RELAY_TLS_DOMAIN", "relay.example.com"), ("FERRYX_RELAY_TLS_STAGING", "0")],
        )
        .unwrap();
        assert!(matches!(config.tls, TlsConfig::Acme { staging: false, .. }));
    }

    #[test]
    fn manual_certificate_mode_uses_443() {
        let config = parse(&["--tls-cert", "/etc/cert.pem", "--tls-key", "/etc/key.pem"], &[])
            .unwrap();
        assert_eq!(config.port, 443);
        assert_eq!(
            config.tls,
            TlsConfig::Manual {
                cert: PathBuf::from("/etc/cert.pem"),
                key: PathBuf::from("/etc/key.pem"),
            }
        );
    }

    #[test]
    fn inconsistent_tls_flags_are_rejected() {
        for args in [
            &["--tls-cert", "c.pem"][..],
            &["--tls-key", "k.pem"][..],
            &["--tls-cert", "c.pem", "--tls-key", "k.pem", "--tls-domain", "a.example"][..],
            &["--tls-staging"][..],
            &["--tls-contact", "ops@example.com"][..],
            &["--tls-domain"][..],
        ] {
            assert!(parse(args, &[]).is_err(), "{args:?} must be rejected");
        }
    }

    #[test]
    fn trusted_proxies_default_to_none_and_reject_malformed_cidrs() {
        assert!(parse(&[], &[]).unwrap().trusted_proxies.is_empty());
        let config = parse(
            &[],
            &[(
                "FERRYX_RELAY_TRUSTED_PROXIES",
                "127.0.0.1/32, ::1, 10.0.0.0/8",
            )],
        )
        .unwrap();
        assert_eq!(config.trusted_proxies.len(), 3);
        for value in ["10.0.0.0/33", "not-an-ip", "10.0.0.0/", "10.0.0.0/8,bogus"] {
            assert!(
                parse(&[], &[("FERRYX_RELAY_TRUSTED_PROXIES", value)]).is_err(),
                "{value:?} must be rejected"
            );
        }
    }
    #[test]
    fn selfhost_requires_an_account_origin_and_normalizes_it() {
        let err = parse(&[], &[("FERRYX_ACCOUNT_ORIGIN", "")])
            .err()
            .expect("unset origin");
        assert!(err.contains("FERRYX_ACCOUNT_ORIGIN"), "{err}");
        assert!(parse(&[], &[("FERRYX_ACCOUNT_ORIGIN", "   ")]).is_err());
        assert_eq!(
            parse(
                &[],
                &[("FERRYX_ACCOUNT_ORIGIN", " https://Relay.Example.com/ ")]
            )
            .unwrap()
            .account_origin,
            "https://relay.example.com"
        );
        assert_eq!(
            parse(&[], &[("FERRYX_ACCOUNT_ORIGIN", "http://127.0.0.1:8787")])
                .unwrap()
                .account_origin,
            "http://127.0.0.1:8787"
        );
        for value in [
            "http://relay.example.com",
            "https://relay.example.com/path",
            "not a url",
        ] {
            let err = parse(&[], &[("FERRYX_ACCOUNT_ORIGIN", value)])
                .err()
                .unwrap_or_else(|| panic!("{value:?} must be rejected"));
            assert!(
                err.starts_with("FERRYX_ACCOUNT_ORIGIN: ACCOUNT_ORIGIN_"),
                "{err}"
            );
        }
    }

    #[test]
    fn commercial_mode_keeps_the_hosted_account_origin_default() {
        let config = parse(
            &[],
            &[
                ("FERRYX_DEPLOYMENT_MODE", "commercial"),
                ("FERRYX_ACCOUNT_ORIGIN", ""),
            ],
        )
        .unwrap();
        assert_eq!(config.account_origin, DEFAULT_RELAY_URL);
    }

    #[test]
    fn bind_defaults_to_all_ipv4_and_accepts_ipv6_and_loopback() {
        assert_eq!(
            parse(&[], &[]).unwrap().bind,
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        );
        for (value, expected) in [
            ("[::]", "::"),
            ("::", "::"),
            ("127.0.0.1", "127.0.0.1"),
            ("[::1]", "::1"),
            ("localhost", "127.0.0.1"),
        ] {
            let expected: IpAddr = expected.parse().unwrap();
            assert_eq!(
                parse(&["--bind", value], &[]).unwrap().bind,
                expected,
                "{value}"
            );
            assert_eq!(
                parse(&[], &[("FERRYX_RELAY_BIND", value)]).unwrap().bind,
                expected,
                "{value}"
            );
        }
        assert_eq!(
            parse(&["--bind", "::1"], &[("FERRYX_RELAY_BIND", "0.0.0.0")])
                .unwrap()
                .bind,
            "::1".parse::<IpAddr>().unwrap(),
            "the flag wins over the environment"
        );
        for value in ["127.0.0.1:8787", "relay.example.com", "[::1", ""] {
            assert!(
                parse(&["--bind", value], &[]).is_err(),
                "{value:?} must be rejected"
            );
        }
        assert!(parse(&["--bind"], &[]).is_err());
        assert!(parse(&[], &[("FERRYX_RELAY_BIND", "bogus")]).is_err());
        for value in [" ", "\t", "  \n"] {
            let err = parse(&[], &[("FERRYX_RELAY_BIND", value)])
                .err()
                .unwrap_or_else(|| panic!("{value:?} must be rejected, not treated as unset"));
            assert!(err.contains("FERRYX_RELAY_BIND"), "{err}");
        }
        assert_eq!(
            parse(&[], &[("FERRYX_RELAY_BIND", "")]).unwrap().bind,
            "0.0.0.0".parse::<IpAddr>().unwrap(),
            "an empty value is unset"
        );
    }
}

