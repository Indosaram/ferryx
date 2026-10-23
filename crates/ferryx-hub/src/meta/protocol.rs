//! JSON-RPC frames for the omo app-server. The client name stays `omo-gateway` because
//! that is the name the daemon's initialize handshake expects.
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub cwd: String,
    pub roots: Vec<String>,
}

pub fn initialize_request() -> String {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "capabilities": { "experimentalApi": true },
            "clientInfo": {
                "name": "omo-gateway",
                "title": "ferryx-hub",
                "version": env!("CARGO_PKG_VERSION")
            }
        }
    })
    .to_string()
}

pub fn thread_start_request(
    system_prompt: Option<&str>,
    model: Option<&str>,
    workspace: Option<&Workspace>,
    enabled_toolsets: Option<&[String]>,
) -> String {
    let mut params = json!({});
    if let Some(prompt) = system_prompt {
        params["developerInstructions"] = json!(prompt);
    }
    if let Some(model) = model {
        params["model"] = json!(model);
    }
    if let Some(workspace) = workspace {
        params["cwd"] = json!(&workspace.cwd);
        params["runtimeWorkspaceRoots"] = json!(&workspace.roots);
    }
    if let Some(tools) = enabled_toolsets {
        params["enabledToolsets"] = json!(tools);
    }
    json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "thread/start",
        "params": params
    })
    .to_string()
}

pub fn thread_resume_request(thread_id: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "thread/resume",
        "params": { "threadId": thread_id }
    })
    .to_string()
}

/// omo-gateway's `thread/settings/update` (id 4), sent before every turn because
/// `thread/start` misses custom providers and `turn/start` may ignore the model.
pub fn thread_settings_update_request(thread_id: &str, model: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "thread/settings/update",
        "params": { "threadId": thread_id, "model": model, "effort": default_effort() }
    })
    .to_string()
}

fn default_effort() -> String {
    crate::hub_env::var("OMON_DEFAULT_EFFORT")
        .or_else(|_| crate::hub_env::var("DEFAULT_EFFORT"))
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "high".to_owned())
}

pub fn turn_start_request(thread_id: &str, user_prompt: &str, model: Option<&str>) -> String {
    let mut params = json!({
        "threadId": thread_id,
        "input": [{ "type": "text", "text": user_prompt }]
    });
    if let Some(model) = model {
        params["model"] = json!(model);
    }
    params["effort"] = json!(default_effort());
    json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "turn/start",
        "params": params
    })
    .to_string()
}

pub fn turn_steer_request(
    id: u64,
    thread_id: &str,
    expected_turn_id: &str,
    guidance: &str,
) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "turn/steer",
        "params": {
            "threadId": thread_id,
            "expectedTurnId": expected_turn_id,
            "input": [{ "type": "text", "text": guidance }]
        }
    })
    .to_string()
}

pub fn approval_denial_response(req_id: &Value) -> String {
    approval_denial_with_reason(
        req_id,
        "denied by gateway policy - do not retry this tool; ask the user for the information instead",
    )
}

pub fn approval_denial_with_reason(req_id: &Value, reason: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "result": { "allow": false, "decision": "decline", "reason": reason }
    })
    .to_string()
}

pub fn approval_allow_response(req_id: &Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "result": { "allow": true, "decision": "accept" }
    })
    .to_string()
}

pub fn user_input_cancellation(frame: &Value) -> Option<String> {
    if frame.get("method").and_then(Value::as_str) != Some("item/tool/requestUserInput") {
        return None;
    }
    let id = frame.get("id")?;
    Some(
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "answers": {}, "cancelled": true }
        })
        .to_string(),
    )
}

pub fn is_approval_request(method: &str) -> bool {
    method.contains("Approval")
        || method.contains("requestApproval")
        || (method.starts_with("item/") && method.ends_with("/request"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_start_carries_a_workspace_and_omits_it_when_absent() {
        let workspace = Workspace {
            cwd: "/var/ferryx/agents/bot".into(),
            roots: vec!["/var/ferryx/agents/bot".into(), "/var/ferryx/shared".into()],
        };
        let with = thread_start_request(
            Some("instructions"),
            Some("model-x"),
            Some(&workspace),
            None,
        );
        let value: Value = serde_json::from_str(&with).unwrap();
        assert_eq!(value["method"], "thread/start");
        assert_eq!(value["params"]["cwd"], "/var/ferryx/agents/bot");
        assert_eq!(
            value["params"]["runtimeWorkspaceRoots"][1],
            "/var/ferryx/shared"
        );
        let without = thread_start_request(Some("instructions"), Some("model-x"), None, None);
        let value: Value = serde_json::from_str(&without).unwrap();
        assert!(value["params"].get("cwd").is_none());
        assert!(is_approval_request("item/tool/requestApproval"));
        assert!(
            user_input_cancellation(&json!({"method":"item/tool/requestUserInput","id":9}))
                .is_some()
        );
    }
}
