//! `ferryx browser mcp`: a stdio MCP server (JSON-RPC 2.0, one message per line).
//!
//! It answers `initialize`, `ping`, `tools/list` and `tools/call`. Tools are the catalog's
//! canonical browser tools (MCP name: the [`ToolId`] with `.` replaced by `_`, for example
//! `browser_act`) that this platform supports, plus two controller tools for asking the
//! desktop user for an origin grant and waiting on the answer.
//!
//! A tool call is translated by the pipe adapter ([`super::pipe::translate`]) into the same
//! request `ferryx browser <cmd>` sends and is admitted by the running app on the one CLI
//! authority path, with the paired controller credential. There is no policy in this module:
//! a revoked controller or a missing grant comes back as a tool error carrying the app's code.

use serde_json::{json, Map, Value};
use tokio::io::{AsyncBufRead, AsyncWrite};

use super::catalog::{Platform, ToolId};
use super::pipe::{self, CallError, FrameError, Transport, MAX_FRAME_BYTES};

/// Protocol revisions this server speaks; the first is offered when the client asks for another.
const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// An advertised tool: its MCP name, the pipe op it runs and how it is described.
struct McpTool {
    name: String,
    op: &'static str,
    description: &'static str,
    schema: Value,
}

fn object_schema(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required })
}

fn tab_schema() -> Value {
    object_schema(json!({ "tab_id": { "type": "string", "description": "Browser tab id from browser_list" } }), &["tab_id"])
}

fn wait_condition_schema() -> Value {
    json!({
        "type": "object",
        "description": "Page state to wait for: {\"text\"}, {\"selector\"}, {\"fragment\"} (URL contains) or {\"state\"} (load state), optionally with \"timeoutMs\"",
    })
}

/// A catalog tool exposed over MCP: its id, the pipe op it runs, a description and its schema.
type CatalogTool = (ToolId, &'static str, &'static str, Value);

/// Catalog tools exposed over MCP, with the pipe op each one runs.
fn catalog_tools() -> Vec<CatalogTool> {
    vec![
        (ToolId::List, "list", "List the browser tabs this controller may see", object_schema(json!({}), &[])),
        (
            ToolId::Open,
            "open",
            "Open a new browser tab at a URL",
            object_schema(
                json!({
                    "url": { "type": "string" },
                    "workspace_id": { "type": "string" },
                    "worktree_path": { "type": "string" },
                }),
                &["url"],
            ),
        ),
        (ToolId::Snapshot, "observe", "Observe a tab: interactive elements with refs for browser_act", tab_schema()),
        (
            ToolId::Act,
            "act",
            "Act on an element ref from the latest observe of the same generation",
            object_schema(
                json!({
                    "tab_id": { "type": "string" },
                    "generation": { "type": "integer", "minimum": 0 },
                    "action": {
                        "type": "object",
                        "properties": {
                            "kind": {
                                "type": "string",
                                "enum": ["click", "dblclick", "hover", "focus", "check", "uncheck", "scroll_into_view", "select", "fill", "type", "keypress", "scroll"],
                            },
                            "ref": { "type": "string" },
                            "value": { "type": "string" },
                            "text": { "type": "string" },
                            "key": { "type": "string" },
                            "x": { "type": "integer" },
                            "y": { "type": "integer" },
                        },
                        "required": ["kind"],
                    },
                    "postcondition": wait_condition_schema(),
                }),
                &["tab_id", "generation", "action"],
            ),
        ),
        (
            ToolId::Navigate,
            "navigate",
            "Navigate a tab to a URL",
            object_schema(json!({ "tab_id": { "type": "string" }, "url": { "type": "string" } }), &["tab_id", "url"]),
        ),
        (ToolId::Back, "back", "Go back in a tab's history", tab_schema()),
        (ToolId::Forward, "forward", "Go forward in a tab's history", tab_schema()),
        (ToolId::Reload, "reload", "Reload a tab", tab_schema()),
        (ToolId::Close, "close", "Close a tab", tab_schema()),
        (
            ToolId::Wait,
            "wait",
            "Wait until a tab shows a page state",
            object_schema(
                json!({ "tab_id": { "type": "string" }, "condition": wait_condition_schema() }),
                &["tab_id", "condition"],
            ),
        ),
        (
            ToolId::Eval,
            "eval",
            "Evaluate a script in a tab (needs an eval grant)",
            object_schema(json!({ "tab_id": { "type": "string" }, "script": { "type": "string" } }), &["tab_id", "script"]),
        ),
    ]
}

/// Every tool this server advertises on the running platform.
fn tools() -> Vec<McpTool> {
    let platform = Platform::current();
    let mut tools: Vec<McpTool> = catalog_tools()
        .into_iter()
        .filter(|(tool, ..)| tool.spec().check_platform(platform).is_ok())
        .map(|(tool, op, description, schema)| McpTool {
            name: tool.as_str().replace('.', "_"),
            op,
            description,
            schema,
        })
        .collect();
    tools.push(McpTool {
        name: "browser_request_grant".into(),
        op: "grant",
        description: "Ask the desktop user for an exact-origin grant; returns a pending approval id",
        schema: object_schema(
            json!({
                "origin": { "type": "string" },
                "scopes": { "type": "array", "items": { "type": "string", "enum": ["read", "interact", "eval"] } },
            }),
            &["origin", "scopes"],
        ),
    });
    tools.push(McpTool {
        name: "browser_await_approval".into(),
        op: "await",
        description: "Wait until the desktop user resolves an approval",
        schema: object_schema(json!({ "approval_id": { "type": "string" } }), &["approval_id"]),
    });
    tools
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = json!({ "code": code, "message": message });
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

fn tool_result(outcome: Result<Value, CallError>) -> Value {
    match outcome {
        Ok(value) => json!({
            "content": [{ "type": "text", "text": value.to_string() }],
            "structuredContent": value,
            "isError": false,
        }),
        Err(error) => json!({
            "content": [{ "type": "text", "text": format!("{}: {}", error.code, error.message) }],
            "structuredContent": error,
            "isError": true,
        }),
    }
}

fn initialize(params: Option<&Value>) -> Value {
    let requested = params.and_then(|params| params.get("protocolVersion")).and_then(Value::as_str);
    let version = requested.filter(|version| PROTOCOL_VERSIONS.contains(version)).unwrap_or(PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "ferryx-browser", "version": env!("CARGO_PKG_VERSION") },
    })
}

fn list_tools() -> Value {
    let tools: Vec<Value> = tools()
        .into_iter()
        .map(|tool| json!({ "name": tool.name, "description": tool.description, "inputSchema": tool.schema }))
        .collect();
    json!({ "tools": tools })
}

async fn call_tool<T: Transport>(transport: &T, id: Value, params: Option<&Value>) -> Value {
    let Some(name) = params.and_then(|params| params.get("name")).and_then(Value::as_str) else {
        return rpc_error(id, INVALID_PARAMS, "tools/call needs a tool name", None);
    };
    let Some(tool) = tools().into_iter().find(|tool| tool.name == name) else {
        return rpc_error(id, INVALID_PARAMS, "unknown tool", Some(json!({ "name": name })));
    };
    let arguments = match params.and_then(|params| params.get("arguments")) {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(arguments)) => arguments.clone(),
        Some(_) => return rpc_error(id, INVALID_PARAMS, "tool arguments must be an object", None),
    };
    let outcome = match pipe::translate(tool.op, &arguments) {
        Ok(call) => pipe::execute(transport, call).await,
        Err(error) => Err(error),
    };
    result(id, tool_result(outcome))
}

/// Answers one JSON-RPC message; notifications get no response.
pub(crate) async fn handle<T: Transport>(transport: &T, frame: Result<Value, FrameError>) -> Option<Value> {
    let message = match frame {
        Ok(message) => message,
        Err(error) => {
            let code = match error {
                FrameError::Malformed(_) => PARSE_ERROR,
                FrameError::TooLarge { .. } => INVALID_REQUEST,
            };
            let message = match error {
                FrameError::Malformed(_) => "Parse error",
                FrameError::TooLarge { .. } => "Invalid Request: message too large",
            };
            return Some(rpc_error(Value::Null, code, message, Some(json!({ "code": error.code() }))));
        }
    };
    let Value::Object(message) = message else {
        return Some(rpc_error(Value::Null, INVALID_REQUEST, "Invalid Request", None));
    };
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str);
    let (Some(method), true) = (method, message.get("jsonrpc").and_then(Value::as_str) == Some("2.0")) else {
        return Some(rpc_error(id.unwrap_or(Value::Null), INVALID_REQUEST, "Invalid Request", None));
    };
    // A message without an id is a notification (`notifications/initialized` and others).
    let id = id?;
    let params = message.get("params");
    Some(match method {
        "initialize" => result(id, initialize(params)),
        "ping" => result(id, json!({})),
        "tools/list" => result(id, list_tools()),
        "tools/call" => call_tool(transport, id, params).await,
        _ => rpc_error(id, METHOD_NOT_FOUND, "Method not found", None),
    })
}

/// Runs the server over any reader/writer pair until EOF.
pub(crate) async fn run<T, R, W>(transport: &T, reader: R, writer: W, max: usize) -> std::io::Result<()>
where
    T: Transport,
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    pipe::serve(reader, writer, max, move |frame| async move { handle(transport, frame).await }).await
}

/// `ferryx browser mcp`: serves stdin/stdout as the selected controller.
pub async fn run_stdio(controller: Option<String>) -> Result<(), String> {
    let transport = pipe::SocketTransport::new(controller);
    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    run(&transport, stdin, tokio::io::stdout(), MAX_FRAME_BYTES)
        .await
        .map_err(|error| format!("browser MCP server I/O failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::agent::pipe::tests::FakeTransport;

    async fn exchange(transport: &impl Transport, message: Value) -> Value {
        handle(transport, Ok(message)).await.expect("a request gets a response")
    }

    fn call(id: u64, name: &str, arguments: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": name, "arguments": arguments } })
    }

    #[tokio::test]
    async fn initialize_negotiates_a_supported_version_and_names_the_server() {
        let fake = FakeTransport::default();
        let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26", "capabilities": {} } });
        let response = exchange(&fake, request).await;
        assert_eq!(response["id"], 1);
        assert_eq!(response["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(response["result"]["serverInfo"]["name"], "ferryx-browser");
        assert!(response["result"]["capabilities"]["tools"].is_object());
        let unknown = json!({ "jsonrpc": "2.0", "id": 2, "method": "initialize", "params": { "protocolVersion": "1999-01-01" } });
        assert_eq!(exchange(&fake, unknown).await["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
    }

    #[tokio::test]
    async fn notifications_get_no_response() {
        let fake = FakeTransport::default();
        let initialized = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(handle(&fake, Ok(initialized)).await.is_none());
    }

    #[tokio::test]
    async fn tools_list_advertises_catalog_tools_with_object_schemas() {
        let fake = FakeTransport::default();
        let response = exchange(&fake, json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })).await;
        let tools = response["result"]["tools"].as_array().expect("tools array");
        let names: Vec<&str> = tools.iter().filter_map(|tool| tool["name"].as_str()).collect();
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "tool names are unique: {names:?}");
        assert!(names.iter().all(|name| name.chars().all(|c| c.is_ascii_lowercase() || c == '_')));
        for expected in ["browser_list", "browser_snapshot", "browser_act", "browser_navigate", "browser_request_grant"] {
            assert!(names.contains(&expected), "{expected} is advertised: {names:?}");
        }
        assert!(tools.iter().all(|tool| tool["inputSchema"]["type"] == "object"));
    }

    #[tokio::test]
    async fn tool_calls_run_the_cli_request_and_report_refusals_as_tool_errors() {
        let fake = FakeTransport::default();
        let listed = exchange(&fake, call(1, "browser_list", json!({}))).await;
        assert_eq!(listed["result"]["isError"], false);
        assert_eq!(listed["result"]["structuredContent"]["type"], "list");
        let refused = exchange(&fake, call(2, "browser_snapshot", json!({ "tab_id": "t1" }))).await;
        assert_eq!(refused["result"]["isError"], true);
        assert_eq!(refused["result"]["structuredContent"]["code"], "BROWSER_FORBIDDEN");
        let invalid = exchange(&fake, call(3, "browser_snapshot", json!({}))).await;
        assert_eq!(invalid["result"]["isError"], true);
        assert_eq!(fake.seen.borrow().len(), 2, "an invalid call is not sent");
    }

    #[tokio::test]
    async fn protocol_errors_use_json_rpc_codes() {
        let fake = FakeTransport::default();
        let unknown_tool = exchange(&fake, call(1, "browser_teleport", json!({}))).await;
        assert_eq!(unknown_tool["error"]["code"], INVALID_PARAMS);
        let unknown_method = exchange(&fake, json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/list" })).await;
        assert_eq!(unknown_method["error"]["code"], METHOD_NOT_FOUND);
        let not_rpc = exchange(&fake, json!({ "id": 3, "method": "ping" })).await;
        assert_eq!(not_rpc["error"]["code"], INVALID_REQUEST);
        let malformed = handle(&fake, Err(FrameError::Malformed("eof".into()))).await.expect("answered");
        assert_eq!(malformed["id"], Value::Null);
        assert_eq!(malformed["error"]["code"], PARSE_ERROR);
        let oversized = handle(&fake, Err(FrameError::TooLarge { max: 1 })).await.expect("answered");
        assert_eq!(oversized["error"]["code"], INVALID_REQUEST);
        assert!(fake.seen.borrow().is_empty());
    }

    #[tokio::test]
    async fn stdio_framing_answers_in_order_and_skips_notifications() {
        let fake = FakeTransport::default();
        let input = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"browser_list\"}}\n",
        );
        let mut output = Vec::new();
        run(&fake, input.as_bytes(), &mut output, MAX_FRAME_BYTES).await.expect("server runs to EOF");
        let ids: Vec<Value> = String::from_utf8(output)
            .expect("utf-8 output")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("JSON line")["id"].clone())
            .collect();
        assert_eq!(ids, vec![json!(1), json!(2), json!(3)]);
    }

    /// The MCP adapter against the real browser CLI server: calls are admitted by the same
    /// controller authority as `ferryx browser <cmd>`, and a revoked controller is refused.
    #[cfg(all(feature = "desktop", any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    mod through_policy {
        use super::*;
        use crate::browser::agent::authority::AuthorityState;
        use crate::browser::agent::service::with_state_for_test;
        use crate::browser::agent::wire::{AgentRequest, AgentResponse};
        use crate::browser::agent::{ApprovalResolution, ControllerId, GrantScope};
        use crate::browser::{BrowserManager, BrowserProfileId, CreateBrowserRequest};
        use crate::ipc::browser_cli::{BrowserCliRequest, BrowserCliResponse, ControllerAuth};
        use std::path::PathBuf;
        use std::sync::Arc;
        use std::time::Instant;

        const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef";
        const ORIGIN: &str = "https://mcp-policy.test";

        struct ServerTransport {
            endpoint: PathBuf,
            auth: Option<ControllerAuth>,
        }

        impl Transport for ServerTransport {
            async fn browser(&self, request: BrowserCliRequest) -> Result<BrowserCliResponse, String> {
                crate::ipc::browser_cli::send_browser_cli_request_for_test(request, &self.endpoint, self.auth.clone())
                    .await
                    .map_err(|error| error.to_string())
            }

            async fn agent(&self, _request: AgentRequest) -> Result<AgentResponse, String> {
                Err("the controller sub-protocol is not exercised here".into())
            }
        }

        fn pair(state: &mut AuthorityState) -> ControllerId {
            let now = Instant::now();
            let id = ControllerId::generate();
            let pairing = state.request_pairing(id, "mcp-policy", SECRET, now).expect("pairing request");
            state.resolve(pairing, ApprovalResolution::ApproveOnce, now).expect("pairing approved");
            id
        }

        fn grant_interact(state: &mut AuthorityState, id: ControllerId) {
            let now = Instant::now();
            let grant = state.request_origin_grant(id, ORIGIN, &[GrantScope::Interact], now).expect("grant request");
            state.resolve(grant, ApprovalResolution::ApproveOnce, now).expect("grant approved");
        }

        fn tool_code(response: &Value) -> Option<&str> {
            response["result"]["structuredContent"]["code"].as_str()
        }

        #[tokio::test]
        async fn mcp_calls_are_admitted_by_controller_policy_and_refused_after_revoke() {
            let app = tauri::test::mock_builder()
                .build(tauri::test::mock_context(tauri::test::noop_assets()))
                .expect("mock app");
            let manager = Arc::new(BrowserManager::new());
            let session = manager
                .register_session(CreateBrowserRequest {
                    browser_id: None,
                    workspace_id: Some("mcp-policy".into()),
                    worktree_path: None,
                    url: format!("{ORIGIN}/form"),
                    profile: Some(BrowserProfileId::Default),
                    zoom_factor: None,
                    bounds: None,
                    visible: Some(true),
                })
                .expect("register session");
            let temp_dir = tempfile::tempdir().expect("tempdir");
            let endpoint = temp_dir.path().join("browser.sock");
            crate::ipc::browser_cli::start_browser_cli_server_for_test(app.handle().clone(), Arc::clone(&manager), &endpoint)
                .expect("start browser CLI server");

            let id = with_state_for_test(pair);
            let controller = ServerTransport {
                endpoint,
                auth: Some(ControllerAuth { id: id.to_string(), secret: SECRET.into() }),
            };
            let tab = session.browser_id;

            // Policy runs: a paired controller without a grant sees no tabs and may not observe.
            let listed = exchange(&controller, call(1, "browser_list", json!({}))).await;
            assert_eq!(listed["result"]["isError"], false, "{listed}");
            let sessions = listed["result"]["structuredContent"]["sessions"].as_array().expect("sessions");
            assert!(sessions.iter().all(|summary| summary["browserId"] != json!(tab)), "{listed}");
            let observed = exchange(&controller, call(2, "browser_snapshot", json!({ "tab_id": tab }))).await;
            assert_eq!(observed["result"]["isError"], true, "{observed}");
            assert_eq!(tool_code(&observed), Some("BROWSER_FORBIDDEN"), "{observed}");

            // With an interact grant the act is admitted; it then fails at the page (the ref
            // was never observed), not at the authority.
            with_state_for_test(|state| grant_interact(state, id));
            let generation = manager.get_state(&tab).expect("session state").generation;
            let act = json!({ "tab_id": tab, "generation": generation, "action": { "kind": "click", "ref": "d1.1" } });
            let admitted = exchange(&controller, call(3, "browser_act", act.clone())).await;
            let admitted_code = tool_code(&admitted);
            for refusal in ["BROWSER_CONTROLLER_REVOKED", "BROWSER_FORBIDDEN", "BROWSER_CONTROLLER_PAIRING_REQUIRED"] {
                assert_ne!(admitted_code, Some(refusal), "{admitted}");
            }

            // Revoked: every call, act included, is refused by the authority.
            assert!(with_state_for_test(|state| state.revoke(&id, Instant::now())));
            let revoked = exchange(&controller, call(4, "browser_act", act)).await;
            assert_eq!(revoked["result"]["isError"], true, "{revoked}");
            assert_eq!(tool_code(&revoked), Some("BROWSER_CONTROLLER_REVOKED"), "{revoked}");
            let revoked_list = exchange(&controller, call(5, "browser_list", json!({}))).await;
            assert_eq!(tool_code(&revoked_list), Some("BROWSER_CONTROLLER_REVOKED"), "{revoked_list}");
        }
    }
}
