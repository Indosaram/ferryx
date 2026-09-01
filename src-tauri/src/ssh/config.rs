use super::SshHost;
use super::SshAuthMethod;
use super::SshHostSource;

const IMPORT_CAP: usize = 100;

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigHost {
    pub alias: String,
    pub hostname: Option<String>,
    pub username: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
    pub jump_host: Option<String>,
}

pub fn parse_ssh_config(text: &str) -> Vec<ConfigHost> {
    let mut hosts: Vec<ConfigHost> = Vec::new();
    let mut current: Option<ConfigHost> = None;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((keyword, value)) = split_keyword_value(line) else {
            continue;
        };
        let keyword = keyword.to_ascii_lowercase();
        match keyword.as_str() {
            "host" => {
                if let Some(previous) = current.take() {
                    hosts.push(previous);
                }
                let alias = value.split_whitespace().next().unwrap_or_default().to_string();
                if alias.is_empty() || alias.starts_with('!') || alias.contains('*') || alias.contains('?') {
                    current = None;
                } else {
                    current = Some(ConfigHost {
                        alias,
                        hostname: None,
                        username: None,
                        port: None,
                        identity_file: None,
                        jump_host: None,
                    });
                }
            }
            "hostname" => {
                if let Some(host) = current.as_mut() {
                    host.hostname = Some(value.to_string());
                }
            }
            "user" => {
                if let Some(host) = current.as_mut() {
                    host.username = Some(value.to_string());
                }
            }
            "port" => {
                if let (Some(host), Ok(port)) = (current.as_mut(), value.parse::<u16>()) {
                    host.port = Some(port);
                }
            }
            "identityfile" => {
                if let Some(host) = current.as_mut() {
                    if host.identity_file.is_none() {
                        host.identity_file = Some(value.to_string());
                    }
                }
            }
            "proxyjump" => {
                if let Some(host) = current.as_mut() {
                    host.jump_host = Some(value.to_string());
                }
            }
            _ => {}
        }
    }

    if let Some(previous) = current.take() {
        hosts.push(previous);
    }
    hosts
}

fn split_keyword_value(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    let (keyword, rest) = match trimmed.find(char::is_whitespace) {
        Some(index) => (&trimmed[..index], &trimmed[index..]),
        None => return None,
    };
    Some((keyword, rest.trim()))
}

pub fn import_aliases(config_hosts: &[ConfigHost], tombstones: &[String]) -> Vec<SshHost> {
    config_hosts
        .iter()
        .filter_map(|entry| config_host_to_ssh_host(entry))
        .filter(|host| !tombstones.iter().any(|tombstone| tombstone == &host.key()))
        .take(IMPORT_CAP)
        .collect()
}

fn config_host_to_ssh_host(entry: &ConfigHost) -> Option<SshHost> {
    if entry.alias.is_empty() {
        return None;
    }
    Some(SshHost {
        id: uuid_like(&entry.alias),
        label: entry.alias.clone(),
        hostname: entry.hostname.clone().unwrap_or_else(|| entry.alias.clone()),
        username: entry.username.clone(),
        port: entry.port,
        identity_file: entry.identity_file.clone(),
        jump_host: entry.jump_host.clone(),
        source: SshHostSource::Config,
        auth_method: if entry.identity_file.is_some() {
            SshAuthMethod::Key
        } else {
            SshAuthMethod::Agent
        },
        disabled: None,
    })
}

fn uuid_like(seed: &str) -> String {
    format!("ssh-{seed}")
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let text = "Host ok\n  Port not-a-number\n  HostName h\nGARBAGE_LINE_WITHOUT_VALUE\nHost two\n";
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
        // bastion has no IdentityFile ⇒ agent auth
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
}
