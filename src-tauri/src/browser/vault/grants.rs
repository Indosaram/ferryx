//! Ephemeral, exact-origin, single-use credential capabilities. No secret is stored here.
use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rand::RngCore;

use super::CredentialOrigin;
use crate::browser::agent::VaultAction;

pub const GRANT_TTL: Duration = Duration::from_secs(60);
pub const MAX_GRANTS: usize = 32;

struct Grant {
    handle: String,
    item_id: String,
    origin: CredentialOrigin,
    action: VaultAction,
    expires: Instant,
}

#[derive(Default)]
pub struct CredentialGrants {
    grants: Mutex<HashMap<String, Grant>>,
    /// Monotonic generation. Revocation, reclaim, and navigation bump it; a fill compares the
    /// value it captured before reading the secret against this one immediately before the
    /// write, so an invalidation landing between the fence and the write is not lost.
    generation: Mutex<u64>,
}

impl CredentialGrants {
    pub fn issue(&self, handle: &str, item_id: &str, origin: CredentialOrigin, action: VaultAction) -> String {
        self.issue_at(handle, item_id, origin, action, Instant::now())
    }

    fn issue_at(&self, handle: &str, item_id: &str, origin: CredentialOrigin, action: VaultAction, now: Instant) -> String {
        let mut grants = self.grants.lock();
        grants.retain(|_, grant| now < grant.expires);
        if grants.len() >= MAX_GRANTS {
            if let Some(oldest) = grants.iter().min_by_key(|(_, grant)| grant.expires).map(|(id, _)| id.clone()) {
                grants.remove(&oldest);
            }
        }
        let token = loop {
            let mut bytes = [0u8; 16];
            rand::rngs::OsRng.fill_bytes(&mut bytes);
            let candidate: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            if !grants.contains_key(&candidate) { break candidate; }
        };
        grants.insert(token.clone(), Grant {
            handle: if handle.is_empty() { token.clone() } else { handle.to_string() },
            item_id: item_id.to_string(), origin, action, expires: now + GRANT_TTL,
        });
        token
    }

    /// Removes even an incorrectly presented token: a capability gets one attempt.
    pub fn consume(&self, token: &str, handle: &str, action: VaultAction, origin: &CredentialOrigin) -> bool {
        self.take(token, handle, action, origin).is_some()
    }

    pub(crate) fn take(&self, token: &str, handle: &str, action: VaultAction, origin: &CredentialOrigin) -> Option<String> {
        self.take_at(token, handle, action, origin, Instant::now())
    }

    fn take_at(&self, token: &str, handle: &str, action: VaultAction, origin: &CredentialOrigin, now: Instant) -> Option<String> {
        let grant = self.grants.lock().remove(token)?;
        (grant.handle == handle && grant.action == action && &grant.origin == origin && now < grant.expires)
            .then_some(grant.item_id)
    }

    /// Clears every outstanding grant and invalidates any fill already in flight.
    pub fn revoke_all(&self) {
        self.grants.lock().clear();
        self.bump_generation();
    }

    /// The value a fill captures before it reads the secret.
    pub fn generation(&self) -> u64 { *self.generation.lock() }

    /// Invalidates in-flight fills without touching outstanding grants (reclaim, navigation).
    pub fn bump_generation(&self) {
        let mut g = self.generation.lock();
        *g = g.wrapping_add(1);
    }

    /// True when nothing invalidated the fill since the value was captured.
    pub fn still_current(&self, captured: u64) -> bool { self.generation() == captured }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn origin(host: &str) -> CredentialOrigin { format!("https://{host}").try_into().unwrap() }
    // F3/F4: a fill captures the generation before reading the secret and re-checks it right
    // before the write. Any invalidation in that window must be visible to the check.
    #[test]
    fn generation_invalidates_an_in_flight_fill() {
        let grants = CredentialGrants::default();
        let captured = grants.generation();
        assert!(grants.still_current(captured));
        grants.bump_generation();
        assert!(!grants.still_current(captured));
    }

    // F3: a full revocation (origin revoke / controller revoke) must also invalidate.
    #[test]
    fn revoke_all_invalidates_an_in_flight_fill() {
        let grants = CredentialGrants::default();
        let now = Instant::now();
        let token = grants.issue_at("h", "item", origin("a.test"), VaultAction::Fill, now);
        let captured = grants.generation();
        grants.revoke_all();
        assert!(!grants.still_current(captured), "a revoked authority must abort the fill");
        assert!(grants.take_at(&token, "h", VaultAction::Fill, &origin("a.test"), now).is_none());
    }

    #[test]
    fn expired_grant_is_refused() {
        let grants = CredentialGrants::default();
        let now = Instant::now();
        let token = grants.issue_at("h", "item", origin("a.test"), VaultAction::Fill, now);
        assert!(grants.take_at(&token, "h", VaultAction::Fill, &origin("a.test"), now + GRANT_TTL).is_none());
    }
    #[test]
    fn action_is_bound() {
        let grants = CredentialGrants::default();
        let token = grants.issue("h", "item", origin("a.test"), VaultAction::RequestUse);
        assert!(!grants.consume(&token, "h", VaultAction::Fill, &origin("a.test")));
    }
    #[test]
    fn origin_is_bound() {
        let grants = CredentialGrants::default();
        let token = grants.issue("h", "item", origin("a.test"), VaultAction::Fill);
        assert!(!grants.consume(&token, "h", VaultAction::Fill, &origin("b.test")));
    }
    #[test]
    fn consumed_grant_cannot_be_reused() {
        let grants = CredentialGrants::default();
        let token = grants.issue("h", "item", origin("a.test"), VaultAction::Fill);
        assert!(grants.consume(&token, "h", VaultAction::Fill, &origin("a.test")));
        assert!(!grants.consume(&token, "h", VaultAction::Fill, &origin("a.test")));
    }
    #[test]
    fn handle_binding_and_revocation_are_enforced() {
        let grants = CredentialGrants::default();
        let token = grants.issue("h", "item", origin("a.test"), VaultAction::Fill);
        assert!(!grants.consume(&token, "other", VaultAction::Fill, &origin("a.test")));
        let token = grants.issue("h", "item", origin("a.test"), VaultAction::Fill);
        grants.revoke_all();
        assert!(!grants.consume(&token, "h", VaultAction::Fill, &origin("a.test")));
    }
    #[test]
    fn thirty_third_issue_evicts_oldest() {
        let grants = CredentialGrants::default();
        let now = Instant::now();
        let first = grants.issue_at("h", "item", origin("a.test"), VaultAction::Fill, now);
        for i in 1..=MAX_GRANTS {
            grants.issue_at("h", "item", origin("a.test"), VaultAction::Fill, now + Duration::from_millis(i as u64));
        }
        assert_eq!(grants.grants.lock().len(), MAX_GRANTS);
        assert!(!grants.consume(&first, "h", VaultAction::Fill, &origin("a.test")));
    }
}
