//! Remote Browser Screencast Security, URL Allowlist & Path Sanitization
//! Authoritative Spec: docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md (§6.1, §6.3, §6.4)

use std::collections::HashMap;
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use tauri::Url;
use crate::remote::auth::DevicePermission;

pub const MAX_EVAL_RESULT_BYTES: usize = 65_536; // 64 KiB UTF-8
pub const MAX_REQUEST_WIRE_BYTES: usize = 64 * 1024;
pub const MAX_SCRIPT_BYTES: usize = 32 * 1024;
pub const MAX_FILL_BYTES: usize = 16 * 1024;
pub const MAX_RESPONSE_WIRE_BYTES: usize = 512 * 1024;

pub const DEDUP_CACHE_MAX_ENTRIES: usize = 16;
pub const DEDUP_CACHE_MAX_BYTES: usize = 1024 * 1024; // 1 MiB
pub const DEDUP_CACHE_TTL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecurityError {
    #[error("Permission denied: {0}")]
    PermissionDenied(&'static str),
    #[error("Disallowed URL scheme: {0}")]
    DisallowedScheme(String),
    #[error("URL contains embedded credentials")]
    EmbeddedCredentials,
    #[error("Invalid URL format: {0}")]
    InvalidUrl(String),
    #[error("Desktop lock violation: browser {0} not in shared inventory")]
    DesktopLockViolation(String),
    #[error("Rate limit exceeded: {0}")]
    RateLimitExceeded(&'static str),
    #[error("Request outcome unknown: request sequence expired")]
    OutcomeUnknown,
}

pub fn require_permission(
    granted: DevicePermission,
    required: DevicePermission,
) -> Result<(), SecurityError> {
    match (granted, required) {
        (DevicePermission::Control, _) => Ok(()),
        (DevicePermission::View, DevicePermission::View) => Ok(()),
        (DevicePermission::View, DevicePermission::Control) => {
            Err(SecurityError::PermissionDenied("Control permission required"))
        }
    }
}

pub fn sanitize_url(url_str: &str) -> Result<String, SecurityError> {
    let parsed = Url::parse(url_str).map_err(|e| SecurityError::InvalidUrl(e.to_string()))?;

    let scheme = parsed.scheme().to_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(SecurityError::DisallowedScheme(scheme));
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(SecurityError::EmbeddedCredentials);
    }

    Ok(parsed.to_string())
}

pub fn sanitize_public_string(raw: &str) -> String {
    // Redact absolute paths matching Unix /Users/, /home/, /private/, Windows drives C:\, D:\, or UNC \\
    // Preserves HTTP/HTTPS URLs even if they contain /Users/, /home/, /private/, or C:\ (e.g. in path or query).
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        let remainder = &raw[idx..];
        if remainder.starts_with("http://") || remainder.starts_with("https://") {
            out.push(ch);
            while let Some(&(_, c)) = chars.peek() {
                if c.is_whitespace()
                    || c == '"'
                    || c == '\''
                    || c == '`'
                    || c == '<'
                    || c == '>'
                    || c == ')'
                    || c == '}'
                    || c == ']'
                {
                    break;
                }
                let (_, next_ch) = chars.next().unwrap();
                out.push(next_ch);
            }
        } else if remainder.starts_with("/Users/")
            || remainder.starts_with("/home/")
            || remainder.starts_with("/private/")
        {
            out.push_str("[redacted-path]");
            // Advance past path characters until whitespace or delimiter
            while let Some(&(_, c)) = chars.peek() {
                if c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == ')' || c == '}' {
                    break;
                }
                chars.next();
            }
        } else if remainder.len() >= 3
            && remainder.as_bytes()[0].is_ascii_alphabetic()
            && remainder.as_bytes()[1] == b':'
            && (remainder.as_bytes()[2] == b'\\' || remainder.as_bytes()[2] == b'/')
        {
            out.push_str("[redacted-path]");
            while let Some(&(_, c)) = chars.peek() {
                if c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == ')' || c == '}' {
                    break;
                }
                chars.next();
            }
        } else if remainder.starts_with("\\\\") {
            out.push_str("[redacted-path]");
            while let Some(&(_, c)) = chars.peek() {
                if c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == ')' || c == '}' {
                    break;
                }
                chars.next();
            }
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn truncate_eval_result(result_str: &str) -> (String, bool) {
    if result_str.len() <= MAX_EVAL_RESULT_BYTES {
        (result_str.to_string(), false)
    } else {
        let boundary = result_str.floor_char_boundary(MAX_EVAL_RESULT_BYTES);
        (result_str[..boundary].to_string(), true)
    }
}

pub struct RequestDeduplicator {
    cache: Mutex<HashMap<u64, (Instant, Vec<u8>)>>,
    highest_seen_seq: Mutex<u64>,
}

impl RequestDeduplicator {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            highest_seen_seq: Mutex::new(0),
        }
    }

    pub fn check_or_record(&self, seq: u64, now: Instant) -> Result<Option<Vec<u8>>, SecurityError> {
        let mut cache = self.cache.lock();
        // Prune expired entries
        cache.retain(|_, (ts, _)| now.saturating_duration_since(*ts) <= DEDUP_CACHE_TTL);

        let mut highest = self.highest_seen_seq.lock();
        if seq <= *highest {
            if let Some((_, result)) = cache.get(&seq) {
                return Ok(Some(result.clone()));
            } else {
                return Err(SecurityError::OutcomeUnknown);
            }
        }

        *highest = seq;
        Ok(None)
    }

    pub fn record_result(&self, seq: u64, result: Vec<u8>, now: Instant) {
        let mut cache = self.cache.lock();
        if cache.len() < DEDUP_CACHE_MAX_ENTRIES {
            let total_bytes: usize = cache.values().map(|(_, b)| b.len()).sum();
            if total_bytes + result.len() <= DEDUP_CACHE_MAX_BYTES {
                cache.insert(seq, (now, result));
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_url_scheme_allowlist_and_embedded_credentials() {
        // http and https allowed
        assert!(sanitize_url("http://localhost:3000").is_ok());
        assert!(sanitize_url("https://example.com/test?q=1").is_ok());

        // File, javascript, data, tauri schemes rejected
        assert!(matches!(
            sanitize_url("file:///etc/passwd"),
            Err(SecurityError::DisallowedScheme(_))
        ));
        assert!(matches!(
            sanitize_url("javascript:alert(1)"),
            Err(SecurityError::DisallowedScheme(_))
        ));
        assert!(matches!(
            sanitize_url("data:text/html,<b>hi</b>"),
            Err(SecurityError::DisallowedScheme(_))
        ));
        assert!(matches!(
            sanitize_url("tauri://localhost"),
            Err(SecurityError::DisallowedScheme(_))
        ));

        // Embedded credentials rejected
        assert!(matches!(
            sanitize_url("https://admin:secret@example.com"),
            Err(SecurityError::EmbeddedCredentials)
        ));
    }

    #[test]
    fn test_eval_result_truncation_64k_boundary() {
        let ascii_small = "hello world";
        let (res, trunc) = truncate_eval_result(ascii_small);
        assert_eq!(res, "hello world");
        assert!(!trunc);

        // String exactly 65,536 bytes
        let exact = "a".repeat(MAX_EVAL_RESULT_BYTES);
        let (res, trunc) = truncate_eval_result(&exact);
        assert_eq!(res.len(), MAX_EVAL_RESULT_BYTES);
        assert!(!trunc);

        // String larger than 65,536 bytes
        let larger = "a".repeat(MAX_EVAL_RESULT_BYTES + 10);
        let (res, trunc) = truncate_eval_result(&larger);
        assert_eq!(res.len(), MAX_EVAL_RESULT_BYTES);
        assert!(trunc);

        // Multi-byte boundary safety (Korean/Emoji)
        let multi_byte = "가".repeat(30_000); // 30,000 * 3 bytes = 90,000 bytes
        let (res, trunc) = truncate_eval_result(&multi_byte);
        assert!(res.len() <= MAX_EVAL_RESULT_BYTES);
        assert!(trunc);
        // Must be valid UTF-8 and end on clean char boundary
        assert!(std::str::from_utf8(res.as_bytes()).is_ok());
    }

    #[test]
    fn test_driver_claim_requires_control_permission() {
        assert!(require_permission(DevicePermission::Control, DevicePermission::Control).is_ok());
        assert!(require_permission(DevicePermission::View, DevicePermission::View).is_ok());
        assert!(matches!(
            require_permission(DevicePermission::View, DevicePermission::Control),
            Err(SecurityError::PermissionDenied(_))
        ));
    }

    #[test]
    fn test_path_sanitization() {
        let unix_path = "Error occurred at /Users/alice/projects/orca/file.txt during load";
        assert_eq!(
            sanitize_public_string(unix_path),
            "Error occurred at [redacted-path] during load"
        );

        let win_path = "Failed to open C:\\Users\\Administrator\\app\\secret.json in viewer";
        assert_eq!(
            sanitize_public_string(win_path),
            "Failed to open [redacted-path] in viewer"
        );
    }

    #[test]
    fn test_request_deduplication_and_caching() {
        let dedup = RequestDeduplicator::new();
        let now = Instant::now();

        // 1st request with seq 1: new request
        assert_eq!(dedup.check_or_record(1, now).unwrap(), None);
        dedup.record_result(1, b"res1".to_vec(), now);

        // Duplicate request with seq 1: returns cached result
        assert_eq!(dedup.check_or_record(1, now).unwrap(), Some(b"res1".to_vec()));

        // 2nd request with seq 2: new request
        assert_eq!(dedup.check_or_record(2, now).unwrap(), None);

        // Ancient / evicted request with seq 0: returns OutcomeUnknown
        assert!(matches!(
            dedup.check_or_record(0, now),
            Err(SecurityError::OutcomeUnknown)
        ));
    }
}
