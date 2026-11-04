use super::*;
use runtime::{supervise, Event};
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, watch};

fn manifest() -> Manifest {
    Manifest { name: "clock".into(), version: "1".into(),
        command: vec!["/nonexistent-ferryx-plugin-test".into()],
        capabilities: ["write".into()].into() }
}

#[test]
fn ungranted_write_is_forbidden() {
    let request = DaemonRequest::Write { session_id: "other".into(), data: b"x".to_vec() };
    let refusal = authorize(&manifest(), &request, &pane_grants::PaneGrants::default()).unwrap_err();
    assert_eq!(refusal.code, "FORBIDDEN");
    assert_eq!(refusal.reason, "PANE_GRANT_REQUIRED");
}

#[test]
fn capabilities_and_grants_are_both_required() {
    let grants = pane_grants::PaneGrants::default();
    grants.add(pane_grants::PaneGrant { from: "plugin:clock".into(), to: "other".into(),
        ops: [pane_grants::GrantOp::Write].into() }).unwrap();
    let request = DaemonRequest::Write { session_id: "other".into(), data: vec![] };
    assert!(authorize(&manifest(), &request, &grants).is_ok());
    let mut missing = manifest();
    missing.capabilities.clear();
    assert_eq!(authorize(&missing, &request, &grants).unwrap_err().code, "FORBIDDEN");
    assert!(authorize(&manifest(), &DaemonRequest::Shutdown, &grants).is_err());
}

#[test]
fn approvals_survive_reload_and_reject_changed_commands() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plugins.json");
    assert!(!Approvals::load(&path).unwrap().enabled(&manifest()));
    Approvals { manifests: vec![manifest()] }.save(&path).unwrap();
    let restored = Approvals::load(&path).unwrap();
    assert!(restored.enabled(&manifest()));
    let mut changed = manifest();
    changed.command.push("new-argument".into());
    assert!(!restored.enabled(&changed));
}

#[tokio::test]
async fn disabled_plugin_never_spawned() {
    let (_stop, stopped) = watch::channel(false);
    let (events, mut observed) = mpsc::unbounded_channel();
    supervise(manifest(), &Approvals::default(), Arc::new(pane_grants::PaneGrants::default()),
        stopped, events, |_| async { panic!("disabled plugin dispatched") }).await.unwrap();
    assert_eq!(observed.recv().await, None);
}

#[tokio::test(start_paused = true)]
async fn failed_launch_restarts_with_backoff_max_three() {
    let plugin = manifest();
    let approvals = Approvals { manifests: vec![plugin.clone()] };
    let (_stop, stopped) = watch::channel(false);
    let (events, mut observed) = mpsc::unbounded_channel();
    let started = tokio::time::Instant::now();
    let result = supervise(plugin, &approvals, Arc::new(pane_grants::PaneGrants::default()),
        stopped, events, |_| async { Ok(serde_json::Value::Null) }).await;
    assert!(result.is_err());
    for restart in 0..=3 { assert_eq!(observed.recv().await, Some(Event::Exited { restart })); }
    assert_eq!(observed.recv().await, None);
    assert_eq!(started.elapsed(), Duration::from_secs(7));
}

#[tokio::test]
async fn disable_reaps_running_plugin() {
    let mut plugin = manifest();
    #[cfg(unix)]
    { plugin.command = vec!["/bin/cat".into()]; }
    #[cfg(windows)]
    { plugin.command = vec![std::env::var("COMSPEC").unwrap(), "/c".into(), "set /p line=".into()]; }
    let approvals = Approvals { manifests: vec![plugin.clone()] };
    let (stop, stopped) = watch::channel(false);
    let (events, mut observed) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        supervise(plugin, &approvals, Arc::new(pane_grants::PaneGrants::default()),
            stopped, events, |_| async { Ok(serde_json::Value::Null) }).await
    });
    let event = tokio::time::timeout(Duration::from_secs(10), observed.recv()).await.unwrap();
    assert!(matches!(event, Some(Event::Started(pid)) if pid > 0));
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(10), task).await.unwrap().unwrap().unwrap();
    assert_eq!(observed.recv().await, None);
}

#[test]
fn plugin_content_round_trips_for_layout_recovery() {
    let content = crate::daemon::layout::Content::Plugin { plugin: "clock".into(), view: "main".into() };
    let bytes = serde_json::to_vec(&content).unwrap();
    assert_eq!(serde_json::from_slice::<crate::daemon::layout::Content>(&bytes).unwrap(), content);
}

#[test]
fn render_permission_is_identity_local_and_public_mutations_are_admin_gated() {
    let mut plugin = manifest();
    let request = DaemonRequest::PluginRender { view: "main".into(), html: "<time>12:00</time>".into() };
    let grants = pane_grants::PaneGrants::default();
    assert!(authorize(&plugin, &request, &grants).is_err());
    plugin.capabilities.insert("render".into());
    assert!(authorize(&plugin, &request, &grants).is_ok());
    for request in [DaemonRequest::PluginSetEnabled { name: "clock".into(), enabled: true },
        DaemonRequest::PluginAttach { plugin: "clock".into(), window: None }, request] {
        assert!(matches!(pane_grants::gate_for(&request), Some(pane_grants::Gate::Admin)));
    }
}

#[tokio::test]
async fn registry_restores_windows_idle_restart_consent_and_disable_persists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("approvals.json");
    Approvals { manifests: vec![manifest()] }.save(&path).unwrap();
    let registry = registry::Registry::new(path.clone());
    registry.restore().await.unwrap();
    assert!(registry.approvals.lock().enabled(&manifest()));
    registry.approve("clock".into(), false).await.unwrap();
    assert!(Approvals::load(&path).unwrap().manifests.is_empty());
}

#[test]
fn plugin_request_schema_uses_the_real_daemon_encoding() {
    let request: DaemonRequest = serde_json::from_value(serde_json::json!({
        "type":"pluginRender", "view":"main", "html":"<time>12:00</time>"
    })).unwrap();
    assert!(matches!(request, DaemonRequest::PluginRender { view, .. } if view == "main"));
    let catalog = crate::cli::api_catalog::describe_json("panes").unwrap();
    let requests = catalog["pluginRequests"]["requests"].as_array().unwrap();
    assert!(requests.iter().any(|r| r["type"] == "pluginRender" && r["capability"] == "render"));
}

#[tokio::test(start_paused = true)]
async fn crashed_process_restarts_at_most_three_times() {
    let mut plugin = manifest();
    #[cfg(unix)]
    { plugin.command = vec!["/bin/sh".into(), "-c".into(), "exit 1".into()]; }
    #[cfg(windows)]
    { plugin.command = vec![std::env::var("COMSPEC").unwrap(), "/c".into(), "exit 1".into()]; }
    let approvals = Approvals { manifests: vec![plugin.clone()] };
    let (_stop, stopped) = watch::channel(false);
    let (events, mut observed) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        supervise(plugin, &approvals, Arc::new(pane_grants::PaneGrants::default()),
            stopped, events, |_| async { Ok(serde_json::Value::Null) }).await
    });
    for restart in 0..=3 {
        assert!(matches!(observed.recv().await, Some(Event::Started(pid)) if pid > 0));
        assert_eq!(observed.recv().await, Some(Event::Exited { restart }));
    }
    assert!(task.await.unwrap().is_err());
    assert_eq!(observed.recv().await, None);
}
