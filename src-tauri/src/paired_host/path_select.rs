use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const PATH_PROBE_DEADLINE: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AttachPath {
    Lan,
    Tailscale,
    SshForward,
    Relay,
}

impl AttachPath {
    pub fn tie_break_rank(self) -> u8 {
        match self {
            AttachPath::Lan => 0,
            AttachPath::Tailscale => 1,
            AttachPath::SshForward => 2,
            AttachPath::Relay => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathOutcome {
    pub path: AttachPath,
    pub rtt: Option<Duration>,
}

impl PathOutcome {
    pub fn reachable(path: AttachPath, rtt: Duration) -> Self {
        Self {
            path,
            rtt: Some(rtt),
        }
    }

    pub fn unreachable(path: AttachPath) -> Self {
        Self { path, rtt: None }
    }
}

pub fn select_path(outcomes: &[PathOutcome]) -> Option<AttachPath> {
    outcomes
        .iter()
        .filter(|outcome| outcome.rtt.is_some())
        .min_by_key(|outcome| {
            (
                outcome.rtt.expect("filtered to measured paths"),
                outcome.path.tie_break_rank(),
            )
        })
        .map(|outcome| outcome.path)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidatePath {
    pub path: AttachPath,
    pub base_origin: String,
    pub auth_token: Option<String>,
    pub expected_machine_id: Option<String>,
}

impl CandidatePath {
    pub fn new(path: AttachPath, base_origin: impl Into<String>, auth_token: Option<String>) -> Self {
        Self {
            path,
            base_origin: base_origin.into(),
            auth_token,
            expected_machine_id: None,
        }
    }

    pub fn with_expected_machine_id(
        path: AttachPath,
        base_origin: impl Into<String>,
        auth_token: Option<String>,
        expected_machine_id: Option<String>,
    ) -> Self {
        Self {
            path,
            base_origin: base_origin.into(),
            auth_token,
            expected_machine_id,
        }
    }
}

pub async fn probe_candidate(
    client: &reqwest::Client,
    candidate: &CandidatePath,
) -> PathOutcome {
    let Some(ref token) = candidate.auth_token else {
        return PathOutcome::unreachable(candidate.path);
    };

    let base = candidate.base_origin.trim_end_matches('/');
    let probe_url = format!("{base}/api/v1/capabilities");

    let start = Instant::now();
    let response = client
        .get(&probe_url)
        .bearer_auth(token)
        .timeout(PATH_PROBE_DEADLINE)
        .send()
        .await;

    match response {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                if let Some(ref expected_machine_id) = candidate.expected_machine_id {
                    match resp.json::<serde_json::Value>().await {
                        Ok(body) => {
                            if body.get("machineId").and_then(|v| v.as_str())
                                == Some(expected_machine_id.as_str())
                            {
                                PathOutcome::reachable(candidate.path, start.elapsed())
                            } else {
                                PathOutcome::unreachable(candidate.path)
                            }
                        }
                        Err(_) => PathOutcome::unreachable(candidate.path),
                    }
                } else {
                    PathOutcome::reachable(candidate.path, start.elapsed())
                }
            } else {
                PathOutcome::unreachable(candidate.path)
            }
        }
        Err(_) => PathOutcome::unreachable(candidate.path),
    }
}

pub async fn probe_candidates_concurrent(
    client: &reqwest::Client,
    candidates: &[CandidatePath],
) -> Vec<PathOutcome> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let mut set = tokio::task::JoinSet::new();
    for candidate in candidates.iter().cloned() {
        let client = client.clone();
        set.spawn(async move {
            let outcome = probe_candidate(&client, &candidate).await;
            (candidate.path, outcome)
        });
    }

    let timeout_fut = tokio::time::sleep(PATH_PROBE_DEADLINE);
    tokio::pin!(timeout_fut);

    let mut outcomes_map = HashMap::new();
    for c in candidates {
        outcomes_map.insert(c.path, PathOutcome::unreachable(c.path));
    }

    loop {
        tokio::select! {
            biased;
            _ = &mut timeout_fut => {
                set.abort_all();
                break;
            }
            Some(res) = set.join_next() => {
                if let Ok((path, outcome)) = res {
                    outcomes_map.insert(path, outcome);
                }
                if set.is_empty() {
                    break;
                }
            }
            else => break,
        }
    }

    candidates
        .iter()
        .map(|c| outcomes_map.get(&c.path).cloned().unwrap_or_else(|| PathOutcome::unreachable(c.path)))
        .collect()
}

#[derive(Clone, Default)]
pub struct SessionAttachOrigins {
    origins: Arc<Mutex<HashMap<String, String>>>,
}

impl SessionAttachOrigins {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, session_id: &str) -> Option<String> {
        self.origins.lock().unwrap().get(session_id).cloned()
    }

    pub fn record_if_absent(&self, session_id: &str, origin: &str) -> String {
        let mut map = self.origins.lock().unwrap();
        map.entry(session_id.to_string())
            .or_insert_with(|| origin.to_string())
            .clone()
    }

    pub fn remove(&self, session_id: &str) -> Option<String> {
        self.origins.lock().unwrap().remove(session_id)
    }
}

pub static GLOBAL_SESSION_ATTACH_ORIGINS: std::sync::LazyLock<SessionAttachOrigins> =
    std::sync::LazyLock::new(SessionAttachOrigins::new);

/// Confirms and pins the origin for a session after an attach handshake succeeds.
///
/// Must be called by the caller after the handshake returns successfully.
pub fn confirm_attach_origin(session_id: &str, origin: &str) -> String {
    GLOBAL_SESSION_ATTACH_ORIGINS.record_if_absent(session_id, origin)
}

/// Releases the pinned origin for a session.
///
/// Must be called by the caller when the attach session or stream closes.
pub fn release_attach_origin(session_id: &str) -> Option<String> {
    GLOBAL_SESSION_ATTACH_ORIGINS.remove(session_id)
}

pub async fn resolve_attach_base_origin(
    session_id: &str,
    candidates: &[CandidatePath],
    fallback_relay_origin: &str,
    client: &reqwest::Client,
) -> String {
    if let Some(existing) = GLOBAL_SESSION_ATTACH_ORIGINS.get(session_id) {
        return existing;
    }

    if candidates.is_empty() {
        return fallback_relay_origin.to_string();
    }

    let outcomes = probe_candidates_concurrent(client, candidates).await;
    let chosen_path = select_path(&outcomes);

    let winner_origin = chosen_path
        .and_then(|path| candidates.iter().find(|c| c.path == path).map(|c| c.base_origin.as_str()))
        .unwrap_or(fallback_relay_origin);

    winner_origin.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_select_picks_fastest_authenticated() {
        let outcomes = [
            PathOutcome::reachable(AttachPath::Relay, Duration::from_millis(40)),
            PathOutcome::reachable(AttachPath::SshForward, Duration::from_millis(10)),
            PathOutcome::unreachable(AttachPath::Lan),
        ];
        assert_eq!(
            select_path(&outcomes),
            Some(AttachPath::SshForward),
            "Injected RTTs of relay 40ms, SSH 10ms, LAN timeout select SSH"
        );
    }

    #[test]
    fn the_lowest_measured_round_trip_wins() {
        let outcomes = [
            PathOutcome::reachable(AttachPath::Relay, Duration::from_millis(40)),
            PathOutcome::reachable(AttachPath::Lan, Duration::from_millis(5)),
            PathOutcome::reachable(AttachPath::Tailscale, Duration::from_millis(12)),
        ];
        assert_eq!(select_path(&outcomes), Some(AttachPath::Lan));
    }

    #[test]
    fn a_tie_breaks_lan_then_tailscale_then_ssh_then_relay() {
        let same = Duration::from_millis(7);
        let all = [
            PathOutcome::reachable(AttachPath::Relay, same),
            PathOutcome::reachable(AttachPath::SshForward, same),
            PathOutcome::reachable(AttachPath::Tailscale, same),
            PathOutcome::reachable(AttachPath::Lan, same),
        ];
        assert_eq!(select_path(&all), Some(AttachPath::Lan));
        assert_eq!(select_path(&all[..3]), Some(AttachPath::Tailscale));
        assert_eq!(select_path(&all[..2]), Some(AttachPath::SshForward));
        assert_eq!(select_path(&all[..1]), Some(AttachPath::Relay));
    }

    #[test]
    fn a_candidate_without_an_authenticated_round_trip_is_not_selectable() {
        let outcomes = [
            PathOutcome::unreachable(AttachPath::Lan),
            PathOutcome::reachable(AttachPath::Relay, Duration::from_millis(90)),
        ];
        assert_eq!(select_path(&outcomes), Some(AttachPath::Relay));
        assert_eq!(
            select_path(&[PathOutcome::unreachable(AttachPath::Lan)]),
            None,
            "a health check alone must not select a path"
        );
        assert_eq!(select_path(&[]), None);
    }

    #[tokio::test]
    async fn unauthenticated_health_only_server_is_not_selected() {
        use axum::{routing::get, Json, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/api/v1/health", get(|| async { Json(serde_json::json!({"status": "ok"})) }))
            .route("/api/v1/capabilities", get(|headers: axum::http::HeaderMap| async move {
                if let Some(auth) = headers.get("authorization") {
                    if auth == "Bearer valid-token" {
                        return (axum::http::StatusCode::OK, Json(serde_json::json!({"ok": true})));
                    }
                }
                (axum::http::StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "unauthorized"})))
            }));
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let unauth_candidate = CandidatePath::new(
            AttachPath::Lan,
            format!("http://{addr}"),
            Some("bad-token".into()),
        );
        let outcome = probe_candidate(&client, &unauth_candidate).await;
        assert_eq!(
            outcome,
            PathOutcome::unreachable(AttachPath::Lan),
            "401 / unauthenticated probe must fail closed to unreachable"
        );

        let no_token_candidate = CandidatePath::new(
            AttachPath::Lan,
            format!("http://{addr}"),
            None,
        );
        let outcome_no_token = probe_candidate(&client, &no_token_candidate).await;
        assert_eq!(
            outcome_no_token,
            PathOutcome::unreachable(AttachPath::Lan),
            "Candidate without token must be unreachable"
        );

        let auth_candidate = CandidatePath::new(
            AttachPath::Lan,
            format!("http://{addr}"),
            Some("valid-token".into()),
        );
        let outcome_valid = probe_candidate(&client, &auth_candidate).await;
        assert!(outcome_valid.rtt.is_some(), "Valid token must be reachable");

        server.abort();
    }

    #[tokio::test]
    async fn open_attach_id_stays_on_relay_when_later_probe_says_ssh_is_faster() {
        use axum::{routing::get, Json, Router};
        let session_id = "session-pinned-stays";
        release_attach_origin(session_id);

        let machine_id = "machine-pinned-stays";

        // Relay mock server
        let relay_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay_addr = relay_listener.local_addr().unwrap();
        let relay_app = Router::new().route(
            "/api/v1/capabilities",
            get(|| async {
                (
                    axum::http::StatusCode::OK,
                    Json(serde_json::json!({ "machineId": "machine-pinned-stays" })),
                )
            }),
        );
        let relay_server = tokio::spawn(async move {
            axum::serve(relay_listener, relay_app).await.unwrap();
        });

        // SSH forward mock server
        let ssh_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ssh_addr = ssh_listener.local_addr().unwrap();
        let ssh_app = Router::new().route(
            "/api/v1/capabilities",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                (
                    axum::http::StatusCode::OK,
                    Json(serde_json::json!({ "machineId": "machine-pinned-stays" })),
                )
            }),
        );
        let ssh_server = tokio::spawn(async move {
            axum::serve(ssh_listener, ssh_app).await.unwrap();
        });

        let relay_origin = format!("http://{relay_addr}");
        let ssh_origin = format!("http://{ssh_addr}");
        let client = reqwest::Client::new();

        // 1. First probe setup: relay candidate is fast (0 delay), SSH candidate is slow (50ms delay).
        let candidates_relay_wins = vec![
            CandidatePath::with_expected_machine_id(
                AttachPath::Relay,
                relay_origin.clone(),
                Some("valid-token".into()),
                Some(machine_id.into()),
            ),
            CandidatePath::with_expected_machine_id(
                AttachPath::SshForward,
                ssh_origin.clone(),
                Some("valid-token".into()),
                Some(machine_id.into()),
            ),
        ];

        let chosen = resolve_attach_base_origin(
            session_id,
            &candidates_relay_wins,
            &relay_origin,
            &client,
        )
        .await;
        assert_eq!(chosen, relay_origin, "Relay should win first probe");
        confirm_attach_origin(session_id, &chosen);

        // 2. Second attach on same session id: swap candidates/setup so SSH would be faster.
        // Even if candidate list has SSH first and with 0 delay (or relay has artificially long delay),
        // resolve_attach_base_origin must return the pinned relay origin without probing.
        let slow_relay_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let slow_relay_addr = slow_relay_listener.local_addr().unwrap();
        let slow_relay_app = Router::new().route(
            "/api/v1/capabilities",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                (
                    axum::http::StatusCode::OK,
                    Json(serde_json::json!({ "machineId": "machine-pinned-stays" })),
                )
            }),
        );
        let slow_relay_server = tokio::spawn(async move {
            axum::serve(slow_relay_listener, slow_relay_app).await.unwrap();
        });

        let fast_ssh_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let fast_ssh_addr = fast_ssh_listener.local_addr().unwrap();
        let fast_ssh_app = Router::new().route(
            "/api/v1/capabilities",
            get(|| async {
                (
                    axum::http::StatusCode::OK,
                    Json(serde_json::json!({ "machineId": "machine-pinned-stays" })),
                )
            }),
        );
        let fast_ssh_server = tokio::spawn(async move {
            axum::serve(fast_ssh_listener, fast_ssh_app).await.unwrap();
        });

        let fast_ssh_origin = format!("http://{fast_ssh_addr}");
        let slow_relay_origin = format!("http://{slow_relay_addr}");

        let candidates_ssh_wins = vec![
            CandidatePath::with_expected_machine_id(
                AttachPath::SshForward,
                fast_ssh_origin.clone(),
                Some("valid-token".into()),
                Some(machine_id.into()),
            ),
            CandidatePath::with_expected_machine_id(
                AttachPath::Relay,
                slow_relay_origin.clone(),
                Some("valid-token".into()),
                Some(machine_id.into()),
            ),
        ];

        let pinned_chosen = resolve_attach_base_origin(
            session_id,
            &candidates_ssh_wins,
            &slow_relay_origin,
            &client,
        )
        .await;
        assert_eq!(
            pinned_chosen, relay_origin,
            "An open attach id must stay on the pinned relay origin even when later probe has faster SSH"
        );

        // 3. Release the origin and verify that unpinning allows the now-faster SSH origin to win.
        release_attach_origin(session_id);
        let unpinned_chosen = resolve_attach_base_origin(
            session_id,
            &candidates_ssh_wins,
            &slow_relay_origin,
            &client,
        )
        .await;
        assert_eq!(
            unpinned_chosen, fast_ssh_origin,
            "After release_attach_origin, a new choice must pick the faster SSH origin"
        );

        release_attach_origin(session_id);

        relay_server.abort();
        ssh_server.abort();
        slow_relay_server.abort();
        fast_ssh_server.abort();
    }

    #[tokio::test]
    async fn a_catch_all_200_server_without_the_expected_machine_id_is_not_selectable() {
        use axum::{routing::get, Json, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/api/v1/capabilities",
            get(|| async {
                (
                    axum::http::StatusCode::OK,
                    Json(serde_json::json!({ "machineId": "someone-else" })),
                )
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let candidate = CandidatePath::with_expected_machine_id(
            AttachPath::Lan,
            format!("http://{addr}"),
            Some("valid-token".into()),
            Some("expected-machine-123".into()),
        );
        let outcome = probe_candidate(&client, &candidate).await;
        assert_eq!(
            outcome,
            PathOutcome::unreachable(AttachPath::Lan),
            "Catch-all 200 with wrong machineId must produce unreachable"
        );

        server.abort();
    }

    #[tokio::test]
    async fn a_capabilities_response_with_the_expected_machine_id_is_selectable() {
        use axum::{routing::get, Json, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let expected_id = "target-machine-xyz";
        let app = Router::new().route(
            "/api/v1/capabilities",
            get(|| async {
                (
                    axum::http::StatusCode::OK,
                    Json(serde_json::json!({ "machineId": "target-machine-xyz" })),
                )
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let candidate = CandidatePath::with_expected_machine_id(
            AttachPath::Lan,
            format!("http://{addr}"),
            Some("valid-token".into()),
            Some(expected_id.into()),
        );
        let outcome = probe_candidate(&client, &candidate).await;
        assert!(
            outcome.rtt.is_some(),
            "Candidate with matching machineId must be reachable"
        );
        assert_eq!(outcome.path, AttachPath::Lan);

        let chosen = select_path(&[outcome]);
        assert_eq!(chosen, Some(AttachPath::Lan), "Matching candidate can win selection");

        server.abort();
    }

    #[tokio::test]
    async fn resolving_does_not_pin_an_origin_until_the_attach_is_confirmed() {
        let session_id = "test-session-pin-lifecycle-unique-01";
        release_attach_origin(session_id);

        let fallback_relay = "https://relay.example.com";
        let client = reqwest::Client::new();

        // 1. After resolve_attach_base_origin, GLOBAL_SESSION_ATTACH_ORIGINS.get(session) is None
        let resolved = resolve_attach_base_origin(session_id, &[], fallback_relay, &client).await;
        assert_eq!(resolved, fallback_relay);
        assert_eq!(
            GLOBAL_SESSION_ATTACH_ORIGINS.get(session_id),
            None,
            "resolve_attach_base_origin must not pin the origin"
        );

        // 2. After confirm_attach_origin, it is the chosen origin
        let confirmed = confirm_attach_origin(session_id, &resolved);
        assert_eq!(confirmed, fallback_relay);
        assert_eq!(
            GLOBAL_SESSION_ATTACH_ORIGINS.get(session_id),
            Some(fallback_relay.to_string()),
            "confirm_attach_origin pins the origin"
        );

        // 3. After release_attach_origin, it is None again
        let released = release_attach_origin(session_id);
        assert_eq!(released, Some(fallback_relay.to_string()));
        assert_eq!(
            GLOBAL_SESSION_ATTACH_ORIGINS.get(session_id),
            None,
            "release_attach_origin unpins the origin"
        );
    }
}
