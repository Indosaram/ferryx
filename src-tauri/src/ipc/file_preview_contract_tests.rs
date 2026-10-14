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
        target: Some(FilePreviewTarget {
            line: 3,
            col: Some(7),
        }),
        resolved_path: Some("/tmp/notes.md".into()),
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
            "resolvedPath",
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
        vec![
            "byteLength",
            "displayName",
            "handle",
            "kind",
            "mediaType",
            "mediaUrl"
        ]
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
        FilePreviewKind::Audio,
        FilePreviewKind::Pdf,
    ]
    .into_iter()
    .map(|kind| serde_json::to_value(kind).expect("kind serializes"))
    .collect();
    assert_eq!(
        values,
        vec![
            json!("text"),
            json!("markdown"),
            json!("image"),
            json!("video"),
            json!("audio"),
            json!("pdf"),
        ]
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

    let plain = preview_error(
        FilePreviewErrorReason::ExpiredHandle,
        "handle expired",
        None,
    );
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

// --- admit_session_cwd: session-failure policy for file-link opens ---

/// A local (non-SSH, non-paired) workspace id as the UI sends it.
const LOCAL_WS: &str = "ws-local-1";
const SSH_WS: &str = "ssh:0123abcd";
const PAIRED_WS: &str = "daemon:host-1/ws";

fn session_not_found() -> IpcError {
    IpcError::new(IpcErrorCode::SessionNotFound, "terminal session 'cli' is not available")
}

fn ssh_refusal() -> IpcError {
    IpcError::new(IpcErrorCode::Unsupported, "session belongs to an SSH workspace")
        .with_details(json!({ "kind": "ssh" }))
}

fn paired_refusal() -> IpcError {
    IpcError::new(IpcErrorCode::Unsupported, "session is relayed from a paired host")
        .with_details(json!({ "kind": "pairedHost" }))
}

struct RemoteCache {
    _dir: tempfile::TempDir,
    cache_root: PathBuf,
    fetched: PathBuf,
    outside: PathBuf,
}

fn remote_cache() -> RemoteCache {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache_root = dir.path().join("cache");
    let fetched = crate::ipc::remote_file::remote_cache_path(&cache_root, "ssh:ws", "/srv/a.png");
    std::fs::create_dir_all(fetched.parent().unwrap()).unwrap();
    std::fs::write(&fetched, b"png").unwrap();
    let outside = dir.path().join("elsewhere.png");
    std::fs::write(&outside, b"png").unwrap();
    RemoteCache {
        _dir: dir,
        cache_root,
        fetched,
        outside,
    }
}

#[test]
fn local_session_keeps_its_live_cwd() {
    let cwd = PathBuf::from("/work/repo");
    let admitted =
        admit_session_cwd("src/a.rs", Ok(Some(cwd.clone())), Some(LOCAL_WS), None, None).unwrap();
    assert_eq!(admitted, Some(cwd));
}

#[test]
fn unknown_session_in_a_local_workspace_does_not_block_an_absolute_path() {
    let admitted = admit_session_cwd(
        "/tmp/report.png",
        Err(session_not_found()),
        Some(LOCAL_WS),
        None,
        None,
    )
    .expect("absolute path must open without a live session in a local workspace");
    assert_eq!(admitted, None);
}

#[test]
fn unknown_session_in_a_local_workspace_does_not_block_a_home_rooted_path() {
    // ~ and ~/x resolve against the home directory, so a dead local shell's cwd is irrelevant.
    for path in ["~", "~/shots/run.png"] {
        let admitted = admit_session_cwd(path, Err(session_not_found()), Some(LOCAL_WS), None, None)
            .unwrap_or_else(|error| panic!("{path} must open without a live session: {error:?}"));
        assert_eq!(admitted, None, "{path}");
    }
    // Remote workspaces and a missing workspace id still refuse it.
    assert!(admit_session_cwd("~/x", Err(session_not_found()), Some(SSH_WS), None, None).is_err());
    assert!(admit_session_cwd("~/x", Err(session_not_found()), None, None, None).is_err());
    // "~user" is not home-rooted for this process and stays a relative token.
    assert!(admit_session_cwd("~other/x", Err(session_not_found()), Some(LOCAL_WS), None, None).is_err());
}

#[test]
fn unknown_session_in_a_local_workspace_relative_token_falls_back_to_caller_cwd_or_refuses() {
    let fallback = PathBuf::from("/work/repo");
    let admitted = admit_session_cwd(
        "src/a.rs",
        Err(session_not_found()),
        Some(LOCAL_WS),
        Some(fallback.clone()),
        None,
    )
    .unwrap();
    assert_eq!(admitted, Some(fallback));

    let error =
        admit_session_cwd("src/a.rs", Err(session_not_found()), Some(LOCAL_WS), None, None)
            .expect_err("relative token without any cwd must stay an error");
    assert_eq!(error.code, IpcErrorCode::SessionNotFound);
}

#[test]
fn remote_sessions_open_only_fetched_cache_files() {
    let cache = remote_cache();
    let root = crate::ipc::remote_file::remote_files_root(&cache.cache_root);
    let fetched = cache.fetched.to_string_lossy().into_owned();

    for refusal in [ssh_refusal, paired_refusal] {
        let admitted = admit_session_cwd(&fetched, Err(refusal()), None, None, Some(&root))
            .expect("a fetched cache file opens as a local file");
        assert_eq!(admitted, None);
    }
}

#[test]
fn remote_sessions_refuse_absolute_paths_outside_the_cache() {
    let cache = remote_cache();
    let root = crate::ipc::remote_file::remote_files_root(&cache.cache_root);
    let outside = cache.outside.to_string_lossy().into_owned();
    let escape = format!("{}/../../elsewhere.png", cache.fetched.parent().unwrap().display());
    let root_itself = root.to_string_lossy().into_owned();

    for refusal in [ssh_refusal, paired_refusal] {
        for path in [
            "/srv/app/x.png",
            outside.as_str(),
            escape.as_str(),
            root_itself.as_str(),
        ] {
            let error = admit_session_cwd(path, Err(refusal()), None, None, Some(&root))
                .expect_err("remote-printed absolute paths must not open locally");
            assert_eq!(error.code, IpcErrorCode::Unsupported, "{path}");
        }
        let error = admit_session_cwd(
            &cache.fetched.to_string_lossy(),
            Err(refusal()),
            None,
            None,
            None,
        )
        .expect_err("no cache root means no remote exception");
        assert_eq!(error.code, IpcErrorCode::Unsupported);
    }
}

#[test]
fn remote_sessions_refuse_relative_tokens_even_with_caller_cwd() {
    let cache = remote_cache();
    let root = crate::ipc::remote_file::remote_files_root(&cache.cache_root);
    for refusal in [ssh_refusal, paired_refusal] {
        let error = admit_session_cwd(
            "src/a.rs",
            Err(refusal()),
            None,
            Some(PathBuf::from("/work/repo")),
            Some(&root),
        )
        .expect_err("relative remote tokens must not resolve locally");
        assert_eq!(error.code, IpcErrorCode::Unsupported);
    }
}

// --- RS-F1: a failed lookup must never reopen a remote path from the local disk ---

#[test]
fn remote_workspace_session_whose_lookup_failed_refuses_local_paths() {
    for workspace in [SSH_WS, PAIRED_WS] {
        for (path, fallback) in [
            ("/etc/hosts", None),
            ("/etc/hosts", Some(PathBuf::from("/work/repo"))),
            ("src/a.rs", Some(PathBuf::from("/work/repo"))),
        ] {
            let error = admit_session_cwd(
                path,
                Err(session_not_found()),
                Some(workspace),
                fallback,
                None,
            )
            .expect_err("a remote-printed path must not open from the local disk");
            assert_eq!(error.code, IpcErrorCode::Unsupported, "{workspace} {path}");
        }
    }
}

#[test]
fn remote_workspace_refuses_even_a_successful_local_lookup() {
    for workspace in [SSH_WS, PAIRED_WS] {
        let error = admit_session_cwd(
            "src/a.rs",
            Ok(Some(PathBuf::from("/work/repo"))),
            Some(workspace),
            None,
            None,
        )
        .expect_err("a remote workspace never resolves against a local cwd");
        assert_eq!(error.code, IpcErrorCode::Unsupported, "{workspace}");
    }
}

#[test]
fn unknown_session_without_a_workspace_id_refuses() {
    for (path, fallback) in [
        ("/etc/hosts", None),
        ("src/a.rs", Some(PathBuf::from("/work/repo"))),
    ] {
        let error = admit_session_cwd(path, Err(session_not_found()), None, fallback, None)
            .expect_err("a session id without a workspace id cannot prove the session is local");
        assert_eq!(error.code, IpcErrorCode::SessionNotFound, "{path}");
    }
}

#[test]
fn fetched_cache_file_is_admitted_whatever_the_workspace_or_lookup() {
    let cache = remote_cache();
    let root = crate::ipc::remote_file::remote_files_root(&cache.cache_root);
    let fetched = cache.fetched.to_string_lossy().into_owned();
    for workspace in [None, Some(SSH_WS), Some(PAIRED_WS)] {
        for session in [
            Err(session_not_found()),
            Err(ssh_refusal()),
            Ok(Some(PathBuf::from("/work/repo"))),
        ] {
            let local_lookup = session.is_ok();
            let admitted = admit_session_cwd(&fetched, session, workspace, None, Some(&root))
                .expect("a fetched remote-files cache file opens locally");
            if !local_lookup {
                assert_eq!(admitted, None, "{workspace:?}");
            }
        }
    }
}

#[test]
fn admission_reports_when_only_the_fetched_cache_admitted_the_path() {
    let cache = remote_cache();
    let root = crate::ipc::remote_file::remote_files_root(&cache.cache_root);
    let fetched = cache.fetched.to_string_lossy().into_owned();
    for (workspace, session) in [
        (Some(SSH_WS), Err(session_not_found())),
        (Some(PAIRED_WS), Ok(Some(PathBuf::from("/work/repo")))),
        (None, Err(ssh_refusal())),
        (Some(LOCAL_WS), Err(paired_refusal())),
    ] {
        let admitted =
            admit_session_cwd_admission(&fetched, session, workspace, None, Some(&root)).unwrap();
        assert_eq!(admitted, SessionCwdAdmission::FetchedRemoteFile, "{workspace:?}");
    }

    let local =
        admit_session_cwd_admission("/tmp/report.png", Err(session_not_found()), Some(LOCAL_WS), None, Some(&root))
            .unwrap();
    assert_eq!(local, SessionCwdAdmission::Cwd(None));
    let live = PathBuf::from("/work/repo");
    let local_cwd =
        admit_session_cwd_admission("src/a.rs", Ok(Some(live.clone())), Some(LOCAL_WS), None, Some(&root))
            .unwrap();
    assert_eq!(local_cwd, SessionCwdAdmission::Cwd(Some(live)));
}

#[test]
fn interpreter_run_script_types_are_not_passive_documents() {
    for name in [
        "evil.py", "EVIL.PY", "x.rb", "x.lua", "x.jsx", "x.mjs", "x.cjs", "x.r", "x.ex", "x.exs",
        "x.hs", "x.scala", "x.kt", "x.swift", "x.xml", "x.bat", "x.command", "x.terminal",
        "x.exe", "x.png.exe", "x.jar", "x.svg", "noext",
    ] {
        assert!(!is_passive_document(Path::new(name)), "{name} must not be launchable");
    }
    for name in ["a.png", "A.PNG", "a.pdf", "a.md", "a.txt", "a.log", "a.json", "a.mp4", "a.mp3", "a.csv"] {
        assert!(is_passive_document(Path::new(name)), "{name} stays launchable");
    }
}
