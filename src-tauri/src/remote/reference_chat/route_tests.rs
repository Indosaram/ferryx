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
const REFERENCE_CHAT_ROUTES: [(&str, &str); 9] = [
    ("GET", "history"),
    ("GET", "screen"),
    ("GET", "prompt"),
    ("POST", "submit"),
    ("POST", "stop"),
    ("POST", "answer"),
    ("POST", "files"),
    ("GET", "files/00000000-0000-0000-0000-000000000000"),
    ("DELETE", "files/00000000-0000-0000-0000-000000000000"),
];

/// The owner id these tests fence their targets with. It travels as part of the identity tuple;
/// the gateway does not invent a second owner concept it cannot verify.
const ROUTE_TEST_OWNER: &str = "owner-reference-route";

/// One bound gateway, serving the production router on a loopback port.
struct ReferenceRouteServer {
    addr: SocketAddr,
    tasks: tokio::task::JoinSet<()>,
}

impl ReferenceRouteServer {
    async fn start(state: Arc<RemoteGatewayState>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            axum::serve(listener, create_remote_router(state))
                .await
                .expect("serve");
        });
        Self { addr, tasks }
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
             service owns, with their output-hub sequence ranges: {owned_sessions:?}",
            addr = self.addr,
            elapsed = started.elapsed().as_millis(),
        )
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
        let mut params = vec![
            ("hostId".to_string(), host.to_string()),
            ("ownerId".to_string(), ROUTE_TEST_OWNER.to_string()),
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
        serde_json::json!({
            "requestId": request_id,
            "target": {
                "hostId": reference_files::reference_host_id(),
                "ownerId": ROUTE_TEST_OWNER,
                "epoch": self.daemon_epoch.as_str(),
                "backendSessionId": self.session_id.as_str(),
            },
            "params": params,
        })
        .to_string()
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
             terminal service owns {owned:?}",
            id = self.session_id,
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
        const CURSOR_QUERY: &[u8] = b"\x1b[6n";
        const CURSOR_REPORT: &[u8] = b"\x1b[1;1R";
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
        let request_id = uuid::Uuid::new_v4().to_string();
        let (status, body) = self
            .server
            .request(
                "POST",
                &self.route("submit"),
                Some(&self.control),
                Some(&self.mutation(
                    &request_id,
                    serde_json::json!({ "text": command, "origin": "chat" }),
                )),
            )
            .await;
        assert_eq!(status, 200, "submit: {body}");
        let mut queries = 0usize;
        let mut answered = 0usize;
        let mut last_write_error: Option<String> = None;
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
                        // answered here. Whether this host's pane asks is not assumed: the count
                        // and the dispatch are reported in the failure below.
                        let detected = seen
                            .windows(CURSOR_QUERY.len())
                            .filter(|window| *window == CURSOR_QUERY)
                            .count();
                        if detected > queries {
                            queries = detected;
                        }
                        while answered < queries {
                            if let Err(error) = self
                                .state
                                .session_backend
                                .write_input(&self.session_id, CURSOR_REPORT)
                                .await
                            {
                                last_write_error = Some(error);
                                return false;
                            }
                            answered += 1;
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
            panic!(
                "the pane must run {marker} and print it within the deadline: {} bytes observed \
                 ({seeded} replayed at the attach, replay gap {gap}), {queries} cursor query(ies) \
                 detected, {answered} answer(s) dispatched, last write error {last_write_error:?}; \
                 {pane}",
                seen.len()
            );
        }
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
    assert_eq!(
        staged["data"]["mentionText"], "@reference-fixture.txt ",
        "the mention is editable text, not an opaque attachment: {body}"
    );
    let attachment_id = staged["data"]["receipt"]["attachmentId"]
        .as_str()
        .expect("attachmentId")
        .to_string();

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
