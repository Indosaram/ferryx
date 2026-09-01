use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Auth,
    Network,
    RemoteCmd(String),
    DaemonGone,
    Unknown(String),
}

impl Kind {
    pub fn event_kind(&self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::Network => "network",
            Self::RemoteCmd(_) => "remote-command",
            Self::DaemonGone => "daemon-gone",
            Self::Unknown(_) => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub exit_code: Option<i32>,
    pub stderr: &'a str,
    pub daemon_missing: bool,
}

pub fn classify(inputs: Inputs<'_>) -> Kind {
    if inputs.daemon_missing {
        return Kind::DaemonGone;
    }

    if inputs.exit_code == Some(255) && inputs.stderr.contains("Permission denied") {
        Kind::Auth
    } else if inputs.stderr.contains("Connection timed out")
        || inputs.stderr.contains("Connection refused")
    {
        Kind::Network
    } else if inputs.stderr.contains("command not found") {
        Kind::RemoteCmd(inputs.stderr.to_owned())
    } else {
        Kind::Unknown(inputs.stderr.to_owned())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconnectLadder {
    pub delays: [Duration; 6],
    pub max_attempts: usize,
}

impl Default for ReconnectLadder {
    fn default() -> Self {
        Self {
            delays: [1, 2, 5, 10, 30, 30].map(Duration::from_secs),
            max_attempts: 6,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LadderState {
    pub attempt: usize,
    pub next_at: Option<Duration>,
    ladder: ReconnectLadder,
}

impl Default for LadderState {
    fn default() -> Self {
        Self::new(ReconnectLadder::default())
    }
}

impl LadderState {
    pub fn new(ladder: ReconnectLadder) -> Self {
        Self {
            attempt: 0,
            next_at: None,
            ladder,
        }
    }

    pub fn next(&mut self) -> Option<Duration> {
        if self.attempt >= self.ladder.max_attempts || self.attempt >= self.ladder.delays.len() {
            self.next_at = None;
            return None;
        }

        let delay = self.ladder.delays[self.attempt];
        self.attempt += 1;
        self.next_at = Some(delay);
        Some(delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(exit_code: Option<i32>, stderr: &str, daemon_missing: bool) -> Inputs<'_> {
        Inputs {
            exit_code,
            stderr,
            daemon_missing,
        }
    }

    #[test]
    fn classifies_auth_errors() {
        assert_eq!(
            classify(inputs(Some(255), "Permission denied (publickey).", false)),
            Kind::Auth
        );
        assert_eq!(
            classify(inputs(
                Some(255),
                "user@example: Permission denied (publickey,password).",
                false,
            )),
            Kind::Auth
        );
    }

    #[test]
    fn classifies_network_errors() {
        assert_eq!(
            classify(inputs(
                Some(255),
                "ssh: connect to host example port 22: Connection timed out",
                false,
            )),
            Kind::Network
        );
        assert_eq!(
            classify(inputs(
                Some(255),
                "ssh: connect to host example port 22: Connection refused",
                false,
            )),
            Kind::Network
        );
    }

    #[test]
    fn classifies_remote_command_errors() {
        for stderr in [
            "sh: ferryx: command not found",
            "zsh:1: command not found: /home/user/.ferryx-remote/ferryx",
        ] {
            assert_eq!(
                classify(inputs(Some(127), stderr, false)),
                Kind::RemoteCmd(stderr.to_owned())
            );
        }
    }

    #[test]
    fn classifies_daemon_missing_errors() {
        assert_eq!(
            classify(inputs(Some(1), "daemon endpoint does not exist", true)),
            Kind::DaemonGone
        );
        assert_eq!(
            classify(inputs(None, "connection to resident daemon was lost", true)),
            Kind::DaemonGone
        );
    }

    #[test]
    fn classifies_unknown_errors() {
        for (exit_code, stderr) in [(Some(1), "unexpected EOF"), (None, "channel closed")] {
            assert_eq!(
                classify(inputs(exit_code, stderr, false)),
                Kind::Unknown(stderr.to_owned())
            );
        }
    }

    #[test]
    fn ladder_yields_expected_sequence_then_stops() {
        let mut state = LadderState::default();
        let delays = std::iter::from_fn(|| state.next())
            .map(|delay| delay.as_secs())
            .collect::<Vec<_>>();

        assert_eq!(delays, vec![1, 2, 5, 10, 30, 30]);
        assert_eq!(state.attempt, 6);
        assert_eq!(state.next(), None);
        assert_eq!(state.next_at, None);
    }
}
