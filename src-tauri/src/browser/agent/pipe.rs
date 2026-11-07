//! `ferryx browser pipe`: a persistent NDJSON automation channel on stdin/stdout.
//!
//! One request per line, snake_case keys: `{"id":1,"op":"observe","tab_id":"<id>"}`. Each
//! request is answered with one line `{"id":..,"ok":true,"result":..}` or
//! `{"id":..,"ok":false,"error":{"code","message","details"?}}`, strictly in request order.
//! Frames are bounded ([`MAX_FRAME_BYTES`]); a malformed or oversized frame is answered with a
//! typed error and the next frame is still served. Stdout carries frames only; diagnostics go
//! to stderr. EOF on stdin ends the pipe cleanly.
//!
//! The pipe holds no policy of its own: every op becomes the same `BrowserCliRequest` (or
//! controller sub-protocol request) a `ferryx browser <cmd>` call sends, carrying the paired
//! controller credential, and the running app admits it on the one CLI authority path.
//! The MCP adapter ([`super::mcp`]) shares the framing, translation and transport here.

use serde::Serialize;
use serde_json::{json, Map, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use super::catalog::GrantScope;
use super::protocol::BROWSER_CLI_REQUEST_INVALID;
use super::wire::{AgentRequest, AgentResponse};
use crate::browser::model::{BrowserAutomationAction, BrowserAutomationRequest, BrowserWaitCondition};
use crate::ipc::browser_cli::{BrowserCliRequest, BrowserCliResponse, ControllerAuth};

/// Largest accepted frame, matching the app's per-request limit.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Ops the pipe understands, named in its protocol errors.
pub const PIPE_OPS: &str =
    "list, open, observe, act, navigate, back, forward, reload, close, wait, eval, grant, await";

/// A frame that could not be read as a request.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    #[error("frame exceeds the maximum of {max} bytes")]
    TooLarge { max: usize },
    #[error("frame is not valid JSON: {0}")]
    Malformed(String),
}

impl FrameError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::TooLarge { .. } => "PIPE_FRAME_TOO_LARGE",
            Self::Malformed(_) => "PIPE_FRAME_MALFORMED",
        }
    }
}

/// Reads one newline-terminated frame of at most `max` bytes. An oversized line is drained to
/// its end (never buffered) so the next frame starts cleanly. `Ok(None)` is EOF.
async fn read_frame<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max: usize,
) -> std::io::Result<Option<Result<Vec<u8>, FrameError>>> {
    let mut frame = Vec::new();
    let mut oversized = false;
    let mut seen = false;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if !seen {
                return Ok(None);
            }
            break;
        }
        seen = true;
        let newline = available.iter().position(|byte| *byte == b'\n');
        let line_end = newline.unwrap_or(available.len());
        if !oversized {
            if frame.len() + line_end > max {
                oversized = true;
                frame = Vec::new();
            } else {
                frame.extend_from_slice(&available[..line_end]);
            }
        }
        reader.consume(newline.map_or(line_end, |index| index + 1));
        if newline.is_some() {
            break;
        }
    }
    if oversized {
        return Ok(Some(Err(FrameError::TooLarge { max })));
    }
    if frame.last() == Some(&b'\r') {
        frame.pop();
    }
    Ok(Some(Ok(frame)))
}

/// Serves frames from `reader` until EOF. Each frame is answered (when `handle` returns a
/// response) before the next is read, so responses keep request order. Blank lines are skipped.
pub(crate) async fn serve<R, W, F, Fut>(
    mut reader: R,
    mut writer: W,
    max: usize,
    mut handle: F,
) -> std::io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
    F: FnMut(Result<Value, FrameError>) -> Fut,
    Fut: std::future::Future<Output = Option<Value>>,
{
    while let Some(frame) = read_frame(&mut reader, max).await? {
        let parsed = match frame {
            Ok(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => continue,
            Ok(bytes) => serde_json::from_slice::<Value>(&bytes)
                .map_err(|error| FrameError::Malformed(error.to_string())),
            Err(error) => Err(error),
        };
        if let Some(response) = handle(parsed).await {
            let mut line = serde_json::to_vec(&response).map_err(std::io::Error::other)?;
            line.push(b'\n');
            writer.write_all(&line).await?;
            writer.flush().await?;
        }
    }
    Ok(())
}

/// How the adapters reach the running app. Production uses [`SocketTransport`].
pub(crate) trait Transport {
    async fn browser(&self, request: BrowserCliRequest) -> Result<BrowserCliResponse, String>;
    async fn agent(&self, request: AgentRequest) -> Result<AgentResponse, String>;
}

/// Sends requests over the browser CLI socket as the selected paired controller
/// (`--controller`/`FERRYX_BROWSER_CONTROLLER`, else the default selection).
pub(crate) struct SocketTransport {
    controller: Option<String>,
}

impl SocketTransport {
    pub(crate) fn new(controller: Option<String>) -> Self {
        Self { controller }
    }

    /// Read per request, so a pairing or revocation made while the adapter runs takes effect.
    fn auth(&self) -> Result<Option<ControllerAuth>, String> {
        match super::client::store_dir() {
            Some(dir) => super::client::load_auth(&dir, self.controller.as_deref()),
            None => Ok(None),
        }
    }
}

impl Transport for SocketTransport {
    async fn browser(&self, request: BrowserCliRequest) -> Result<BrowserCliResponse, String> {
        let auth = self.auth()?;
        crate::ipc::browser_cli::send_browser_cli_request_as(request, auth)
            .await
            .map_err(|error| error.to_string())
    }

    async fn agent(&self, request: AgentRequest) -> Result<AgentResponse, String> {
        let auth = self.auth()?;
        crate::ipc::browser_cli::send_agent_request(auth, request).await
    }
}

/// One adapter call, already in the app's request vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Call {
    Browser(BrowserCliRequest),
    Agent(AgentRequest),
}

/// A refused or failed call: the app's `{code,message,details}` or a local translation error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CallError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl CallError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.to_string(), message: message.into(), details: None }
    }
}

fn invalid(message: impl Into<String>) -> CallError {
    CallError::new(BROWSER_CLI_REQUEST_INVALID, message)
}

fn optional_string(args: &Map<String, Value>, key: &str) -> Result<Option<String>, CallError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(invalid(format!("`{key}` must be a string"))),
    }
}

fn required_string(args: &Map<String, Value>, key: &str) -> Result<String, CallError> {
    optional_string(args, key)?.ok_or_else(|| invalid(format!("missing `{key}`")))
}

fn tab_id(args: &Map<String, Value>) -> Result<String, CallError> {
    match optional_string(args, "tab_id")? {
        Some(id) => Ok(id),
        None => optional_string(args, "browser_id")?.ok_or_else(|| invalid("missing `tab_id`")),
    }
}

fn wait_condition(value: &Value, key: &str) -> Result<BrowserWaitCondition, CallError> {
    serde_json::from_value(value.clone()).map_err(|error| invalid(format!("invalid `{key}`: {error}")))
}

/// `action:{kind, ref?, ...}` in the pipe's snake_case shape to the typed act action.
fn act_action(action: &Map<String, Value>) -> Result<BrowserAutomationAction, CallError> {
    let kind = action
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("`action.kind` must be a string"))?;
    let kind = match kind {
        "scroll_into_view" | "scroll-into-view" => "scrollIntoView",
        other => other,
    };
    let mut shaped: Map<String, Value> = action
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "kind" | "ref"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    shaped.insert("type".into(), Value::from(kind));
    if let Some(reference) = action.get("ref") {
        shaped.insert("reference".into(), reference.clone());
    }
    serde_json::from_value(Value::Object(shaped))
        .map_err(|error| invalid(format!("invalid `action`: {error}")))
}

/// Maps one op and its arguments to the request the CLI would send. Nothing is sent here, so
/// an op this build does not advertise is refused before any dispatch.
pub(crate) fn translate(op: &str, args: &Map<String, Value>) -> Result<Call, CallError> {
    use BrowserCliRequest as R;
    let request = match op {
        "list" => R::List,
        "open" => R::Open {
            url: required_string(args, "url")?,
            workspace_id: optional_string(args, "workspace_id")?,
            worktree_path: optional_string(args, "worktree_path")?,
        },
        "observe" => R::Snapshot { browser_id: tab_id(args)? },
        "act" => {
            let generation = args
                .get("generation")
                .and_then(Value::as_u64)
                .ok_or_else(|| invalid("`generation` must be an unsigned integer"))?;
            let action = args
                .get("action")
                .and_then(Value::as_object)
                .ok_or_else(|| invalid("`action` must be an object"))?;
            let postcondition = match args.get("postcondition") {
                None | Some(Value::Null) => None,
                Some(value) => Some(wait_condition(value, "postcondition")?),
            };
            R::Act {
                request: BrowserAutomationRequest {
                    browser_id: tab_id(args)?,
                    generation,
                    action: act_action(action)?,
                    postcondition,
                },
            }
        }
        "navigate" => R::Navigate { browser_id: tab_id(args)?, url: required_string(args, "url")? },
        "back" => R::Back { browser_id: tab_id(args)? },
        "forward" => R::Forward { browser_id: tab_id(args)? },
        "reload" => R::Reload { browser_id: tab_id(args)? },
        "close" => R::Close { browser_id: tab_id(args)? },
        "wait" => {
            let condition = args.get("condition").ok_or_else(|| invalid("missing `condition`"))?;
            R::Wait { browser_id: tab_id(args)?, condition: wait_condition(condition, "condition")? }
        }
        "eval" => R::Eval { browser_id: tab_id(args)?, script: required_string(args, "script")? },
        "grant" => {
            let scopes = match args.get("scopes") {
                None | Some(Value::Null) => Vec::new(),
                Some(value) => serde_json::from_value::<Vec<GrantScope>>(value.clone())
                    .map_err(|error| invalid(format!("invalid `scopes`: {error}")))?,
            };
            return Ok(Call::Agent(AgentRequest::Grant {
                origin: optional_string(args, "origin")?,
                scopes,
                tool: optional_string(args, "tool")?,
                path: optional_string(args, "path")?,
            }));
        }
        "await" => {
            return Ok(Call::Agent(AgentRequest::Await { approval_id: required_string(args, "approval_id")? }))
        }
        "upload" | "native" => {
            return Err(CallError::new("UNSUPPORTED", format!("{op} is not advertised by this Ferryx build")))
        }
        other => {
            return Err(CallError::new(
                "PIPE_UNKNOWN_OP",
                format!("unknown op `{other}`; valid ops: {PIPE_OPS}"),
            ))
        }
    };
    Ok(Call::Browser(request))
}

fn serialized<S: Serialize>(value: &S) -> Result<Value, CallError> {
    serde_json::to_value(value)
        .map_err(|error| CallError::new("BROWSER_CLI_RESPONSE_INVALID", error.to_string()))
}

/// Sends a call and maps the app's answer: its error responses become [`CallError`]s.
pub(crate) async fn execute<T: Transport>(transport: &T, call: Call) -> Result<Value, CallError> {
    let unavailable = |message: String| CallError::new("BROWSER_CLI_UNAVAILABLE", message);
    match call {
        Call::Browser(request) => match transport.browser(request).await.map_err(unavailable)? {
            BrowserCliResponse::Error { code, message, details } => Err(CallError { code, message, details }),
            response => serialized(&response),
        },
        Call::Agent(request) => match transport.agent(request).await.map_err(unavailable)? {
            AgentResponse::Error { code, message, details } => Err(CallError { code, message, details }),
            response => serialized(&response),
        },
    }
}

fn success(id: Value, result: Value) -> Value {
    json!({ "id": id, "ok": true, "result": result })
}

fn failure(id: Value, error: &CallError) -> Value {
    json!({ "id": id, "ok": false, "error": error })
}

/// Answers one pipe frame.
pub(crate) async fn handle<T: Transport>(transport: &T, frame: Result<Value, FrameError>) -> Value {
    let request = match frame {
        Ok(request) => request,
        Err(error) => return failure(Value::Null, &CallError::new(error.code(), error.to_string())),
    };
    let Value::Object(request) = request else {
        return failure(Value::Null, &invalid("request must be a JSON object"));
    };
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let Some(op) = request.get("op").and_then(Value::as_str) else {
        return failure(id, &invalid(format!("missing `op`; valid ops: {PIPE_OPS}")));
    };
    let outcome = match translate(op, &request) {
        Ok(call) => execute(transport, call).await,
        Err(error) => Err(error),
    };
    match outcome {
        Ok(result) => success(id, result),
        Err(error) => failure(id, &error),
    }
}

/// Runs the pipe over any reader/writer pair until EOF.
pub(crate) async fn run<T, R, W>(transport: &T, reader: R, writer: W, max: usize) -> std::io::Result<()>
where
    T: Transport,
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    serve(reader, writer, max, move |frame| async move { Some(handle(transport, frame).await) }).await
}

/// `ferryx browser pipe`: serves stdin/stdout as the selected controller.
pub async fn run_stdio(controller: Option<String>) -> Result<(), String> {
    let transport = SocketTransport::new(controller);
    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    run(&transport, stdin, tokio::io::stdout(), MAX_FRAME_BYTES)
        .await
        .map_err(|error| format!("browser pipe I/O failed: {error}"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Answers like the app would without a grant: list is empty, observe is refused, the
    /// rest succeed. Records what was sent.
    #[derive(Default)]
    pub(crate) struct FakeTransport {
        pub(crate) seen: RefCell<Vec<BrowserCliRequest>>,
        pub(crate) agent_seen: RefCell<Vec<AgentRequest>>,
    }

    impl Transport for FakeTransport {
        async fn browser(&self, request: BrowserCliRequest) -> Result<BrowserCliResponse, String> {
            self.seen.borrow_mut().push(request.clone());
            Ok(match request {
                BrowserCliRequest::List => BrowserCliResponse::List { sessions: Vec::new() },
                BrowserCliRequest::Snapshot { .. } => BrowserCliResponse::Error {
                    code: "BROWSER_FORBIDDEN".into(),
                    message: "no grant".into(),
                    details: None,
                },
                _ => BrowserCliResponse::Navigated,
            })
        }

        async fn agent(&self, request: AgentRequest) -> Result<AgentResponse, String> {
            self.agent_seen.borrow_mut().push(request);
            Ok(AgentResponse::Pending { approval_id: "a1".into() })
        }
    }

    async fn run_lines(fake: &FakeTransport, input: &[u8], max: usize) -> Vec<Value> {
        let mut output = Vec::new();
        run(fake, input, &mut output, max).await.expect("pipe runs to EOF");
        String::from_utf8(output)
            .expect("utf-8 output")
            .lines()
            .map(|line| serde_json::from_str(line).expect("every output line is one JSON frame"))
            .collect()
    }

    fn error_code(response: &Value) -> &str {
        response["error"]["code"].as_str().unwrap_or_default()
    }

    #[tokio::test]
    async fn eof_ends_the_pipe_cleanly_and_a_last_unterminated_frame_is_answered() {
        let fake = FakeTransport::default();
        assert!(run_lines(&fake, b"", MAX_FRAME_BYTES).await.is_empty());
        let responses = run_lines(&fake, br#"{"id":7,"op":"list"}"#, MAX_FRAME_BYTES).await;
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["id"], 7);
        assert_eq!(responses[0]["ok"], true);
        assert_eq!(responses[0]["result"]["type"], "list");
    }

    #[tokio::test]
    async fn malformed_frame_gets_a_typed_error_and_the_next_frame_is_served() {
        let fake = FakeTransport::default();
        let input = b"{not json\n[1]\n{\"id\":2,\"op\":\"list\"}\n";
        let responses = run_lines(&fake, input, MAX_FRAME_BYTES).await;
        assert_eq!(responses.len(), 3);
        assert_eq!(responses[0]["id"], Value::Null);
        assert_eq!(error_code(&responses[0]), "PIPE_FRAME_MALFORMED");
        assert_eq!(error_code(&responses[1]), BROWSER_CLI_REQUEST_INVALID);
        assert_eq!(responses[2]["id"], 2);
        assert_eq!(responses[2]["ok"], true);
    }

    #[tokio::test]
    async fn oversized_frame_is_refused_without_breaking_the_stream() {
        let fake = FakeTransport::default();
        let mut input = format!("{{\"id\":1,\"op\":\"list\",\"pad\":\"{}\"}}\n", "x".repeat(200)).into_bytes();
        input.extend_from_slice(b"{\"id\":2,\"op\":\"list\"}\n");
        let responses = run_lines(&fake, &input, 64).await;
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0]["id"], Value::Null);
        assert_eq!(error_code(&responses[0]), "PIPE_FRAME_TOO_LARGE");
        assert_eq!(responses[1]["id"], 2);
        assert_eq!(fake.seen.borrow().len(), 1, "the oversized request is never sent");
    }

    #[tokio::test]
    async fn frame_of_exactly_the_limit_is_accepted() {
        let fake = FakeTransport::default();
        let frame = br#"{"id":1,"op":"list"}"#;
        let mut input = frame.to_vec();
        input.push(b'\n');
        let responses = run_lines(&fake, &input, frame.len()).await;
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["ok"], true);
    }

    #[tokio::test]
    async fn responses_keep_request_order_and_blank_lines_are_skipped() {
        let fake = FakeTransport::default();
        let input = concat!(
            "{\"id\":1,\"op\":\"list\"}\n",
            "\n",
            "{\"id\":2,\"op\":\"observe\",\"tab_id\":\"t1\"}\r\n",
            "   \n",
            "{\"id\":3,\"op\":\"navigate\",\"tab_id\":\"t1\",\"url\":\"https://example.test/\"}\n",
            "{\"id\":4,\"op\":\"back\",\"tab_id\":\"t1\"}\n",
        );
        let responses = run_lines(&fake, input.as_bytes(), MAX_FRAME_BYTES).await;
        let ids: Vec<Value> = responses.iter().map(|response| response["id"].clone()).collect();
        assert_eq!(ids, vec![json!(1), json!(2), json!(3), json!(4)]);
        assert_eq!(error_code(&responses[1]), "BROWSER_FORBIDDEN");
        assert_eq!(
            *fake.seen.borrow(),
            vec![
                BrowserCliRequest::List,
                BrowserCliRequest::Snapshot { browser_id: "t1".into() },
                BrowserCliRequest::Navigate { browser_id: "t1".into(), url: "https://example.test/".into() },
                BrowserCliRequest::Back { browser_id: "t1".into() },
            ]
        );
    }

    fn args(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("object arguments")
    }

    #[test]
    fn act_translates_to_the_cli_act_request() {
        let call = translate(
            "act",
            &args(json!({
                "tab_id": "t1",
                "generation": 3,
                "action": {"kind": "click", "ref": "d1.2"},
                "postcondition": {"text": "Saved"}
            })),
        )
        .expect("act translates");
        assert_eq!(
            call,
            Call::Browser(BrowserCliRequest::Act {
                request: BrowserAutomationRequest {
                    browser_id: "t1".into(),
                    generation: 3,
                    action: BrowserAutomationAction::Click { reference: "d1.2".into() },
                    postcondition: Some(BrowserWaitCondition::Text { text: "Saved".into() }),
                },
            })
        );
        let fill = translate(
            "act",
            &args(json!({
                "tab_id": "t1",
                "generation": 1,
                "action": {"kind": "fill", "ref": "d1.1", "value": "Ferryx"}
            })),
        )
        .expect("fill translates");
        let Call::Browser(BrowserCliRequest::Act { request }) = fill else {
            panic!("fill is an act");
        };
        assert_eq!(
            request.action,
            BrowserAutomationAction::Fill { reference: "d1.1".into(), value: "Ferryx".into() }
        );
        let missing = translate("act", &args(json!({"tab_id": "t1", "action": {"kind": "click", "ref": "d1"}})));
        assert_eq!(missing.expect_err("generation is required").code, BROWSER_CLI_REQUEST_INVALID);
    }

    #[test]
    fn grant_and_await_use_the_controller_sub_protocol() {
        let grant = translate(
            "grant",
            &args(json!({"origin": "http://127.0.0.1:18761", "scopes": ["read", "interact"]})),
        );
        assert_eq!(
            grant.expect("grant translates"),
            Call::Agent(AgentRequest::Grant {
                origin: Some("http://127.0.0.1:18761".into()),
                scopes: vec![GrantScope::Read, GrantScope::Interact],
                tool: None,
                path: None,
            })
        );
        let awaited = translate("await", &args(json!({"approval_id": "x"}))).expect("await translates");
        assert_eq!(awaited, Call::Agent(AgentRequest::Await { approval_id: "x".into() }));
    }

    #[tokio::test]
    async fn unadvertised_and_unknown_ops_are_refused_before_dispatch() {
        let fake = FakeTransport::default();
        let input = concat!(
            "{\"id\":1,\"op\":\"upload\",\"path\":\"/tmp/a\",\"ref\":\"d1\"}\n",
            "{\"id\":2,\"op\":\"native\",\"action\":\"key\"}\n",
            "{\"id\":3,\"op\":\"teleport\"}\n",
            "{\"id\":4}\n",
        );
        let responses = run_lines(&fake, input.as_bytes(), MAX_FRAME_BYTES).await;
        assert_eq!(responses.len(), 4);
        assert_eq!(error_code(&responses[0]), "UNSUPPORTED");
        assert_eq!(error_code(&responses[1]), "UNSUPPORTED");
        assert_eq!(error_code(&responses[2]), "PIPE_UNKNOWN_OP");
        assert_eq!(responses[3]["id"], 4);
        assert_eq!(error_code(&responses[3]), BROWSER_CLI_REQUEST_INVALID);
        assert!(fake.seen.borrow().is_empty());
        assert!(fake.agent_seen.borrow().is_empty());
    }
}
