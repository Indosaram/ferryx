//! The desktop chat lens over one pane. The daemon owns the panes and the reference-chat input
//! queue. Reads reuse its connection; mutations use an independent one and need no network listener.

use std::sync::Arc;

use tauri::State;

use crate::daemon::protocol::{
    DaemonRequest, DaemonResponse, DesktopReferenceChatIdentity, DesktopReferenceChatReply,
    DesktopReferenceChatRequest,
};
use crate::daemon::DaemonClient;
use crate::ipc::{IpcError, IpcErrorCode};
use crate::remote::reference_chat::files as reference_files;

fn daemon_failure(response: DaemonResponse) -> IpcError {
    match response {
        DaemonResponse::Error { message, code: Some(code), details } => {
            let error = IpcError::new(IpcErrorCode::from_code_str(&code), message);
            match details {
                Some(details) => error.with_details(details),
                None => error,
            }
        }
        DaemonResponse::Error { message, .. } => IpcError::internal(message),
        _ => IpcError::internal("Unexpected daemon reference-chat response"),
    }
}

pub(crate) fn identity_from(response: DaemonResponse) -> Result<DesktopReferenceChatIdentity, IpcError> {
    match response {
        DaemonResponse::ReferenceChatIdentityOk { identity } => Ok(identity),
        other => Err(daemon_failure(other)),
    }
}

pub(crate) fn reply_from(response: DaemonResponse) -> Result<DesktopReferenceChatReply, IpcError> {
    match response {
        DaemonResponse::ReferenceChatOk { reply } => Ok(reply),
        other => Err(daemon_failure(other)),
    }
}

#[tauri::command]
pub async fn cmd_desktop_reference_chat_identity(
    daemon: State<'_, Arc<DaemonClient>>,
) -> Result<DesktopReferenceChatIdentity, IpcError> {
    identity_from(daemon.send_request_one_shot(DaemonRequest::ReferenceChatIdentity).await?)
}

#[tauri::command]
pub async fn cmd_desktop_reference_chat(
    daemon: State<'_, Arc<DaemonClient>>,
    request: DesktopReferenceChatRequest,
) -> Result<DesktopReferenceChatReply, IpcError> {
    let is_read = request.kind.is_read();
    let request = DaemonRequest::ReferenceChat { request };
    reply_from(if is_read {
        daemon.send_request(request).await?
    } else {
        daemon.send_request_one_shot(request).await?
    })
}

/// One file the desktop chat lens stages on the pane's own host.
///
/// The type mirrors the reply the HTTP `POST /api/v1/reference-chat/{sessionId}/files` route
/// answers, so the GUI maps both transports through one path: a receipt, its display name and
/// the editable `@path ` mention the submitted text carries. Bytes never reach the browser as
/// a path the agent could not open — they are written into the pane's own working directory.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopChatFileReply {
    pub status: u16,
    pub body: serde_json::Value,
}

fn file_failure(status: u16, code: &str, message: impl Into<String>) -> DesktopChatFileReply {
    let message: String = message.into();
    DesktopChatFileReply {
        status,
        body: serde_json::json!({
            "error": { "code": code, "message": message, "retryable": false }
        }),
    }
}

/// The HTTP status and wire code a staging refusal reports, in the route's own vocabulary.
fn file_refusal(code: crate::scoped_contracts::ScopeErrorCode) -> (u16, &'static str) {
    use crate::scoped_contracts::ScopeErrorCode as Code;
    match code {
        Code::InvalidRequest => (400, "INVALID_REQUEST"),
        Code::Unauthorized => (401, "UNAUTHORIZED"),
        Code::Forbidden => (403, "FORBIDDEN"),
        Code::NotFound => (404, "NOT_FOUND"),
        Code::TargetExpired => (410, "TARGET_EXPIRED"),
        Code::PayloadTooLarge => (413, "PAYLOAD_TOO_LARGE"),
        Code::ControlConflict | Code::RequestConflict | Code::OperationOutcomeUnknown => {
            (409, "CONTROL_CONFLICT")
        }
        Code::InventoryIncomplete => (503, "INVENTORY_INCOMPLETE"),
        Code::Timeout => (504, "TIMEOUT"),
        Code::CaptureUnsupported => (422, "CAPTURE_UNSUPPORTED"),
        Code::Unsupported | Code::ProviderOwned => (422, "UNSUPPORTED"),
    }
}

/// The directory a described pane stages into, or the refusal that pane earns.
///
/// Pure so the paired-host rule is testable without a live daemon: it is the one decision here
/// that can put bytes on the wrong computer. A pane the local daemon merely proxies for a paired
/// host is refused, because its `cwd` names a directory that exists on THAT machine — writing
/// here would stage the bytes on the wrong computer under a mention the pane's agent could never
/// open. Those files belong to the paired host's own file routes.
fn pane_directory(
    session: &crate::daemon::protocol::DaemonSessionDetails,
) -> Result<std::path::PathBuf, DesktopChatFileReply> {
    let paired = session.is_paired_proxy_actor()
        || session
            .workspace_id
            .as_deref()
            .is_some_and(|workspace| workspace.starts_with("daemon:"));
    if paired {
        return Err(file_failure(
            422,
            "UNSUPPORTED",
            "this pane lives on a paired host; its files are staged by that host's daemon",
        ));
    }
    session
        .cwd
        .as_deref()
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_dir())
        .ok_or_else(|| {
            file_failure(
                409,
                "UNSUPPORTED",
                "this pane has no working directory a staged file could be opened from",
            )
        })
}

/// The working directory a staged file must resolve from: the pane's live cwd, as the daemon
/// that owns the pane reports it. There is deliberately no fallback to the worktree the pane
/// was spawned in — the mention this produces has to resolve from the directory the agent
/// actually runs in, and a worktree identity carries no path to fall back to.
///
/// The HTTP route prefers the foreground process group's cwd; the desktop bridge asks the
/// daemon instead of reading another process's kernel state, which is the same directory in
/// every case the route can read and never requires the daemon and GUI to share a machine.
async fn pane_worktree(
    daemon: &Arc<DaemonClient>,
    session_id: &str,
) -> Result<std::path::PathBuf, DesktopChatFileReply> {
    let described = daemon
        .send_request(DaemonRequest::DescribeSession {
            session_id: session_id.to_string(),
        })
        .await
        .map_err(|error| {
            file_failure(
                503,
                "HOST_UNAVAILABLE",
                format!("the daemon that owns this pane did not answer: {error}"),
            )
        })?;
    match described {
        DaemonResponse::DescribeSessionOk { session, .. } => pane_directory(&session),
        DaemonResponse::Error { message, code, .. } => Err(file_failure(
            409,
            code.as_deref().unwrap_or("UNSUPPORTED"),
            message,
        )),
        other => Err(file_failure(
            500,
            "INTERNAL_ERROR",
            format!("unexpected daemon describe response: {other:?}"),
        )),
    }
}

/// Stage one file for a pane from the desktop lens.
///
/// `params` is the frozen `ReferenceFileStagePayload` (`{name, mediaType, sizeBytes,
/// contentBase64}`), parsed here so the wire shape stays the one the route and the web client
/// already use. The write and its bounds checks are the route's own
/// (`reference_files::stage_reference_file_in`), so a file the route would refuse is refused
/// here with the same code.
///
/// Two of the route's rules are about the TURN rather than the one file, and the desktop lens
/// applies them here because it has no in-memory staged registry to fold: the staged set already
/// on disk under this pane's staging base is counted against `reference_file_bounds_check`, and
/// the base is recorded in the gateway's ledger so the 24 h sweep reaches files staged from the
/// desktop even when no route ever touches this pane (`server.rs` stage route, `files.rs`).
#[tauri::command]
pub async fn cmd_desktop_reference_chat_file_stage(
    daemon: State<'_, Arc<DaemonClient>>,
    session_id: String,
    params: serde_json::Value,
) -> Result<DesktopChatFileReply, IpcError> {
    let payload = match serde_json::from_value::<
        crate::remote::reference_chat::types::ReferenceFileStagePayload,
    >(params)
    {
        Ok(payload) => payload,
        Err(error) => {
            return Ok(file_failure(
                400,
                "INVALID_REQUEST",
                format!("the stage params are not readable: {error}"),
            ))
        }
    };
    let worktree = match pane_worktree(&daemon, &session_id).await {
        Ok(worktree) => worktree,
        Err(reply) => return Ok(reply),
    };
    let ledger = reference_staging_ledger();
    let host_id = reference_files::reference_host_id();
    let staged = crate::ipc::run_blocking(move || {
        Ok::<_, crate::ipc::error::IpcError>(stage_with_turn_bounds(&host_id, &worktree, &payload, ledger.as_deref()))
    })
    .await;
    match staged {
        Ok(Ok(receipt)) => Ok(DesktopChatFileReply {
            status: 200,
            body: serde_json::json!({
                "receipt": receipt.receipt,
                "displayName": receipt.display_name,
                "mentionText": receipt.mention_text,
            }),
        }),
        Ok(Err(code)) => {
            let (status, wire) = file_refusal(code);
            Ok(file_failure(
                status,
                wire,
                format!("the file could not be staged on this host: {code:?}"),
            ))
        }
        Err(error) => Ok(file_failure(
            500,
            "INTERNAL_ERROR",
            format!("the staging thread did not finish: {error}"),
        )),
    }
}

/// The gateway's staging-base ledger, in the same owner-private data directory the remote
/// gateway records into, so one sweep reaches files staged by either surface. `None` when this
/// machine keeps no remote state on disk; the stage then proceeds unrecorded, as the route does.
fn reference_staging_ledger() -> Option<std::path::PathBuf> {
    crate::remote::auth::canonical_identity_dir()
        .ok()
        .map(|dir| dir.join(reference_files::REFERENCE_STAGING_LEDGER_NAME))
}

/// Stage one payload under the turn's bounds, then record its base for the sweep.
///
/// The per-turn bounds are the frozen scoped ones the route applies (`4` files / `20 MB` per
/// turn); the set they are applied against is what this pane's staging base holds right now,
/// minus the directories already older than the unreferenced TTL (those are swept, and counting
/// them would let a stale file block a live one).
fn stage_with_turn_bounds(
    host_id: &str,
    worktree: &std::path::Path,
    payload: &crate::remote::reference_chat::types::ReferenceFileStagePayload,
    ledger: Option<&std::path::Path>,
) -> Result<crate::remote::reference_chat::types::ReferenceFileReceipt, crate::scoped_contracts::ScopeErrorCode> {
    let ttl_ms = reference_files::reference_unreferenced_ttl_ms();
    let base = reference_files::reference_staging_base_dir(worktree);
    let (existing_files, existing_turn_bytes) = staged_turn_usage(&base, ttl_ms);
    reference_files::reference_file_bounds_check(payload.size_bytes, existing_files, existing_turn_bytes)?;
    let receipt = reference_files::stage_reference_file_in(host_id, worktree, payload)?;
    if let Some(ledger) = ledger {
        if let Err(error) = reference_files::reference_record_staging_base(ledger, &base) {
            // A stage that succeeded must not fail because its sweep record could not be written.
            tracing::warn!("desktop reference chat: staging base not recorded for the sweep: {error}");
        }
    }
    Ok(receipt)
}

/// The live staged set one staging base holds: its attachment directories (canonical UUID names,
/// never a planted entry) that are not yet older than the TTL, and the bytes they carry.
fn staged_turn_usage(base: &std::path::Path, ttl_ms: u64) -> (usize, u64) {
    let Ok(entries) = std::fs::read_dir(base) else {
        return (0, 0);
    };
    let mut files = 0usize;
    let mut bytes = 0u64;
    for entry in entries.flatten() {
        let is_attachment = entry
            .file_name()
            .to_str()
            .is_some_and(|name| uuid::Uuid::parse_str(name).is_ok());
        let Ok(metadata) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if !is_attachment || !metadata.is_dir() {
            continue;
        }
        let age_ms = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .map(|age| age.as_millis() as u64);
        if age_ms.is_some_and(|age| age > ttl_ms) {
            continue;
        }
        let size = std::fs::read_dir(entry.path())
            .map(|inner| {
                inner
                    .flatten()
                    .filter_map(|file| file.metadata().ok())
                    .filter(|file| file.is_file())
                    .map(|file| file.len())
                    .sum::<u64>()
            })
            .unwrap_or(0);
        files += 1;
        bytes = bytes.saturating_add(size);
    }
    (files, bytes)
}

/// Remove one file this lens staged, by the attachment id its receipt named.
///
/// Deletion is explicit and never implicit: a failed send deletes nothing, so this command is
/// only ever reached from the user's own remove control (or the composer's cancelled upload).
#[tauri::command]
pub async fn cmd_desktop_reference_chat_file_remove(
    daemon: State<'_, Arc<DaemonClient>>,
    session_id: String,
    attachment_id: String,
) -> Result<DesktopChatFileReply, IpcError> {
    if let Some(refusal) = attachment_id_refusal(&attachment_id) {
        return Ok(refusal);
    }
    let worktree = match pane_worktree(&daemon, &session_id).await {
        Ok(worktree) => worktree,
        Err(reply) => return Ok(reply),
    };
    let dir = reference_files::reference_staging_base_dir(&worktree).join(&attachment_id);
    let removed = crate::ipc::run_blocking(move || {
        Ok::<_, crate::ipc::error::IpcError>(reference_files::reference_cancel_staged_file(&dir))
    })
    .await;
    match removed {
        Ok(true) => Ok(DesktopChatFileReply {
            status: 200,
            body: serde_json::json!({ "ok": true }),
        }),
        Ok(false) => Ok(file_failure(
            500,
            "INTERNAL_ERROR",
            "the staged file could not be removed from this host",
        )),
        Err(error) => Ok(file_failure(
            500,
            "INTERNAL_ERROR",
            format!("the removal thread did not finish: {error}"),
        )),
    }
}

/// The refusal a removal request earns before it touches the filesystem, if any.
///
/// The id becomes a directory name under the staging root, so it must be one: the canonical
/// hyphenated lowercase UUID is the only shape this host mints, and anything else (a relative
/// segment, a braced or URN form) could name a directory that is not this host's.
fn attachment_id_refusal(attachment_id: &str) -> Option<DesktopChatFileReply> {
    let trimmed = attachment_id.trim();
    if trimmed.is_empty() {
        return Some(file_failure(
            400,
            "INVALID_REQUEST",
            "a removal names no attachment id",
        ));
    }
    let canonical = uuid::Uuid::parse_str(trimmed)
        .ok()
        .is_some_and(|parsed| parsed.hyphenated().to_string() == trimmed);
    if !canonical {
        return Some(file_failure(
            400,
            "INVALID_REQUEST",
            "an attachment id is a UUID",
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_map_and_untyped_errors_preserve_the_daemon_failure() {
        let reply = DesktopReferenceChatReply { status: 200, body: serde_json::json!({"ok": true}) };
        assert_eq!(reply_from(DaemonResponse::ReferenceChatOk { reply: reply.clone() }).unwrap(), reply);
        let old = reply_from(DaemonResponse::Error {
            message: "Malformed request: unknown variant".into(),
            code: None,
            details: None,
        })
        .unwrap_err();
        assert_eq!(old.code, IpcErrorCode::InternalError);
        assert_eq!(old.message, "Malformed request: unknown variant");
        let identity = DesktopReferenceChatIdentity {
            host_id: "local".into(),
            owner_id: "o".into(),
            epoch: "7".into(),
        };
        assert_eq!(
            identity_from(DaemonResponse::ReferenceChatIdentityOk { identity: identity.clone() }).unwrap(),
            identity
        );
    }

    fn wire_code(reply: &DesktopChatFileReply) -> String {
        reply.body["error"]["code"].as_str().unwrap_or_default().to_string()
    }

    #[test]
    fn every_scope_error_maps_to_one_wire_code_and_status() {
        use crate::scoped_contracts::ScopeErrorCode as Code;
        let all = [
            (Code::InvalidRequest, 400, "INVALID_REQUEST"),
            (Code::Unauthorized, 401, "UNAUTHORIZED"),
            (Code::Forbidden, 403, "FORBIDDEN"),
            (Code::NotFound, 404, "NOT_FOUND"),
            (Code::TargetExpired, 410, "TARGET_EXPIRED"),
            (Code::PayloadTooLarge, 413, "PAYLOAD_TOO_LARGE"),
            (Code::ControlConflict, 409, "CONTROL_CONFLICT"),
            (Code::RequestConflict, 409, "CONTROL_CONFLICT"),
            (Code::OperationOutcomeUnknown, 409, "CONTROL_CONFLICT"),
            (Code::InventoryIncomplete, 503, "INVENTORY_INCOMPLETE"),
            (Code::Timeout, 504, "TIMEOUT"),
            (Code::CaptureUnsupported, 422, "CAPTURE_UNSUPPORTED"),
            (Code::Unsupported, 422, "UNSUPPORTED"),
            (Code::ProviderOwned, 422, "UNSUPPORTED"),
        ];
        for (code, status, wire) in all {
            assert_eq!(file_refusal(code), (status, wire), "{code:?}");
        }
    }

    fn pane(id: &str, workspace: Option<&str>, cwd: Option<&str>) -> crate::daemon::protocol::DaemonSessionDetails {
        let mut details = crate::daemon::protocol::DaemonSessionDetails::new(
            crate::daemon::protocol::DaemonSessionDetailsParams {
                session_id: id.to_string(),
                workspace_id: workspace.map(str::to_string),
                worktree: None,
                cwd: cwd.map(str::to_string),
                cols: 80,
                rows: 24,
                running: true,
                start_sequence: None,
                end_sequence: None,
                last_output_age_ms: None,
                suspended: false,
            },
        );
        details.is_paired_proxy = None;
        details
    }

    #[test]
    fn a_pane_on_a_paired_host_is_refused_instead_of_staging_here() {
        let cwd = std::env::temp_dir().to_string_lossy().to_string();
        let proxied = pane("s", Some("daemon:other"), Some(&cwd));
        let refused = pane_directory(&proxied).unwrap_err();
        assert_eq!(refused.status, 422);
        assert_eq!(wire_code(&refused), "UNSUPPORTED");
        assert!(refused.body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("paired host"));

        let mut flagged = pane("s", Some("local"), Some(&cwd));
        flagged.is_paired_proxy = Some(true);
        assert_eq!(pane_directory(&flagged).unwrap_err().status, 422);

        // A local pane whose cwd is a real directory still stages here.
        assert_eq!(
            pane_directory(&pane("s", Some("local"), Some(&cwd))).unwrap(),
            std::path::PathBuf::from(&cwd)
        );
        // A pane whose directory is already gone (or never set) names no place to open from.
        let missing = std::env::temp_dir().join("ferryx-g5-absent-directory");
        let gone = pane_directory(&pane("s", Some("local"), Some(&missing.to_string_lossy()))).unwrap_err();
        assert_eq!(gone.status, 409);
        assert_eq!(pane_directory(&pane("s", Some("local"), None)).unwrap_err().status, 409);
    }

    /// The desktop stage path applies the same per-turn bounds the HTTP route does: four files
    /// and twenty megabytes for the turn, counted against what this pane's staging base already
    /// holds. Without this, a desktop pane could stage an unbounded set that the web client would
    /// have been refused.
    #[test]
    fn desktop_staging_applies_the_routes_per_turn_bounds() {
        use base64::Engine as _;
        use crate::remote::reference_chat::types::ReferenceFileStagePayload;
        use crate::scoped_contracts::{
            AttachmentMediaType, ScopeErrorCode, ATTACHMENT_MAX_FILES_PER_TURN,
        };

        let root = tempfile::tempdir().expect("tempdir");
        let worktree = root.path();
        let payload = |name: &str, bytes: usize| ReferenceFileStagePayload {
            name: name.to_string(),
            media_type: AttachmentMediaType::Text,
            size_bytes: bytes as u64,
            content_base64: base64::engine::general_purpose::STANDARD.encode(vec![b'a'; bytes]),
        };

        // The first four files of a turn fit; the fifth is refused with the route's own code.
        for index in 0..ATTACHMENT_MAX_FILES_PER_TURN {
            let staged = stage_with_turn_bounds("host", worktree, &payload(&format!("f{index}.txt"), 8), None);
            assert!(staged.is_ok(), "file {index} of the turn must stage: {staged:?}");
        }
        let refused = stage_with_turn_bounds("host", worktree, &payload("fifth.txt", 8), None).unwrap_err();
        assert_eq!(refused, ScopeErrorCode::PayloadTooLarge);

        // A file larger than the per-file cap is refused before anything is written.
        let oversized = stage_with_turn_bounds(
            "host",
            worktree,
            &payload("huge.txt", crate::scoped_contracts::ATTACHMENT_MAX_FILE_BYTES as usize + 1),
            None,
        );
        assert_eq!(oversized.unwrap_err(), ScopeErrorCode::PayloadTooLarge);
    }

    /// A file staged from the desktop is recorded for the sweep, so the 24 h TTL reaches it even
    /// though no HTTP route ever touched this pane (the route records the same base in
    /// `server.rs`).
    #[test]
    fn desktop_staging_records_its_base_for_the_ttl_sweep() {
        use base64::Engine as _;
        use crate::remote::reference_chat::types::ReferenceFileStagePayload;
        use crate::scoped_contracts::AttachmentMediaType;

        let root = tempfile::tempdir().expect("tempdir");
        let worktree = root.path();
        let ledger = root.path().join(reference_files::REFERENCE_STAGING_LEDGER_NAME);
        let payload = ReferenceFileStagePayload {
            name: "notes.txt".to_string(),
            media_type: AttachmentMediaType::Text,
            size_bytes: 4,
            content_base64: base64::engine::general_purpose::STANDARD.encode(b"note"),
        };

        stage_with_turn_bounds("host", worktree, &payload, Some(&ledger)).expect("stage succeeds");

        let recorded = std::fs::read_to_string(&ledger).expect("the ledger is written");
        let base = reference_files::reference_staging_base_dir(worktree);
        assert_eq!(
            recorded.lines().collect::<Vec<_>>(),
            vec![base.to_str().expect("utf-8 tempdir path")],
            "the stage records exactly the base it wrote into"
        );
        // The sweep reaches it: the base is under the staging directory name the sweep accepts.
        assert_eq!(
            base.file_name().and_then(|name| name.to_str()),
            Some(reference_files::REFERENCE_STAGING_DIR_NAME)
        );
    }

    #[test]
    fn a_removal_only_accepts_the_uuid_shape_this_host_mints() {
        for refused in ["", "   ", "..", "../escape", "att-notes", "a/b"] {
            let reply = attachment_id_refusal(refused).expect("must be refused");
            assert_eq!(reply.status, 400, "{refused:?}");
            assert_eq!(wire_code(&reply), "INVALID_REQUEST", "{refused:?}");
        }
        // Other spellings of a real id are not directory names this host created.
        let minted = uuid::Uuid::new_v4();
        let canonical = minted.to_string();
        assert!(attachment_id_refusal(&canonical).is_none());
        for refused in [
            minted.simple().to_string(),
            minted.braced().to_string(),
            minted.urn().to_string(),
            canonical.to_uppercase(),
        ] {
            let reply = attachment_id_refusal(&refused).expect("must be refused");
            assert_eq!(reply.status, 400, "{refused:?}");
        }
    }
}
