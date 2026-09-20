use super::SshAuthMethod;
use super::SshHost;
use super::SshHostSource;
use std::path::{Path, PathBuf};

const IMPORT_CAP: usize = 100;
const MAX_INCLUDE_DEPTH: usize = 8;

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
    let default_dir = dirs_fallback_ssh_dir();
    parse_ssh_config_with_dir(text, default_dir.as_deref())
}

pub fn parse_ssh_config_with_dir(text: &str, config_dir: Option<&Path>) -> Vec<ConfigHost> {
    let mut visited = Vec::new();
    let mut hosts = Vec::new();
    parse_ssh_config_internal(text, config_dir, 0, &mut visited, &mut hosts);
    hosts
}

fn dirs_fallback_ssh_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|h| PathBuf::from(h).join(".ssh"))
}

fn parse_ssh_config_internal(
    text: &str,
    config_dir: Option<&Path>,
    depth: usize,
    visited: &mut Vec<PathBuf>,
    hosts: &mut Vec<ConfigHost>,
) {
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
            "include" => {
                flush_host(&mut current, config_dir, hosts);
                if depth < MAX_INCLUDE_DEPTH {
                    let parsed_path = parse_ssh_value(value);
                    if !parsed_path.is_empty() {
                        include_files(&parsed_path, config_dir, depth, visited, hosts);
                    }
                }
            }
            "host" => {
                flush_host(&mut current, config_dir, hosts);
                let alias = value
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_string();
                if alias.is_empty()
                    || alias.starts_with('!')
                    || alias.contains('*')
                    || alias.contains('?')
                {
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
                        let parsed_val = parse_ssh_value(value);
                        if !parsed_val.is_empty() {
                            host.identity_file = Some(parsed_val);
                        }
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

    flush_host(&mut current, config_dir, hosts);
}

fn flush_host(
    current: &mut Option<ConfigHost>,
    config_dir: Option<&Path>,
    hosts: &mut Vec<ConfigHost>,
) {
    if let Some(mut host) = current.take() {
        finalize_host(&mut host, config_dir);
        merge_host(hosts, host);
    }
}

fn finalize_host(host: &mut ConfigHost, _config_dir: Option<&Path>) {
    let local_user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    // OpenSSH expands %d to the local user's home directory, not the config directory.
    let local_home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok();

    if let Some(ref user) = host.username {
        let expanded = expand_tokens(user, None, None, Some(&local_user), local_home.as_deref());
        host.username = Some(expanded);
    }

    let hostname = host.hostname.as_deref().unwrap_or(&host.alias);
    let remote_user = host.username.as_deref();

    if let Some(ref id_file) = host.identity_file {
        let expanded = expand_tokens(
            id_file,
            Some(hostname),
            remote_user,
            Some(&local_user),
            local_home.as_deref(),
        );
        host.identity_file = Some(expanded);
    }

    if let Some(ref jump) = host.jump_host {
        let expanded = expand_tokens(
            jump,
            Some(hostname),
            remote_user,
            Some(&local_user),
            local_home.as_deref(),
        );
        host.jump_host = Some(expanded);
    }
}

fn merge_host(hosts: &mut Vec<ConfigHost>, host: ConfigHost) {
    if let Some(existing) = hosts.iter_mut().find(|h| h.alias == host.alias) {
        if existing.hostname.is_none() {
            existing.hostname = host.hostname;
        }
        if existing.username.is_none() {
            existing.username = host.username;
        }
        if existing.port.is_none() {
            existing.port = host.port;
        }
        if existing.identity_file.is_none() {
            existing.identity_file = host.identity_file;
        }
        if existing.jump_host.is_none() {
            existing.jump_host = host.jump_host;
        }
    } else {
        hosts.push(host);
    }
}

fn include_files(
    pattern: &str,
    config_dir: Option<&Path>,
    depth: usize,
    visited: &mut Vec<PathBuf>,
    hosts: &mut Vec<ConfigHost>,
) {
    let resolved_pattern = expand_include_path(pattern, config_dir);
    let matched_files = resolve_glob_files(&resolved_pattern);

    for file_path in matched_files {
        let canonical = file_path.canonicalize().unwrap_or_else(|_| file_path.clone());
        if visited.contains(&canonical) {
            continue;
        }
        visited.push(canonical);

        if let Ok(content) = std::fs::read_to_string(&file_path) {
            parse_ssh_config_internal(
                &content,
                file_path.parent(),
                depth + 1,
                visited,
                hosts,
            );
        }
    }
}

fn expand_include_path(pattern: &str, config_dir: Option<&Path>) -> PathBuf {
    if let Some(rest) = pattern.strip_prefix("~/").or_else(|| pattern.strip_prefix(r"~\")) {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return PathBuf::from(home).join(rest);
        }
        return PathBuf::from(pattern);
    }
    let p = Path::new(pattern);
    if p.is_absolute() {
        p.to_path_buf()
    } else if let Some(base) = config_dir {
        base.join(pattern)
    } else if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        PathBuf::from(home).join(".ssh").join(pattern)
    } else {
        PathBuf::from("~/.ssh").join(pattern)
    }
}

fn resolve_glob_files(path: &Path) -> Vec<PathBuf> {
    let path_str = path.to_string_lossy();
    if !path_str.contains('*') {
        if path.is_file() {
            return vec![path.to_path_buf()];
        }
        return Vec::new();
    }

    let Some(parent) = path.parent() else {
        return Vec::new();
    };
    let Some(file_pattern) = path.file_name().and_then(|n| n.to_str()) else {
        return Vec::new();
    };

    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };

    let mut matches = Vec::new();
    for entry in entries.flatten() {
        let entry_path = entry.path();
        if !entry_path.is_file() {
            continue;
        }
        if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
            if simple_wildcard_match(file_pattern, name) {
                matches.push(entry_path);
            }
        }
    }
    matches.sort();
    matches
}

fn simple_wildcard_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut current = text;
    if !parts[0].is_empty() {
        if !current.starts_with(parts[0]) {
            return false;
        }
        current = &current[parts[0].len()..];
    }
    for &part in &parts[1..parts.len() - 1] {
        if part.is_empty() {
            continue;
        }
        let Some(pos) = current.find(part) else {
            return false;
        };
        current = &current[pos + part.len()..];
    }
    let last = parts[parts.len() - 1];
    if !last.is_empty() {
        current.ends_with(last)
    } else {
        true
    }
}

fn expand_tokens(
    template: &str,
    hostname: Option<&str>,
    remote_user: Option<&str>,
    local_user: Option<&str>,
    home_dir: Option<&str>,
) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.peek() {
                Some('%') => {
                    chars.next();
                    out.push('%');
                }
                Some('d') => {
                    chars.next();
                    if let Some(home) = home_dir {
                        out.push_str(home);
                    } else {
                        out.push_str("%d");
                    }
                }
                Some('h') => {
                    chars.next();
                    if let Some(h) = hostname {
                        out.push_str(h);
                    } else {
                        out.push_str("%h");
                    }
                }
                Some('r') => {
                    chars.next();
                    if let Some(r) = remote_user {
                        out.push_str(r);
                    } else if let Some(u) = local_user {
                        out.push_str(u);
                    } else {
                        out.push_str("%r");
                    }
                }
                Some('u') => {
                    chars.next();
                    if let Some(u) = local_user {
                        out.push_str(u);
                    } else {
                        out.push_str("%u");
                    }
                }
                Some(&other) => {
                    out.push('%');
                    out.push(other);
                    chars.next();
                }
                None => {
                    out.push('%');
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn split_keyword_value(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    let index = trimmed.find(|c: char| c.is_whitespace() || c == '=')?;
    let keyword = trimmed[..index].trim();
    let mut rest = trimmed[index..].trim_start();
    if let Some(after_eq) = rest.strip_prefix('=') {
        rest = after_eq.trim_start();
    }
    Some((keyword, rest.trim_end()))
}

fn parse_ssh_value(input: &str) -> String {
    let trimmed = input.trim();
    if let Some(rest) = trimmed.strip_prefix('"') {
        let mut result = String::new();
        let mut in_escape = false;
        let mut chars = rest.char_indices().peekable();
        while let Some((_, c)) = chars.next() {
            if in_escape {
                if c == '"' {
                    result.push('"');
                } else if c == '\\' {
                    result.push('\\');
                } else {
                    result.push('\\');
                    result.push(c);
                }
                in_escape = false;
            } else if c == '\\' {
                in_escape = true;
            } else if c == '"' {
                break;
            } else {
                result.push(c);
            }
        }
        result
    } else {
        let end = trimmed
            .find(|c: char| c.is_whitespace() || c == '#')
            .unwrap_or(trimmed.len());
        trimmed[..end].to_string()
    }
}

pub fn import_aliases(config_hosts: &[ConfigHost], tombstones: &[String]) -> Vec<SshHost> {
    let mut seen_aliases = std::collections::HashSet::new();
    config_hosts
        .iter()
        .filter_map(|entry| config_host_to_ssh_host(entry))
        .filter(|host| seen_aliases.insert(host.label.clone()))
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
        hostname: entry
            .hostname
            .clone()
            .unwrap_or_else(|| entry.alias.clone()),
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
#[path = "config_tests.rs"]
mod tests;
