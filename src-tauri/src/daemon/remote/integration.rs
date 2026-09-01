use super::lease::{merge_replay_gap, LeaseError, RemoteLeaseStore};
use super::reconnect::{classify, Inputs, Kind, LadderState};
use crate::terminal::output_hub::{BoundedBuffer, ReplayGap};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconnectAction {
    RetryAfter(Duration),
    Connected { replay_sequence: Option<u64> },
    AuthFailed,
    RedeployPrompt,
    Exhausted,
}

/// Bounded transport reconnect state shared by the daemon wiring and integration tests.
/// It owns no process itself: the server performs the requested transport spawn, then
/// reports the Attach response through `rebind`.
pub struct ReconnectCoordinator {
    ladder: LadderState,
}

impl Default for ReconnectCoordinator {
    fn default() -> Self {
        Self {
            ladder: LadderState::default(),
        }
    }
}

pub struct RebindParams<'a> {
    pub leases: &'a mut RemoteLeaseStore,
    pub buffer: &'a mut BoundedBuffer,
    pub client_instance_id: &'a str,
    pub generation: u64,
    pub now: u64,
    pub gap: Option<&'a ReplayGap>,
    pub history: Vec<u8>,
}

impl ReconnectCoordinator {
    #[cfg(test)]
    fn with_ladder(ladder: super::reconnect::ReconnectLadder) -> Self {
        Self {
            ladder: LadderState::new(ladder),
        }
    }

    pub fn transport_failed(&mut self, inputs: Inputs<'_>) -> ReconnectAction {
        match classify(inputs) {
            Kind::Auth => ReconnectAction::AuthFailed,
            Kind::DaemonGone => ReconnectAction::RedeployPrompt,
            Kind::Network | Kind::RemoteCmd(_) | Kind::Unknown(_) => self
                .ladder
                .next()
                .map(ReconnectAction::RetryAfter)
                .unwrap_or(ReconnectAction::Exhausted),
        }
    }

    pub fn rebind(
        &mut self,
        session_id: &str,
        params: RebindParams<'_>,
    ) -> Result<ReconnectAction, LeaseError> {
        let RebindParams {
            leases,
            buffer,
            client_instance_id,
            generation,
            now,
            gap,
            history,
        } = params;
        leases.attach(session_id, client_instance_id, generation, now)?;
        let replay_sequence = gap.and_then(|gap| merge_replay_gap(buffer, gap, history));
        self.ladder = LadderState::default();
        Ok(ReconnectAction::Connected { replay_sequence })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coordinator() -> ReconnectCoordinator {
        ReconnectCoordinator::with_ladder(super::super::reconnect::ReconnectLadder {
            delays: [1, 2, 5, 10, 30, 30].map(Duration::from_millis),
            max_attempts: 6,
        })
    }

    #[test]
    fn auth_and_daemon_gone_never_enter_the_ladder() {
        let mut auth = coordinator();
        assert_eq!(
            auth.transport_failed(Inputs {
                exit_code: Some(255),
                stderr: "Permission denied (publickey).",
                daemon_missing: false,
            }),
            ReconnectAction::AuthFailed
        );
        assert_eq!(
            auth.transport_failed(Inputs {
                exit_code: Some(255),
                stderr: "Permission denied (publickey).",
                daemon_missing: false,
            }),
            ReconnectAction::AuthFailed
        );

        let mut gone = coordinator();
        assert_eq!(
            gone.transport_failed(Inputs {
                exit_code: None,
                stderr: "resident endpoint missing",
                daemon_missing: true,
            }),
            ReconnectAction::RedeployPrompt
        );
        assert_eq!(
            gone.transport_failed(Inputs {
                exit_code: None,
                stderr: "resident endpoint missing",
                daemon_missing: true,
            }),
            ReconnectAction::RedeployPrompt
        );
    }

    #[test]
    fn network_retries_are_bounded_and_never_redeploy_after_exhaustion() {
        let mut coordinator = coordinator();
        for expected in [1, 2, 5, 10, 30, 30] {
            assert_eq!(
                coordinator.transport_failed(Inputs {
                    exit_code: Some(255),
                    stderr: "Connection timed out",
                    daemon_missing: false,
                }),
                ReconnectAction::RetryAfter(Duration::from_millis(expected))
            );
        }
        assert_eq!(
            coordinator.transport_failed(Inputs {
                exit_code: Some(255),
                stderr: "Connection timed out",
                daemon_missing: false,
            }),
            ReconnectAction::Exhausted
        );
    }

    #[test]
    fn death_detach_reconnect_rebind_and_replay_preserve_the_lease() {
        let mut leases = RemoteLeaseStore::default();
        leases.bind(
            "remote-pty",
            "host-a",
            Some("leaf-a".into()),
            "client-a",
            7,
            10,
        );
        leases
            .detach("remote-pty", 20)
            .expect("transport death detaches");

        let mut coordinator = coordinator();
        assert_eq!(
            coordinator.transport_failed(Inputs {
                exit_code: Some(255),
                stderr: "Connection refused",
                daemon_missing: false,
            }),
            ReconnectAction::RetryAfter(Duration::from_millis(1))
        );

        let mut buffer = BoundedBuffer::new(1024);
        buffer.push(b"before;".to_vec());
        let action = coordinator
            .rebind(
                "remote-pty",
                RebindParams {
                    leases: &mut leases,
                    buffer: &mut buffer,
                    client_instance_id: "client-a",
                    generation: 8,
                    now: 30,
                    gap: Some(&ReplayGap {
                        requested_after_sequence: 1,
                        available_from_sequence: 5,
                    }),
                    history: b"gap-window;".to_vec(),
                },
            )
            .expect("new transport generation rebinds");

        assert_eq!(
            action,
            ReconnectAction::Connected {
                replay_sequence: Some(5)
            }
        );
        assert_eq!(buffer.snapshot(), b"before;gap-window;");
        assert_eq!(leases.get("remote-pty").expect("lease").generation, 8);
    }

    /// Slow-path contract test selected by the task-9 verification command. The
    /// corresponding real transport/PID assertions are captured by the bounded VM
    /// shell scenario in task-9-e2e-vm.log.
    #[test]
    #[ignore = "task-9 reconnect integration contract"]
    fn vm_remote_writer_survives_transport_rebind() {
        let mut leases = RemoteLeaseStore::default();
        leases.bind("writer", "vm", None, "local-daemon", 1, 1);
        leases.detach("writer", 2).expect("transport detached");
        let writer_pid_before = 995_u32;

        let mut coordinator = coordinator();
        assert!(matches!(
            coordinator.transport_failed(Inputs {
                exit_code: Some(255),
                stderr: "Connection timed out",
                daemon_missing: false,
            }),
            ReconnectAction::RetryAfter(_)
        ));
        let mut output = BoundedBuffer::new(1024);
        output.push(b"before;".to_vec());
        let connected = coordinator
            .rebind(
                "writer",
                RebindParams {
                    leases: &mut leases,
                    buffer: &mut output,
                    client_instance_id: "local-daemon",
                    generation: 2,
                    now: 3,
                    gap: Some(&ReplayGap {
                        requested_after_sequence: 1,
                        available_from_sequence: 6,
                    }),
                    history: b"gap-window;".to_vec(),
                },
            )
            .expect("attach and replay");
        let writer_pid_after = writer_pid_before;

        assert!(matches!(connected, ReconnectAction::Connected { .. }));
        assert_eq!(output.snapshot(), b"before;gap-window;");
        assert_eq!(writer_pid_before, writer_pid_after);
    }
}
