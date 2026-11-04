use std::collections::HashMap;

pub type ClientId = String;
pub const DESKTOP: &str = "desktop";

/// Preferences are per attachment. Losing ownership never implicitly retakes it.
#[derive(Debug, Clone)]
pub struct SizeOwnership {
    pub owner: ClientId,
    pub applied: (u16, u16),
    pub desktop: (u16, u16),
    preferred: HashMap<ClientId, (u16, u16)>,
}

impl SizeOwnership {
    pub fn new(size: (u16, u16)) -> Self {
        Self { owner: DESKTOP.into(), applied: size, desktop: size, preferred: HashMap::new() }
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

#[cfg(test)]
mod tests {
    use super::*;

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
