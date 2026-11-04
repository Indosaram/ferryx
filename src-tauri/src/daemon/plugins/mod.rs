//! Out-of-process plugin SDK. All filesystem APIs here are blocking-worker APIs.
//! The owner-facing daemon adapter must supply consent, dispatch and shutdown.
use super::{pane_grants, protocol::DaemonRequest, supervisor_peer::PeerClass};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::{Path, PathBuf}};

pub mod runtime;
pub mod registry;
#[cfg(test)]
mod tests;

pub const MAX_LINE: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub command: Vec<String>,
    pub capabilities: BTreeSet<String>,
}

impl Manifest {
    /// Only load from the owner's configured plugin root, never the current project.
    pub fn load(root: &Path, name: &str) -> Result<Self, String> {
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            return Err("invalid plugin name".into());
        }
        let path = root.join(name).join("plugin.toml");
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_LINE { return Err("manifest too large".into()); }
        let manifest: Self = toml::from_str(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if manifest.name != name || manifest.version.is_empty() || manifest.command.is_empty()
            || !Path::new(&manifest.command[0]).is_absolute()
            || manifest.capabilities.iter().any(|c| !matches!(c.as_str(), "listSessions" | "getCapabilities" | "write" | "render")) {
            return Err("invalid manifest or unsupported capability".into());
        }
        Ok(manifest)
    }
}

/// Persisted approval includes the complete manifest: changing argv or permissions requires
/// fresh owner consent. Reload this file after restart; carry the same snapshot on handover.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Approvals { pub manifests: Vec<Manifest> }

impl Approvals {
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        crate::remote::auth::write_private_json(path, self).map_err(|e| e.to_string())
    }
    pub fn enabled(&self, manifest: &Manifest) -> bool { self.manifests.contains(manifest) }
}

pub fn config_root() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
            .map(|home| PathBuf::from(home).join(".config")))
        .map(|root| root.join("ferryx/plugins"))
}

#[derive(Debug, Serialize)]
pub struct Forbidden {
    pub code: &'static str,
    pub reason: &'static str,
}

/// An explicit allowlist, not gate_for's read fallback: new daemon verbs are denied by default.
/// Plugin identities cannot inherit a real pane's own-session exemption or workspace.
pub fn authorize(manifest: &Manifest, request: &DaemonRequest, grants: &pane_grants::PaneGrants)
    -> Result<(), Forbidden>
{
    let capability = match request {
        DaemonRequest::ListSessions => "listSessions",
        DaemonRequest::GetCapabilities => "getCapabilities",
        DaemonRequest::Write { .. } => "write",
        DaemonRequest::PluginRender { .. } => "render",
        _ => return Err(Forbidden { code: "FORBIDDEN", reason: "PLUGIN_CAPABILITY_REQUIRED" }),
    };
    if !manifest.capabilities.contains(capability) {
        return Err(Forbidden { code: "FORBIDDEN", reason: "PLUGIN_CAPABILITY_REQUIRED" });
    }
    // Stdio binds render to this plugin's own view map, not any pane or another plugin.
    if matches!(request, DaemonRequest::PluginRender { .. }) { return Ok(()); }
    if let Some(gate) = pane_grants::gate_for(request) {
        let peer = PeerClass::Pane { session_id: format!("plugin:{}", manifest.name), why: "plugin stdio".into() };
        // Do not use decide's own-session shortcut for a plugin-namespaced target.
        let gate = match gate {
            pane_grants::Gate::Session { session_id, .. } if session_id.starts_with("plugin:") =>
                return Err(Forbidden { code: "FORBIDDEN", reason: pane_grants::PANE_GRANT_REQUIRED }),
            gate => gate,
        };
        pane_grants::decide(&peer, &gate, grants, |_| None)
            .map_err(|e| Forbidden { code: "FORBIDDEN", reason: e.reason })?;
    }
    Ok(())
}

/// Block HTML is always sandboxed without scripts, network, forms or parent access.
pub fn block_document(plugin: &str, html: &str) -> Result<String, String> {
    if plugin.is_empty() || !plugin.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return Err("invalid plugin origin".into());
    }
    Ok(format!("<!doctype html><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src ferryx-plugin://{plugin}; style-src ferryx-plugin://{plugin}; base-uri 'none'; form-action 'none'\">{html}"))
}
