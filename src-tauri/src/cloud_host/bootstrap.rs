//! Pinned bootstrap for a Ferryx cloud host.
//!
//! The script this module embeds turns a bare Linux x86_64 image into a reachable
//! host RPC daemon without `sudo`: it installs a pinned Node when the image has
//! none, installs the pinned `omo-ai` release from an integrity-checked tarball
//! under a lock behind an `.install-complete` marker, and starts
//! `omo host ensure --socket "$HOME/.ferryx-cloud/rpc.sock"`. Everything it needs
//! arrives as environment variables, so no argument ever carries a value that has
//! to be quoted.
//!
//! The engine is omo because only an omo host daemon finalizes a turn: the older
//! mhc release accepts the whole RPC protocol and then never applies the model
//! reply. The mhc download path is still in the script, selected by
//! `FX_ENGINE=mhc`, and [`Engine::Mhc`] is the only way to reach it.
//!
//! `FX_ROLE=ssh` stops there: the desktop reaches the socket through the SSH
//! forward. `FX_ROLE=relay` additionally installs and supervises the ferryx relay
//! client, the only process that has to outlive the bootstrap. The host daemon
//! still exits after 15 minutes idle by design
//! (`DEFAULT_HOST_IDLE_EXIT_MS = 900000`), and `ferryx-cli cloud` re-runs
//! `omo host ensure` before every dispatch.

use std::collections::BTreeMap;

/// The embedded bootstrap script. It is the single source of truth: the pins
/// below are handed to it through [`env_for`], never substituted into its text.
pub const SCRIPT: &str = include_str!("../../resources/cloud-host/bootstrap.sh");

/// Pinned `omo-ai` npm release.
pub const OMO_AI_VERSION: &str = "5.1.79";
/// npm integrity (`sha512-<base64>`) of the `omo-ai` tarball for that release.
pub const OMO_AI_INTEGRITY: &str =
    "sha512-a1BBnDJClmBxZFA2KQAOSlPBjnXxOlqCw9R99WvlmuRCwmtCgNsHSYe8BEfLFFYaSBbAOCGymGtT5N2orfVT6Q==";
/// Pinned Node runtime, installed only when the image has none new enough.
pub const NODE_VERSION: &str = "v24.22.3";
/// sha256 of `node-<NODE_VERSION>-linux-x64.tar.gz`.
pub const NODE_SHA256: &str = "061ec3e3062115705b56b51fa9cc330ab50d34b6969387d4e4b1a50c6809acc2";

/// Pinned mhc release tag; used only by [`Engine::Mhc`].
pub const MHC_VERSION: &str = "v2026.11.16";
/// Pinned sha256 of `mhc-linux-x86_64.tar.gz` for that release; [`Engine::Mhc`] only.
pub const MHC_SHA256: &str = "8a377337109b33bb00d2ec54ce57a499a53a4839c8cd353a28b2c73ec36dc77d";
/// minisign public key that signed that asset; [`Engine::Mhc`] only.
pub const MHC_MINISIGN_PUB: &str = "RWR4GANxRiLz3QbBlgDTEW7J++qZ1fVmYPhYEN7cAnSsW4IJJ/smaLju";
/// The legacy mhc release asset (`uname -m` = x86_64).
pub const MHC_ASSET: &str = "mhc-linux-x86_64.tar.gz";

/// Pinned ferryx-cli download for the relay role.
///
/// Built from `feat/cloud-host` with `--no-default-features` targeting
/// `x86_64-unknown-linux-gnu.2.31` (glibc floor `GLIBC_2.30`), carrying the
/// `/api/v1/cloud/socket` relay stream route and `account enroll --code-file`.
pub const FERRYX_CLI_URL: &str = "https://github.com/Indosaram/ferryx/releases/download/cloud-cli-20261123/ferryx-cli-linux-amd64-headless";
/// Pinned sha256 of [`FERRYX_CLI_URL`].
pub const FERRYX_CLI_SHA256: &str =
    "54f093619d991789e1b0ce84354611900ed4ecfcdfb3003f4730d9168558fe66";

/// Exit code: host is not Linux x86_64.
pub const EXIT_UNSUPPORTED_PLATFORM: i32 = 10;
/// Exit code: a required tool (`curl`/`wget`, `tar`, a sha256 helper) is missing.
pub const EXIT_MISSING_TOOL: i32 = 11;
/// Exit code: sha256 or npm integrity verification failed; nothing was installed.
pub const EXIT_VERIFY_FAILED: i32 = 12;
/// Exit code: the pinned engine could not be installed, or its launcher does not
/// run and report the pinned version.
pub const EXIT_ENGINE_INSTALL_FAILED: i32 = 13;

/// Environment variable selecting the engine, from [`Engine::as_str`].
pub const ENV_ENGINE: &str = "FX_ENGINE";
/// Environment variable carrying [`OMO_AI_VERSION`].
pub const ENV_OMO_AI_VERSION: &str = "FX_OMO_AI_VERSION";
/// Environment variable carrying [`OMO_AI_INTEGRITY`].
pub const ENV_OMO_AI_INTEGRITY: &str = "FX_OMO_AI_INTEGRITY";
/// Environment variable carrying [`NODE_VERSION`].
pub const ENV_NODE_VERSION: &str = "FX_NODE_VERSION";
/// Environment variable carrying [`NODE_SHA256`].
pub const ENV_NODE_SHA256: &str = "FX_NODE_SHA256";
/// Environment variable carrying [`MHC_VERSION`]; [`Engine::Mhc`] only.
pub const ENV_MHC_VERSION: &str = "FX_MHC_VERSION";
/// Environment variable carrying [`MHC_SHA256`]; [`Engine::Mhc`] only.
pub const ENV_MHC_SHA256: &str = "FX_MHC_SHA256";
/// Environment variable carrying [`MHC_MINISIGN_PUB`]; [`Engine::Mhc`] only.
pub const ENV_MHC_MINISIGN_PUB: &str = "FX_MHC_MINISIGN_PUB";
/// Environment variable carrying the role from [`Role`].
pub const ENV_ROLE: &str = "FX_ROLE";
/// Environment variable carrying [`FERRYX_CLI_URL`]; relay role only.
pub const ENV_FERRYX_CLI_URL: &str = "FX_FERRYX_CLI_URL";
/// Environment variable carrying [`FERRYX_CLI_SHA256`]; relay role only.
pub const ENV_FERRYX_CLI_SHA256: &str = "FX_FERRYX_CLI_SHA256";

/// How the desktop will reach the RPC socket on this host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    /// The provider exposes SSH; the desktop forwards the unix socket over it.
    Ssh,
    /// No SSH; the box dials out through the ferryx relay with `ferryx-cli --daemon`.
    Relay,
}

impl Role {
    /// The value of `FX_ROLE` for this role.
    pub const fn as_str(self) -> &'static str {
        match self {
            Role::Ssh => "ssh",
            Role::Relay => "relay",
        }
    }
}

/// The agent engine the box runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Engine {
    /// `omo-ai`: the engine that finalizes turns over the host RPC socket.
    Omo,
    /// The legacy mhc release, kept only so an old host can still be rebuilt.
    Mhc,
}

impl Engine {
    /// The value of `FX_ENGINE` for this engine.
    pub const fn as_str(self) -> &'static str {
        match self {
            Engine::Omo => "omo",
            Engine::Mhc => "mhc",
        }
    }
}

/// The versions a bootstrap run is allowed to install.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pins {
    /// Engine to install.
    pub engine: Engine,
    /// `omo-ai` npm version, e.g. `5.1.79`; [`Engine::Omo`] only.
    pub omo_ai_version: &'static str,
    /// npm integrity of that `omo-ai` tarball; [`Engine::Omo`] only.
    pub omo_ai_integrity: &'static str,
    /// Node runtime installed when the image has none new enough; [`Engine::Omo`] only.
    pub node_version: &'static str,
    /// sha256 of that Node tarball; [`Engine::Omo`] only.
    pub node_sha256: &'static str,
    /// mhc release tag, e.g. `v2026.11.16`; [`Engine::Mhc`] only.
    pub mhc_version: &'static str,
    /// sha256 of the x86_64 Linux mhc tarball; [`Engine::Mhc`] only.
    pub mhc_sha256: &'static str,
    /// minisign public key that signed that tarball; [`Engine::Mhc`] only.
    pub mhc_minisign_pub: &'static str,
    /// ferryx-cli download for the relay role; empty until pinned.
    pub ferryx_cli_url: &'static str,
    /// sha256 of `ferryx_cli_url`; empty until pinned.
    pub ferryx_cli_sha256: &'static str,
}

impl Pins {
    /// The pins this build ships with.
    pub const PINNED: Pins = Pins {
        engine: Engine::Omo,
        omo_ai_version: OMO_AI_VERSION,
        omo_ai_integrity: OMO_AI_INTEGRITY,
        node_version: NODE_VERSION,
        node_sha256: NODE_SHA256,
        mhc_version: MHC_VERSION,
        mhc_sha256: MHC_SHA256,
        mhc_minisign_pub: MHC_MINISIGN_PUB,
        ferryx_cli_url: FERRYX_CLI_URL,
        ferryx_cli_sha256: FERRYX_CLI_SHA256,
    };
}

/// The script to run on the box.
pub fn script() -> &'static str {
    SCRIPT
}

/// The exact environment for one bootstrap run.
///
/// Only these variables are set: the script reads nothing else, and the caller
/// adds no secrets here (agent credentials travel separately as 0600 files).
/// Each engine gets only its own pins, so a run can never mix the two.
/// `FX_FERRYX_CLI_URL`/`FX_FERRYX_CLI_SHA256` are omitted while they are still
/// unpinned, which makes the relay role fail with a named error rather than run
/// an unverified download.
pub fn env_for(role: Role, pins: Pins) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert(ENV_ENGINE.to_string(), pins.engine.as_str().to_string());
    env.insert(ENV_ROLE.to_string(), role.as_str().to_string());
    match pins.engine {
        Engine::Omo => {
            env.insert(
                ENV_OMO_AI_VERSION.to_string(),
                pins.omo_ai_version.to_string(),
            );
            env.insert(
                ENV_OMO_AI_INTEGRITY.to_string(),
                pins.omo_ai_integrity.to_string(),
            );
            env.insert(ENV_NODE_VERSION.to_string(), pins.node_version.to_string());
            env.insert(ENV_NODE_SHA256.to_string(), pins.node_sha256.to_string());
        }
        Engine::Mhc => {
            env.insert(ENV_MHC_VERSION.to_string(), pins.mhc_version.to_string());
            env.insert(ENV_MHC_SHA256.to_string(), pins.mhc_sha256.to_string());
            env.insert(
                ENV_MHC_MINISIGN_PUB.to_string(),
                pins.mhc_minisign_pub.to_string(),
            );
        }
    }
    if role == Role::Relay {
        if !pins.ferryx_cli_url.is_empty() {
            env.insert(
                ENV_FERRYX_CLI_URL.to_string(),
                pins.ferryx_cli_url.to_string(),
            );
        }
        if !pins.ferryx_cli_sha256.is_empty() {
            env.insert(
                ENV_FERRYX_CLI_SHA256.to_string(),
                pins.ferryx_cli_sha256.to_string(),
            );
        }
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_role_env_carries_the_omo_pins_and_nothing_else() {
        let env = env_for(Role::Ssh, Pins::PINNED);
        let keys: Vec<&str> = env.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                ENV_ENGINE,
                ENV_NODE_SHA256,
                ENV_NODE_VERSION,
                ENV_OMO_AI_INTEGRITY,
                ENV_OMO_AI_VERSION,
                ENV_ROLE
            ]
        );
        assert_eq!(env[ENV_ENGINE], "omo");
        assert_eq!(env[ENV_OMO_AI_VERSION], OMO_AI_VERSION);
        assert_eq!(env[ENV_OMO_AI_INTEGRITY], OMO_AI_INTEGRITY);
        assert_eq!(env[ENV_NODE_VERSION], NODE_VERSION);
        assert_eq!(env[ENV_NODE_SHA256], NODE_SHA256);
        assert_eq!(env[ENV_ROLE], "ssh");
        // The legacy engine's pins must never ride along with an omo run.
        assert!(!env.contains_key(ENV_MHC_VERSION));
        assert!(!env.contains_key(ENV_MHC_SHA256));
        assert!(!env.contains_key(ENV_MHC_MINISIGN_PUB));
    }

    #[test]
    fn mhc_engine_still_gets_its_own_three_pins() {
        let env = env_for(
            Role::Ssh,
            Pins {
                engine: Engine::Mhc,
                ..Pins::PINNED
            },
        );
        assert_eq!(env[ENV_ENGINE], "mhc");
        assert_eq!(env[ENV_MHC_VERSION], MHC_VERSION);
        assert_eq!(env[ENV_MHC_SHA256], MHC_SHA256);
        assert_eq!(env[ENV_MHC_MINISIGN_PUB], MHC_MINISIGN_PUB);
        assert!(!env.contains_key(ENV_OMO_AI_VERSION));
        assert!(!env.contains_key(ENV_OMO_AI_INTEGRITY));
        assert!(!env.contains_key(ENV_NODE_VERSION));
    }

    #[test]
    fn relay_role_adds_the_cli_pins_only_once_they_are_pinned() {
        let unpinned = env_for(
            Role::Relay,
            Pins {
                ferryx_cli_url: "",
                ferryx_cli_sha256: "",
                ..Pins::PINNED
            },
        );
        assert_eq!(unpinned[ENV_ROLE], "relay");
        assert!(!unpinned.contains_key(ENV_FERRYX_CLI_URL));
        assert!(!unpinned.contains_key(ENV_FERRYX_CLI_SHA256));

        let shipped = env_for(Role::Relay, Pins::PINNED);
        assert_eq!(shipped[ENV_ROLE], "relay");
        assert_eq!(shipped[ENV_FERRYX_CLI_URL], FERRYX_CLI_URL);
        assert_eq!(shipped[ENV_FERRYX_CLI_SHA256], FERRYX_CLI_SHA256);

        let pinned = env_for(
            Role::Relay,
            Pins {
                ferryx_cli_url: "https://example.invalid/ferryx-cli",
                ferryx_cli_sha256: "aa",
                ..Pins::PINNED
            },
        );
        assert_eq!(
            pinned[ENV_FERRYX_CLI_URL],
            "https://example.invalid/ferryx-cli"
        );
        assert_eq!(pinned[ENV_FERRYX_CLI_SHA256], "aa");
    }

    #[test]
    fn shipped_pins_are_well_formed() {
        assert!(!OMO_AI_VERSION.is_empty());
        assert!(OMO_AI_VERSION.chars().all(|c| c.is_ascii_digit() || c == '.'));
        // npm integrity of a sha512 digest: the prefix plus 88 base64 characters.
        let sri = OMO_AI_INTEGRITY
            .strip_prefix("sha512-")
            .expect("omo-ai integrity must be in npm SRI form");
        assert_eq!(sri.len(), 88);
        assert_eq!(NODE_SHA256.len(), 64);
        assert!(NODE_SHA256.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(NODE_VERSION.starts_with('v'));
        assert_eq!(MHC_SHA256.len(), 64);
        assert!(MHC_SHA256.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(MHC_VERSION.starts_with('v'));
        assert!(MHC_MINISIGN_PUB.starts_with("RW"));
        // Relay CLI is pinned with a valid HTTPS URL and 64-char lowercase hex sha256.
        assert!(FERRYX_CLI_URL.starts_with("https://"));
        assert_eq!(FERRYX_CLI_SHA256.len(), 64);
        assert!(FERRYX_CLI_SHA256
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn exit_codes_stay_stable() {
        // Documented and depended on by callers and by the QA harness.
        assert_eq!(EXIT_UNSUPPORTED_PLATFORM, 10);
        assert_eq!(EXIT_MISSING_TOOL, 11);
        assert_eq!(EXIT_VERIFY_FAILED, 12);
        assert_eq!(EXIT_ENGINE_INSTALL_FAILED, 13);
    }

    #[test]
    fn script_is_the_bootstrap_that_prints_the_ok_line() {
        // Guards against embedding a truncated script or a stale copy: the
        // things every caller depends on must be in the text that ships.
        let script = script();
        assert!(script.contains("FX_BOOTSTRAP_OK socket="));
        assert!(script.contains("--socket"));
        assert!(script.contains("rpc.sock"));
        assert!(script.contains(".install-complete"));
        assert!(script.contains("FX_ROLE"));
        assert!(script.contains("host ensure"));
        assert!(script.contains("FX_ENGINE"));
        assert!(script.contains("npm install"));
    }
}
