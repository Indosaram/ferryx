use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tauri::Manager;

use crate::daemon::client::DaemonClient;
use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use crate::remote::design_mode::DesignModeSnapshot;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
const MAX_PNG_BYTES: usize = 20 * 1024 * 1024;
const MAX_WRITE_ATTEMPTS: u64 = 8;
const RETENTION_DURATION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignFeedbackDelivery {
    pub png_path: String,
    pub prompt: String,
    pub bytes_written: usize,
}

/// Untrusted field text is flattened to one line: control characters (including CR/LF/TAB and ESC)
/// become spaces, runs of whitespace collapse, and the result is trimmed. A prompt assembled from
/// sanitized fields can therefore never inject extra lines or terminal control sequences into the
/// target session's PTY.
pub fn sanitize_prompt_field(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn design_feedback_png_path(dir: &Path, timestamp_ms: u64, nonce: u64) -> PathBuf {
    dir.join(format!("{timestamp_ms}-{nonce:016x}.png"))
}

pub fn build_design_feedback_prompt(
    snapshot: &DesignModeSnapshot,
    png_path: &str,
    memo: &str,
) -> String {
    let element_line = if let Some(el) = snapshot.dom_elements.first() {
        format!(
            "Element: {} id={} bounds=({},{},{},{})",
            sanitize_prompt_field(&el.tag),
            sanitize_prompt_field(&el.id),
            el.bounds[0],
            el.bounds[1],
            el.bounds[2],
            el.bounds[3]
        )
    } else {
        "Element: (capture only)".to_string()
    };

    let sanitized_memo = sanitize_prompt_field(memo);
    let note_line = if sanitized_memo.is_empty() {
        "Note: (no memo)".to_string()
    } else {
        format!("Note: {sanitized_memo}")
    };

    format!(
        "Design feedback from the in-app browser.\n{element_line}\nScreenshot: {}\n{note_line}\n",
        sanitize_prompt_field(png_path)
    )
}

/// Best-effort retention: expired captures are deleted oldest first. Every failure is ignored so a
/// prune problem can never fail a delivery.
fn prune_expired_captures(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let cutoff = SystemTime::now()
        .checked_sub(RETENTION_DURATION)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut captures: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("png") {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|metadata| metadata.modified()) else {
            continue;
        };
        captures.push((modified, path));
    }
    captures.sort_by_key(|(modified, _)| *modified);
    for (modified, path) in captures {
        if modified < cutoff {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn write_capture(dir: &Path, timestamp_ms: u64, nonce: u64, bytes: &[u8]) -> Result<PathBuf, IpcError> {
    std::fs::create_dir_all(dir).map_err(|error| {
        IpcError::new(
            IpcErrorCode::IoError,
            format!("Failed to create design-feedback directory: {error}"),
        )
    })?;

    for attempt in 0..MAX_WRITE_ATTEMPTS {
        let candidate = design_feedback_png_path(dir, timestamp_ms, nonce.wrapping_add(attempt));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(mut file) => {
                file.write_all(bytes).map_err(|error| {
                    IpcError::new(
                        IpcErrorCode::IoError,
                        format!("Failed to write screenshot PNG: {error}"),
                    )
                })?;
                prune_expired_captures(dir);
                return Ok(candidate);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(IpcError::new(
                    IpcErrorCode::IoError,
                    format!("Failed to create screenshot PNG: {error}"),
                ))
            }
        }
    }

    Err(IpcError::new(
        IpcErrorCode::IoError,
        "Failed to allocate a unique screenshot PNG name",
    ))
}

#[tauri::command]
pub async fn cmd_design_feedback_deliver<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    session_id: String,
    memo: String,
    snapshot: DesignModeSnapshot,
) -> Result<DesignFeedbackDelivery, IpcError> {
    if memo.trim().is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidRequest,
            "Feedback memo cannot be empty",
        ));
    }

    let png_bytes = STANDARD
        .decode(snapshot.screenshot_png_base64.trim())
        .map_err(|e| {
            IpcError::new(
                IpcErrorCode::ParseError,
                format!("Invalid screenshot base64: {e}"),
            )
        })?;

    if png_bytes.len() > MAX_PNG_BYTES {
        return Err(IpcError::new(
            IpcErrorCode::InvalidRequest,
            format!(
                "Screenshot PNG size ({} bytes) exceeds the {} byte limit",
                png_bytes.len(),
                MAX_PNG_BYTES
            ),
        ));
    }

    if !png_bytes.starts_with(&PNG_SIGNATURE) {
        return Err(IpcError::new(
            IpcErrorCode::ParseError,
            "Screenshot data is not a PNG image",
        ));
    }

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| IpcError::internal(format!("Failed to resolve app data dir: {e}")))?;
    let dir = app_data_dir.join("design-feedback");
    let timestamp_ms = snapshot.timestamp_ms;
    let nonce: u64 = rand::random();

    let png_path = run_blocking(move || write_capture(&dir, timestamp_ms, nonce, &png_bytes)).await?;

    let png_path_str = png_path.to_string_lossy().to_string();
    let prompt = build_design_feedback_prompt(&snapshot, &png_path_str, &memo);

    let daemon_client = app.state::<Arc<DaemonClient>>().inner().clone();
    daemon_client
        .write_terminal(&session_id, prompt.clone().into_bytes())
        .await?;

    let bytes_written = prompt.len();

    Ok(DesignFeedbackDelivery {
        png_path: png_path_str,
        prompt,
        bytes_written,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::design_mode::DomElementBox;

    fn snapshot_with(elements: Vec<DomElementBox>) -> DesignModeSnapshot {
        DesignModeSnapshot {
            session_id: "s-1".into(),
            timestamp_ms: 1234567890,
            screenshot_png_base64: String::new(),
            outer_html: String::new(),
            css: String::new(),
            dom_elements: elements,
        }
    }

    #[test]
    fn design_feedback_prompt_carries_memo_element_and_screenshot_path() {
        let snapshot = snapshot_with(vec![DomElementBox {
            id: "el-1".into(),
            tag: "button".into(),
            bounds: [10.0, 20.0, 100.0, 40.0],
            text: Some("Submit".into()),
        }]);

        let prompt = build_design_feedback_prompt(
            &snapshot,
            "/tmp/x.png",
            "This button needs 8px left margin",
        );

        assert!(prompt.contains("Design feedback from the in-app browser."));
        assert!(prompt.contains("button"));
        assert!(prompt.contains("id=el-1"));
        assert!(prompt.contains("/tmp/x.png"));
        assert!(prompt.contains("This button needs 8px left margin"));
        assert!(prompt.ends_with('\n'));
        assert!(!prompt.ends_with("\n\n"));
    }

    #[test]
    fn design_feedback_prompt_degrades_without_element_or_memo() {
        let prompt = build_design_feedback_prompt(&snapshot_with(vec![]), "/tmp/capture.png", "");

        assert!(prompt.contains("Element: (capture only)"));
        assert!(prompt.contains("Note: (no memo)"));
        assert!(prompt.ends_with('\n'));
        assert!(!prompt.ends_with("\n\n"));
    }

    #[test]
    fn design_feedback_prompt_flattens_control_characters() {
        let prompt = build_design_feedback_prompt(
            &snapshot_with(vec![]),
            "/tmp/capture.png",
            "add 8px\r\nmargin\u{1b}[31m now",
        );

        let note_line = prompt
            .lines()
            .find(|line| line.starts_with("Note:"))
            .expect("prompt must carry a note line");

        assert!(note_line.contains("add 8px margin"));
        assert!(!prompt.contains('\r'));
        assert!(!prompt.contains('\u{1b}'));
        assert_eq!(prompt.lines().count(), 4);
    }

    #[test]
    fn design_feedback_prompt_flattens_element_fields() {
        let prompt = build_design_feedback_prompt(
            &snapshot_with(vec![DomElementBox {
                id: "bad\nid\r\nname".into(),
                tag: "div\nbutton".into(),
                bounds: [0.0, 0.0, 10.0, 10.0],
                text: None,
            }]),
            "/tmp/capture.png",
            "valid memo",
        );

        let element_line = prompt
            .lines()
            .find(|line| line.starts_with("Element:"))
            .expect("prompt must carry an element line");

        assert_eq!(prompt.lines().count(), 4);
        assert!(element_line.contains("div button"));
        assert!(element_line.contains("id=bad id name"));
    }

    #[test]
    fn design_feedback_png_path_uses_timestamp_nonce_and_png_extension() {
        let dir = Path::new("/var/app/design-feedback");
        let timestamp = 1718000000123;
        let path = design_feedback_png_path(dir, timestamp, 0x4a12_beef_1234_5678);

        let file_name = path.file_name().unwrap().to_str().unwrap();
        assert!(file_name.starts_with(&format!("{timestamp}-")));
        assert!(file_name.ends_with(".png"));
    }

    #[test]
    fn write_capture_never_reuses_an_existing_name() {
        let dir = tempfile::tempdir().unwrap();
        let first = write_capture(dir.path(), 1718000000123, 7, b"\x89PNG\r\n\x1a\nfirst").unwrap();
        let second = write_capture(dir.path(), 1718000000123, 7, b"\x89PNG\r\n\x1a\nsecond").unwrap();

        assert_ne!(first, second);
        assert_eq!(std::fs::read(&first).unwrap(), b"\x89PNG\r\n\x1a\nfirst");
        assert_eq!(std::fs::read(&second).unwrap(), b"\x89PNG\r\n\x1a\nsecond");
    }
}
