pub mod bridge;
pub mod browse;
pub mod config;
pub mod direct;
pub mod exec;
#[path = "../ferryx_scope/ssh/mod.rs"]
pub mod helper_runtime;
pub mod helper_setup;
pub mod helper_assets;
pub mod operations;
pub mod projects;
pub mod runtime;
pub mod state_bridge;
pub mod worktree;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize)]
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
}

fn deserialize_optional_string_lenient<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let val: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    match val {
        Some(serde_json::Value::String(s)) => Ok(Some(s)),
        _ => Ok(None),
    }
}

impl<'de> Deserialize<'de> for SshHost {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawSshHost {
            id: Option<String>,

            label: Option<String>,
            #[serde(default, deserialize_with = "deserialize_optional_string_lenient")]
            name: Option<String>,
            #[serde(default, deserialize_with = "deserialize_optional_string_lenient")]
            alias: Option<String>,
            #[serde(default, deserialize_with = "deserialize_optional_string_lenient")]
            title: Option<String>,

            hostname: Option<String>,
            #[serde(default, deserialize_with = "deserialize_optional_string_lenient")]
            host: Option<String>,

            username: Option<String>,
            #[serde(default, deserialize_with = "deserialize_optional_string_lenient")]
            user: Option<String>,

            port: Option<u16>,

            identity_file: Option<String>,
            #[serde(default, rename = "identity_file", deserialize_with = "deserialize_optional_string_lenient")]
            identity_file_snake: Option<String>,
            #[serde(default, deserialize_with = "deserialize_optional_string_lenient")]
            key: Option<String>,
            #[serde(default, deserialize_with = "deserialize_optional_string_lenient")]
            key_path: Option<String>,

            jump_host: Option<String>,
            #[serde(default, rename = "jump_host", deserialize_with = "deserialize_optional_string_lenient")]
            jump_host_snake: Option<String>,
            #[serde(default, deserialize_with = "deserialize_optional_string_lenient")]
            proxy_jump: Option<String>,

            source: Option<SshHostSource>,
            auth_method: Option<SshAuthMethod>,
            disabled: Option<bool>,
        }

        let raw = RawSshHost::deserialize(deserializer)?;
        let id = raw.id.filter(|s| !s.is_empty()).ok_or_else(|| {
            serde::de::Error::missing_field("id")
        })?;

        let hostname = raw
            .hostname
            .or(raw.host)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_default();

        let label = raw
            .label
            .or(raw.name)
            .or(raw.alias)
            .or(raw.title)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| if !hostname.is_empty() { Some(hostname.clone()) } else { None })
            .unwrap_or_else(|| id.clone());

        let username = raw
            .username
            .or(raw.user)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let identity_file = raw
            .identity_file
            .or(raw.identity_file_snake)
            .or(raw.key)
            .or(raw.key_path)
            .filter(|p| !p.trim().is_empty());

        let jump_host = raw
            .jump_host
            .or(raw.jump_host_snake)
            .or(raw.proxy_jump)
            .filter(|j| !j.trim().is_empty());

        let auth_method = raw.auth_method.unwrap_or_else(|| {
            if identity_file.is_some() {
                SshAuthMethod::Key
            } else {
                SshAuthMethod::Agent
            }
        });

        let source = raw.source.unwrap_or(SshHostSource::Manual);

        Ok(SshHost {
            id,
            label,
            hostname,
            username,
            port: raw.port,
            identity_file,
            jump_host,
            source,
            auth_method,
            disabled: raw.disabled,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SshHostSource {
    #[serde(alias = "CONFIG", alias = "Config")]
    Config,
    #[default]
    #[serde(alias = "MANUAL", alias = "Manual")]
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SshAuthMethod {
    #[default]
    #[serde(alias = "AGENT", alias = "Agent")]
    Agent,
    #[serde(alias = "KEY", alias = "Key")]
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
    }

    #[test]
    fn serde_resilient_omaki_payload_with_name_host_user() {
        let json = serde_json::json!({
            "id": "omaki-100-91-254-71",
            "name": "omaki",
            "host": "100.91.254.71",
            "port": 22,
            "user": "indo",
            "authMethod": "agent",
            "source": "manual",
            "remoteContinuity": "on"
        });
        let host: SshHost = serde_json::from_value(json).expect("deserialize omaki payload");
        assert_eq!(host.id, "omaki-100-91-254-71");
        assert_eq!(host.label, "omaki");
        assert_eq!(host.hostname, "100.91.254.71");
        assert_eq!(host.username.as_deref(), Some("indo"));
        assert_eq!(host.port, Some(22));
        assert_eq!(host.source, SshHostSource::Manual);
        assert_eq!(host.auth_method, SshAuthMethod::Agent);
    }

    #[test]
    fn serde_mixed_canonical_and_legacy_keys_prefers_canonical_without_duplicate_field_error() {
        let json = serde_json::json!({
            "id": "h-mixed",
            "label": "CanonicalLabel",
            "name": "LegacyName",
            "alias": "LegacyAlias",
            "title": "LegacyTitle",
            "hostname": "canonical.example.com",
            "host": "legacy.example.com",
            "username": "c_user",
            "user": "l_user",
            "identityFile": "~/.ssh/canonical_key",
            "key": "~/.ssh/legacy_key",
            "jumpHost": "canonical-jump",
            "proxyJump": "legacy-jump"
        });
        let host: SshHost = serde_json::from_value(json).expect("deserialize mixed keys");
        assert_eq!(host.label, "CanonicalLabel");
        assert_eq!(host.hostname, "canonical.example.com");
        assert_eq!(host.username.as_deref(), Some("c_user"));
        assert_eq!(host.identity_file.as_deref(), Some("~/.ssh/canonical_key"));
        assert_eq!(host.jump_host.as_deref(), Some("canonical-jump"));

        // Matching canonical and legacy values also succeed
        let json_equal = serde_json::json!({
            "id": "h-equal",
            "label": "SameBox",
            "name": "SameBox",
            "hostname": "box.example.com",
            "host": "box.example.com"
        });
        let host_equal: SshHost = serde_json::from_value(json_equal).expect("deserialize matching keys");
        assert_eq!(host_equal.label, "SameBox");
        assert_eq!(host_equal.hostname, "box.example.com");
    }

    #[test]
    fn serde_preserves_id_byte_for_byte_and_requires_nonempty_id() {
        let json_spaced = serde_json::json!({
            "id": "  durable-id-with-spaces  ",
            "label": "Box",
            "hostname": "box.example.com"
        });
        let host: SshHost = serde_json::from_value(json_spaced).expect("preserve id");
        assert_eq!(host.id, "  durable-id-with-spaces  ");

        let json_missing_id = serde_json::json!({
            "label": "Box",
            "hostname": "box.example.com"
        });
        assert!(serde_json::from_value::<SshHost>(json_missing_id).is_err());

        let json_empty_id = serde_json::json!({
            "id": "",
            "label": "Box",
            "hostname": "box.example.com"
        });
        assert!(serde_json::from_value::<SshHost>(json_empty_id).is_err());
    }

    #[test]
    fn serde_resilient_missing_label_falls_back_to_hostname_or_id() {
        let json = serde_json::json!({
            "id": "custom-box",
            "hostname": "192.168.1.100"
        });
        let host: SshHost = serde_json::from_value(json).expect("deserialize missing label");
        assert_eq!(host.id, "custom-box");
        assert_eq!(host.label, "192.168.1.100");
        assert_eq!(host.hostname, "192.168.1.100");
        assert_eq!(host.source, SshHostSource::Manual);
        assert_eq!(host.auth_method, SshAuthMethod::Agent);

        let json_id_only = serde_json::json!({
            "id": "only-id"
        });
        let host_id: SshHost = serde_json::from_value(json_id_only).expect("deserialize id only");
        assert_eq!(host_id.label, "only-id");
    }

    #[test]
    fn serde_identity_file_empty_whitespace_normalizes_to_agent_auth() {
        // Whitespace identity file should normalize to None and infer Agent auth
        let json_ws = serde_json::json!({
            "id": "h-ws",
            "hostname": "example.com",
            "identityFile": "   "
        });
        let host: SshHost = serde_json::from_value(json_ws).expect("deserialize ws key");
        assert_eq!(host.identity_file, None);
        assert_eq!(host.auth_method, SshAuthMethod::Agent);

        // Valid identity file infers Key auth
        let json_valid = serde_json::json!({
            "id": "h-key",
            "hostname": "key.example.com",
            "identityFile": "~/.ssh/id_ed25519"
        });
        let host_key: SshHost = serde_json::from_value(json_valid).expect("deserialize valid key");
        assert_eq!(host_key.identity_file.as_deref(), Some("~/.ssh/id_ed25519"));
        assert_eq!(host_key.auth_method, SshAuthMethod::Key);

        // Explicit authMethod is preserved regardless of key presence
        let json_explicit = serde_json::json!({
            "id": "h-agent",
            "hostname": "example.com",
            "identityFile": "~/.ssh/id_ed25519",
            "authMethod": "agent"
        });
        let host_explicit: SshHost = serde_json::from_value(json_explicit).expect("deserialize explicit auth");
        assert_eq!(host_explicit.auth_method, SshAuthMethod::Agent);
    }

    #[test]
    fn serde_aliases_path_and_jump_host() {
        let json = serde_json::json!({
            "id": "h-aliases",
            "name": "JumpBox",
            "host": "internal.box",
            "user": "root",
            "keyPath": "/keys/admin",
            "proxyJump": "bastion.example.com",
            "source": "CONFIG",
            "authMethod": "KEY"
        });
        let host: SshHost = serde_json::from_value(json).expect("deserialize aliases");
        assert_eq!(host.label, "JumpBox");
        assert_eq!(host.hostname, "internal.box");
        assert_eq!(host.username.as_deref(), Some("root"));
        assert_eq!(host.identity_file.as_deref(), Some("/keys/admin"));
        assert_eq!(host.jump_host.as_deref(), Some("bastion.example.com"));
        assert_eq!(host.source, SshHostSource::Config);
        assert_eq!(host.auth_method, SshAuthMethod::Key);
    }

    #[test]
    fn serde_resilient_canonical_host_with_non_string_legacy_fields() {
        // A valid canonical host must never fail because legacy/extension keys contain non-string values
        let json = serde_json::json!({
            "id": "stable",
            "label": "Box",
            "hostname": "example.com",
            "source": "manual",
            "authMethod": "agent",
            "name": { "display": "old metadata" },
            "host": ["old", "hosts"],
            "user": 12345,
            "identity_file": { "path": "/legacy/key" },
            "key": false,
            "jump_host": 99,
            "proxyJump": null
        });
        let host: SshHost = serde_json::from_value(json).expect("deserialize canonical host with non-string legacy fields");
        assert_eq!(host.id, "stable");
        assert_eq!(host.label, "Box");
        assert_eq!(host.hostname, "example.com");
        assert_eq!(host.source, SshHostSource::Manual);
        assert_eq!(host.auth_method, SshAuthMethod::Agent);
    }
}
