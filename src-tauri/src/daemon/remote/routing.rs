use crate::ssh::RemoteContinuity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnRoute {
    LocalSshPty,
    ResidentDaemon,
    DeployThenResidentDaemon,
    EnableContinuity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnDispatch {
    pub route: SpawnRoute,
    pub spawn_local_pty: bool,
    pub deploy: bool,
    pub start_session: bool,
    pub emit_enable_action: bool,
}

/// Production policy boundary consumed by `DaemonServer::handle_spawn`.
/// Keeping the complete dispatch result here makes all side-effect choices
/// testable without executing SSH or deploying a binary.
pub fn dispatch_spawn(continuity: RemoteContinuity, daemon_live: bool) -> SpawnDispatch {
    match (continuity, daemon_live) {
        (RemoteContinuity::Off, _) => SpawnDispatch {
            route: SpawnRoute::LocalSshPty,
            spawn_local_pty: true,
            deploy: false,
            start_session: false,
            emit_enable_action: false,
        },
        (RemoteContinuity::Auto | RemoteContinuity::On, true) => SpawnDispatch {
            route: SpawnRoute::ResidentDaemon,
            spawn_local_pty: false,
            deploy: false,
            start_session: true,
            emit_enable_action: false,
        },
        (RemoteContinuity::On, false) => SpawnDispatch {
            route: SpawnRoute::DeployThenResidentDaemon,
            spawn_local_pty: false,
            deploy: true,
            start_session: true,
            emit_enable_action: false,
        },
        (RemoteContinuity::Auto, false) => SpawnDispatch {
            route: SpawnRoute::EnableContinuity,
            spawn_local_pty: true,
            deploy: false,
            start_session: false,
            emit_enable_action: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_with_live_daemon_dispatches_local_pty() {
        assert_eq!(
            dispatch_spawn(RemoteContinuity::Off, true),
            SpawnDispatch {
                route: SpawnRoute::LocalSshPty,
                spawn_local_pty: true,
                deploy: false,
                start_session: false,
                emit_enable_action: false,
            }
        );
    }

    #[test]
    fn on_with_live_daemon_dispatches_transport_start_session() {
        assert_eq!(
            dispatch_spawn(RemoteContinuity::On, true),
            SpawnDispatch {
                route: SpawnRoute::ResidentDaemon,
                spawn_local_pty: false,
                deploy: false,
                start_session: true,
                emit_enable_action: false,
            }
        );
    }

    #[test]
    fn on_without_daemon_dispatches_deploy_then_start_session() {
        assert_eq!(
            dispatch_spawn(RemoteContinuity::On, false),
            SpawnDispatch {
                route: SpawnRoute::DeployThenResidentDaemon,
                spawn_local_pty: false,
                deploy: true,
                start_session: true,
                emit_enable_action: false,
            }
        );
    }

    #[test]
    fn auto_without_daemon_dispatches_enable_event_and_local_pty_only() {
        assert_eq!(
            dispatch_spawn(RemoteContinuity::Auto, false),
            SpawnDispatch {
                route: SpawnRoute::EnableContinuity,
                spawn_local_pty: true,
                deploy: false,
                start_session: false,
                emit_enable_action: true,
            }
        );
    }
}
