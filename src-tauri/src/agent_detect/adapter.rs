//! What a provider can actually do, as opposed to what its screen looks like.
//!
//! The 16 `agent_detect` manifests describe one thing only: how to read a provider's screen
//! (`manifest.rs` `RawRule` carries a region and a matcher, nothing else). Nothing in the crate
//! could previously answer "may this provider be rolled back?" - a question a checkpoint restore
//! has to answer *before* it touches a byte, because a provider whose conversation cannot be
//! rewound leaves the filesystem and the conversation out of step.
//!
//! So capabilities are a separate, declared fact. A manifest may state them in a `[capabilities]`
//! table; a manifest that says nothing gets [`ProviderCapabilities::default`], which is all
//! `false`. That default is the whole safety argument: a provider that declares nothing can do
//! nothing, so the conservative outcome is also the silent one. It is also why adding the field
//! changed no manifest: every existing file keeps parsing byte-identically, and every provider
//! therefore reports no rollback until someone measures and declares it.
//!
//! Deliberately *not* inferred here: the fact that agent control can type into any pane says
//! nothing about whether that provider can restore a conversation, and this module never derives
//! one capability from the transport. `prompt`, `answer` and `stop` stay declared facts too, so a
//! caller can always tell "the provider cannot do it" from "nobody wrote it down".

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::engine::SHIPPED_MANIFESTS;
use super::manifest::{load_manifest_from_str, CompiledManifest};

/// One thing a provider may or may not be able to do.
///
/// The wire names are the ones a manifest spells and a caller reads back, so they are stable:
/// `prompt`, `answer`, `stop`, `usage`, `rollbackConversation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderCapability {
    /// Hand a turn to the provider.
    Prompt,
    /// Answer the question the provider is blocked on.
    Answer,
    /// Interrupt the running turn.
    Stop,
    /// Read token or cost usage for the turn.
    Usage,
    /// Rewind the provider's own conversation to a recorded point.
    RollbackConversation,
}

impl ProviderCapability {
    /// Every capability, in the order a manifest prints them.
    pub const ALL: [Self; 5] = [
        Self::Prompt,
        Self::Answer,
        Self::Stop,
        Self::Usage,
        Self::RollbackConversation,
    ];

    /// The manifest and wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Answer => "answer",
            Self::Stop => "stop",
            Self::Usage => "usage",
            Self::RollbackConversation => "rollbackConversation",
        }
    }

    /// The capability a manifest key names, or `None` for a key this build does not know.
    pub fn from_wire_name(name: &str) -> Option<Self> {
        let name = name.trim();
        Self::ALL.into_iter().find(|cap| cap.as_str() == name)
    }
}

/// What one provider declares it can do.
///
/// Every field is `false` until a manifest says otherwise, and the table itself is `default` at
/// the struct level: a manifest that declares only `rollbackConversation` leaves the other four
/// `false` rather than failing to parse. Conservative in both directions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderCapabilities {
    pub prompt: bool,
    pub answer: bool,
    pub stop: bool,
    pub usage: bool,
    /// A recorded conversation can be rewound. `false` for every shipped provider today, and
    /// that value is correct rather than a placeholder: the manifests are screen matchers and
    /// none of them owns a conversation to rewind.
    pub rollback_conversation: bool,
}

impl ProviderCapabilities {
    /// The declared capability a caller asked about.
    pub fn supports(self, capability: ProviderCapability) -> bool {
        match capability {
            ProviderCapability::Prompt => self.prompt,
            ProviderCapability::Answer => self.answer,
            ProviderCapability::Stop => self.stop,
            ProviderCapability::Usage => self.usage,
            ProviderCapability::RollbackConversation => self.rollback_conversation,
        }
    }

    /// Everything this provider declares, in [`ProviderCapability::ALL`] order.
    pub fn declared(self) -> Vec<ProviderCapability> {
        ProviderCapability::ALL
            .into_iter()
            .filter(|cap| self.supports(*cap))
            .collect()
    }

    /// True when the provider declared nothing, which is the state of all 16 shipped manifests.
    pub fn declares_nothing(self) -> bool {
        self == Self::default()
    }
}

/// A provider's identity plus what it declared.
///
/// Built from a loaded manifest ([`Self::from_manifest`]) or from a provider id and a capability
/// table a caller already holds. The id is the manifest id, never a display name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAdapter {
    pub id: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub capabilities: ProviderCapabilities,
}

impl ProviderAdapter {
    pub fn new(
        id: impl Into<String>,
        aliases: Vec<String>,
        capabilities: ProviderCapabilities,
    ) -> Self {
        Self {
            id: id.into(),
            aliases,
            capabilities,
        }
    }

    /// The adapter a manifest declares. A manifest with no `[capabilities]` table yields
    /// [`ProviderCapabilities::default`], so this is the one place a raw manifest becomes a
    /// capability answer.
    pub fn from_manifest(manifest: &CompiledManifest) -> Self {
        Self {
            id: manifest.id.clone(),
            aliases: manifest.aliases.clone(),
            capabilities: manifest.capabilities,
        }
    }

    pub fn supports(&self, capability: ProviderCapability) -> bool {
        self.capabilities.supports(capability)
    }

    /// The id or any alias matches, case-insensitively.
    pub fn answers_to(&self, provider: &str) -> bool {
        let provider = provider.trim();
        self.id.eq_ignore_ascii_case(provider)
            || self
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(provider))
    }

    /// `Ok` only when this provider declared `capability`. The refusal names both sides so a
    /// caller can report why without guessing.
    pub fn require(&self, capability: ProviderCapability) -> Result<(), ProviderRefusal> {
        if self.supports(capability) {
            Ok(())
        } else {
            Err(ProviderRefusal::MissingCapability {
                provider: self.id.clone(),
                capability,
            })
        }
    }
}

/// Why a provider is refused. This is data, not a wire error: `agent_control::policy` turns it
/// into the `ScopeError` the rest of the control plane speaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderRefusal {
    /// No shipped manifest carries this id or alias, so no capability can be claimed.
    UnknownProvider(String),
    /// The provider is known and did not declare the capability.
    MissingCapability {
        provider: String,
        capability: ProviderCapability,
    },
}

impl std::fmt::Display for ProviderRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownProvider(provider) => write!(
                f,
                "unknown provider '{provider}': its capabilities cannot be established"
            ),
            Self::MissingCapability {
                provider,
                capability,
            } => write!(
                f,
                "provider '{provider}' does not declare {}",
                capability.as_str()
            ),
        }
    }
}

impl std::error::Error for ProviderRefusal {}

/// Every shipped provider's adapter, in [`SHIPPED_MANIFESTS`] order, built once.
///
/// The manifests are compiled assets that `default_engine` already expects to be valid, so a
/// failure here is a build problem, not a runtime condition to swallow.
pub fn shipped_adapters() -> &'static [ProviderAdapter] {
    static ADAPTERS: OnceLock<Vec<ProviderAdapter>> = OnceLock::new();
    ADAPTERS.get_or_init(|| {
        SHIPPED_MANIFESTS
            .iter()
            .map(|raw| load_manifest_from_str(raw).expect("shipped manifests must be valid"))
            .map(|manifest| ProviderAdapter::from_manifest(&manifest))
            .collect()
    })
}

/// The adapter for a provider id or alias, when a shipped manifest answers to it.
pub fn provider_adapter(provider: &str) -> Option<&'static ProviderAdapter> {
    shipped_adapters()
        .iter()
        .find(|adapter| adapter.answers_to(provider))
}

/// The gate a restore calls before it touches anything: `Ok` only when the provider declared
/// conversation rollback.
///
/// An unknown provider is refused too. "I could not find the manifest" is not evidence that a
/// conversation can be rewound, and defaulting the other way would make every typo a permission.
pub fn refuse_unless_rollback(provider: &ProviderAdapter) -> Result<(), ProviderRefusal> {
    provider.require(ProviderCapability::RollbackConversation)
}

/// [`refuse_unless_rollback`] for a provider named only by id or alias.
pub fn refuse_unless_rollback_for(provider: &str) -> Result<(), ProviderRefusal> {
    match provider_adapter(provider) {
        Some(adapter) => refuse_unless_rollback(adapter),
        None => Err(ProviderRefusal::UnknownProvider(provider.trim().to_string())),
    }
}
