//! Security unit tests covering URL allowlist, permissions, path redaction, eval truncation, and deduplication
//! Authoritative Spec: docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md (§6.1, §6.3, §6.4, §7.1, Phase 7A)

use super::browser_security::*;
use crate::remote::auth::DevicePermission;
use std::time::{Duration, Instant};

#[test]
fn test_driver_claim_requires_control_permission() {
    // 1. Control device can perform Control operations
    assert!(require_permission(DevicePermission::Control, DevicePermission::Control).is_ok());

    // 2. Control device can perform View operations
    assert!(require_permission(DevicePermission::Control, DevicePermission::View).is_ok());

    // 3. View-only device can perform View operations
    assert!(require_permission(DevicePermission::View, DevicePermission::View).is_ok());

    // 4. View-only device claiming Control must be rejected with PermissionDenied
    let err = require_permission(DevicePermission::View, DevicePermission::Control)
        .expect_err("View-only device must not acquire Control permission");
    assert_eq!(
        err,
        SecurityError::PermissionDenied("Control permission required")
    );
}

#[test]
fn test_url_allowlist_schemes_and_credentials() {
    // 1. Allowed schemes: http and https
    assert!(sanitize_url("http://localhost:3000/app").is_ok());
    assert!(sanitize_url("https://example.com/api/v1?test=1").is_ok());
    assert!(sanitize_url("https://127.0.0.1:8080").is_ok());

    // 2. Disallowed schemes: file, javascript, data, tauri, internal
    let disallowed = [
        "file:///etc/passwd",
        "file:///C:/Windows/System32/drivers/etc/hosts",
        "javascript:alert(document.cookie)",
        "data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==",
        "tauri://localhost/index.html",
        "about:blank",
        "blob:https://example.com/uuid",
    ];
    for url in disallowed {
        let err = sanitize_url(url).expect_err(&format!("Scheme should be disallowed: {url}"));
        assert!(
            matches!(err, SecurityError::DisallowedScheme(_)),
            "Expected DisallowedScheme for {url}, got {err:?}"
        );
    }

    // 3. Embedded credentials must be rejected
    let with_credentials = [
        "http://admin:secret@example.com/dashboard",
        "https://user:password@localhost:8080",
        "http://token:@internal.service",
        "https://admin@example.com",
    ];
    for url in with_credentials {
        let err = sanitize_url(url).expect_err(&format!("URL with credentials should be rejected: {url}"));
        assert_eq!(err, SecurityError::EmbeddedCredentials);
    }

    // 4. Malformed URLs rejected with InvalidUrl
    assert!(matches!(
        sanitize_url("not a valid url: //"),
        Err(SecurityError::InvalidUrl(_))
    ));
}

#[test]
fn test_local_filesystem_path_redaction() {
    // 1. Unix home and user paths
    let unix_user = "Failed to load /Users/alice/Library/Application Support/orca/settings.json";
    let redacted_user = sanitize_public_string(unix_user);
    assert!(!redacted_user.contains("/Users/"));
    assert_eq!(redacted_user, "Failed to load [redacted-path] Support/orca/settings.json");

    let unix_home = "Exception in /home/developer/code/project/src/main.rs:42";
    let redacted_home = sanitize_public_string(unix_home);
    assert!(!redacted_home.contains("/home/"));
    assert_eq!(redacted_home, "Exception in [redacted-path]");

    let unix_private = "Socket located at /private/var/folders/xx/tmp.sock";
    let redacted_private = sanitize_public_string(unix_private);
    assert!(!redacted_private.contains("/private/"));
    assert_eq!(redacted_private, "Socket located at [redacted-path]");

    // 2. Windows drive paths
    let win_c = r"File missing: C:\Users\Runner\workspace\build.log";
    let redacted_win_c = sanitize_public_string(win_c);
    assert!(!redacted_win_c.contains(r"C:\"));
    assert_eq!(redacted_win_c, "File missing: [redacted-path]");

    let win_d_slash = "Output saved to D:/projects/secret/result.bin";
    let redacted_win_d = sanitize_public_string(win_d_slash);
    assert!(!redacted_win_d.contains("D:/"));
    assert_eq!(redacted_win_d, "Output saved to [redacted-path]");

    // 3. UNC network paths
    let unc_path = r"Network share \\storage\backups\private_key.pem unreachable";
    let redacted_unc = sanitize_public_string(unc_path);
    assert!(!redacted_unc.contains(r"\\storage"));
    assert_eq!(redacted_unc, "Network share [redacted-path] unreachable");

    // 4. Explicit verification of all required path patterns:
    // assert NO raw filesystem paths (/Users/, /home/, /private/, C:\, \\) leak
    let raw_paths = [
        "/Users/indo/orca/file.txt",
        "/home/ubuntu/app/config.toml",
        "/private/tmp/socket.sock",
        r"C:\Windows\System32\cmd.exe",
        r"\\nas\share\document.pdf",
    ];
    for raw in raw_paths {
        let redacted = sanitize_public_string(raw);
        assert!(!redacted.contains("/Users/"), "Must not leak /Users/: {redacted}");
        assert!(!redacted.contains("/home/"), "Must not leak /home/: {redacted}");
        assert!(!redacted.contains("/private/"), "Must not leak /private/: {redacted}");
        assert!(!redacted.contains(r"C:\"), r"Must not leak C:\: {redacted}");
        assert!(!redacted.contains(r"\\"), r"Must not leak \\: {redacted}");
        assert!(redacted.contains("[redacted-path]"));
    }

    // 5. Safe strings without paths remain unchanged
    let safe_str = "Navigation completed successfully with status 200";
    assert_eq!(sanitize_public_string(safe_str), safe_str);
}

#[test]
fn test_eval_result_truncation_exact_64k_boundary_and_utf8_char_boundaries() {
    // 1. Exactly 65,535 bytes -> not truncated
    let s_65535 = "A".repeat(MAX_EVAL_RESULT_BYTES - 1);
    let (res, trunc) = truncate_eval_result(&s_65535);
    assert_eq!(res.len(), 65_535);
    assert!(!trunc);

    // 2. Exactly 65,536 bytes -> not truncated
    let s_65536 = "B".repeat(MAX_EVAL_RESULT_BYTES);
    let (res, trunc) = truncate_eval_result(&s_65536);
    assert_eq!(res.len(), 65_536);
    assert!(!trunc);

    // 3. Exactly 65,537 bytes -> truncated to 65,536 bytes, truncated: true
    let s_65537 = "C".repeat(MAX_EVAL_RESULT_BYTES + 1);
    let (res, trunc) = truncate_eval_result(&s_65537);
    assert_eq!(res.len(), 65_536);
    assert!(trunc);

    // 4. Korean 3-byte character boundary safety: "한" is 3 bytes (0xED, 0x95, 0x9C)
    // 21,845 characters * 3 bytes = 65,535 bytes. Adding one more character brings it to 65,538 bytes.
    // Truncation at 65,536 must cleanly back up to byte 65,535 and NOT split the 3-byte character!
    let korean_text = "한".repeat(21_846); // 65,538 bytes
    let (res_kr, trunc_kr) = truncate_eval_result(&korean_text);
    assert!(trunc_kr);
    assert_eq!(res_kr.len(), 65_535); // Clean 3-byte character boundary
    assert!(std::str::from_utf8(res_kr.as_bytes()).is_ok());

    // 5. Emoji 4-byte character boundary safety: "🚀" is 4 bytes
    // 16,384 characters * 4 bytes = 65,536 bytes exactly.
    let emoji_exact = "🚀".repeat(16_384);
    let (res_em_exact, trunc_em_exact) = truncate_eval_result(&emoji_exact);
    assert_eq!(res_em_exact.len(), 65_536);
    assert!(!trunc_em_exact);

    // 16,385 characters * 4 bytes = 65,540 bytes. Truncates at 65,536.
    let emoji_overflow = "🚀".repeat(16_385);
    let (res_em_over, trunc_em_over) = truncate_eval_result(&emoji_overflow);
    assert!(trunc_em_over);
    assert_eq!(res_em_over.len(), 65_536);
    assert!(std::str::from_utf8(res_em_over.as_bytes()).is_ok());
}

#[test]
fn test_request_deduplication_and_30s_cache_retention() {
    let dedup = RequestDeduplicator::new();
    let t0 = Instant::now();

    // 1. Initial request with sequence 1: admitted (None)
    let check1 = dedup.check_or_record(1, t0).expect("Initial check ok");
    assert!(check1.is_none());

    // Record result for request 1
    let result1 = serde_json::json!({ "success": true, "result": "click_ok" }).to_string().into_bytes();
    dedup.record_result(1, result1.clone(), t0);

    // 2. Immediate duplicate of sequence 1 at t0 + 10s: returns cached result
    let t1 = t0 + Duration::from_secs(10);
    let dup1 = dedup.check_or_record(1, t1).expect("Duplicate check ok");
    assert_eq!(dup1, Some(result1.clone()));

    // 3. New request with sequence 2 at t0 + 20s: admitted (None)
    let t2 = t0 + Duration::from_secs(20);
    let check2 = dedup.check_or_record(2, t2).expect("Seq 2 check ok");
    assert!(check2.is_none());
    let result2 = b"nav_ok".to_vec();
    dedup.record_result(2, result2.clone(), t2);

    // 4. Duplicate of sequence 1 at t0 + 29s: still within 30s TTL, returns cached result
    let t3 = t0 + Duration::from_secs(29);
    let dup1_still_valid = dedup.check_or_record(1, t3).expect("Within 30s ok");
    assert_eq!(dup1_still_valid, Some(result1));

    // 5. At t0 + 31s: request 1 has exceeded 30s cache retention TTL and is pruned.
    // Because highest_seen_seq is 2, sequence 1 has expired from cache -> returns OutcomeUnknown
    let t4 = t0 + Duration::from_secs(31);
    let expired_seq1 = dedup.check_or_record(1, t4);
    assert_eq!(expired_seq1, Err(SecurityError::OutcomeUnknown));

    // However, request 2 was recorded at t0 + 20s. At t4 (t0 + 31s), elapsed is 11s <= 30s TTL,
    // so request 2 remains in cache!
    let dup2_valid = dedup.check_or_record(2, t4).expect("Seq 2 still valid");
    assert_eq!(dup2_valid, Some(result2));

    // 6. Ancient sequence 0 (never seen or long evicted): returns OutcomeUnknown
    let ancient = dedup.check_or_record(0, t4);
    assert_eq!(ancient, Err(SecurityError::OutcomeUnknown));
}

#[test]
fn test_boundary_payload_limits_constants() {
    // Spec §6.3: Command name <= 64 KiB, script <= 32 KiB, fill <= 16 KiB
    assert_eq!(MAX_REQUEST_WIRE_BYTES, 64 * 1024);
    assert_eq!(MAX_SCRIPT_BYTES, 32 * 1024);
    assert_eq!(MAX_FILL_BYTES, 16 * 1024);
    assert_eq!(MAX_EVAL_RESULT_BYTES, 64 * 1024);
}
