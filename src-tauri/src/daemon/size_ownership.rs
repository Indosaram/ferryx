use std::collections::HashMap;

pub type ClientId = String;
pub const DESKTOP: &str = "desktop";

/// Preferences are per attachment. Losing ownership never implicitly retakes it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SizeOwnership {
    pub owner: ClientId,
    pub applied: (u16, u16),
    pub desktop: (u16, u16),
    #[serde(default)]
    pub generation: u64,
    preferred: HashMap<ClientId, (u16, u16)>,
}

impl SizeOwnership {
    pub fn new(size: (u16, u16)) -> Self {
        Self { owner: DESKTOP.into(), applied: size, desktop: size, generation: 0, preferred: HashMap::new() }
    }

    pub fn desktop_resize(&mut self, size: (u16, u16)) {
        self.desktop = size;
        self.owner = DESKTOP.into();
        self.applied = size;
    }

    pub fn remote_resize(&mut self, client: &str, size: (u16, u16), control: bool, take: bool) -> bool {
        if !control { return false; }
        let first = !self.preferred.contains_key(client);
        self.preferred.insert(client.into(), size);
        if first || take { self.owner = client.into(); }
        if self.owner != client { return false; }
        self.applied = size;
        true
    }

    pub fn detach(&mut self, client: &str) -> bool {
        self.preferred.remove(client);
        if self.owner != client { return false; }
        self.owner = DESKTOP.into();
        self.applied = self.desktop;
        true
    }
}

pub type SizeOwnershipSnapshot = HashMap<String, SizeOwnership>;
pub type SizeOwnershipRegistry = std::sync::Arc<parking_lot::Mutex<SizeOwnershipSnapshot>>;

pub fn persist(path: &std::path::Path, snapshot: &SizeOwnershipSnapshot) -> Result<(), String> {
    use std::io::Write;
    let bytes = serde_json::to_vec(snapshot).map_err(|error| error.to_string())?;
    let parent = path.parent().ok_or("Size ownership path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    file.write_all(&bytes).and_then(|_| file.as_file().sync_all()).map_err(|error| error.to_string())?;
    file.persist(path).map_err(|error| error.to_string())?;
    Ok(())
}

pub fn restore(path: &std::path::Path) -> Result<SizeOwnershipSnapshot, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(error.to_string()),
    };
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_ownership_restore_after_persisted_restart() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("size-ownership.json");
        let mut state = SizeOwnership::new((120, 40));
        state.remote_resize("phone-a", (60, 30), true, false);
        state.remote_resize("phone-b", (70, 35), true, false);
        state.remote_resize("phone-a", (61, 31), true, true);
        state.generation = 17;
        let before = HashMap::from([("pty-1".into(), state)]);
        persist(&path, &before).unwrap();
        let mut restored = restore(&path).unwrap();
        assert_eq!(restored, before);
        let state = restored.get_mut("pty-1").unwrap();
        assert!(!state.remote_resize("phone-b", (71, 36), true, false));
        assert!(state.detach("phone-a"));
        assert_eq!(state.applied, (120, 40));
        assert_eq!(state.generation, 17);
    }

    #[test]
    fn phone_detach_restores_desktop() {
        let mut state = SizeOwnership::new((120, 40));
        assert!(state.remote_resize("phone", (60, 30), true, false));
        assert!(state.detach("phone"));
        assert_eq!((state.owner.as_str(), state.applied), (DESKTOP, (120, 40)));
    }

    #[test]
    fn viewer_cannot_resize_or_take() {
        let mut state = SizeOwnership::new((120, 40));
        assert!(!state.remote_resize("viewer", (10, 10), false, true));
        assert_eq!((state.owner.as_str(), state.applied), (DESKTOP, (120, 40)));
    }

    #[test]
    fn desktop_focus_reclaims() {
        let mut state = SizeOwnership::new((120, 40));
        state.remote_resize("phone", (60, 30), true, false);
        state.desktop_resize((130, 45));
        assert!(!state.remote_resize("phone", (61, 31), true, false));
        assert_eq!(state.applied, (130, 45));
        assert_eq!(state.preferred["phone"], (61, 31));
    }

    #[test]
    fn last_explicit_taker_wins_without_oscillation() {
        let mut state = SizeOwnership::new((120, 40));
        state.remote_resize("a", (60, 30), true, false);
        state.remote_resize("b", (70, 35), true, false);
        state.remote_resize("a", (60, 30), true, true);
        for _ in 0..100 {
            assert!(!state.remote_resize("b", (70, 35), true, false));
            assert!(state.remote_resize("a", (60, 30), true, false));
            assert_eq!((state.owner.as_str(), state.applied), ("a", (60, 30)));
        }
        assert!(!state.detach("b"));
        assert_eq!(state.owner, "a");
    }
}
