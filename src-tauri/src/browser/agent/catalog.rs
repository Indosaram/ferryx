//! Canonical browser tool catalog: identifiers, scopes and platform support (C2).

use std::fmt;
use std::str::FromStr;

use serde::{Serialize, Serializer};

use super::protocol::AuthorityError;

/// Authority scope of a tool. Eval is never implied by Interact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    Read,
    Interact,
    Eval,
    Native,
    File,
}

/// Per-origin grant scope held by a local controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GrantScope {
    Read,
    Interact,
    Eval,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Platform {
    MacOs,
    Windows,
    Linux,
    Other,
}

impl Platform {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Other
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MacOs => "macos",
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::Other => "other",
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Platforms on which a tool is actually implemented in this build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PlatformSupport {
    AllPlatforms,
    DesktopOnly { reason: &'static str },
}

/// Typed refusal for a tool that is not implemented on the running platform.
/// Callers must surface this instead of fabricating success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Unsupported {
    pub tool: ToolId,
    pub platform: Platform,
    pub reason: &'static str,
}

macro_rules! tool_ids {
    ($($variant:ident => $name:literal,)+) => {
        /// Canonical tool identifier shared by every ingress and by tools/list.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum ToolId {
            $($variant,)+
        }

        impl ToolId {
            pub const ALL: &'static [ToolId] = &[$(ToolId::$variant,)+];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(ToolId::$variant => $name,)+
                }
            }
        }
    };
}

tool_ids! {
    List => "browser.list",
    Identify => "browser.identify",
    Snapshot => "browser.snapshot",
    GetState => "browser.get_state",
    Console => "browser.console",
    Wait => "browser.wait",
    DialogList => "browser.dialog.list",
    DownloadList => "browser.download.list",
    CookiesRead => "browser.cookies.read",
    StorageRead => "browser.storage.read",
    ViewerSubscribe => "browser.viewer.subscribe",
    ViewerUnsubscribe => "browser.viewer.unsubscribe",
    Open => "browser.open",
    Navigate => "browser.navigate",
    Back => "browser.back",
    Forward => "browser.forward",
    Reload => "browser.reload",
    Close => "browser.close",
    Act => "browser.act",
    Highlight => "browser.highlight",
    DialogRespond => "browser.dialog.respond",
    DialogPolicy => "browser.dialog.policy",
    DownloadCancel => "browser.download.cancel",
    CookiesWrite => "browser.cookies.write",
    StorageWrite => "browser.storage.write",
    DriverClaim => "browser.driver.claim",
    DriverRelease => "browser.driver.release",
    DriverHeartbeat => "browser.driver.heartbeat",
    RemoteAttach => "browser.remote.attach",
    Eval => "browser.eval",
    WaitFunction => "browser.wait_function",
    Focus => "browser.focus",
    TabActivate => "browser.tab.activate",
    Screenshot => "browser.screenshot",
    StateExport => "browser.state.export",
    StateImport => "browser.state.import",
    DownloadStart => "browser.download.start",
    OpenFile => "editor.open_file",
}

impl fmt::Display for ToolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ToolId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl FromStr for ToolId {
    type Err = AuthorityError;

    /// Resolves a capability name; anything outside the catalog is refused.
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|tool| tool.as_str() == name)
            .ok_or_else(|| AuthorityError::UnknownCapability {
                name: name.to_string(),
            })
    }
}

/// Registry entry for one canonical tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSpec {
    pub id: ToolId,
    pub scope: Scope,
    /// Returns or mutates page-private data (content, script results, pixels,
    /// cookies, storage, exported state).
    pub sensitive: bool,
    /// Exact-origin grant a local controller needs on the tab's current document.
    pub origin_grant: Option<GrantScope>,
    /// Requires holding the tab (local) or driver (remote) lease.
    pub requires_lease: bool,
    pub support: PlatformSupport,
}

const SCREENSHOT_UNSUPPORTED_REASON: &str =
    "native webview capture supports macOS, Windows and Linux only";

const fn spec(
    id: ToolId,
    scope: Scope,
    sensitive: bool,
    origin_grant: Option<GrantScope>,
    requires_lease: bool,
) -> ToolSpec {
    ToolSpec {
        id,
        scope,
        sensitive,
        origin_grant,
        requires_lease,
        support: PlatformSupport::AllPlatforms,
    }
}

impl ToolId {
    /// Scope, sensitivity and platform support. Exhaustive: adding a tool
    /// without classifying it fails to compile.
    pub const fn spec(self) -> ToolSpec {
        use GrantScope as G;
        use Scope as S;
        match self {
            Self::List => spec(self, S::Read, false, None, false),
            Self::Identify => spec(self, S::Read, false, None, false),
            Self::Snapshot => spec(self, S::Read, true, Some(G::Read), false),
            Self::GetState => spec(self, S::Read, false, Some(G::Read), false),
            Self::Console => spec(self, S::Read, true, Some(G::Read), false),
            Self::Wait => spec(self, S::Read, false, Some(G::Read), true),
            Self::DialogList => spec(self, S::Read, true, Some(G::Read), false),
            Self::DownloadList => spec(self, S::Read, true, None, false),
            Self::CookiesRead => spec(self, S::Read, true, Some(G::Eval), false),
            Self::StorageRead => spec(self, S::Read, true, Some(G::Eval), false),
            Self::ViewerSubscribe => spec(self, S::Read, true, Some(G::Read), false),
            Self::ViewerUnsubscribe => spec(self, S::Read, false, None, false),
            Self::Open => spec(self, S::Interact, false, None, false),
            Self::Navigate => spec(self, S::Interact, false, Some(G::Interact), true),
            Self::Back => spec(self, S::Interact, false, Some(G::Interact), true),
            Self::Forward => spec(self, S::Interact, false, Some(G::Interact), true),
            Self::Reload => spec(self, S::Interact, false, Some(G::Interact), true),
            Self::Close => spec(self, S::Interact, false, None, true),
            Self::Act => spec(self, S::Interact, false, Some(G::Interact), true),
            Self::Highlight => spec(self, S::Interact, false, Some(G::Interact), true),
            Self::DialogRespond => spec(self, S::Interact, false, Some(G::Interact), true),
            Self::DialogPolicy => spec(self, S::Interact, false, Some(G::Interact), true),
            Self::DownloadCancel => spec(self, S::Interact, false, None, false),
            Self::CookiesWrite => spec(self, S::Interact, true, Some(G::Eval), true),
            Self::StorageWrite => spec(self, S::Interact, true, Some(G::Eval), true),
            Self::DriverClaim => spec(self, S::Interact, false, None, false),
            Self::DriverRelease => spec(self, S::Interact, false, None, false),
            Self::DriverHeartbeat => spec(self, S::Interact, false, None, false),
            Self::RemoteAttach => spec(self, S::Interact, false, None, false),
            Self::Eval => spec(self, S::Eval, true, Some(G::Eval), true),
            Self::WaitFunction => spec(self, S::Eval, true, Some(G::Eval), true),
            Self::Focus => spec(self, S::Native, false, None, false),
            Self::TabActivate => spec(self, S::Native, false, None, false),
            Self::Screenshot => ToolSpec {
                id: self,
                scope: S::File,
                sensitive: true,
                origin_grant: Some(G::Read),
                requires_lease: false,
                support: PlatformSupport::DesktopOnly {
                    reason: SCREENSHOT_UNSUPPORTED_REASON,
                },
            },
            Self::StateExport => spec(self, S::File, true, Some(G::Eval), false),
            Self::StateImport => spec(self, S::File, true, Some(G::Eval), true),
            Self::DownloadStart => spec(self, S::File, true, Some(G::Read), false),
            Self::OpenFile => spec(self, S::File, false, None, false),
        }
    }
}

impl ToolSpec {
    pub fn check_platform(&self, platform: Platform) -> Result<(), Unsupported> {
        match self.support {
            PlatformSupport::AllPlatforms => Ok(()),
            PlatformSupport::DesktopOnly { reason } => {
                if matches!(
                    platform,
                    Platform::MacOs | Platform::Windows | Platform::Linux
                ) {
                    Ok(())
                } else {
                    Err(Unsupported {
                        tool: self.id,
                        platform,
                        reason,
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_ids_round_trip_and_are_unique() {
        for tool in ToolId::ALL {
            assert_eq!(tool.as_str().parse::<ToolId>(), Ok(*tool));
            assert_eq!(tool.spec().id, *tool);
        }
        let mut names: Vec<&str> = ToolId::ALL.iter().map(|tool| tool.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ToolId::ALL.len());
    }

    #[test]
    fn unknown_capability_name_is_refused() {
        for name in ["browser.teleport", "", "BROWSER.EVAL", "browser.eval ", "*"] {
            let error = name
                .parse::<ToolId>()
                .expect_err("unknown capability must be refused");
            assert_eq!(error.code(), super::super::BROWSER_UNKNOWN_CAPABILITY);
        }
    }

    #[test]
    fn sensitive_tools_are_honestly_marked() {
        for tool in [
            ToolId::Eval,
            ToolId::WaitFunction,
            ToolId::CookiesRead,
            ToolId::CookiesWrite,
            ToolId::StorageRead,
            ToolId::StorageWrite,
            ToolId::StateExport,
            ToolId::StateImport,
            ToolId::Snapshot,
            ToolId::Screenshot,
        ] {
            assert!(tool.spec().sensitive, "{tool} must be sensitive");
        }
        for tool in [ToolId::Eval, ToolId::WaitFunction] {
            assert_eq!(tool.spec().scope, Scope::Eval);
        }
        for tool in [ToolId::Act, ToolId::Navigate, ToolId::Highlight] {
            assert_ne!(tool.spec().origin_grant, Some(GrantScope::Eval));
        }
    }

    #[test]
    fn screenshot_platform_support_is_typed_unsupported() {
        let spec = ToolId::Screenshot.spec();
        for platform in [Platform::MacOs, Platform::Windows, Platform::Linux] {
            assert_eq!(spec.check_platform(platform), Ok(()));
        }
        let unsupported = spec
            .check_platform(Platform::Other)
            .expect_err("must be unsupported");
        assert_eq!(unsupported.tool, ToolId::Screenshot);
        assert_eq!(unsupported.platform, Platform::Other);
        assert_eq!(ToolId::Eval.spec().check_platform(Platform::Linux), Ok(()));
    }
}
