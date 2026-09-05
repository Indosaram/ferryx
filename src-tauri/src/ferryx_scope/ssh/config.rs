use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Mutex};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostConfig {
    pub id: String,
    pub name: String,
    pub hostname: String,
    pub user: String,
    pub port: u16,
    pub identity_file: Option<PathBuf>,
    pub proxy_jump: Option<String>,
    pub known_hosts_file: PathBuf,
}
impl HostConfig {
    pub fn argv(&self) -> Result<Vec<String>, String> {
        let safe = |s: &str| !s.is_empty() && !s.starts_with('-') && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._:@[]-".contains(&b));
        if self.port == 0 || !safe(&self.hostname) || !safe(&self.user) || self.proxy_jump.as_ref().is_some_and(|s| !safe(s)) || self.known_hosts_file.as_os_str().is_empty() {
            return Err("INVALID_REQUEST: hostname, user, port or trust file is invalid".into());
        }
        let mut args: Vec<String> = ["-T", "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=yes", "-o", "ConnectTimeout=3", "-o", "ConnectionAttempts=1", "-o", "ServerAliveInterval=3", "-o", "ServerAliveCountMax=2", "-p"].into_iter().map(str::to_owned).collect();
        args.push(self.port.to_string());
        args.extend(["-o".into(), format!("UserKnownHostsFile={}", self.known_hosts_file.display())]);
        if let Some(key) = &self.identity_file { args.extend(["-i".into(), key.to_string_lossy().into_owned(), "-o".into(), "IdentitiesOnly=yes".into()]); }
        if let Some(jump) = &self.proxy_jump { args.extend(["-J".into(), jump.clone()]); }
        args.extend(["--".into(), format!("{}@{}", self.user, self.hostname)]);
        Ok(args)
    }
}
pub struct HostStore { pub path: PathBuf, pub lock: Mutex<()> }
impl HostStore {
    pub fn update(&self, hosts: &[HostConfig]) -> Result<(), String> {
        let _guard = self.lock.lock().map_err(|e| e.to_string())?;
        if self.path.exists() {
            let _: Vec<HostConfig> = serde_json::from_slice(&std::fs::read(&self.path).map_err(|e| e.to_string())?).map_err(|e| format!("HOST_STORE_CORRUPT: {e}"))?;
        }
        let mut ids = std::collections::HashSet::new();
        for host in hosts { host.argv()?; if !ids.insert(&host.id) { return Err("duplicate host ID".into()); } }
        let parent = self.path.parent().ok_or("missing store parent")?;
        let tmp = parent.join(format!(".hosts-{}", uuid::Uuid::new_v4()));
        let result = (|| {
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new(); options.write(true).create_new(true);
            #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
            let mut file = options.open(&tmp).map_err(|e| e.to_string())?;
            super::private_file(&tmp)?;
            file.write_all(&serde_json::to_vec(hosts).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
        })();
        if result.is_err() && tmp.exists() { std::fs::remove_file(tmp).map_err(|e| e.to_string())?; }
        result
    }
}
