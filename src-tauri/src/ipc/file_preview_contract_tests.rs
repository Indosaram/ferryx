//! Wire-shape tests for the shared preview contract.
//!
//! These assert machine-consumed values only (JSON keys, discriminant spellings,
//! error envelope fields) — the exact surface `ui/src/lib/filePreviewTypes.ts`
//! compiles against.
use super::*;
use serde_json::json;

fn sample_payload() -> FilePreviewPayload {
    FilePreviewPayload {
        handle: "h-1".into(),
        display_name: "notes.md".into(),
        kind: FilePreviewKind::Markdown,
        byte_length: 12,
        encoding: Some(FilePreviewEncoding::Utf8),
        media_type: None,
        media_url: None,
        text: Some("# 안녕".into()),
        line_count: Some(1),
        target: Some(FilePreviewTarget { line: 3, col: Some(7) }),
    }
}

#[test]
fn payload_serializes_camel_case_keys() {
    let value = serde_json::to_value(sample_payload()).expect("payload serializes");
    let object = value.as_object().expect("payload is a JSON object");
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "byteLength",
            "displayName",
            "encoding",
            "handle",
            "kind",
            "lineCount",
            "mediaType",
            "mediaUrl",
            "target",
            "text",
        ]
    );
    assert_eq!(object["kind"], json!("markdown"));
    assert_eq!(object["encoding"], json!("utf-8"));
    assert_eq!(object["target"], json!({ "line": 3, "col": 7 }));
}

#[test]
fn child_asset_serializes_camel_case_keys() {
    let value = serde_json::to_value(FilePreviewChildAsset {
        handle: "h-2".into(),
        display_name: "diagram.png".into(),
        kind: FilePreviewKind::Image,
        byte_length: 40,
        media_type: "image/png".into(),
        media_url: "http://127.0.0.1:1/abc".into(),
    })
    .expect("child asset serializes");
    let object = value.as_object().expect("child asset is a JSON object");
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["byteLength", "displayName", "handle", "kind", "mediaType", "mediaUrl"]
    );
    assert_eq!(object["kind"], json!("image"));
}

#[test]
fn payload_round_trips_through_the_wire_shape() {
    let payload = sample_payload();
    let encoded = serde_json::to_string(&payload).expect("serializes");
    let decoded: FilePreviewPayload = serde_json::from_str(&encoded).expect("deserializes");
    assert_eq!(decoded, payload);
}

#[test]
fn every_kind_has_a_lowercase_wire_value() {
    let values: Vec<serde_json::Value> = [
        FilePreviewKind::Text,
        FilePreviewKind::Markdown,
        FilePreviewKind::Image,
        FilePreviewKind::Video,
    ]
    .into_iter()
    .map(|kind| serde_json::to_value(kind).expect("kind serializes"))
    .collect();
    assert_eq!(
        values,
        vec![json!("text"), json!("markdown"), json!("image"), json!("video")]
    );
}

#[test]
fn reason_wire_spellings_match_as_str() {
    let reasons = [
        FilePreviewErrorReason::MissingFile,
        FilePreviewErrorReason::PermissionDenied,
        FilePreviewErrorReason::NotRegularFile,
        FilePreviewErrorReason::RemoteUnsupported,
        FilePreviewErrorReason::TooLarge,
        FilePreviewErrorReason::UnsupportedEncoding,
        FilePreviewErrorReason::UnsupportedFormat,
        FilePreviewErrorReason::FileChanged,
        FilePreviewErrorReason::ExpiredHandle,
    ];
    for reason in reasons {
        assert_eq!(
            serde_json::to_value(reason).expect("reason serializes"),
            json!(reason.as_str())
        );
    }
    assert_eq!(reasons.len(), 9);
}

#[test]
fn preview_error_carries_machine_reason_and_extra_details() {
    let error = preview_error(
        FilePreviewErrorReason::TooLarge,
        "the file is larger than the preview limit",
        Some(json!({ "byteLength": 3_000_000u64, "limit": limits::TEXT_MAX_BYTES })),
    );
    assert_eq!(error.code, IpcErrorCode::Unsupported);
    let details = error.details.expect("details present");
    assert_eq!(details["reason"], json!("TooLarge"));
    assert_eq!(details["byteLength"], json!(3_000_000u64));
    assert_eq!(details["limit"], json!(2_097_152u64));

    let plain = preview_error(FilePreviewErrorReason::ExpiredHandle, "handle expired", None);
    assert_eq!(plain.code, IpcErrorCode::InvalidArgument);
    assert_eq!(
        plain.details.expect("details present"),
        json!({ "reason": "ExpiredHandle" })
    );
}

#[test]
fn frozen_limits_match_the_plan() {
    assert_eq!(limits::TEXT_MAX_BYTES, 2 * 1024 * 1024);
    assert_eq!(limits::MAX_RENDERED_LINES, 50_000);
    assert_eq!(limits::IMAGE_MAX_BYTES, 32 * 1024 * 1024);
    assert_eq!(limits::IMAGE_MAX_PIXELS, 40_000_000);
    assert_eq!(limits::MAX_CHILD_HANDLES, 32);
    assert_eq!(limits::MAX_CONCURRENT_MEDIA_REQUESTS, 8);
    assert_eq!(limits::STREAM_CHUNK_BYTES, 64 * 1024);
}
