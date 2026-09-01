use super::SshHost;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SshArgvError {
    #[error("ssh host field '{field}' must not start with '-': '{value}'")]
    UnsafeField { field: &'static str, value: String },
}

/// Guards the argv boundary: every host-derived ssh argument must never be
/// parsed as an ssh option, so leading dashes are rejected before exec.
pub fn validate_host_argv_fields(host: &SshHost) -> Result<(), SshArgvError> {
    fn check(field: &'static str, value: Option<&String>) -> Result<(), SshArgvError> {
        if let Some(value) = value {
            if value.starts_with('-') {
                return Err(SshArgvError::UnsafeField {
                    field,
                    value: value.clone(),
                });
            }
        }
        Ok(())
    }
    check("hostname", Some(&host.hostname))?;
    check("username", host.username.as_ref())?;
    check("identityFile", host.identity_file.as_ref())?;
    check("jumpHost", host.jump_host.as_ref())?;
    Ok(())
}

pub fn probe_argv(host: &SshHost) -> Vec<String> {
    let mut argv = vec![
        "ssh".to_string(),
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "ConnectTimeout=2.5".to_string(),
    ];
    append_target(&mut argv, host);
    argv
}

pub fn interactive_argv(host: &SshHost) -> Vec<String> {
    let mut argv = vec!["ssh".to_string(), "-tt".to_string()];
    if let Some(port) = host.port {
        argv.push("-p".to_string());
        argv.push(port.to_string());
    }
    if let Some(identity) = &host.identity_file {
        argv.push("-i".to_string());
        argv.push(identity.clone());
    }
    if let Some(jump) = &host.jump_host {
        argv.push("-J".to_string());
        argv.push(jump.clone());
    }
    append_target(&mut argv, host);
    argv
}

fn append_target(argv: &mut Vec<String>, host: &SshHost) {
    argv.push(host.target());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::SshAuthMethod;
    use crate::ssh::SshHostSource;

    fn host(
        port: Option<u16>,
        identity: Option<&str>,
        jump: Option<&str>,
        user: Option<&str>,
    ) -> SshHost {
        SshHost {
            id: "h".into(),
            label: "l".into(),
            hostname: "maho-win".into(),
            username: user.map(Into::into),
            port,
            identity_file: identity.map(Into::into),
            jump_host: jump.map(Into::into),
            source: SshHostSource::Config,
            auth_method: SshAuthMethod::Agent,
            disabled: None,
            repo_root: None,
            remote_continuity: crate::ssh::RemoteContinuity::Auto,
        }
    }

    #[test]
    fn red_probe_argv_shape() {
        assert_eq!(
            probe_argv(&host(None, None, None, None)),
            vec![
                "ssh",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=2.5",
                "maho-win"
            ]
        );
        assert_eq!(
            probe_argv(&host(None, None, None, Some("sook")))
                .last()
                .map(String::as_str),
            Some("sook@maho-win")
        );
    }

    #[test]
    fn red_host_fields_reject_option_injection() {
        let mut bad_hostname = host(None, None, None, None);
        bad_hostname.hostname = "-oProxyCommand=evil".into();
        assert!(validate_host_argv_fields(&bad_hostname).is_err());

        let mut bad_user = host(None, None, None, None);
        bad_user.username = Some("-4".into());
        assert!(validate_host_argv_fields(&bad_user).is_err());

        let mut bad_identity = host(None, None, None, None);
        bad_identity.identity_file = Some("-oBatchMode=yes".into());
        assert!(validate_host_argv_fields(&bad_identity).is_err());

        let mut bad_jump = host(None, None, None, None);
        bad_jump.jump_host = Some("-J".into());
        assert!(validate_host_argv_fields(&bad_jump).is_err());

        // Well-formed fields pass, including empty-ish normal values.
        assert!(validate_host_argv_fields(&host(
            None,
            Some("~/.ssh/id_ed25519"),
            Some("bastion:22"),
            Some("sook")
        ))
        .is_ok());
    }

    #[test]
    fn red_interactive_argv_flags() {
        let bare = interactive_argv(&host(None, None, None, None));
        assert_eq!(bare, vec!["ssh", "-tt", "maho-win"]);

        let full = interactive_argv(&host(
            Some(2200),
            Some("~/.ssh/id_ed25519"),
            Some("bastion"),
            Some("sook"),
        ));
        assert_eq!(
            full,
            vec![
                "ssh",
                "-tt",
                "-p",
                "2200",
                "-i",
                "~/.ssh/id_ed25519",
                "-J",
                "bastion",
                "sook@maho-win"
            ]
        );
    }
}
