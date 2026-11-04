use serde::{Deserialize, Serialize};
use std::sync::{mpsc, OnceLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Action {
    Status,
    Reload,
    SetEnabled { enabled: bool },
    Key { keys: String, session: String },
}
pub(super) enum Request {
    Stop,
    Control(Action, tokio::sync::oneshot::Sender<Result<serde_json::Value, String>>),
    Event(String, serde_json::Value),
}
pub type Host = std::sync::Arc<dyn Fn(&str, serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync>;
static WORKER: OnceLock<mpsc::Sender<Request>> = OnceLock::new();
pub(super) fn publish(sender: mpsc::Sender<Request>) -> Result<(), String> {
    WORKER.set(sender).map_err(|_| "Script worker already published".into())
}
pub async fn request(action: Action) -> Result<serde_json::Value, String> {
    let (send, receive) = tokio::sync::oneshot::channel();
    WORKER.get().ok_or("Script worker unavailable")?.send(Request::Control(action, send))
        .map_err(|error| error.to_string())?;
    tokio::time::timeout(std::time::Duration::from_secs(35), receive).await
        .map_err(|error| error.to_string())?.map_err(|error| error.to_string())?
}
pub fn event(name: String, value: serde_json::Value) -> Result<(), String> {
    WORKER.get().ok_or("Script worker unavailable")?.send(Request::Event(name, value))
        .map_err(|error| error.to_string())
}
pub(super) fn set_enabled(enabled: bool) -> Result<(), String> {
    let root = super::store::root()?;
    let path = root.join("settings.json");
    let mut value = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|e| e.to_string())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(error) => return Err(error.to_string()),
    };
    if !value.is_object() { return Err("Settings must be an object".into()); }
    if value.get("scripting").is_some_and(|scripting| !scripting.is_object() && !scripting.is_null()) {
        return Err("Scripting settings must be an object".into());
    }
    value["scripting"]["enabled"] = enabled.into();
    super::store::write(&path, &value)
}
