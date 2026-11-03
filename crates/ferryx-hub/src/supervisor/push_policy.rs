//! Phone-push policy for decision cards (plan D13), kept pure so the hub tests exercise the
//! real filter, mute, dedupe and batch-window logic. The gateway (`src-tauri`) owns delivery
//! and persistence; it asks this module whether a card may push and what to send.
//!
//! Push is for new `panel.decision` escalations only: retries, recoveries and finished sessions
//! arrive as other frames and never reach [`Admission::Send`].

use super::panel_contract::{Escalation, SessionRef};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Batch window for decision pushes: the hub's own `lanes::dispatch::BATCH_WINDOW_SECS`.
pub const DECISION_BATCH_WINDOW: Duration = Duration::from_secs(crate::lanes::dispatch::BATCH_WINDOW_SECS as u64);

/// How long a "this client shows the panel" report stays believed. Clients re-report on a
/// shorter heartbeat, so a crashed or backgrounded client cannot silence pushes forever.
pub const PANEL_PRESENCE_STALE: Duration = Duration::from_secs(120);

/// Sessions remembered at once. Past the bound the dedupe memory restarts.
const TRACKED_SESSIONS: usize = 1024;

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The path a decision push opens: the panel on that session's card. `session_ref` is
/// `<hostId>:<backendSessionId>`, so it is percent-encoded.
pub fn decision_deep_link(session_ref: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(session_ref.as_bytes()).collect();
    format!("/?panel=decision&session={encoded}")
}

/// The path that opens the panel without naming one card (several cards held by the window).
pub const PANEL_DEEP_LINK: &str = "/?panel=decision";

/// What one owner panel frame means for push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelFrame {
    /// Not a decision frame, or a malformed one: never a push.
    Ignore,
    /// The card left the panel (answered, withdrawn): its next escalation is new again.
    Resolved { session_ref: String },
    /// An escalation appeared for `session_ref`.
    Decision { session_ref: String, prompt_id: String },
}

/// Reads a frame from the owner panel channel. Only `panel.decision` can matter; the frame's
/// `sessionRef` must name the same session as the escalation inside it, so a card can never push
/// a deep link to another session.
pub fn classify_frame(frame: &Value) -> PanelFrame {
    if frame.get("type").and_then(Value::as_str) != Some("panel.decision") {
        return PanelFrame::Ignore;
    }
    let Some(session_ref) = frame
        .get("sessionRef")
        .and_then(Value::as_str)
        .filter(|raw| SessionRef::parse(raw).is_ok())
    else {
        return PanelFrame::Ignore;
    };
    let Some(decision) = frame.get("decision").filter(|value| !value.is_null()) else {
        return PanelFrame::Resolved { session_ref: session_ref.to_string() };
    };
    match serde_json::from_value::<Escalation>(decision.clone()) {
        Ok(escalation) if escalation.session_ref.as_str() == session_ref => PanelFrame::Decision {
            session_ref: session_ref.to_string(),
            prompt_id: escalation.prompt.prompt_id,
        },
        _ => PanelFrame::Ignore,
    }
}

/// What the window does with a decision that passed every other check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowVerdict {
    /// Send now.
    Send,
    /// Held for the open window; `flush_in` is set for the first held card, which owns the flush.
    Held { flush_in: Option<Duration> },
}

/// Why a decision did not push.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// The owner muted this session: it stays in the panel, it never pushes.
    Muted,
    /// The same card was already announced for this session.
    NotNew,
    /// A client shows the panel right now, so the card is already in front of the owner.
    PanelShown,
}

/// The gate's answer for one decision frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Skipped(Skip),
    Send,
    Held { flush_in: Option<Duration> },
}

#[derive(Debug, Default)]
struct GateInner {
    /// The prompt id last announced per session ref: a repeat of it is not a new card.
    announced: HashMap<String, String>,
    last_sent: Option<Instant>,
    held: Vec<String>,
}

/// Decides which decision frames are new cards and paces them: at most one push per window,
/// the rest folded into the flush.
#[derive(Debug, Default)]
pub struct DecisionGate {
    inner: Mutex<GateInner>,
}

impl DecisionGate {
    /// The whole policy for one decision frame. Order matters: a muted session is not even
    /// recorded as announced; a card seen while the panel is shown is recorded so it never
    /// pushes later; only then does the batch window decide between sending and holding.
    pub fn admit(
        &self,
        session_ref: &str,
        prompt_id: &str,
        muted: bool,
        panel_shown: bool,
        now: Instant,
    ) -> Admission {
        if muted {
            return Admission::Skipped(Skip::Muted);
        }
        if !self.is_new_card(session_ref, prompt_id) {
            return Admission::Skipped(Skip::NotNew);
        }
        if panel_shown {
            return Admission::Skipped(Skip::PanelShown);
        }
        match self.window(session_ref, now) {
            WindowVerdict::Send => Admission::Send,
            WindowVerdict::Held { flush_in } => Admission::Held { flush_in },
        }
    }

    /// True when `prompt_id` is a card this session has not announced yet; records it.
    pub fn is_new_card(&self, session_ref: &str, prompt_id: &str) -> bool {
        let mut inner = locked(&self.inner);
        if inner.announced.get(session_ref).is_some_and(|last| last == prompt_id) {
            return false;
        }
        if !inner.announced.contains_key(session_ref) && inner.announced.len() >= TRACKED_SESSIONS {
            inner.announced.clear();
        }
        inner.announced.insert(session_ref.to_string(), prompt_id.to_string());
        true
    }

    /// The card left the panel (answered or gone): its next escalation is new again.
    pub fn resolved(&self, session_ref: &str) {
        locked(&self.inner).announced.remove(session_ref);
    }

    /// Admits or holds one card against the batch window. A session already held stays held
    /// once: the flush names sessions, not frames.
    pub fn window(&self, session_ref: &str, now: Instant) -> WindowVerdict {
        let mut inner = locked(&self.inner);
        let open = inner
            .last_sent
            .and_then(|sent| DECISION_BATCH_WINDOW.checked_sub(now.saturating_duration_since(sent)))
            .filter(|left| !left.is_zero());
        match open {
            None => {
                inner.last_sent = Some(now);
                WindowVerdict::Send
            }
            Some(left) => {
                if inner.held.iter().any(|held| held == session_ref) {
                    return WindowVerdict::Held { flush_in: None };
                }
                inner.held.push(session_ref.to_string());
                WindowVerdict::Held { flush_in: (inner.held.len() == 1).then_some(left) }
            }
        }
    }

    /// Takes the held cards at flush time; a non-empty flush starts a new window.
    pub fn take_held(&self, now: Instant) -> Vec<String> {
        let mut inner = locked(&self.inner);
        let held = std::mem::take(&mut inner.held);
        if !held.is_empty() {
            inner.last_sent = Some(now);
        }
        held
    }

    /// The cards to push at flush time: held ones, minus sessions muted meanwhile, none at all
    /// while a client shows the panel.
    pub fn flush(&self, now: Instant, is_muted: impl Fn(&str) -> bool, panel_shown: bool) -> Vec<String> {
        let mut held = self.take_held(now);
        held.retain(|session_ref| !is_muted(session_ref));
        if panel_shown {
            held.clear();
        }
        held
    }
}

/// Which clients currently show the panel. Any one of them keeps pushes back.
#[derive(Debug, Default)]
pub struct PanelPresence {
    viewers: Mutex<HashMap<String, Instant>>,
}

impl PanelPresence {
    /// A client (keyed by device id) reports whether it shows the panel right now.
    pub fn report(&self, viewer: &str, visible: bool, now: Instant) {
        let mut viewers = locked(&self.viewers);
        viewers.retain(|_, at| now.saturating_duration_since(*at) < PANEL_PRESENCE_STALE);
        if visible {
            viewers.insert(viewer.to_string(), now);
        } else {
            viewers.remove(viewer);
        }
    }

    /// True while any client's last visible report is fresh.
    pub fn shown(&self, now: Instant) -> bool {
        locked(&self.viewers)
            .values()
            .any(|at| now.saturating_duration_since(*at) < PANEL_PRESENCE_STALE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn frame(session_ref: &str, prompt_id: &str) -> Value {
        json!({
            "type": "panel.decision",
            "sessionRef": session_ref,
            "decision": {
                "sessionRef": session_ref,
                "prompt": {
                    "promptId": prompt_id, "kind": "approval", "question": "Run it?",
                    "command": null, "options": ["Yes", "No"], "screenRevision": null
                },
                "recommendedIndex": 1,
                "reason": "Remote database.",
                "createdAtMs": 1
            }
        })
    }

    #[test]
    fn the_batch_window_is_the_hubs_own_thirty_seconds() {
        assert_eq!(DECISION_BATCH_WINDOW.as_secs() as i64, crate::lanes::dispatch::BATCH_WINDOW_SECS);
        assert_eq!(DECISION_BATCH_WINDOW, Duration::from_secs(30));
    }

    #[test]
    fn only_a_decision_frame_with_a_matching_escalation_can_push() {
        assert_eq!(
            classify_frame(&frame("mac:s1", "p1")),
            PanelFrame::Decision { session_ref: "mac:s1".into(), prompt_id: "p1".into() }
        );
        // Recovery retries, finished sessions, rules and unknown frames never push.
        for kind in ["panel.action", "panel.session", "panel.rules", "supervisorChanged"] {
            let mut other = frame("mac:s1", "p1");
            other["type"] = json!(kind);
            assert_eq!(classify_frame(&other), PanelFrame::Ignore, "{kind}");
        }
        // A card for another session than the frame names would deep-link to the wrong card.
        let mut crossed = frame("mac:s1", "p1");
        crossed["decision"]["sessionRef"] = json!("mac:s2");
        assert_eq!(classify_frame(&crossed), PanelFrame::Ignore);
        // Malformed refs and escalations are ignored, not guessed at.
        assert_eq!(classify_frame(&frame("no-colon", "p1")), PanelFrame::Ignore);
        let mut unusable = frame("mac:s1", "p1");
        unusable["decision"] = json!({"reason": "x"});
        assert_eq!(classify_frame(&unusable), PanelFrame::Ignore);
        // A withdrawn card re-arms its session.
        assert_eq!(
            classify_frame(&json!({"type": "panel.decision", "sessionRef": "mac:s1", "decision": null})),
            PanelFrame::Resolved { session_ref: "mac:s1".into() }
        );
    }

    #[test]
    fn the_deep_link_names_the_card_and_survives_colons_and_spaces() {
        assert_eq!(decision_deep_link("mac:s1"), "/?panel=decision&session=mac%3As1");
        assert_eq!(decision_deep_link("a b:c&d"), "/?panel=decision&session=a+b%3Ac%26d");
        assert_eq!(PANEL_DEEP_LINK, "/?panel=decision");
    }

    #[test]
    fn a_muted_session_never_pushes_and_is_not_marked_announced() {
        let gate = DecisionGate::default();
        let t0 = Instant::now();
        assert_eq!(gate.admit("mac:s1", "p1", true, false, t0), Admission::Skipped(Skip::Muted));
        // Unmuted, the same card is still new: the mute must not have consumed it.
        assert_eq!(gate.admit("mac:s1", "p1", false, false, t0), Admission::Send);
        // The mute is per session.
        assert_eq!(gate.admit("mac:s2", "p1", true, false, t0), Admission::Skipped(Skip::Muted));
    }

    #[test]
    fn a_repeated_card_is_not_new_until_it_resolves() {
        let gate = DecisionGate::default();
        let t0 = Instant::now();
        assert_eq!(gate.admit("mac:s1", "p1", false, false, t0), Admission::Send);
        let later = t0 + DECISION_BATCH_WINDOW * 2;
        assert_eq!(gate.admit("mac:s1", "p1", false, false, later), Admission::Skipped(Skip::NotNew));
        assert_eq!(gate.admit("mac:s1", "p2", false, false, later), Admission::Send, "a new prompt is a new card");
        gate.resolved("mac:s1");
        let much_later = later + DECISION_BATCH_WINDOW * 2;
        assert_eq!(gate.admit("mac:s1", "p2", false, false, much_later), Admission::Send);
    }

    #[test]
    fn a_shown_panel_suppresses_the_card_for_good() {
        let gate = DecisionGate::default();
        let t0 = Instant::now();
        assert_eq!(gate.admit("mac:s1", "p1", false, true, t0), Admission::Skipped(Skip::PanelShown));
        // The card was seen on the panel: showing the panel does not queue it, and leaving the
        // panel later does not resurrect it as a push.
        assert_eq!(gate.admit("mac:s1", "p1", false, false, t0), Admission::Skipped(Skip::NotNew));
        // A suppressed card does not open the batch window for the next one.
        assert_eq!(gate.admit("mac:s2", "p1", false, false, t0), Admission::Send);
    }

    #[test]
    fn the_window_sends_one_holds_the_rest_and_flushes_once() {
        let gate = DecisionGate::default();
        let t0 = Instant::now();
        assert_eq!(gate.admit("a", "p", false, false, t0), Admission::Send);
        assert_eq!(
            gate.admit("b", "p", false, false, t0 + Duration::from_secs(5)),
            Admission::Held { flush_in: Some(Duration::from_secs(25)) }
        );
        assert_eq!(
            gate.admit("c", "p", false, false, t0 + Duration::from_secs(6)),
            Admission::Held { flush_in: None },
            "only the first held card owns the flush"
        );
        let flushed = gate.flush(t0 + Duration::from_secs(30), |_| false, false);
        assert_eq!(flushed, vec!["b".to_string(), "c".to_string()]);
        // The flush opened a new window.
        assert!(matches!(gate.window("d", t0 + Duration::from_secs(31)), WindowVerdict::Held { .. }));
        assert_eq!(gate.take_held(t0 + Duration::from_secs(62)), vec!["d".to_string()]);
        // Quiet for a whole window: the next card goes straight out.
        assert_eq!(gate.window("e", t0 + Duration::from_secs(93)), WindowVerdict::Send);
    }

    #[test]
    fn a_session_held_twice_is_flushed_once() {
        let gate = DecisionGate::default();
        let t0 = Instant::now();
        assert_eq!(gate.window("a", t0), WindowVerdict::Send);
        assert!(matches!(gate.window("b", t0 + Duration::from_secs(1)), WindowVerdict::Held { flush_in: Some(_) }));
        assert_eq!(gate.window("b", t0 + Duration::from_secs(2)), WindowVerdict::Held { flush_in: None });
        assert_eq!(gate.take_held(t0 + DECISION_BATCH_WINDOW), vec!["b".to_string()]);
    }

    #[test]
    fn the_flush_drops_sessions_muted_meanwhile_and_everything_while_the_panel_is_shown() {
        let gate = DecisionGate::default();
        let t0 = Instant::now();
        let hold = |gate: &DecisionGate| {
            gate.window("a", t0);
            gate.window("b", t0 + Duration::from_secs(1));
            gate.window("c", t0 + Duration::from_secs(2));
        };
        hold(&gate);
        assert_eq!(
            gate.flush(t0 + DECISION_BATCH_WINDOW, |session| session == "b", false),
            vec!["c".to_string()]
        );

        let shown = DecisionGate::default();
        hold(&shown);
        assert!(shown.flush(t0 + DECISION_BATCH_WINDOW, |_| false, true).is_empty());
        assert!(shown.take_held(t0 + DECISION_BATCH_WINDOW).is_empty(), "the held cards were consumed");
    }

    #[test]
    fn any_fresh_viewer_keeps_pushes_back_and_a_stale_or_hidden_one_does_not() {
        let presence = PanelPresence::default();
        let t0 = Instant::now();
        assert!(!presence.shown(t0));
        presence.report("phone", true, t0);
        presence.report("tablet", true, t0 + Duration::from_secs(10));
        assert!(presence.shown(t0 + Duration::from_secs(11)));
        // One viewer leaving does not unsuppress while another still shows the panel.
        presence.report("phone", false, t0 + Duration::from_secs(12));
        assert!(presence.shown(t0 + Duration::from_secs(13)));
        presence.report("tablet", false, t0 + Duration::from_secs(14));
        assert!(!presence.shown(t0 + Duration::from_secs(15)));
        // A client that stops heartbeating stops counting.
        presence.report("phone", true, t0 + Duration::from_secs(20));
        assert!(presence.shown(t0 + Duration::from_secs(20) + PANEL_PRESENCE_STALE - Duration::from_secs(1)));
        assert!(!presence.shown(t0 + Duration::from_secs(20) + PANEL_PRESENCE_STALE));
    }
}
