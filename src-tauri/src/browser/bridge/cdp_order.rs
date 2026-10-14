//! Ordering of the WebView2 trusted-bridge CDP setup (design 3.3), as a pure state machine so
//! it can be unit-tested on every host. `windows.rs` feeds it CDP completions and executes
//! the commands it returns.
//!
//! The order is the fail-safe one: the binding is added and the page-world leak probe must
//! pass BEFORE the broker is registered with Page.addScriptToEvaluateOnNewDocument. A failure
//! at any stage removes the binding and, if it was registered, the broker script (by the
//! identifier CDP returned), so a degraded tab runs no broker and keeps native link and
//! download behaviour instead of dead clicks.
//!
//! Runtime.removeBinding only unsubscribes the agent; it leaves the `__ferryxTrusted`
//! function on the world's global object. So a downgrade from a stage where the broker may
//! already run in the current document first disarms it in the tracked main-frame
//! ferryx-bridge world (DISARM_BROKER_SCRIPT), after which broker.js `channel()` returns
//! null and clicks fall back to native behaviour.
//!
//! A leak-probe error is not a leak: it is retried once per probe, and only a second error
//! (or a positive `typeof` result other than "undefined") downgrades.

/// Evaluated in the tracked main-frame ferryx-bridge world on downgrade. broker.js
/// `channel()` checks the flag; the delete is belt and braces.
pub const DISARM_BROKER_SCRIPT: &str =
    "globalThis.__ferryxBridgeDisabled = true; delete globalThis.__ferryxTrusted; true";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CdpBridgeStage {
    /// Page.enable, Page.getFrameTree, Runtime.enable and Runtime.addBinding in flight.
    #[default]
    Setup,
    /// The binding exists; the page-world leak probe is running.
    Probing,
    /// The probe passed; Page.addScriptToEvaluateOnNewDocument is in flight.
    RegisteringBroker,
    /// Broker registered, tab upgraded.
    Isolated,
    /// Sticky: something failed or leaked; the binding and broker were removed.
    Downgraded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CdpBridgeEvent {
    /// The setup chain (ending with Runtime.addBinding) succeeded.
    SetupSucceeded,
    /// The leak probe saw `typeof __ferryxTrusted === "undefined"` in the page world.
    ProbePassed,
    /// Page.addScriptToEvaluateOnNewDocument returned this identifier.
    BrokerRegistered(String),
    /// The leak probe could not produce a verdict (ExecuteScript error or a non-string reply).
    ProbeErrored,
    /// A new main-frame ferryx-bridge world appeared while isolated: re-probe the new document.
    MainWorldCreated,
    /// Any CDP failure, a positive leak, or a repeated probe error.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CdpBridgeCommand {
    RunLeakProbe,
    RegisterBroker,
    /// Record TrustedBridgeKind::Isolated and give the current document its world.
    BecomeIsolated,
    /// Runtime.evaluate DISARM_BROKER_SCRIPT in this ferryx-bridge execution context.
    DisarmBroker(i64),
    RemoveBinding,
    /// Page.removeScriptToEvaluateOnNewDocument with this identifier.
    RemoveBroker(String),
    /// Record TrustedBridgeKind::Unavailable.
    MarkUnavailable,
}

/// The leak-probe verdict for one ExecuteScript completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// `typeof __ferryxTrusted` was "undefined" in the page world.
    Passed,
    /// `typeof` returned another string: the page world sees something by that name.
    Leaked,
    /// The script errored (None) or returned no string, so there is no verdict.
    Inconclusive,
}

impl ProbeOutcome {
    pub fn event(self) -> CdpBridgeEvent {
        match self {
            ProbeOutcome::Passed => CdpBridgeEvent::ProbePassed,
            ProbeOutcome::Leaked => CdpBridgeEvent::Failed,
            ProbeOutcome::Inconclusive => CdpBridgeEvent::ProbeErrored,
        }
    }
}

/// ExecuteScript result JSON (None when the call itself failed) -> verdict.
pub fn leak_probe_outcome(result: Option<&str>) -> ProbeOutcome {
    match result.map(|raw| serde_json::from_str::<String>(raw.trim())) {
        Some(Ok(value)) if value == "undefined" => ProbeOutcome::Passed,
        Some(Ok(_)) => ProbeOutcome::Leaked,
        _ => ProbeOutcome::Inconclusive,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CdpBridgeLifecycle {
    stage: CdpBridgeStage,
    broker_script_id: Option<String>,
    /// The current probe already used its one retry after an error.
    probe_retried: bool,
}

impl CdpBridgeLifecycle {
    pub fn stage(&self) -> CdpBridgeStage {
        self.stage
    }

    pub fn is_isolated(&self) -> bool {
        self.stage == CdpBridgeStage::Isolated
    }

    pub fn is_downgraded(&self) -> bool {
        self.stage == CdpBridgeStage::Downgraded
    }

    pub fn broker_script_id(&self) -> Option<&str> {
        self.broker_script_id.as_deref()
    }

    /// Advances on one event and returns the commands to run, in order. `main_context` is
    /// the tracked main-frame ferryx-bridge execution context at the time of the event; a
    /// downgrade disarms the broker there first. Events that do not fit the current stage are
    /// ignored, except a late broker registration after a downgrade, which is removed at once.
    pub fn on(&mut self, event: CdpBridgeEvent, main_context: Option<i64>) -> Vec<CdpBridgeCommand> {
        use CdpBridgeCommand as C;
        use CdpBridgeStage as S;
        match (self.stage, event) {
            (S::Downgraded, CdpBridgeEvent::BrokerRegistered(id)) => vec![C::RemoveBroker(id)],
            (S::Downgraded, _) => Vec::new(),
            (S::Probing | S::Isolated, CdpBridgeEvent::ProbeErrored) if !self.probe_retried => {
                self.probe_retried = true;
                vec![C::RunLeakProbe]
            }
            (S::Probing | S::Isolated, CdpBridgeEvent::ProbeErrored) => {
                self.on(CdpBridgeEvent::Failed, main_context)
            }
            (_, CdpBridgeEvent::Failed) => {
                // The new-document broker may already run once Page.addScriptToEvaluateOnNewDocument
                // was sent, and does after BecomeIsolated's catch-up.
                let broker_may_be_live = matches!(self.stage, S::RegisteringBroker | S::Isolated);
                self.stage = S::Downgraded;
                let mut commands = Vec::new();
                if let (true, Some(context)) = (broker_may_be_live, main_context) {
                    commands.push(C::DisarmBroker(context));
                }
                commands.push(C::RemoveBinding);
                if let Some(id) = self.broker_script_id.take() {
                    commands.push(C::RemoveBroker(id));
                }
                commands.push(C::MarkUnavailable);
                commands
            }
            (S::Setup, CdpBridgeEvent::SetupSucceeded) => {
                self.stage = S::Probing;
                self.probe_retried = false;
                vec![C::RunLeakProbe]
            }
            (S::Probing, CdpBridgeEvent::ProbePassed) => {
                self.stage = S::RegisteringBroker;
                vec![C::RegisterBroker]
            }
            (S::RegisteringBroker, CdpBridgeEvent::BrokerRegistered(id)) => {
                self.stage = S::Isolated;
                self.broker_script_id = Some(id);
                vec![C::BecomeIsolated]
            }
            (S::Isolated, CdpBridgeEvent::MainWorldCreated) => {
                self.probe_retried = false;
                vec![C::RunLeakProbe]
            }
            _ => Vec::new(),
        }
    }
}

/// Page.addScriptToEvaluateOnNewDocument result -> script identifier.
pub fn parse_script_identifier(result: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(result).ok()?;
    value["identifier"]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use CdpBridgeCommand as C;
    use CdpBridgeEvent as E;

    #[test]
    fn broker_is_registered_only_after_binding_and_probe() {
        let mut life = CdpBridgeLifecycle::default();
        assert_eq!(life.on(E::ProbePassed, None), vec![]);
        assert_eq!(life.on(E::SetupSucceeded, None), vec![C::RunLeakProbe]);
        assert_eq!(life.on(E::ProbePassed, None), vec![C::RegisterBroker]);
        assert!(!life.is_isolated());
        assert_eq!(life.on(E::BrokerRegistered("7".into()), None), vec![C::BecomeIsolated]);
        assert!(life.is_isolated());
        assert_eq!(life.broker_script_id(), Some("7"));
    }

    #[test]
    fn no_broker_when_the_setup_chain_fails() {
        let mut life = CdpBridgeLifecycle::default();
        assert_eq!(life.on(E::Failed, None), vec![C::RemoveBinding, C::MarkUnavailable]);
        assert!(life.is_downgraded());
        assert_eq!(life.on(E::SetupSucceeded, None), vec![]);
        assert_eq!(life.on(E::ProbePassed, None), vec![]);
    }

    #[test]
    fn no_broker_when_the_leak_probe_fails() {
        let mut life = CdpBridgeLifecycle::default();
        life.on(E::SetupSucceeded, None);
        let commands = life.on(E::Failed, None);
        assert_eq!(commands, vec![C::RemoveBinding, C::MarkUnavailable]);
        assert!(!commands.contains(&C::RegisterBroker));
        assert_eq!(life.on(E::ProbePassed, None), vec![]);
        assert_eq!(life.broker_script_id(), None);
    }

    #[test]
    fn downgrade_after_isolation_removes_the_broker_by_identifier() {
        let mut life = CdpBridgeLifecycle::default();
        life.on(E::SetupSucceeded, None);
        life.on(E::ProbePassed, None);
        life.on(E::BrokerRegistered("42".into()), None);
        assert_eq!(
            life.on(E::Failed, None),
            vec![C::RemoveBinding, C::RemoveBroker("42".into()), C::MarkUnavailable]
        );
        assert!(life.is_downgraded());
        assert_eq!(life.on(E::Failed, None), vec![]);
    }

    #[test]
    fn a_broker_registered_after_a_downgrade_is_removed_at_once() {
        let mut life = CdpBridgeLifecycle::default();
        life.on(E::SetupSucceeded, None);
        life.on(E::ProbePassed, None);
        life.on(E::Failed, None);
        assert_eq!(life.on(E::BrokerRegistered("9".into()), None), vec![C::RemoveBroker("9".into())]);
        assert!(!life.is_isolated());
    }

    #[test]
    fn script_identifier_parses() {
        assert_eq!(parse_script_identifier(r#"{"identifier":"3"}"#).as_deref(), Some("3"));
        assert_eq!(parse_script_identifier(r#"{"identifier":""}"#), None);
        assert_eq!(parse_script_identifier("{}"), None);
        assert_eq!(parse_script_identifier("nope"), None);
    }

    fn isolated(id: &str) -> CdpBridgeLifecycle {
        let mut life = CdpBridgeLifecycle::default();
        life.on(E::SetupSucceeded, None);
        life.on(E::ProbePassed, None);
        life.on(E::BrokerRegistered(id.into()), Some(17));
        assert!(life.is_isolated());
        life
    }

    #[test]
    fn disarm_runs_in_the_tracked_world_before_an_isolated_tab_is_marked_unavailable() {
        let mut life = isolated("42");
        let commands = life.on(E::Failed, Some(17));
        assert_eq!(
            commands,
            vec![
                C::DisarmBroker(17),
                C::RemoveBinding,
                C::RemoveBroker("42".into()),
                C::MarkUnavailable,
            ]
        );
        let disarm = commands.iter().position(|c| *c == C::DisarmBroker(17)).unwrap();
        let unavailable = commands.iter().position(|c| *c == C::MarkUnavailable).unwrap();
        assert!(disarm < unavailable);
        assert!(life.is_downgraded());
    }

    #[test]
    fn disarm_runs_when_a_tab_downgrades_while_the_broker_registration_is_in_flight() {
        let mut life = CdpBridgeLifecycle::default();
        life.on(E::SetupSucceeded, None);
        life.on(E::ProbePassed, None);
        assert_eq!(
            life.on(E::Failed, Some(5)),
            vec![C::DisarmBroker(5), C::RemoveBinding, C::MarkUnavailable]
        );
    }

    #[test]
    fn disarm_is_skipped_before_any_broker_was_registered_or_without_a_tracked_world() {
        let mut probing = CdpBridgeLifecycle::default();
        probing.on(E::SetupSucceeded, None);
        assert_eq!(probing.on(E::Failed, Some(5)), vec![C::RemoveBinding, C::MarkUnavailable]);
        let mut life = isolated("42");
        assert_eq!(
            life.on(E::Failed, None),
            vec![C::RemoveBinding, C::RemoveBroker("42".into()), C::MarkUnavailable]
        );
    }

    #[test]
    fn disarm_probe_error_alone_retries_the_initial_probe_instead_of_downgrading() {
        let mut life = CdpBridgeLifecycle::default();
        life.on(E::SetupSucceeded, None);
        assert_eq!(life.on(E::ProbeErrored, None), vec![C::RunLeakProbe]);
        assert_eq!(life.stage(), CdpBridgeStage::Probing);
        assert_eq!(life.on(E::ProbePassed, None), vec![C::RegisterBroker]);
    }

    #[test]
    fn disarm_reprobe_error_alone_keeps_an_isolated_tab_and_retries_once() {
        let mut life = isolated("42");
        assert_eq!(life.on(E::MainWorldCreated, Some(17)), vec![C::RunLeakProbe]);
        assert_eq!(life.on(E::ProbeErrored, Some(17)), vec![C::RunLeakProbe]);
        assert!(life.is_isolated());
        assert_eq!(life.on(E::ProbePassed, Some(17)), vec![]);
        assert!(life.is_isolated());
        // A later document gets its own retry budget.
        assert_eq!(life.on(E::MainWorldCreated, Some(18)), vec![C::RunLeakProbe]);
        assert_eq!(life.on(E::ProbeErrored, Some(18)), vec![C::RunLeakProbe]);
        assert!(life.is_isolated());
    }

    #[test]
    fn disarm_runs_when_a_repeated_probe_error_finally_downgrades() {
        let mut life = isolated("42");
        life.on(E::MainWorldCreated, Some(18));
        life.on(E::ProbeErrored, Some(18));
        assert_eq!(
            life.on(E::ProbeErrored, Some(18)),
            vec![
                C::DisarmBroker(18),
                C::RemoveBinding,
                C::RemoveBroker("42".into()),
                C::MarkUnavailable,
            ]
        );
        assert!(life.is_downgraded());
    }

    #[test]
    fn disarm_probe_outcome_counts_only_a_typeof_string_as_a_verdict() {
        assert_eq!(leak_probe_outcome(Some("\"undefined\"")), ProbeOutcome::Passed);
        assert_eq!(leak_probe_outcome(Some(" \"undefined\"\n")), ProbeOutcome::Passed);
        assert_eq!(leak_probe_outcome(Some("\"function\"")), ProbeOutcome::Leaked);
        assert_eq!(leak_probe_outcome(Some("\"object\"")), ProbeOutcome::Leaked);
        assert_eq!(leak_probe_outcome(None), ProbeOutcome::Inconclusive);
        assert_eq!(leak_probe_outcome(Some("null")), ProbeOutcome::Inconclusive);
        assert_eq!(leak_probe_outcome(Some("undefined")), ProbeOutcome::Inconclusive);
        assert_eq!(leak_probe_outcome(Some("")), ProbeOutcome::Inconclusive);
    }
}
