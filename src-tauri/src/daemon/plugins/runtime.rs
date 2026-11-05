//! Async process supervision with bounded JSON-lines and cancellation-safe teardown.
use super::{authorize, Approvals, Manifest, MAX_LINE};
use crate::daemon::{pane_grants::PaneGrants, protocol::DaemonRequest};
use serde_json::{json, Value};
use std::{future::Future, process::Stdio, sync::Arc, time::Duration};
use tokio::{io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command, sync::{mpsc, watch}};

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Started(u32),
    Exited { restart: u8 },
}

/// Runs until disabled or until the initial process and three restart attempts have failed.
/// The adapter must serialize one supervisor per plugin and await shutdown before handover;
/// no plugin may be started by both predecessor and successor at once.
pub async fn supervise<F, Fut>(
    manifest: Manifest,
    approvals: &Approvals,
    grants: Arc<PaneGrants>,
    disabled: watch::Receiver<bool>,
    events: mpsc::UnboundedSender<Event>,
    dispatch: F,
) -> Result<(), String>
where F: FnMut(DaemonRequest) -> Fut, Fut: Future<Output = Result<Value, String>>,
{
    supervise_with_cleanup(manifest, approvals, grants, disabled, events, dispatch, || {}).await
}

pub async fn supervise_with_cleanup<F, Fut, C>(
    manifest: Manifest,
    approvals: &Approvals,
    grants: Arc<PaneGrants>,
    mut disabled: watch::Receiver<bool>,
    events: mpsc::UnboundedSender<Event>,
    mut dispatch: F,
    mut cleanup: C,
) -> Result<(), String>
where F: FnMut(DaemonRequest) -> Fut, Fut: Future<Output = Result<Value, String>>, C: FnMut(),
{
    if !approvals.enabled(&manifest) || *disabled.borrow() { return Ok(()); }
    for restart in 0..=3u8 {
        if restart > 0 {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(1 << (restart - 1))) => {},
                _ = disabled.changed() => return Ok(()),
            }
        }
        if *disabled.borrow() { return Ok(()); }
        let approved = manifest.clone();
        let bytes = tokio::task::spawn_blocking(move || approved.approved_bytes()).await
            .map_err(|e| e.to_string())??;
        if *disabled.borrow() { return Ok(()); }
        let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
        command.env_clear();
        #[cfg(not(test))]
        command.arg("--plugin-wasm-runner");
        #[cfg(test)]
        command.args(["--exact", "daemon::plugins::tests::wasm_runner_child", "--nocapture"])
            .env("FERRYX_WASM_RUNNER_TEST", "1");
        let result = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .kill_on_drop(true).spawn();
        let mut child = match result {
            Ok(child) => child,
            Err(error) => {
                tracing::warn!(plugin = %manifest.name, %error, restart, "plugin launch failed");
                if events.send(Event::Exited { restart }).is_err() { return Err("plugin observer closed".into()); }
                continue;
            }
        };
        let pid = child.id().ok_or("plugin pid unavailable")?;
        if events.send(Event::Started(pid)).is_err() { return Err("plugin observer closed".into()); }
        let serve = async {
            let mut input = child.stdin.take().ok_or("plugin stdin unavailable")?;
            input.write_all(&(bytes.len() as u32).to_le_bytes()).await.map_err(|e| e.to_string())?;
            input.write_all(&bytes).await.map_err(|e| e.to_string())?;
            input.flush().await.map_err(|e| e.to_string())?;
            let output = child.stdout.take().ok_or("plugin stdout unavailable")?;
            let mut output = BufReader::new(output);
            #[cfg(test)]
            loop {
                let mut line = String::new();
                output.read_line(&mut line).await.map_err(|e| e.to_string())?;
                if line.trim_end().ends_with("FERRYX_WASM_RUNNER_READY") { break; }
                if line.is_empty() { return Err("test runner closed before ready".into()); }
            }
            loop {
                let mut line = Vec::new();
                let count = (&mut output).take((MAX_LINE + 1) as u64)
                    .read_until(b'\n', &mut line).await.map_err(|e| e.to_string())?;
                if count == 0 { return Ok::<(), String>(()); }
                if count > MAX_LINE || line.last() != Some(&b'\n') {
                    return Err("plugin JSON line exceeds limit or lacks newline".into());
                }
                let response = match serde_json::from_slice::<DaemonRequest>(&line) {
                    Ok(request) => match authorize(&manifest, &request, &grants) {
                        Ok(()) => match tokio::time::timeout(Duration::from_secs(5), dispatch(request)).await {
                            Ok(Ok(value)) => value,
                            Ok(Err(message)) => json!({"code":"PLUGIN_REQUEST_FAILED", "message":message}),
                            Err(_) => return Err("plugin host operation timed out".into()),
                        },
                        Err(error) => serde_json::to_value(error).map_err(|e| e.to_string())?,
                    },
                    Err(error) => json!({"code":"INVALID_REQUEST", "message":error.to_string()}),
                };
                let mut bytes = serde_json::to_vec(&response).map_err(|e| e.to_string())?;
                if bytes.len() > MAX_LINE { return Err("plugin response exceeds limit".into()); }
                bytes.push(b'\n');
                input.write_all(&bytes).await.map_err(|e| e.to_string())?;
                input.flush().await.map_err(|e| e.to_string())?;
            }
        };
        let stopped = tokio::select! {
            result = serve => {
                if let Err(error) = result { tracing::warn!(plugin = %manifest.name, %error, "plugin stdio failed"); }
                false
            },
            _ = disabled.changed() => true,
            _ = tokio::time::sleep(Duration::from_secs(3605)) => false,
        };
        // Also kills a process that closes stdout but does not exit. Await reaping on disable.
        if child.try_wait().map_err(|e| e.to_string())?.is_none() {
            child.kill().await.map_err(|e| e.to_string())?;
        }
        child.wait().await.map_err(|e| e.to_string())?;
        cleanup();
        if stopped { return Ok(()); }
        if events.send(Event::Exited { restart }).is_err() { return Err("plugin observer closed".into()); }
    }
    Err("plugin restart limit reached (3)".into())
}
