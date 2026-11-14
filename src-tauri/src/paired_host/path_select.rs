use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const PATH_PROBE_DEADLINE: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AttachPath {
    Lan,
    Direct,
    Tailscale,
    SshForward,
    Relay,
}

impl AttachPath {
    pub fn tie_break_rank(self) -> u8 {
        match self {
            AttachPath::Lan => 0,
            AttachPath::Direct => 1,
            AttachPath::Tailscale => 2,
            AttachPath::SshForward => 3,
            AttachPath::Relay => 4,
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
    /// How long THIS candidate may take before it is given up on. Direct routes are probes that
    /// should answer quickly; a relay is only reachable through a cold handshake that legitimately
    /// takes longer. Without a per-candidate budget the short probe deadline applied to both, so a
    /// stale direct route could make a host unattachable over a relay that would have answered.
    pub probe_budget: Option<Duration>,
}

impl CandidatePath {
    pub fn new(path: AttachPath, base_origin: impl Into<String>, auth_token: Option<String>) -> Self {
        Self {
            path,
            base_origin: base_origin.into(),
            auth_token,
            expected_machine_id: None,
            probe_budget: None,
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
            probe_budget: None,
        }
    }
}

#[derive(Debug)]
pub struct SelectedChannel<T> {
    pub path: AttachPath,
    pub base_origin: String,
    pub channel: T,
    pub rtt: Duration,
}

pub async fn select_and_reuse_channel<T, F, Fut>(
    candidates: &[CandidatePath],
    deadline: Duration,
    connect: F,
) -> Option<SelectedChannel<T>>
where
    F: Fn(CandidatePath) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<(T, Duration), ()>> + Send + 'static,
    T: Send + 'static,
{
    if candidates.is_empty() {
        return None;
    }

    let mut set = tokio::task::JoinSet::new();
    for candidate in candidates.iter().cloned() {
        let fut = connect(candidate.clone());
        let path = candidate.path;
        let budget = candidate.probe_budget.unwrap_or(deadline);
        set.spawn(async move {
            // A candidate with a longer budget is allowed to outlive the shared probe deadline;
            // the select loop keeps waiting for it while the short-budget candidates are gone.
            let outcome = match tokio::time::timeout(budget, fut).await {
                Ok(result) => result,
                Err(_) => Err(()),
            };
            match outcome {
                Ok((channel, rtt)) => Some(SelectedChannel {
                    path,
                    base_origin: candidate.base_origin,
                    channel,
                    rtt,
                }),
                Err(_) => None,
            }
        });
    }

    // The whole race waits for the LONGEST candidate budget: each task already enforces its own
    // budget internally, so a short-budget candidate reports its failure on time while a candidate
    // that is allowed more time is still running. Aborting everything at the shortest deadline is
    // what denied the relay the establishment budget it needs.
    let overall = candidates
        .iter()
        .map(|candidate| candidate.probe_budget.unwrap_or(deadline))
        .max()
        .unwrap_or(deadline);
    let timeout_fut = tokio::time::sleep(overall);
    tokio::pin!(timeout_fut);

    let mut winner: Option<SelectedChannel<T>> = None;

    loop {
        tokio::select! {
            biased;
            _ = &mut timeout_fut => {
                set.abort_all();
                break;
            }
            Some(res) = set.join_next() => {
                // FIRST SUCCESS WINS. The candidate that just completed a handshake is the only
                // route PROVEN reachable right now, and the channel it produced is consumed by
                // the win. Holding that channel open to compare RTTs against a slower route would
                // trade a working channel for one that may still fail, so RTT is carried on
                // SelectedChannel for the caller's telemetry only - it is not a selection key.
                // (The previous code compared RTTs and then broke immediately in the same arm,
                // which made the comparison unreachable and the contract look like best-RTT.)
                if let Ok(Some(channel)) = res {
                    winner = Some(channel);
                    set.abort_all();
                    break;
                }
                if set.is_empty() {
                    break;
                }
            }
            else => break,
        }
    }

    winner
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

pub fn confirm_attach_origin(session_id: &str, origin: &str) -> String {
    GLOBAL_SESSION_ATTACH_ORIGINS.record_if_absent(session_id, origin)
}

pub fn release_attach_origin(session_id: &str) -> Option<String> {
    GLOBAL_SESSION_ATTACH_ORIGINS.remove(session_id)
}

/// A verified direct route for `host_id`, if one is live; never probes.
pub fn direct_candidate(
    routes: &super::direct_route::DirectRoutes,
    host_id: &str,
    auth_token: Option<String>,
    expected_machine_id: Option<String>,
) -> Option<CandidatePath> {
    routes.origin_for(host_id).map(|origin| {
        CandidatePath::with_expected_machine_id(AttachPath::Direct, origin, auth_token, expected_machine_id)
    })
}

pub fn resolve_attach_base_origin(
    session_id: &str,
    candidates: &[CandidatePath],
    machine_id: &str,
    fallback_relay_origin: &str,
) -> String {
    // A candidate naming a DIFFERENT machine is never chosen, whatever its position: a stale
    // paired record must not steer this client onto another host. Defence in depth, not the
    // primary binding: today's builder stamps every candidate with the same recorded id, so what
    // really pins the machine is the id in the Noise prologue. This filter is kept so a future
    // caller that mixes candidates cannot silently dial a foreign route.
    let matches_this_machine = |candidate: &&CandidatePath| {
        candidate
            .expected_machine_id
            .as_deref()
            .is_none_or(|expected| expected == machine_id)
    };
    // A pinned origin is reused only when it is one of THIS machine's candidates. Otherwise a
    // reused session id, whose earlier allocation pinned a foreign route, would be sent there and
    // the filter below would never run.
    if let Some(existing) = GLOBAL_SESSION_ATTACH_ORIGINS.get(session_id) {
        if candidates
            .iter()
            .filter(matches_this_machine)
            .any(|candidate| candidate.base_origin == existing)
        {
            return existing;
        }
    }
    // The relay is the only origin guaranteed to reach the machine: a direct route that just lost
    // the race is exactly the one most likely to be stale, so a blind `first()` fallback would
    // retry the failed path instead of the working one.
    candidates
        .iter()
        .filter(matches_this_machine)
        .find(|c| c.path == AttachPath::Relay)
        .or_else(|| candidates.iter().find(|candidate| matches_this_machine(candidate)))
        .map(|c| c.base_origin.as_str())
        .unwrap_or(fallback_relay_origin)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_candidate_naming_another_machine_is_rejected_before_it_connects() {
        let candidate = CandidatePath::with_expected_machine_id(
            AttachPath::Direct,
            "http://127.0.0.1:1",
            None,
            "machine-a".into(),
        );
        assert!(super::super::attach::candidate_names_expected_machine_for_test(
            &candidate,
            "machine-a"
        ));
        assert!(!super::super::attach::candidate_names_expected_machine_for_test(
            &candidate,
            "machine-b"
        ));
    }

    #[test]
    fn a_candidate_without_an_identity_claim_is_still_eligible() {
        let candidate = CandidatePath::new(AttachPath::Relay, "https://relay.test", None);
        assert!(super::super::attach::candidate_names_expected_machine_for_test(
            &candidate,
            "any-machine"
        ));
    }

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
        let same_rtt = Duration::from_millis(15);

        let outcomes_all = [
            PathOutcome::reachable(AttachPath::Relay, same_rtt),
            PathOutcome::reachable(AttachPath::Tailscale, same_rtt),
            PathOutcome::reachable(AttachPath::Lan, same_rtt),
            PathOutcome::reachable(AttachPath::SshForward, same_rtt),
        ];
        assert_eq!(select_path(&outcomes_all), Some(AttachPath::Lan));

        let outcomes_no_lan = [
            PathOutcome::reachable(AttachPath::Relay, same_rtt),
            PathOutcome::reachable(AttachPath::SshForward, same_rtt),
            PathOutcome::reachable(AttachPath::Tailscale, same_rtt),
        ];
        assert_eq!(select_path(&outcomes_no_lan), Some(AttachPath::Tailscale));

        let outcomes_ssh_relay = [
            PathOutcome::reachable(AttachPath::Relay, same_rtt),
            PathOutcome::reachable(AttachPath::SshForward, same_rtt),
        ];
        assert_eq!(select_path(&outcomes_ssh_relay), Some(AttachPath::SshForward));
    }

    #[test]
    fn a_candidate_without_an_authenticated_round_trip_is_not_selectable() {
        let outcomes = [
            PathOutcome::unreachable(AttachPath::Lan),
            PathOutcome::reachable(AttachPath::Relay, Duration::from_millis(90)),
        ];
        assert_eq!(select_path(&outcomes), Some(AttachPath::Relay));

        let health_only_outcomes = [
            PathOutcome::unreachable(AttachPath::Lan),
        ];
        assert_eq!(
            select_path(&health_only_outcomes),
            None,
            "a health check alone must not select a path"
        );

        let empty: [PathOutcome; 0] = [];
        assert_eq!(select_path(&empty), None);
    }

    #[test]
    fn direct_ranks_between_lan_and_tailscale_on_equal_rtt() {
        let same = Duration::from_millis(15);
        let with_lan = [
            PathOutcome::reachable(AttachPath::Relay, same),
            PathOutcome::reachable(AttachPath::Direct, same),
            PathOutcome::reachable(AttachPath::Lan, same),
        ];
        assert_eq!(select_path(&with_lan), Some(AttachPath::Lan));
        let without_lan = [
            PathOutcome::reachable(AttachPath::Relay, same),
            PathOutcome::reachable(AttachPath::Tailscale, same),
            PathOutcome::reachable(AttachPath::Direct, same),
        ];
        assert_eq!(select_path(&without_lan), Some(AttachPath::Direct));
    }

    fn direct_and_relay() -> Vec<CandidatePath> {
        vec![
            CandidatePath::new(AttachPath::Direct, "http://127.0.0.1:1", Some("t".into())),
            CandidatePath::new(AttachPath::Relay, "https://relay.example.com", Some("t".into())),
        ]
    }

    #[tokio::test]
    async fn direct_failure_selects_relay() {
        let winner = select_and_reuse_channel(&direct_and_relay(), PATH_PROBE_DEADLINE, |c| async move {
            match c.path {
                AttachPath::Relay => Ok((c.path, Duration::from_millis(40))),
                _ => Err(()),
            }
        })
        .await
        .expect("relay remains available");
        assert_eq!(winner.path, AttachPath::Relay);
    }

    #[tokio::test]
    async fn a_direct_attempt_that_never_answers_cannot_block_relay() {
        let winner = select_and_reuse_channel(&direct_and_relay(), PATH_PROBE_DEADLINE, |c| async move {
            match c.path {
                AttachPath::Relay => Ok((c.path, Duration::from_millis(40))),
                _ => std::future::pending().await,
            }
        })
        .await
        .expect("relay wins while direct hangs");
        assert_eq!(winner.path, AttachPath::Relay);

        let only_direct = vec![direct_and_relay().remove(0)];
        let none = select_and_reuse_channel(&only_direct, Duration::from_millis(20), |_| async move {
            std::future::pending::<Result<(AttachPath, Duration), ()>>().await
        })
        .await;
        assert!(none.is_none(), "a hung direct path is bounded by the probe deadline");
    }

    #[tokio::test(start_paused = true)]
    async fn reachable_direct_is_preferred_over_a_slower_relay() {
        // Paused clock: each connect completes exactly at its simulated latency.
        let winner = select_and_reuse_channel(&direct_and_relay(), PATH_PROBE_DEADLINE, |c| async move {
            let latency = match c.path {
                AttachPath::Direct => Duration::from_millis(5),
                _ => Duration::from_millis(40),
            };
            tokio::time::sleep(latency).await;
            Ok((c.path, latency))
        })
        .await
        .expect("direct wins");
        assert_eq!(winner.path, AttachPath::Direct);
    }

    #[test]
    fn direct_candidate_only_exists_for_a_published_route() {
        use super::super::direct_route::{DirectGuard, DirectRoutes};
        let routes = DirectRoutes::new();
        assert_eq!(direct_candidate(&routes, "h", None, None), None);
        let epoch = routes.begin_attempt("h", std::time::Instant::now()).unwrap();
        routes.publish("h", epoch, "http://127.0.0.1:7".into(), DirectGuard::default());
        let candidate = direct_candidate(&routes, "h", None, Some("m".into())).unwrap();
        assert_eq!(candidate.path, AttachPath::Direct);
        assert_eq!(candidate.base_origin, "http://127.0.0.1:7");
        assert_eq!(candidate.expected_machine_id.as_deref(), Some("m"));
    }

    /// A candidate that names another machine must never be dialled: the client would complete a
    /// handshake with a host it did not mean to reach. It is dropped, not failed, so the relay
    /// candidate in the same set still attaches.
    /// The accept-side gate: a candidate that names another machine must not be chosen even when
    /// it is the only direct route and even when it sorts first. The candidates here are hand-built:
    /// production currently stamps every candidate with the same recorded id, so this pins the
    /// contract of the filter rather than reproducing a sequence the real builder emits.
    #[test]
    fn a_candidate_for_another_machine_is_never_chosen() {
        let session_id = "session-accept-side-machine-pin";
        release_attach_origin(session_id);

        let candidates = vec![
            CandidatePath::with_expected_machine_id(
                AttachPath::Direct,
                "http://127.0.0.1:43821",
                Some("token".into()),
                Some("a-different-machine".into()),
            ),
            CandidatePath::with_expected_machine_id(
                AttachPath::Relay,
                "https://relay.example.com",
                Some("token".into()),
                Some("a-different-machine".into()),
            ),
        ];

        let origin = resolve_attach_base_origin(
            session_id,
            &candidates,
            "the-machine-this-client-paired-with",
            "https://fallback.example.com",
        );

        assert_eq!(
            origin, "https://fallback.example.com",
            "every candidate names another machine, so the relay fallback is the only safe origin"
        );
    }

    /// The same gate with a usable relay present: the other machine's direct route is skipped and
    /// the relay that names THIS machine wins.
    #[test]
    fn a_matching_relay_wins_over_a_foreign_direct_route() {
        let session_id = "session-accept-side-machine-pin-relay";
        release_attach_origin(session_id);

        let candidates = vec![
            CandidatePath::with_expected_machine_id(
                AttachPath::Direct,
                "http://127.0.0.1:43821",
                Some("token".into()),
                Some("a-different-machine".into()),
            ),
            CandidatePath::with_expected_machine_id(
                AttachPath::Relay,
                "https://relay.example.com",
                Some("token".into()),
                Some("this-machine".into()),
            ),
        ];

        let origin = resolve_attach_base_origin(
            session_id,
            &candidates,
            "this-machine",
            "https://fallback.example.com",
        );

        assert_eq!(
            origin, "https://relay.example.com",
            "the foreign direct route is skipped, the matching relay is used"
        );
    }

    #[test]
    fn candidates_naming_another_machine_are_dropped() {
        let candidates = vec![
            CandidatePath::with_expected_machine_id(
                AttachPath::Direct,
                "http://127.0.0.1:7",
                Some("t".into()),
                Some("other-machine".into()),
            ),
            CandidatePath::with_expected_machine_id(
                AttachPath::Relay,
                "https://relay.example.com",
                Some("t".into()),
                Some("this-machine".into()),
            ),
            CandidatePath::new(AttachPath::Lan, "http://192.168.1.9:7", Some("t".into())),
        ];

        let eligible = super::super::attach::eligible_candidates(&candidates, "this-machine");

        assert_eq!(eligible.len(), 2, "the foreign-machine route is dropped");
        assert!(eligible.iter().all(|c| c.expected_machine_id.as_deref() != Some("other-machine")));
        assert!(eligible.iter().any(|c| c.path == AttachPath::Relay));
        assert!(
            eligible.iter().any(|c| c.path == AttachPath::Lan),
            "a candidate with no pinned machine stays eligible"
        );
    }

    #[test]
    fn a_pinned_origin_for_another_machine_is_not_reused() {
        let session = format!("pinned-foreign-{}", uuid::Uuid::new_v4());
        super::confirm_attach_origin(&session, "https://foreign.example.com");
        let candidates = vec![CandidatePath::with_expected_machine_id(
            AttachPath::Relay,
            "https://relay.this-machine.example",
            Some("t".into()),
            Some("this-machine".into()),
        )];

        let origin =
            super::resolve_attach_base_origin(&session, &candidates, "this-machine", "https://fallback");

        assert_eq!(
            origin, "https://relay.this-machine.example",
            "an origin pinned by an earlier allocation for another machine must not be dialled"
        );
    }

    #[test]
    fn a_pinned_origin_that_is_this_machines_candidate_is_reused() {
        let session = format!("pinned-own-{}", uuid::Uuid::new_v4());
        super::confirm_attach_origin(&session, "https://relay.this-machine.example");
        let candidates = vec![CandidatePath::with_expected_machine_id(
            AttachPath::Relay,
            "https://relay.this-machine.example",
            Some("t".into()),
            Some("this-machine".into()),
        )];

        assert_eq!(
            super::resolve_attach_base_origin(&session, &candidates, "this-machine", "https://fallback"),
            "https://relay.this-machine.example",
            "a pin that still matches this machine's route is kept"
        );
    }

    /// The relay is the only origin guaranteed to reach the machine, so the fallback for a session
    /// that has not been pinned yet must never be a direct route that just failed the race.
    #[test]
    fn an_unpinned_fallback_prefers_the_relay_over_a_stale_direct_route() {
        let session_id = "session-unpinned-fallback-prefers-relay";
        release_attach_origin(session_id);

        let candidates = vec![
            CandidatePath::new(AttachPath::Direct, "http://127.0.0.1:43821", Some("token".into())),
            CandidatePath::new(AttachPath::Relay, "https://relay.example.com", Some("token".into())),
        ];

        let origin = resolve_attach_base_origin(
            session_id,
            &candidates,
            "machine-this-client-is",
            "https://fallback.example.com",
        );

        assert_eq!(
            origin, "https://relay.example.com",
            "a blind first() fallback would retry the direct route that just lost"
        );
        release_attach_origin(session_id);
    }

    #[tokio::test]
    async fn open_attach_id_stays_on_relay_when_later_probe_says_ssh_is_faster() {
        let session_id = "session-pinned-stays";
        release_attach_origin(session_id);

        let relay_origin = "https://relay.example.com";
        let ssh_origin = "http://127.0.0.1:43821";

        let candidates = vec![
            CandidatePath::new(AttachPath::Relay, relay_origin, Some("token".into())),
            CandidatePath::new(AttachPath::SshForward, ssh_origin, Some("token".into())),
        ];

        let winner = select_and_reuse_channel(
            &candidates,
            Duration::from_secs(3),
            |c| async move {
                let delay = if c.path == AttachPath::Relay { 10 } else { 50 };
                tokio::time::sleep(Duration::from_millis(delay)).await;
                Ok((c.base_origin.clone(), Duration::from_millis(delay)))
            },
        )
        .await
        .expect("relay should win");
        assert_eq!(winner.path, AttachPath::Relay);
        confirm_attach_origin(session_id, &winner.base_origin);

        assert_eq!(
            GLOBAL_SESSION_ATTACH_ORIGINS.get(session_id),
            Some(relay_origin.to_string()),
            "Pinned attach stays on relay origin"
        );

        release_attach_origin(session_id);
        let new_winner = select_and_reuse_channel(
            &candidates,
            Duration::from_secs(3),
            |c| async move {
                let delay = if c.path == AttachPath::SshForward { 10 } else { 50 };
                tokio::time::sleep(Duration::from_millis(delay)).await;
                Ok((c.base_origin.clone(), Duration::from_millis(delay)))
            },
        )
        .await
        .expect("ssh should win");
        assert_eq!(new_winner.path, AttachPath::SshForward);
        confirm_attach_origin(session_id, &new_winner.base_origin);
        assert_eq!(
            GLOBAL_SESSION_ATTACH_ORIGINS.get(session_id),
            Some(ssh_origin.to_string())
        );

        release_attach_origin(session_id);
    }

    #[tokio::test]
    async fn resolving_does_not_pin_an_origin_until_the_attach_is_confirmed() {
        let session_id = "test-session-pin-lifecycle-unique-01";
        release_attach_origin(session_id);

        let fallback_relay = "https://relay.example.com";

        assert_eq!(
            GLOBAL_SESSION_ATTACH_ORIGINS.get(session_id),
            None,
            "origin is not pinned initially"
        );

        let confirmed = confirm_attach_origin(session_id, fallback_relay);
        assert_eq!(confirmed, fallback_relay);
        assert_eq!(
            GLOBAL_SESSION_ATTACH_ORIGINS.get(session_id),
            Some(fallback_relay.to_string()),
            "confirm_attach_origin pins the origin"
        );

        let released = release_attach_origin(session_id);
        assert_eq!(released, Some(fallback_relay.to_string()));
        assert_eq!(
            GLOBAL_SESSION_ATTACH_ORIGINS.get(session_id),
            None,
            "release_attach_origin unpins the origin"
        );
    }

    #[tokio::test]
    async fn winning_channel_is_reused_and_losers_are_cancelled_and_closed() {
        let relay_closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let lan_closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ssh_closed = Arc::new(std::sync::atomic::AtomicBool::new(false));

        struct TrackedChannel {
            name: &'static str,
            closed: Arc<std::sync::atomic::AtomicBool>,
        }
        impl Drop for TrackedChannel {
            fn drop(&mut self) {
                self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }

        let candidates = vec![
            CandidatePath::new(AttachPath::Relay, "https://relay.example.com", Some("token".into())),
            CandidatePath::new(AttachPath::SshForward, "http://127.0.0.1:43821", Some("token".into())),
            CandidatePath::new(AttachPath::Lan, "http://192.168.1.50:43821", Some("token".into())),
        ];

        let r_closed = Arc::clone(&relay_closed);
        let s_closed = Arc::clone(&ssh_closed);
        let l_closed = Arc::clone(&lan_closed);

        let selected = select_and_reuse_channel(
            &candidates,
            Duration::from_secs(3),
            move |candidate| {
                let r_c = Arc::clone(&r_closed);
                let s_c = Arc::clone(&s_closed);
                let l_c = Arc::clone(&l_closed);
                async move {
                    match candidate.path {
                        AttachPath::Relay => {
                            let ch = TrackedChannel { name: "relay", closed: r_c };
                            tokio::time::sleep(Duration::from_millis(40)).await;
                            Ok((ch, Duration::from_millis(40)))
                        }
                        AttachPath::SshForward => {
                            let ch = TrackedChannel { name: "ssh", closed: s_c };
                            tokio::time::sleep(Duration::from_millis(10)).await;
                            Ok((ch, Duration::from_millis(10)))
                        }
                        AttachPath::Lan => {
                            let ch = TrackedChannel { name: "lan", closed: l_c };
                            tokio::time::sleep(Duration::from_secs(4)).await;
                            Ok((ch, Duration::from_secs(4)))
                        }
                        AttachPath::Tailscale | AttachPath::Direct => {
                            tokio::time::sleep(Duration::from_secs(4)).await;
                            Err(())
                        }
                    }
                }
            },
        ).await;

        assert!(selected.is_some(), "Winning channel must be selected");
        let winner = selected.unwrap();
        assert_eq!(winner.path, AttachPath::SshForward, "Fastest path (SSH 10ms) must win");
        assert_eq!(winner.channel.name, "ssh", "Winning channel must be reused");
        assert!(!ssh_closed.load(std::sync::atomic::Ordering::SeqCst), "Winning channel must NOT be closed while in use");

        tokio::time::sleep(Duration::from_millis(15)).await;

        assert!(relay_closed.load(std::sync::atomic::Ordering::SeqCst), "Losing relay task must be cancelled/closed");
        assert!(lan_closed.load(std::sync::atomic::Ordering::SeqCst), "Losing LAN task must be cancelled/closed");

        drop(winner);
        assert!(ssh_closed.load(std::sync::atomic::Ordering::SeqCst), "Winning channel closes on drop");
    }
}
