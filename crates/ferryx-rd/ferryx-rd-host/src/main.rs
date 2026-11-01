use std::io::{self, BufRead, Write};
use std::sync::mpsc;

use anyhow::{bail, Context, Result};
use clap::Parser;
#[cfg(target_os = "macos")]
use ferryx_rd_host::{accessibility_is_trusted, request_accessibility};
use ferryx_rd_host::{random_pin, ConsentPrompt, HostConfig, HostServer, PairingStore};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "ferryx-rd-host",
    version,
    about = "MahoRD screen streaming host for Windows, macOS, and Linux",
    long_about = "MahoRD host daemon: captures, encodes, and streams this computer's screen to paired clients over an encrypted transport, with remote input injection and optional relay fallback.\n\nFirst run on macOS: the system prompts for Screen Recording and Accessibility. Grant both in System Settings > Privacy & Security, then relaunch the host. Screen Recording is required for capture; Accessibility is required for remote input injection."
)]
struct Cli {
    /// Use this exact 8-digit bootstrap PIN.
    #[arg(long, value_name = "PIN", conflicts_with = "pin")]
    bootstrap_pin: Option<String>,

    /// Generate and display a fresh 8-digit bootstrap PIN (`--pin generate`).
    #[arg(long, value_name = "generate", conflicts_with = "bootstrap_pin")]
    pin: Option<String>,

    /// List persisted paired devices and exit. Active-session listing and kicks
    /// are API-only via HostServer; this daemon has no local control IPC server.
    #[arg(long, conflicts_with_all = ["revoke", "bootstrap_pin", "pin"])]
    list_paired: bool,

    /// Revoke one pairing ID and exit.
    #[arg(long, value_name = "ID", conflicts_with_all = ["list_paired", "bootstrap_pin", "pin"])]
    revoke: Option<String>,

    /// LAN HEVC bitrate in Mbps. Accepted range: 50 through 150.
    /// Without this flag the compatibility default is 8 Mbps.
    #[arg(long, value_name = "MBPS", value_parser = clap::value_parser!(u32).range(50..=150))]
    lan_bitrate_mbps: Option<u32>,

    /// Disable ScreenCaptureKit host-audio capture.
    #[arg(long)]
    no_audio: bool,

    /// Automatically approve incoming pairing requests (non-interactive / automation).
    #[arg(long)]
    auto_approve: bool,

    /// Output name to capture (Linux). Auto-detects the focused output when omitted.
    #[arg(long, value_name = "NAME")]
    output: Option<String>,

    /// Register and start the LocalSystem service.
    #[arg(long)]
    install_service: bool,

    /// Stop and remove the LocalSystem service.
    #[arg(long)]
    uninstall_service: bool,

    /// SCM entry point for the LocalSystem service.
    #[arg(long)]
    service_run: bool,

    /// Mark this process as a service-spawned session worker.
    #[arg(long)]
    session_worker: bool,

    /// Relay base URL (`ws://`, `wss://`, `http://`, or `https://`). Also read from MAHO_RELAY_URL.
    #[arg(long, value_name = "URL")]
    relay: Option<String>,

    /// Stable relay host id. Defaults to a sanitized hostname. Also read from MAHO_RELAY_HOST_ID.
    #[arg(long, value_name = "ID")]
    relay_host_id: Option<String>,

    /// Disable remote input injection policy.
    #[arg(long)]
    no_input: bool,

    /// Disable clipboard synchronization policy.
    #[arg(long)]
    no_clipboard: bool,

    /// TCP port to listen on (default: 19730).
    #[arg(long, value_name = "PORT")]
    tcp_port: Option<u16>,

    /// UDP port to listen on (default: 19731).
    #[arg(long, value_name = "PORT")]
    udp_port: Option<u16>,
}

pub fn select_pin<F>(
    bootstrap_pin: Option<String>,
    pin_opt: Option<&str>,
    mut generator: F,
) -> Result<String>
where
    F: FnMut() -> String,
{
    match (bootstrap_pin, pin_opt) {
        (Some(pin), None) => validate_pin(pin),
        (None, Some("generate")) | (None, None) => Ok(generator()),
        (None, Some(other)) => bail!("--pin accepts only 'generate', got '{other}'"),
        (Some(_), Some(_)) => unreachable!("clap enforces conflicts"),
    }
}

#[cfg(target_os = "windows")]
fn init_windows_dpi() {
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

fn main() -> Result<()> {
    #[cfg(target_os = "windows")]
    init_windows_dpi();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    #[cfg(target_os = "windows")]
    {
        if cli.install_service {
            ferryx_rd_host::service_windows::install(&std::env::current_exe()?)?;
            println!("Installed service MahoRDHost");
            return Ok(());
        }
        if cli.uninstall_service {
            ferryx_rd_host::service_windows::uninstall()?;
            println!("Removed service MahoRDHost");
            return Ok(());
        }
        if cli.service_run {
            ferryx_rd_host::service_windows::log_to_file();
            ferryx_rd_host::service_windows::run_service_dispatcher()?;
            return Ok(());
        }
    }

    #[cfg(target_os = "windows")]
    {
        if cli.session_worker {
            ferryx_rd_host::service_windows::log_to_file();
            ferryx_rd_host::host_log("worker: process started");
            ferryx_rd_host::service_windows::publish_input_desktop();
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if cli.install_service || cli.uninstall_service || cli.service_run {
            bail!("service mode is only supported on Windows");
        }
    }

    #[cfg(target_os = "windows")]
    let store = if cli.session_worker {
        PairingStore::service_default()
    } else {
        PairingStore::host_default()?
    };
    #[cfg(not(target_os = "windows"))]
    let store = PairingStore::host_default()?;

    if cli.list_paired {
        let mut records = store.load_all()?;
        records.sort_by_key(|left| std::cmp::Reverse(left.added_at_unix_ms));
        if records.is_empty() {
            println!("No paired devices.");
        } else {
            for record in records {
                println!(
                    "{}\t{}\t{}",
                    record.id, record.name, record.added_at_unix_ms
                );
            }
        }
        return Ok(());
    }
    if let Some(id) = cli.revoke {
        if store.revoke(&id)? {
            println!("Revoked pairing {id}");
        } else {
            bail!("pairing ID not found: {id}");
        }
        return Ok(());
    }

    let pin = select_pin(cli.bootstrap_pin, cli.pin.as_deref(), random_pin)?;

    onboard_permissions();

    let auto_approve = cli.auto_approve || cli.session_worker;
    let (consent_tx, consent_rx) = mpsc::channel::<ConsentPrompt>();
    std::thread::Builder::new()
        .name("ferryx-rd-host-consent".into())
        .spawn(move || {
            if auto_approve {
                while let Ok(prompt) = consent_rx.recv() {
                    prompt.respond(true);
                }
            } else {
                consent_loop(consent_rx);
            }
        })
        .context("failed to start consent UI channel")?;

    let mut config = {
        #[cfg(target_os = "macos")]
        {
            HostConfig::macos_default(Some(pin.clone()), store)?
        }
        #[cfg(target_os = "windows")]
        {
            // A worker spawned onto the secure desktop starts before DXGI can
            // describe an output, so startup waits instead of exiting: exiting
            // here makes the service respawn it forever.
            if cli.session_worker {
                ferryx_rd_host::windows_default_blocking(Some(pin.clone()), store)?
            } else {
                HostConfig::windows_default(Some(pin.clone()), store)?
            }
        }
        #[cfg(target_os = "linux")]
        {
            let monitors = ferryx_rd_host::probe_hyprland_monitors();
            let output = ferryx_rd_host::resolve_output_target(
                cli.output.clone(),
                std::env::var("MAHO_OUTPUT").ok(),
                monitors.as_ref(),
            );
            if let Some(name) = &output {
                eprintln!("capturing output: {name}");
            }
            HostConfig::linux_default(Some(pin.clone()), store, output)?
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            bail!(
                "ferryx-rd-host does not support this platform yet (macOS, Windows and Linux are wired)"
            );
        }
    };
    config.capture_audio = !cli.no_audio;
    config.consent_sender = Some(consent_tx);
    if cli.no_input {
        config
            .allow_input
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
    if cli.no_clipboard {
        config
            .allow_clipboard
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
    if let Some(port) = cli.tcp_port {
        config.tcp_addr.set_port(port);
    }
    if let Some(port) = cli.udp_port {
        config.udp_addr.set_port(port);
    }
    if let Some(mbps) = cli.lan_bitrate_mbps {
        config.bitrate = mbps * 1_000_000;
    }
    if cli.session_worker {
        ferryx_rd_host::host_log("worker: display ready; binding server");
    }

    let server = HostServer::bind(config)?;
    println!("MahoRD bootstrap PIN: {pin}");
    if cli.session_worker {
        // The service worker has no console, so the println above is silently
        // discarded and the bootstrap PIN would be invisible: a first pairing
        // could never be completed remotely. Duplicate it into the service log
        // (SYSTEM/Administrators only) instead.
        ferryx_rd_host::host_log(&format!("worker: bootstrap PIN {pin}"));
    }
    println!("TCP listening on {}", server.tcp_addr()?);
    println!("UDP listening on {}", server.udp_addr()?);
    let mut relay_handle = None;
    if let Some(url) = cli
        .relay
        .clone()
        .or_else(|| std::env::var("MAHO_RELAY_URL").ok())
    {
        let secret = std::env::var("RELAY_AUTH_SECRET").unwrap_or_default();
        if secret.is_empty() {
            bail!("--relay requires RELAY_AUTH_SECRET");
        }
        let host_id = cli.relay_host_id.clone().or_else(|| {
            std::env::var("MAHO_RELAY_HOST_ID")
                .ok()
                .filter(|value| !value.is_empty())
        });
        let host_id = host_id.unwrap_or_else(|| {
            ferryx_rd_host::relay::sanitize_host_id(
                &hostname::get().unwrap_or_default().to_string_lossy(),
            )
        });
        let connect_code = ferryx_rd_proto::RelayConnectCode {
            host_id: host_id.clone(),
            url: url.clone(),
        }
        .encode();
        if ferryx_rd_proto::RelayConnectCode::parse(&connect_code).is_none() {
            bail!("invalid relay connect metadata");
        }
        println!("MahoRD relay connect code (enter with PIN separately): {connect_code}");
        relay_handle = Some(ferryx_rd_host::relay::spawn_host_relay(
            ferryx_rd_host::relay::HostRelayConfig {
                url,
                host_id,
                secret: secret.into_bytes(),
                local_tcp: server.tcp_addr()?,
                local_udp: server.udp_addr()?,
            },
        )?);
        println!("Relay registration enabled");
    }
    if cli.session_worker {
        ferryx_rd_host::host_log("worker: serving");
        // Recycle watchdog: a worker spawned during desktop churn can accept
        // TCP forever while no TLS handshake ever completes (the churn wedge
        // seen at the logon screen). Retire after the startup grace so the
        // supervisor respawns a clean worker.
        let started = std::time::Instant::now();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let (accepts, successes) = ferryx_rd_host::worker_stream_counters();
            if ferryx_rd_host::windows_session::worker_should_recycle(
                accepts,
                successes,
                started.elapsed(),
                std::time::Duration::from_secs(30),
            ) {
                ferryx_rd_host::host_log(&format!(
                    "worker: recycling after {accepts} accepts with no completed handshake"
                ));
                std::process::exit(0);
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::thread::Builder::new()
            .name("ferryx-rd-host-serve-watchdog".into())
            .spawn(move || {
                let limit = std::time::Duration::from_secs(30);
                let mut last_iterations = ferryx_rd_host::server_loop_iterations();
                let mut last_progress = std::time::Instant::now();
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    let current = ferryx_rd_host::server_loop_iterations();
                    let now = std::time::Instant::now();
                    if current > last_iterations {
                        last_iterations = current;
                        last_progress = now;
                    } else if serve_loop_should_exit(
                        last_iterations,
                        current,
                        last_progress,
                        now,
                        limit,
                    ) {
                        ferryx_rd_host::host_log(
                            "accept loop is stalled; process is exiting so the supervisor can restart it",
                        );
                        std::process::exit(1);
                    }
                }
            })
            .ok();
    }
    println!("Pairing approval requests will appear in this terminal.");
    server.serve()?;
    drop(relay_handle);
    Ok(())
}

#[cfg(any(not(target_os = "windows"), test))]
fn serve_loop_should_exit(
    last_iterations: u64,
    current: u64,
    last_progress: std::time::Instant,
    now: std::time::Instant,
    limit: std::time::Duration,
) -> bool {
    current == last_iterations && now.saturating_duration_since(last_progress) > limit
}

fn validate_pin(pin: String) -> Result<String> {
    if pin.len() != 8 || !pin.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("bootstrap PIN must be exactly 8 decimal digits");
    }
    Ok(pin)
}

fn consent_loop(receiver: mpsc::Receiver<ConsentPrompt>) {
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    while let Ok(prompt) = receiver.recv() {
        print!("Approve pairing for '{}' [y/N]? ", prompt.client_name);
        let _ = io::stdout().flush();
        let approved = lines
            .next()
            .and_then(Result::ok)
            .is_some_and(|line| matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes"));
        prompt.respond(approved);
    }
}

#[cfg(target_os = "macos")]
fn onboard_permissions() {
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }

    let screen_recording = unsafe { CGPreflightScreenCaptureAccess() };
    if !screen_recording {
        eprintln!(
            "Screen Recording permission is required. macOS will prompt now; grant it in System Settings > Privacy & Security > Screen Recording, then relaunch ferryx-rd-host."
        );
        let _ = unsafe { CGRequestScreenCaptureAccess() };
    }
    if !accessibility_is_trusted() {
        eprintln!(
            "Accessibility permission is required for remote input. macOS will prompt now; enable ferryx-rd-host (or this terminal) in System Settings > Privacy & Security > Accessibility, then relaunch."
        );
        let _ = request_accessibility();
    }
}

#[cfg(not(target_os = "macos"))]
fn onboard_permissions() {
    #[cfg(target_os = "windows")]
    {
        eprintln!(
            "Windows host: DXGI Desktop Duplication + Media Foundation/NVENC are used; no TCC-style permission prompts are needed."
        );
    }
    #[cfg(target_os = "linux")]
    {
        eprintln!(
            "Linux host: wlroots/Hyprland screencopy capture; VAAPI hardware encoding when available, OpenH264 software otherwise."
        );
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        eprintln!("ferryx-rd-host capture and input are not wired for this platform yet.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pin_default_selects_injected_generator_branch() {
        let mut call_count = 0;
        let pin = select_pin(None, None, || {
            call_count += 1;
            "GENERATED_8888".to_string()
        })
        .unwrap();
        assert_eq!(call_count, 1, "default invocation must invoke generator");
        assert_eq!(pin, "GENERATED_8888");
    }

    #[test]
    fn test_pin_generate_flag_selects_injected_generator_branch() {
        let mut call_count = 0;
        let pin = select_pin(None, Some("generate"), || {
            call_count += 1;
            "GENERATED_7777".to_string()
        })
        .unwrap();
        assert_eq!(call_count, 1, "--pin generate must invoke generator");
        assert_eq!(pin, "GENERATED_7777");
    }

    #[test]
    fn test_pin_explicit_bootstrap_pin_bypasses_generator() {
        let mut call_count = 0;
        let pin = select_pin(Some("87654321".into()), None, || {
            call_count += 1;
            "GENERATED_FAIL".to_string()
        })
        .unwrap();
        assert_eq!(
            call_count, 0,
            "explicit bootstrap PIN must bypass generator"
        );
        assert_eq!(pin, "87654321");
    }

    #[test]
    fn test_pin_validation_accepts_8_digits_and_rejects_invalid() {
        assert!(validate_pin("12345678".into()).is_ok());
        assert!(validate_pin("00000000".into()).is_ok());
        assert!(validate_pin("99999999".into()).is_ok());
        assert!(validate_pin("1234567".into()).is_err());
        assert!(validate_pin("123456789".into()).is_err());
        assert!(validate_pin("1234abcd".into()).is_err());
        assert!(validate_pin(" 1234567".into()).is_err());
    }

    #[test]
    fn test_random_pin_retained_and_format_valid() {
        let pin = random_pin();
        assert_eq!(pin.len(), 8);
        assert!(pin.bytes().all(|b| b.is_ascii_digit()));
    }

    #[test]
    fn test_service_cli_flags_parse() {
        let cli = Cli::parse_from([
            "ferryx-rd-host",
            "--install-service",
            "--uninstall-service",
            "--service-run",
            "--session-worker",
        ]);
        assert!(cli.install_service);
        assert!(cli.uninstall_service);
        assert!(cli.service_run);
        assert!(cli.session_worker);
    }

    #[test]
    fn test_serve_loop_should_exit_truth_table() {
        let now = std::time::Instant::now();
        let limit = std::time::Duration::from_secs(30);

        // Advancing iterations: never exit
        assert!(!serve_loop_should_exit(
            10,
            11,
            now - std::time::Duration::from_secs(40),
            now,
            limit
        ));

        // Stalled iterations within limit: do not exit
        assert!(!serve_loop_should_exit(
            10,
            10,
            now - std::time::Duration::from_secs(20),
            now,
            limit
        ));

        // Stalled iterations exceeding limit: exit
        assert!(serve_loop_should_exit(
            10,
            10,
            now - std::time::Duration::from_secs(35),
            now,
            limit
        ));
    }
}
