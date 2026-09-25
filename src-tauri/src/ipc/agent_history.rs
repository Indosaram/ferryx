use crate::agent_history::history::{Entry, History, HistoryError, Message, Page};
use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use crate::scoped_contracts::CanonicalProvider;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub fn provider_history_roots(home: &Path) -> Vec<(CanonicalProvider, PathBuf)> {
    vec![
        (CanonicalProvider::Claude, home.join(".claude").join("projects")),
        (CanonicalProvider::Codex, home.join(".codex").join("sessions")),
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

    #[test]
    fn provider_history_roots_maps_claude_and_codex_to_home_dirs() {
        let home = Path::new("/home/dev");
        let roots = provider_history_roots(home);
        assert_eq!(
            roots,
            vec![
                (CanonicalProvider::Claude, PathBuf::from("/home/dev/.claude/projects")),
                (CanonicalProvider::Codex, PathBuf::from("/home/dev/.codex/sessions")),
            ]
        );
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
