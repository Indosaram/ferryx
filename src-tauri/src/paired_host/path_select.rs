use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
        set.spawn(async move {
            match fut.await {
                Ok((channel, rtt)) => Some(SelectedChannel {
                    path: candidate.path,
                    base_origin: candidate.base_origin,
                    channel,
                    rtt,
                }),
                Err(_) => None,
            }
        });
    }

    let timeout_fut = tokio::time::sleep(deadline);
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
                if let Ok(Some(channel)) = res {
                    let is_better = match &winner {
                        None => true,
                        Some(current) => (channel.rtt, channel.path.tie_break_rank()) < (current.rtt, current.path.tie_break_rank()),
                    };
                    if is_better {
                        winner = Some(channel);
                        set.abort_all();
                        break;
                    }
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

pub fn resolve_attach_base_origin(
    session_id: &str,
    candidates: &[CandidatePath],
    fallback_relay_origin: &str,
) -> String {
    if let Some(existing) = GLOBAL_SESSION_ATTACH_ORIGINS.get(session_id) {
        return existing;
    }
    candidates
        .first()
        .map(|c| c.base_origin.as_str())
        .unwrap_or(fallback_relay_origin)
        .to_string()
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
                        AttachPath::Tailscale => {
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
