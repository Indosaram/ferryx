use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::manifest::{
    load_manifest_from_str, matcher_needs_lower, CompiledManifest, RuleState,
};
use super::region::{Region, RegionView, ScreenInput};

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

struct IndexedRule {
    manifest_idx: usize,
    rule_idx: usize,
    priority: i32,
    global_order: usize,
    region_idx: usize,
    needs_lower: bool,
}

struct CachedRegion<'a> {
    view: RegionView<'a>,
    text_lower: Option<String>,
}

pub struct DetectionEngine {
    manifests: Vec<CompiledManifest>,
    regions: Vec<Region>,
    /// Flattened rules pre-sorted by `(priority DESC, global_order ASC)` so `detect()`
    /// can return immediately on the first matching rule with identical semantics.
    sorted_rules: Vec<IndexedRule>,
}

impl DetectionEngine {
    pub fn new(manifests: Vec<CompiledManifest>) -> Self {
        let mut region_ids: HashMap<Region, usize> = HashMap::new();
        let mut regions: Vec<Region> = Vec::new();
        let mut sorted_rules: Vec<IndexedRule> = Vec::new();
        let mut global_order = 0usize;

        for (manifest_idx, manifest) in manifests.iter().enumerate() {
            for (rule_idx, rule) in manifest.rules.iter().enumerate() {
                let region_idx = *region_ids.entry(rule.region.clone()).or_insert_with(|| {
                    let idx = regions.len();
                    regions.push(rule.region.clone());
                    idx
                });
                sorted_rules.push(IndexedRule {
                    manifest_idx,
                    rule_idx,
                    priority: rule.priority,
                    global_order,
                    region_idx,
                    needs_lower: matcher_needs_lower(&rule.matcher),
                });
                global_order += 1;
            }
        }

        sorted_rules.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.global_order.cmp(&b.global_order))
        });

        Self {
            manifests,
            regions,
            sorted_rules,
        }
    }

    pub fn detect(
        &self,
        input: &ScreenInput,
        previous: Option<AgentActivity>,
    ) -> Option<Detection> {
        let mut resolved: Vec<Option<CachedRegion<'_>>> = Vec::with_capacity(self.regions.len());
        resolved.resize_with(self.regions.len(), || None);

        for entry in &self.sorted_rules {
            let manifest = &self.manifests[entry.manifest_idx];
            let rule = &manifest.rules[entry.rule_idx];
            let cached = resolved[entry.region_idx].get_or_insert_with(|| CachedRegion {
                view: self.regions[entry.region_idx].resolve(input),
                text_lower: None,
            });
            let matched = if entry.needs_lower {
                let lower = cached
                    .text_lower
                    .get_or_insert_with(|| cached.view.text.to_lowercase());
                rule.matcher.matches(&cached.view, lower)
            } else {
                rule.matcher.matches(&cached.view, "")
            };
            if matched {
                if rule.skip_state_update || rule.state == RuleState::Unknown {
                    return previous.map(|prev| Detection {
                        state: prev,
                        rule_id: rule.id.clone(),
                        manifest_id: manifest.id.clone(),
                    });
                }

                let state = match rule.state {
                    RuleState::Working => AgentActivity::Working,
                    RuleState::Blocked => AgentActivity::Blocked,
                    RuleState::Idle => AgentActivity::Idle,
                    RuleState::Unknown => unreachable!(),
                };

                return Some(Detection {
                    state,
                    rule_id: rule.id.clone(),
                    manifest_id: manifest.id.clone(),
                });
            }
        }

        // No rule matched: HOLD previous state
        previous.map(|prev| Detection {
            state: prev,
            rule_id: String::new(),
            manifest_id: String::new(),
        })
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
    // Measured 2026-10-22 (.omo-parity/facts/screens/<agent>/). Same global
    // evaluation contract; omp and pi share senpi chrome with omo, so their rules
    // anchor on agent-only footers and outrank the omo rules they shadow.
    include_str!("manifests/omp.toml"),
    include_str!("manifests/pi.toml"),
    include_str!("manifests/crush.toml"),
    include_str!("manifests/aider.toml"),
    include_str!("manifests/droid.toml"),
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
