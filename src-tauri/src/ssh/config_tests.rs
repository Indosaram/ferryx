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
    let imported = import_aliases(&parsed, &[]);
    let plan = crate::ssh::direct::ssh_plan(&imported[0], "true".into(), false).unwrap();
    let values: Vec<_> = plan
        .args
        .windows(2)
        .filter(|pair| pair[0] == "-i")
        .map(|pair| pair[1].as_str())
        .collect();
    assert_eq!(values, vec![key]);
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
