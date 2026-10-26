//! Continuous-input handover QA helper. Not product code.
//!
//! Drives the product input path, `DaemonClient::new_with_socket(..).write_terminal(..)`, on a
//! fixed schedule (default every 20 ms) into one session of an isolated daemon while the harness
//! (`handover-qa.mjs --input-probe`) runs a v5 handover underneath it. The probe never retries a
//! write itself: whatever `write_terminal` returns is the receipt. Retries inside
//! `write_terminal` (HANDOVER_BUSY, the client's own reconnect rules) are the behavior under test.
//! Writes are sequential: a slow write delays the next one, and that lateness is recorded per
//! sequence instead of being hidden by concurrent sends that could reorder input.
//!
//! Each write sends `<marker>-<seq:06>\n`. stdout carries one JSON object per line:
//!   {"type":"start",...}    once, before the first write
//!   {"type":"write",...}    one per attempted sequence; result "ok" | "error" | "timeout"
//!   {"type":"ready",...}    once, after --ready-after acknowledged writes
//!   {"type":"summary",...}  once, at the end; "pass" only when every sequence returned Ok
//! An error or timeout is never a success: its delivery is "unknown" unless the error itself says
//! nothing was written ("reportedNotWritten"); either way the run fails.
//! Exit status: 0 only when the summary passes, 1 otherwise, 64 on bad arguments, and 2 when
//! started as `--daemon` (the client's spawn fallback re-executes current_exe with --daemon; this
//! binary must never pretend to be a daemon).
//!
//! Usage:
//!   pty_handover_input_probe --socket <runtime>/daemon.sock --session <id> --marker <alnum>
//!       [--count N] [--interval-ms MS] [--ready-after N] [--write-timeout-ms MS] [--deadline-ms MS]
//!
//! Run it with FERRYX_RUNTIME_DIR naming the isolated runtime that holds --socket, so every path
//! the client derives stays inside that runtime and never reaches the per-user daemon.

use ferryx_lib::daemon::DaemonClient;
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use tokio::time::Instant;

const USAGE: &str = "usage: pty_handover_input_probe --socket <path> --session <id> --marker <alnum> \
[--count N] [--interval-ms MS] [--ready-after N] [--write-timeout-ms MS] [--deadline-ms MS]";

struct Options {
    socket: PathBuf,
    session: String,
    marker: String,
    count: u32,
    interval_ms: u64,
    ready_after: u32,
    write_timeout_ms: u64,
    deadline_ms: u64,
}

fn take<'a>(iter: &mut std::slice::Iter<'a, String>, flag: &str) -> Result<&'a String, String> {
    iter.next().ok_or_else(|| format!("missing value for {flag}"))
}

fn number<T>(text: &str, flag: &str, min: T, max: T) -> Result<T, String>
where
    T: std::str::FromStr + PartialOrd + std::fmt::Display + Copy,
{
    let parsed: T = text
        .parse()
        .map_err(|_| format!("{flag} needs an integer, got {text}"))?;
    if parsed < min || parsed > max {
        return Err(format!("{flag} must be within {min}..={max}, got {parsed}"));
    }
    Ok(parsed)
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut socket = None;
    let mut session = None;
    let mut marker = None;
    let mut count = 500_u32;
    let mut interval_ms = 20_u64;
    let mut ready_after = 5_u32;
    let mut write_timeout_ms = 10_000_u64;
    let mut deadline_ms = None;
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        match flag.as_str() {
            "--socket" => socket = Some(PathBuf::from(take(&mut iter, flag)?)),
            "--session" => session = Some(take(&mut iter, flag)?.clone()),
            "--marker" => marker = Some(take(&mut iter, flag)?.clone()),
            "--count" => count = number(take(&mut iter, flag)?, flag, 1, 100_000)?,
            "--interval-ms" => interval_ms = number(take(&mut iter, flag)?, flag, 1, 10_000)?,
            "--ready-after" => ready_after = number(take(&mut iter, flag)?, flag, 1, 100_000)?,
            "--write-timeout-ms" => {
                write_timeout_ms = number(take(&mut iter, flag)?, flag, 1, 600_000)?;
            }
            "--deadline-ms" => deadline_ms = Some(number(take(&mut iter, flag)?, flag, 1, 3_600_000)?),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let socket = socket.ok_or("--socket is required")?;
    if !socket.is_absolute() {
        return Err(format!("--socket must be absolute, got {}", socket.display()));
    }
    let session = session.filter(|value| !value.is_empty()).ok_or("--session is required")?;
    let marker = marker.ok_or("--marker is required")?;
    // The marker travels through a shell `read` in the fixture, so it stays plain alphanumeric.
    if marker.is_empty() || marker.len() > 64 || !marker.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(format!("--marker must be 1..=64 ASCII alphanumerics, got {marker:?}"));
    }
    if ready_after >= count {
        return Err(format!("--ready-after ({ready_after}) must be below --count ({count})"));
    }
    let deadline_ms = deadline_ms.unwrap_or(u64::from(count) * interval_ms + write_timeout_ms);
    Ok(Options {
        socket,
        session,
        marker,
        count,
        interval_ms,
        ready_after,
        write_timeout_ms,
        deadline_ms,
    })
}

fn emit(value: &Value) -> std::io::Result<()> {
    let mut out = std::io::stdout().lock();
    serde_json::to_writer(&mut out, value)?;
    out.write_all(b"\n")?;
    out.flush()
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

async fn run(options: Options) -> std::io::Result<bool> {
    let client = DaemonClient::new_with_socket(options.socket.clone());
    let interval = Duration::from_millis(options.interval_ms);
    let write_timeout = Duration::from_millis(options.write_timeout_ms);
    emit(&json!({
        "type": "start",
        "pid": std::process::id(),
        "socket": options.socket.display().to_string(),
        "sessionId": options.session,
        "marker": options.marker,
        "count": options.count,
        "intervalMs": options.interval_ms,
        "readyAfter": options.ready_after,
        "writeTimeoutMs": options.write_timeout_ms,
        "deadlineMs": options.deadline_ms,
    }))?;

    let origin = Instant::now();
    let deadline = origin + Duration::from_millis(options.deadline_ms);
    let mut attempted = 0_u32;
    let mut acknowledged = 0_u32;
    let mut failed = 0_u32;
    let mut max_latency = Duration::ZERO;
    let mut max_late = Duration::ZERO;
    let mut ready = false;
    let mut stop_reason: Option<&str> = None;

    for seq in 0..options.count {
        // Absolute schedule: sequence n is due at origin + n * interval, whatever came before.
        let scheduled = origin + interval * seq;
        if scheduled >= deadline {
            stop_reason = Some("deadline");
            break;
        }
        tokio::time::sleep_until(scheduled).await;
        let payload = format!("{}-{seq:06}\n", options.marker);
        let started = Instant::now();
        let outcome = tokio::time::timeout_at(
            (started + write_timeout).min(deadline),
            client.write_terminal(&options.session, payload.clone().into_bytes()),
        )
        .await;
        let completed = Instant::now();
        attempted += 1;
        let latency = completed.duration_since(started);
        let late = started.saturating_duration_since(scheduled);
        max_latency = max_latency.max(latency);
        max_late = max_late.max(late);

        let mut receipt = json!({
            "type": "write",
            "seq": seq,
            "payload": payload.trim_end(),
            "bytes": payload.len(),
            "scheduledMs": millis(scheduled.duration_since(origin)),
            "startedMs": millis(started.duration_since(origin)),
            "completedMs": millis(completed.duration_since(origin)),
            "latencyMs": millis(latency),
            "lateMs": millis(late),
        });
        let timed_out = match outcome {
            Ok(Ok(())) => {
                acknowledged += 1;
                receipt["result"] = json!("ok");
                false
            }
            Ok(Err(error)) => {
                failed += 1;
                let reported_not_written = error
                    .details
                    .as_ref()
                    .and_then(|details| details.get("inputWritten"))
                    .and_then(Value::as_bool)
                    == Some(false);
                receipt["result"] = json!("error");
                receipt["delivery"] = json!(if reported_not_written { "reportedNotWritten" } else { "unknown" });
                receipt["error"] = serde_json::to_value(&error).unwrap_or_else(|serialize_error| {
                    json!({ "serializeError": serialize_error.to_string(), "display": error.to_string() })
                });
                false
            }
            Err(_elapsed) => {
                // The write future was dropped mid-flight: nothing is known about delivery, and
                // the client's connection state is no longer trusted, so the run stops here.
                failed += 1;
                receipt["result"] = json!("timeout");
                receipt["delivery"] = json!("unknown");
                true
            }
        };
        emit(&receipt)?;
        if !ready && acknowledged >= options.ready_after {
            ready = true;
            emit(&json!({
                "type": "ready",
                "acknowledged": acknowledged,
                "atMs": millis(Instant::now().duration_since(origin)),
            }))?;
        }
        if timed_out {
            stop_reason = Some("writeTimeout");
            break;
        }
    }

    let not_attempted = options.count - attempted;
    let pass = failed == 0 && not_attempted == 0;
    emit(&json!({
        "type": "summary",
        "pass": pass,
        "count": options.count,
        "attempted": attempted,
        "acknowledged": acknowledged,
        "failed": failed,
        "notAttempted": not_attempted,
        "stopReason": stop_reason,
        "ready": ready,
        "maxLatencyMs": millis(max_latency),
        "maxLateMs": millis(max_late),
        "elapsedMs": millis(Instant::now().duration_since(origin)),
    }))?;
    Ok(pass)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--daemon") {
        eprintln!("pty_handover_input_probe: refusing --daemon; the client tried to spawn a daemon from the probe binary");
        return ExitCode::from(2);
    }
    let options = match parse_options(&args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("pty_handover_input_probe: {message}\n{USAGE}");
            return ExitCode::from(64);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("pty_handover_input_probe: tokio runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run(options)) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("pty_handover_input_probe: stdout: {error}");
            ExitCode::FAILURE
        }
    }
}
