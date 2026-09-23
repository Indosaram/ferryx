use std::time::Duration;

pub const PATH_PROBE_DEADLINE: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
}
