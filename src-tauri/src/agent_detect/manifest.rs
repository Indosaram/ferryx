use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::matcher::{CompiledMatcherNode, MatcherError, MatcherNode};
use super::region::{Region, RegionError, ScreenInput};

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("Invalid region in rule '{rule_id}': {source}")]
    Region {
        rule_id: String,
        #[source]
        source: RegionError,
    },
    #[error("Invalid matcher in rule '{rule_id}': {source}")]
    Matcher {
        rule_id: String,
        #[source]
        source: MatcherError,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleState {
    Working,
    Blocked,
    Idle,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawRule {
    pub id: String,
    pub state: RuleState,
    pub priority: i32,
    pub region: String,
    #[serde(default)]
    pub visible_working: Option<bool>,
    #[serde(default)]
    pub visible_blocker: Option<bool>,
    #[serde(default)]
    pub visible_idle: Option<bool>,
    #[serde(default)]
    pub skip_state_update: Option<bool>,
    #[serde(flatten)]
    pub matcher: MatcherNode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawManifest {
    pub id: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub min_engine_version: Option<u32>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub rules: Vec<RawRule>,
}

#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub id: String,
    pub state: RuleState,
    pub priority: i32,
    pub region: Region,
    pub visible_working: bool,
    pub visible_blocker: bool,
    pub visible_idle: bool,
    pub skip_state_update: bool,
    pub matcher: CompiledMatcherNode,
    pub declaration_index: usize,
}

impl CompiledRule {
    pub fn matches(&self, input: &ScreenInput) -> bool {
        let view = self.region.resolve(input);
        let text_lower = view.text.to_lowercase();
        self.matcher.matches(&view, &text_lower)
    }
}

#[derive(Debug, Clone)]
pub struct CompiledManifest {
    pub id: String,
    pub version: Option<String>,
    pub min_engine_version: Option<u32>,
    pub aliases: Vec<String>,
    pub updated_at: Option<String>,
    pub rules: Vec<CompiledRule>,
}

pub fn load_manifest_from_str(s: &str) -> Result<CompiledManifest, ManifestError> {
    let raw: RawManifest = toml::from_str(s)?;
    let mut compiled_rules = Vec::with_capacity(raw.rules.len());
    for (idx, rule) in raw.rules.into_iter().enumerate() {
        let region = rule
            .region
            .parse::<Region>()
            .map_err(|source| ManifestError::Region {
                rule_id: rule.id.clone(),
                source,
            })?;
        let matcher = rule
            .matcher
            .compile()
            .map_err(|source| ManifestError::Matcher {
                rule_id: rule.id.clone(),
                source,
            })?;
        compiled_rules.push(CompiledRule {
            id: rule.id,
            state: rule.state,
            priority: rule.priority,
            region,
            visible_working: rule.visible_working.unwrap_or(false),
            visible_blocker: rule.visible_blocker.unwrap_or(false),
            visible_idle: rule.visible_idle.unwrap_or(false),
            skip_state_update: rule.skip_state_update.unwrap_or(false),
            matcher,
            declaration_index: idx,
        });
    }
    Ok(CompiledManifest {
        id: raw.id,
        version: raw.version,
        min_engine_version: raw.min_engine_version,
        aliases: raw.aliases,
        updated_at: raw.updated_at,
        rules: compiled_rules,
    })
}
