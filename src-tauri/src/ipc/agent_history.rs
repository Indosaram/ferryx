use crate::agent_history::history::{Entry, History, HistoryError, Message, Page};
use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use crate::scoped_contracts::CanonicalProvider;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// On-disk store root per provider, one entry for every `CanonicalProvider`, resolved against this
/// process's environment. See [`provider_history_roots_with_env`].
pub fn provider_history_roots(home: &Path) -> Vec<(CanonicalProvider, PathBuf)> {
    provider_history_roots_with_env(home, |key| std::env::var_os(key))
}

/// Store root per provider: the evidenced env override when it is set to an absolute path, else
/// the default measured in `.omo-parity/facts`. A provider whose CLI is not installed simply
/// yields a `ROOT_UNAVAILABLE` warning instead of an error. Only overrides with evidence are
/// honoured; the per-platform table is `.omo-parity/facts/store-roots-platforms-2026-10-23.md`.
pub fn provider_history_roots_with_env(
    home: &Path,
    get_env: impl Fn(&str) -> Option<OsString>,
) -> Vec<(CanonicalProvider, PathBuf)> {
    provider_history_roots_for(home, get_env, cfg!(windows))
}

/// One provider's store root from [`provider_history_roots_with_env`], so the remote readers
/// (`remote/reference_chat/history.rs`) resolve the same root the history dialog does.
pub fn history_root_for(
    home: &Path,
    provider: CanonicalProvider,
    get_env: impl Fn(&str) -> Option<OsString>,
) -> PathBuf {
    provider_history_roots_with_env(home, get_env)
        .into_iter()
        .find_map(|(candidate, root)| (candidate == provider).then_some(root))
        .expect("every CanonicalProvider has a history root")
}

fn provider_history_roots_for(
    home: &Path,
    get_env: impl Fn(&str) -> Option<OsString>,
    windows: bool,
) -> Vec<(CanonicalProvider, PathBuf)> {
    // A relative override would resolve against this process's cwd, not the agent's, so only an
    // absolute value counts (droid's own resolver refuses a relative FACTORY_HOME_OVERRIDE too).
    let env_dir = |key: &str| {
        get_env(key)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let share = home.join(".local").join("share");
    // XDG data home, honoured by opencode (its bundled xdg-basedir: `XDG_DATA_HOME||join(home,".local","share")`,
    // strings of the installed opencode 1.18 binary) and by crush (facts/remote-crush.md:110).
    let data_home = env_dir("XDG_DATA_HOME").unwrap_or_else(|| share.clone());
    vec![
        // CLAUDE_CONFIG_DIR replaces ~/.claude: installed claude 2.1.308 binary strings name "the
        // Claude Code config directory ($CLAUDE_CONFIG_DIR or ~/.claude)".
        (
            CanonicalProvider::Claude,
            env_dir("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home.join(".claude")).join("projects"),
        ),
        // CODEX_HOME replaces ~/.codex: installed codex-cli 0.160.0 binary strings
        // "`$CODEX_HOME/config.toml` (normally `~/.codex/config.toml`)" and "CODEX_HOME sessions".
        (
            CanonicalProvider::Codex,
            env_dir("CODEX_HOME").unwrap_or_else(|| home.join(".codex")).join("sessions"),
        ),
        // OMO_CODING_AGENT_DIR is the omo agent dir; same precedent as dag/paths.rs:36 and
        // terminal/resume_cwd.rs:35.
        (
            CanonicalProvider::Omo,
            env_dir("OMO_CODING_AGENT_DIR")
                .unwrap_or_else(|| home.join(".omo").join("agent"))
                .join("sessions"),
        ),
        // SENPI_CODING_AGENT_SESSION_DIR names the session dir itself (explore/history.md:74);
        // SENPI_CODING_AGENT_DIR names the agent dir that holds sessions/ (facts/local-1.md:13).
        (
            CanonicalProvider::Pi,
            env_dir("SENPI_CODING_AGENT_SESSION_DIR").unwrap_or_else(|| {
                env_dir("SENPI_CODING_AGENT_DIR")
                    .unwrap_or_else(|| home.join(".senpi").join("agent"))
                    .join("sessions")
            }),
        ),
        // omp: PI_CODING_AGENT_DIR is the agent dir, sessions = <agentDir>/sessions
        // (facts/remote-omp.md:122). `--profile` is a per-launch flag, not visible here.
        (
            CanonicalProvider::Omp,
            env_dir("PI_CODING_AGENT_DIR")
                .unwrap_or_else(|| home.join(".omp").join("agent"))
                .join("sessions"),
        ),
        // gjc: GJC_CODING_AGENT_DIR is the agent dir (default ~/.gjc/agent, facts/gjc-raw/help-root.txt:154)
        // and sessions = agentSubdir(..., "sessions") under it (facts/remote-gjc.md:25). The XDG
        // profile layout is unmeasured (facts/remote-gjc.md:109) and not guessed.
        (
            CanonicalProvider::Gjc,
            env_dir("GJC_CODING_AGENT_DIR")
                .unwrap_or_else(|| home.join(".gjc").join("agent"))
                .join("sessions"),
        ),
        (CanonicalProvider::Antigravity, home.join(".gemini").join("antigravity-cli")),
        (CanonicalProvider::Opencode, data_home.join("opencode")),
        (CanonicalProvider::Copilot, home.join(".copilot").join("session-state")),
        (CanonicalProvider::Cursor, home.join(".cursor").join("chats")),
        (CanonicalProvider::Kimi, home.join(".kimi").join("sessions")),
        (CanonicalProvider::Cline, home.join(".cline").join("data").join("sessions")),
        // GROK_HOME replaces ~/.grok; sessions = $GROK_HOME/sessions (facts/remote-grok.md:154).
        (
            CanonicalProvider::Grok,
            env_dir("GROK_HOME").unwrap_or_else(|| home.join(".grok")).join("sessions"),
        ),
        // FACTORY_HOME_OVERRIDE replaces the home directory, and .factory/sessions is still
        // appended under it (explore/resume.md:130, the cited entire.io commit).
        (
            CanonicalProvider::Droid,
            env_dir("FACTORY_HOME_OVERRIDE")
                .unwrap_or_else(|| home.to_path_buf())
                .join(".factory")
                .join("sessions"),
        ),
        // crush global data: $CRUSH_GLOBAL_DATA, else $XDG_DATA_HOME/crush, else Windows
        // %LOCALAPPDATA%\crush, else ~/.local/share/crush (facts/remote-crush.md:110).
        (
            CanonicalProvider::Crush,
            env_dir("CRUSH_GLOBAL_DATA").unwrap_or_else(|| {
                if let Some(xdg) = env_dir("XDG_DATA_HOME") {
                    xdg.join("crush")
                } else if let Some(local) = env_dir("LOCALAPPDATA").filter(|_| windows) {
                    local.join("crush")
                } else {
                    share.join("crush")
                }
            }),
        ),
    ]
}

pub fn map_history_error(error: HistoryError) -> IpcError {
    match error {
        HistoryError::Unsupported => {
            IpcError::new(IpcErrorCode::InvalidRequest, "unsupported history provider")
        }
        HistoryError::InvalidRequest => {
            IpcError::new(IpcErrorCode::InvalidRequest, "invalid history request")
        }
        HistoryError::NotFound => {
            IpcError::new(IpcErrorCode::NotFound, "history entry not found")
        }
        HistoryError::SourceChanged => {
            IpcError::new(IpcErrorCode::InternalError, "history source modified during scan")
        }
        HistoryError::OutsideRoot => {
            IpcError::new(IpcErrorCode::InternalError, "history path escapes root directory")
        }
        HistoryError::Io => {
            IpcError::new(IpcErrorCode::IoError, "history i/o error")
        }
        HistoryError::ProviderOwned => {
            IpcError::new(IpcErrorCode::InternalError, "history conversation already owned")
        }
        HistoryError::InvalidIdentity => {
            IpcError::new(IpcErrorCode::InternalError, "invalid history session identity")
        }
    }
}

fn history_store() -> Result<&'static Mutex<History>, IpcError> {
    static STORE: OnceLock<Option<Mutex<History>>> = OnceLock::new();
    let store_opt = STORE.get_or_init(|| {
        crate::ipc::file_link::home_dir().map(|home| {
            Mutex::new(History::new(provider_history_roots(&home)))
        })
    });
    store_opt.as_ref().ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::InternalError,
            "failed to resolve user home directory for history store",
        )
    })
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_agent_history_search(
    provider: String,
    cwd: Option<String>,
    query: String,
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<Page<Entry>, IpcError> {
    if query.len() > 4096 {
        return Err(IpcError::new(
            IpcErrorCode::InvalidRequest,
            "query cannot exceed 4096 bytes",
        ));
    }
    let limit = limit.unwrap_or(20);

    run_blocking(move || {
        let store = history_store()?;
        let mut guard = store.lock().map_err(|e| {
            IpcError::new(IpcErrorCode::InternalError, format!("history store mutex poisoned: {e}"))
        })?;
        guard
            .search(&provider, cwd.as_deref(), &query, cursor.as_deref(), limit)
            .map_err(map_history_error)
    })
    .await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_agent_history_read(
    entry_key: String,
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<Page<Message>, IpcError> {
    if entry_key.trim().is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidRequest,
            "entry_key cannot be empty",
        ));
    }
    let limit = limit.unwrap_or(20);

    run_blocking(move || {
        let store = history_store()?;
        let guard = store.lock().map_err(|e| {
            IpcError::new(IpcErrorCode::InternalError, format!("history store mutex poisoned: {e}"))
        })?;
        guard
            .read(&entry_key, cursor.as_deref(), limit)
            .map_err(map_history_error)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// An absolute override base on every target (`/o` is not absolute on Windows).
    fn abs(rel: &str) -> PathBuf {
        let base = if cfg!(windows) { PathBuf::from(r"C:\o") } else { PathBuf::from("/o") };
        rel.split('/').fold(base, |path, part| path.join(part))
    }

    /// Roots for a fake environment; never reads or writes this process's environment.
    fn roots_with(vars: &[(&str, PathBuf)], windows: bool) -> HashMap<CanonicalProvider, PathBuf> {
        let env: HashMap<String, OsString> =
            vars.iter().map(|(key, value)| (key.to_string(), value.clone().into_os_string())).collect();
        provider_history_roots_for(Path::new("/home/dev"), |key| env.get(key).cloned(), windows)
            .into_iter()
            .collect()
    }

    fn root_for(vars: &[(&str, PathBuf)], provider: CanonicalProvider) -> PathBuf {
        roots_with(vars, false).remove(&provider).expect("every provider has a root")
    }

    #[test]
    fn provider_history_roots_maps_every_provider_to_its_measured_home_dir() {
        let home = Path::new("/home/dev");
        let roots = provider_history_roots_with_env(home, |_| None);
        assert_eq!(
            roots,
            vec![
                (CanonicalProvider::Claude, PathBuf::from("/home/dev/.claude/projects")),
                (CanonicalProvider::Codex, PathBuf::from("/home/dev/.codex/sessions")),
                (CanonicalProvider::Omo, PathBuf::from("/home/dev/.omo/agent/sessions")),
                (CanonicalProvider::Pi, PathBuf::from("/home/dev/.senpi/agent/sessions")),
                (CanonicalProvider::Omp, PathBuf::from("/home/dev/.omp/agent/sessions")),
                (CanonicalProvider::Gjc, PathBuf::from("/home/dev/.gjc/agent/sessions")),
                (CanonicalProvider::Antigravity, PathBuf::from("/home/dev/.gemini/antigravity-cli")),
                (CanonicalProvider::Opencode, PathBuf::from("/home/dev/.local/share/opencode")),
                (CanonicalProvider::Copilot, PathBuf::from("/home/dev/.copilot/session-state")),
                (CanonicalProvider::Cursor, PathBuf::from("/home/dev/.cursor/chats")),
                (CanonicalProvider::Kimi, PathBuf::from("/home/dev/.kimi/sessions")),
                (CanonicalProvider::Cline, PathBuf::from("/home/dev/.cline/data/sessions")),
                (CanonicalProvider::Grok, PathBuf::from("/home/dev/.grok/sessions")),
                (CanonicalProvider::Droid, PathBuf::from("/home/dev/.factory/sessions")),
                (CanonicalProvider::Crush, PathBuf::from("/home/dev/.local/share/crush")),
            ]
        );
    }

    #[test]
    fn every_canonical_provider_has_exactly_one_history_root() {
        let roots = provider_history_roots_with_env(Path::new("/home/dev"), |_| None);
        for provider in crate::agent_history::history::PROVIDERS {
            assert_eq!(
                roots.iter().filter(|(candidate, _)| *candidate == provider).count(),
                1,
                "{provider:?} must have exactly one configured history root",
            );
        }
    }

    #[test]
    fn history_root_honours_claude_config_dir() {
        let dir = abs("claude");
        assert_eq!(root_for(&[("CLAUDE_CONFIG_DIR", dir.clone())], CanonicalProvider::Claude), dir.join("projects"));
    }

    #[test]
    fn history_root_honours_codex_home() {
        let dir = abs("codex");
        assert_eq!(root_for(&[("CODEX_HOME", dir.clone())], CanonicalProvider::Codex), dir.join("sessions"));
    }

    #[test]
    fn history_root_honours_omo_coding_agent_dir() {
        let dir = abs("omo-agent");
        assert_eq!(root_for(&[("OMO_CODING_AGENT_DIR", dir.clone())], CanonicalProvider::Omo), dir.join("sessions"));
    }

    #[test]
    fn history_root_honours_senpi_coding_agent_dir_for_pi() {
        let dir = abs("senpi-agent");
        assert_eq!(root_for(&[("SENPI_CODING_AGENT_DIR", dir.clone())], CanonicalProvider::Pi), dir.join("sessions"));
    }

    #[test]
    fn history_root_prefers_senpi_session_dir_over_agent_dir_for_pi() {
        let sessions = abs("pi-sessions");
        let vars = [("SENPI_CODING_AGENT_DIR", abs("senpi-agent")), ("SENPI_CODING_AGENT_SESSION_DIR", sessions.clone())];
        assert_eq!(root_for(&vars, CanonicalProvider::Pi), sessions);
    }

    #[test]
    fn history_root_honours_pi_coding_agent_dir_for_omp() {
        let dir = abs("omp-agent");
        assert_eq!(root_for(&[("PI_CODING_AGENT_DIR", dir.clone())], CanonicalProvider::Omp), dir.join("sessions"));
    }

    #[test]
    fn history_root_honours_gjc_coding_agent_dir() {
        let dir = abs("gjc-agent");
        assert_eq!(root_for(&[("GJC_CODING_AGENT_DIR", dir.clone())], CanonicalProvider::Gjc), dir.join("sessions"));
    }

    #[test]
    fn history_root_honours_grok_home() {
        let dir = abs("grok");
        assert_eq!(root_for(&[("GROK_HOME", dir.clone())], CanonicalProvider::Grok), dir.join("sessions"));
    }

    #[test]
    fn history_root_honours_factory_home_override_as_a_home() {
        let dir = abs("factory-home");
        assert_eq!(
            root_for(&[("FACTORY_HOME_OVERRIDE", dir.clone())], CanonicalProvider::Droid),
            dir.join(".factory").join("sessions"),
        );
    }

    #[test]
    fn history_root_honours_xdg_data_home_for_opencode_and_crush() {
        let dir = abs("xdg-data");
        let roots = roots_with(&[("XDG_DATA_HOME", dir.clone())], false);
        assert_eq!(roots[&CanonicalProvider::Opencode], dir.join("opencode"));
        assert_eq!(roots[&CanonicalProvider::Crush], dir.join("crush"));
    }

    #[test]
    fn history_root_prefers_crush_global_data_over_xdg_data_home() {
        let global = abs("crush-global");
        let vars = [("XDG_DATA_HOME", abs("xdg-data")), ("CRUSH_GLOBAL_DATA", global.clone())];
        assert_eq!(root_for(&vars, CanonicalProvider::Crush), global);
    }

    #[test]
    fn history_root_uses_localappdata_for_crush_on_windows_only() {
        let local = abs("localappdata");
        let vars = [("LOCALAPPDATA", local.clone())];
        assert_eq!(roots_with(&vars, true)[&CanonicalProvider::Crush], local.join("crush"));
        assert_eq!(
            roots_with(&vars, false)[&CanonicalProvider::Crush],
            PathBuf::from("/home/dev/.local/share/crush"),
        );
        // XDG_DATA_HOME still wins over LOCALAPPDATA (facts/remote-crush.md:110 order).
        let xdg = abs("xdg-data");
        let both = [("LOCALAPPDATA", local), ("XDG_DATA_HOME", xdg.clone())];
        assert_eq!(roots_with(&both, true)[&CanonicalProvider::Crush], xdg.join("crush"));
    }

    #[test]
    fn history_root_ignores_empty_and_relative_overrides() {
        let vars = [("GROK_HOME", PathBuf::from("relative/grok")), ("CODEX_HOME", PathBuf::new())];
        let roots = roots_with(&vars, false);
        assert_eq!(roots[&CanonicalProvider::Grok], PathBuf::from("/home/dev/.grok/sessions"));
        assert_eq!(roots[&CanonicalProvider::Codex], PathBuf::from("/home/dev/.codex/sessions"));
    }

    #[test]
    fn history_errors_map_to_ipc_error_codes() {
        assert_eq!(
            map_history_error(HistoryError::Unsupported).code,
            IpcErrorCode::InvalidRequest
        );
        assert_eq!(
            map_history_error(HistoryError::InvalidRequest).code,
            IpcErrorCode::InvalidRequest
        );
        assert_eq!(
            map_history_error(HistoryError::NotFound).code,
            IpcErrorCode::NotFound
        );
        assert_ne!(
            map_history_error(HistoryError::Io).code,
            IpcErrorCode::NotFound
        );
    }

    #[test]
    fn history_page_serializes_with_camel_case_wire_keys() {
        let message = Message {
            ordinal: 1,
            role: "user".into(),
            text: "hello world".into(),
            id: Some("msg-1".into()),
            parent_id: None,
        };
        let page = Page {
            items: vec![message],
            next_cursor: Some("c1".into()),
            partial: false,
            warnings: vec![],
        };

        let val = serde_json::to_value(&page).expect("Page serializes to Value");
        let obj = val.as_object().expect("Page serializes to JSON object");

        assert!(obj.contains_key("items"), "must contain camelCase 'items'");
        assert!(obj.contains_key("nextCursor"), "must contain camelCase 'nextCursor'");
        assert!(obj.contains_key("partial"), "must contain camelCase 'partial'");
        assert!(obj.contains_key("warnings"), "must contain camelCase 'warnings'");
        assert!(!obj.contains_key("next_cursor"), "must NOT contain snake_case 'next_cursor'");
    }
}
