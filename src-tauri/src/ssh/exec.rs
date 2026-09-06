use super::SshHost;

pub fn probe_argv(host: &SshHost) -> Vec<String> {
    let mut argv = vec![
        "ssh".to_string(),
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "ConnectTimeout=2.5".to_string(),
    ];
    append_options(&mut argv, host);
    append_target(&mut argv, host);
    argv
}

pub fn interactive_argv(host: &SshHost) -> Vec<String> {
    let mut argv = vec!["ssh".to_string(), "-tt".to_string()];
    append_options(&mut argv, host);
    append_target(&mut argv, host);
    argv
}

fn append_options(argv: &mut Vec<String>, host: &SshHost) {
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
        }
    }

    #[test]
    fn probe_honors_saved_connection_options() {
        let argv = probe_argv(&host(
            Some(2200),
            Some("/keys/key with space"),
            Some("jump"),
            Some("user"),
        ));
        for pair in [
            ["-p", "2200"],
            ["-i", "/keys/key with space"],
            ["-J", "jump"],
        ] {
            assert!(
                argv.windows(2).any(|args| args == pair),
                "missing {pair:?} in {argv:?}"
            );
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
