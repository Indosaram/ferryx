use super::*;

#[test]
fn p10_identity_file_import_preserves_semantic_argv() {
    let key = std::env::temp_dir().join("QA Person's key");
    let key = key.to_str().unwrap();
    let text = format!(
        "Host fixture\n IdentityFile \"{key}\" # key comment\n IdentityFile ~/ignored\n"
    );
    let parsed = parse_ssh_config(&text);
    assert_eq!(parsed[0].identity_file.as_deref(), Some(key));
    let mut imported = import_aliases(&parsed, &[]);
    // Import still records the key; the launched ssh reads it from the user's own Host block.
    assert_eq!(imported[0].identity_file.as_deref(), Some(key));
    // Pasted or custom-path text: ssh cannot resolve the alias, so the key stays on the command line.
    let plan = crate::ssh::direct::ssh_plan(&imported[0], "true".into(), false).unwrap();
    let values: Vec<_> = plan
        .args
        .windows(2)
        .filter(|pair| pair[0] == "-i")
        .map(|pair| pair[1].as_str())
        .collect();
    assert_eq!(values, vec![key]);
    imported[0].system_config_alias = Some("fixture".into());
    let plan = crate::ssh::direct::ssh_plan(&imported[0], "true".into(), false).unwrap();
    assert!(!plan.args.iter().any(|arg| arg == "-i"), "{:?}", plan.args);
    assert_eq!(plan.args[plan.args.len() - 2], "fixture");
}

#[test]
fn config_host_connects_by_alias_without_explicit_flags() {
    let text = "Host work-box\n HostName 10.9.8.7\n User dev\n Port 2200\n IdentityFile /keys/id\n ProxyJump bastion\n ProxyCommand nc %h %p\n";
    let mut imported = import_aliases(&parse_ssh_config(text), &[]);
    imported[0].system_config_alias = Some("work-box".into());
    let host = &imported[0];
    assert_eq!(host.id, "ssh-work-box");
    assert_eq!(host.config_alias(), Some("work-box"));
    assert_eq!(host.hostname, "10.9.8.7");
    for argv in [
        crate::ssh::exec::interactive_argv(host),
        crate::ssh::exec::probe_argv(host),
        crate::ssh::direct::ssh_plan(host, "true".into(), false).unwrap().args,
    ] {
        for flag in ["-p", "-i", "-J"] {
            assert!(!argv.iter().any(|arg| arg == flag), "{flag} in {argv:?}");
        }
        assert!(argv.iter().any(|arg| arg == "dev@work-box"), "{argv:?}");
        assert!(!argv.iter().any(|arg| arg.contains("10.9.8.7")), "{argv:?}");
    }
    // A renamed label does not change the connect target.
    let mut renamed = host.clone();
    renamed.label = "My box".into();
    assert_eq!(renamed.connect_target(), "dev@work-box");
}

#[test]
fn config_alias_is_validated_as_a_host_token() {
    let mut host = import_aliases(&parse_ssh_config("Host ok\n"), &[]).remove(0);
    host.system_config_alias = Some("ok".into());
    assert!(crate::ssh::direct::validate_host(&host).is_ok());
    for bad in ["-oProxyCommand=x", "a b", "a;b"] {
        host.system_config_alias = Some(bad.into());
        assert!(crate::ssh::direct::validate_host(&host).is_err(), "{bad}");
    }
}

/// D1: only a default-`~/.ssh/config` import connects by alias. A pasted or custom-path import
/// carries no provenance and keeps every endpoint field on the command line, which is also what a
/// store written before provenance existed gets.
#[test]
fn pasted_or_custom_path_import_keeps_explicit_endpoint_flags() {
    let text = "Host pasted-box\n HostName 10.9.8.7\n User dev\n Port 2200\n IdentityFile /keys/id\n ProxyJump bastion\n";
    let host = import_aliases(&parse_ssh_config(text), &[]).remove(0);
    assert_eq!(host.id, "ssh-pasted-box");
    assert_eq!(host.source, SshHostSource::Config);
    assert_eq!(host.config_alias(), None);
    let argv = crate::ssh::direct::ssh_plan(&host, "true".into(), false).unwrap().args;
    for pair in [["-p", "2200"], ["-i", "/keys/id"], ["-J", "bastion"]] {
        assert!(argv.windows(2).any(|w| w == pair), "{pair:?} missing: {argv:?}");
    }
    assert_eq!(argv[argv.len() - 2], "dev@10.9.8.7");
    assert!(!argv.iter().any(|arg| arg.contains("pasted-box")), "{argv:?}");

    // An older store entry (id ssh-<alias>, source config, no provenance) behaves the same.
    let legacy: crate::ssh::SshHost = serde_json::from_value(serde_json::json!({
        "id": "ssh-pasted-box", "label": "pasted-box", "hostname": "10.9.8.7", "username": "dev",
        "port": 2200, "source": "config", "authMethod": "agent"
    }))
    .unwrap();
    assert_eq!(legacy.config_alias(), None);
    assert_eq!(legacy.connect_target(), "dev@10.9.8.7");
    // Provenance round-trips through the store format.
    let mut marked = legacy.clone();
    marked.system_config_alias = Some("pasted-box".into());
    let back: crate::ssh::SshHost =
        serde_json::from_str(&serde_json::to_string(&marked).unwrap()).unwrap();
    assert_eq!(back.config_alias(), Some("pasted-box"));
    assert!(!serde_json::to_string(&legacy).unwrap().contains("systemConfigAlias"));
}

#[test]
fn p10_identity_file_config_quoting_grammar() {
    for (directive, expected) in [
        (
            r#"IdentityFile "C:\Users\QA Person\.ssh\id_ed25519""#,
            r"C:\Users\QA Person\.ssh\id_ed25519",
        ),
        (
            r#"IdentityFile="~/.ssh/key with space""#,
            "~/.ssh/key with space",
        ),
        (
            r#"IdentityFile = "/path/with spaces/key""#,
            "/path/with spaces/key",
        ),
        (r#"IdentityFile = /path/plain/key"#, "/path/plain/key"),
        (r#"IdentityFile = ~/.ssh/key # comment"#, "~/.ssh/key"),
        (r#"IdentityFile "~/.ssh/key\"quote""#, "~/.ssh/key\"quote"),
        (r#"IdentityFile ~/.ssh/plain # comment"#, "~/.ssh/plain"),
        (r#"IdentityFile "~/.ssh/key#literal""#, "~/.ssh/key#literal"),
    ] {
        let parsed = parse_ssh_config(&format!("Host fixture\n {directive}\n"));
        assert_eq!(
            parsed[0].identity_file.as_deref(),
            Some(expected),
            "{directive}"
        );
    }
}

const SAMPLE: &str = "\
# comment line
Host win
    HostName maho-win.example.com
    User sook
    Port 2200
    IdentityFile ~/.ssh/id_ed25519
    ProxyJump bastion

Host bastion
    HostName bastion.example.com

Host *.prod
    User deploy

Host !skip.me *.all
    User ignored
";

#[test]
fn red_parse_extracts_named_aliases_only() {
    let hosts = parse_ssh_config(SAMPLE);
    let aliases: Vec<&str> = hosts.iter().map(|host| host.alias.as_str()).collect();
    assert_eq!(aliases, vec!["win", "bastion"]);
    let win = &hosts[0];
    assert_eq!(win.hostname.as_deref(), Some("maho-win.example.com"));
    assert_eq!(win.username.as_deref(), Some("sook"));
    assert_eq!(win.port, Some(2200));
    assert!(win.identity_file.as_deref().unwrap().contains("id_ed25519"));
    assert_eq!(win.jump_host.as_deref(), Some("bastion"));
}

#[test]
fn red_malformed_lines_are_skipped() {
    let text =
        "Host ok\n  Port not-a-number\n  HostName h\nGARBAGE_LINE_WITHOUT_VALUE\nHost two\n";
    let hosts = parse_ssh_config(text);
    assert_eq!(hosts.len(), 2);
    assert_eq!(hosts[0].port, None);
}

#[test]
fn red_import_dedupes_against_tombstones_and_caps() {
    let hosts = parse_ssh_config(SAMPLE);
    let tombstones = vec![hosts[0].alias.clone()];
    let imported = import_aliases(&hosts, &tombstones_of(&tombstones, &hosts));
    assert_eq!(imported.len(), 1);
    assert_eq!(imported[0].label, "bastion");
    assert_eq!(imported[0].source, SshHostSource::Config);
    assert_eq!(imported[0].auth_method, SshAuthMethod::Agent);
}

fn tombstones_of(aliases: &[String], hosts: &[ConfigHost]) -> Vec<String> {
    aliases
        .iter()
        .filter_map(|alias| {
            hosts.iter().find(|host| &host.alias == alias).map(|host| {
                let user = host.username.clone().unwrap_or_default();
                let hostname = host.hostname.clone().unwrap_or_else(|| alias.clone());
                let port = host.port.unwrap_or(22);
                if user.is_empty() {
                    format!("{hostname}:{port}")
                } else {
                    format!("{user}@{hostname}:{port}")
                }
            })
        })
        .collect()
}

#[test]
fn red_import_caps_at_100() {
    let mut text = String::new();
    for index in 0..150 {
        text.push_str(&format!("Host h{index}\n  HostName h{index}.example\n"));
    }
    let hosts = parse_ssh_config(&text);
    let imported = import_aliases(&hosts, &[]);
    assert_eq!(imported.len(), 100);
}

#[test]
fn red_include_depth_and_cycle_guards() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.conf");
    let b = dir.path().join("b.conf");
    std::fs::write(&a, format!("Include {}\nHost host_a\n  HostName a.example\n", b.display())).unwrap();
    std::fs::write(&b, format!("Include {}\nHost host_b\n  HostName b.example\n", a.display())).unwrap();

    let hosts = parse_ssh_config(&format!("Include {}\n", a.display()));
    let aliases: Vec<&str> = hosts.iter().map(|h| h.alias.as_str()).collect();
    assert!(aliases.contains(&"host_a"));
    assert!(aliases.contains(&"host_b"));
}

#[test]
fn red_include_relative_glob_and_first_wins() {
    let dir = tempfile::tempdir().unwrap();
    let conf_d = dir.path().join("conf.d");
    std::fs::create_dir(&conf_d).unwrap();

    let f1 = conf_d.join("01-work.conf");
    let f2 = conf_d.join("02-override.conf");

    std::fs::write(&f1, "Host work\n  HostName work.original.com\n  User workuser\n").unwrap();
    std::fs::write(&f2, "Host work\n  HostName work.override.com\n  Port 2222\nHost other\n  HostName other.com\n").unwrap();

    let main = "Host work\n  User mainuser\nInclude conf.d/*.conf\n";
    let hosts = parse_ssh_config_with_dir(main, Some(dir.path()));

    let work = hosts.iter().find(|h| h.alias == "work").expect("work host exists");
    assert_eq!(work.username.as_deref(), Some("mainuser"));
    assert_eq!(work.hostname.as_deref(), Some("work.original.com"));
    assert_eq!(work.port, Some(2222));

    assert!(hosts.iter().any(|h| h.alias == "other"));
}

#[test]
fn red_missing_include_is_skipped_without_panic() {
    let text = "Include /nonexistent/path/never_there_*.conf\nHost valid\n  HostName valid.com\n";
    let hosts = parse_ssh_config(text);
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].alias, "valid");
}

#[test]
fn red_token_expansion_d_h_r_u() {
    let dir = tempfile::tempdir().unwrap();
    let text = "\
Host server
  HostName srv.example.com
  User testuser
  IdentityFile %d/id_%h_%r
  ProxyJump bastion:%h
";
    let hosts = parse_ssh_config_with_dir(text, Some(dir.path()));
    let srv = &hosts[0];
    // OpenSSH: %d is the local user's home directory, never the config directory.
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .expect("%d requires a resolvable local home directory");
    let expected_key = format!("{home}/id_srv.example.com_testuser");
    assert_eq!(srv.identity_file.as_deref(), Some(expected_key.as_str()));
    assert!(
        !expected_key.contains(&dir.path().display().to_string()),
        "%d must not expand to the config directory: {expected_key}"
    );
    assert_eq!(srv.jump_host.as_deref(), Some("bastion:srv.example.com"));
}

/// Live check against a real sshd, run with HOME pointing at a temporary home whose
/// `~/.ssh/config` holds FERRYX_ALIAS_QA_ALIAS (reaching the server only through its
/// ProxyCommand) and FERRYX_ALIAS_QA_PASTED (HostName/Port/IdentityFile straight to the server).
/// No ssh wrapper: the real ssh reads the default config itself. The default-config import connects
/// by alias; a pasted import of the same blocks has no provenance and connects with explicit flags.
#[tokio::test]
#[ignore = "requires a live sshd on 127.0.0.1 and FERRYX_ALIAS_QA_*"]
async fn live_config_alias_connects_through_proxy_command() {
    let var = |name: &str| std::env::var(format!("FERRYX_ALIAS_QA_{name}")).unwrap();
    let config = std::path::PathBuf::from(var("HOME")).join(".ssh").join("config");
    let text = std::fs::read_to_string(&config).unwrap();
    let find = |alias: String| {
        import_aliases(&parse_ssh_config(&text), &[])
            .into_iter()
            .find(|host| host.label == alias)
            .expect("alias imported")
    };
    // What `cmd_ssh_import_config` records for an import of the default config file.
    let mut host = find(var("ALIAS"));
    host.system_config_alias = Some(var("ALIAS"));
    let argv = crate::ssh::direct::ssh_plan(&host, "true".into(), false).unwrap().args;
    println!("QA system-config argv: {argv:?}");
    let summary = crate::ipc::ssh::ssh_test_connection_summary(host.clone()).await;
    println!(
        "QA system-config host: reachable={} diagnostic={:?}",
        summary.reachable, summary.diagnostic
    );
    assert!(summary.reachable, "{:?}", summary.diagnostic);

    // A pasted import of the ProxyCommand-only block cannot use the block: it dials HostName.
    let pasted_proxy = find(var("ALIAS"));
    assert_eq!(pasted_proxy.config_alias(), None);
    let summary = crate::ipc::ssh::ssh_test_connection_summary(pasted_proxy).await;
    println!(
        "QA pasted proxy-only host: reachable={} code={:?}",
        summary.reachable,
        summary.diagnostic.as_ref().map(|d| d.code.to_string())
    );
    assert!(!summary.reachable, "a pasted host must not pick up the alias block");

    // A pasted import of a direct block connects through its explicit -p/-i.
    let pasted = find(var("PASTED"));
    let argv = crate::ssh::direct::ssh_plan(&pasted, "true".into(), false).unwrap().args;
    println!("QA pasted argv: {argv:?}");
    assert!(argv.windows(2).any(|pair| pair[0] == "-p"));
    assert!(argv.windows(2).any(|pair| pair[0] == "-i"));
    let summary = crate::ipc::ssh::ssh_test_connection_summary(pasted).await;
    println!(
        "QA pasted host: reachable={} diagnostic={:?}",
        summary.reachable, summary.diagnostic
    );
    assert!(summary.reachable, "{:?}", summary.diagnostic);
}
