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

pub const REDACTED_PATH: &str = "[redacted-path]";

/// Characters that terminate a public URL token.
fn url_token_end(rest: &str) -> usize {
    rest.find(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | ')' | '}' | ']')
    })
    .unwrap_or(rest.len())
}

/// Characters that terminate a filesystem path token.
fn path_token_end(rest: &str) -> usize {
    rest.find(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | ')' | '}' | ']' | ',' | ';')
    })
    .unwrap_or(rest.len())
}

/// A path reference only starts at a token boundary. Without this gate an inner
/// slash (`Support/orca/x`, `and/or`) or a scheme letter (`file:///x`) would be
/// mistaken for the start of an absolute path.
fn is_path_token_start(prev: Option<char>) -> bool {
    match prev {
        None => true,
        Some(c) => !(c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '~')),
    }
}

/// Length of a leading absolute filesystem path, or `None` when `rest` does not
/// start with one.
///
/// Covers every absolute POSIX path (`/Volumes/...`, `/var/folders/...`,
/// `/tmp/...`, `/opt/...`, platform home dirs) rather than a fixed prefix list,
/// plus `~/` home-relative paths, Windows drive paths and UNC shares (R4-17).
fn absolute_path_len(rest: &str) -> Option<usize> {
    let end = path_token_end(rest);
    if end == 0 {
        return None;
    }
    let token = &rest[..end];

    // UNC share: \\server\share\file
    if token.starts_with("\\\\") && token.len() > 2 {
        return Some(end);
    }

    // Windows drive: C:\dir or D:/dir
    let bytes = token.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return Some(end);
    }

    // Home-relative path: ~/dir
    if let Some(body) = token.strip_prefix("~/") {
        return (!body.is_empty()).then_some(end);
    }

    // Absolute POSIX path: requires a named first segment and at least one more
    // separator, so prose like "and/or" is never mistaken for a path.
    let body = token.strip_prefix('/')?;
    let first = body.chars().next()?;
    if !(first.is_alphanumeric() || matches!(first, '.' | '_' | '-')) {
        return None;
    }
    body.contains('/').then_some(end)
}

/// Redacts every absolute filesystem path while leaving HTTP(S) URLs byte-identical,
/// so public discovery payloads, command results and error strings can never carry a
/// local path off the machine (§6.4, R4-17).
pub fn sanitize_public_string(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut cursor = 0usize;
    let mut prev: Option<char> = None;

    while cursor < raw.len() {
        let rest = &raw[cursor..];

        // HTTP(S) URLs are public identifiers and pass through untouched, even when
        // their path segments look like local directories.
        if rest.starts_with("http://") || rest.starts_with("https://") {
            let end = url_token_end(rest).max(1);
            out.push_str(&rest[..end]);
            cursor += end;
            prev = rest[..end].chars().next_back();
            continue;
        }

        if is_path_token_start(prev) {
            if let Some(len) = absolute_path_len(rest) {
                out.push_str(REDACTED_PATH);
                cursor += len;
                prev = Some(']');
                continue;
            }
        }

        let ch = rest.chars().next().expect("non-empty remainder");
        out.push(ch);
        cursor += ch.len_utf8();
        prev = Some(ch);
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

    /// R4-17: every absolute filesystem path is redacted, not only a hand-picked
    /// prefix list. HTTP(S) URLs stay intact so public URLs survive the sanitizer.
    #[test]
    fn test_r4_17_absolute_filesystem_paths_are_redacted_beyond_known_prefixes() {
        let leaks = [
            "Snapshot failed for /Volumes/T9-Mac/project/ferryx/secret.json",
            "Socket at /var/folders/x9/T/ferryx.sock is gone",
            "Temp file /tmp/ferryx-capture-1.jpeg missing",
            "Config /opt/homebrew/etc/ferryx.toml unreadable",
            "Bundle /Applications/Ferryx.app/Contents/MacOS/ferryx crashed",
            "Cache /Library/Caches/com.ferryx.app/state.db locked",
            "Profile ~/Projects/ferryx/profile.json denied",
        ];
        for raw in leaks {
            let redacted = sanitize_public_string(raw);
            assert!(
                redacted.contains("[redacted-path]"),
                "absolute path must be redacted: {raw} -> {redacted}"
            );
            for leaked in [
                "/Volumes/", "/var/folders", "/tmp/", "/opt/", "/Applications/", "/Library/", "~/Projects",
            ] {
                assert!(
                    !redacted.contains(leaked),
                    "sanitized output still leaks {leaked}: {redacted}"
                );
            }
        }

        // Non-http schemes must not shelter an embedded filesystem path.
        assert_eq!(
            sanitize_public_string("file:///Volumes/T9-Mac/private/key.pem"),
            "file://[redacted-path]"
        );

        // HTTP(S) URLs remain byte-identical, including path segments that look local.
        for url in [
            "https://example.com/Volumes/T9-Mac/report",
            "http://localhost:3000/tmp/preview",
            "https://example.com/var/folders/x/y",
        ] {
            assert_eq!(sanitize_public_string(url), url, "URL must be preserved");
        }

        // Ordinary prose with a single relative slash stays untouched.
        assert_eq!(
            sanitize_public_string("navigate and/or reload completed"),
            "navigate and/or reload completed"
        );
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
