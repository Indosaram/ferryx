//! The one seam browser features call to ask whether a site may use a capability.
//!
//! The site-permissions lane owns the real policy (prompts, per-origin store, settings). Until
//! it lands, [`decide`] answers through the installed [`SitePermissionPolicy`], and the default
//! policy denies everything, so no site gains a capability by accident.

use parking_lot::RwLock;
use std::sync::{Arc, LazyLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SitePermission {
    Notifications,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SitePermissionDecision {
    Granted,
    Denied,
    /// Not decided yet: the policy may ask the user and answer later.
    Ask,
}

pub trait SitePermissionPolicy: Send + Sync {
    /// `origin` is `scheme://host[:port]`.
    fn decide(&self, origin: &str, permission: SitePermission) -> SitePermissionDecision {
        let _ = (origin, permission);
        SitePermissionDecision::Denied
    }
}

/// Denies every request.
pub struct DenyAll;

impl SitePermissionPolicy for DenyAll {}

static POLICY: LazyLock<RwLock<Arc<dyn SitePermissionPolicy>>> =
    LazyLock::new(|| RwLock::new(Arc::new(DenyAll)));

/// Installs the app's policy (the site-permissions lane calls this at startup).
pub fn install_policy(policy: Arc<dyn SitePermissionPolicy>) {
    *POLICY.write() = policy;
}

pub fn decide(origin: &str, permission: SitePermission) -> SitePermissionDecision {
    let policy = Arc::clone(&POLICY.read());
    policy.decide(origin, permission)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_policy_denies() {
        assert_eq!(
            DenyAll.decide("https://mail.test", SitePermission::Notifications),
            SitePermissionDecision::Denied
        );
    }
}
