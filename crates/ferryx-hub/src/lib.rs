//! Ferryx hub. It runs inside `ferryx-cli --daemon` and talks to panes through agent control.
//!
//! Slack, Discord, and voice compile behind `hub-slack`, `hub-discord`, and `hub-voice`.
//! The connectors stay idle until a token or speech key is configured.
#![deny(clippy::print_stdout)]

pub mod agent_api;
pub mod agent_control;
pub mod adoption;
pub mod autonomy;
pub mod browser;
pub mod chat;
pub mod checkpoint;
pub mod checkpoint_import;
pub mod child_env;
#[cfg(feature = "hub-voice")]
pub mod codex_speech;
#[cfg(feature = "hub-voice")]
pub mod realtime_speech;
pub mod config;
pub mod context;
pub mod control;
pub mod cron;
pub mod cron_exec;
pub mod cron_remediate;
pub mod commitments;
pub mod dashboard_api;
pub mod discord_surface;
#[cfg(feature = "hub-discord")]
pub mod discord_voice;
pub mod effect;
pub mod error;
pub mod fleet;
pub mod goals;
pub mod holder;
pub mod http;
pub mod hub;
pub mod hub_env;
pub mod import;
pub mod inbox;
pub mod inbox_cards;
pub mod inbox_prefilter;
pub mod inbox_sources;
pub mod inbox_watch;
pub mod judge;
pub mod lane_pr_sync;
pub mod lane_settle;
pub mod lanes;
pub mod ledger;
pub mod lifecycle;
pub mod locale;
pub mod mail_brief;
pub mod mcp;
pub mod memory;
pub mod meta;
pub mod mirror;
pub mod model_policy;
pub mod outages;
pub mod owner_alert;
pub mod owner_directives;
pub mod readiness;
pub mod report_feedback;
pub mod report_gate;
pub mod report_mutation;
pub mod report_adoption;
pub mod report_rules;
pub mod retention;
pub mod schedule;
pub mod security;
pub mod sessions;
pub mod storage;
pub mod supervisor;
pub mod table_image;
pub mod tools;
pub mod tracker;
pub mod usage;
pub mod voice;
pub mod voice_pcm;
pub mod voice_session;

pub use config::{secret_from_env, HubConfig};
pub use error::{HubError, Result};
pub use hub::Hub;

/// Short-lived directories for tests that bind unix sockets.
///
/// A unix socket path is capped at 104 bytes on macOS (`sun_path`, including the terminating
/// NUL) and at 108 on Linux. A path built under `env::temp_dir()` already spends most of that
/// on a macOS `TMPDIR` (`/var/folders/xx/xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx/T/`), and
/// [`schedule::bind_native_socket`] adds a staging entry inside the socket's parent on top. So
/// socket tests take their directory from here, which is short by construction, and drop it.
#[cfg(test)]
pub(crate) mod test_dirs {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    /// A fresh empty directory that removes itself and everything in it on drop.
    pub(crate) struct ShortDir {
        path: PathBuf,
    }

    impl ShortDir {
        /// A directory tagged `tag`, unique to this process and this call.
        pub(crate) fn new(tag: &str) -> Self {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = base().join(format!("fx-{}-{n}-{tag}", std::process::id()));
            std::fs::create_dir_all(&path).expect("a short test directory is created");
            Self { path }
        }

        /// `name` inside the directory.
        pub(crate) fn join(&self, name: &str) -> PathBuf {
            self.path.join(name)
        }
    }

    impl Drop for ShortDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    /// The shortest directory this platform offers for a socket tree: `/tmp` where it exists,
    /// the system temp directory elsewhere.
    fn base() -> PathBuf {
        #[cfg(unix)]
        {
            let tmp = Path::new("/tmp");
            if tmp.is_dir() {
                return tmp.to_path_buf();
            }
        }
        std::env::temp_dir()
    }
}
