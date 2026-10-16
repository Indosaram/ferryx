//! Owning-host file staging and preview for the Herdr reference chat (plan task 11).
//!
//! Ported from 'devswha/herdr-web-ui' @
//! '54e5a1f67090cb09552d182e7e30dd0ecc314918' (MIT, 'docs/chat/HERDR_LICENSE').
//! Upstream anchors, read at the pinned revision:
//!
//! | Upstream | What this module ports |
//! |---|---|
//! | 'server/paste.ts' (savePaneImage, PasteImageError) | a pasted file lands on the host that
//!   owns the pane and reaches the program as a PATH it opens, never as browser bytes |
//! | 'server/conversation.ts' file and image parts | a staged file belongs to the same
//!   conversation as typed text, so it needs no second transport |
//!
//! Frozen contract: 'docs/chat/herdr-port-contract.md' section 5 (Files - owner-host, bounded)
//! and section 6 (the 'stage_reference_file' entry point). This module declares no shared type,
//! registers no route and edits no sibling lane; the integration owner registers it and task 13
//! owns the route wiring.
//!
//! ## What this module is, and is not
//!
//! * Staging happens on the OWNING host. The receipt is the existing owner-private
//!   'AttachmentReceipt' (host, opaque id, sha256, size, media type) and carries NO filesystem
//!   path; the path the agent receives is the editable '@<dir>/<id>/<name> ' mention. The bytes
//!   are staged INSIDE the pane's own working directory (under 'REFERENCE_STAGING_DIR_NAME'),
//!   so that relative mention resolves, from the pane's cwd, to exactly the uploaded bytes.
//! * The bytes never travel through a managed turn and no child process is launched anywhere in
//!   this file. 'reference_file_send_plan' returns an ordinary 'ReferenceSubmitPayload' whose
//!   'attachment_ids' is EMPTY: the mention is text the user can still edit, and it reaches the
//!   pane through the ordered submit lane (task 7) exactly like typed text.
//! * Bounds are the frozen scoped ones: 'ATTACHMENT_MAX_FILE_BYTES',
//!   'ATTACHMENT_MAX_FILES_PER_TURN', 'ATTACHMENT_MAX_TURN_BYTES' and
//!   'ATTACHMENT_UNREFERENCED_TTL_MS'. Nothing here re-declares them.
//! * A refused payload stages nothing; a failed send deletes nothing. Deletion is always
//!   explicit ('reference_delete_staged_file' / 'reference_cancel_staged_file').
//!
//! ## Reuse seam (W2)
//!
//! The owner-safe rules here are the rules W2's 'remote/attachment_api.rs' applies on the owning
//! host: a name with no separator, parent or control component; containment proven by
//! canonicalising BOTH the root and the candidate before a component-wise prefix compare (so a
//! symlink that leaves the root is refused); and owner-private modes (0700 directory, 0600 file)
//! on unix. W2's route layer owns the HTTP surface and the shared attachment store; this lane
//! owns the reference chat's use of them, so it mirrors those rules rather than importing a
//! module that has not been ported into this worktree yet. When the integration owner brings
//! 'attachment_api.rs' across, these helpers can delegate to it without changing a signature.
//!
//! ## One honest mapping
//!
//! The frozen 'ScopeErrorCode' has no internal-error variant, so a staging I/O failure is
//! reported as 'ScopeErrorCode::Unsupported' by this pure module; the route layer (task 13) maps
//! it to the HTTP status it already uses for a failed staging write. Nothing here invents a
//! second error enum.

use std::path::{Component, Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use sha2::{Digest, Sha256};

use crate::scoped_contracts::{
    AttachmentReceipt, ScopeErrorCode, ATTACHMENT_MAX_FILE_BYTES, ATTACHMENT_MAX_FILES_PER_TURN,
    ATTACHMENT_MAX_TURN_BYTES, ATTACHMENT_UNREFERENCED_TTL_MS,
};

use super::types::{
    reference_chat_route, ReferenceFileReceipt, ReferenceFileStagePayload, ReferenceSubmitOrigin,
    ReferenceSubmitPayload,
};

/// Directory under the pane's working directory that holds this lane's staged files.
pub const REFERENCE_STAGING_DIR_NAME: &str = ".ferryx-reference-chat-files";
/// File, in the gateway's owner-private data directory, listing every staging base it used.
pub const REFERENCE_STAGING_LEDGER_NAME: &str = "reference-chat-staging-bases.txt";
/// Environment override for the owning host id reported on the receipt.
pub const REFERENCE_HOST_ID_ENV: &str = "FERRYX_HOST_ID";
/// Host id used when the host did not name itself.
pub const REFERENCE_LOCAL_HOST_ID: &str = "local";
/// Longest accepted staged file name, in bytes (the scoped attachment rule).
pub const REFERENCE_FILE_MAX_NAME_BYTES: usize = 255;
/// Route suffix for the file routes, under the frozen reference-chat prefix.
pub const REFERENCE_FILE_ROUTE_SUFFIX: &str = "files";

/// The staging base inside one pane's working directory.
pub fn reference_staging_base_dir(worktree: &Path) -> PathBuf {
    worktree.join(REFERENCE_STAGING_DIR_NAME)
}

/// The owning host's id, as the receipt reports it.
pub fn reference_host_id() -> String {
    std::env::var(REFERENCE_HOST_ID_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| REFERENCE_LOCAL_HOST_ID.to_string())
}

/// The unreferenced staged-file TTL: the frozen scoped value, never a local constant.
pub fn reference_unreferenced_ttl_ms() -> u64 {
    ATTACHMENT_UNREFERENCED_TTL_MS
}

/// The route that stages and lists this target's files (task 13 registers it).
pub fn reference_files_route(session_id: &str) -> String {
    reference_chat_route(session_id, REFERENCE_FILE_ROUTE_SUFFIX)
}

/// The route that previews or deletes one staged file.
pub fn reference_file_route(session_id: &str, attachment_id: &str) -> String {
    reference_chat_route(
        session_id,
        &format!("{REFERENCE_FILE_ROUTE_SUFFIX}/{attachment_id}"),
    )
}

/// A staged file's name, or the refusal that keeps it out of the staging root.
///
/// A name is a NAME: no directory component, no parent, no control byte, no whitespace (an
/// '@path ' mention cannot carry it; the TS twin refuses it too), bounded length.
pub fn validate_reference_file_name(raw: &str) -> Result<String, ScopeErrorCode> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > REFERENCE_FILE_MAX_NAME_BYTES {
        return Err(ScopeErrorCode::InvalidRequest);
    }
    // ':' is refused too: on Windows 'a:x.png' is a drive-relative path, and joining it replaces
    // the staging root instead of naming a file inside it.
    if trimmed.contains('/') || trimmed.contains(char::from(92)) || trimmed.contains(':') {
        return Err(ScopeErrorCode::InvalidRequest);
    }
    if trimmed == "." || trimmed == ".." {
        return Err(ScopeErrorCode::InvalidRequest);
    }
    if trimmed.chars().any(char::is_control) {
        return Err(ScopeErrorCode::InvalidRequest);
    }
    if trimmed.chars().any(char::is_whitespace) {
        return Err(ScopeErrorCode::InvalidRequest);
    }
    Ok(trimmed.to_string())
}

/// Is this mention path safe to hand to a program as a relative path?
///
/// Refuses an absolute path, a Windows drive prefix, a home-relative path, a backslash
/// separator, a control byte, an empty or whitespace-padded path, and any `.` or `..`
/// component - refused whether it is written as its own segment (`sub/./shot.png`, `a/../b`)
/// or spelled with a leading dot (`./shot.png`, `../x`). The TS twin
/// (`ui/src/remote/chat/referenceFiles.ts`, `referenceMentionPathIsSafe`) applies the same
/// segment rule; a divergence between the two guards is a defect in its own right.
pub fn reference_mention_path_is_safe(relative: &str) -> bool {
    if relative.is_empty() || relative.trim() != relative {
        return false;
    }
    if relative.chars().any(char::is_control) {
        return false;
    }
    if relative.starts_with('/') || relative.contains(char::from(92)) || relative.starts_with('~')
    {
        return false;
    }
    if reference_has_drive_prefix(relative) {
        return false;
    }
    // A name is a NAME: every segment must be a real one, so `.` and `..` are refused both as
    // their own segment (`sub/./shot.png`, `a/../b`) and when they lead the path. The host
    // component walk this replaces rejected a leading dot only when a separator followed it,
    // so an interior `/./` slipped through while the TS twin refused it.
    relative
        .split('/')
        .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn reference_has_drive_prefix(path: &str) -> bool {
    let mut chars = path.chars();
    match (chars.next(), chars.next()) {
        (Some(letter), Some(':')) => letter.is_ascii_alphabetic(),
        _ => false,
    }
}

/// Does a host-resolved path still live inside the root it was resolved against?
///
/// The compare is component-wise ('/root/wt2' is not inside '/root/wt'), which is what makes
/// this the fence for a symlink that left the root: the caller resolves the symlink first
/// ('resolve_reference_preview_path' does), then asks this question about the real path.
pub fn reference_path_within_root(root: &Path, resolved: &Path) -> bool {
    if root.as_os_str().is_empty() || resolved.as_os_str().is_empty() {
        return false;
    }
    if resolved
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return false;
    }
    let root = reference_normalized_root(root);
    let resolved = reference_normalized_root(resolved);
    resolved == root || resolved.starts_with(&root)
}

/// Strip the extended-length prefix Windows canonicalisation adds, so a canonical path compares
/// against a caller-supplied root as plain text (the rule W2's containment check applies).
#[cfg(windows)]
fn reference_normalized_root(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy().into_owned();
    let slash = char::from(92);
    let extended: String = [slash, slash, '?', slash].iter().collect();
    if let Some(rest) = text.strip_prefix(&extended) {
        let unc: String = [slash, slash, '?', slash, 'U', 'N', 'C', slash]
            .iter()
            .collect();
        if let Some(unc_rest) = text.strip_prefix(&unc) {
            return PathBuf::from(format!("{slash}{slash}{unc_rest}"));
        }
        return PathBuf::from(rest);
    }
    PathBuf::from(text)
}

#[cfg(not(windows))]
fn reference_normalized_root(path: &Path) -> PathBuf {
    path.to_path_buf()
}

/// Resolve a preview path under the worktree root, or refuse it.
///
/// Traversal is refused before any filesystem call; containment is proven afterwards against the
/// canonicalised root, so a symlink that resolves outside the root is refused with
/// 'ScopeErrorCode::Forbidden' even though its own path looked relative.
pub fn resolve_reference_preview_path(
    root: &Path,
    relative: &str,
) -> Result<PathBuf, ScopeErrorCode> {
    if !reference_mention_path_is_safe(relative) {
        return Err(ScopeErrorCode::InvalidRequest);
    }
    let canonical_root = std::fs::canonicalize(root).map_err(|_| ScopeErrorCode::NotFound)?;
    let candidate = reference_normalized_root(&canonical_root).join(relative);
    let canonical_candidate =
        std::fs::canonicalize(&candidate).map_err(|_| ScopeErrorCode::NotFound)?;
    if !reference_path_within_root(&canonical_root, &canonical_candidate) {
        return Err(ScopeErrorCode::Forbidden);
    }
    Ok(canonical_candidate)
}

/// The bounds a turn's staged files must respect, from the frozen scoped limits.
pub fn reference_file_bounds_check(
    size_bytes: u64,
    existing_files: usize,
    existing_turn_bytes: u64,
) -> Result<(), ScopeErrorCode> {
    if size_bytes > ATTACHMENT_MAX_FILE_BYTES {
        return Err(ScopeErrorCode::PayloadTooLarge);
    }
    if existing_files.saturating_add(1) > ATTACHMENT_MAX_FILES_PER_TURN {
        return Err(ScopeErrorCode::PayloadTooLarge);
    }
    if existing_turn_bytes.saturating_add(size_bytes) > ATTACHMENT_MAX_TURN_BYTES {
        return Err(ScopeErrorCode::PayloadTooLarge);
    }
    Ok(())
}

/// Read a staged file for preview.
///
/// The staged path lives in the pane's own worktree, where the agent can replace it: a symlink or
/// any other non-regular file there (a link to a secret, a device that never reaches EOF) is
/// refused without being followed, the read is capped at [ATTACHMENT_MAX_FILE_BYTES], and bytes
/// whose length differs from the receipt are not the file that was staged.
pub fn read_reference_staged_file(
    path: &Path,
    expected_size: u64,
) -> Result<Vec<u8>, ScopeErrorCode> {
    use std::io::Read as _;

    let metadata = std::fs::symlink_metadata(path).map_err(|_| ScopeErrorCode::NotFound)?;
    if !metadata.file_type().is_file() {
        return Err(ScopeErrorCode::NotFound);
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        // A link swapped in between the check above and this open is refused too.
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(|_| ScopeErrorCode::NotFound)?;
    if !file.metadata().map_err(|_| ScopeErrorCode::NotFound)?.is_file() {
        return Err(ScopeErrorCode::NotFound);
    }
    let mut bytes = Vec::new();
    file.take(ATTACHMENT_MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ScopeErrorCode::NotFound)?;
    if bytes.len() as u64 > ATTACHMENT_MAX_FILE_BYTES {
        return Err(ScopeErrorCode::PayloadTooLarge);
    }
    if bytes.len() as u64 != expected_size {
        return Err(ScopeErrorCode::NotFound);
    }
    Ok(bytes)
}

/// Decode a stage payload's bytes, refusing an oversized file and a declared size that does not
/// match the bytes actually carried.
pub fn decode_reference_file_payload(
    payload: &ReferenceFileStagePayload,
) -> Result<Vec<u8>, ScopeErrorCode> {
    if payload.size_bytes > ATTACHMENT_MAX_FILE_BYTES {
        return Err(ScopeErrorCode::PayloadTooLarge);
    }
    let decoded = BASE64_STANDARD
        .decode(payload.content_base64.as_bytes())
        .map_err(|_| ScopeErrorCode::InvalidRequest)?;
    if decoded.len() as u64 != payload.size_bytes {
        return Err(ScopeErrorCode::InvalidRequest);
    }
    Ok(decoded)
}

/// The mention text for a staged path: '@name ', still editable by the user.
pub fn reference_mention_text(path: &str) -> String {
    ReferenceFileReceipt::mention_for(path)
}

/// Append a mention to a draft, adding the separating space only when one is needed.
pub fn reference_append_mention(current: &str, path: &str) -> String {
    let mention = reference_mention_text(path);
    let needs_space = current
        .chars()
        .next_back()
        .is_some_and(|last| !last.is_whitespace());
    if needs_space {
        return format!("{current} {mention}");
    }
    format!("{current}{mention}")
}

/// The submit a staged file produces: the mention as EDITABLE TEXT, no managed attachment.
///
/// 'attachment_ids' is deliberately empty. The staged file is already on the owning host; the
/// only thing that has to reach the program is the path, and it travels as typed text through
/// the ordered submit lane. A plan is pure: building it stages, sends and deletes nothing.
pub fn reference_file_send_plan(
    draft_text: &str,
    receipts: &[ReferenceFileReceipt],
) -> ReferenceSubmitPayload {
    let mut text = draft_text.to_string();
    for receipt in receipts {
        let path = receipt
            .mention_text
            .strip_prefix('@')
            .unwrap_or(&receipt.mention_text)
            .trim_end();
        text = reference_append_mention(&text, path);
    }
    ReferenceSubmitPayload {
        text,
        attachment_ids: Vec::new(),
        origin: ReferenceSubmitOrigin::Chat,
    }
}

/// Stage a payload under an explicit host id, inside the pane working directory 'worktree'.
///
/// The bytes are written once, into '<worktree>/REFERENCE_STAGING_DIR_NAME/<id>/<name>': the
/// base is verified to be a real directory this user owns with no group/other access (never a
/// symlink), the per-attachment directory is created 0700 and the file 0600 on unix. The
/// receipt names no absolute path; its mention is the path RELATIVE to the worktree. A refused
/// payload leaves the worktree untouched, and a write that fails removes the directory it
/// created rather than leaving a partial file behind.
pub fn stage_reference_file_in(
    host_id: &str,
    worktree: &Path,
    payload: &ReferenceFileStagePayload,
) -> Result<ReferenceFileReceipt, ScopeErrorCode> {
    let name = validate_reference_file_name(&payload.name)?;
    let bytes = decode_reference_file_payload(payload)?;

    if !worktree.is_dir() {
        return Err(ScopeErrorCode::NotFound);
    }
    let base_dir = reference_staging_base_dir(worktree);
    ensure_reference_staging_base(&base_dir)?;
    let attachment_id = uuid::Uuid::new_v4().to_string();
    let staging_dir = base_dir.join(&attachment_id);
    create_reference_staging_dir(&staging_dir)?;

    let staged_path = staging_dir.join(&name);
    if write_reference_staged_file(&staged_path, &bytes).is_err() {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(reference_staging_failure_code());
    }

    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let sha256 = format!("{:x}", hasher.finalize());

    let mention_path = format!("{REFERENCE_STAGING_DIR_NAME}/{attachment_id}/{name}");
    Ok(ReferenceFileReceipt {
        receipt: AttachmentReceipt {
            host_id: host_id.to_string(),
            attachment_id,
            sha256,
            size_bytes: bytes.len() as u64,
            media_type: payload.media_type,
        },
        display_name: name.clone(),
        mention_text: ReferenceFileReceipt::mention_for(&mention_path),
    })
}

/// Cancel a staged attachment's directory: the explicit cleanup path a cancelled upload takes.
pub fn reference_cancel_staged_file(staging_dir: &Path) -> bool {
    if !staging_dir.exists() {
        return true;
    }
    std::fs::remove_dir_all(staging_dir).is_ok()
}

/// Delete one staged file: the only deletion in this module, and never implicit.
pub fn reference_delete_staged_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if metadata.is_dir() {
        return std::fs::remove_dir_all(path).is_ok();
    }
    std::fs::remove_file(path).is_ok()
}

/// The code a staging I/O failure reports through the frozen enum (see the module header).
pub fn reference_staging_failure_code() -> ScopeErrorCode {
    ScopeErrorCode::Unsupported
}

/// Remove staging directories older than the TTL, whether or not this process registered them.
///
/// The in-memory registry is process-local, so after a restart only the directory mtime says
/// how old a staged file is. Only '<uuid>' directories are considered; anything else is kept.
pub fn reference_prune_staging_root(base: &Path, ttl_ms: u64) -> usize {
    // Never follow a planted symlink or sweep a base someone else controls: the same checks a
    // stage applies, without creating anything.
    if verify_reference_staging_base(base).is_err() {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(base) else {
        return 0;
    };
    let ttl = std::time::Duration::from_millis(ttl_ms);
    let now = std::time::SystemTime::now();
    let mut removed = 0;
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
        let stale = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > ttl);
        if stale && std::fs::remove_dir_all(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Remember a staging base in the ledger, so a later sweep reaches it even when no stage ever
/// targets that directory again and the in-memory registry has been lost to a restart.
pub fn reference_record_staging_base(ledger: &Path, base: &Path) -> std::io::Result<()> {
    let Some(line) = base.to_str().filter(|line| !line.contains(['\n', '\r'])) else {
        return Ok(());
    };
    let _guard = REFERENCE_STAGING_LEDGER_LOCK.lock();
    let existing = match std::fs::read_to_string(ledger) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    if existing.lines().any(|recorded| recorded == line) {
        return Ok(());
    }
    if let Some(parent) = ledger.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    use std::io::Write as _;
    writeln!(options.open(ledger)?, "{line}")
}

/// Sweep every staging base the ledger names with [reference_prune_staging_root], and forget
/// the bases that no longer exist. Only paths whose last component is the staging directory
/// name are ever swept, so a tampered line cannot aim the sweep at another directory.
pub fn reference_prune_recorded_staging_bases(ledger: &Path, ttl_ms: u64) -> usize {
    let _guard = REFERENCE_STAGING_LEDGER_LOCK.lock();
    let Ok(text) = std::fs::read_to_string(ledger) else {
        return 0;
    };
    let mut removed = 0;
    let mut kept = Vec::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let base = Path::new(line);
        if base.file_name().and_then(|name| name.to_str()) != Some(REFERENCE_STAGING_DIR_NAME) {
            continue;
        }
        if std::fs::symlink_metadata(base).is_err() {
            continue;
        }
        removed += reference_prune_staging_root(base, ttl_ms);
        kept.push(line);
    }
    if kept.len() != text.lines().count() {
        let mut body = kept.join("\n");
        if !body.is_empty() {
            body.push('\n');
        }
        let _ = std::fs::write(ledger, body);
    }
    removed
}

/// Serializes this process's read-modify-write passes over the ledger.
static REFERENCE_STAGING_LEDGER_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

/// Create (0700) or verify the staging base: a real directory, never a symlink, owned by this
/// user and closed to group/other on unix. A base that fails any check is refused, never fixed
/// up, because someone else may already control it.
fn ensure_reference_staging_base(base: &Path) -> Result<(), ScopeErrorCode> {
    if std::fs::symlink_metadata(base).is_err() {
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            builder.mode(0o700);
        }
        builder
            .create(base)
            .map_err(|_| reference_staging_failure_code())?;
        // Keep staged uploads out of the pane's git status.
        let _ = std::fs::write(base.join(".gitignore"), b"*\n");
    }
    verify_reference_staging_base(base)
}

/// Verify an existing staging base: a real directory, never a symlink, owned by this user and
/// closed to group/other on unix.
fn verify_reference_staging_base(base: &Path) -> Result<(), ScopeErrorCode> {
    let metadata = std::fs::symlink_metadata(base).map_err(|_| reference_staging_failure_code())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ScopeErrorCode::Forbidden);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        // SAFETY: geteuid has no arguments, pointers, or caller preconditions.
        let uid = unsafe { libc::geteuid() };
        if metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
            return Err(ScopeErrorCode::Forbidden);
        }
    }
    Ok(())
}

fn create_reference_staging_dir(dir: &Path) -> Result<(), ScopeErrorCode> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder
        .create(dir)
        .map_err(|_| reference_staging_failure_code())
}

fn write_reference_staged_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    file.write_all(bytes)?;
    file.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use crate::scoped_contracts::AttachmentMediaType;
    use sha2::{Digest, Sha256};

    // rs-files-routes-r6-1
    #[test]
    fn a_staged_file_read_refuses_a_symlink_an_oversized_file_and_a_size_mismatch() {
        let root = scratch_dir();
        std::fs::write(root.join("shot.png"), b"png!").expect("write staged");
        assert_eq!(
            read_reference_staged_file(&root.join("shot.png"), 4).expect("the staged file reads"),
            b"png!".to_vec()
        );
        assert_eq!(
            read_reference_staged_file(&root.join("shot.png"), 5),
            Err(ScopeErrorCode::NotFound),
            "a file whose size differs from its receipt is not the staged file"
        );

        #[cfg(unix)]
        {
            std::fs::write(root.join("secret"), b"key").expect("write secret");
            std::os::unix::fs::symlink(root.join("secret"), root.join("link.png"))
                .expect("symlink");
            assert_eq!(
                read_reference_staged_file(&root.join("link.png"), 3),
                Err(ScopeErrorCode::NotFound),
                "a symlink swapped in at the staged path is never followed"
            );
        }

        let big = vec![0u8; (ATTACHMENT_MAX_FILE_BYTES + 1) as usize];
        std::fs::write(root.join("big.png"), &big).expect("write big");
        assert_eq!(
            read_reference_staged_file(&root.join("big.png"), ATTACHMENT_MAX_FILE_BYTES + 1),
            Err(ScopeErrorCode::PayloadTooLarge),
            "nothing past the file cap is served"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    fn scratch_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ferryx-ref-files-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn staged_dir(root: &Path, attachment_id: &str) -> PathBuf {
        root.join(REFERENCE_STAGING_DIR_NAME).join(attachment_id)
    }

    fn payload(name: &str, bytes: &[u8]) -> ReferenceFileStagePayload {
        ReferenceFileStagePayload {
            name: name.to_string(),
            media_type: AttachmentMediaType::Png,
            size_bytes: bytes.len() as u64,
            content_base64: BASE64_STANDARD.encode(bytes),
        }
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!("{:x}", hasher.finalize())
    }

    fn backslash() -> char {
        char::from(92)
    }

    #[test]
    fn staging_writes_an_owner_private_file_and_an_opaque_receipt() {
        let dir = scratch_dir();
        let bytes = b"fake png body";
        let receipt = stage_reference_file_in("host-test", &dir, &payload("shot.png", bytes))
            .expect("staged");

        assert_eq!(receipt.display_name, "shot.png");
        assert_eq!(
            receipt.mention_text,
            format!(
                "@{REFERENCE_STAGING_DIR_NAME}/{}/shot.png ",
                receipt.receipt.attachment_id
            )
        );
        assert_eq!(receipt.receipt.host_id, "host-test");
        assert_eq!(receipt.receipt.size_bytes, bytes.len() as u64);
        assert_eq!(receipt.receipt.sha256, sha256_hex(bytes));
        assert!(!receipt.receipt.attachment_id.is_empty());

        let staged = staged_dir(&dir, &receipt.receipt.attachment_id).join("shot.png");
        assert!(staged.is_file());
        assert_eq!(std::fs::read(&staged).expect("read back"), bytes);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let file_mode = std::fs::metadata(&staged)
                .expect("file metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(file_mode, 0o600);
            let dir_mode = std::fs::metadata(staged.parent().expect("parent"))
                .expect("dir metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(dir_mode, 0o700);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_names_with_separators_parent_dirs_or_control_bytes_are_refused() {
        let mut newline_name = String::from("line");
        newline_name.push(char::from(10));
        newline_name.push_str("break.png");

        for bad in ["", "   ", "../evil.png", "a/b.png", "..", "."] {
            assert_eq!(
                validate_reference_file_name(bad),
                Err(ScopeErrorCode::InvalidRequest),
                "name {bad:?} must be refused"
            );
        }
        assert_eq!(
            validate_reference_file_name(&newline_name),
            Err(ScopeErrorCode::InvalidRequest)
        );

        let mut windows_separator = String::from("a");
        windows_separator.push(backslash());
        windows_separator.push_str("b.png");
        assert_eq!(
            validate_reference_file_name(&windows_separator),
            Err(ScopeErrorCode::InvalidRequest)
        );

        let mut control = String::from("bad");
        control.push(char::from(1));
        control.push_str("name.png");
        assert_eq!(
            validate_reference_file_name(&control),
            Err(ScopeErrorCode::InvalidRequest)
        );

        assert_eq!(
            validate_reference_file_name("  shot.png  "),
            Ok("shot.png".to_string())
        );
        assert!(validate_reference_file_name(&"a".repeat(REFERENCE_FILE_MAX_NAME_BYTES)).is_ok());
        assert_eq!(
            validate_reference_file_name(&"a".repeat(REFERENCE_FILE_MAX_NAME_BYTES + 1)),
            Err(ScopeErrorCode::InvalidRequest)
        );
    }

    #[test]
    fn mention_paths_must_be_relative_and_free_of_parent_components() {
        for good in ["shot.png", "sub/dir/shot.png", "a-b_c.1.png"] {
            assert!(reference_mention_path_is_safe(good), "{good} must be accepted");
        }

        let mut windows_absolute = String::new();
        windows_absolute.push(backslash());
        windows_absolute.push_str("windows");
        windows_absolute.push(backslash());
        windows_absolute.push_str("system32");

        let mut backslash_relative = String::from("sub");
        backslash_relative.push(backslash());
        backslash_relative.push_str("shot.png");

        let mut control = String::from("bad");
        control.push(char::from(1));
        control.push_str("path.png");

        for bad in [
            "",
            " ",
            "/etc/passwd",
            "~/secrets",
            "../outside.png",
            "sub/../../outside.png",
            "C:/windows",
            "sub/./shot.png",
        ] {
            assert!(!reference_mention_path_is_safe(bad), "{bad} must be refused");
        }
        assert!(!reference_mention_path_is_safe(&windows_absolute));
        assert!(!reference_mention_path_is_safe(&backslash_relative));
        assert!(!reference_mention_path_is_safe(&control));
    }

    #[test]
    fn preview_paths_refuse_traversal_absolute_and_missing_targets() {
        let root = scratch_dir();
        std::fs::create_dir_all(root.join("sub")).expect("create sub");
        std::fs::write(root.join("sub/ok.txt"), b"ok").expect("write ok");

        let resolved = resolve_reference_preview_path(&root, "sub/ok.txt").expect("contained");
        assert!(resolved.ends_with("ok.txt"));

        assert_eq!(
            resolve_reference_preview_path(&root, "../escape.txt"),
            Err(ScopeErrorCode::InvalidRequest)
        );
        assert_eq!(
            resolve_reference_preview_path(&root, "/etc/passwd"),
            Err(ScopeErrorCode::InvalidRequest)
        );
        assert_eq!(
            resolve_reference_preview_path(&root, "sub/missing.txt"),
            Err(ScopeErrorCode::NotFound)
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn preview_paths_refuse_a_symlink_that_escapes_the_root() {
        let root = scratch_dir();
        let outside = scratch_dir();
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::create_dir_all(&outside).expect("create outside");
        std::fs::write(outside.join("secret.txt"), b"secret").expect("write secret");
        std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("link.txt"))
            .expect("symlink");

        assert_eq!(
            resolve_reference_preview_path(&root, "link.txt"),
            Err(ScopeErrorCode::Forbidden)
        );

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn bounds_refuse_oversize_over_count_and_over_budget() {
        assert!(reference_file_bounds_check(1024, 0, 0).is_ok());
        assert_eq!(
            reference_file_bounds_check(ATTACHMENT_MAX_FILE_BYTES + 1, 0, 0),
            Err(ScopeErrorCode::PayloadTooLarge)
        );
        assert_eq!(
            reference_file_bounds_check(1, ATTACHMENT_MAX_FILES_PER_TURN, 0),
            Err(ScopeErrorCode::PayloadTooLarge)
        );
        assert_eq!(
            reference_file_bounds_check(1, 0, ATTACHMENT_MAX_TURN_BYTES),
            Err(ScopeErrorCode::PayloadTooLarge)
        );
        assert!(reference_file_bounds_check(
            ATTACHMENT_MAX_FILE_BYTES,
            ATTACHMENT_MAX_FILES_PER_TURN - 1,
            0
        )
        .is_ok());
    }

    #[test]
    fn decoded_bytes_must_match_the_declared_size() {
        let mut mismatched = payload("shot.png", b"abc");
        mismatched.size_bytes = 4;
        assert_eq!(
            stage_reference_file_in("host-test", &scratch_dir(), &mismatched),
            Err(ScopeErrorCode::InvalidRequest)
        );

        let mut undecodable = payload("shot.png", b"abc");
        undecodable.content_base64 = "not base64 !!!".to_string();
        assert_eq!(
            decode_reference_file_payload(&undecodable),
            Err(ScopeErrorCode::InvalidRequest)
        );

        let mut oversized = payload("shot.png", b"abc");
        oversized.size_bytes = ATTACHMENT_MAX_FILE_BYTES + 1;
        assert_eq!(
            decode_reference_file_payload(&oversized),
            Err(ScopeErrorCode::PayloadTooLarge)
        );
    }

    #[test]
    fn a_refused_payload_stages_nothing() {
        let dir = scratch_dir();
        let mut refused = payload("shot.png", b"body");
        refused.name = "../escape.png".to_string();

        assert!(stage_reference_file_in("host-test", &dir, &refused).is_err());
        assert!(
            !dir.join(REFERENCE_STAGING_DIR_NAME).exists(),
            "a refused payload must not create the staging root"
        );
    }

    #[test]
    fn cancel_and_delete_remove_only_the_staged_state() {
        let dir = scratch_dir();
        let receipt = stage_reference_file_in("host-test", &dir, &payload("shot.png", b"body"))
            .expect("staged");
        let attachment_dir = staged_dir(&dir, &receipt.receipt.attachment_id);
        let staged = attachment_dir.join("shot.png");
        assert!(staged.is_file());

        assert!(reference_delete_staged_file(&staged));
        assert!(!staged.exists());
        assert!(!reference_delete_staged_file(&staged));

        assert!(reference_cancel_staged_file(&attachment_dir));
        assert!(!attachment_dir.exists());
        assert!(reference_cancel_staged_file(&attachment_dir));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn send_plan_carries_editable_mentions_and_no_managed_attachment() {
        let dir = scratch_dir();
        let first =
            stage_reference_file_in("host-test", &dir, &payload("one.png", b"1")).expect("first");
        let second =
            stage_reference_file_in("host-test", &dir, &payload("two.txt", b"2")).expect("second");

        let plan = reference_file_send_plan("look at this", &[first.clone(), second.clone()]);

        assert_eq!(
            plan.text,
            format!("look at this {}{}", first.mention_text, second.mention_text)
        );
        assert!(
            plan.attachment_ids.is_empty(),
            "no managed-turn attachment id may be set"
        );
        assert_eq!(plan.origin, ReferenceSubmitOrigin::Chat);

        assert!(staged_dir(&dir, &first.receipt.attachment_id)
            .join("one.png")
            .is_file());
        assert!(staged_dir(&dir, &second.receipt.attachment_id)
            .join("two.txt")
            .is_file());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_mention_is_whitespace_aware() {
        assert_eq!(reference_append_mention("", "shot.png"), "@shot.png ");
        assert_eq!(reference_append_mention("hello", "shot.png"), "hello @shot.png ");
        assert_eq!(reference_append_mention("hello ", "shot.png"), "hello @shot.png ");

        let mut with_newline = String::from("hello");
        with_newline.push(char::from(10));
        let mut expected_with_newline = String::from("hello");
        expected_with_newline.push(char::from(10));
        expected_with_newline.push_str("@shot.png ");
        assert_eq!(
            reference_append_mention(&with_newline, "shot.png"),
            expected_with_newline
        );
    }

    #[test]
    fn a_failed_send_deletes_nothing_and_the_draft_survives() {
        let dir = scratch_dir();
        let receipt = stage_reference_file_in("host-test", &dir, &payload("shot.png", b"body"))
            .expect("staged");
        let draft = "still typing";
        let staged = staged_dir(&dir, &receipt.receipt.attachment_id).join("shot.png");
        let plan = reference_file_send_plan(draft, std::slice::from_ref(&receipt));
        assert!(plan.text.starts_with(draft));
        assert!(plan.attachment_ids.is_empty());
        assert!(staged.is_file(), "a send plan never removes staged state");

        assert!(reference_delete_staged_file(&staged));
        assert!(!staged.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// rs-files-routes-r5-1: a drive-prefixed name ('a:x.png') would replace the staging root on
    /// Windows when joined, so ':' is refused like a separator.
    #[test]
    fn file_names_with_a_drive_prefix_are_refused() {
        for bad in ["a:x.png", "C:shot.png", "note:stream"] {
            assert_eq!(
                validate_reference_file_name(bad),
                Err(ScopeErrorCode::InvalidRequest),
                "name {bad:?} must be refused: joining it can leave the staging root"
            );
        }
    }

    /// ui-reference-logic-2: the Rust validator refuses whitespace exactly as the TS twin does,
    /// because a mention cannot carry it.
    #[test]
    fn file_names_with_whitespace_are_refused_like_the_ts_twin() {
        for bad in ["two words.png", "Screenshot 2026-10-13 at 10.00.00.png"] {
            assert_eq!(
                validate_reference_file_name(bad),
                Err(ScopeErrorCode::InvalidRequest),
                "name {bad:?} must be refused: its mention could not carry it"
            );
        }
        assert_eq!(
            validate_reference_file_name("~notes.txt"),
            Ok("~notes.txt".to_string())
        );
    }

    /// rs-files-routes-7: a staging directory older than the TTL is removed by a fresh prune
    /// pass, even though no in-memory registry of this process ever knew it.
    #[cfg(unix)]
    #[test]
    fn a_stale_staging_directory_is_pruned_without_the_registry() {
        let base = scratch_dir();
        let stale = base.join(uuid::Uuid::new_v4().to_string());
        let fresh = base.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&stale).expect("stale dir");
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700))
                .expect("owner-private base");
        }
        std::fs::write(stale.join("old.png"), b"old").expect("stale file");
        std::fs::create_dir_all(&fresh).expect("fresh dir");
        let old = std::time::SystemTime::now()
            - std::time::Duration::from_millis(ATTACHMENT_UNREFERENCED_TTL_MS + 60_000);
        std::fs::File::open(&stale)
            .expect("open stale dir")
            .set_modified(old)
            .expect("age stale dir");

        let removed = reference_prune_staging_root(&base, ATTACHMENT_UNREFERENCED_TTL_MS);

        assert!(
            !stale.exists(),
            "a staging directory older than the TTL must be removed after a restart"
        );
        assert!(fresh.exists(), "a fresh staging directory must be kept");
        assert_eq!(removed, 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// rs-files-routes-r3-1: a staging base that is a symlink is never followed by the TTL
    /// prune, so stale UUID-named directories outside the worktree survive.
    #[cfg(unix)]
    #[test]
    fn the_ttl_prune_never_follows_a_symlinked_staging_base() {
        let outside = scratch_dir();
        let victim = outside.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&victim).expect("outside dir");
        std::fs::write(victim.join("keep.txt"), b"not ours").expect("outside file");
        let old = std::time::SystemTime::now()
            - std::time::Duration::from_millis(ATTACHMENT_UNREFERENCED_TTL_MS + 60_000);
        std::fs::File::open(&victim)
            .expect("open outside dir")
            .set_modified(old)
            .expect("age outside dir");
        let worktree = scratch_dir();
        std::fs::create_dir_all(&worktree).expect("worktree");
        let base = reference_staging_base_dir(&worktree);
        std::os::unix::fs::symlink(&outside, &base).expect("plant symlink base");

        let removed = reference_prune_staging_root(&base, ATTACHMENT_UNREFERENCED_TTL_MS);

        let survived = victim.exists();
        let _ = std::fs::remove_dir_all(&worktree);
        let _ = std::fs::remove_dir_all(&outside);
        assert!(survived, "a prune must not delete through a symlinked staging base");
        assert_eq!(removed, 0);
    }

    /// rs-files-routes-9: a staging base this user does not exclusively own is refused, whether
    /// it is a symlink planted by someone else or a directory other users can write into.
    #[cfg(unix)]
    #[test]
    fn a_staging_base_that_is_a_symlink_or_shared_is_refused() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = scratch_dir();
        let elsewhere = scratch_dir();
        std::fs::create_dir_all(&root).expect("root");
        std::fs::create_dir_all(&elsewhere).expect("elsewhere");
        std::os::unix::fs::symlink(&elsewhere, root.join(REFERENCE_STAGING_DIR_NAME))
            .expect("plant symlink");
        assert!(
            stage_reference_file_in("host-test", &root, &payload("shot.png", b"x")).is_err(),
            "a staging base that is a symlink must be refused"
        );
        assert_eq!(
            std::fs::read_dir(&elsewhere).expect("read elsewhere").count(),
            0,
            "nothing may be written through a planted symlink"
        );

        let shared_root = scratch_dir();
        let shared_base = shared_root.join(REFERENCE_STAGING_DIR_NAME);
        std::fs::create_dir_all(&shared_base).expect("shared base");
        std::fs::set_permissions(&shared_base, std::fs::Permissions::from_mode(0o777))
            .expect("chmod 0777");
        assert!(
            stage_reference_file_in("host-test", &shared_root, &payload("shot.png", b"x"))
                .is_err(),
            "a staging base other users can write into must be refused"
        );

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&elsewhere);
        let _ = std::fs::remove_dir_all(&shared_root);
    }

    #[test]
    fn routes_and_ttl_reuse_the_frozen_contract() {
        assert_eq!(reference_files_route("sess-1"), "/api/v1/reference-chat/sess-1/files");
        assert_eq!(
            reference_file_route("sess-1", "att-9"),
            "/api/v1/reference-chat/sess-1/files/att-9"
        );
        assert_eq!(reference_unreferenced_ttl_ms(), ATTACHMENT_UNREFERENCED_TTL_MS);
        assert_eq!(reference_staging_failure_code(), ScopeErrorCode::Unsupported);
    }
}
