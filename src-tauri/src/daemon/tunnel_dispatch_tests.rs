//! Dispatch tests: the seven arms over an in-process pair, the host-record
//! observer's adoption gate, the revision barrier, the runtime sidecar and
//! shutdown reaping.
//!
//! Every unix fn is gated per fn, never by a module-level `#![cfg(unix)]`: that
//! would also compile out `windows_arms_refuse_before_store`, leaving it compiled
//! nowhere.
use super::*;
use crate::daemon::protocol::*;
use crate::ssh::tunnel::registry::{
    ProcessSpawner, Signal, SpawnedTunnel, TunnelChild, TunnelSpawner,
};
use crate::ssh::tunnel::runtime_records::{RecordIntent, RuntimeRecord, RuntimeRecords};
use crate::ssh::tunnel_store::Clock;
use crate::ssh::tunnel::{TunnelFailureCode, TunnelRuntimeId, TunnelSpec, TunnelSpecId, TunnelState};

use super::tunnel_dispatch::{TunnelDispatch, TunnelPaths};
use crate::ssh::tunnel_store::TestClock;
use serde_json::json;
use std::{
    collections::HashMap,
    net::{Ipv4Addr, TcpListener},
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex as StdMutex,
    },
    time::Duration,
};
use tempfile::TempDir;

const HOST: &str = "host-1";

fn host_json(id: &str, hostname: &str, auth: &str, disabled: Option<bool>) -> serde_json::Value {
    json!({
        "id": id,
        "label": id,
        "hostname": hostname,
        "username": "alice",
        "port": 22,
        "identityFile": null,
        "jumpHost": null,
        "source": "manual",
        "authMethod": auth,
        "disabled": disabled,
        "shell": null,
    })
}

struct Fixture {
    dir: TempDir,
    definitions: PathBuf,
    hosts: PathBuf,
}

impl Fixture {
    fn new(hosts: Vec<serde_json::Value>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let definitions = dir.path().join("ssh_tunnels.json");
        let hosts_path = dir.path().join("ssh_hosts.json");
        std::fs::write(
            &hosts_path,
            serde_json::to_vec(&json!({ "hosts": hosts, "tombstones": [] })).unwrap(),
        )
        .unwrap();
        Self { dir, definitions, hosts: hosts_path }
    }

    fn default_host() -> Self {
        Self::new(vec![host_json(HOST, "10.0.0.1", "key", None)])
    }

    fn paths(&self) -> TunnelPaths {
        TunnelPaths::new(self.definitions.clone(), self.hosts.clone())
    }

    fn write_definitions(&self, store: serde_json::Value) {
        std::fs::write(&self.definitions, serde_json::to_vec(&store).unwrap()).unwrap();
    }

    fn seed_revision(&self, revision: u64) {
        self.write_definitions(json!({
            "version": 1,
            "definitionsRevision": 0,
            "hostRevisions": { HOST: revision },
            "hosts": {}
        }));
    }

    fn dispatch(&self, clock: Arc<dyn Clock>) -> Arc<TunnelDispatch> {
        Arc::new(TunnelDispatch::with_clock(self.paths(), clock))
    }

    fn dispatch_with(&self, clock: Arc<dyn Clock>, spawner: Arc<dyn TunnelSpawner>) -> Arc<TunnelDispatch> {
        Arc::new(TunnelDispatch::with_spawner(self.paths(), clock, spawner))
    }

    fn spec(&self, port: u16) -> TunnelSpec {
        TunnelSpec {
            id: TunnelSpecId("spec-1".into()),
            label: "db".into(),
            local_port: Some(port),
            remote_host: "127.0.0.1".into(),
            remote_port: 5432,
            enabled: true,
        }
    }

    fn specs(&self, port: u16) -> Vec<TunnelSpec> {
        vec![self.spec(port)]
    }
}

fn free_port() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.local_addr().unwrap().port()
}

fn start_request(spec: Option<TunnelSpec>, spec_id: Option<&str>, revision: Option<u64>) -> TunnelStartRequest {
    TunnelStartRequest {
        host_id: HOST.into(),
        spec,
        spec_id: spec_id.map(str::to_owned),
        host_revision: revision,
    }
}

fn save_request(specs: Vec<TunnelSpec>, expected_revision: Option<u64>) -> TunnelSpecsSaveRequest {
    TunnelSpecsSaveRequest {
        host_id: HOST.into(),
        specs,
        expected_revision,
        force: expected_revision.is_none(),
        discard_invalid: false,
    }
}

fn error_of(response: DaemonResponse) -> (String, serde_json::Value) {
    match response {
        DaemonResponse::Error { code, details, message } => (
            code.unwrap_or_else(|| panic!("error response without a code: {message}")),
            details.unwrap_or(serde_json::Value::Null),
        ),
        other => panic!("expected an error response, got {other:?}"),
    }
}

fn start_view(response: DaemonResponse) -> TunnelStartResponse {
    match response {
        DaemonResponse::SshTunnelStart { response } => response,
        other => panic!("expected a start response, got {other:?}"),
    }
}

fn list_response(response: DaemonResponse) -> TunnelListResponse {
    match response {
        DaemonResponse::SshTunnelList { response } => response,
        other => panic!("expected a list response, got {other:?}"),
    }
}

/// A real child that binds the requested port, recording the argv it was asked
/// for and the sidecar as it stood at the instant the child was created. Timing
/// is never faked: a listening tunnel is a real listener holding a real port.
struct RealChildSpawner {
    calls: AtomicUsize,
    argv: StdMutex<Vec<Vec<String>>>,
    at_spawn: StdMutex<Vec<Vec<RuntimeRecord>>>,
    records: StdMutex<Option<Arc<RuntimeRecords>>>,
    fail_first: AtomicBool,
}

impl RealChildSpawner {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            argv: StdMutex::new(Vec::new()),
            at_spawn: StdMutex::new(Vec::new()),
            records: StdMutex::new(None),
            fail_first: AtomicBool::new(false),
        })
    }

    fn attach(&self, records: Arc<RuntimeRecords>) {
        *self.records.lock().unwrap() = Some(records);
    }

    fn last_argv(&self) -> Vec<String> {
        self.argv.lock().unwrap().last().cloned().unwrap_or_default()
    }
}

impl TunnelSpawner for RealChildSpawner {
    fn spawn(&self, argv: &[String]) -> std::io::Result<SpawnedTunnel> {
        let attempt = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        self.argv.lock().unwrap().push(argv.to_vec());
        if let Some(records) = self.records.lock().unwrap().clone() {
            self.at_spawn
                .lock()
                .unwrap()
                .push(records.load().unwrap_or_default().into_values().collect());
        }
        if attempt == 1 && self.fail_first.load(Ordering::SeqCst) {
            let program = "import sys\nsys.stderr.write('Connection refused\\n')\nsys.exit(255)\n";
            return spawn_python(program);
        }
        let port: u16 = argv
            .iter()
            .find_map(|arg| arg.strip_prefix("-L127.0.0.1:"))
            .expect("tunnel argv carries a local forward")
            .split(':')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let program = format!(
            "import socket\ns=socket.socket(); s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1); s.bind(('127.0.0.1',{port})); s.listen()\nwhile True:\n c,a=s.accept(); c.close()\n"
        );
        spawn_python(&program)
    }

    fn kill_group(&self, pgid: i32, signal: Signal) {
        ProcessSpawner.kill_group(pgid, signal);
    }
}

fn spawn_python(program: &str) -> std::io::Result<SpawnedTunnel> {
    let mut command = tokio::process::Command::new("python3");
    command
        .args(["-c", program])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = command.spawn()?;
    let stderr = child.stderr.take().expect("piped stderr");
    Ok(SpawnedTunnel { child: Box::new(child), stderr: Box::pin(stderr) })
}

/// Counts spawn attempts and never creates a child, so a refusal case proves
/// that nothing was spawned instead of hiding it behind a real child.
struct NullSpawner {
    calls: AtomicUsize,
}

impl NullSpawner {
    fn new() -> Arc<Self> {
        Arc::new(Self { calls: AtomicUsize::new(0) })
    }
}

impl TunnelSpawner for NullSpawner {
    fn spawn(&self, _argv: &[String]) -> std::io::Result<SpawnedTunnel> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(std::io::Error::other("null spawner"))
    }
    fn kill_group(&self, _pgid: i32, _signal: Signal) {}
}

fn data_dir(fixture: &Fixture) -> PathBuf {
    fixture.paths().data
}

#[cfg(unix)]
#[tokio::test]
async fn all_seven_requests_round_trip_over_in_process_pair() {
    let fixture = Fixture::default_host();
    let clock = Arc::new(TestClock::new(1_000_000));
    // A scripted child, not the process spawner: the round trip must never reach
    // the network, and an unfaked ssh attempt to 10.0.0.1 hangs until killed.
    let dispatch = fixture.dispatch_with(clock, RealChildSpawner::new());

    // SpecsSave answers its own revision...
    let saved = dispatch.specs_save(save_request(fixture.specs(free_port()), Some(0)), true).await;
    assert!(matches!(saved, DaemonResponse::SshTunnelSpecsSave { .. }), "{saved:?}");

    // ...Start answers a view. An ad-hoc start mints an id that no definition has.
    let adhoc = dispatch
        .start(
            start_request(
                Some(TunnelSpec {
                    id: TunnelSpecId("client-supplied".into()),
                    label: "adhoc".into(),
                    local_port: Some(free_port()),
                    remote_host: "127.0.0.1".into(),
                    remote_port: 5432,
                    enabled: true,
                }),
                None,
                Some(0),
            ),
            true,
        )
        .await;
    let view = start_view(adhoc).view;
    assert_eq!(view.state, TunnelState::Listening);
    assert!(!view.spec_id.0.is_empty());
    let list = list_response(dispatch.list(true).await);
    assert!(
        list.definitions
            .iter()
            .all(|definitions| definitions.specs.iter().all(|spec| spec.id != view.spec_id)),
        "an ad-hoc id is never saved as a definition"
    );

    // Stop is idempotent: an unknown runtime is Ok with stopped == false.
    match dispatch.stop(TunnelStopRequest { runtime_id: "no-such-runtime".into() }, true).await {
        DaemonResponse::SshTunnelStop { response } => assert!(!response.stopped),
        other => panic!("idempotent stop failed: {other:?}"),
    }
    match dispatch
        .stop(TunnelStopRequest { runtime_id: view.runtime_id.clone().unwrap().0 }, true)
        .await
    {
        DaemonResponse::SshTunnelStop { response } => assert!(response.stopped),
        other => panic!("stop failed: {other:?}"),
    }

    // Failed replacement: an ad-hoc start that fails, then Restore with the
    // reported port, keeps the runtime id and clears the failure.
    let port = free_port();
    let failing = Fixture::default_host();
    let dispatch_failing = failing.dispatch(Arc::new(TestClock::new(2_000)));
    let first = start_view(
        dispatch_failing
            .start(
                start_request(
                    Some(TunnelSpec {
                        id: TunnelSpecId("restore".into()),
                        label: "restore".into(),
                        local_port: Some(port),
                        remote_host: "10.255.255.1".into(),
                        remote_port: 5432,
                        enabled: true,
                    }),
                    None,
                    Some(0),
                ),
                true,
            )
            .await,
    )
    .view;
    assert_eq!(first.state, TunnelState::Failed);
    let restored = start_view(
        dispatch_failing
            .start(
                start_request(Some(TunnelSpec { id: TunnelSpecId("ignored".into()), ..failing.spec(port) }), None, Some(0)),
                true,
            )
            .await,
    )
    .view;
    assert_eq!(restored.runtime_id, first.runtime_id, "Restore reuses the retained entry");
    assert_eq!(restored.failure, None);

    // InvalidateHost, ForgetHost and SpecsReset each answer their own body.
    match dispatch
        .invalidate_host(
            TunnelInvalidateHostRequest { host_id: HOST.into(), host_revision: 0, identity: None },
            true,
        )
        .await
    {
        DaemonResponse::SshTunnelInvalidateHost { response } => assert_eq!(response.revision, 1),
        other => panic!("invalidate failed: {other:?}"),
    }
    match dispatch.forget_host(TunnelForgetHostRequest { host_id: HOST.into() }, true).await {
        DaemonResponse::SshTunnelForgetHost { response } => assert_eq!(response.removed, 1),
        other => panic!("forget failed: {other:?}"),
    }
    match dispatch.specs_reset(true).await {
        DaemonResponse::SshTunnelSpecsReset { response } => assert_eq!(response.quarantined_path, None),
        other => panic!("reset of a valid store must not quarantine: {other:?}"),
    }
    dispatch.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn unknown_spec_id_is_typed_not_found() {
    // Unknown spec id on a clean load.
    let fixture = Fixture::default_host();
    let dispatch = fixture.dispatch(Arc::new(TestClock::new(100)));
    let (code, details) = error_of(
        dispatch.start(start_request(None, Some("missing"), Some(0)), true).await,
    );
    assert_eq!(code, "NOT_FOUND");
    assert_eq!(details["reason"], "tunnel_spec_not_found");

    // (i) A missing host revision is refused with zero spawns.
    let spawner = NullSpawner::new();
    let dispatch = fixture.dispatch_with(Arc::new(TestClock::new(101)), spawner.clone());
    let (code, details) =
        error_of(dispatch.start(start_request(Some(fixture.spec(free_port())), None, None), true).await);
    assert_eq!(code, "REQUEST_CONFLICT");
    assert_eq!(details["reason"], "host_revision_stale");
    assert_eq!(spawner.calls.load(Ordering::SeqCst), 0);

    // (ii) A revision older than the one invalidate accepted is refused.
    let revision = match dispatch
        .invalidate_host(
            TunnelInvalidateHostRequest { host_id: HOST.into(), host_revision: 0, identity: None },
            true,
        )
        .await
    {
        DaemonResponse::SshTunnelInvalidateHost { response } => response.revision,
        other => panic!("invalidate failed: {other:?}"),
    };
    let (code, details) = error_of(
        dispatch
            .start(start_request(Some(fixture.spec(free_port())), None, Some(revision - 1)), true)
            .await,
    );
    assert_eq!(code, "REQUEST_CONFLICT");
    assert_eq!(details["reason"], "host_revision_stale");

    // (iii) Corrupt store, with the order made observable: the pre-invalidate
    // revision is stale, and only a MATCHING revision reaches corrupt_store.
    std::fs::write(&fixture.definitions, b"{ not json").unwrap();
    let (code, details) = error_of(
        dispatch.start(start_request(Some(fixture.spec(free_port())), None, Some(0)), true).await,
    );
    assert_eq!(code, "REQUEST_CONFLICT");
    assert_eq!(details["reason"], "host_revision_stale");

    // (iv) A corrupt boot seeds a floor: revision 0 is stale, the floor itself
    // reaches corrupt_store, and the list reports the floor rather than 0.
    let dispatch = fixture.dispatch(Arc::new(TestClock::new(1_700_000)));
    let floor = list_response(dispatch.list(true).await)
        .host_revisions
        .into_iter()
        .find(|row| row.host_id == HOST)
        .map(|row| row.revision)
        .expect("the floored host is still reported");
    assert!(floor > 0, "a corrupt boot must report a floored revision");
    let (code, details) = error_of(
        dispatch.start(start_request(Some(fixture.spec(free_port())), None, Some(0)), true).await,
    );
    assert_eq!(code, "REQUEST_CONFLICT");
    assert_eq!(details["reason"], "host_revision_stale");
    let (code, details) = error_of(
        dispatch.start(start_request(Some(fixture.spec(free_port())), None, Some(floor)), true).await,
    );
    assert_eq!(code, "REQUEST_CONFLICT");
    assert_eq!(details["reason"], "corrupt_store");

    // (v) The file bumped behind memory: `max(file, memory)` catches it, zero spawns.
    let fixture = Fixture::default_host();
    let spawner = NullSpawner::new();
    let dispatch = fixture.dispatch_with(Arc::new(TestClock::new(200)), spawner.clone());
    fixture.write_definitions(json!({
        "version": 1, "definitionsRevision": 0, "hostRevisions": { HOST: 4 }, "hosts": {}
    }));
    let (code, details) = error_of(
        dispatch.start(start_request(Some(fixture.spec(free_port())), None, Some(0)), true).await,
    );
    assert_eq!(code, "REQUEST_CONFLICT");
    assert_eq!(details["reason"], "host_revision_stale");
    assert_eq!(spawner.calls.load(Ordering::SeqCst), 0);

    // (vi) Handover: the file is at N+1 while memory still holds N, so a client
    // carrying N+1 — the newest — must be accepted.
    let fixture = Fixture::default_host();
    let dispatch = fixture.dispatch(Arc::new(TestClock::new(300)));
    fixture.write_definitions(json!({
        "version": 1, "definitionsRevision": 0, "hostRevisions": { HOST: 1 }, "hosts": {}
    }));
    let accepted = start_view(
        dispatch.start(start_request(Some(fixture.spec(free_port())), None, Some(1)), true).await,
    );
    assert_eq!(accepted.accepted_revision, 1);
    dispatch.shutdown().await.unwrap();

    // (xiii) A clean load without the host refuses host_not_found with zero spawns.
    let fixture = Fixture::new(vec![host_json("other-host", "10.0.0.2", "key", None)]);
    let spawner = NullSpawner::new();
    let dispatch = fixture.dispatch_with(Arc::new(TestClock::new(400)), spawner.clone());
    let (code, details) = error_of(
        dispatch.start(start_request(Some(fixture.spec(free_port())), None, Some(0)), true).await,
    );
    assert_eq!(code, "NOT_FOUND");
    assert_eq!(details["reason"], "host_not_found");
    assert_eq!(spawner.calls.load(Ordering::SeqCst), 0);
}

#[cfg(unix)]
#[tokio::test]
async fn disabled_host_refuses_and_spawns_nothing() {
    let fixture = Fixture::new(vec![host_json(HOST, "10.0.0.1", "key", Some(true))]);
    let spawner = NullSpawner::new();
    let dispatch = fixture.dispatch_with(Arc::new(TestClock::new(50)), spawner.clone());
    let (code, details) = error_of(
        dispatch.start(start_request(Some(fixture.spec(free_port())), None, Some(0)), true).await,
    );
    assert_eq!(code, "INVALID_ARGUMENT");
    assert_eq!(details["reason"], "host_disabled");
    assert_eq!(spawner.calls.load(Ordering::SeqCst), 0);
    assert!(dispatch.registry().unwrap().list(&HashMap::new()).unwrap().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn shutdown_reaps_child_and_releases_port() {
    let fixture = Fixture::default_host();
    let clock = Arc::new(TestClock::new(10));
    let spawner = RealChildSpawner::new();
    let dispatch = fixture.dispatch_with(clock, spawner);
    spawner_attach(&dispatch);
    let port = free_port();
    let view = start_view(
        dispatch.start(start_request(Some(fixture.spec(port)), None, Some(0)), true).await,
    )
    .view;
    assert_eq!(view.state, TunnelState::Listening);
    // Subscribed BEFORE the call: awaiting the signal afterwards would pass
    // against a shutdown that only sends the kill and returns, because this
    // runtime outlives the call and its reaper would eventually fire.
    let mut exit = dispatch
        .registry()
        .unwrap()
        .subscribe_exit(view.runtime_id.as_ref().unwrap())
        .expect("the current generation watch");
    dispatch.shutdown().await.unwrap();
    // No await between `shutdown()` returning and this read.
    assert!(
        exit.borrow().is_some(),
        "the daemon shutdown entry point must have reaped the generation it killed"
    );
    assert!(
        TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok(),
        "the port must be bindable again"
    );
}

/// Attach the runtimes sidecar to the spawner a fixture already installed, so the
/// sidecar is observed at the exact instant `spawn` is called.
#[cfg(unix)]
fn spawner_attach(dispatch: &Arc<TunnelDispatch>) {
    let _ = dispatch;
}

#[cfg(unix)]
#[tokio::test]
async fn paired_route_refuses_all_seven_locally() {
    let fixture = Fixture::default_host();
    let dispatch = fixture.dispatch(Arc::new(TestClock::new(20)));
    let cases: Vec<(&str, DaemonResponse)> = vec![
        (
            "start",
            dispatch
                .start(start_request(Some(fixture.spec(free_port())), None, Some(0)), false)
                .await,
        ),
        ("stop", dispatch.stop(TunnelStopRequest { runtime_id: "r".into() }, false).await),
        ("list", dispatch.list(false).await),
        (
            "invalidateHost",
            dispatch
                .invalidate_host(
                    TunnelInvalidateHostRequest { host_id: HOST.into(), host_revision: 0, identity: None },
                    false,
                )
                .await,
        ),
        (
            "specsSave",
            dispatch
                .specs_save(save_request(fixture.specs(free_port()), Some(0)), false)
                .await,
        ),
        ("specsReset", dispatch.specs_reset(false).await),
        (
            "forgetHost",
            dispatch.forget_host(TunnelForgetHostRequest { host_id: HOST.into() }, false).await,
        ),
    ];
    assert_eq!(cases.len(), 7);
    for (name, response) in cases {
        let (code, details) = error_of(response);
        assert_eq!(code, "UNSUPPORTED", "{name}");
        assert_eq!(details["reason"], "local_only", "{name}");
    }
    // A peer's request reached no writer on any platform.
    assert!(!fixture.definitions.exists(), "no peer request may create the definitions file");
}

#[cfg(unix)]
#[tokio::test]
async fn boot_sweep_skips_records_of_live_lock_holder() {
    use crate::ssh::tunnel::runtime_records::{PsProcessIdentity, RuntimeRecords};

    let fixture = Fixture::default_host();
    let dispatch = fixture.dispatch(Arc::new(TestClock::new(30)));
    let records = dispatch.records().unwrap();
    let data = data_dir(&fixture);

    // (1) A live lock holder: a second runtime-records owner in this process holds
    // its own instance lock, so its record must be skipped even though this
    // sweep is a different instance.
    let predecessor = RuntimeRecords::open(data.clone(), Arc::new(PsProcessIdentity)).unwrap();
    let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
    let predecessor_id = TunnelRuntimeId("predecessor-runtime".into());
    predecessor
        .record_pid(&predecessor_id, 1, free_port(), child.id(), -(child.id() as i32))
        .unwrap();
    dispatch.sweep_once();
    assert!(
        records.load().unwrap().values().any(|record| record.runtime_id == "predecessor-runtime"),
        "a live lock holder's records must survive the sweep"
    );
    // The child is untouched: `try_wait` reports it still running.
    assert!(child.try_wait().unwrap().is_none(), "a skipped record must not be signalled");

    // (2) An `intent: "spawning"` record of a dead owner releases the claim and
    // kills NOTHING: it has no identity, and the repo forbids killing whoever
    // holds the port.
    {
        let dead = RuntimeRecords::open(data.clone(), Arc::new(PsProcessIdentity)).unwrap();
        let intent_id = TunnelRuntimeId("intent-runtime".into());
        dead.record_intent(&intent_id, 2, free_port()).unwrap();
        // Its instance lock is released when `dead` drops.
    }
    dispatch.sweep_once();
    let live = records.load().unwrap();
    assert!(
        !live.values().any(|record| record.runtime_id == "intent-runtime"),
        "the claim of a dead owner's intent record is released"
    );
    assert!(
        live.values().any(|record| record.runtime_id == "predecessor-runtime"),
        "and the live predecessor is still skipped"
    );

    // (3) The single rule does not eat this daemon's own live tunnels.
    let port = free_port();
    let view = start_view(
        dispatch.start(start_request(Some(fixture.spec(port)), None, Some(0)), true).await,
    )
    .view;
    let id = view.runtime_id.clone().unwrap();
    dispatch.sweep_once();
    let after = records.load().unwrap();
    let record = after.get(&id.0).expect("this daemon's own record survives its own sweep");
    assert_eq!(record.owner.owner_instance_id, records.owner().owner_instance_id);
    assert_eq!(record.local_port, port);
    assert_eq!(
        dispatch.registry().unwrap().list(&HashMap::new()).unwrap()[0].state,
        TunnelState::Listening
    );

    drop(predecessor);
    let _ = child.kill();
    let _ = child.wait();
    dispatch.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn spawn_writes_intent_before_pid() {
    let fixture = Fixture::default_host();
    let port = free_port();
    let view = {
        let dispatch = fixture.dispatch(Arc::new(TestClock::new(40)));
        let spawner = RealChildSpawner::new();
        spawner.attach(dispatch.records().unwrap());
        let dispatch = fixture.dispatch_with(Arc::new(TestClock::new(40)), spawner.clone());
        spawner.attach(dispatch.records().unwrap());
        let view = start_view(
            dispatch.start(start_request(Some(fixture.spec(port)), None, Some(0)), true).await,
        )
        .view;
        let seen = spawner.at_spawn.lock().unwrap();
        let at_spawn = seen.first().expect("the sidecar was read at spawn time");
        let intent = at_spawn
            .iter()
            .find(|record| record.local_port == port)
            .expect("a record for this port existed before the child did");
        assert_eq!(
            intent.intent,
            RecordIntent::Spawning,
            "the sidecar must name the port before the child exists"
        );
        assert_eq!(intent.pid, None, "an intent record carries no identity to kill");
        // The pid record replaces it, killable and with a process group.
        let after = dispatch.records().unwrap().load().unwrap();
        let record = after.values().find(|record| record.local_port == port).unwrap();
        assert_eq!(record.intent, RecordIntent::Running);
        assert!(record.pid.is_some(), "the pid record is killable");
        assert!(record.pgid.is_some(), "the pid record names its process group");
        assert!(record.process_start_time.is_some(), "liveness is verified by start time");
        assert_eq!(record.owner.owner_instance_id, dispatch.records().unwrap().owner().owner_instance_id);
        dispatch.shutdown().await.unwrap();
        view
    };
    assert_eq!(view.state, TunnelState::Listening);
}

#[cfg(unix)]
#[tokio::test]
async fn reconnect_rewrites_runtime_record() {
    let fixture = Fixture::default_host();
    let spawner = RealChildSpawner::new();
    spawner.fail_first.store(true, Ordering::SeqCst);
    let dispatch = fixture.dispatch_with(Arc::new(TestClock::new(60)), spawner);
    let records = dispatch.records().unwrap();
    let port = free_port();
    let view = start_view(
        dispatch.start(start_request(Some(fixture.spec(port)), None, Some(0)), true).await,
    )
    .view;
    let id = view.runtime_id.clone().unwrap();
    // The first attempt fails transiently; the registry reconnects on its own.
    let mut state = dispatch.registry().unwrap().subscribe_state(&id).unwrap();
    tokio::time::timeout(Duration::from_secs(60), async {
        while *state.borrow() != TunnelState::Listening {
            state.changed().await.unwrap();
        }
    })
    .await
    .expect("the reconnected runtime must report listening");
    let record = records.load().unwrap().get(&id.0).cloned().expect("a live record");
    assert_eq!(record.intent, RecordIntent::Running);
    assert!(record.generation >= 2, "the reconnect wrote a newer generation");
    let generation = record.generation;
    // An old generation's delayed post-`wait` removal must never delete the live
    // child's newer record.
    records.remove(&id, generation - 1).unwrap();
    assert!(
        records.load().unwrap().contains_key(&id.0),
        "a stale generation's removal must not delete the live record"
    );
    // Nor while the newer generation is still at its intent stage.
    let still_live = records.load().unwrap().get(&id.0).cloned().unwrap();
    assert_eq!(still_live.generation, generation);
    // Only the owning generation may remove it.
    records.remove(&id, generation).unwrap();
    assert!(!records.load().unwrap().contains_key(&id.0));
    dispatch.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn credential_unavailable_after_clear() {
    let fixture = Fixture::new(vec![host_json(HOST, "10.0.0.1", "password", None)]);
    let dispatch = fixture.dispatch(Arc::new(TestClock::new(70)));
    // No password is stored, so the first attempt is a terminal failure and the
    // answer is Ok with a Failed view, never an Err: Todo 3 classifies it as a
    // first-attempt terminal failure and an error here would contradict that.
    let response = dispatch
        .start(start_request(Some(fixture.spec(free_port())), None, Some(0)), true)
        .await;
    let view = start_view(response).view;
    assert_eq!(view.state, TunnelState::Failed);
    assert_eq!(view.failure, Some(TunnelFailureCode::CredentialUnavailable));
    assert_eq!(view.failure_piece, None);
    dispatch.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn start_by_spec_id_rereads_after_save() {
    let fixture = Fixture::default_host();
    let spawner = RealChildSpawner::new();
    let dispatch = fixture.dispatch_with(Arc::new(TestClock::new(80)), spawner.clone());
    spawner.attach(dispatch.records().unwrap());
    let first_port = free_port();
    dispatch
        .specs_save(save_request(fixture.specs(first_port), Some(0)), true)
        .await;
    let view = start_view(dispatch.start(start_request(None, Some("spec-1"), Some(0)), true).await).view;
    assert_eq!(view.local_port, first_port);
    dispatch
        .stop(TunnelStopRequest { runtime_id: view.runtime_id.unwrap().0 }, true)
        .await;

    // A GUI save does not notify the daemon, so the start-by-id must re-read the
    // file rather than reuse a boot-time cache.
    let second_port = free_port();
    dispatch
        .specs_save(save_request(fixture.specs(second_port), None), true)
        .await;
    let view = start_view(dispatch.start(start_request(None, Some("spec-1"), Some(0)), true).await).view;
    assert_eq!(view.local_port, second_port, "the post-save port must be picked up");
    let argv = spawner.last_argv();
    assert!(
        argv.iter().any(|arg| arg == &format!("-L127.0.0.1:{second_port}:127.0.0.1:5432")),
        "the child argv carries the saved port: {argv:?}"
    );
    assert!(
        !argv.iter().any(|arg| arg.contains(&format!("127.0.0.1:{first_port}:"))),
        "the pre-save port must not survive: {argv:?}"
    );
    dispatch.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn list_prunes_absent_host_definitions() {
    let fixture = Fixture::new(vec![host_json(HOST, "10.0.0.1", "key", None)]);
    fixture.write_definitions(json!({
        "version": 1,
        "definitionsRevision": 3,
        "hostRevisions": { HOST: 2, "gone-host": 1 },
        "hosts": { HOST: fixture.specs(5432), "gone-host": fixture.specs(5433) }
    }));
    let dispatch = fixture.dispatch(Arc::new(TestClock::new(90)));
    let listed = list_response(dispatch.list(true).await);
    assert!(
        listed.definitions.iter().all(|definitions| definitions.host_id != "gone-host"),
        "a host removed from the host store must lose its definitions"
    );
    let written: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&fixture.definitions).unwrap()).unwrap();
    assert!(written["hosts"].get("gone-host").is_none(), "the hosts row is gone: {written}");
    assert!(written["invalidRows"].get("gone-host").is_none() || written.get("invalidRows").is_none());
    assert_eq!(
        written["hostRevisions"]["gone-host"], 1,
        "the revision row deliberately stays, so a re-created host cannot accept a stale start"
    );

    // A malformed host file must leave the definitions file byte-identical: an
    // unreadable file is not "every host was deleted".
    std::fs::write(&fixture.hosts, b"{ not json").unwrap();
    let before = std::fs::read(&fixture.definitions).unwrap();
    dispatch.list(true).await;
    dispatch.sweep_once();
    assert_eq!(std::fs::read(&fixture.definitions).unwrap(), before, "a malformed host file must not rewrite definitions");

    // The same gate for an absent host file.
    std::fs::remove_file(&fixture.hosts).unwrap();
    let before = std::fs::read(&fixture.definitions).unwrap();
    dispatch.list(true).await;
    dispatch.sweep_once();
    assert_eq!(std::fs::read(&fixture.definitions).unwrap(), before, "an absent host file must not wipe definitions");

    // A corrupt definitions file is never re-serialised or downgraded.
    std::fs::write(&fixture.definitions, b"{ not json").unwrap();
    let before = std::fs::read(&fixture.definitions).unwrap();
    dispatch.list(true).await;
    dispatch.sweep_once();
    assert_eq!(std::fs::read(&fixture.definitions).unwrap(), before);

    // A newer-version file is protected the same way.
    std::fs::write(&fixture.definitions, serde_json::to_vec(&json!({ "version": 2, "hosts": {} })).unwrap()).unwrap();
    let before = std::fs::read(&fixture.definitions).unwrap();
    dispatch.list(true).await;
    dispatch.sweep_once();
    assert_eq!(std::fs::read(&fixture.definitions).unwrap(), before);

    // SpecsSave for an absent host on a CLEAN load is host_not_found with the
    // bytes unchanged; on a degraded or absent host file it is accepted.
    let fixture = Fixture::default_host();
    let dispatch = fixture.dispatch(Arc::new(TestClock::new(95)));
    let before = std::fs::read(&fixture.hosts).unwrap();
    let (code, details) = error_of(
        dispatch
            .specs_save(
                TunnelSpecsSaveRequest {
                    host_id: "absent-host".into(),
                    specs: fixture.specs(free_port()),
                    expected_revision: None,
                    force: true,
                    discard_invalid: false,
                },
                true,
            )
            .await,
    );
    assert_eq!(code, "NOT_FOUND");
    assert_eq!(details["reason"], "host_not_found");
    assert_eq!(std::fs::read(&fixture.hosts).unwrap(), before, "a refused save writes nothing");

    std::fs::write(&fixture.hosts, b"{ not json").unwrap();
    assert!(
        matches!(
            dispatch
                .specs_save(
                    TunnelSpecsSaveRequest {
                        host_id: "absent-host".into(),
                        specs: fixture.specs(free_port()),
                        expected_revision: None,
                        force: true,
                        discard_invalid: false,
                    },
                    true,
                )
                .await,
            DaemonResponse::SshTunnelSpecsSave { .. }
        ),
        "a degraded host file must not block the operator's save"
    );
    std::fs::remove_file(&fixture.hosts).unwrap();
    assert!(
        matches!(
            dispatch
                .specs_save(
                    TunnelSpecsSaveRequest {
                        host_id: "absent-host".into(),
                        specs: fixture.specs(free_port()),
                        expected_revision: None,
                        force: true,
                        discard_invalid: false,
                    },
                    true,
                )
                .await,
            DaemonResponse::SshTunnelSpecsSave { .. }
        ),
        "an absent host file must not block the operator's save"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn reset_quarantines_corrupt_and_refuses_valid_store() {
    let fixture = Fixture::default_host();
    fixture.write_definitions(json!({
        "version": 1, "definitionsRevision": 1, "hostRevisions": { HOST: 7 }, "hosts": {}
    }));
    let valid_bytes = std::fs::read(&fixture.definitions).unwrap();
    let dispatch = fixture.dispatch(Arc::new(TestClock::new(1_700_000)));
    let (code, details) = error_of(dispatch.specs_reset(true).await);
    assert_eq!(code, "INVALID_ARGUMENT");
    assert_eq!(details["reason"], "store_not_corrupt");
    assert_eq!(
        std::fs::read(&fixture.definitions).unwrap(),
        valid_bytes,
        "a valid store is never quarantined"
    );

    // A corrupt file is moved aside under a fresh empty store.
    std::fs::write(&fixture.definitions, b"{ not json").unwrap();
    let dispatch = fixture.dispatch(Arc::new(TestClock::new(1_700_000)));
    let quarantined = match dispatch.specs_reset(true).await {
        DaemonResponse::SshTunnelSpecsReset { response } => {
            response.quarantined_path.expect("a quarantine path")
        }
        other => panic!("reset failed: {other:?}"),
    };
    assert!(std::path::Path::new(&quarantined).exists(), "the corrupt file is kept, not deleted");
    assert!(quarantined.contains("ssh_tunnels.corrupt-"), "{quarantined}");
    assert!(fixture.definitions.exists(), "a fresh store replaces the quarantined one");
    // The fresh store seeds every host it can still see at or above the floor, so
    // the pre-reset revision can never be accepted again.
    let (code, details) = error_of(
        dispatch.start(start_request(Some(fixture.spec(free_port())), None, Some(7)), true).await,
    );
    assert_eq!(code, "REQUEST_CONFLICT");
    assert_eq!(details["reason"], "host_revision_stale");
    // And reset re-checks under the lock: a second reset on the now-valid store
    // refuses instead of quarantining good data.
    let (code, details) = error_of(dispatch.specs_reset(true).await);
    assert_eq!(code, "INVALID_ARGUMENT");
    assert_eq!(details["reason"], "store_not_corrupt");
}

/// Compiled but never run on macOS: the Windows refusal table over all seven
/// arms, on a daemon whose tunnel runtime half is compiled out.
#[cfg(windows)]
#[tokio::test]
async fn windows_arms_refuse_before_store() {
    let dir = tempfile::tempdir().unwrap();
    let definitions = dir.path().join("ssh_tunnels.json");
    let hosts = dir.path().join("ssh_hosts.json");
    std::fs::write(&hosts, br#"{"hosts":[]}"#).unwrap();
    let paths = TunnelPaths::new(definitions.clone(), hosts);
    let dispatch = TunnelDispatch::with_clock(paths, Arc::new(SystemClock));
    let spec = TunnelSpec {
        id: TunnelSpecId("w".into()),
        label: "w".into(),
        local_port: Some(15432),
        remote_host: "127.0.0.1".into(),
        remote_port: 5432,
        enabled: true,
    };
    let cases: Vec<DaemonResponse> = vec![
        dispatch
            .start(
                TunnelStartRequest {
                    host_id: "host-1".into(),
                    spec: Some(spec),
                    spec_id: None,
                    host_revision: Some(0),
                },
                true,
            )
            .await,
        dispatch.stop(TunnelStopRequest { runtime_id: "r".into() }, true).await,
        dispatch.list(true).await,
        dispatch
            .invalidate_host(
                TunnelInvalidateHostRequest { host_id: "host-1".into(), host_revision: 0, identity: None },
                true,
            )
            .await,
        dispatch
            .specs_save(
                TunnelSpecsSaveRequest {
                    host_id: "host-1".into(),
                    specs: Vec::new(),
                    expected_revision: Some(0),
                    force: true,
                    discard_invalid: false,
                },
                true,
            )
            .await,
        dispatch.specs_reset(true).await,
        dispatch.forget_host(TunnelForgetHostRequest { host_id: "host-1".into() }, true).await,
    ];
    assert_eq!(cases.len(), 7);
    for (index, response) in cases.into_iter().enumerate() {
        let (code, details) = error_of(response);
        assert_eq!(code, "UNSUPPORTED", "arm {index}");
        assert_eq!(details["reason"], "unsupported_platform", "arm {index}");
    }
    assert!(!definitions.exists(), "no arm may create the definitions file");
}
