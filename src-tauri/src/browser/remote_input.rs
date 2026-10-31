use crate::browser::model::LogicalRect;
use crate::ipc::error::IpcErrorCode;
use std::time::{Duration, Instant};

pub const MAX_FILL_BYTES: usize = 16 * 1024; // 16 KiB
pub const MAX_EVAL_SCRIPT_BYTES: usize = 32 * 1024; // 32 KiB
pub const MAX_EVAL_OUTPUT_BYTES: usize = 65_536; // 65,536 UTF-8 bytes
pub const MAX_FRAME_AGE: Duration = Duration::from_millis(2000); // 2.0s

#[derive(Debug, Clone, PartialEq)]
pub struct LogicalPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RemoteInputError {
    Unsupported(&'static str),
    /// Point input refused because the target sits on an input class v1 cannot drive
    /// (iframe / canvas / dialog). Typed so no caller has to infer the class from the message.
    RefusedInput(RefusedInputToken),
    OutOfBounds { u: f64, v: f64 },
    NonFiniteCoordinate,
    InvalidReference(&'static str),
    FillTooLarge { actual: usize, max: usize },
    EvalScriptTooLarge { actual: usize, max: usize },
    EvalApprovalRequired,
    KeyNotAllowed(String),
    StaleViewport { expected: u64, actual: u64 },
    StaleFrameAge { age_ms: u64, max_ms: u64 },
    StaleDocumentGeneration { expected: u64, actual: u64 },
}

impl std::fmt::Display for RemoteInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(msg) => write!(f, "unsupported operation: {}", msg),
            Self::RefusedInput(token) => {
                write!(f, "unsupported operation: {}", token.canonical_message())
            }
            Self::OutOfBounds { u, v } => {
                write!(f, "normalized coordinates out of bounds: ({}, {})", u, v)
            }
            Self::NonFiniteCoordinate => write!(f, "coordinate is NaN or Infinity"),
            Self::InvalidReference(msg) => write!(f, "invalid element reference: {}", msg),
            Self::FillTooLarge { actual, max } => {
                write!(f, "fill text too large: {} bytes (max {})", actual, max)
            }
            Self::EvalScriptTooLarge { actual, max } => {
                write!(f, "eval script too large: {} bytes (max {})", actual, max)
            }
            Self::EvalApprovalRequired => {
                write!(f, "eval operation requires explicit driver approval")
            }
            Self::KeyNotAllowed(k) => {
                write!(f, "keypress '{}' not allowed (page-scoped allowlist)", k)
            }
            Self::StaleViewport { expected, actual } => {
                write!(
                    f,
                    "stale viewport revision: expected {}, actual {}",
                    expected, actual
                )
            }
            Self::StaleFrameAge { age_ms, max_ms } => {
                write!(
                    f,
                    "stale frame: age {} ms exceeds max {} ms",
                    age_ms, max_ms
                )
            }
            Self::StaleDocumentGeneration { expected, actual } => {
                write!(
                    f,
                    "stale document generation: expected {}, actual {}",
                    expected, actual
                )
            }
        }
    }
}

impl std::error::Error for RemoteInputError {}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefusedInputExplanation {
    pub code: String,
    pub reason: String,
    pub input_class: String,
    pub remediation: String,
}

impl std::fmt::Display for RefusedInputExplanation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.reason)
    }
}

impl RefusedInputExplanation {
    /// Public wire shape of the explanation, carried as the optional `details` field of a
    /// `BrowserError` frame. Built with `json!` so the key names are the single source of
    /// truth and no fallible serialization step can silently drop the explanation.
    pub fn to_details(&self) -> serde_json::Value {
        serde_json::json!({
            "code": self.code,
            "reason": self.reason,
            "inputClass": self.input_class,
            "remediation": self.remediation,
        })
    }
}

/// Machine token a page-side point-click script puts in the `refused` field of its action
/// result. Parsed as an enum: the token is never matched by substring, and an unknown token
/// is a decode failure instead of a silently accepted click.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RefusedInputToken {
    Iframe,
    Canvas,
    Dialog,
}

impl RefusedInputToken {
    /// Wire token exactly as the page-side script emits it.
    pub fn as_token(self) -> &'static str {
        match self {
            Self::Iframe => "iframe",
            Self::Canvas => "canvas",
            Self::Dialog => "dialog",
        }
    }

    /// Canonical refusal message for this input class. Byte-identical to the message
    /// `map_point_mainframe` produces for the same class, so the refusal explanation stays
    /// single-sourced between the page-side detection and the coordinate mapper.
    pub fn canonical_message(self) -> &'static str {
        match self {
            Self::Iframe => {
                "point input on iframes is unsupported in v1; use reference-based interaction instead"
            }
            Self::Canvas => {
                "point input on canvas is unsupported in v1; use reference-based interaction instead"
            }
            Self::Dialog => {
                "point input on dialogs is unsupported in v1; use reference-based interaction instead"
            }
        }
    }

    /// Typed refusal for this token, carrying the canonical message.
    pub fn to_input_error(self) -> RemoteInputError {
        RemoteInputError::RefusedInput(self)
    }

    /// `input_class` this token classifies to in `RemoteInputError::explanation`.
    pub fn explanation_input_class(self) -> &'static str {
        match self {
            Self::Iframe => "iframe_point",
            Self::Canvas => "canvas_point",
            Self::Dialog => "dialog_point",
        }
    }

    /// Remediation shown to the operator for this input class.
    pub fn remediation(self) -> &'static str {
        match self {
            Self::Iframe => {
                "Point clicks on iframes are unsupported in v1; use reference-based interaction instead."
            }
            Self::Canvas => {
                "Point clicks on canvas elements are unsupported in v1; use reference-based interaction instead."
            }
            Self::Dialog => {
                "Point clicks on dialogs are unsupported in v1; use host dialog controls or reference-based interaction instead."
            }
        }
    }
}

/// Typed outcome of decoding an automation action result payload.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionDecodeError {
    /// The page refused the input for a machine-token reason (iframe / canvas / dialog).
    Refused(RemoteInputError),
    /// The target element was not present at the requested location.
    NotFound(String),
    /// The page reported a failure that carries no recognized structured code.
    Failed(String),
    /// The payload was not a decodable action result.
    Malformed(String),
}

impl std::fmt::Display for ActionDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(err) => write!(f, "{}", err),
            Self::NotFound(message) => write!(f, "target element not found: {}", message),
            Self::Failed(message) => write!(f, "action failed: {}", message),
            Self::Malformed(message) => write!(f, "malformed action result: {}", message),
        }
    }
}

impl std::error::Error for ActionDecodeError {}

impl ActionDecodeError {
    /// Structured explanation for a typed refusal, if this outcome is one.
    pub fn explanation(&self) -> Option<RefusedInputExplanation> {
        match self {
            Self::Refused(err) => Some(err.explanation()),
            _ => None,
        }
    }
}

impl RemoteInputError {
    /// Maps to existing structured `IpcErrorCode` enum variants from `crate::ipc::error::IpcErrorCode`.
    pub fn ipc_error_code(&self) -> IpcErrorCode {
        match self {
            Self::Unsupported(_) => IpcErrorCode::Unsupported,
            Self::RefusedInput(_) => IpcErrorCode::Unsupported,
            Self::OutOfBounds { .. } => IpcErrorCode::BrowserBoundsInvalid,
            Self::NonFiniteCoordinate => IpcErrorCode::InvalidArgument,
            Self::InvalidReference(_) => IpcErrorCode::BrowserInvalidReference,
            Self::FillTooLarge { .. } | Self::EvalScriptTooLarge { .. } => {
                IpcErrorCode::PayloadTooLarge
            }
            Self::EvalApprovalRequired => IpcErrorCode::PermissionDenied,
            Self::KeyNotAllowed(_) => IpcErrorCode::InvalidRequest,
            Self::StaleViewport { .. } => IpcErrorCode::StaleRevision,
            Self::StaleFrameAge { .. } => IpcErrorCode::Timeout,
            Self::StaleDocumentGeneration { .. } => IpcErrorCode::StaleGeneration,
        }
    }

    /// Stable error code string.
    pub fn error_code_str(&self) -> &'static str {
        match self {
            Self::Unsupported(_) => "UNSUPPORTED",
            Self::RefusedInput(_) => "UNSUPPORTED",
            Self::OutOfBounds { .. } => "BROWSER_BOUNDS_INVALID",
            Self::NonFiniteCoordinate => "INVALID_ARGUMENT",
            Self::InvalidReference(_) => "BROWSER_INVALID_REFERENCE",
            Self::FillTooLarge { .. } | Self::EvalScriptTooLarge { .. } => "PAYLOAD_TOO_LARGE",
            Self::EvalApprovalRequired => "PERMISSION_DENIED",
            Self::KeyNotAllowed(_) => "KEY_NOT_ALLOWED",
            Self::StaleViewport { .. } => "STALE_REVISION",
            Self::StaleFrameAge { .. } => "TIMEOUT",
            Self::StaleDocumentGeneration { .. } => "STALE_GENERATION",
        }
    }

    /// Structured human-readable explanation and remediation for refused remote input.
    pub fn explanation(&self) -> RefusedInputExplanation {
        match self {
            // The refusal class is typed, never inferred from the message text.
            Self::RefusedInput(token) => RefusedInputExplanation {
                code: self.error_code_str().to_string(),
                reason: token.canonical_message().to_string(),
                input_class: token.explanation_input_class().to_string(),
                remediation: token.remediation().to_string(),
            },
            Self::Unsupported(msg) => RefusedInputExplanation {
                code: self.error_code_str().to_string(),
                reason: msg.to_string(),
                input_class: "unsupported".to_string(),
                remediation:
                    "Point input on this surface is unsupported in v1; use reference-based interaction instead."
                        .to_string(),
            },
            Self::OutOfBounds { u, v } => RefusedInputExplanation {
                code: self.error_code_str().to_string(),
                reason: format!(
                    "Normalized coordinates ({:.3}, {:.3}) are outside the viewport [0.0, 1.0]",
                    u, v
                ),
                input_class: "malformed_point".to_string(),
                remediation:
                    "Ensure click target coordinates are within the active viewport bounds."
                        .to_string(),
            },
            Self::NonFiniteCoordinate => RefusedInputExplanation {
                code: self.error_code_str().to_string(),
                reason: "Coordinates must be finite numbers (received NaN or Infinity)"
                    .to_string(),
                input_class: "malformed_point".to_string(),
                remediation: "Provide valid numeric coordinate values.".to_string(),
            },
            Self::InvalidReference(msg) => RefusedInputExplanation {
                code: self.error_code_str().to_string(),
                reason: format!("Invalid element reference: {}", msg),
                input_class: "reference".to_string(),
                remediation: "Acquire a fresh snapshot and verify element target reference."
                    .to_string(),
            },
            Self::FillTooLarge { actual, max } => RefusedInputExplanation {
                code: self.error_code_str().to_string(),
                reason: format!(
                    "Fill text too large: {} bytes (max allowed: {} bytes)",
                    actual, max
                ),
                input_class: "fill".to_string(),
                remediation: "Split input into chunks smaller than 16 KiB.".to_string(),
            },
            Self::EvalScriptTooLarge { actual, max } => RefusedInputExplanation {
                code: self.error_code_str().to_string(),
                reason: format!(
                    "Eval script too large: {} bytes (max allowed: {} bytes)",
                    actual, max
                ),
                input_class: "eval".to_string(),
                remediation: "Reduce script size to under 32 KiB.".to_string(),
            },
            Self::EvalApprovalRequired => RefusedInputExplanation {
                code: self.error_code_str().to_string(),
                reason: "Remote JavaScript evaluation requires explicit driver approval"
                    .to_string(),
                input_class: "eval".to_string(),
                remediation: "Grant driver execution approval before invoking eval.".to_string(),
            },
            Self::KeyNotAllowed(k) => RefusedInputExplanation {
                code: self.error_code_str().to_string(),
                reason: format!(
                    "Key '{}' is refused (app-chrome shortcuts and non-allowlisted keys are blocked)",
                    k
                ),
                input_class: "key".to_string(),
                remediation:
                    "Use standard text input or allowlisted navigation keys (Enter, Tab, Esc, Arrows)."
                        .to_string(),
            },
            Self::StaleViewport { expected, actual } => RefusedInputExplanation {
                code: self.error_code_str().to_string(),
                reason: format!(
                    "Viewport revision mismatch: expected {}, actual {}",
                    expected, actual
                ),
                input_class: "viewport".to_string(),
                remediation: "Wait for latest frame render and re-dispatch on current revision."
                    .to_string(),
            },
            Self::StaleFrameAge { age_ms, max_ms } => RefusedInputExplanation {
                code: self.error_code_str().to_string(),
                reason: format!(
                    "Frame age {} ms exceeds staleness fence {} ms",
                    age_ms, max_ms
                ),
                input_class: "timing".to_string(),
                remediation: "Wait for incoming live stream frame to refresh.".to_string(),
            },
            Self::StaleDocumentGeneration { expected, actual } => RefusedInputExplanation {
                code: self.error_code_str().to_string(),
                reason: format!(
                    "Document generation mismatch: expected {}, actual {}",
                    expected, actual
                ),
                input_class: "generation".to_string(),
                remediation:
                    "Document navigated; re-acquire elements on current document generation."
                        .to_string(),
            },
        }
    }
}

/// Maps normalized coordinate (u, v) in [0.0, 1.0] onto the captured viewport, in the
/// viewport-relative CSS pixels `document.elementFromPoint` expects. The rect's x/y place
/// the webview inside the app window and are display geometry only, so they are not added.
/// Enforces v1 binding decision: MAIN-FRAME only! Iframes, canvas, dialogs are explicitly rejected as unsupported.
pub fn map_point_mainframe(
    u: f64,
    v: f64,
    capture_rect: &LogicalRect,
    in_subframe: bool,
    in_canvas: bool,
    in_dialog: bool,
) -> Result<LogicalPoint, RemoteInputError> {
    if in_subframe || in_canvas || in_dialog {
        // A single refused class is carried as the typed token; a combination that has no
        // single token keeps the descriptive multi-class message.
        return Err(match (in_subframe, in_canvas, in_dialog) {
            (true, false, false) => RemoteInputError::RefusedInput(RefusedInputToken::Iframe),
            (false, true, false) => RemoteInputError::RefusedInput(RefusedInputToken::Canvas),
            (false, false, true) => RemoteInputError::RefusedInput(RefusedInputToken::Dialog),
            _ => RemoteInputError::Unsupported(
                "point input on iframes, canvas, and dialogs is unsupported in v1; use reference-based interaction instead",
            ),
        });
    }

    if !u.is_finite() || !v.is_finite() {
        return Err(RemoteInputError::NonFiniteCoordinate);
    }

    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return Err(RemoteInputError::OutOfBounds { u, v });
    }

    let x = u * capture_rect.width;
    let y = v * capture_rect.height;

    Ok(LogicalPoint { x, y })
}

/// Fences point input against frame staleness and viewport revisions.
pub fn validate_point_timing_and_viewport(
    frame_timestamp: Instant,
    current_viewport_revision: u64,
    frame_viewport_revision: u64,
    current_generation: u64,
    frame_generation: u64,
) -> Result<(), RemoteInputError> {
    let age = Instant::now().duration_since(frame_timestamp);
    if age > MAX_FRAME_AGE {
        return Err(RemoteInputError::StaleFrameAge {
            age_ms: age.as_millis() as u64,
            max_ms: MAX_FRAME_AGE.as_millis() as u64,
        });
    }

    if current_viewport_revision != frame_viewport_revision {
        return Err(RemoteInputError::StaleViewport {
            expected: current_viewport_revision,
            actual: frame_viewport_revision,
        });
    }

    if current_generation != frame_generation {
        return Err(RemoteInputError::StaleDocumentGeneration {
            expected: current_generation,
            actual: frame_generation,
        });
    }

    Ok(())
}

/// Validates fill payload.
pub fn validate_fill(reference: &str, value: &str) -> Result<(), RemoteInputError> {
    let trimmed_ref = reference.trim();
    if trimmed_ref.is_empty() {
        return Err(RemoteInputError::InvalidReference(
            "reference cannot be empty",
        ));
    }

    if value.len() > MAX_FILL_BYTES {
        return Err(RemoteInputError::FillTooLarge {
            actual: value.len(),
            max: MAX_FILL_BYTES,
        });
    }

    Ok(())
}

/// Page-scoped keypress allowlist.
/// Rejects chrome-level shortcuts (Cmd+T, Cmd+W, Cmd+Q, Alt+F4, Super keys).
pub fn validate_page_key(key: &str) -> Result<String, RemoteInputError> {
    if key.chars().any(char::is_control) {
        return Err(RemoteInputError::KeyNotAllowed("control character".into()));
    }
    // The KeyboardEvent key value for the space bar is " ", which trimming would erase.
    // "Space" is its `code` name, accepted as an alias and dispatched as the key value.
    if key == " " || key.trim() == "Space" {
        return Ok(" ".to_string());
    }
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err(RemoteInputError::KeyNotAllowed("empty key".into()));
    }

    // Explicitly forbidden dangerous app-chrome shortcuts
    let lower = trimmed.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "tabclose"
            | "newtab"
            | "appquit"
            | "commandpalette"
            | "settings"
            | "sidebartoggle"
            | "super"
            | "meta"
            | "alt+f4"
            | "cmd+q"
            | "cmd+w"
            | "cmd+t"
            | "ctrl+w"
            | "ctrl+t"
    ) {
        return Err(RemoteInputError::KeyNotAllowed(trimmed.to_string()));
    }

    // Allowed page-scoped keys:
    // Single unicode characters (letters, numbers, punctuation) or standard named navigation keys
    let is_named_allowed = matches!(
        trimmed,
        "Enter"
            | "Backspace"
            | "Tab"
            | "Escape"
            | "Delete"
            | "ArrowUp"
            | "ArrowDown"
            | "ArrowLeft"
            | "ArrowRight"
            | "PageUp"
            | "PageDown"
            | "Home"
            | "End"
    );

    let is_single_char = trimmed.chars().count() == 1;

    if is_named_allowed || is_single_char {
        Ok(trimmed.to_string())
    } else {
        Err(RemoteInputError::KeyNotAllowed(trimmed.to_string()))
    }
}

/// Validates eval script. Must have explicit owner approval and size <= 32 KiB.
pub fn validate_eval_script(script: &str, has_approval: bool) -> Result<(), RemoteInputError> {
    if !has_approval {
        return Err(RemoteInputError::EvalApprovalRequired);
    }

    if script.len() > MAX_EVAL_SCRIPT_BYTES {
        return Err(RemoteInputError::EvalScriptTooLarge {
            actual: script.len(),
            max: MAX_EVAL_SCRIPT_BYTES,
        });
    }

    Ok(())
}

/// Truncates eval result at 65,536 UTF-8 bytes at character boundary, preserving truncate semantics.
pub fn truncate_eval_result(output: &str) -> (String, bool) {
    if output.len() <= MAX_EVAL_OUTPUT_BYTES {
        return (output.to_string(), false);
    }

    // Find the largest character boundary <= MAX_EVAL_OUTPUT_BYTES
    let mut boundary = MAX_EVAL_OUTPUT_BYTES;
    while boundary > 0 && !output.is_char_boundary(boundary) {
        boundary -= 1;
    }

    (output[..boundary].to_string(), true)
}

/// Structured failure code the host-built click/fill/type scripts write for a missing target.
pub const ACTION_CODE_NOT_FOUND: &str = "not_found";

/// Decodes the JSON evaluation result returned by automation actions (fill, type)
/// and verifies that target lookup succeeded (R5-6).
///
/// A failure is classified only by its structured `code` field; the free-text `error`
/// is page-reachable and never selects the kind.
pub fn decode_action_result(raw: &str) -> Result<(), ActionDecodeError> {
    let parsed: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| ActionDecodeError::Malformed(format!("invalid action result json: {e}")))?;
    let obj = if let Some(s) = parsed.as_str() {
        serde_json::from_str::<serde_json::Value>(s).map_err(|e| {
            ActionDecodeError::Malformed(format!("invalid nested action result json: {e}"))
        })?
    } else {
        parsed
    };
    if obj.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err_msg = obj
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("target element not found");
        return Err(
            match obj.get("code").and_then(|v| v.as_str()) {
                Some(ACTION_CODE_NOT_FOUND) => ActionDecodeError::NotFound(err_msg.to_string()),
                _ => ActionDecodeError::Failed(err_msg.to_string()),
            },
        );
    }
    Ok(())
}

/// Builds the page-side point-click script executed in the target webview.
///
/// After resolving the element under the point it refuses — without clicking — when the
/// element is a frame, a canvas, or inside an open dialog, returning a machine token the
/// Rust decoder parses as a typed refusal. This is the only place the token strings are
/// written; the tokens come from [`RefusedInputToken::as_token`].
pub fn build_point_click_script(x: f64, y: f64) -> String {
    format!(
        r#"(function() {{
            const el = document.elementFromPoint({}, {});
            if (!el) return JSON.stringify({{ ok: false, code: "not_found", error: "no element at coordinates" }});
            const tag = el.tagName ? el.tagName.toUpperCase() : "";
            if (tag === "IFRAME" || tag === "FRAME") return JSON.stringify({{ ok: false, refused: "{}" }});
            if (tag === "CANVAS") return JSON.stringify({{ ok: false, refused: "{}" }});
            if (el.closest && el.closest("dialog[open]")) return JSON.stringify({{ ok: false, refused: "{}" }});
            el.click();
            return JSON.stringify({{ ok: true }});
        }})()"#,
        x,
        y,
        RefusedInputToken::Iframe.as_token(),
        RefusedInputToken::Canvas.as_token(),
        RefusedInputToken::Dialog.as_token(),
    )
}

/// Builds the page-side selector-click script executed in the target webview.
///
/// `selector_json` is a JSON string literal the host built from a verified selector; the
/// script never reads page text to pick its failure kind.
pub fn build_selector_click_script(selector_json: &str) -> String {
    format!(
        r#"(function() {{
            const el = document.querySelector({});
            if (!el) return JSON.stringify({{ ok: false, code: "not_found", error: "element not found" }});
            el.click();
            return JSON.stringify({{ ok: true }});
        }})()"#,
        selector_json
    )
}

/// Typed decode of a point-click action result payload (R4-8 / audit D-10).
///
/// - `{"ok":true}` -> `Ok(())`
/// - `{"ok":false,"refused":"<iframe|canvas|dialog>"}` -> typed refusal carrying the same
///   canonical message `map_point_mainframe` uses for that input class
/// - `{"ok":false,"code":"not_found",...}` -> typed not-found
/// - any other `{"ok":false,...}` -> generic failure; the free-text `error` is
///   page-reachable and never selects the kind
/// - anything else (non-JSON, non-object, non-boolean `ok`, unknown `refused` token) -> malformed
pub fn decode_point_click_result(raw: &str) -> Result<(), ActionDecodeError> {
    let parsed: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| ActionDecodeError::Malformed(format!("invalid action result json: {e}")))?;
    let obj = if let Some(nested) = parsed.as_str() {
        serde_json::from_str::<serde_json::Value>(nested).map_err(|e| {
            ActionDecodeError::Malformed(format!("invalid nested action result json: {e}"))
        })?
    } else {
        parsed
    };
    let map = obj.as_object().ok_or_else(|| {
        ActionDecodeError::Malformed("action result is not a json object".to_string())
    })?;

    match map.get("ok") {
        Some(serde_json::Value::Bool(true)) => return Ok(()),
        Some(serde_json::Value::Bool(false)) => {}
        Some(other) => {
            return Err(ActionDecodeError::Malformed(format!(
                "action result 'ok' must be a boolean, got {other}"
            )))
        }
        None => {
            return Err(ActionDecodeError::Malformed(
                "action result is missing 'ok'".to_string(),
            ))
        }
    }

    if let Some(token_value) = map.get("refused") {
        let token: RefusedInputToken = serde_json::from_value(token_value.clone()).map_err(|_| {
            ActionDecodeError::Malformed(format!("unrecognized refused token: {token_value}"))
        })?;
        return Err(ActionDecodeError::Refused(token.to_input_error()));
    }

    let message = map
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("target element not found")
        .to_string();
    Err(match map.get("code").and_then(|v| v.as_str()) {
        Some(ACTION_CODE_NOT_FOUND) => ActionDecodeError::NotFound(message),
        _ => ActionDecodeError::Failed(message),
    })
}

type ImeSupersessionMap = std::collections::HashMap<(String, u64, String), u64>;

/// Tracks the highest queued IME fill revision per (browser_id, lease_epoch, target)
/// to enforce supersession at execution time (R5-13).
/// Entries are pruned per (browser_id, lease_epoch) when that lease ends
/// (see [`prune_ime_supersession`]).
pub static IME_SUPERSESSION_TRACKER: std::sync::LazyLock<parking_lot::Mutex<ImeSupersessionMap>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

/// Drops every IME supersession entry recorded for `browser_id` under `lease_epoch`.
pub fn prune_ime_supersession(browser_id: &str, lease_epoch: u64) {
    IME_SUPERSESSION_TRACKER
        .lock()
        .retain(|(b, e, _), _| !(b == browser_id && *e == lease_epoch));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_input_mainframe_only() {
        let rect = LogicalRect {
            x: 50.0,
            y: 100.0,
            width: 800.0,
            height: 600.0,
        };

        // Center of viewport (0.5, 0.5). The webview sits at (50, 100) in the window, but
        // the click point is viewport-relative, so that offset is not added.
        let pt = map_point_mainframe(0.5, 0.5, &rect, false, false, false).unwrap();
        assert_eq!(pt.x, 400.0);
        assert_eq!(pt.y, 300.0);

        // Top-left (0.0, 0.0)
        let pt_tl = map_point_mainframe(0.0, 0.0, &rect, false, false, false).unwrap();
        assert_eq!(pt_tl.x, 0.0);
        assert_eq!(pt_tl.y, 0.0);

        // Bottom-right (1.0, 1.0) is the viewport's far corner.
        let pt_br = map_point_mainframe(1.0, 1.0, &rect, false, false, false).unwrap();
        assert_eq!(pt_br.x, 800.0);
        assert_eq!(pt_br.y, 600.0);
    }

    #[test]
    fn test_point_input_rejects_out_of_bounds_and_nan() {
        let rect = LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };

        // Negative
        assert!(matches!(
            map_point_mainframe(-0.1, 0.5, &rect, false, false, false),
            Err(RemoteInputError::OutOfBounds { .. })
        ));

        // Greater than 1.0
        assert!(matches!(
            map_point_mainframe(0.5, 1.05, &rect, false, false, false),
            Err(RemoteInputError::OutOfBounds { .. })
        ));

        // NaN
        assert!(matches!(
            map_point_mainframe(f64::NAN, 0.5, &rect, false, false, false),
            Err(RemoteInputError::NonFiniteCoordinate)
        ));

        // Infinity
        assert!(matches!(
            map_point_mainframe(0.5, f64::INFINITY, &rect, false, false, false),
            Err(RemoteInputError::NonFiniteCoordinate)
        ));
    }

    #[test]
    fn test_point_input_rejects_iframe_canvas_dialog() {
        let rect = LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };

        // Iframe subframe
        let err_iframe = map_point_mainframe(0.5, 0.5, &rect, true, false, false);
        assert_eq!(
            err_iframe,
            Err(RemoteInputError::RefusedInput(RefusedInputToken::Iframe))
        );

        // Canvas
        let err_canvas = map_point_mainframe(0.5, 0.5, &rect, false, true, false);
        assert_eq!(
            err_canvas,
            Err(RemoteInputError::RefusedInput(RefusedInputToken::Canvas))
        );

        // Dialog
        let err_dialog = map_point_mainframe(0.5, 0.5, &rect, false, false, true);
        assert_eq!(
            err_dialog,
            Err(RemoteInputError::RefusedInput(RefusedInputToken::Dialog))
        );

        // A combination with no single token keeps the descriptive unsupported message.
        assert!(matches!(
            map_point_mainframe(0.5, 0.5, &rect, true, true, false),
            Err(RemoteInputError::Unsupported(_))
        ));
    }

    #[test]
    fn test_point_input_refusal_explanations_adversarial() {
        let rect = LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };

        // 1. Iframe refusal with structured explanation
        let err_iframe = map_point_mainframe(0.5, 0.5, &rect, true, false, false).unwrap_err();
        assert_eq!(err_iframe.error_code_str(), "UNSUPPORTED");
        assert_eq!(err_iframe.ipc_error_code(), IpcErrorCode::Unsupported);
        let expl_iframe = err_iframe.explanation();
        assert_eq!(expl_iframe.input_class, "iframe_point");
        assert!(expl_iframe.reason.contains("iframes"));

        // 2. Canvas refusal with structured explanation
        let err_canvas = map_point_mainframe(0.5, 0.5, &rect, false, true, false).unwrap_err();
        assert_eq!(err_canvas.error_code_str(), "UNSUPPORTED");
        assert_eq!(err_canvas.ipc_error_code(), IpcErrorCode::Unsupported);
        let expl_canvas = err_canvas.explanation();
        assert_eq!(expl_canvas.input_class, "canvas_point");
        assert!(expl_canvas.reason.contains("canvas"));

        // 3. Dialog refusal with structured explanation
        let err_dialog = map_point_mainframe(0.5, 0.5, &rect, false, false, true).unwrap_err();
        assert_eq!(err_dialog.error_code_str(), "UNSUPPORTED");
        assert_eq!(err_dialog.ipc_error_code(), IpcErrorCode::Unsupported);
        let expl_dialog = err_dialog.explanation();
        assert_eq!(expl_dialog.input_class, "dialog_point");
        assert!(expl_dialog.reason.contains("dialogs"));

        // 4. App-chrome key refusal with structured explanation
        let err_key = validate_page_key("Cmd+T").unwrap_err();
        assert_eq!(err_key.error_code_str(), "KEY_NOT_ALLOWED");
        assert_eq!(err_key.ipc_error_code(), IpcErrorCode::InvalidRequest);
        let expl_key = err_key.explanation();
        assert_eq!(expl_key.input_class, "key");
        assert!(expl_key.reason.contains("Cmd+T"));

        // 5. Malformed point refusal (out of bounds & non-finite)
        let err_oob = map_point_mainframe(-0.5, 0.5, &rect, false, false, false).unwrap_err();
        assert_eq!(err_oob.error_code_str(), "BROWSER_BOUNDS_INVALID");
        assert_eq!(err_oob.ipc_error_code(), IpcErrorCode::BrowserBoundsInvalid);
        let expl_oob = err_oob.explanation();
        assert_eq!(expl_oob.input_class, "malformed_point");

        let err_nan = map_point_mainframe(f64::NAN, 0.5, &rect, false, false, false).unwrap_err();
        assert_eq!(err_nan.error_code_str(), "INVALID_ARGUMENT");
        assert_eq!(err_nan.ipc_error_code(), IpcErrorCode::InvalidArgument);
        let expl_nan = err_nan.explanation();
        assert_eq!(expl_nan.input_class, "malformed_point");
    }

    #[test]
    fn test_reference_click_and_fill() {
        // Valid fill
        assert!(validate_fill("elem-42", "hello world").is_ok());

        // Empty reference rejected
        assert!(matches!(
            validate_fill("   ", "hello"),
            Err(RemoteInputError::InvalidReference(_))
        ));

        // Over 16 KiB fill rejected
        let huge_value = "a".repeat(16 * 1024 + 1);
        assert!(matches!(
            validate_fill("elem-1", &huge_value),
            Err(RemoteInputError::FillTooLarge { .. })
        ));
    }

    #[test]
    fn validate_page_key_rejects_control_characters() {
        for key in ["\u{0}", "\u{7}", "\u{1b}", "\u{7f}", "\u{85}"] {
            assert!(
                matches!(validate_page_key(key), Err(RemoteInputError::KeyNotAllowed(_))),
                "control character {key:?} must be rejected"
            );
        }
        assert_eq!(validate_page_key("a").unwrap(), "a");
    }

    #[test]
    fn test_keypress_allowlist() {
        // Allowed keys
        assert_eq!(validate_page_key("a").unwrap(), "a");
        assert_eq!(validate_page_key("Enter").unwrap(), "Enter");
        assert_eq!(validate_page_key("Backspace").unwrap(), "Backspace");
        assert_eq!(validate_page_key("Tab").unwrap(), "Tab");
        assert_eq!(validate_page_key("ArrowDown").unwrap(), "ArrowDown");
        // The space bar is dispatched with its KeyboardEvent key value " ", whether the
        // client sends that value or the "Space" code name.
        assert_eq!(validate_page_key(" ").unwrap(), " ");
        assert_eq!(validate_page_key("Space").unwrap(), " ");

        // Disallowed chrome shortcuts
        assert!(matches!(
            validate_page_key("cmd+q"),
            Err(RemoteInputError::KeyNotAllowed(_))
        ));
        assert!(matches!(
            validate_page_key("NewTab"),
            Err(RemoteInputError::KeyNotAllowed(_))
        ));
        assert!(matches!(
            validate_page_key("TabClose"),
            Err(RemoteInputError::KeyNotAllowed(_))
        ));
        assert!(matches!(
            validate_page_key("Alt+F4"),
            Err(RemoteInputError::KeyNotAllowed(_))
        ));
    }

    #[test]
    fn test_eval_64k_cap_and_approval() {
        // Approval required
        assert!(matches!(
            validate_eval_script("2 + 2", false),
            Err(RemoteInputError::EvalApprovalRequired)
        ));
        assert!(validate_eval_script("2 + 2", true).is_ok());

        // Script > 32 KiB rejected
        let huge_script = "x".repeat(32 * 1024 + 1);
        assert!(matches!(
            validate_eval_script(&huge_script, true),
            Err(RemoteInputError::EvalScriptTooLarge { .. })
        ));

        // Result truncation at 65,536 UTF-8 bytes
        let short_out = "Hello World";
        let (out, truncated) = truncate_eval_result(short_out);
        assert_eq!(out, short_out);
        assert!(!truncated);

        // Long string truncated
        let long_out = "한".repeat(30_000); // 30,000 * 3 = 90,000 bytes
        let (out, truncated) = truncate_eval_result(&long_out);
        assert!(truncated);
        assert!(out.len() <= 65_536);
        assert!(
            std::str::from_utf8(out.as_bytes()).is_ok(),
            "must be valid UTF-8 boundary"
        );
    }

    #[test]
    fn test_r5_6_decode_action_result_rejects_missing_target() {
        // Ok result
        assert!(decode_action_result(r#"{"ok":true}"#).is_ok());
        assert!(decode_action_result(r#""{\"ok\":true}""#).is_ok());

        // Target not found error
        let err1 = decode_action_result(r#"{"ok":false,"error":"element not found"}"#).unwrap_err();
        assert_eq!(err1, ActionDecodeError::Failed("element not found".into()));

        let err2 =
            decode_action_result(r#""{\"ok\":false,\"error\":\"no element at coordinates\"}""#)
                .unwrap_err();
        assert_eq!(err2, ActionDecodeError::Failed("no element at coordinates".into()));

        // Missing ok or ok != true
        assert!(decode_action_result(r#"{"error":"failed"}"#).is_err());
    }

    /// M5: only the host-written `code` classifies a failure; free text never does.
    #[test]
    fn decode_action_result_classifies_by_code_not_text() {
        assert_eq!(
            decode_action_result(r#"{"ok":false,"error":"element not found"}"#),
            Err(ActionDecodeError::Failed("element not found".into()))
        );
        assert_eq!(
            decode_action_result(r#"{"ok":false,"code":"not_found","error":"element not found"}"#),
            Err(ActionDecodeError::NotFound("element not found".into()))
        );
        assert_eq!(
            decode_action_result(r#""{\"ok\":false,\"code\":\"not_found\"}""#),
            Err(ActionDecodeError::NotFound("target element not found".into()))
        );
        assert_eq!(
            decode_action_result(r#"{"ok":false,"code":"other","error":"x"}"#),
            Err(ActionDecodeError::Failed("x".into()))
        );
        assert!(matches!(
            decode_action_result("not json"),
            Err(ActionDecodeError::Malformed(_))
        ));
    }

    // RED mutation: change any string in `RefusedInputToken::canonical_message` (or in the
    // matching `map_point_mainframe` arm) so the two sources disagree -> this test fails.
    #[test]
    fn test_refused_token_messages_match_point_mainframe() {
        let rect = LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };

        for (token, in_subframe, in_canvas, in_dialog) in [
            (RefusedInputToken::Iframe, true, false, false),
            (RefusedInputToken::Canvas, false, true, false),
            (RefusedInputToken::Dialog, false, false, true),
        ] {
            let mapped = map_point_mainframe(0.5, 0.5, &rect, in_subframe, in_canvas, in_dialog)
                .expect_err("refused input class must not map to a point");
            assert_eq!(
                mapped.to_string(),
                format!("unsupported operation: {}", token.canonical_message()),
                "page-side refusal token '{}' must reuse the mapper's canonical message",
                token.as_token()
            );
            assert!(matches!(
                token.to_input_error(),
                RemoteInputError::RefusedInput(matched) if matched == token
            ));
            assert_eq!(
                token.to_input_error().explanation().input_class,
                token.explanation_input_class()
            );
        }
    }

    // RED mutation: drop the `refused` branch of `decode_point_click_result` (falling through
    // to the generic failure) -> the iframe/canvas/dialog assertions below fail.
    #[test]
    fn test_decode_point_click_result_typed_outcomes() {
        assert_eq!(decode_point_click_result(r#"{"ok":true}"#), Ok(()));
        assert_eq!(decode_point_click_result(r#""{\"ok\":true}""#), Ok(()));

        for (payload, token) in [
            (r#"{"ok":false,"refused":"iframe"}"#, RefusedInputToken::Iframe),
            (r#"{"ok":false,"refused":"canvas"}"#, RefusedInputToken::Canvas),
            (r#"{"ok":false,"refused":"dialog"}"#, RefusedInputToken::Dialog),
        ] {
            let decoded = decode_point_click_result(payload).expect_err("refusal must not decode as ok");
            match &decoded {
                ActionDecodeError::Refused(err) => {
                    assert_eq!(err, &token.to_input_error());
                    assert_eq!(err.error_code_str(), "UNSUPPORTED");
                }
                other => panic!("expected typed refusal for {payload}, got {other:?}"),
            }
            let explanation = decoded
                .explanation()
                .expect("a typed refusal must carry its explanation");
            assert_eq!(explanation.code, "UNSUPPORTED");
            assert_eq!(explanation.reason, token.canonical_message());
            assert!(!explanation.remediation.is_empty());
        }

        assert_eq!(
            decode_point_click_result(r#"{"ok":false,"code":"not_found","error":"element not found"}"#),
            Err(ActionDecodeError::NotFound("element not found".to_string()))
        );
        assert_eq!(
            decode_point_click_result(r#"{"ok":false}"#),
            Err(ActionDecodeError::Failed(
                "target element not found".to_string()
            ))
        );
    }

    /// Finding 3: click scripts tag a missing target with the structured code, and the
    /// point-click decoder classifies by that code alone; untagged text is a generic failure.
    #[test]
    fn click_scripts_and_decoder_classify_missing_target_by_code() {
        let tagged = r#"ok: false, code: "not_found""#;
        let selector_script = build_selector_click_script(r##""#btn""##);
        assert!(
            selector_script.contains(tagged),
            "selector-click script must tag a missing target with code not_found: {selector_script}"
        );
        let point_script = build_point_click_script(1.0, 2.0);
        assert!(
            point_script.contains(tagged),
            "point-click script must tag an empty point with code not_found: {point_script}"
        );

        assert_eq!(
            decode_point_click_result(r#"{"ok":false,"code":"not_found","error":"element not found"}"#),
            Err(ActionDecodeError::NotFound("element not found".into()))
        );
        assert_eq!(
            decode_point_click_result(r#""{\"ok\":false,\"code\":\"not_found\"}""#),
            Err(ActionDecodeError::NotFound("target element not found".into()))
        );
        assert_eq!(
            decode_point_click_result(r#"{"ok":false,"error":"element not found"}"#),
            Err(ActionDecodeError::Failed("element not found".into()))
        );
        assert_eq!(
            decode_point_click_result(r#"{"ok":false,"code":"other","error":"x"}"#),
            Err(ActionDecodeError::Failed("x".into()))
        );
    }

    // RED mutation: accept an unknown `refused` token or a non-boolean `ok` instead of
    // failing the decode -> the malformed assertions below fail.
    #[test]
    fn test_decode_point_click_result_rejects_malformed_payloads() {
        for payload in [
            "not json at all",
            r#""not nested json""#,
            r#"[1,2,3]"#,
            r#"{"ok":"yes"}"#,
            r#"{"error":"failed"}"#,
            r#"{"ok":false,"refused":"iframe_pointer"}"#,
            r#"{"ok":false,"refused":true}"#,
        ] {
            match decode_point_click_result(payload) {
                Err(ActionDecodeError::Malformed(message)) => assert!(
                    !message.is_empty(),
                    "malformed decode must explain itself for {payload}"
                ),
                other => panic!("expected a malformed decode for {payload}, got {other:?}"),
            }
        }
    }

    // RED mutation: delete the `IFRAME`/`FRAME` refusal branch from `build_point_click_script`
    // (or emit a different token) -> the iframe assertions below fail.
    #[test]
    fn test_point_click_script_refuses_frames_canvas_and_dialogs() {
        let script = build_point_click_script(12.5, 34.25);

        assert!(
            script.contains("elementFromPoint(12.5, 34.25)"),
            "the script must probe the requested point: {script}"
        );
        assert!(script.contains("tag === \"IFRAME\" || tag === \"FRAME\""));
        assert!(script.contains("tag === \"CANVAS\""));
        assert!(script.contains("el.closest(\"dialog[open]\")"));

        let click_at = script
            .find("el.click()")
            .expect("the script must click the resolved element");
        for token in [
            RefusedInputToken::Iframe,
            RefusedInputToken::Canvas,
            RefusedInputToken::Dialog,
        ] {
            let refusal = format!("refused: \"{}\"", token.as_token());
            let refusal_at = script
                .find(&refusal)
                .unwrap_or_else(|| panic!("the script must refuse '{}' without clicking", token.as_token()));
            assert!(
                refusal_at < click_at,
                "the '{}' refusal must be returned before any click",
                token.as_token()
            );
        }

        // Exactly one click site: a refusal branch can never fall through into it.
        assert_eq!(script.matches("el.click()").count(), 1);

        // The tokens the script emits are the ones the decoder accepts.
        for token in [
            RefusedInputToken::Iframe,
            RefusedInputToken::Canvas,
            RefusedInputToken::Dialog,
        ] {
            let payload = format!(r#"{{"ok":false,"refused":"{}"}}"#, token.as_token());
            assert_eq!(
                decode_point_click_result(&payload),
                Err(ActionDecodeError::Refused(token.to_input_error())),
                "the emitted token '{}' must decode to its typed refusal",
                token.as_token()
            );
        }
    }

    // RED mutation: rename a key in `RefusedInputExplanation::to_details` (e.g. `inputClass`
    // -> `input_class`) -> the wire-shape assertions below fail.
    #[test]
    fn test_refused_explanation_details_wire_shape() {
        for token in [
            RefusedInputToken::Iframe,
            RefusedInputToken::Canvas,
            RefusedInputToken::Dialog,
        ] {
            let explanation = token.to_input_error().explanation();
            let details = explanation.to_details();
            let object = details.as_object().expect("details must be a json object");
            assert_eq!(object.len(), 4);
            for key in ["code", "reason", "inputClass", "remediation"] {
                assert!(
                    object.get(key).and_then(|v| v.as_str()).is_some(),
                    "details.{key} must be a string for token {}",
                    token.as_token()
                );
            }
            assert_eq!(
                details.get("code").and_then(|v| v.as_str()),
                Some("UNSUPPORTED")
            );
            assert_eq!(
                details.get("inputClass").and_then(|v| v.as_str()),
                Some(token.explanation_input_class())
            );
            // The hand-built wire shape must equal the serde form of the same struct.
            assert_eq!(
                details,
                serde_json::to_value(&explanation).expect("explanation is serializable")
            );
        }
    }
}
