//! The two typed refusals a tunnel request can receive before any runtime exists,
//! and the argv pin that keeps the probe paths from ever growing a forwarding.
//!
//! Both refusals name BOTH halves: `code` is always an `IpcErrorCode`, and the
//! tunnel classification travels in `details`, never in `code`. The two routes
//! carry different `details` keys on purpose - the platform refusal is a start's
//! `failureCode` (it is a `TunnelFailureCode` verdict), while a route refusal is
//! a `reason` (it is not a tunnel state at all). One string for both would make
//! "which layer refused me?" unanswerable from the wire.
//!
//! There are exactly TWO predicates and they are different types - do not
//! collapse them:
//! 1. the daemon-side local-transport predicate,
//!    `daemon::server::is_local_transport`, which this module calls with a real
//!    `daemon::server::ConnectionTransport` peer. The daemon then answers that
//!    peer `local_only` and never advertises `sshTunnelV1` to it;
//! 2. the GUI-side gateway check, `ipc::ssh_tunnel::Refusal`, which never sees a
//!    peer at all because the remote-gateway route is decided in the IPC layer,
//!    and which refuses with `remote_gateway_route`.
//!
//! The argv half of this module is a pin, not a feature: `ssh_plan`,
//! `master_args` and `client_reuse_args` are probe paths that must stay
//! forwarding-free (MH-9/IS-4), while the interactive Unix-socket `-R`
//! agent-state forward is deliberate and stays exactly as it is.

#[cfg(test)]
mod tests {
    use crate::daemon::protocol::{DaemonResponse, SSH_TUNNEL_CAPABILITY, TunnelStartRequest};
    use crate::daemon::server::{ConnectionTransport, Target, capability_vector, is_local_transport};
    use crate::daemon::tunnel_dispatch::{TunnelDispatch, TunnelPaths};
    use crate::ipc::IpcErrorCode;
    use crate::ipc::ssh_tunnel::Refusal;
    use crate::ssh::master::{ConnectionKey, client_reuse_args, master_args};
    use crate::ssh::tunnel::{TunnelSpec, TunnelSpecId};
    use crate::ssh::{SshAuthMethod, SshHost, SshHostSource};
    use std::path::Path;

    fn host() -> SshHost {
        SshHost {
            id: "host-one".into(),
            label: "Test host".into(),
            hostname: "example.test".into(),
            username: Some("user".into()),
            port: Some(2200),
            // `validate_host` requires an absolute identity path on the host OS, so
            // the fixture has to be absolute on Windows too or the probe test would
            // only ever build on Unix.
            identity_file: Some(if cfg!(windows) {
                r"C:\keys\id".to_string()
            } else {
                "/keys/id".to_string()
            }),
            jump_host: None,
            source: SshHostSource::Manual,
            auth_method: SshAuthMethod::Key,
            disabled: None,
            shell: None,
        }
    }

    fn spec() -> TunnelSpec {
        TunnelSpec {
            id: TunnelSpecId("spec-1".into()),
            label: "db".into(),
            local_port: Some(15432),
            remote_host: "127.0.0.1".into(),
            remote_port: 5432,
            enabled: true,
        }
    }

    fn forward_flags(args: &[String]) -> Vec<&str> {
        args.iter()
            .filter(|arg| matches!(arg.as_str(), "-L" | "-R" | "-D"))
            .map(String::as_str)
            .collect()
    }

    /// Case (a): on Windows a start is refused before anything exists. The
    /// classification is a `failureCode` (a tunnel verdict), the code is
    /// `UNSUPPORTED`, the message names the missing control socket, and no child
    /// and no registry ever come into being — so neither the definitions file nor
    /// the runtimes sidecar is created.
    #[cfg(windows)]
    #[tokio::test]
    async fn windows_start_refuses_unsupported_platform() {
        let directory = tempfile::tempdir().unwrap();
        let definitions = directory.path().join("ssh_tunnels.json");
        let hosts = directory.path().join("ssh_hosts.json");
        std::fs::write(&hosts, br#"{"hosts":[]}"#).unwrap();
        let dispatch = TunnelDispatch::new(TunnelPaths::new(definitions.clone(), hosts));

        let request = TunnelStartRequest {
            host_id: "host-one".into(),
            spec: Some(spec()),
            spec_id: None,
            host_revision: Some(0),
        };
        let (code, details) = match dispatch.start(request, true).await {
            DaemonResponse::Error { code, details, .. } => (code, details),
            other => panic!("a Windows start must be refused, got {other:?}"),
        };
        assert_eq!(code.as_deref(), Some("UNSUPPORTED"));
        let details = details.expect("a typed refusal carries details");
        assert_eq!(
            details["failureCode"],
            crate::ipc::ssh_tunnel::UNSUPPORTED_PLATFORM,
            "the classification is the Windows TunnelFailureCode, not a bare reason"
        );
        assert_eq!(details["platform"], "windows");
        // The missing facility is a machine value, not prose: the registry's
        // process-group reaping and the runtimes sidecar both need the control
        // socket this platform does not have.
        assert_eq!(details["missingFacility"], "control_socket");
        // No child was spawned, so no runtime record and no sidecar exist; and the
        // refusal precedes the store, so the definitions file was never created.
        assert!(!definitions.exists(), "a refused start writes no definition");
        assert!(
            !directory.path().join("ssh_tunnel_runtimes.json").exists(),
            "a refused start spawns no child, so no runtime record exists"
        );
        assert!(
            !directory.path().join("ssh_tunnel_runtimes.lock").exists(),
            "a refused start opens no runtimes sidecar"
        );
        // Windows must not advertise the capability at either advertisement site,
        // because advertising what the platform then refuses is worse than silence.
        for local in [true, false] {
            assert!(
                !capability_vector(local, Target::Windows).contains(&SSH_TUNNEL_CAPABILITY),
                "Windows advertises no tunnel capability"
            );
        }
    }

    /// Case (b): the paired route is not the local one, and the two layers that
    /// decide it use different reasons. Predicate (1) is the daemon's, called here
    /// with a real peer; predicate (2) is the GUI's gateway check.
    #[tokio::test]
    async fn paired_route_is_not_local() {
        // (1) The daemon-side predicate, passed a peer. A paired peer is not local,
        // and that is precisely what removes the capability from the vector.
        let paired = ConnectionTransport::Paired;
        assert!(!is_local_transport(paired), "a paired connection is not the local socket");
        assert!(is_local_transport(ConnectionTransport::Local));
        assert!(
            !capability_vector(is_local_transport(paired), Target::Unix)
                .contains(&SSH_TUNNEL_CAPABILITY),
            "a paired peer must never be told the tunnel capability exists"
        );
        assert!(
            capability_vector(is_local_transport(ConnectionTransport::Local), Target::Unix)
                .contains(&SSH_TUNNEL_CAPABILITY),
            "the local owner socket is the one transport that advertises it"
        );

        // The daemon's own answer to that same peer: `UNSUPPORTED` + `local_only`.
        let directory = tempfile::tempdir().unwrap();
        let definitions = directory.path().join("ssh_tunnels.json");
        let hosts = directory.path().join("ssh_hosts.json");
        std::fs::write(&hosts, br#"{"hosts":[]}"#).unwrap();
        let dispatch = TunnelDispatch::new(TunnelPaths::new(definitions.clone(), hosts));
        let request = TunnelStartRequest {
            host_id: "host-one".into(),
            spec: Some(spec()),
            spec_id: None,
            host_revision: Some(0),
        };
        let (code, details) = match dispatch.start(request, is_local_transport(paired)).await {
            DaemonResponse::Error { code, details, .. } => (code, details),
            other => panic!("a paired start must be refused, got {other:?}"),
        };
        assert_eq!(code.as_deref(), Some("UNSUPPORTED"));
        let details = details.expect("a typed refusal carries details");
        assert_eq!(details["reason"], "local_only");
        assert!(
            !definitions.exists(),
            "a peer's request never reaches the store writer"
        );

        // (2) The GUI-side gateway check. It sees no peer: the route is its input.
        let gateway = Refusal::RemoteGatewayRoute.into_error();
        assert_eq!(gateway.code, IpcErrorCode::Unsupported);
        assert_eq!(
            gateway.details.as_ref().unwrap()["reason"],
            "remote_gateway_route"
        );
        // The two routes have different reasons and must not share one string.
        assert_ne!(
            details["reason"],
            gateway.details.as_ref().unwrap()["reason"],
            "the paired route and the remote-gateway route are different refusals"
        );
    }

    /// The argv pin: the probe paths stay forwarding-free and the interactive
    /// agent-state `-R` stays exactly as it was.
    #[test]
    fn probe_argv_carries_no_forwarding() {
        // Non-interactive `ssh_plan`: clear-all, and no forwarding flag at all.
        let plan = crate::ssh::direct::ssh_plan(&host(), "true".into(), false)
            .expect("the probe plan builds");
        assert!(
            plan.args.iter().any(|arg| arg == "ClearAllForwardings=yes"),
            "the non-interactive probe keeps ClearAllForwardings=yes"
        );
        assert_eq!(
            plan.args.iter().filter(|arg| arg.as_str() == "-T").count(),
            1,
            "the non-interactive probe is the -T form"
        );
        assert!(
            forward_flags(&plan.args).is_empty(),
            "the non-interactive probe emits no -L/-R/-D: {:?}",
            forward_flags(&plan.args)
        );

        // The interactive `ssh_plan` itself carries no forwarding either; the
        // agent-state `-R` is added by `shell_plan_with_session`, and that pair is
        // pinned as present-but-unchanged (IS-4 protects it, so its absence is
        // never asserted).
        let interactive = crate::ssh::direct::ssh_plan(&host(), "true".into(), true)
            .expect("the interactive plan builds");
        assert!(
            forward_flags(&interactive.args).is_empty(),
            "ssh_plan never forwards on its own: {:?}",
            forward_flags(&interactive.args)
        );
        assert!(
            !interactive.args.iter().any(|arg| arg == "ClearAllForwardings=yes"),
            "clear-all is scoped to the non-interactive path"
        );

        let shell = crate::ssh::direct::shell_plan_with_session(
            &host(),
            "/srv/app",
            Some("sid-1"),
            Some("/tmp/ferryx-agent-state.sock"),
        )
        .expect("the session plan builds");
        assert!(
            shell.args.windows(2).any(|pair| pair[0] == "-R"
                && pair[1] == "/tmp/ferryx-agent-sid-1.sock:/tmp/ferryx-agent-state.sock"),
            "the interactive Unix-socket -R is unchanged: {:?}",
            shell.args
        );
        assert!(shell
            .args
            .windows(2)
            .any(|pair| pair[0] == "-o" && pair[1] == "StreamLocalBindUnlink=yes"));
        assert_eq!(
            forward_flags(&shell.args),
            vec!["-R"],
            "the only forwarding in the whole plan is the agent-state one"
        );

        // `master_args`: clear-all present, no forwarding flag.
        let key = ConnectionKey::of(&host());
        let master = master_args(&key, Path::new("/run/ferryx/x.sock"));
        assert!(
            master.iter().any(|arg| arg == "ClearAllForwardings=yes"),
            "the master never inherits the user's forwards"
        );
        assert!(master.iter().any(|arg| arg == "-N"));
        assert!(
            forward_flags(&master).is_empty(),
            "a master forwards nothing: {:?}",
            forward_flags(&master)
        );

        // `client_reuse_args` never had a forwarding option — it is a socket client
        // that cannot forward — so pin exactly what it is. Asserting clear-all here
        // would be a false claim about the code.
        assert_eq!(
            client_reuse_args(Path::new("/run/ferryx/x.sock")),
            vec![
                "-o".to_string(),
                "ControlMaster=no".to_string(),
                "-o".to_string(),
                "ControlPath=/run/ferryx/x.sock".to_string(),
            ],
            "the socket client is a client: ControlMaster=no + ControlPath, nothing else"
        );
    }
}
