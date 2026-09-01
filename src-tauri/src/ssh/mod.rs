pub mod config;
pub mod exec;
pub mod remote_deploy;
pub mod worktree;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshHost {
    pub id: String,
    pub label: String,
    pub hostname: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jump_host: Option<String>,
    pub source: SshHostSource,
    pub auth_method: SshAuthMethod,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    /// Remote repository root used to scope `git -C` for worktree ops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    /// Whether SSH panes should use a resident daemon for continuity.
    #[serde(default)]
    pub remote_continuity: RemoteContinuity,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteContinuity {
    #[default]
    Auto,
    Off,
    On,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SshHostSource {
    Config,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SshAuthMethod {
    Agent,
    Key,
}

impl SshHost {
    /// Target part of an ssh command line: `[user@]hostname`.
    pub fn target(&self) -> String {
        match &self.username {
            Some(user) if !user.is_empty() => format!("{user}@{}", self.hostname),
            _ => self.hostname.clone(),
        }
    }

    /// Tombstone/dedupe key: `user@hostname:port` or `hostname:port`.
    pub fn key(&self) -> String {
        let port = self.port.unwrap_or(22);
        match &self.username {
            Some(user) if !user.is_empty() => format!("{user}@{}:{port}", self.hostname),
            _ => format!("{}:{port}", self.hostname),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(username: Option<&str>, hostname: &str, port: Option<u16>) -> SshHost {
        SshHost {
            id: "h1".into(),
            label: "l".into(),
            hostname: hostname.into(),
            username: username.map(Into::into),
            port,
            identity_file: None,
            jump_host: None,
            source: SshHostSource::Manual,
            auth_method: SshAuthMethod::Agent,
            disabled: None,
            repo_root: None,
            remote_continuity: RemoteContinuity::Auto,
        }
    }

    #[test]
    fn target_with_and_without_user() {
        assert_eq!(
            host(Some("sook"), "maho-win", None).target(),
            "sook@maho-win"
        );
        assert_eq!(host(None, "maho-win", None).target(), "maho-win");
    }

    #[test]
    fn key_includes_user_and_default_port() {
        assert_eq!(
            host(Some("sook"), "maho-win", None).key(),
            "sook@maho-win:22"
        );
        assert_eq!(host(None, "h", Some(2200)).key(), "h:2200");
    }

    #[test]
    fn serde_camel_case_round_trip() {
        let json = serde_json::json!({
            "id": "h1", "label": "box", "hostname": "h", "username": "u",
            "port": 22, "source": "config", "authMethod": "agent"
        });
        let host: SshHost = serde_json::from_value(json).expect("deserialize");
        assert_eq!(host.username.as_deref(), Some("u"));
        assert_eq!(host.source, SshHostSource::Config);
        assert!(host.identity_file.is_none());
        assert_eq!(host.remote_continuity, RemoteContinuity::Auto);
    }
}
