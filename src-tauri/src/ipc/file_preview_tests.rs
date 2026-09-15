//! Contract tests for [`crate::ipc::file_preview`] (plan task 1).
//!
//! Every test observes a frozen requirement: retained-descriptor identity,
//! resource bounds, decoder strictness, window/handle isolation, Markdown child
//! containment and the exact bytes/headers of the loopback range service.
//! No test sleeps: HTTP cancellation is proven by reading the first streamed
//! chunk, revoking, and observing the truncated delivery.
use super::*;
use crate::ipc::file_preview_contract::{limits, FilePreviewEncoding, FilePreviewKind};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

const WINDOW: &str = "main";

fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    let mut file = std::fs::File::create(&path).expect("create fixture");
    file.write_all(bytes).expect("write fixture");
    path
}

fn request(dir: &Path, path: &str) -> FilePreviewOpenRequest {
    FilePreviewOpenRequest {
        window_label: WINDOW.to_string(),
        path: path.to_string(),
        cwd: Some(dir.to_path_buf()),
        line: None,
        col: None,
    }
}

async fn service() -> Arc<FilePreviewService> {
    FilePreviewService::start()
        .await
        .expect("capability server starts on loopback")
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("http client")
}

fn reason_of(error: &crate::ipc::error::IpcError) -> String {
    error
        .details
        .as_ref()
        .and_then(|details| details.get("reason"))
        .and_then(|reason| reason.as_str())
        .unwrap_or("<missing>")
        .to_string()
}

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        let pixels = vec![0u8; (width as usize) * (height as usize) * 4];
        writer.write_image_data(&pixels).expect("png data");
    }
    out
}

fn gif_bytes(width: u16, height: u16) -> Vec<u8> {
    let mut out = b"GIF89a".to_vec();
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&[0x00, 0x00, 0x00]);
    out
}

fn webp_vp8x_bytes(width: u32, height: u32) -> Vec<u8> {
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(b"WEBPVP8X");
    out.extend_from_slice(&10u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    let w = width - 1;
    let h = height - 1;
    out.extend_from_slice(&w.to_le_bytes()[..3]);
    out.extend_from_slice(&h.to_le_bytes()[..3]);
    out
}

fn jpeg_bytes(width: u16, height: u16) -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
    out.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&[0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

fn mp4_bytes(len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    out[..12].copy_from_slice(b"\0\0\0\x18ftypisom");
    out
}

// ---------------------------------------------------------------- pure units

#[test]
fn range_header_parses_the_three_supported_single_forms() {
    assert_eq!(
        parse_range_header("bytes=2-5", 10),
        RangeOutcome::Satisfiable { start: 2, end: 5 }
    );
    assert_eq!(
        parse_range_header("bytes=4-", 10),
        RangeOutcome::Satisfiable { start: 4, end: 9 }
    );
    assert_eq!(
        parse_range_header("bytes=-3", 10),
        RangeOutcome::Satisfiable { start: 7, end: 9 }
    );
    // suffix longer than the file clamps to the whole file
    assert_eq!(
        parse_range_header("bytes=-50", 10),
        RangeOutcome::Satisfiable { start: 0, end: 9 }
    );
    // end beyond EOF clamps
    assert_eq!(
        parse_range_header("bytes=8-99", 10),
        RangeOutcome::Satisfiable { start: 8, end: 9 }
    );
}

#[test]
fn range_header_rejects_multi_malformed_and_unsatisfiable_forms() {
    for raw in [
        "bytes=0-1,4-5",
        "bytes=abc",
        "items=0-1",
        "bytes=",
        "bytes=-",
        "bytes=5-2",
        "bytes=-0",
        "bytes=99999999999999999999-",
    ] {
        assert_eq!(
            parse_range_header(raw, 10),
            RangeOutcome::Unsatisfiable,
            "range {raw} must be refused"
        );
    }
    // start at or past EOF is unsatisfiable, including on an empty file
    assert_eq!(parse_range_header("bytes=10-", 10), RangeOutcome::Unsatisfiable);
    assert_eq!(parse_range_header("bytes=0-", 0), RangeOutcome::Unsatisfiable);
}

#[test]
fn preview_kind_and_media_type_follow_the_frozen_extension_allowlist() {
    assert_eq!(classify_extension("notes.md"), Some(FilePreviewKind::Markdown));
    assert_eq!(classify_extension("NOTES.MARKDOWN"), Some(FilePreviewKind::Markdown));
    assert_eq!(classify_extension("a.png"), Some(FilePreviewKind::Image));
    assert_eq!(classify_extension("a.jpeg"), Some(FilePreviewKind::Image));
    assert_eq!(classify_extension("a.webp"), Some(FilePreviewKind::Image));
    assert_eq!(classify_extension("a.mp4"), Some(FilePreviewKind::Video));
    assert_eq!(classify_extension("a.mov"), Some(FilePreviewKind::Video));
    // SVG is shown as text, never as an active document
    assert_eq!(classify_extension("a.svg"), Some(FilePreviewKind::Text));
    assert_eq!(classify_extension("a.rs"), Some(FilePreviewKind::Text));
    // unsupported media containers are refused, not guessed
    assert_eq!(classify_extension("a.heic"), None);
    assert_eq!(classify_extension("a.avif"), None);
    assert_eq!(video_media_type("a.mov"), Some("video/quicktime"));
    assert_eq!(video_media_type("a.webm"), Some("video/webm"));
    assert_eq!(video_media_type("a.mkv"), None);
}

#[test]
fn image_dimensions_come_from_real_signatures_not_extensions() {
    assert_eq!(
        image_signature(&png_bytes(7, 5)),
        Some(ImageSignature { media_type: "image/png", width: 7, height: 5 })
    );
    assert_eq!(
        image_signature(&gif_bytes(12, 9)),
        Some(ImageSignature { media_type: "image/gif", width: 12, height: 9 })
    );
    assert_eq!(
        image_signature(&webp_vp8x_bytes(640, 480)),
        Some(ImageSignature { media_type: "image/webp", width: 640, height: 480 })
    );
    assert_eq!(
        image_signature(&jpeg_bytes(300, 200)),
        Some(ImageSignature { media_type: "image/jpeg", width: 300, height: 200 })
    );
    assert_eq!(image_signature(b"not an image at all"), None);
    // a PNG renamed to .jpg is still a PNG; the signature decides the MIME
    let mut truncated = png_bytes(4, 4);
    truncated.truncate(10);
    assert_eq!(image_signature(&truncated), None);
}

#[test]
fn text_decoder_is_strict_and_bom_aware() {
    let (text, encoding) = decode_text("안녕\r\nhello".as_bytes()).expect("utf-8 decodes");
    assert_eq!(text, "안녕\r\nhello");
    assert_eq!(encoding, FilePreviewEncoding::Utf8);

    let mut utf16le = vec![0xFF, 0xFE];
    for unit in "가A".encode_utf16() {
        utf16le.extend_from_slice(&unit.to_le_bytes());
    }
    let (text, encoding) = decode_text(&utf16le).expect("utf-16le decodes");
    assert_eq!(text, "가A");
    assert_eq!(encoding, FilePreviewEncoding::Utf16Le);

    let mut utf16be = vec![0xFE, 0xFF];
    for unit in "가A".encode_utf16() {
        utf16be.extend_from_slice(&unit.to_be_bytes());
    }
    let (text, encoding) = decode_text(&utf16be).expect("utf-16be decodes");
    assert_eq!(text, "가A");
    assert_eq!(encoding, FilePreviewEncoding::Utf16Be);

    // UTF-8 BOM is consumed, not rendered
    let mut bom_utf8 = vec![0xEF, 0xBB, 0xBF];
    bom_utf8.extend_from_slice(b"hi");
    assert_eq!(decode_text(&bom_utf8).expect("bom utf-8").0, "hi");

    // latin-1 / malformed bytes are refused, never lossily decoded
    assert!(decode_text(&[0x68, 0xE9, 0x6C]).is_none());
    // NUL-bearing binary is refused
    assert!(decode_text(b"he\0llo").is_none());
    // odd-length UTF-16 payload is refused
    assert!(decode_text(&[0xFF, 0xFE, 0x41]).is_none());
}

#[test]
fn line_counting_ignores_a_single_trailing_newline() {
    assert_eq!(count_lines(""), 0);
    assert_eq!(count_lines("a"), 1);
    assert_eq!(count_lines("a\n"), 1);
    assert_eq!(count_lines("a\nb"), 2);
    assert_eq!(count_lines("a\r\nb\r\n"), 2);
    assert_eq!(count_lines("\n\n"), 2);
}

#[test]
fn caret_target_is_one_based_and_clamped_to_unicode_scalars() {
    let document = "가나다\nsecond line\n";
    assert_eq!(clamp_target(document, Some(2), Some(3)), Some(FilePreviewTarget { line: 2, col: Some(3) }));
    // line past EOF clamps to the last line
    assert_eq!(clamp_target(document, Some(99), None), Some(FilePreviewTarget { line: 2, col: None }));
    // column counts scalars, not bytes: "가나다" is 3 scalars, so col clamps to 4
    assert_eq!(clamp_target(document, Some(1), Some(50)), Some(FilePreviewTarget { line: 1, col: Some(4) }));
    // zero/absent inputs
    assert_eq!(clamp_target(document, Some(0), Some(0)), Some(FilePreviewTarget { line: 1, col: Some(1) }));
    assert_eq!(clamp_target(document, None, Some(4)), None);
    assert_eq!(clamp_target("", Some(3), Some(3)), Some(FilePreviewTarget { line: 1, col: Some(1) }));
}

#[test]
fn only_the_desktop_root_window_may_hold_capabilities() {
    assert!(ensure_trusted_window("main").is_ok());
    for untrusted in ["browser-6f1c", "preview-child", "", "MAIN"] {
        let error = ensure_trusted_window(untrusted).expect_err("untrusted webview refused");
        assert_eq!(reason_of(&error), "PermissionDenied");
    }
}

#[test]
fn allowed_origins_never_include_a_wildcard() {
    assert!(is_allowed_origin("http://127.0.0.1:5173"));
    assert!(is_allowed_origin("http://localhost:5173"));
    assert!(is_allowed_origin("tauri://localhost"));
    assert!(is_allowed_origin("http://tauri.localhost"));
    for denied in ["*", "null", "http://evil.test", "https://127.0.0.1:5173", "http://127.0.0.1"] {
        assert!(!is_allowed_origin(denied), "{denied} must be refused");
    }
}

// ---------------------------------------------------------------- open rules

#[tokio::test]
async fn open_decodes_bounded_text_without_a_media_capability() {
    let dir = TempDir::new().expect("tempdir");
    write_file(dir.path(), "notes.txt", "안녕하세요\nsecond\n".as_bytes());
    let service = service().await;

    let payload = service
        .open(FilePreviewOpenRequest { line: Some(2), col: Some(3), ..request(dir.path(), "notes.txt") })
        .await
        .expect("text payload");

    assert_eq!(payload.kind, FilePreviewKind::Text);
    assert_eq!(payload.display_name, "notes.txt");
    assert_eq!(payload.byte_length, "안녕하세요\nsecond\n".len() as u64);
    assert_eq!(payload.encoding, Some(FilePreviewEncoding::Utf8));
    assert_eq!(payload.text.as_deref(), Some("안녕하세요\nsecond\n"));
    assert_eq!(payload.line_count, Some(2));
    assert_eq!(payload.target, Some(FilePreviewTarget { line: 2, col: Some(3) }));
    assert_eq!(payload.media_url, None);
    assert_eq!(payload.media_type, None);
    assert_eq!(payload.handle.len(), 64);
    assert!(payload.handle.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    // the opaque handle leaks no filesystem path
    assert!(!payload.handle.contains("notes"));
}

#[tokio::test]
async fn open_offloads_file_io_off_the_async_reactor() {
    let dir = TempDir::new().expect("tempdir");
    write_file(dir.path(), "a.txt", b"hello");
    let service = service().await;

    // On a current-thread runtime a spawned task only runs when the current
    // task yields. If `open` read the file inline it would never yield.
    let observed = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&observed);
    tokio::spawn(async move { flag.store(true, Ordering::SeqCst) });
    service.open(request(dir.path(), "a.txt")).await.expect("payload");
    assert!(
        observed.load(Ordering::SeqCst),
        "open must await a blocking-pool task instead of reading on the reactor"
    );
}

#[tokio::test]
async fn open_refuses_non_regular_and_missing_paths() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::create_dir(dir.path().join("subdir")).expect("mkdir");
    let service = service().await;

    let missing = service.open(request(dir.path(), "nope.txt")).await.expect_err("missing");
    assert_eq!(reason_of(&missing), "MissingFile");

    let directory = service.open(request(dir.path(), "subdir")).await.expect_err("directory");
    assert_eq!(reason_of(&directory), "NotRegularFile");

    #[cfg(unix)]
    {
        let fifo = dir.path().join("pipe.txt");
        let c_path = std::ffi::CString::new(fifo.to_string_lossy().as_bytes()).expect("cstring");
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0, "mkfifo");
        let error = service.open(request(dir.path(), "pipe.txt")).await.expect_err("fifo");
        assert_eq!(reason_of(&error), "NotRegularFile");

        let device = service.open(request(dir.path(), "/dev/zero")).await.expect_err("device");
        assert_eq!(reason_of(&device), "NotRegularFile");
    }
}

#[tokio::test]
async fn open_refuses_remote_specs_urls_and_nul_bytes() {
    let dir = TempDir::new().expect("tempdir");
    let service = service().await;

    for token in ["https://example.test/a.txt", "user@host:/etc/passwd", "ssh://box/a.txt"] {
        let error = service.open(request(dir.path(), token)).await.expect_err("remote refused");
        assert_eq!(reason_of(&error), "RemoteUnsupported", "token {token}");
    }
    let nul = service.open(request(dir.path(), "a\0b.txt")).await.expect_err("nul refused");
    assert_eq!(nul.code, crate::ipc::error::IpcErrorCode::InvalidArgument);
}

#[tokio::test]
async fn text_bounds_report_too_large_without_truncating() {
    let dir = TempDir::new().expect("tempdir");
    let oversized = vec![b'a'; limits::TEXT_MAX_BYTES as usize + 1];
    write_file(dir.path(), "big.txt", &oversized);
    let mut many_lines = "x\n".repeat(limits::MAX_RENDERED_LINES);
    many_lines.push('x');
    write_file(dir.path(), "lines.txt", many_lines.as_bytes());
    write_file(dir.path(), "edge.txt", "x\n".repeat(limits::MAX_RENDERED_LINES).as_bytes());
    let service = service().await;

    let big = service.open(request(dir.path(), "big.txt")).await.expect_err("2 MiB + 1");
    assert_eq!(reason_of(&big), "TooLarge");
    let details = big.details.as_ref().expect("machine details");
    assert_eq!(details.get("byteLength").and_then(|v| v.as_u64()), Some(limits::TEXT_MAX_BYTES + 1));
    assert_eq!(details.get("limit").and_then(|v| v.as_u64()), Some(limits::TEXT_MAX_BYTES));

    let lines = service.open(request(dir.path(), "lines.txt")).await.expect_err("50_001 lines");
    assert_eq!(reason_of(&lines), "TooLarge");

    let edge = service.open(request(dir.path(), "edge.txt")).await.expect("exactly 50_000 lines");
    assert_eq!(edge.line_count, Some(limits::MAX_RENDERED_LINES));
}

#[tokio::test]
async fn undecodable_text_reports_unsupported_encoding() {
    let dir = TempDir::new().expect("tempdir");
    write_file(dir.path(), "latin.txt", &[0x68, 0xE9, 0x6C, 0x6C, 0x6F]);
    write_file(dir.path(), "binary.txt", &[0x00, 0x01, 0x02, 0x03]);
    let service = service().await;

    for name in ["latin.txt", "binary.txt"] {
        let error = service.open(request(dir.path(), name)).await.expect_err("undecodable");
        assert_eq!(reason_of(&error), "UnsupportedEncoding", "{name}");
    }
}

#[tokio::test]
async fn image_open_checks_signature_and_pixel_bounds_before_the_client_decodes() {
    let dir = TempDir::new().expect("tempdir");
    write_file(dir.path(), "ok.png", &png_bytes(6, 4));
    write_file(dir.path(), "lying.png", b"GIF87 no wait");
    write_file(dir.path(), "huge.png", &png_bytes(10_000, 4_001));
    let service = service().await;

    let payload = service.open(request(dir.path(), "ok.png")).await.expect("png payload");
    assert_eq!(payload.kind, FilePreviewKind::Image);
    assert_eq!(payload.media_type.as_deref(), Some("image/png"));
    assert_eq!(payload.text, None);
    assert_eq!(payload.encoding, None);
    let url = payload.media_url.clone().expect("capability url");
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");
    assert!(url.ends_with(&payload.handle), "{url}");
    assert!(!url.contains("ok.png"), "capability url must not expose the path: {url}");

    let lying = service.open(request(dir.path(), "lying.png")).await.expect_err("bad signature");
    assert_eq!(reason_of(&lying), "UnsupportedFormat");

    let huge = service.open(request(dir.path(), "huge.png")).await.expect_err("40 megapixels");
    assert_eq!(reason_of(&huge), "TooLarge");
    assert_eq!(
        huge.details.as_ref().and_then(|d| d.get("limit")).and_then(|v| v.as_u64()),
        Some(limits::IMAGE_MAX_PIXELS)
    );
}

#[tokio::test]
async fn video_open_exposes_a_streaming_capability_not_a_body() {
    let dir = TempDir::new().expect("tempdir");
    write_file(dir.path(), "clip.mp4", &mp4_bytes(4096));
    write_file(dir.path(), "clip.mkv", &mp4_bytes(64));
    let service = service().await;

    let payload = service.open(request(dir.path(), "clip.mp4")).await.expect("video payload");
    assert_eq!(payload.kind, FilePreviewKind::Video);
    assert_eq!(payload.media_type.as_deref(), Some("video/mp4"));
    assert_eq!(payload.byte_length, 4096);
    assert_eq!(payload.text, None);
    assert!(payload.media_url.is_some());

    let unknown = service.open(request(dir.path(), "clip.mkv")).await.expect_err("unknown container");
    assert_eq!(reason_of(&unknown), "UnsupportedFormat");
}

#[tokio::test]
async fn opening_b_revokes_a_for_the_same_window() {
    let dir = TempDir::new().expect("tempdir");
    write_file(dir.path(), "a.png", &png_bytes(2, 2));
    write_file(dir.path(), "b.png", &png_bytes(2, 2));
    let service = service().await;
    let client = http();

    let first = service.open(request(dir.path(), "a.png")).await.expect("a");
    let second = service.open(request(dir.path(), "b.png")).await.expect("b");
    assert_ne!(first.handle, second.handle);

    let stale = client.get(first.media_url.clone().unwrap()).send().await.expect("request a");
    assert_eq!(stale.status(), reqwest::StatusCode::NOT_FOUND);
    let live = client.get(second.media_url.clone().unwrap()).send().await.expect("request b");
    assert_eq!(live.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn close_is_idempotent_and_scoped_to_the_owning_window() {
    let dir = TempDir::new().expect("tempdir");
    write_file(dir.path(), "a.png", &png_bytes(2, 2));
    let service = service().await;
    let client = http();
    let payload = service.open(request(dir.path(), "a.png")).await.expect("payload");
    let url = payload.media_url.clone().unwrap();

    // another window cannot revoke this window's capability
    service.close("browser-6f1c", &payload.handle);
    assert_eq!(client.get(&url).send().await.expect("still live").status(), reqwest::StatusCode::OK);

    service.close(WINDOW, &payload.handle);
    service.close(WINDOW, &payload.handle);
    service.close(WINDOW, "deadbeef");
    assert_eq!(client.get(&url).send().await.expect("revoked").status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn destroying_a_window_revokes_every_capability_it_owns() {
    let dir = TempDir::new().expect("tempdir");
    write_file(dir.path(), "doc.md", b"![x](img.png)\n");
    write_file(dir.path(), "img.png", &png_bytes(2, 2));
    let service = service().await;
    let client = http();

    let parent = service.open(request(dir.path(), "doc.md")).await.expect("markdown");
    let child = service
        .open_child_image(WINDOW, &parent.handle, "img.png")
        .await
        .expect("child image");
    assert_eq!(client.get(&child.media_url).send().await.expect("live").status(), reqwest::StatusCode::OK);

    service.close_window(WINDOW);
    assert_eq!(
        client.get(&child.media_url).send().await.expect("revoked").status(),
        reqwest::StatusCode::NOT_FOUND
    );
}

// ------------------------------------------------- retained descriptor identity

#[cfg(unix)]
#[tokio::test]
async fn a_replaced_path_never_redirects_the_retained_handle() {
    let dir = TempDir::new().expect("tempdir");
    let real = write_file(dir.path(), "real.png", &png_bytes(2, 2));
    let secret = write_file(dir.path(), "secret.png", &png_bytes(9, 9));
    let link = dir.path().join("link.png");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");
    let service = service().await;
    let client = http();

    let payload = service.open(request(dir.path(), "link.png")).await.expect("via symlink");
    let url = payload.media_url.clone().unwrap();
    let original = std::fs::read(&real).expect("original bytes");
    assert_eq!(payload.byte_length, original.len() as u64);

    // repoint the symlink and delete/replace the original name
    std::fs::remove_file(&link).expect("unlink");
    std::os::unix::fs::symlink(&secret, &link).expect("relink");
    std::fs::remove_file(&real).expect("remove original name");

    let response = client.get(&url).send().await.expect("still served");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.bytes().await.expect("body").as_ref(), original.as_slice());
}

#[tokio::test]
async fn truncating_the_open_inode_requires_an_explicit_reload() {
    let dir = TempDir::new().expect("tempdir");
    let path = write_file(dir.path(), "clip.mp4", &mp4_bytes(8192));
    let service = service().await;
    let client = http();
    let payload = service.open(request(dir.path(), "clip.mp4")).await.expect("payload");
    let url = payload.media_url.clone().unwrap();

    std::fs::OpenOptions::new().write(true).open(&path).expect("open rw").set_len(16).expect("truncate");

    let response = client.get(&url).send().await.expect("changed");
    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);
    assert_eq!(response.headers().get("x-preview-reason").unwrap(), "FileChanged");
}

// ------------------------------------------------------------ HTTP semantics

#[tokio::test]
async fn full_get_returns_the_exact_body_and_frozen_headers() {
    let dir = TempDir::new().expect("tempdir");
    let bytes = png_bytes(3, 3);
    write_file(dir.path(), "a.png", &bytes);
    let service = service().await;
    let payload = service.open(request(dir.path(), "a.png")).await.expect("payload");
    let url = payload.media_url.clone().unwrap();

    let response = http().get(&url).send().await.expect("GET");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let headers = response.headers().clone();
    assert_eq!(headers.get("content-type").unwrap(), "image/png");
    assert_eq!(headers.get("content-length").unwrap(), bytes.len().to_string().as_str());
    assert_eq!(headers.get("accept-ranges").unwrap(), "bytes");
    assert_eq!(headers.get("cache-control").unwrap(), "no-store");
    assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
    assert!(headers.get("access-control-allow-origin").is_none(), "no CORS header without Origin");
    assert_eq!(response.bytes().await.expect("body").as_ref(), bytes.as_slice());
}

#[tokio::test]
async fn head_has_no_body_but_advertises_the_full_length() {
    let dir = TempDir::new().expect("tempdir");
    let bytes = png_bytes(3, 3);
    write_file(dir.path(), "a.png", &bytes);
    let service = service().await;
    let payload = service.open(request(dir.path(), "a.png")).await.expect("payload");

    let response = http().head(payload.media_url.clone().unwrap()).send().await.expect("HEAD");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers().get("content-length").unwrap(), bytes.len().to_string().as_str());
    assert_eq!(response.headers().get("accept-ranges").unwrap(), "bytes");
    assert!(response.bytes().await.expect("body").is_empty());
}

#[tokio::test]
async fn single_ranges_return_206_with_exact_bytes() {
    let dir = TempDir::new().expect("tempdir");
    let bytes = mp4_bytes(64);
    write_file(dir.path(), "clip.mp4", &bytes);
    let service = service().await;
    let client = http();
    let payload = service.open(request(dir.path(), "clip.mp4")).await.expect("payload");
    let url = payload.media_url.clone().unwrap();

    let response = client.get(&url).header("Range", "bytes=2-5").send().await.expect("range");
    assert_eq!(response.status(), reqwest::StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers().get("content-range").unwrap(), "bytes 2-5/64");
    assert_eq!(response.headers().get("content-length").unwrap(), "4");
    assert_eq!(response.bytes().await.expect("body").as_ref(), &bytes[2..=5]);

    let response = client.get(&url).header("Range", "bytes=-3").send().await.expect("suffix");
    assert_eq!(response.status(), reqwest::StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers().get("content-range").unwrap(), "bytes 61-63/64");
    assert_eq!(response.bytes().await.expect("body").as_ref(), &bytes[61..64]);

    let response = client.get(&url).header("Range", "bytes=60-").send().await.expect("open end");
    assert_eq!(response.status(), reqwest::StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers().get("content-range").unwrap(), "bytes 60-63/64");
    assert_eq!(response.bytes().await.expect("body").as_ref(), &bytes[60..64]);

    let response = client.head(&url).header("Range", "bytes=2-5").send().await.expect("range head");
    assert_eq!(response.status(), reqwest::StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers().get("content-range").unwrap(), "bytes 2-5/64");
    assert!(response.bytes().await.expect("body").is_empty());
}

#[tokio::test]
async fn unsatisfiable_and_multi_ranges_return_416_with_the_length() {
    let dir = TempDir::new().expect("tempdir");
    write_file(dir.path(), "clip.mp4", &mp4_bytes(64));
    let service = service().await;
    let client = http();
    let url = service
        .open(request(dir.path(), "clip.mp4"))
        .await
        .expect("payload")
        .media_url
        .unwrap();

    for raw in ["bytes=64-70", "bytes=0-1,4-5", "bytes=xyz", "bytes=5-2"] {
        let response = client.get(&url).header("Range", raw).send().await.expect("range");
        assert_eq!(
            response.status(),
            reqwest::StatusCode::RANGE_NOT_SATISFIABLE,
            "range {raw} must be 416"
        );
        assert_eq!(response.headers().get("content-range").unwrap(), "bytes */64");
        assert!(response.bytes().await.expect("body").is_empty());
    }
}

#[tokio::test]
async fn a_zero_length_file_serves_an_empty_200_and_refuses_ranges() {
    let dir = TempDir::new().expect("tempdir");
    write_file(dir.path(), "empty.mp4", b"");
    let service = service().await;
    let client = http();
    let payload = service.open(request(dir.path(), "empty.mp4")).await.expect("payload");
    let url = payload.media_url.clone().unwrap();
    assert_eq!(payload.byte_length, 0);

    let response = client.get(&url).send().await.expect("GET");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers().get("content-length").unwrap(), "0");
    assert!(response.bytes().await.expect("body").is_empty());

    let response = client.get(&url).header("Range", "bytes=0-").send().await.expect("range");
    assert_eq!(response.status(), reqwest::StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(response.headers().get("content-range").unwrap(), "bytes */0");
}

#[tokio::test]
async fn host_and_origin_are_checked_and_cors_is_never_wildcarded() {
    let dir = TempDir::new().expect("tempdir");
    write_file(dir.path(), "a.png", &png_bytes(2, 2));
    let service = service().await;
    let client = http();
    let url = service.open(request(dir.path(), "a.png")).await.expect("payload").media_url.unwrap();

    let forged = client.get(&url).header("Host", "evil.test").send().await.expect("forged host");
    assert_eq!(forged.status(), reqwest::StatusCode::FORBIDDEN);

    let hostile = client
        .get(&url)
        .header("Origin", "http://evil.test")
        .send()
        .await
        .expect("hostile origin");
    assert_eq!(hostile.status(), reqwest::StatusCode::FORBIDDEN);

    let allowed = client
        .get(&url)
        .header("Origin", "http://127.0.0.1:5173")
        .send()
        .await
        .expect("desktop origin");
    assert_eq!(allowed.status(), reqwest::StatusCode::OK);
    assert_eq!(
        allowed.headers().get("access-control-allow-origin").unwrap(),
        "http://127.0.0.1:5173"
    );

    // media GET without an Origin header stays allowed (native <video>/<img>)
    let bare = client.get(&url).send().await.expect("no origin");
    assert_eq!(bare.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn only_get_and_head_are_routed_and_unknown_handles_404() {
    let dir = TempDir::new().expect("tempdir");
    write_file(dir.path(), "a.png", &png_bytes(2, 2));
    let service = service().await;
    let client = http();
    let url = service.open(request(dir.path(), "a.png")).await.expect("payload").media_url.unwrap();

    for method in [reqwest::Method::POST, reqwest::Method::PUT, reqwest::Method::DELETE] {
        let response = client.request(method.clone(), &url).send().await.expect("method");
        assert_eq!(response.status(), reqwest::StatusCode::METHOD_NOT_ALLOWED, "{method}");
    }
    let unknown = format!("{}/{}", service.origin(), "0".repeat(64));
    assert_eq!(client.get(&unknown).send().await.expect("unknown").status(), reqwest::StatusCode::NOT_FOUND);
    // no directory listing at the capability root
    assert_eq!(
        client.get(service.origin()).send().await.expect("root").status(),
        reqwest::StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn concurrent_media_requests_are_capped_with_429() {
    let dir = TempDir::new().expect("tempdir");
    write_file(dir.path(), "a.png", &png_bytes(2, 2));
    let service = service().await;
    let client = http();
    let url = service.open(request(dir.path(), "a.png")).await.expect("payload").media_url.unwrap();

    let gate = service.media_gate();
    let held = gate
        .try_acquire_many(limits::MAX_CONCURRENT_MEDIA_REQUESTS as u32)
        .expect("all permits available");
    let overloaded = client.get(&url).send().await.expect("overload");
    assert_eq!(overloaded.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);

    drop(held);
    assert_eq!(client.get(&url).send().await.expect("recovered").status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn closing_mid_stream_stops_delivery_at_the_next_chunk() {
    use futures_util::StreamExt;

    let dir = TempDir::new().expect("tempdir");
    let total = limits::STREAM_CHUNK_BYTES * 128; // 8 MiB
    write_file(dir.path(), "clip.mp4", &mp4_bytes(total));
    let service = service().await;
    let payload = service.open(request(dir.path(), "clip.mp4")).await.expect("payload");

    let response = http()
        .get(payload.media_url.clone().unwrap())
        .send()
        .await
        .expect("GET");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let mut stream = response.bytes_stream();

    // subscribe to the first delivered chunk, then revoke: no sleeping
    let first = stream.next().await.expect("first chunk").expect("chunk bytes");
    assert!(!first.is_empty());
    service.close(WINDOW, &payload.handle);

    let mut delivered = first.len();
    let mut aborted = false;
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(bytes) => delivered += bytes.len(),
            Err(_) => {
                aborted = true;
                break;
            }
        }
    }
    assert!(delivered < total, "delivery must stop early, got {delivered} of {total}");
    assert!(aborted, "an incomplete body must surface as a transport error, not a silent success");
    // the permit is released, so the service keeps serving other work
    assert_eq!(service.media_gate().available_permits(), limits::MAX_CONCURRENT_MEDIA_REQUESTS);
}

// --------------------------------------------------- Markdown child boundary

#[tokio::test]
async fn markdown_children_resolve_only_under_the_document_directory() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("docs/assets")).expect("mkdir");
    write_file(dir.path(), "docs/guide.md", b"![x](assets/logo.png)\n");
    write_file(dir.path(), "docs/assets/logo.png", &png_bytes(5, 5));
    write_file(dir.path(), "outside.png", &png_bytes(5, 5));
    let service = service().await;
    let client = http();

    let parent = service.open(request(dir.path(), "docs/guide.md")).await.expect("markdown");
    assert_eq!(parent.kind, FilePreviewKind::Markdown);

    let child = service
        .open_child_image(WINDOW, &parent.handle, "assets/logo.png")
        .await
        .expect("contained child image");
    assert_eq!(child.kind, FilePreviewKind::Image);
    assert_eq!(child.media_type, "image/png");
    assert_eq!(child.display_name, "logo.png");
    assert_eq!(child.byte_length, png_bytes(5, 5).len() as u64);
    assert_eq!(
        client.get(&child.media_url).send().await.expect("child GET").status(),
        reqwest::StatusCode::OK
    );

    for escape in ["../outside.png", "/etc/hosts", "assets/../../outside.png", "https://evil.test/a.png"] {
        let error = service
            .open_child_image(WINDOW, &parent.handle, escape)
            .await
            .expect_err("escape refused");
        assert!(
            matches!(reason_of(&error).as_str(), "PermissionDenied" | "RemoteUnsupported"),
            "{escape} gave {}",
            reason_of(&error)
        );
    }

    // non-image children are refused by the child endpoint
    write_file(dir.path(), "docs/other.md", b"# other\n");
    let wrong_kind = service
        .open_child_image(WINDOW, &parent.handle, "other.md")
        .await
        .expect_err("markdown is not an image asset");
    assert_eq!(reason_of(&wrong_kind), "UnsupportedFormat");

    // a text handle owns no child boundary at all
    write_file(dir.path(), "plain.txt", b"hi");
    let plain = service.open(request(dir.path(), "plain.txt")).await.expect("text");
    let refused = service
        .open_child_image(WINDOW, &plain.handle, "logo.png")
        .await
        .expect_err("text parent");
    assert_eq!(reason_of(&refused), "PermissionDenied");
}

#[cfg(unix)]
#[tokio::test]
async fn a_child_symlink_escaping_the_document_directory_is_refused() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("docs")).expect("mkdir");
    write_file(dir.path(), "docs/guide.md", b"# g\n");
    let outside = write_file(dir.path(), "secret.png", &png_bytes(2, 2));
    std::os::unix::fs::symlink(&outside, dir.path().join("docs/evil.png")).expect("symlink");
    let service = service().await;

    let parent = service.open(request(dir.path(), "docs/guide.md")).await.expect("markdown");
    let error = service
        .open_child_image(WINDOW, &parent.handle, "evil.png")
        .await
        .expect_err("symlink escape refused");
    assert_eq!(reason_of(&error), "PermissionDenied");
}

#[tokio::test]
async fn child_handles_are_capped_per_document() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("docs")).expect("mkdir");
    write_file(dir.path(), "docs/guide.md", b"# g\n");
    for index in 0..=limits::MAX_CHILD_HANDLES {
        write_file(dir.path(), &format!("docs/img{index}.png"), &png_bytes(2, 2));
    }
    let service = service().await;
    let parent = service.open(request(dir.path(), "docs/guide.md")).await.expect("markdown");

    for index in 0..limits::MAX_CHILD_HANDLES {
        service
            .open_child_image(WINDOW, &parent.handle, &format!("img{index}.png"))
            .await
            .unwrap_or_else(|error| panic!("child {index} must be granted: {error:?}"));
    }
    let error = service
        .open_child_image(WINDOW, &parent.handle, &format!("img{}.png", limits::MAX_CHILD_HANDLES))
        .await
        .expect_err("cap enforced");
    assert_eq!(reason_of(&error), "TooLarge");
    assert_eq!(
        error.details.as_ref().and_then(|d| d.get("limit")).and_then(|v| v.as_u64()),
        Some(limits::MAX_CHILD_HANDLES as u64)
    );
}

#[tokio::test]
async fn markdown_document_links_navigate_only_inside_the_parent_boundary() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("docs/deep")).expect("mkdir");
    write_file(dir.path(), "docs/guide.md", b"[next](deep/next.md)\n");
    write_file(dir.path(), "docs/deep/next.md", "# next\n다음\n".as_bytes());
    write_file(dir.path(), "outside.md", b"# outside\n");
    let service = service().await;

    let parent = service.open(request(dir.path(), "docs/guide.md")).await.expect("markdown");
    let navigated = service
        .open_child_document(WINDOW, &parent.handle, "deep/next.md")
        .await
        .expect("contained document");
    assert_eq!(navigated.kind, FilePreviewKind::Markdown);
    assert_eq!(navigated.display_name, "next.md");
    assert_eq!(navigated.text.as_deref(), Some("# next\n다음\n"));
    assert_ne!(navigated.handle, parent.handle);

    // the navigated document becomes the window's main handle and revokes the parent
    let stale = service
        .open_child_image(WINDOW, &parent.handle, "logo.png")
        .await
        .expect_err("old parent revoked");
    assert_eq!(reason_of(&stale), "ExpiredHandle");

    // and its own children resolve under the NEW document directory
    write_file(dir.path(), "docs/deep/logo.png", &png_bytes(2, 2));
    service
        .open_child_document(WINDOW, &navigated.handle, "../guide.md")
        .await
        .expect_err("traversal above the new document directory is refused");

    for escape in ["../../outside.md", "/etc/hosts"] {
        let error = service
            .open_child_document(WINDOW, &navigated.handle, escape)
            .await
            .expect_err("escape refused");
        assert_eq!(reason_of(&error), "PermissionDenied", "{escape}");
    }
}

#[tokio::test]
async fn expired_handles_report_the_machine_reason() {
    let dir = TempDir::new().expect("tempdir");
    write_file(dir.path(), "docs.md", b"# d\n");
    let service = service().await;
    let parent = service.open(request(dir.path(), "docs.md")).await.expect("markdown");
    service.close(WINDOW, &parent.handle);

    let error = service
        .open_child_image(WINDOW, &parent.handle, "logo.png")
        .await
        .expect_err("revoked parent");
    assert_eq!(reason_of(&error), "ExpiredHandle");
    assert_eq!(error.code, crate::ipc::error::IpcErrorCode::InvalidArgument);
}

#[tokio::test]
async fn a_refused_session_resolution_maps_to_remote_unsupported() {
    let refusal = crate::ipc::error::IpcError::new(
        crate::ipc::error::IpcErrorCode::Unsupported,
        "refusing to open a remote path on this machine (paired-host session)",
    );
    assert_eq!(reason_of(&map_session_error(refusal)), "RemoteUnsupported");

    let missing = crate::ipc::error::IpcError::new(
        crate::ipc::error::IpcErrorCode::SessionNotFound,
        "session not found",
    );
    let mapped = map_session_error(missing);
    assert_eq!(mapped.code, crate::ipc::error::IpcErrorCode::SessionNotFound);
}
