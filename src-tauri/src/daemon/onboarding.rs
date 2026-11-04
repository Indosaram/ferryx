//! Onboarding is independent of terminal snapshots and available before daemon startup.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const CAPABILITY: &str = "onboardingStateV1";
pub const STEPS: &[&str] = &["intro", "features", "agents", "enroll", "skills", "cli",
    "project", "supervisor", "try", "phone", "account"];
pub type Registry = std::sync::Arc<parking_lot::Mutex<Store>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    pub version: u8,
    pub completed_steps: Vec<String>,
    pub dismissed: bool,
}

impl Default for State {
    fn default() -> Self { Self { version: 1, completed_steps: vec![], dismissed: false } }
}

impl State {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.completed_steps.len() > STEPS.len()
            || self.completed_steps.iter().any(|step| !STEPS.contains(&step.as_str())) {
            return Err("Invalid onboarding state".into());
        }
        Ok(())
    }
}

pub fn path(daemon: bool) -> Result<PathBuf, String> {
    crate::worktree::naming::data_dir_with_env(std::env::var_os)
        .map(|root| root.join(if daemon { "onboarding-daemon.json" } else { "onboarding.json" }))
        .ok_or_else(|| "No onboarding data directory".into())
}

/// Missing bootstrap is seeded from the GUI cache; corrupt data is repaired, not trusted.
pub fn read(path: &Path) -> Result<Option<State>, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    match serde_json::from_slice::<State>(&bytes) {
        Ok(state) if state.validate().is_ok() => Ok(Some(state)),
        _ => { let state = State::default(); write(path, &state)?; Ok(Some(state)) }
    }
}

pub fn write(path: &Path, state: &State) -> Result<(), String> {
    use std::io::Write;
    state.validate()?;
    let parent = path.parent().ok_or("Missing onboarding parent")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    file.write_all(&serde_json::to_vec(state).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    file.as_file().sync_all().map_err(|error| error.to_string())?;
    file.persist(path).map_err(|error| error.to_string())?;
    Ok(())
}

pub struct Store { path: PathBuf }
impl Store {
    pub fn new(path: PathBuf) -> Self { Self { path } }
    pub fn get(&self) -> Result<Option<State>, String> { read(&self.path) }
    pub fn set(&self, state: State, migrate: bool) -> Result<State, String> {
        if migrate { if let Some(current) = self.get()? { return Ok(current); } }
        write(&self.path, &state)?;
        Ok(state)
    }
}

pub fn qa_no_daemon_value(debug: bool, value: Option<&std::ffi::OsStr>) -> bool {
    debug && value == Some(std::ffi::OsStr::new("1"))
}
pub fn qa_no_daemon() -> bool {
    qa_no_daemon_value(cfg!(debug_assertions), std::env::var_os("FERRYX_QA_NO_DAEMON").as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bootstrap_restart_corruption_and_daemon_migration_once() {
        let dir = tempfile::tempdir().unwrap();
        let bootstrap = dir.path().join("onboarding.json");
        let state = State { completed_steps: vec!["intro".into(), "features".into(), "agents".into()], ..State::default() };
        write(&bootstrap, &state).unwrap();
        assert_eq!(read(&bootstrap).unwrap(), Some(state.clone()));
        let daemon = dir.path().join("onboarding-daemon.json");
        Store::new(daemon.clone()).set(state.clone(), true).unwrap();
        assert_eq!(Store::new(daemon.clone()).set(State::default(), true).unwrap(), state);
        assert_eq!(Store::new(daemon).get().unwrap(), Some(state));
        std::fs::write(&bootstrap, b"broken").unwrap();
        assert_eq!(read(&bootstrap).unwrap(), Some(State::default()));
        assert_eq!(read(&bootstrap).unwrap(), Some(State::default()));
    }
    #[test]
    fn qa_daemon_bypass_is_debug_only_and_opt_in() {
        let one = Some(std::ffi::OsStr::new("1"));
        assert!(qa_no_daemon_value(true, one));
        assert!(!qa_no_daemon_value(false, one));
        assert!(!qa_no_daemon_value(true, None));
    }
}
