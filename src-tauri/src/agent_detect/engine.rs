use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::manifest::{load_manifest_from_str, CompiledManifest, CompiledRule, RuleState};
use super::region::ScreenInput;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentActivity {
    Working,
    Blocked,
    Idle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    pub state: AgentActivity,
    pub rule_id: String,
    pub manifest_id: String,
}

pub struct DetectionEngine {
    manifests: Vec<CompiledManifest>,
}

struct MatchCandidate<'a> {
    manifest_id: &'a str,
    rule: &'a CompiledRule,
    global_order: usize,
}

impl DetectionEngine {
    pub fn new(manifests: Vec<CompiledManifest>) -> Self {
        Self { manifests }
    }

    pub fn detect(
        &self,
        input: &ScreenInput,
        previous: Option<AgentActivity>,
    ) -> Option<Detection> {
        let mut matches: Vec<MatchCandidate> = Vec::new();
        let mut global_order = 0;

        for manifest in &self.manifests {
            for rule in &manifest.rules {
                if rule.matches(input) {
                    matches.push(MatchCandidate {
                        manifest_id: &manifest.id,
                        rule,
                        global_order,
                    });
                }
                global_order += 1;
            }
        }

        // Sort by priority descending; ties broken by declaration order (ascending global_order).
        matches.sort_by(|a, b| {
            b.rule
                .priority
                .cmp(&a.rule.priority)
                .then_with(|| a.global_order.cmp(&b.global_order))
        });

        if let Some(winner) = matches.first() {
            if winner.rule.skip_state_update || winner.rule.state == RuleState::Unknown {
                return previous.map(|prev| Detection {
                    state: prev,
                    rule_id: winner.rule.id.clone(),
                    manifest_id: winner.manifest_id.to_string(),
                });
            }

            let state = match winner.rule.state {
                RuleState::Working => AgentActivity::Working,
                RuleState::Blocked => AgentActivity::Blocked,
                RuleState::Idle => AgentActivity::Idle,
                RuleState::Unknown => unreachable!(),
            };

            Some(Detection {
                state,
                rule_id: winner.rule.id.clone(),
                manifest_id: winner.manifest_id.to_string(),
            })
        } else {
            // No rule matched: HOLD previous state
            previous.map(|prev| Detection {
                state: prev,
                rule_id: String::new(),
                manifest_id: String::new(),
            })
        }
    }
}

pub static SHIPPED_MANIFESTS: &[&str] = &[
    include_str!("manifests/omo.toml"),
    include_str!("manifests/opencode.toml"),
    include_str!("manifests/codex.toml"),
    include_str!("manifests/claude.toml"),
    // Ported from herdr's embedded corpus 2026-08-28. All manifests are evaluated
    // globally (see agent_detect/mod.rs), so every rule here must identify a STATE
    // specifically enough to be safe outside its own agent's pane.
    include_str!("manifests/antigravity.toml"),
    include_str!("manifests/cline.toml"),
    include_str!("manifests/copilot.toml"),
    include_str!("manifests/cursor.toml"),
    include_str!("manifests/grok.toml"),
    include_str!("manifests/kimi.toml"),
    // Sourced from the GJC (gajae-code) TUI sources 2026-08-31. Same global
    // evaluation contract: every rule carries gjc-exclusive on-screen evidence.
    include_str!("manifests/gjc.toml"),
];

pub fn default_engine() -> &'static DetectionEngine {
    static ENGINE: OnceLock<DetectionEngine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let manifests = SHIPPED_MANIFESTS
            .iter()
            .map(|raw| load_manifest_from_str(raw).expect("shipped manifests must be valid"))
            .collect();
        DetectionEngine::new(manifests)
    })
}
