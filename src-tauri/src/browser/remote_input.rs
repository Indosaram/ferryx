use crate::browser::model::LogicalRect;
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

/// Maps normalized coordinate (u, v) in [0.0, 1.0] onto logical capture rect.
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
        return Err(RemoteInputError::Unsupported(
            "point input on iframes, canvas, and dialogs is unsupported in v1; use reference-based interaction instead",
        ));
    }

    if !u.is_finite() || !v.is_finite() {
        return Err(RemoteInputError::NonFiniteCoordinate);
    }

    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return Err(RemoteInputError::OutOfBounds { u, v });
    }

    let x = capture_rect.x + u * capture_rect.width;
    let y = capture_rect.y + v * capture_rect.height;

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
            | "Space"
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

/// Decodes the JSON evaluation result returned by automation actions (click, fill)
/// and verifies that target lookup succeeded (R5-6).
pub fn decode_action_result(raw: &str) -> Result<(), String> {
    let parsed: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("invalid action result json: {e}"))?;
    let obj = if let Some(s) = parsed.as_str() {
        serde_json::from_str::<serde_json::Value>(s)
            .map_err(|e| format!("invalid nested action result json: {e}"))?
    } else {
        parsed
    };
    if obj.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err_msg = obj
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("target element not found");
        return Err(err_msg.to_string());
    }
    Ok(())
}

/// Tracks the highest queued IME fill revision per (browser_id, lease_epoch, target)
/// to enforce supersession at execution time (R5-13).
pub static IME_SUPERSESSION_TRACKER: std::sync::LazyLock<
    parking_lot::Mutex<std::collections::HashMap<(String, u64, String), u64>>,
> = std::sync::LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

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

        // Center of viewport (0.5, 0.5)
        let pt = map_point_mainframe(0.5, 0.5, &rect, false, false, false).unwrap();
        assert_eq!(pt.x, 50.0 + 400.0);
        assert_eq!(pt.y, 100.0 + 300.0);

        // Top-left (0.0, 0.0)
        let pt_tl = map_point_mainframe(0.0, 0.0, &rect, false, false, false).unwrap();
        assert_eq!(pt_tl.x, 50.0);
        assert_eq!(pt_tl.y, 100.0);
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
        assert!(matches!(err_iframe, Err(RemoteInputError::Unsupported(_))));

        // Canvas
        let err_canvas = map_point_mainframe(0.5, 0.5, &rect, false, true, false);
        assert!(matches!(err_canvas, Err(RemoteInputError::Unsupported(_))));

        // Dialog
        let err_dialog = map_point_mainframe(0.5, 0.5, &rect, false, false, true);
        assert!(matches!(err_dialog, Err(RemoteInputError::Unsupported(_))));
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
    fn test_keypress_allowlist() {
        // Allowed keys
        assert_eq!(validate_page_key("a").unwrap(), "a");
        assert_eq!(validate_page_key("Enter").unwrap(), "Enter");
        assert_eq!(validate_page_key("Backspace").unwrap(), "Backspace");
        assert_eq!(validate_page_key("Tab").unwrap(), "Tab");
        assert_eq!(validate_page_key("ArrowDown").unwrap(), "ArrowDown");

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
        assert_eq!(err1, "element not found");

        let err2 =
            decode_action_result(r#""{\"ok\":false,\"error\":\"no element at coordinates\"}""#)
                .unwrap_err();
        assert_eq!(err2, "no element at coordinates");

        // Missing ok or ok != true
        assert!(decode_action_result(r#"{"error":"failed"}"#).is_err());
    }
}
