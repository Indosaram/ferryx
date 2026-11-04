use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::{Path, PathBuf}, sync::Arc};

pub type Registry = Arc<parking_lot::Mutex<Store>>;
pub type Ledger = BTreeMap<String, u64>;

pub struct Store {
    pub ledger: Ledger,
    pub paused: bool,
    path: PathBuf,
}

#[derive(Default, Deserialize, Serialize)]
pub struct Settings {
    #[serde(default)]
    pub scripting: Scripting,
}

#[derive(Default, Deserialize, Serialize)]
pub struct Scripting {
    #[serde(default)]
    pub enabled: bool,
}

pub fn root() -> Result<PathBuf, String> {
    crate::worktree::naming::data_dir_with_env(|key| std::env::var_os(key))
        .ok_or_else(|| "No scripting data directory".into())
}

pub fn config_path() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    base.map(|base| base.join("ferryx/init.luau"))
        .ok_or_else(|| "No scripting config directory".into())
}

pub fn source(enabled: bool, path: &Path) -> Result<Option<String>, String> {
    if !enabled { return Ok(None); }
    match std::fs::read_to_string(path) {
        Ok(source) => Ok(Some(source)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

pub fn enabled(root: &Path) -> Result<bool, String> {
    match std::fs::read(root.join("settings.json")) {
        Ok(bytes) => serde_json::from_slice::<Settings>(&bytes)
            .map(|settings| settings.scripting.enabled).map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

pub fn write<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    use std::io::Write;
    let parent = path.parent().ok_or("Missing script data parent")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    file.write_all(&serde_json::to_vec(value).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    file.as_file().sync_all().map_err(|error| error.to_string())?;
    file.persist(path).map_err(|error| error.to_string())?;
    Ok(())
}

impl Store {
    pub fn load(root: &Path) -> Result<Self, String> {
        let path = root.join("script-ledger.json");
        let ledger = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string())?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ledger::new(),
            Err(error) => return Err(error.to_string()),
        };
        Ok(Self { ledger, paused: false, path })
    }

    pub fn adopt(&mut self, ledger: Ledger) -> Result<(), String> {
        write(&self.path, &ledger)?;
        self.ledger = ledger;
        Ok(())
    }

    pub fn claim(&mut self, id: &str, period: u64, now: u64) -> Result<bool, String> {
        if self.paused || self.ledger.get(id).is_some_and(|last| now.saturating_sub(*last) < period) {
            return Ok(false);
        }
        let mut next = self.ledger.clone();
        next.insert(id.into(), now);
        self.adopt(next)?;
        Ok(true)
    }
}
