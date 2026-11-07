//! Site permissions for browser tabs on both engines: camera, microphone, screen capture,
//! notifications, location, clipboard reading and anything else an engine asks about.
//!
//! Every request goes through [`Broker::request`]. A decision remembered for the site answers
//! at once; otherwise the request waits for the user's answer in the pane's prompt. An
//! [`Answer`] dropped unanswered (tab closed, engine gone, prompt dismissed) denies, so nothing
//! is granted without a remembered Allow or an explicit Allow click. The broker never calls an
//! answer itself: it returns it to the caller, who sends it after releasing its locks (an
//! engine may answer synchronously and call back in).
//!
//! Remembered decisions are per profile, origin and kind, stored host-side in
//! `<data>/browser-site-permissions.json`. The private profile never remembers anything.

pub use ferryx_browser_engine_protocol::PermissionKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[cfg(feature = "desktop")]
mod app;
#[cfg(feature = "desktop")]
pub use app::*;
#[cfg(all(feature = "desktop", target_os = "linux"))]
pub mod linux;
#[cfg(all(feature = "desktop", target_os = "macos"))]
pub mod macos;
#[cfg(all(feature = "desktop", target_os = "windows"))]
pub mod windows;

/// Where remembered decisions live under the app data dir.
pub const STORE_FILE: &str = "browser-site-permissions.json";
/// The profile whose decisions are never remembered.
const PRIVATE_PROFILE: &str = "private";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Decision {
    Allow,
    Block,
}

/// One remembered decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SitePermission {
    pub profile: String,
    pub origin: String,
    #[serde(with = "kind_name")]
    pub kind: PermissionKind,
    pub decision: Decision,
}

mod kind_name {
    use super::PermissionKind;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(kind: &PermissionKind, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(kind.as_str())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PermissionKind, D::Error> {
        let name = String::deserialize(deserializer)?;
        PermissionKind::parse(&name)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown permission kind {name}")))
    }
}

fn kind_names<S: serde::Serializer>(kinds: &[PermissionKind], serializer: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let mut seq = serializer.serialize_seq(Some(kinds.len()))?;
    for kind in kinds {
        seq.serialize_element(kind.as_str())?;
    }
    seq.end()
}

/// The origin a decision is remembered for: `scheme://host[:port]` of an http(s) URL or
/// origin. Anything else (file:, data:, about:, an opaque origin) has no origin to remember a
/// decision for, and its requests are denied without a prompt.
pub fn site_origin(url_or_origin: &str) -> Option<String> {
    let parsed = url::Url::parse(url_or_origin.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return None;
    }
    Some(parsed.origin().ascii_serialization())
}

/// What the store says about a request before anyone is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Block,
    Ask,
}

/// A site's standing for one kind, for features that check before (or without) prompting,
/// such as the page Notification shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SitePermissionDecision {
    Granted,
    Denied,
    /// Nothing remembered: a request has to ask the user.
    Ask,
}

/// The remembered decisions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SitePermissionStore {
    entries: Vec<SitePermission>,
}

impl SitePermissionStore {
    pub fn from_json(text: &str) -> serde_json::Result<Self> {
        serde_json::from_str(text)
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    pub fn entries(&self) -> &[SitePermission] {
        &self.entries
    }

    pub fn get(&self, profile: &str, origin: &str, kind: PermissionKind) -> Option<Decision> {
        self.entries
            .iter()
            .find(|entry| entry.profile == profile && entry.origin == origin && entry.kind == kind)
            .map(|entry| entry.decision)
    }

    /// Remembers a decision; returns false (and remembers nothing) for the private profile.
    pub fn set(&mut self, profile: &str, origin: &str, kind: PermissionKind, decision: Decision) -> bool {
        if profile == PRIVATE_PROFILE {
            return false;
        }
        self.entries
            .retain(|entry| !(entry.profile == profile && entry.origin == origin && entry.kind == kind));
        self.entries.push(SitePermission {
            profile: profile.to_string(),
            origin: origin.to_string(),
            kind,
            decision,
        });
        true
    }

    /// Forgets the site's decision for `kind`, or all of the site's decisions when `kind` is
    /// `None`. Returns whether anything was forgotten.
    pub fn revoke(&mut self, profile: &str, origin: &str, kind: Option<PermissionKind>) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| {
            !(entry.profile == profile && entry.origin == origin && kind.is_none_or(|kind| entry.kind == kind))
        });
        self.entries.len() != before
    }

    /// A remembered Block of any requested kind blocks; only a remembered Allow of every
    /// requested kind allows; anything else asks.
    pub fn verdict(&self, profile: &str, origin: &str, kinds: &[PermissionKind]) -> Verdict {
        if profile == PRIVATE_PROFILE || kinds.is_empty() {
            return Verdict::Ask;
        }
        let decisions: Vec<Option<Decision>> =
            kinds.iter().map(|kind| self.get(profile, origin, *kind)).collect();
        if decisions.contains(&Some(Decision::Block)) {
            Verdict::Block
        } else if decisions.iter().all(|decision| *decision == Some(Decision::Allow)) {
            Verdict::Allow
        } else {
            Verdict::Ask
        }
    }

    /// The remembered decision for one kind, as [`SitePermissionDecision`].
    pub fn decision(&self, profile: &str, origin: &str, kind: PermissionKind) -> SitePermissionDecision {
        match self.verdict(profile, origin, &[kind]) {
            Verdict::Allow => SitePermissionDecision::Granted,
            Verdict::Block => SitePermissionDecision::Denied,
            Verdict::Ask => SitePermissionDecision::Ask,
        }
    }
}

/// Reads the store; a missing file is an empty store. Blocking.
pub fn load_store(path: &Path) -> std::io::Result<SitePermissionStore> {
    match std::fs::read_to_string(path) {
        Ok(text) => SitePermissionStore::from_json(&text).map_err(std::io::Error::other),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(SitePermissionStore::default()),
        Err(error) => Err(error),
    }
}

/// Writes the store atomically (temp file, then rename). Blocking.
pub fn save_store(path: &Path, store: &SitePermissionStore) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, store.to_json().map_err(std::io::Error::other)?)?;
    std::fs::rename(&temp, path)
}

/// Delivers the decision to the engine: `true` grants everything the request asked for.
pub type Responder = Box<dyn FnOnce(bool) + Send>;

/// A request's one answer. Dropping it unanswered denies the request.
pub struct Answer(Option<Responder>);

impl Answer {
    pub fn new(responder: Responder) -> Self {
        Self(Some(responder))
    }

    pub fn send(mut self, allow: bool) {
        if let Some(responder) = self.0.take() {
            responder(allow);
        }
    }
}

impl Drop for Answer {
    fn drop(&mut self) {
        if let Some(responder) = self.0.take() {
            responder(false);
        }
    }
}

/// A request waiting for the user, as the pane's prompt shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPrompt {
    pub request_id: u64,
    pub browser_id: String,
    pub profile: String,
    pub origin: String,
    #[serde(serialize_with = "kind_names")]
    pub kinds: Vec<PermissionKind>,
    /// Whether the prompt may offer "Remember" (not for the private profile).
    pub can_remember: bool,
}

struct Pending {
    prompt: PendingPrompt,
    answer: Answer,
    /// The engine's own id for the request (CEF), to match a dismissal it reports.
    external: Option<String>,
}

/// What became of a request.
pub enum Outcome {
    /// Decided at once (a remembered decision, or denied because there is no web origin to
    /// ask about): send the answer with this value.
    Decided(Answer, bool),
    /// Waiting for the user.
    Prompt(PendingPrompt),
}

/// A request as the engine reports it.
pub struct PermissionAsk {
    pub browser_id: String,
    pub profile: String,
    /// The requesting page's URL or origin; `None` when the engine did not say.
    pub origin: Option<String>,
    pub kinds: Vec<PermissionKind>,
    /// The engine's own id for the request, when it can dismiss it on its own.
    pub external: Option<String>,
}

/// The user's answer, applied: the prompt it answered, the engine answer to send, and whether
/// the store changed.
pub struct Decided {
    pub prompt: PendingPrompt,
    pub answer: Answer,
    pub store_changed: bool,
}

/// Requests waiting for the user.
#[derive(Default)]
pub struct Broker {
    next_id: u64,
    pending: BTreeMap<u64, Pending>,
}

fn dedup_kinds(kinds: &[PermissionKind]) -> Vec<PermissionKind> {
    let mut kinds = kinds.to_vec();
    kinds.sort();
    kinds.dedup();
    kinds
}

impl Broker {
    pub fn request(&mut self, store: &SitePermissionStore, ask: PermissionAsk, answer: Answer) -> Outcome {
        let kinds = dedup_kinds(&ask.kinds);
        let Some(origin) = ask.origin.as_deref().and_then(site_origin) else {
            return Outcome::Decided(answer, false);
        };
        if kinds.is_empty() {
            return Outcome::Decided(answer, false);
        }
        match store.verdict(&ask.profile, &origin, &kinds) {
            Verdict::Allow => Outcome::Decided(answer, true),
            Verdict::Block => Outcome::Decided(answer, false),
            Verdict::Ask => {
                self.next_id += 1;
                let prompt = PendingPrompt {
                    request_id: self.next_id,
                    browser_id: ask.browser_id,
                    can_remember: ask.profile != PRIVATE_PROFILE,
                    profile: ask.profile,
                    origin,
                    kinds,
                };
                self.pending.insert(
                    prompt.request_id,
                    Pending { prompt: prompt.clone(), answer, external: ask.external },
                );
                Outcome::Prompt(prompt)
            }
        }
    }

    /// Applies the user's answer to the store; `None` when the request is no longer pending.
    /// The caller sends `answer` with `allow`.
    pub fn decide(
        &mut self,
        store: &mut SitePermissionStore,
        request_id: u64,
        allow: bool,
        remember: bool,
    ) -> Option<Decided> {
        let pending = self.pending.remove(&request_id)?;
        let mut store_changed = false;
        if remember {
            let decision = if allow { Decision::Allow } else { Decision::Block };
            for kind in &pending.prompt.kinds {
                store_changed |= store.set(&pending.prompt.profile, &pending.prompt.origin, *kind, decision);
            }
        }
        Some(Decided { prompt: pending.prompt, answer: pending.answer, store_changed })
    }

    /// The engine dropped a request on its own; dropping the returned answer denies it.
    pub fn dismiss_external(&mut self, external: &str) -> Option<(PendingPrompt, Answer)> {
        let id = self
            .pending
            .iter()
            .find(|(_, pending)| pending.external.as_deref() == Some(external))
            .map(|(id, _)| *id)?;
        self.pending.remove(&id).map(|pending| (pending.prompt, pending.answer))
    }

    /// The tab closed; dropping the returned answers denies its requests.
    pub fn forget_browser(&mut self, browser_id: &str) -> Vec<(PendingPrompt, Answer)> {
        let ids: Vec<u64> = self
            .pending
            .iter()
            .filter(|(_, pending)| pending.prompt.browser_id == browser_id)
            .map(|(id, _)| *id)
            .collect();
        ids.into_iter()
            .filter_map(|id| self.pending.remove(&id))
            .map(|pending| (pending.prompt, pending.answer))
            .collect()
    }

    /// The prompts a tab is waiting on, oldest first.
    pub fn pending_for(&self, browser_id: &str) -> Vec<PendingPrompt> {
        self.pending
            .values()
            .filter(|pending| pending.prompt.browser_id == browser_id)
            .map(|pending| pending.prompt.clone())
            .collect()
    }
}

/// Which capture devices a tab is using, for the pane's in-use indicator.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaCapture {
    pub camera: bool,
    pub microphone: bool,
    pub screen: bool,
}

impl MediaCapture {
    /// The capture a grant of `kinds` turns on.
    pub fn of(kinds: &[PermissionKind]) -> Self {
        Self {
            camera: kinds.contains(&PermissionKind::Camera),
            microphone: kinds.contains(&PermissionKind::Microphone),
            screen: kinds.contains(&PermissionKind::Screen),
        }
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            camera: self.camera || other.camera,
            microphone: self.microphone || other.microphone,
            screen: self.screen || other.screen,
        }
    }

    pub fn any(self) -> bool {
        self.camera || self.microphone || self.screen
    }
}

#[cfg(test)]
mod tests;
