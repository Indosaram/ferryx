use super::*;
use runtime::{supervise, Event};
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, watch};

#[test]
fn wasm_runner_child() {
    if std::env::var_os("FERRYX_WASM_RUNNER_TEST").is_some() {
        println!("FERRYX_WASM_RUNNER_READY");
        std::process::exit(runner::run_stdio());
    }
}

fn manifest() -> Manifest {
    Manifest { name: "clock".into(), version: "1".into(),
        module: std::env::temp_dir().join("nonexistent-ferryx-plugin-test.wasm"), sha256: "0".repeat(64),
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
    grants.add(pane_grants::PaneGrant { from: "plugin:clock".into(), to: "*".into(),
        ops: [pane_grants::GrantOp::Admin].into() }).unwrap();
    for forbidden in [DaemonRequest::Shutdown, DaemonRequest::AbortHandover,
        DaemonRequest::PaneGrant { request: pane_grants::PaneGrantAction::Add {
            grant: pane_grants::PaneGrant { from: "plugin:clock".into(), to: "*".into(),
                ops: [pane_grants::GrantOp::Admin].into() } } }] {
        assert!(authorize(&manifest(), &forbidden, &grants).is_err());
    }
    grants.remove("plugin:clock", "*").unwrap();
    grants.remove("plugin:clock", "other").unwrap();
    assert!(authorize(&manifest(), &request, &grants).is_err());
}

#[test]
fn native_manifests_and_legacy_approvals_never_migrate_implicitly() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("clock")).unwrap();
    std::fs::write(dir.path().join("clock/plugin.toml"),
        "name='clock'\nversion='1'\ncommand=['/bin/sh']\ncapabilities=['render']").unwrap();
    assert!(Manifest::load(dir.path(), "clock").unwrap_err().starts_with("PLUGIN_NATIVE_UNSUPPORTED"));
    let path = dir.path().join("approvals.json");
    std::fs::write(&path, br#"{"manifests":[{"name":"clock","version":"1","command":["/bin/sh"],"capabilities":["render"]}]}"#).unwrap();
    assert!(Approvals::load(&path).unwrap().manifests.is_empty());
    assert!(Approvals::decode(&std::fs::read(&path).unwrap()).unwrap().manifests.is_empty());
}

#[test]
fn changed_bytes_require_fresh_content_approval_even_at_same_path() {
    use sha2::{Digest, Sha256};
    let dir = tempfile::tempdir().unwrap();
    let mut plugin = manifest();
    plugin.module = dir.path().join("module.wasm");
    let original = b"\0asm\x01\0\0\0";
    plugin.sha256 = format!("{:x}", Sha256::digest(original));
    std::fs::write(&plugin.module, original).unwrap();
    let approval = Approvals { manifests: vec![plugin.clone()] };
    assert_eq!(plugin.approved_bytes().unwrap(), original);
    std::fs::write(&plugin.module, b"\0asm\x01\0\0\0changed").unwrap();
    assert!(plugin.approved_bytes().is_err());
    plugin.sha256 = "1".repeat(64);
    assert!(!approval.enabled(&plugin));
}

#[tokio::test]
async fn wasm_render_bridge_dispatches_only_approved_operation_and_disable_reaps() {
    use sha2::{Digest, Sha256};
    let dir = tempfile::tempdir().unwrap();
    let request = r#"{"type":"pluginRender","view":"main","html":"<time>12:00</time>"}"#;
    let encoded: String = request.bytes().map(|byte| format!("\\{byte:02x}")).collect();
    let bytes = wat::parse_str(format!(r#"(module
        (import "ferryx" "request" (func $request (param i32 i32 i32 i32) (result i32)))
        (memory (export "memory") 2)
        (data (i32.const 0) "{encoded}")
        (func (export "tick") (param i64)
            i32.const 0 i32.const {} i32.const 65536 i32.const 4096 call $request drop))"#, request.len())).unwrap();
    let mut plugin = manifest();
    plugin.capabilities = ["render".into()].into();
    plugin.module = dir.path().join("render.wasm");
    plugin.sha256 = format!("{:x}", Sha256::digest(&bytes));
    std::fs::write(&plugin.module, &bytes).unwrap();
    let approvals = Approvals { manifests: vec![plugin.clone()] };
    let (stop, stopped) = watch::channel(false);
    let (events, mut observed) = mpsc::unbounded_channel();
    let (rendered, mut rendering) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        supervise(plugin, &approvals, Arc::new(pane_grants::PaneGrants::default()),
            stopped, events, move |request| {
                rendered.send(request).unwrap();
                async { Ok(serde_json::json!({"type":"renderOk"})) }
            }).await
    });
    tokio::time::timeout(Duration::from_secs(20), async {
        assert!(matches!(observed.recv().await, Some(Event::Started(_))));
        assert!(matches!(rendering.recv().await, Some(DaemonRequest::PluginRender { view, html })
            if view == "main" && html == "<time>12:00</time>"));
        stop.send(true).unwrap();
        task.await.unwrap().unwrap();
    }).await.expect("render and teardown complete");
    assert_eq!(observed.recv().await, None);
}

#[test]
fn approvals_survive_reload_and_reject_changed_module_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plugins.json");
    assert!(!Approvals::load(&path).unwrap().enabled(&manifest()));
    Approvals { manifests: vec![manifest()] }.save(&path).unwrap();
    let restored = Approvals::load(&path).unwrap();
    assert!(restored.enabled(&manifest()));
    let mut changed = manifest();
    changed.sha256 = "1".repeat(64);
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

#[tokio::test]
async fn missing_module_never_starts_runner() {
    let plugin = manifest();
    let approvals = Approvals { manifests: vec![plugin.clone()] };
    let (_stop, stopped) = watch::channel(false);
    let (events, mut observed) = mpsc::unbounded_channel();
    let result = supervise(plugin, &approvals, Arc::new(pane_grants::PaneGrants::default()),
        stopped, events, |_| async { Ok(serde_json::Value::Null) }).await;
    assert!(result.is_err());
    assert_eq!(observed.recv().await, None);
}

#[tokio::test]
async fn disable_reaps_running_plugin() {
    let mut plugin = manifest();
    let dir = tempfile::tempdir().unwrap();
    let bytes = wat::parse_str("(module (func (export \"tick\") (param i64)))").unwrap();
    std::fs::write(dir.path().join("module.wasm"), &bytes).unwrap();
    plugin.module = dir.path().join("module.wasm");
    use sha2::{Digest, Sha256};
    plugin.sha256 = format!("{:x}", Sha256::digest(&bytes));
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
    let root = tempfile::tempdir().unwrap();
    let mut store = crate::daemon::layout::Store::new(root.path().into());
    store.apply("main", 0, crate::daemon::layout::Intent::AddTabWithSession {
        tab_id: "clock-block".into(), label: "clock".into(), content: content.clone(), activate: true,
    }).unwrap();
    let restored = crate::daemon::layout::Store::new(root.path().into()).get("main").unwrap();
    assert!(matches!(&restored.layout.tabs[0].pane_tree,
        crate::daemon::layout::Node::Leaf { content: saved, .. } if saved == &content));
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

#[tokio::test]
async fn crashed_process_restarts_at_most_three_times() {
    let mut plugin = manifest();
    let dir = tempfile::tempdir().unwrap();
    let bytes = b"\0asm\x01\0\0\0";
    std::fs::write(dir.path().join("module.wasm"), bytes).unwrap();
    plugin.module = dir.path().join("module.wasm");
    use sha2::{Digest, Sha256};
    plugin.sha256 = format!("{:x}", Sha256::digest(bytes));
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
