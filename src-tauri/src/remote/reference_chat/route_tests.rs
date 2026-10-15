//! Route-composition regression source for the reference-chat routes (plan task 13).
//!
//! These tests drive the PRODUCTION router: every request below goes through
//! `create_remote_router`, the same bearer-token authentication, the same per-device permission
//! and the same target fences a real remote client meets. No transport is mocked, so a route that
//! is not registered, an authentication check that runs after the body is read, or a target fence
//! that is missing shows up here as a wrong status rather than as a passing unit test.
//!
//! Deferred execution: authored with the routes, run only at the complete-code merge barrier.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::http::Method;

use crate::remote::auth::{DeviceAccessScope, DevicePermission};
use crate::remote::relay_server::{allowed_http_route, validate_http_query};
use crate::remote::server::create_remote_router;
use crate::remote::state::RemoteGatewayState;

use super::files as reference_files;
use super::types::REFERENCE_CHAT_ROUTE_PREFIX;

/// The frozen route table, as the plan and the contract name it.
const REFERENCE_CHAT_ROUTES: [(&str, &str); 10] = [
    ("GET", "history"),
    ("GET", "screen"),
    ("GET", "prompt"),
    ("POST", "submit"),
    ("POST", "stop"),
    ("POST", "answer"),
    ("POST", "files"),
    ("GET", "files"),
    ("GET", "files/00000000-0000-0000-0000-000000000000"),
    ("DELETE", "files/00000000-0000-0000-0000-000000000000"),
];

/// An owner id that is NOT this gateway's own. The gateway mints its owner identity itself
/// (\`RemoteGatewayState::reference_owner_id\`) and publishes it beside the host id, so a caller
/// cannot choose one; this value is what a caller choosing for itself would send, and the fence
/// must refuse it.
const ROUTE_TEST_FOREIGN_OWNER: &str = "owner-reference-route";

/// The cursor-position query a pane's terminal may ask before it starts, and the report that
/// answers it. Windows ConPTY asks; the answer is what lets the shell start at all.
const ROUTE_CURSOR_QUERY: &[u8] = b"\x1b[6n";
const ROUTE_CURSOR_REPORT: &[u8] = b"\x1b[1;1R";

/// How many create-path phases one fixture's hook retains. Bounded on purpose: the log is a
/// diagnostic, and the create path emits a fixed, small set of phases.
const ROUTE_PHASE_LIMIT: usize = 16;

/// How many of a pane's observed bytes a failure message renders before it stops.
const ROUTE_SEEN_DUMP_LIMIT: usize = 256;

/// The phases this fixture's daemon reported, with the ms since the hook was armed.
///
/// A plain shared leaf: the daemon's hook captures only this, never the fixture and never the
/// gateway, so arming it cannot form a reference cycle back into the state that owns it.
type RoutePhaseLog = Arc<std::sync::Mutex<Vec<(String, u128)>>>;

/// A bounded, escaped rendering of the bytes a pane produced.
///
/// Printable ASCII stands for itself and every other byte becomes `\xNN`, so a cursor query, a
/// fragment of one, a prompt or an echo of the typed line is readable in a failure message instead
/// of being summarised as a count.
fn escaped_bytes(bytes: &[u8], cap: usize) -> String {
    let shown = bytes.len().min(cap);
    let mut rendered = String::with_capacity(shown * 4);
    for byte in &bytes[..shown] {
        if byte.is_ascii_graphic() || *byte == b' ' {
            rendered.push(*byte as char);
        } else {
            rendered.push_str(&format!("\\x{byte:02x}"));
        }
    }
    if bytes.len() > shown {
        rendered.push_str(&format!("... ({} more bytes)", bytes.len() - shown));
    }
    rendered
}

/// One bound gateway, serving the production router on a loopback port.
struct ReferenceRouteServer {
    addr: SocketAddr,
    /// The gateway this server serves, retained so the fixture's own failure path can report what
    /// that same process still observes. `start` hands the router its reference; this is the
    /// second one it keeps.
    state: Arc<RemoteGatewayState>,
    /// The create-path phases this fixture's daemon reported, in order (see [RoutePhaseLog]).
    phases: RoutePhaseLog,
    tasks: tokio::task::JoinSet<()>,
}

impl ReferenceRouteServer {
    async fn start(state: Arc<RemoteGatewayState>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let mut tasks = tokio::task::JoinSet::new();
        // The router keeps exactly the reference it was handed; this struct retains a second one,
        // so a request that fails to complete can still be diagnosed against that same gateway.
        let router_state = Arc::clone(&state);
        // The create path already reports its phases through the daemon's own test hook
        // (`DaemonWorkspaceService::transaction_probe`, `#[cfg(test)]`, `None` until armed).
        // Arming it here is what turns "the create did not complete" into a located stage: the
        // hook captures only the log below - never the fixture, never the gateway - so nothing
        // here points back at the state that owns the hook.
        let phases: RoutePhaseLog = Arc::new(std::sync::Mutex::new(Vec::new()));
        let armed_at = std::time::Instant::now();
        let sink = Arc::clone(&phases);
        let hook: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |phase: &str| {
            if let Ok(mut log) = sink.lock() {
                if log.len() < ROUTE_PHASE_LIMIT {
                    log.push((phase.to_string(), armed_at.elapsed().as_millis()));
                }
            }
        });
        if let Some(services) = state.machine_services.as_ref() {
            *services.workspaces.transaction_probe.write() = Some(hook);
        }
        tasks.spawn(async move {
            axum::serve(listener, create_remote_router(router_state))
                .await
                .expect("serve");
        });
        Self { addr, state, phases, tasks }
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<&str>,
    ) -> (u16, String) {
        match self.request_observed(method, path, token, body).await {
            Ok(response) => response,
            // A bare transport error cannot say whether the router was still serving, so the
            // failure carries the diagnosis instead of only the reqwest error.
            Err(diagnosis) => panic!("{diagnosis}"),
        }
    }

    /// Send one request, and on transport failure report what this fixture could still observe.
    ///
    /// The request, its bound and every caller's assertions are unchanged; only the text of the
    /// failure is. What it adds is the discrimination a bare `TimedOut` cannot make: whether the
    /// bound router was still answering, and what the owning daemon had published meanwhile.
    async fn request_observed(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<&str>,
    ) -> Result<(u16, String), String> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("client");
        let url = format!("http://{}{path}", self.addr);
        let mut builder = match method {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "DELETE" => client.delete(&url),
            other => panic!("unsupported method {other}"),
        };
        if let Some(token) = token {
            builder = builder.bearer_auth(token);
        }
        if let Some(body) = body {
            builder = builder
                .header("content-type", "application/json")
                .body(body.to_string());
        }
        let started = std::time::Instant::now();
        let response = match builder.send().await {
            Ok(response) => response,
            Err(error) => {
                return Err(self.transport_diagnosis(method, path, started, &error).await);
            }
        };
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        Ok((status, text))
    }

    /// What is still observable after this fixture's own request failed to complete.
    ///
    /// One fresh, independently bounded probe of the same bound router is the discriminator: an
    /// accept loop that answers it is still serving, so a request that did not complete is a
    /// handler that has not finished rather than a router that is gone. The sessions the gateway
    /// can see and the sessions this host's terminal service owns (with each pane's output-hub
    /// sequence range) are reported alongside, so a spawn that happened without a response is
    /// visible too. This is a report, not an assertion: nothing here changes a test's verdict.
    async fn transport_diagnosis(
        &self,
        method: &str,
        path: &str,
        started: std::time::Instant,
        error: &reqwest::Error,
    ) -> String {
        let liveness = match reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(2))
            .build()
        {
            Ok(probe) => {
                match probe
                    .get(format!("http://{}/api/v1/capabilities", self.addr))
                    .send()
                    .await
                {
                    Ok(response) => format!(
                        "a fresh probe of the same router answered {}",
                        response.status().as_u16()
                    ),
                    Err(probe_error) => format!(
                        "a fresh probe of the same router did not answer either: {probe_error}"
                    ),
                }
            }
            Err(build_error) => format!("the probe client could not be built: {build_error}"),
        };
        let backend_sessions = self.state.session_backend.list_sessions().await;
        let hub = self.state.terminal_service.output_hub();
        let owned_sessions: Vec<(String, Option<(Option<u64>, Option<u64>)>)> = self
            .state
            .terminal_service
            .list_sessions()
            .into_iter()
            .map(|id| {
                let range = hub.session_sequence_range(&id);
                (id, range)
            })
            .collect();
        format!(
            "{method} http://{addr}{path} did not complete after {elapsed} ms ({error}); {liveness}; \
             sessions this gateway can see: {backend_sessions:?}; sessions this host's terminal \
             service owns, with their output-hub sequence ranges: {owned_sessions:?}; {timeline}",
            addr = self.addr,
            elapsed = started.elapsed().as_millis(),
            timeline = self.phase_timeline(),
        )
    }

    /// The create-path phases this fixture's daemon reported, in order, bounded by the hook.
    ///
    /// Read on failure only. An empty log means the create path never reached its first phase,
    /// which is itself the answer for a request that never completed.
    fn phase_timeline(&self) -> String {
        match self.phases.lock() {
            Ok(log) if log.is_empty() => {
                "create-path phases: none reported (the hook was never reached)".to_string()
            }
            Ok(log) => {
                let rendered: Vec<String> = log
                    .iter()
                    .map(|(phase, ms)| format!("{phase}@{ms}ms"))
                    .collect();
                format!(
                    "create-path phases (ms since the hook was armed): {}",
                    rendered.join(", ")
                )
            }
            Err(_) => "create-path phases: the log could not be read".to_string(),
        }
    }

    async fn stop(mut self) {
        self.tasks.shutdown().await;
    }
}

/// One gateway with a live shell session and the tokens the route policy is asserted against.
struct ReferenceRouteFixture {
    _root: tempfile::TempDir,
    server: ReferenceRouteServer,
    state: Arc<RemoteGatewayState>,
    control: String,
    view: String,
    session_id: String,
    daemon_epoch: String,
}

async fn fixture() -> ReferenceRouteFixture {
    let root = tempfile::tempdir().expect("tempdir");
    let daemon = crate::daemon::server::DaemonServer::new_with_paths(
        Some(root.path().join("data/config")),
        Some(root.path().join("data/auth")),
    );
    let state = daemon.remote_state().clone();
    let workspace = {
        let services = state.machine_services.as_ref().expect("machine services");
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).expect("project");
        services
            .workspaces
            .register_machine(project.to_str().expect("utf8 path"))
            .expect("register machine workspace")
    };
    let control_pin = state
        .auth_manager
        .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
        .expect("control pin");
    let (control, _) = state
        .auth_manager
        .exchange_pairing_code(&control_pin, "reference-route-control")
        .expect("control token");
    // A view-only device is a Mirror-scope device, and that is the only legitimate door: a
    // Machine-scope grant is refused for anything but Control ("Machine access requires Control
    // permission"), so a Machine-scope View pin cannot be minted at all. The reads below need
    // only a valid token; every mutation route asks for Control and must answer this device 403.
    let view_pin = state.auth_manager.create_pairing_code(DevicePermission::View);
    let (view, _) = state
        .auth_manager
        .exchange_pairing_code(&view_pin, "reference-route-view")
        .expect("view token");
    let server = ReferenceRouteServer::start(Arc::clone(&state)).await;
    let create = serde_json::json!({
        "requestId": uuid::Uuid::new_v4().to_string(),
        "workspaceId": workspace,
        "worktree": null,
        "inheritFromSessionId": null,
        "cwdRelative": null,
        "cols": 100,
        "rows": 30,
        "startup": { "kind": "shell" }
    });
    let (status, body) = server
        .request("POST", "/api/v1/sessions", Some(&control), Some(&create.to_string()))
        .await;
    assert_eq!(status, 201, "session create: {body}");
    let created: serde_json::Value = serde_json::from_str(&body).expect("session body");
    let session_id = created["target"]["sessionId"]
        .as_str()
        .expect("sessionId")
        .to_string();
    let daemon_epoch = created["target"]["daemonEpoch"]
        .as_str()
        .expect("daemonEpoch")
        .to_string();
    ReferenceRouteFixture {
        _root: root,
        server,
        state,
        control,
        view,
        session_id,
        daemon_epoch,
    }
}

impl ReferenceRouteFixture {
    fn route(&self, suffix: &str) -> String {
        format!("{REFERENCE_CHAT_ROUTE_PREFIX}/{}/{suffix}", self.session_id)
    }

    /// The read query a reference-chat read binds its target with.
    fn read_query(
        &self,
        registry_id: &str,
        epoch: &str,
        host: &str,
        extras: &[(&str, String)],
    ) -> String {
        self.read_query_for(&self.session_id, registry_id, epoch, host, extras)
    }

    /// The same query, naming the session the caller claims it is reading.
    ///
    /// The route's own id and the query's backendSessionId are two separate claims about one
    /// read, and the gateway fences them separately: a query naming another session is refused
    /// before the target is looked up, which the ownership fence cannot stand in for.
    fn read_query_for(
        &self,
        backend_session_id: &str,
        registry_id: &str,
        epoch: &str,
        host: &str,
        extras: &[(&str, String)],
    ) -> String {
        self.read_query_naming_owner(
            backend_session_id,
            registry_id,
            epoch,
            host,
            &self.state.reference_owner_id,
            extras,
        )
    }

    /// The same query with the owner named by the caller, so a foreign owner can be sent.
    fn read_query_naming_owner(
        &self,
        backend_session_id: &str,
        registry_id: &str,
        epoch: &str,
        host: &str,
        owner: &str,
        extras: &[(&str, String)],
    ) -> String {
        let mut params = vec![
            ("hostId".to_string(), host.to_string()),
            ("ownerId".to_string(), owner.to_string()),
            ("epoch".to_string(), epoch.to_string()),
            ("backendSessionId".to_string(), backend_session_id.to_string()),
            ("registryId".to_string(), registry_id.to_string()),
        ];
        for (key, value) in extras {
            params.push(((*key).to_string(), value.clone()));
        }
        params
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("&")
    }

    /// The frozen mutation envelope: the target travels with the request id.
    fn mutation(&self, request_id: &str, params: serde_json::Value) -> String {
        self.mutation_naming_owner(request_id, &self.state.reference_owner_id, params)
    }

    /// The same envelope with the owner named by the caller, so a foreign owner can be sent.
    fn mutation_naming_owner(
        &self,
        request_id: &str,
        owner: &str,
        params: serde_json::Value,
    ) -> String {
        serde_json::json!({
            "requestId": request_id,
            "target": {
                "hostId": reference_files::reference_host_id(),
                "ownerId": owner,
                "epoch": self.daemon_epoch.as_str(),
                "backendSessionId": self.session_id.as_str(),
            },
            "params": params,
        })
        .to_string()
    }

    /// Answer every cursor query the bytes seen so far contain, reporting the counts by reference.
    ///
    /// Returns the write error when dispatching a report failed. The pass is byte-driven and
    /// platform-neutral: a host whose pane never asks simply leaves the counts at zero.
    async fn answer_cursor_queries(
        &self,
        seen: &[u8],
        queries: &mut usize,
        answered: &mut usize,
    ) -> Option<String> {
        let detected = seen
            .windows(ROUTE_CURSOR_QUERY.len())
            .filter(|window| *window == ROUTE_CURSOR_QUERY)
            .count();
        if detected > *queries {
            *queries = detected;
        }
        while *answered < *queries {
            if let Err(error) = self
                .state
                .session_backend
                .write_input(&self.session_id, ROUTE_CURSOR_REPORT)
                .await
            {
                return Some(error);
            }
            *answered += 1;
        }
        None
    }

    /// The pane's retained output sequence range: a write to the pane advances it.
    fn sequence_range(&self) -> Option<(Option<u64>, Option<u64>)> {
        self.state
            .terminal_service
            .output_hub()
            .session_sequence_range(&self.session_id)
    }

    /// The pane's geometry, as the owning daemon reports it.
    async fn geometry(&self) -> (u16, u16) {
        let details = self
            .state
            .session_backend
            .describe_session(&self.session_id)
            .await
            .expect("the session this fixture created is live");
        (details.cols, details.rows)
    }

    /// What this host can say about the pane when a bounded wait on its output did not complete.
    ///
    /// Cause-neutral on purpose: it reports what separates "the pane never produced a byte" from
    /// "the pane produced output the reader never carried" from "the pane is gone", and asserts
    /// none of them.
    async fn pane_diagnosis(&self) -> String {
        let details = self
            .state
            .session_backend
            .describe_session(&self.session_id)
            .await;
        let range = self
            .state
            .terminal_service
            .output_hub()
            .session_sequence_range(&self.session_id);
        let owned = self.state.terminal_service.list_sessions();
        format!(
            "session {id}: details {details:?}; output-hub sequence range {range:?}; this host's \
             terminal service owns {owned:?}; {phases}",
            id = self.session_id,
            phases = self.server.phase_timeline(),
        )
    }

    /// Type one line through the reference submit route and wait for the pane to run it.
    ///
    /// The subscription is taken BEFORE the trigger, and the wait is bounded by an event, never by
    /// a fixed sleep.
    ///
    /// Two properties of the real pane shape the command. First, the session was created with
    /// `startup.kind = "shell"`, and this host's shell resolver starts `pwsh` on Windows, not a
    /// POSIX shell: a `printf` there is not a command the pane runs, and its echoed input alone
    /// would satisfy an assertion that never proved execution. The command is therefore written in
    /// the shell the resolver picks, with the marker split across that shell's own literals so the
    /// contiguous string exists only in the output (the convention `terminal/pty.rs` records:
    /// "write a command whose OUTPUT marker is split so the echoed input never contains it").
    /// Second, this harness is the pane's terminal client: a shell that asks its terminal for the
    /// cursor position before it starts is answered here (Windows ConPTY does; no unix shell is
    /// known to - see `remote/relay_server.rs` and the ssh helper fixtures). That mechanism is NOT
    /// offered as the cause of a failure on another platform. The wait's failure instead reports
    /// the observations that discriminate - the first output seen, whether a cursor query was
    /// detected and answered, whether a write failed, and what the host says about the pane - and
    /// none of them is asserted to be the cause.
    async fn submit_and_await_marker(&self, marker: &str) {
        self.submit_and_await_marker_at(marker, &self.server, &self.route("submit"))
            .await;
    }

    async fn submit_and_await_marker_at(
        &self,
        marker: &str,
        server: &ReferenceRouteServer,
        path: &str,
    ) {
        let (prefix, suffix) = marker
            .split_once('_')
            .expect("the marker names a prefix and a suffix the shell joins");
        let command = if cfg!(windows) {
            format!("Write-Output ('{prefix}_' + '{suffix}')")
        } else {
            format!("printf '{prefix}_%s\\n' '{suffix}'\n")
        };
        let attachment = self
            .state
            .session_backend
            .attach_with_sequence(&self.session_id, None)
            .await
            .expect("attach the pane");
        // The attach's own replay is the pane's first word, and it is the only way to hear it:
        // this harness attaches after the fixture created the session, so anything the pane
        // emitted in between was published before any subscription existed, and a live-only
        // reader can never be told about it. (On Windows that includes ConPTY's cursor query,
        // which leaves the shell unstarted until it is answered; on any platform it includes the
        // shell's own first output.) `output_hub::subscribe_with_sequence` takes the snapshot and
        // the subscription in one critical section, so seeded from the snapshot the window below
        // covers the pane from its first byte onward, in either order of the reader and this
        // attach. That is why the window is seeded; it is not offered as the cause of a failure
        // on any particular platform.
        let mut seen: Vec<u8> = attachment.snapshot.history;
        let seeded = seen.len();
        let gap = attachment.snapshot.gap.is_some();
        let mut watch = attachment.receiver;
        let mut queries = 0usize;
        let mut answered = 0usize;
        let mut last_write_error: Option<String> = None;
        // The replay is scanned BEFORE the wait for live output. On Windows the pane's cursor
        // query arrives in this replay and the shell does not start until it is answered, so a
        // detector that watched only live chunks would wait on a pane that is waiting on the
        // answer. The live loop below repeats the same pass for anything that arrives later.
        if let Some(error) = self.answer_cursor_queries(&seen, &mut queries, &mut answered).await {
            last_write_error = Some(error);
        }
        let request_id = uuid::Uuid::new_v4().to_string();
        let (status, body) = server
            .request(
                "POST",
                path,
                Some(&self.control),
                Some(&self.mutation(
                    &request_id,
                    serde_json::json!({ "text": command, "origin": "chat" }),
                )),
            )
            .await;
        assert_eq!(status, 200, "submit: {body}");
        let observed = tokio::time::timeout(std::time::Duration::from_secs(20), async {
            loop {
                match watch.recv().await {
                    Ok(chunk) => {
                        seen.extend_from_slice(&chunk.bytes);
                        if seen
                            .windows(marker.len())
                            .any(|window| window == marker.as_bytes())
                        {
                            return true;
                        }
                        // A shell that asks its terminal for the cursor before it starts is
                        // answered here, and the same pass already ran over the replay. Whether
                        // this host's pane asks is not assumed: the count and the dispatch are
                        // reported in the failure below.
                        if let Some(error) =
                            self.answer_cursor_queries(&seen, &mut queries, &mut answered).await
                        {
                            last_write_error = Some(error);
                            return false;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return false,
                }
            }
        })
        .await;
        if observed.ok() != Some(true) {
            let pane = self.pane_diagnosis().await;
            // The bytes are the one datum a count cannot carry: a query, a fragment of one, a
            // prompt and an echo of the typed line are all "some bytes and no marker".
            let dump = escaped_bytes(&seen, ROUTE_SEEN_DUMP_LIMIT);
            panic!(
                "the pane must run {marker} and print it within the deadline: {} bytes observed \
                 ({seeded} replayed at the attach, replay gap {gap}), observed bytes: {dump}; \
                 {queries} cursor query(ies) detected, {answered} answer(s) dispatched, last write \
                 error {last_write_error:?}; {pane}",
                seen.len()
            );
        }
    }
}

#[tokio::test]
async fn a_reference_chat_submit_through_the_relay_reaches_the_real_pane() {
    use crate::remote::relay_client::RelayClient;
    use crate::remote::relay_server::{relay_router, RelayState};
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    // Given an isolated gateway, real PTY, and production relay/client with synthetic keys.
    let fixture = fixture().await;
    let relay_state = RelayState::new_with_key_store(
        vec!["reference-route-enrollment".to_string()],
        fixture._root.path().join("relay-keys.json"),
    )
    .expect("isolated relay key store");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("relay listener");
    let addr = listener.local_addr().expect("relay address");
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move {
        axum::serve(
            listener,
            relay_router(relay_state).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("serve isolated relay");
    });
    let key = ed25519_dalek::SigningKey::from_bytes(&[37; 32]);
    let identity = crate::remote::auth::MachineIdentity {
        machine_id: "reference-route-relay".to_string(),
        display_name: "reference route relay".to_string(),
        public_key: STANDARD.encode(key.verifying_key().to_bytes()),
        private_key: STANDARD.encode(key.to_bytes()),
    };
    let client = RelayClient::with_identity_and_auth(
        &format!("ws://{addr}"),
        identity,
        fixture.server.addr.to_string(),
        (*fixture.state.auth_manager).clone(),
    )
    .with_enrollment_token("reference-route-enrollment");
    let coordinator = client.pairing_coordinator();
    tasks.spawn(async move { client.run().await });
    let outcome = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(async {
        // The registration ACK is the readiness barrier; no sleeps or polling.
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            coordinator.generate_pairing(std::time::Duration::from_secs(60)),
        )
        .await
        .expect("relay ready deadline")
        .expect("relay registration ACK");
        let relay_server = ReferenceRouteServer {
            addr,
            state: Arc::clone(&fixture.state),
            phases: Arc::clone(&fixture.server.phases),
            tasks: tokio::task::JoinSet::new(),
        };
        // When the authenticated submit traverses the host HTTP tunnel, then the real shell
        // must execute it and emit the split marker, not merely echo the submitted text.
        fixture
            .submit_and_await_marker_at(
                "RELAY_REFERENCE_ROUTE_MARKER",
                &relay_server,
                &format!("/host/reference-route-relay{}", fixture.route("submit")),
            )
            .await;
    }))
    .await;
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    fixture.server.stop().await;
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

/// Every documented route is registered on the existing gateway, and authentication precedes
/// every path, query and body decision.
#[tokio::test]
async fn every_reference_chat_route_is_registered_behind_authentication() {
    let fixture = fixture().await;
    for (method, suffix) in REFERENCE_CHAT_ROUTES {
        let path = fixture.route(suffix);
        let (status, body) = fixture.server.request(method, &path, None, None).await;
        assert_eq!(
            status, 401,
            "{method} {path} must be a registered, authenticated route (a 404 means unregistered): {body}"
        );
        assert!(
            body.contains("UNAUTHORIZED"),
            "{method} {path} must answer the machine error envelope: {body}"
        );
    }
    fixture.server.stop().await;
}

/// The target fences: the daemon incarnation, the owning host and the session itself.
#[tokio::test]
async fn a_reference_chat_read_refuses_a_foreign_target() {
    let fixture = fixture().await;
    let path = fixture.route("screen");

    let foreign_epoch = fixture.read_query("codex", "0", &reference_files::reference_host_id(), &[]);
    let (status, body) = fixture
        .server
        .request("GET", &format!("{path}?{foreign_epoch}"), Some(&fixture.control), None)
        .await;
    assert_eq!(status, 410, "a foreign daemon incarnation: {body}");
    assert!(body.contains("TARGET_EXPIRED"), "{body}");

    let foreign_host = fixture.read_query("codex", &fixture.daemon_epoch, "not-this-host", &[]);
    let (status, body) = fixture
        .server
        .request("GET", &format!("{path}?{foreign_host}"), Some(&fixture.control), None)
        .await;
    assert_eq!(status, 403, "a foreign owning host: {body}");
    assert!(body.contains("FORBIDDEN"), "{body}");

    let missing_registry =
        fixture.read_query("", &fixture.daemon_epoch, &reference_files::reference_host_id(), &[]);
    let (status, body) = fixture
        .server
        .request(
            "GET",
            &format!("{}?{missing_registry}", fixture.route("history")),
            Some(&fixture.control),
            None,
        )
        .await;
    assert_eq!(status, 400, "a history read names its reader: {body}");
    assert!(body.contains("INVALID_REQUEST"), "{body}");

    // The query's own claim is fenced on its own: this read is addressed to this host's own live
    // session with this host's own incarnation, and names a session that is not the route's. The
    // mismatch is the only thing wrong with it, so a missing fence shows up here as a 200 rather
    // than as a refusal some other fence happened to produce.
    let mismatched = fixture.read_query_for(
        &uuid::Uuid::new_v4().to_string(),
        "codex",
        &fixture.daemon_epoch,
        &reference_files::reference_host_id(),
        &[],
    );
    let (status, body) = fixture
        .server
        .request("GET", &format!("{path}?{mismatched}"), Some(&fixture.control), None)
        .await;
    assert_eq!(status, 403, "a query naming another session: {body}");
    assert!(body.contains("FORBIDDEN"), "{body}");

    // A session this host does not own: the query names the route's own id, so the mismatch fence
    // above has nothing to refuse and the ownership fence is what answers.
    let unknown = uuid::Uuid::new_v4().to_string();
    let unknown_query = fixture.read_query_for(
        &unknown,
        "codex",
        &fixture.daemon_epoch,
        &reference_files::reference_host_id(),
        &[],
    );
    let (status, body) = fixture
        .server
        .request(
            "GET",
            &format!("{REFERENCE_CHAT_ROUTE_PREFIX}/{unknown}/screen?{unknown_query}"),
            Some(&fixture.control),
            None,
        )
        .await;
    assert_eq!(status, 404, "a session this host does not own: {body}");
    assert!(body.contains("NOT_FOUND"), "{body}");

    fixture.server.stop().await;
}

/// A target naming another owner is refused on every original-pane route - reads and mutations
/// alike - and the refusals write nothing and resize nothing.
///
/// This is the plan's screen negative (task 6: "segmented VT versus replay gap/wrong owner and
/// recorded zero writes/resizes"), and the fence that answers it is the shared target binding
/// rather than the screen reader: \`reference_chat::screen\` performs no authorization by design.
/// A valid control runs first, so a refusal produced by an unrelated guard cannot stand in.
#[tokio::test]
async fn a_target_naming_another_owner_is_refused_without_writing_or_resizing() {
    let fixture = fixture().await;
    assert!(
        !fixture.state.reference_owner_id.is_empty()
            && fixture.state.reference_owner_id != ROUTE_TEST_FOREIGN_OWNER,
        "the gateway's owner identity is its own value, never the caller's"
    );

    // Valid control: this gateway's own owner is served, so the refusals below are the fence's.
    let control = fixture.read_query(
        "codex",
        &fixture.daemon_epoch,
        &reference_files::reference_host_id(),
        &[],
    );
    let (status, body) = fixture
        .server
        .request(
            "GET",
            &format!("{}?{control}", fixture.route("screen")),
            Some(&fixture.control),
            None,
        )
        .await;
    assert_eq!(status, 200, "the gateway's own owner must still be served: {body}");
    let geometry_before = fixture.geometry().await;
    let sequences_before = fixture.sequence_range();

    // Every read, with a foreign owner and nothing else changed.
    for suffix in ["screen", "history", "prompt"] {
        let foreign = fixture.read_query_naming_owner(
            &fixture.session_id,
            "codex",
            &fixture.daemon_epoch,
            &reference_files::reference_host_id(),
            ROUTE_TEST_FOREIGN_OWNER,
            &[],
        );
        let (status, body) = fixture
            .server
            .request(
                "GET",
                &format!("{}?{foreign}", fixture.route(suffix)),
                Some(&fixture.control),
                None,
            )
            .await;
        assert_eq!(status, 410, "a foreign owner on {suffix} must be refused: {body}");
        assert!(body.contains("TARGET_EXPIRED"), "typed refusal on {suffix}: {body}");
    }

    // The mutation envelope carries the owner inside its target, and is fenced the same way.
    let request_id = uuid::Uuid::new_v4().to_string();
    let (status, body) = fixture
        .server
        .request(
            "POST",
            &fixture.route("submit"),
            Some(&fixture.control),
            Some(&fixture.mutation_naming_owner(
                &request_id,
                ROUTE_TEST_FOREIGN_OWNER,
                serde_json::json!({ "text": "never typed\n", "origin": "chat" }),
            )),
        )
        .await;
    assert_eq!(status, 410, "a foreign owner must not reach a mutation: {body}");
    assert!(body.contains("TARGET_EXPIRED"), "typed refusal on submit: {body}");

    // Zero writes and zero resizes: nothing was submitted in this test, so the idle pane emits
    // nothing, and a refused target must leave both observables exactly as the control left them.
    assert_eq!(
        fixture.geometry().await,
        geometry_before,
        "a refused target resized the pane"
    );
    assert_eq!(
        fixture.sequence_range(),
        sequences_before,
        "a refused target wrote to the pane"
    );

    fixture.server.stop().await;
}

/// The read-only policy is retained: a view-only device may read and may not mutate.
#[tokio::test]
async fn a_view_only_device_cannot_mutate_a_reference_chat_target() {
    let fixture = fixture().await;
    let query = fixture.read_query(
        "codex",
        &fixture.daemon_epoch,
        &reference_files::reference_host_id(),
        &[],
    );
    let (status, body) = fixture
        .server
        .request(
            "GET",
            &format!("{}?{query}", fixture.route("screen")),
            Some(&fixture.view),
            None,
        )
        .await;
    assert_eq!(status, 200, "a view-only device may still read the pane: {body}");

    let request_id = uuid::Uuid::new_v4().to_string();
    let (status, body) = fixture
        .server
        .request(
            "POST",
            &fixture.route("submit"),
            Some(&fixture.view),
            Some(&fixture.mutation(
                &request_id,
                serde_json::json!({ "text": "never typed\n", "origin": "chat" }),
            )),
        )
        .await;
    assert_eq!(status, 403, "a view-only device must not type: {body}");
    assert!(body.contains("FORBIDDEN"), "{body}");

    let (status, body) = fixture
        .server
        .request(
            "POST",
            &fixture.route("files"),
            Some(&fixture.view),
            Some(&fixture.mutation(
                &uuid::Uuid::new_v4().to_string(),
                serde_json::json!({
                    "name": "never-staged.txt",
                    "mediaType": "text/plain",
                    "sizeBytes": 0,
                    "contentBase64": "",
                }),
            )),
        )
        .await;
    assert_eq!(status, 403, "a view-only device must not stage: {body}");
    assert!(body.contains("FORBIDDEN"), "{body}");

    fixture.server.stop().await;
}

/// One ordered submit: accepted, deduplicated by request id, conflicted on a changed payload —
/// and never a resize of the pane it typed into.
#[tokio::test]
async fn an_ordered_submit_is_deduplicated_and_never_resizes_the_pane() {
    let fixture = fixture().await;
    let before = fixture.geometry().await;
    assert_eq!(before, (100, 30), "the fixture created a 100x30 pane");

    let request_id = uuid::Uuid::new_v4().to_string();
    let payload = serde_json::json!({ "text": "echo reference-route\n", "origin": "chat" });
    let (status, body) = fixture
        .server
        .request(
            "POST",
            &fixture.route("submit"),
            Some(&fixture.control),
            Some(&fixture.mutation(&request_id, payload.clone())),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let result: serde_json::Value = serde_json::from_str(&body).expect("a ScopeResult");
    assert_eq!(result["ok"], true, "{body}");
    assert_eq!(result["requestId"], request_id, "{body}");
    assert_eq!(
        result["data"]["receipt"]["stage"], "accepted",
        "the writer took the bytes; that is accepted, never providerRead: {body}"
    );

    // The same request id with the same payload returns the recorded state.
    let (status, body) = fixture
        .server
        .request(
            "POST",
            &fixture.route("submit"),
            Some(&fixture.control),
            Some(&fixture.mutation(&request_id, payload)),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let duplicate: serde_json::Value = serde_json::from_str(&body).expect("a ScopeResult");
    assert_eq!(duplicate["ok"], true, "a duplicate must replay its recorded state: {body}");

    // The same request id with a different payload is a conflict, not a second send.
    let (status, body) = fixture
        .server
        .request(
            "POST",
            &fixture.route("submit"),
            Some(&fixture.control),
            Some(&fixture.mutation(
                &request_id,
                serde_json::json!({ "text": "echo something else\n", "origin": "chat" }),
            )),
        )
        .await;
    assert_eq!(status, 409, "a conflicting payload must fail: {body}");
    assert!(body.contains("REQUEST_CONFLICT"), "{body}");

    assert_eq!(
        fixture.geometry().await,
        before,
        "no reference-chat route may resize the pane"
    );
    fixture.server.stop().await;
}

/// The reads: the pane's own screen, and a history page that discloses same-pane output rather
/// than claiming a native reader it does not have.
#[tokio::test]
async fn reference_chat_reads_serve_the_pane_screen_and_an_honest_history_page() {
    let fixture = fixture().await;
    let marker = "REFERENCE_ROUTE_MARKER";
    fixture.submit_and_await_marker(marker).await;
    let before = fixture.geometry().await;

    let query = fixture.read_query(
        "codex",
        &fixture.daemon_epoch,
        &reference_files::reference_host_id(),
        &[],
    );
    let (status, body) = fixture
        .server
        .request(
            "GET",
            &format!("{}?{query}", fixture.route("screen")),
            Some(&fixture.control),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let screen: serde_json::Value = serde_json::from_str(&body).expect("a screen snapshot");
    assert_eq!(screen["cols"], 100, "{body}");
    assert_eq!(screen["rows"], 30, "{body}");
    assert_eq!(screen["gap"], false, "{body}");
    assert_eq!(screen["truncated"], false, "{body}");
    assert!(
        screen["text"]
            .as_str()
            .is_some_and(|text| text.contains(marker)),
        "the read must show the pane's own screen: {body}"
    );
    assert!(
        screen["revision"]
            .as_str()
            .is_some_and(|revision| !revision.is_empty()),
        "{body}"
    );

    // A plain shell screen holds no card: a successful read with no prompt.
    let (status, body) = fixture
        .server
        .request(
            "GET",
            &format!("{}?{query}", fixture.route("prompt")),
            Some(&fixture.control),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let card: serde_json::Value = serde_json::from_str(&body).expect("a prompt read");
    assert_eq!(card["prompt"], serde_json::Value::Null, "{body}");
    assert!(
        card["screenRevision"]
            .as_str()
            .is_some_and(|revision| !revision.is_empty()),
        "an answer is validated against the revision it saw: {body}"
    );

    // A registry entry the reference has no reader for is same-pane output, disclosed as such.
    let scrollback = fixture.read_query(
        "droid",
        &fixture.daemon_epoch,
        &reference_files::reference_host_id(),
        &[("limit", "50".to_string())],
    );
    let (status, body) = fixture
        .server
        .request(
            "GET",
            &format!("{}?{scrollback}", fixture.route("history")),
            Some(&fixture.control),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let page: serde_json::Value = serde_json::from_str(&body).expect("a history page");
    assert_eq!(page["availability"], "scrollback", "{body}");
    assert_eq!(page["source"], "scrollback", "{body}");
    assert!(
        page["unavailableReason"]
            .as_str()
            .is_some_and(|reason| !reason.is_empty()),
        "a non-native page must disclose why: {body}"
    );
    assert!(
        page["generation"]
            .as_str()
            .is_some_and(|generation| !generation.is_empty()),
        "{body}"
    );

    // A native reader with no exact store is NOT_FOUND, never another session's newest file.
    let native = fixture.read_query(
        "codex",
        &fixture.daemon_epoch,
        &reference_files::reference_host_id(),
        &[],
    );
    let (status, body) = fixture
        .server
        .request(
            "GET",
            &format!("{}?{native}", fixture.route("history")),
            Some(&fixture.control),
            None,
        )
        .await;
    assert_eq!(
        status, 404,
        "no store identifies this pane's codex conversation: {body}"
    );
    assert!(body.contains("NOT_FOUND"), "{body}");

    assert_eq!(
        fixture.geometry().await,
        before,
        "a read never resizes the pane"
    );
    fixture.server.stop().await;
}

/// The file lane stages on the owning host, previews what it staged, and deletes only explicitly.
#[tokio::test]
async fn staged_reference_files_round_trip_on_the_owning_host() {
    let fixture = fixture().await;
    let bytes = b"reference chat staged fixture\n";
    use base64::Engine as _;
    let content = base64::engine::general_purpose::STANDARD.encode(bytes);
    let request_id = uuid::Uuid::new_v4().to_string();
    let (status, body) = fixture
        .server
        .request(
            "POST",
            &fixture.route("files"),
            Some(&fixture.control),
            Some(&fixture.mutation(
                &request_id,
                serde_json::json!({
                    "name": "reference-fixture.txt",
                    "mediaType": "text/plain",
                    "sizeBytes": bytes.len(),
                    "contentBase64": content,
                }),
            )),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let staged: serde_json::Value = serde_json::from_str(&body).expect("a ScopeResult");
    assert_eq!(staged["ok"], true, "{body}");
    assert_eq!(
        staged["data"]["receipt"]["hostId"],
        reference_files::reference_host_id(),
        "the receipt names the owning host: {body}"
    );
    assert_eq!(staged["data"]["receipt"]["sizeBytes"], bytes.len(), "{body}");
    let attachment_id = staged["data"]["receipt"]["attachmentId"]
        .as_str()
        .expect("attachmentId")
        .to_string();
    assert_eq!(
        staged["data"]["mentionText"],
        format!(
            "@{}/{attachment_id}/reference-fixture.txt ",
            reference_files::REFERENCE_STAGING_DIR_NAME
        ),
        "the mention is editable text naming the worktree-relative staged path: {body}"
    );

    let query = fixture.read_query(
        "codex",
        &fixture.daemon_epoch,
        &reference_files::reference_host_id(),
        &[],
    );
    let (status, body) = fixture
        .server
        .request(
            "GET",
            &format!("{}?{query}", fixture.route(&format!("files/{attachment_id}"))),
            Some(&fixture.control),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let preview: serde_json::Value = serde_json::from_str(&body).expect("a preview");
    assert_eq!(preview["contentBase64"], content, "{body}");

    // A file id that is not this target's is not a preview of anything.
    let foreign = fixture.route(&format!("files/{}", uuid::Uuid::new_v4()));
    let (status, body) = fixture
        .server
        .request("GET", &format!("{foreign}?{query}"), Some(&fixture.control), None)
        .await;
    assert_eq!(status, 404, "a foreign file id: {body}");
    assert!(body.contains("NOT_FOUND"), "{body}");

    // Deletion is explicit, and a view-only device cannot perform it.
    let own = fixture.route(&format!("files/{attachment_id}"));
    let (status, body) = fixture
        .server
        .request("DELETE", &format!("{own}?{query}"), Some(&fixture.view), None)
        .await;
    assert_eq!(status, 403, "a view-only device must not delete: {body}");

    let (status, body) = fixture
        .server
        .request("DELETE", &format!("{own}?{query}"), Some(&fixture.control), None)
        .await;
    assert_eq!(status, 204, "an explicit delete: {body}");

    let (status, body) = fixture
        .server
        .request("GET", &format!("{own}?{query}"), Some(&fixture.control), None)
        .await;
    assert_eq!(status, 404, "the deleted file is gone: {body}");

    fixture.server.stop().await;
}

/// The relay forwards exactly the frozen route table, with exactly the target query keys it uses,
/// and nothing else under the prefix.
#[test]
fn the_relay_allowlist_admits_exactly_the_reference_chat_route_table() {
    let session = "11111111-1111-1111-1111-111111111111";
    for (method, suffix) in REFERENCE_CHAT_ROUTES {
        let method = Method::from_bytes(method.as_bytes()).expect("method");
        let path = format!("reference-chat/{session}/{suffix}");
        assert!(
            allowed_http_route(&method, &path),
            "{method} {path} must be forwarded to the owning host"
        );
    }
    for path in [
        "reference-chat/x",
        "reference-chat/x/history/extra",
        "reference-chat/x/secret",
        "reference-chat/x/submit/../admin",
    ] {
        assert!(
            !allowed_http_route(&Method::GET, path),
            "{path} must not be forwarded"
        );
    }

    let history = format!("reference-chat/{session}/history");
    assert!(validate_http_query(
        &history,
        Some(
            "hostId=local&ownerId=owner&epoch=1&backendSessionId=sess&providerSessionId=provider\
             &registryId=codex&limit=200&cursor=12&cursorStream=codex-transcript:abc"
        )
    )
    .is_ok());
    assert!(
        validate_http_query(&history, Some("repoPath=/etc/passwd")).is_err(),
        "a query key the contract does not define must be refused"
    );
    assert!(validate_http_query(
        &format!("reference-chat/{session}/screen"),
        Some("hostId=local&ownerId=owner&epoch=1&backendSessionId=sess&registryId=codex")
    )
    .is_ok());
    assert!(validate_http_query(
        &format!("reference-chat/{session}/files/attachment-1"),
        Some("hostId=local&ownerId=owner&epoch=1&backendSessionId=sess")
    )
    .is_ok());
    // A mutation carries its target in its own envelope body, so its query is empty.
    assert!(validate_http_query(&format!("reference-chat/{session}/submit"), None).is_ok());
}

/// A backend whose one pane shows a fixed screen and records every write it is handed.
struct ScreenStubBackend {
    session_id: String,
    cols: u16,
    rows: u16,
    screen: Vec<u8>,
    writes: std::sync::Mutex<Vec<Vec<u8>>>,
    /// Every write, as it is handed over, for a test that waits on a specific one.
    written: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
}

impl crate::remote::backend::RemoteSessionBackend for ScreenStubBackend {
    fn list_sessions(&self) -> futures_util::future::BoxFuture<'_, Vec<String>> {
        Box::pin(async move { vec![self.session_id.clone()] })
    }
    fn describe_session<'a>(
        &'a self,
        session_id: &'a str,
    ) -> futures_util::future::BoxFuture<
        'a,
        Result<crate::remote::backend::RemoteSessionDetails, String>,
    > {
        Box::pin(async move {
            if session_id != self.session_id {
                return Err("unknown session".to_string());
            }
            Ok(crate::remote::backend::RemoteSessionDetails {
                session_id: self.session_id.clone(),
                workspace_id: None,
                worktree_label: None,
                worktree_path: None,
                running: true,
                cols: self.cols,
                rows: self.rows,
            })
        })
    }
    fn attach_with_sequence<'a>(
        &'a self,
        _session_id: &'a str,
        _after_sequence: Option<u64>,
    ) -> futures_util::future::BoxFuture<'a, Result<crate::terminal::SessionAttachment, String>>
    {
        Box::pin(async move {
            let (_sender, receiver) = tokio::sync::broadcast::channel(1);
            Ok(crate::terminal::SessionAttachment {
                snapshot: crate::terminal::AttachmentSnapshot {
                    session_id: self.session_id.clone(),
                    history_start_sequence: Some(1),
                    history_end_sequence: Some(1),
                    history: self.screen.clone(),
                    history_segments: vec![crate::terminal::HistorySegment {
                        cols: Some(self.cols),
                        rows: Some(self.rows),
                        bytes: self.screen.clone(),
                    }],
                    gap: None,
                    history_incomplete: false,
                },
                receiver,
            })
        })
    }
    fn write_input<'a>(
        &'a self,
        _session_id: &'a str,
        data: &'a [u8],
    ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
        self.writes.lock().expect("writes").push(data.to_vec());
        if let Some(written) = self.written.as_ref() {
            let _ = written.send(data.to_vec());
        }
        Box::pin(async { Ok(()) })
    }
    fn resize<'a>(
        &'a self,
        _session_id: &'a str,
        _cols: u16,
        _rows: u16,
    ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Err("this stub never resizes".to_string()) })
    }
    fn signal<'a>(
        &'a self,
        _session_id: &'a str,
        _signal: crate::terminal::TerminalSignal,
    ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Err("this stub never signals".to_string()) })
    }
}

/// A screen too large to answer from must not read as "no prompt": a chat message is not typed
/// into the Claude menu at its bottom.
#[tokio::test]
async fn a_chat_submit_is_refused_when_the_screen_cannot_be_read_whole() {
    let session_id = "reference-route-truncated".to_string();
    let (cols, rows) = (300u16, 70u16);
    let mut screen = Vec::new();
    for _ in 0..56 {
        screen.extend_from_slice("x".repeat(usize::from(cols) - 1).as_bytes());
        screen.extend_from_slice(b"\r\n");
    }
    let menu = include_str!("fixtures/claude/prompt/approval-command.screen");
    screen.extend_from_slice(menu.trim_end().replace('\n', "\r\n").as_bytes());
    let backend = Arc::new(ScreenStubBackend {
        session_id: session_id.clone(),
        cols,
        rows,
        screen,
        writes: std::sync::Mutex::new(Vec::new()),
        written: None,
    });
    let state = Arc::new(RemoteGatewayState::new_with_backend(
        Arc::clone(&backend) as Arc<dyn crate::remote::backend::RemoteSessionBackend>,
        crate::worktree::registry::WorkspaceRegistry::new(),
    ));
    let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
    let (control, _) = state
        .auth_manager
        .exchange_pairing_code(&pin, "reference-route-truncated")
        .expect("control token");
    let server = ReferenceRouteServer::start(Arc::clone(&state)).await;
    let epoch = state
        .daemon_epoch
        .load(std::sync::atomic::Ordering::Acquire)
        .to_string();
    let body = serde_json::json!({
        "requestId": uuid::Uuid::new_v4().to_string(),
        "target": {
            "hostId": reference_files::reference_host_id(),
            "ownerId": state.reference_owner_id,
            "epoch": epoch,
            "backendSessionId": session_id,
        },
        "registryId": "claude",
        "params": { "text": "1", "origin": "chat" },
    });
    let (status, response) = server
        .request(
            "POST",
            &format!("{REFERENCE_CHAT_ROUTE_PREFIX}/{session_id}/submit"),
            Some(&control),
            Some(&body.to_string()),
        )
        .await;

    assert_eq!(status, 409, "{response}");
    assert!(response.contains("CONTROL_CONFLICT"), "{response}");
    assert!(
        backend.writes.lock().expect("writes").is_empty(),
        "chat text was typed into a menu the gateway could not rule out"
    );
    server.stop().await;
}

/// A chat submit that names no registry id (the mobile client omits it when the pane's agent is
/// unknown) has no prompt detector to run, so it is still delivered.
#[tokio::test]
async fn a_chat_submit_without_a_registry_id_is_still_delivered() {
    let session_id = "reference-route-no-registry".to_string();
    let backend = Arc::new(ScreenStubBackend {
        session_id: session_id.clone(),
        cols: 80,
        rows: 24,
        screen: b"$ ".to_vec(),
        writes: std::sync::Mutex::new(Vec::new()),
        written: None,
    });
    let state = Arc::new(RemoteGatewayState::new_with_backend(
        Arc::clone(&backend) as Arc<dyn crate::remote::backend::RemoteSessionBackend>,
        crate::worktree::registry::WorkspaceRegistry::new(),
    ));
    let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
    let (control, _) = state
        .auth_manager
        .exchange_pairing_code(&pin, "reference-route-no-registry")
        .expect("control token");
    let server = ReferenceRouteServer::start(Arc::clone(&state)).await;
    let epoch = state
        .daemon_epoch
        .load(std::sync::atomic::Ordering::Acquire)
        .to_string();
    let body = serde_json::json!({
        "requestId": uuid::Uuid::new_v4().to_string(),
        "target": {
            "hostId": reference_files::reference_host_id(),
            "ownerId": state.reference_owner_id,
            "epoch": epoch,
            "backendSessionId": session_id,
        },
        "params": { "text": "hello", "origin": "chat" },
    });
    let (status, response) = server
        .request(
            "POST",
            &format!("{REFERENCE_CHAT_ROUTE_PREFIX}/{session_id}/submit"),
            Some(&control),
            Some(&body.to_string()),
        )
        .await;

    assert_eq!(status, 200, "{response}");
    assert!(response.contains("\"stage\":\"accepted\""), "{response}");
    let writes = backend.writes.lock().expect("writes").clone();
    assert_eq!(writes.first().map(Vec::as_slice), Some(b"hello".as_slice()), "{writes:?}");
    assert_eq!(writes.last().map(Vec::as_slice), Some(b"\r".as_slice()), "{writes:?}");
    server.stop().await;
}

/// A gateway whose one pane this host's output hub does not hold, as a session still served by a
/// legacy daemon after a handover is: the hub cannot say whether that pane has bracketed paste
/// on, and a control device on it.
fn unhubbed_pane(
    session_id: &str,
    written: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
) -> (Arc<ScreenStubBackend>, Arc<RemoteGatewayState>, String) {
    let backend = Arc::new(ScreenStubBackend {
        session_id: session_id.to_string(),
        cols: 80,
        rows: 24,
        screen: b"$ ".to_vec(),
        writes: std::sync::Mutex::new(Vec::new()),
        written,
    });
    let state = Arc::new(RemoteGatewayState::new_with_backend(
        Arc::clone(&backend) as Arc<dyn crate::remote::backend::RemoteSessionBackend>,
        crate::worktree::registry::WorkspaceRegistry::new(),
    ));
    let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
    let (control, _) = state
        .auth_manager
        .exchange_pairing_code(&pin, session_id)
        .expect("control token");
    (backend, state, control)
}

/// The submit envelope for an unhubbed pane, naming no registry id so no screen probe runs.
fn unhubbed_submit(state: &RemoteGatewayState, session_id: &str, text: &str) -> String {
    let epoch = state
        .daemon_epoch
        .load(std::sync::atomic::Ordering::Acquire)
        .to_string();
    serde_json::json!({
        "requestId": uuid::Uuid::new_v4().to_string(),
        "target": {
            "hostId": reference_files::reference_host_id(),
            "ownerId": state.reference_owner_id,
            "epoch": epoch,
            "backendSessionId": session_id,
        },
        "params": { "text": text, "origin": "chat" },
    })
    .to_string()
}

/// rs-input-screen-r3-1: a pane whose bracketed-paste mode this host cannot read (a legacy-peer
/// session after a handover) must not have a multi-line message typed unbracketed, where its
/// inner CR would submit the first line as a turn of its own.
#[tokio::test]
async fn a_multi_line_submit_to_a_pane_whose_paste_mode_is_unknown_types_nothing() {
    let session_id = "reference-route-legacy-peer";
    let (backend, state, control) = unhubbed_pane(session_id, None);
    let server = ReferenceRouteServer::start(Arc::clone(&state)).await;
    let (status, response) = server
        .request(
            "POST",
            &format!("{REFERENCE_CHAT_ROUTE_PREFIX}/{session_id}/submit"),
            Some(&control),
            Some(&unhubbed_submit(&state, session_id, "fix A\nthen B")),
        )
        .await;
    let writes = backend.writes.lock().expect("writes").clone();
    assert!(
        writes.is_empty(),
        "a multi-line message was typed into a pane whose paste mode is unknown: {writes:?}"
    );
    assert_eq!(status, 422, "{response}");
    assert!(response.contains("UNSUPPORTED"), "{response}");
    server.stop().await;
}

/// rs-input-screen-r3-2: a caller that goes away between the paste and its Enter must not leave
/// the paste unsent in the pane. The handler future is dropped exactly as hyper drops it on a
/// disconnect, right after the paste is written.
#[tokio::test]
async fn a_submit_whose_caller_leaves_after_the_paste_still_writes_its_enter() {
    let session_id = "reference-route-caller-left";
    let (written_tx, mut written) = tokio::sync::mpsc::unbounded_channel();
    let (_backend, state, control) = unhubbed_pane(session_id, Some(written_tx));
    let request = axum::http::Request::builder()
        .method("POST")
        .uri(format!("{REFERENCE_CHAT_ROUTE_PREFIX}/{session_id}/submit"))
        .header("authorization", format!("Bearer {control}"))
        .header("content-type", "application/json")
        .body(axum::body::Body::from(unhubbed_submit(&state, session_id, "deploy now")))
        .expect("request");
    let handler = crate::remote::server::reference_chat_submit(
        axum::extract::State(Arc::clone(&state)),
        axum::extract::Path(session_id.to_string()),
        request,
    );
    let paste = tokio::select! {
        response = handler => panic!("the submit finished before its caller left: {:?}", response.status()),
        paste = written.recv() => paste.expect("the paste was written"),
    };
    // The handler future is gone here: the caller left during the gap before the Enter.
    assert_eq!(paste, b"deploy now".to_vec());
    let enter = tokio::time::timeout(std::time::Duration::from_secs(5), written.recv())
        .await
        .ok()
        .flatten();
    assert_eq!(
        enter.as_deref(),
        Some(b"\r".as_slice()),
        "the paste was left unsent in the pane after its caller left"
    );
}

/// A backend whose one pane is served by a real output hub, so a replay gap behaves as it does on
/// an SSH-remote pane whose read reconnected.
struct HubStubBackend {
    session_id: String,
    hub: crate::terminal::TerminalOutputHub,
    writes: std::sync::Mutex<Vec<Vec<u8>>>,
}

impl crate::remote::backend::RemoteSessionBackend for HubStubBackend {
    fn list_sessions(&self) -> futures_util::future::BoxFuture<'_, Vec<String>> {
        Box::pin(async move { vec![self.session_id.clone()] })
    }
    fn describe_session<'a>(
        &'a self,
        session_id: &'a str,
    ) -> futures_util::future::BoxFuture<
        'a,
        Result<crate::remote::backend::RemoteSessionDetails, String>,
    > {
        Box::pin(async move {
            if session_id != self.session_id {
                return Err("unknown session".to_string());
            }
            Ok(crate::remote::backend::RemoteSessionDetails {
                session_id: self.session_id.clone(),
                workspace_id: None,
                worktree_label: None,
                worktree_path: None,
                running: true,
                cols: 80,
                rows: 24,
            })
        })
    }
    fn attach_with_sequence<'a>(
        &'a self,
        session_id: &'a str,
        after_sequence: Option<u64>,
    ) -> futures_util::future::BoxFuture<'a, Result<crate::terminal::SessionAttachment, String>>
    {
        Box::pin(async move {
            self.hub
                .subscribe_with_sequence(session_id, after_sequence)
                .ok_or_else(|| "unknown session".to_string())
        })
    }
    fn write_input<'a>(
        &'a self,
        _session_id: &'a str,
        data: &'a [u8],
    ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
        self.writes.lock().expect("writes").push(data.to_vec());
        Box::pin(async { Ok(()) })
    }
    fn resize<'a>(
        &'a self,
        _session_id: &'a str,
        _cols: u16,
        _rows: u16,
    ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Err("this stub never resizes".to_string()) })
    }
    fn signal<'a>(
        &'a self,
        _session_id: &'a str,
        _signal: crate::terminal::TerminalSignal,
    ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Err("this stub never signals".to_string()) })
    }
}

/// After a replay gap the hub holds only what the pane drew since, which is not the whole screen:
/// a Claude menu drawn before the gap is still on the pane while the hub replays only a spinner
/// line. A chat submit must not read that partial screen as "no prompt" and type into the menu.
#[tokio::test]
async fn a_chat_submit_is_refused_when_the_screen_history_has_a_replay_gap() {
    let session_id = "reference-route-post-gap".to_string();
    let hub = crate::terminal::TerminalOutputHub::default();
    hub.register_session(&session_id);
    hub.record_initial_size(&session_id, 80, 24);
    let menu = include_str!("fixtures/claude/prompt/approval-command.screen");
    hub.publish(&session_id, menu.trim_end().replace('\n', "\r\n").into_bytes())
        .expect("menu published");
    hub.publish_gap(&session_id).expect("gap published");
    hub.publish(&session_id, "\r\u{273b} Thinking\u{2026} (esc to interrupt)".as_bytes().to_vec())
        .expect("spinner published");
    let backend = Arc::new(HubStubBackend {
        session_id: session_id.clone(),
        hub,
        writes: std::sync::Mutex::new(Vec::new()),
    });
    let state = Arc::new(RemoteGatewayState::new_with_backend(
        Arc::clone(&backend) as Arc<dyn crate::remote::backend::RemoteSessionBackend>,
        crate::worktree::registry::WorkspaceRegistry::new(),
    ));
    let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
    let (control, _) = state
        .auth_manager
        .exchange_pairing_code(&pin, "reference-route-post-gap")
        .expect("control token");
    let server = ReferenceRouteServer::start(Arc::clone(&state)).await;
    let epoch = state
        .daemon_epoch
        .load(std::sync::atomic::Ordering::Acquire)
        .to_string();
    let body = serde_json::json!({
        "requestId": uuid::Uuid::new_v4().to_string(),
        "target": {
            "hostId": reference_files::reference_host_id(),
            "ownerId": state.reference_owner_id,
            "epoch": epoch,
            "backendSessionId": session_id,
        },
        "registryId": "claude",
        "params": { "text": "1", "origin": "chat" },
    });
    let (status, response) = server
        .request(
            "POST",
            &format!("{REFERENCE_CHAT_ROUTE_PREFIX}/{session_id}/submit"),
            Some(&control),
            Some(&body.to_string()),
        )
        .await;

    assert_eq!(status, 409, "{response}");
    assert!(response.contains("CONTROL_CONFLICT"), "{response}");
    assert!(
        backend.writes.lock().expect("writes").is_empty(),
        "chat text was typed into a pane whose screen was read from post-gap bytes only"
    );
    server.stop().await;
}

/// An agent with no prompt detector can never be holding a prompt this route would detect, so a
/// gapped screen must not refuse its chat submit: refusing would lock chat out of the pane for
/// good, since nothing it draws later restores a whole-screen read.
#[tokio::test]
async fn a_chat_submit_to_an_agent_without_a_prompt_detector_ignores_a_replay_gap() {
    let session_id = "reference-route-post-gap-no-detector".to_string();
    assert!(super::prompts::ReferencePromptFamily::of_agent("opencode").is_none());
    let hub = crate::terminal::TerminalOutputHub::default();
    hub.register_session(&session_id);
    hub.record_initial_size(&session_id, 80, 24);
    hub.publish(&session_id, b"opencode ready\r\n".to_vec())
        .expect("output published");
    hub.publish_gap(&session_id).expect("gap published");
    hub.publish(&session_id, b"\rworking...".to_vec())
        .expect("post-gap output published");
    let backend = Arc::new(HubStubBackend {
        session_id: session_id.clone(),
        hub,
        writes: std::sync::Mutex::new(Vec::new()),
    });
    let state = Arc::new(RemoteGatewayState::new_with_backend(
        Arc::clone(&backend) as Arc<dyn crate::remote::backend::RemoteSessionBackend>,
        crate::worktree::registry::WorkspaceRegistry::new(),
    ));
    let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
    let (control, _) = state
        .auth_manager
        .exchange_pairing_code(&pin, "reference-route-post-gap-no-detector")
        .expect("control token");
    let server = ReferenceRouteServer::start(Arc::clone(&state)).await;
    let epoch = state
        .daemon_epoch
        .load(std::sync::atomic::Ordering::Acquire)
        .to_string();
    let body = serde_json::json!({
        "requestId": uuid::Uuid::new_v4().to_string(),
        "target": {
            "hostId": reference_files::reference_host_id(),
            "ownerId": state.reference_owner_id,
            "epoch": epoch,
            "backendSessionId": session_id,
        },
        "registryId": "opencode",
        "params": { "text": "hello", "origin": "chat" },
    });
    let (status, response) = server
        .request(
            "POST",
            &format!("{REFERENCE_CHAT_ROUTE_PREFIX}/{session_id}/submit"),
            Some(&control),
            Some(&body.to_string()),
        )
        .await;

    assert_eq!(status, 200, "{response}");
    assert!(
        !backend.writes.lock().expect("writes").is_empty(),
        "chat text to an agent without a prompt detector was refused over a replay gap"
    );
    server.stop().await;
}

/// No control router is mounted under the prefix, and an unknown suffix is not a route.
#[tokio::test]
async fn an_unknown_reference_chat_suffix_is_not_a_route() {
    let fixture = fixture().await;
    let before = fixture.geometry().await;
    for suffix in ["control", "resize", "kill", "create"] {
        let (status, body) = fixture
            .server
            .request("POST", &fixture.route(suffix), Some(&fixture.control), None)
            .await;
        assert_eq!(
            status, 404,
            "reference-chat/{suffix} must not exist: {body}"
        );
    }
    assert_eq!(
        fixture.geometry().await,
        before,
        "the whole route table leaves the pane's geometry alone"
    );
    fixture.server.stop().await;
}

impl ReferenceRouteFixture {
    /// The pane's own working directory, as this gateway's backend reports it.
    async fn worktree(&self) -> std::path::PathBuf {
        self.state
            .session_backend
            .describe_session(&self.session_id)
            .await
            .expect("describe the fixture session")
            .worktree_path
            .expect("the fixture session has a working directory")
    }

    async fn stage(&self, name: &str, bytes: &[u8]) -> (u16, String) {
        use base64::Engine as _;
        let content = base64::engine::general_purpose::STANDARD.encode(bytes);
        self.server
            .request(
                "POST",
                &self.route("files"),
                Some(&self.control),
                Some(&self.mutation(
                    &uuid::Uuid::new_v4().to_string(),
                    serde_json::json!({
                        "name": name,
                        "mediaType": "text/plain",
                        "sizeBytes": bytes.len(),
                        "contentBase64": content,
                    }),
                )),
            )
            .await
    }
}

/// rs-files-routes-1: the mention the agent receives names a path it can open from the pane's
/// own working directory, and that path holds exactly the uploaded bytes.
#[tokio::test]
async fn a_staged_mention_resolves_to_the_uploaded_bytes_under_the_pane_worktree() {
    let fixture = fixture().await;
    let bytes = b"notes staged for the agent\n";
    let (status, body) = fixture.stage("notes.txt", bytes).await;
    assert_eq!(status, 200, "{body}");
    let staged: serde_json::Value = serde_json::from_str(&body).expect("a ScopeResult");
    let mention = staged["data"]["mentionText"].as_str().expect("mentionText").to_string();
    let path = mention
        .strip_prefix('@')
        .expect("an @ mention")
        .trim_end()
        .to_string();
    assert!(
        reference_files::reference_mention_path_is_safe(&path),
        "the mention must be a safe relative path: {mention:?}"
    );
    let worktree = fixture.worktree().await;
    let resolved = worktree.join(&path);
    assert_eq!(
        std::fs::read(&resolved).ok().as_deref(),
        Some(&bytes[..]),
        "<worktree>/{path} must hold the uploaded bytes (worktree {})",
        worktree.display()
    );
    fixture.server.stop().await;
}

/// rs-files-routes-r7-1: a pane whose shell has moved since it was spawned stages into the
/// directory it is in NOW, so the relative mention resolves from the agent's live cwd.
#[tokio::test]
async fn a_staged_mention_resolves_from_the_panes_live_cwd_after_a_cd() {
    let fixture = fixture().await;
    // The shell is up (and, on Windows, its cursor query answered) before it is asked to move.
    fixture.submit_and_await_marker("CWDREADY_ONE").await;
    let worktree = fixture.worktree().await;
    let nested = worktree.join("nested");
    std::fs::create_dir_all(&nested).expect("nested dir");
    let marker = "CWDMOVED_TWO";
    let command = if cfg!(windows) {
        "Set-Location nested; [Environment]::CurrentDirectory = (Get-Location).ProviderPath; Write-Output ('CWDMOVED_' + 'TWO')\r".to_string()
    } else {
        "cd nested && printf 'CWDMOVED_%s\\n' 'TWO'\r".to_string()
    };
    let attachment = fixture
        .state
        .session_backend
        .attach_with_sequence(&fixture.session_id, None)
        .await
        .expect("attach the pane");
    let mut seen = attachment.snapshot.history;
    let mut watch = attachment.receiver;
    fixture
        .state
        .session_backend
        .write_input(&fixture.session_id, command.as_bytes())
        .await
        .expect("type the cd");
    let moved = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            match watch.recv().await {
                Ok(chunk) => {
                    seen.extend_from_slice(&chunk.bytes);
                    if seen.windows(marker.len()).any(|window| window == marker.as_bytes()) {
                        return true;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return false,
            }
        }
    })
    .await;
    assert_eq!(moved.ok(), Some(true), "the pane must run the cd: {}", escaped_bytes(&seen, 4096));

    let bytes = b"staged after a cd\n";
    let (status, body) = fixture.stage("moved.txt", bytes).await;
    assert_eq!(status, 200, "{body}");
    let staged: serde_json::Value = serde_json::from_str(&body).expect("a ScopeResult");
    let mention = staged["data"]["mentionText"].as_str().expect("mentionText").to_string();
    let path = mention.strip_prefix('@').expect("an @ mention").trim_end().to_string();
    assert!(reference_files::reference_mention_path_is_safe(&path), "{mention:?}");
    assert_eq!(
        std::fs::read(nested.join(&path)).ok().as_deref(),
        Some(&bytes[..]),
        "<live cwd>/{path} must hold the uploaded bytes (live cwd {})",
        nested.display()
    );
    fixture.server.stop().await;
}

/// rs-files-routes-5: the per-target file cap holds under concurrent uploads.
#[tokio::test]
async fn concurrent_stages_for_one_target_respect_the_file_cap() {
    let fixture = fixture().await;
    let attempts = (0..8).map(|index| {
        let fixture = &fixture;
        async move { fixture.stage(&format!("parallel-{index}.txt"), b"parallel body").await }
    });
    let results = futures_util::future::join_all(attempts).await;
    let ok = results.iter().filter(|(status, _)| *status == 200).count();
    assert!(
        ok <= crate::scoped_contracts::ATTACHMENT_MAX_FILES_PER_TURN,
        "at most {} concurrent stages may succeed, {ok} did: {results:?}",
        crate::scoped_contracts::ATTACHMENT_MAX_FILES_PER_TURN
    );
    assert!(ok >= 1, "at least one stage must succeed: {results:?}");
    fixture.server.stop().await;
}

/// rs-files-routes-r2-1: files a submitted turn already referenced no longer count against the
/// per-turn bounds, so the next turn can attach a full set again; their bytes stay on disk.
#[tokio::test]
async fn files_a_submitted_turn_referenced_free_the_per_turn_bounds() {
    let fixture = fixture().await;
    let max = crate::scoped_contracts::ATTACHMENT_MAX_FILES_PER_TURN;
    let mut mentions = String::new();
    let mut paths = Vec::new();
    for index in 0..max {
        let (status, body) = fixture
            .stage(&format!("turn-one-{index}.txt"), b"turn one body")
            .await;
        assert_eq!(status, 200, "{body}");
        let staged: serde_json::Value = serde_json::from_str(&body).expect("a ScopeResult");
        let mention = staged["data"]["mentionText"].as_str().expect("mentionText");
        paths.push(mention.trim_start_matches('@').trim_end().to_string());
        mentions.push_str(mention);
    }
    let (status, body) = fixture
        .server
        .request(
            "POST",
            &fixture.route("submit"),
            Some(&fixture.control),
            Some(&fixture.mutation(
                &uuid::Uuid::new_v4().to_string(),
                serde_json::json!({ "text": format!("echo {mentions}"), "origin": "chat" }),
            )),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("\"stage\":\"accepted\""), "{body}");

    let (status, body) = fixture.stage("turn-two.txt", b"turn two body").await;
    assert_eq!(
        status, 200,
        "a new turn must not be refused for files the previous turn already sent: {body}"
    );
    let worktree = fixture.worktree().await;
    for path in &paths {
        assert!(
            worktree.join(path).exists(),
            "a referenced file stays on disk for its TTL: {path}"
        );
    }
    fixture.server.stop().await;
}

/// rs-files-routes-6: a DELETE whose removal fails is not reported as cleaned, and the entry
/// stays addressable so a retry can still remove it.
#[cfg(unix)]
#[tokio::test]
async fn a_failed_staged_file_delete_is_not_204_and_keeps_the_entry() {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = fixture().await;
    let (status, body) = fixture.stage("locked.txt", b"locked body").await;
    assert_eq!(status, 200, "{body}");
    let staged: serde_json::Value = serde_json::from_str(&body).expect("a ScopeResult");
    let attachment_id = staged["data"]["receipt"]["attachmentId"]
        .as_str()
        .expect("attachmentId")
        .to_string();
    let path = staged["data"]["mentionText"]
        .as_str()
        .expect("mentionText")
        .trim_start_matches('@')
        .trim_end()
        .to_string();
    let worktree = fixture.worktree().await;
    let staged_file = worktree.join(&path);
    // Make the attachment directory unremovable: its parent loses write permission.
    let attachment_dir = staged_file.parent().expect("attachment dir").to_path_buf();
    let base = attachment_dir.parent().expect("staging base").to_path_buf();
    let original = std::fs::metadata(&base).expect("base metadata").permissions();
    std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o500)).expect("lock base");

    let query = fixture.read_query(
        "codex",
        &fixture.daemon_epoch,
        &reference_files::reference_host_id(),
        &[],
    );
    let own = format!("{}?{query}", fixture.route(&format!("files/{attachment_id}")));
    let (first, first_body) = fixture
        .server
        .request("DELETE", &own, Some(&fixture.control), None)
        .await;
    let (second, second_body) = fixture
        .server
        .request("DELETE", &own, Some(&fixture.control), None)
        .await;
    std::fs::set_permissions(&base, original).expect("unlock base");
    assert_ne!(first, 204, "a delete that could not remove the file: {first_body}");
    assert_ne!(
        second, 404,
        "the entry must survive a failed delete so a retry can remove it: {second_body}"
    );
    let (third, third_body) = fixture
        .server
        .request("DELETE", &own, Some(&fixture.control), None)
        .await;
    assert_eq!(third, 204, "a retried delete after the fault clears: {third_body}");
    assert!(!attachment_dir.exists(), "the retried delete removed the bytes");
    fixture.server.stop().await;
}

/// rs-files-routes-r3-2: a staged file whose stage response the client never saw (an aborted
/// upload, a page reload) is listed back to the client, so it can be deleted rather than
/// holding a per-turn slot for the whole TTL.
#[tokio::test]
async fn orphaned_staged_files_are_listed_and_can_be_released() {
    let fixture = fixture().await;
    let max = crate::scoped_contracts::ATTACHMENT_MAX_FILES_PER_TURN;
    for index in 0..max {
        let (status, body) = fixture.stage(&format!("orphan-{index}.txt"), b"orphan").await;
        assert_eq!(status, 200, "{body}");
    }
    let (status, body) = fixture.stage("blocked.txt", b"blocked").await;
    assert_eq!(status, 413, "the per-turn cap holds: {body}");

    let query = fixture.read_query(
        "codex",
        &fixture.daemon_epoch,
        &reference_files::reference_host_id(),
        &[],
    );
    let (status, body) = fixture
        .server
        .request(
            "GET",
            &format!("{}?{query}", fixture.route("files")),
            Some(&fixture.control),
            None,
        )
        .await;
    assert_eq!(status, 200, "the target's staged files must be listable: {body}");
    let listed: serde_json::Value = serde_json::from_str(&body).expect("a file list");
    let files = listed["files"].as_array().expect("files array");
    assert_eq!(files.len(), max, "{body}");
    for file in files {
        assert!(file["displayName"].is_string(), "{body}");
        assert!(file["mentionText"].as_str().is_some_and(|m| m.starts_with('@')), "{body}");
        assert_eq!(file["receipt"]["hostId"], reference_files::reference_host_id(), "{body}");
        let id = file["receipt"]["attachmentId"].as_str().expect("attachmentId");
        let own = format!("{}?{query}", fixture.route(&format!("files/{id}")));
        let (status, body) = fixture
            .server
            .request("DELETE", &own, Some(&fixture.control), None)
            .await;
        assert_eq!(status, 204, "a listed orphan can be released: {body}");
    }

    let (status, body) = fixture.stage("after-release.txt", b"after").await;
    assert_eq!(status, 200, "released orphans free the per-turn bounds: {body}");
    fixture.server.stop().await;
}

/// The attachment directories under a pane's staging root, if the root exists yet.
fn staged_attachment_dirs(worktree: &std::path::Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(reference_files::reference_staging_base_dir(worktree))
        .map(|entries| entries.filter_map(|entry| entry.ok().map(|entry| entry.path())).collect())
        .unwrap_or_default()
}

/// rs-files-routes-r4-1: a stage whose caller leaves while its bytes are being written must not
/// leave a hidden reservation behind. The handler future is dropped exactly as hyper drops it on
/// a disconnect, while the write is in flight; afterwards every slot the target holds is either
/// listed (and so releasable) or free, so a full set can be staged again.
#[tokio::test]
async fn a_stage_whose_caller_leaves_mid_write_leaves_no_hidden_reservation() {
    use base64::Engine as _;

    let fixture = fixture().await;
    let worktree = fixture.worktree().await;
    let bytes = vec![b'x'; 4 * 1024 * 1024];
    let request = axum::http::Request::builder()
        .method("POST")
        .uri(fixture.route("files"))
        .header("authorization", format!("Bearer {}", fixture.control))
        .header("content-type", "application/json")
        .body(axum::body::Body::from(fixture.mutation(
            &uuid::Uuid::new_v4().to_string(),
            serde_json::json!({
                "name": "aborted.bin",
                "mediaType": "text/plain",
                "sizeBytes": bytes.len(),
                "contentBase64": base64::engine::general_purpose::STANDARD.encode(&bytes),
            }),
        )))
        .expect("request");
    let mut handler = Box::pin(crate::remote::server::reference_chat_stage_file(
        axum::extract::State(Arc::clone(&fixture.state)),
        axum::extract::Path(fixture.session_id.clone()),
        request,
    ));
    // Drive the handler only until its write has begun on disk; the disk is checked BEFORE each
    // poll, so the handler can never run past the write without this loop seeing it first.
    let mid_write = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            if !staged_attachment_dirs(&worktree).is_empty() {
                return;
            }
            if let std::task::Poll::Ready(response) = futures_util::poll!(handler.as_mut()) {
                panic!("the stage finished before its caller left: {:?}", response.status());
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(mid_write.is_ok(), "the stage never began writing its bytes");
    // The caller leaves while the write is in flight.
    drop(handler);

    let query = fixture.read_query(
        "codex",
        &fixture.daemon_epoch,
        &reference_files::reference_host_id(),
        &[],
    );
    // The stage the caller left must become visible (and so releasable): there is no event for
    // it, so the list is re-read until it shows the file, within a bound.
    let listed = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let (status, body) = fixture
                .server
                .request(
                    "GET",
                    &format!("{}?{query}", fixture.route("files")),
                    Some(&fixture.control),
                    None,
                )
                .await;
            assert_eq!(status, 200, "{body}");
            let listed: serde_json::Value = serde_json::from_str(&body).expect("a file list");
            if !listed["files"].as_array().expect("files array").is_empty() {
                return listed;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the stage whose caller left never became listable, so it cannot be released");
    for file in listed["files"].as_array().expect("files array") {
        let id = file["receipt"]["attachmentId"].as_str().expect("attachmentId");
        let own = format!("{}?{query}", fixture.route(&format!("files/{id}")));
        let (status, body) = fixture
            .server
            .request("DELETE", &own, Some(&fixture.control), None)
            .await;
        assert_eq!(status, 204, "a listed slot can be released: {body}");
    }
    for index in 0..crate::scoped_contracts::ATTACHMENT_MAX_FILES_PER_TURN {
        let (status, body) = fixture.stage(&format!("after-abort-{index}.txt"), b"after").await;
        assert_eq!(
            status, 200,
            "an aborted stage left a reservation that is neither listed nor free: {body}"
        );
    }
    fixture.server.stop().await;
}

/// rs-files-routes-r4-2: a pane whose session runs on an SSH remote must not have its file staged
/// on this gateway's own disk at the remote's project path; the stage is refused, typed, and no
/// bytes land locally.
#[tokio::test]
async fn a_stage_for_an_ssh_remote_pane_is_refused_and_writes_nothing_locally() {
    let fixture = fixture().await;
    let worktree = fixture.worktree().await;
    let store_path = fixture
        .state
        .machine_services
        .as_ref()
        .expect("machine services")
        .sessions
        .remote_sessions_store_path()
        .to_path_buf();
    // The remote's project path exists on this host too, the case where a local write would
    // silently succeed at the wrong place.
    let store = serde_json::json!({
        "version": 3,
        "remoteSessions": [{
            "descriptor": {
                "backendSessionId": fixture.session_id,
                "config": {
                    "host": { "authMethod": "agent", "hostname": "203.0.113.7", "id": "ssh-test" },
                    "environment": { "home": "/home/remote", "platform": "posix" },
                    "projectPath": worktree.to_string_lossy(),
                },
            },
        }],
    });
    std::fs::create_dir_all(store_path.parent().expect("store dir")).expect("store dir");
    std::fs::write(&store_path, store.to_string()).expect("write remote session store");

    let (status, body) = fixture.stage("remote.txt", b"remote body").await;
    assert!(
        staged_attachment_dirs(&worktree).is_empty(),
        "an SSH-remote pane's file was staged on this gateway's disk: {body}"
    );
    assert_eq!(status, 422, "{body}");
    assert!(body.contains("UNSUPPORTED"), "{body}");
    fixture.server.stop().await;
}

