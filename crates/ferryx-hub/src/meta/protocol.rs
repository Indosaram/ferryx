//! Native mhc JSONL RPC records.
use serde_json::{json, Value};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub cwd: String,
    pub roots: Vec<String>,
}
pub fn default_effort() -> String {
    crate::hub_env::var("OMON_DEFAULT_EFFORT")
        .or_else(|_| crate::hub_env::var("DEFAULT_EFFORT"))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "high".into())
}
pub fn command(id: &str, kind: &str) -> Value {
    json!({"id":id,"type":kind})
}
pub fn user_input_cancellation(f: &Value) -> Option<String> {
    if f["type"] != "extension_ui_request"
        || !matches!(
            f["method"].as_str(),
            Some("question" | "select" | "input" | "editor" | "custom_unsupported")
        )
    {
        return None;
    }
    Some(json!({"type":"extension_ui_response","id":f.get("id")?,"cancelled":true}).to_string())
}
