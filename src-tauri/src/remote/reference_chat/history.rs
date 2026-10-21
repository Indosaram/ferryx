//! Reference-chat history acquisition, paging and generation (plan task 3).
//!
//! Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918`
//! (MIT, see `docs/chat/HERDR_LICENSE`). Upstream anchors, read at the pinned revision:
//!
//! | Upstream | What this module ports |
//! |---|---|
//! | `server/conversation.ts` (`turnStarts`, `opensTurn`, `history_id`, `pageBefore`, `newestPage`, `historyChain`/`liveChain`) | the pager: how one page of a store is cut, and what a cursor names |
//! | `server/conversation.ts` session discovery, `server/omo.ts`, `server/pi.ts`, `server/gjc-runtime.ts` | the exact-owner rules: a reader-identified session id, the agent's own session id, or a cwd this pane owns alone |
//! | `src/lib/api.ts` ETag/`version` cache, `ChatView.tsx` identity reset | the `generation` fence, so a response whose answer could have changed is never painted |
//!
//! Frozen contract: `docs/chat/herdr-port-contract.md` sections 4 and 6. This module owns the
//! byte-level page machinery the contract names (`ReferenceHistoryPage`, `ReferenceHistoryCursor`)
//! and nothing else: it declares no shared type, registers no route and edits no sibling lane.
//!
//! ## What "exact owner" means here
//!
//! A page is served for one target — owning host + backend session + daemon incarnation +
//! provider/native session identity where the reader identified one — and the resolution below
//! either binds that target's own store or refuses. It never picks a file by recency:
//!
//! * An identity-exact hit ([`crate::agent_transcript::transcript_path_for_session`] over the
//!   provider session id, the agent's own session id, or the backend session id) is bound.
//! * The cwd is used **only** when no other live session shares it and it holds exactly one
//!   candidate. A shared cwd, or several candidates, is `CONTROL_CONFLICT` — "the newest file in
//!   this directory" is how one pane would show another pane's conversation, which IS-02 forbids.
//! * A reader-identified session with nothing written yet is `notStarted` (a conversation with
//!   zero turns, not a missing one). Nothing identified at all is `NOT_FOUND`.
//! * A registry entry the reference has no reader for is `scrollback`: a legitimate primary
//!   result, labelled as this pane's own output, never an invented conversation.
//!
//! ## Boundaries, recorded rather than silently exceeded
//!
//! * **No provider process is spawned, and no provider RPC is issued.** Acquisition reads the
//!   owning host's own session store, exactly as the reference does; a pane's program is never
//!   asked for its history.
//! * **No root is guessed from the registry label.** The identity-exact lookup uses Ferryx's
//!   existing `.omo` session root ([`crate::agent_transcript`]); a store under a root Ferryx does
//!   not know (Claude's `~/.claude/projects`, `.pi`, `.omp`) is reached through the absolute
//!   `provider_transcript_path` the provider record already resolved, never through a guess.
//! * **A page is cut at turn boundaries**, so it never splits a turn. `limit` counts boundaries —
//!   each boundary opens a human turn, and the page carries that turn and everything that followed
//!   it up to the next boundary — so a page may hold more turns than `limit` (a tool result, a
//!   background-task wake, a notice or a compaction are turns no boundary opens). This mirrors the
//!   pinned reader, whose `turnStarts` are exactly the lines `opensTurn` accepts.
//! * **pi's stream is its projected active branch**, not its raw bytes: the store is an entry tree
//!   and `/tree` leaves entries a page can no longer reach. A tree the reader cannot project is
//!   disclosed as same-pane output ([`super::history_pi::PI_BRANCH_UNREADABLE`]), never approximated.
//! * **A cursor is never re-anchored.** One minted against another stream, past the stream's end
//!   (a rotation/truncation) or below the readable window is refused with `TARGET_EXPIRED`.
//! * **`generation` is computed over the whole stream, never the page**, so pagination cannot
//!   rotate it; an append cannot either; a prefix rewrite, a truncation, a head removal or a
//!   provider change can. That is the fence task 4 discards a late response with.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use sha2::{Digest, Sha256};

use crate::scoped_contracts::{ScopeError, ScopeErrorCode};

use super::types::{
    ReferenceHistoryAvailability, ReferenceHistoryCursor, ReferenceHistoryPage,
    ReferenceHistorySource, ReferenceNativeHistoryKind, ReferenceTargetRef, ReferenceTurn,
    REFERENCE_SCROLLBACK_DISCLOSURE,
};

/// The page size a caller gets when it names none (the reference's own default window).
pub const REFERENCE_HISTORY_DEFAULT_LIMIT: usize = 200;

/// The largest page a caller may ask for. A transcript is megabytes and the client polls, so an
/// unbounded page would put the whole store on the wire on every refresh.
pub const REFERENCE_HISTORY_MAX_LIMIT: usize = 1000;

/// How many bytes of a linear store a reader may hold at once. A real transcript measured 11 MB
/// over 3730 lines, so the newest window is read and the partial first line is dropped rather than
/// handing the parser a torn record. Mirrors the ceiling `server.rs` clamps its own read to.
pub const REFERENCE_HISTORY_MAX_BYTES: usize = 4 * 1024 * 1024;

/// How many bytes of a pi session file may be read. pi's branch cannot be projected from a tail
/// window — the entry tree links by id, so the whole file is needed — and this is the same cap the
/// pi lane refuses a branch past ([`super::history_pi::PI_MAX_BRANCH_BYTES`]).
pub const REFERENCE_HISTORY_MAX_PI_BYTES: usize = super::history_pi::PI_MAX_BRANCH_BYTES;

/// The disclosure a page carries when its store exists but cannot be projected as a conversation.
pub const REFERENCE_HISTORY_UNREADABLE_REASON: &str =
    "the session store could not be read as a conversation";

fn history_error(code: ScopeErrorCode, message: impl Into<String>) -> ScopeError {
    ScopeError {
        code,
        message: message.into(),
        // A read is idempotent: retrying it cannot duplicate anything, but a refusal is a refusal.
        retryable: false,
        details: serde_json::Value::Null,
    }
}

/// Everything the acquisition layer needs to bind a page to exactly one owner.
///
/// The provider/native session identity is **not** duplicated here: it is
/// [`ReferenceTargetRef::provider_session_id`], so the target the route authorized and the target
/// the store was resolved for are the same value by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceHistoryIdentity {
    /// The authorized target: owning host + backend session + daemon incarnation.
    pub target: ReferenceTargetRef,
    /// The registry entry the pane runs, as the inventory publishes it (`omo`, `codex`, `pi`, …).
    /// This is the only label→reader mapping the contract allows
    /// ([`ReferenceNativeHistoryKind::from_registry_id`]), never a terminal title.
    pub registry_id: String,
    /// The pane's cwd, when the daemon knows it. Used only for the cwd-unique fallback.
    pub cwd: Option<String>,
    /// An absolute transcript path the provider record already resolved on the owning host.
    pub provider_transcript_path: Option<PathBuf>,
    /// The agent's own session id for this pane, when the daemon knows it (a handover daemon
    /// knows no agent's conversation until that agent next changes state).
    pub agent_session_id: Option<String>,
}

impl ReferenceHistoryIdentity {
    /// An identity with only a target and a registry label.
    pub fn new(target: ReferenceTargetRef, registry_id: impl Into<String>) -> Self {
        Self {
            target,
            registry_id: registry_id.into(),
            cwd: None,
            provider_transcript_path: None,
            agent_session_id: None,
        }
    }

    /// The identity of a pane whose cwd the daemon knows.
    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// The identity of a pane whose provider record resolved an absolute transcript path.
    pub fn with_provider_transcript_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.provider_transcript_path = Some(path.into());
        self
    }

    /// The identity of a pane whose agent session id the daemon knows.
    pub fn with_agent_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.agent_session_id = Some(session_id.into());
        self
    }

    /// The provider/native session identity, where the reader identified one.
    pub fn provider_session_id(&self) -> Option<&str> {
        self.target.provider_session_id.as_deref().filter(|id| !id.trim().is_empty())
    }

    /// The reader this pane's registry entry maps to.
    pub fn native_kind(&self) -> ReferenceNativeHistoryKind {
        ReferenceNativeHistoryKind::from_registry_id(&self.registry_id)
    }

    /// The session ids that identify this pane's store exactly, most specific first.
    ///
    /// The provider session identity leads (it is what the reader identified), then the agent's
    /// own session id, then the backend session id — which is the agent session id in the common
    /// case and is why the existing reader looks it up too. The backend id is only a stand-in: it
    /// is used when neither the provider nor the agent session is known (a known, unwritten
    /// session must never bind an older session's store), and only for the `.omo` reader, the
    /// only store that lives under the root the lookup searches.
    fn exact_session_ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = Vec::new();
        if let Some(id) = self.provider_session_id() {
            ids.push(id);
        }
        if let Some(id) = self.agent_session_id.as_deref().filter(|id| !id.trim().is_empty()) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        if ids.is_empty() && self.native_kind() == ReferenceNativeHistoryKind::Omo {
            ids.push(self.target.target.backend_session_id.as_str());
        }
        ids
    }
}

/// Another live session on the same host, as far as ownership of a cwd goes.
///
/// Only the session id and the cwd are needed: whether a cwd belongs to one pane or several is
/// decided by the live sessions that share it, not by which transcript is newest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceHistorySibling {
    pub session_id: String,
    pub cwd: Option<String>,
}

impl ReferenceHistorySibling {
    /// A live session that may or may not share a cwd with the pane being resolved.
    pub fn new(session_id: impl Into<String>, cwd: Option<String>) -> Self {
        Self { session_id: session_id.into(), cwd }
    }
}

/// Where the resolved stream's bytes are, or why there are none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceHistoryLocation {
    /// A store on the owning host that this reader may read.
    File(PathBuf),
    /// No native reader: the pane's own output is the conversation (task 6 supplies it).
    Scrollback,
    /// A reader-identified session the agent has not written a turn to yet.
    NotStarted,
}

/// The store one target's page is served from, and the identity a cursor is minted against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceHistoryStream {
    /// The reader family the page is labelled with.
    pub source: ReferenceHistorySource,
    /// The reader this stream belongs to. `Unavailable` means there is no native stream at all.
    pub kind: ReferenceNativeHistoryKind,
    /// How honestly the page's turns are sourced.
    pub availability: ReferenceHistoryAvailability,
    /// Identity of this transcript stream: what a cursor names, and what a foreign cursor is
    /// refused against. Opaque — it never carries an absolute path to the client.
    pub stream_id: String,
    /// Where the bytes are.
    pub location: ReferenceHistoryLocation,
    /// The disclosure the page carries when `availability` is not `native`.
    pub unavailable_reason: Option<String>,
    /// The exact target this stream was resolved for. Every later read is checked against it, so a
    /// page cannot be served for a target the stream does not belong to.
    pub owner: ReferenceTargetRef,
}

impl ReferenceHistoryStream {
    /// Is this an exact native transcript?
    pub fn is_native(&self) -> bool {
        self.availability == ReferenceHistoryAvailability::Native
    }

    /// The file this stream reads, when it has one.
    pub fn file_path(&self) -> Option<&Path> {
        match &self.location {
            ReferenceHistoryLocation::File(path) => Some(path.as_path()),
            _ => None,
        }
    }
}

/// The source a reader family labels its pages with.
pub fn reference_history_source_for_kind(kind: ReferenceNativeHistoryKind) -> ReferenceHistorySource {
    match kind {
        ReferenceNativeHistoryKind::Claude => ReferenceHistorySource::ClaudeTranscript,
        ReferenceNativeHistoryKind::Codex => ReferenceHistorySource::CodexTranscript,
        ReferenceNativeHistoryKind::Omp => ReferenceHistorySource::OmpTranscript,
        ReferenceNativeHistoryKind::Omo => ReferenceHistorySource::OmoTranscript,
        ReferenceNativeHistoryKind::Gjc => ReferenceHistorySource::GjcTranscript,
        ReferenceNativeHistoryKind::Pi => ReferenceHistorySource::PiTranscript,
        ReferenceNativeHistoryKind::Unavailable => ReferenceHistorySource::Scrollback,
    }
}

/// The opaque identity of one target's stream: the target tuple, the reader, and the store.
///
/// The store's path is hashed, never carried: a cursor is client-visible and an absolute owning
/// host path must not cross the wire. A different store (a rotation, a re-resolution to another
/// file) mints a different identity, so a cursor from the old one is refused rather than
/// re-anchored onto the new one.
pub fn reference_history_stream_id(
    identity: &ReferenceHistoryIdentity,
    kind: ReferenceNativeHistoryKind,
    store: Option<&Path>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(identity.target.target.host_id.as_bytes());
    hasher.update(b"|");
    hasher.update(identity.target.target.owner_id.as_bytes());
    hasher.update(b"|");
    hasher.update(identity.target.target.epoch.0.to_string().as_bytes());
    hasher.update(b"|");
    hasher.update(identity.target.target.backend_session_id.as_bytes());
    hasher.update(b"|");
    hasher.update(kind.as_str().as_bytes());
    if let Some(provider) = identity.provider_session_id() {
        hasher.update(b"|p:");
        hasher.update(provider.as_bytes());
    }
    if let Some(store) = store {
        hasher.update(b"|s:");
        hasher.update(store.to_string_lossy().as_bytes());
    }
    let digest = format!("{:x}", hasher.finalize());
    format!("{}:{}", kind.as_str(), &digest[..16])
}

/// Resolve the store one target's page must be read from.
///
/// Returns the stream to read, or a typed refusal — never a different session's file:
///
/// * `FORBIDDEN` — the target names a provider session that is not a session id, or names an
///   absolute transcript path that is not that session's store, or is not a usable path at all.
/// * `CONTROL_CONFLICT` — the pane's cwd is shared by another live session, or holds more than one
///   candidate store, so no reader can say which one is this pane's.
/// * `NOT_FOUND` — nothing identifies a store for this pane at all.
/// * `UNAUTHORIZED` — the target names no backend session.
pub fn resolve_reference_history_stream(
    home: &Path,
    identity: &ReferenceHistoryIdentity,
    siblings: &[ReferenceHistorySibling],
) -> Result<ReferenceHistoryStream, ScopeError> {
    let kind = identity.native_kind();
    let owner = identity.target.clone();

    // A registry entry the reference has no reader for is same-pane output, disclosed as such.
    // It is a legitimate primary result, never a failure and never an invented conversation.
    if !kind.is_native() {
        return Ok(ReferenceHistoryStream {
            source: ReferenceHistorySource::Scrollback,
            kind,
            availability: ReferenceHistoryAvailability::Scrollback,
            stream_id: reference_history_stream_id(identity, kind, None),
            location: ReferenceHistoryLocation::Scrollback,
            unavailable_reason: Some(format!(
                "`{}` has no native transcript reader in the reference; showing this pane's own output",
                identity.registry_id.trim()
            )),
            owner,
        });
    }

    validate_reference_identity(identity)?;

    // 1. The absolute path the provider record already resolved. It is authoritative: if it names
    //    a file, that file is this session's store; if it names nothing, the session is unwritten
    //    and no other file may be substituted for it.
    if let Some(path) = identity.provider_transcript_path.as_deref() {
        if path.is_file() {
            verify_named_transcript_identity(identity, path)?;
            return Ok(native_stream(identity, kind, path));
        }
        if identity.provider_session_id().is_some() {
            return Ok(not_started_stream(identity, kind));
        }
    }

    // 2-4. Identity-exact lookups over the owning host's own session root.
    for session_id in identity.exact_session_ids() {
        if let Some(path) = crate::agent_transcript::transcript_path_for_session(home, session_id) {
            return Ok(native_stream(identity, kind, &path));
        }
    }

    // A reader-identified session with nothing written yet is a conversation with zero turns,
    // which is a different answer from "this pane has no store at all". It is answered before the
    // cwd fallback: a known session must never be bound to another (older) session's file.
    if identity.provider_session_id().is_some()
        || identity.agent_session_id.is_some()
        || identity.provider_transcript_path.is_some()
    {
        return Ok(not_started_stream(identity, kind));
    }

    // 5. The cwd, only when nothing identifies the session, this pane owns the cwd alone, it
    //    holds exactly one candidate, and the candidate store is this reader's (`.omo`) store.
    // omp and gjc keep their stores elsewhere, so an `.omo` file in the cwd is never theirs.
    let reads_omo_store = kind == ReferenceNativeHistoryKind::Omo;
    if let Some(cwd) = identity
        .cwd
        .as_deref()
        .filter(|cwd| reads_omo_store && !cwd.trim().is_empty())
    {
        let backend = identity.target.target.backend_session_id.as_str();
        if let Some(other) = siblings
            .iter()
            .find(|sibling| sibling.session_id != backend && sibling.cwd.as_deref() == Some(cwd))
        {
            return Err(history_error(
                ScopeErrorCode::ControlConflict,
                format!(
                    "another live session (`{}`) shares this pane's cwd; refusing to pick a \
                     transcript for it",
                    other.session_id
                ),
            ));
        }
        let candidates = reference_history_cwd_candidates(home, cwd);
        match candidates.len() {
            0 => {}
            1 => return Ok(native_stream(identity, kind, &candidates[0])),
            count => {
                return Err(history_error(
                    ScopeErrorCode::ControlConflict,
                    format!(
                        "{count} transcripts exist in this pane's cwd and none matches its \
                         identity; the owner is ambiguous"
                    ),
                ))
            }
        }
    }

    Err(history_error(
        ScopeErrorCode::NotFound,
        format!(
            "nothing identifies a `{}` transcript for this session",
            identity.registry_id.trim()
        ),
    ))
}

/// Refuse a stream that does not belong to the target being served.
///
/// Every read runs this before it touches bytes: a page must not be served for a target the
/// stream was not resolved for, and a daemon incarnation change invalidates the resolution rather
/// than silently re-binding it.
pub fn ensure_reference_history_owner(
    stream: &ReferenceHistoryStream,
    identity: &ReferenceHistoryIdentity,
) -> Result<(), ScopeError> {
    let owner = &stream.owner.target;
    let requested = &identity.target.target;
    if owner.host_id != requested.host_id
        || owner.owner_id != requested.owner_id
        || owner.epoch != requested.epoch
        || owner.backend_session_id != requested.backend_session_id
    {
        return Err(history_error(
            ScopeErrorCode::TargetExpired,
            format!(
                "this stream was resolved for backend session `{}` at epoch {} on host `{}`",
                owner.backend_session_id, owner.epoch.0, owner.host_id
            ),
        ));
    }
    if stream.owner.provider_session_id != identity.target.provider_session_id {
        return Err(history_error(
            ScopeErrorCode::Forbidden,
            "this stream was resolved for another provider session",
        ));
    }
    if stream.kind != identity.native_kind() {
        return Err(history_error(
            ScopeErrorCode::Forbidden,
            format!(
                "this stream was resolved for the `{}` reader",
                stream.kind.as_str()
            ),
        ));
    }
    Ok(())
}

fn validate_reference_identity(identity: &ReferenceHistoryIdentity) -> Result<(), ScopeError> {
    if identity.target.target.backend_session_id.trim().is_empty() {
        return Err(history_error(
            ScopeErrorCode::Unauthorized,
            "the target names no backend session",
        ));
    }
    if let Some(provider) = identity.target.provider_session_id.as_deref() {
        if !crate::agent_transcript::is_valid_session_id(provider) {
            return Err(history_error(
                ScopeErrorCode::Forbidden,
                format!("`{provider}` is not a provider session id"),
            ));
        }
    }
    if let Some(agent) = identity.agent_session_id.as_deref() {
        if !crate::agent_transcript::is_valid_session_id(agent) {
            return Err(history_error(
                ScopeErrorCode::Forbidden,
                format!("`{agent}` is not an agent session id"),
            ));
        }
    }
    if let Some(path) = identity.provider_transcript_path.as_deref() {
        if !path.is_absolute() || path.components().any(|part| part.as_os_str() == "..") {
            return Err(history_error(
                ScopeErrorCode::Forbidden,
                "the named transcript path is not a usable absolute path",
            ));
        }
    }
    Ok(())
}

/// A named transcript must be the target's own store.
///
/// The provider record is authoritative about *where* a store is, but not about *whose* it is:
/// a path whose file name encodes another session's id is a wrong identity, and serving it would
/// show one pane another session's conversation.
fn verify_named_transcript_identity(
    identity: &ReferenceHistoryIdentity,
    path: &Path,
) -> Result<(), ScopeError> {
    let Some(provider) = identity.provider_session_id() else {
        return Ok(());
    };
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Err(history_error(
            ScopeErrorCode::Forbidden,
            "the named transcript has no file name to check its identity against",
        ));
    };
    let expected = format!("{provider}.jsonl");
    if !name.ends_with(&expected) {
        return Err(history_error(
            ScopeErrorCode::Forbidden,
            format!("`{name}` is not the store of provider session `{provider}`"),
        ));
    }
    Ok(())
}

fn native_stream(
    identity: &ReferenceHistoryIdentity,
    kind: ReferenceNativeHistoryKind,
    path: &Path,
) -> ReferenceHistoryStream {
    ReferenceHistoryStream {
        source: reference_history_source_for_kind(kind),
        kind,
        availability: ReferenceHistoryAvailability::Native,
        stream_id: reference_history_stream_id(identity, kind, Some(path)),
        location: ReferenceHistoryLocation::File(path.to_path_buf()),
        unavailable_reason: None,
        owner: identity.target.clone(),
    }
}

fn not_started_stream(
    identity: &ReferenceHistoryIdentity,
    kind: ReferenceNativeHistoryKind,
) -> ReferenceHistoryStream {
    ReferenceHistoryStream {
        source: reference_history_source_for_kind(kind),
        kind,
        availability: ReferenceHistoryAvailability::NotStarted,
        stream_id: reference_history_stream_id(identity, kind, None),
        location: ReferenceHistoryLocation::NotStarted,
        unavailable_reason: Some(
            "the agent holds a session it has not written a turn to yet".to_string(),
        ),
        owner: identity.target.clone(),
    }
}

/// The candidate stores in a cwd's own session folder, in a deterministic order.
///
/// Deliberately **not** ordered by mtime: tool activity is not evidence of pane ownership, and a
/// recency pick is how a pane would show another session's conversation. The caller accepts a
/// single candidate and refuses several.
fn reference_history_cwd_candidates(home: &Path, cwd: &str) -> Vec<PathBuf> {
    let folder = home
        .join(".omo")
        .join("agent")
        .join("sessions")
        .join(super::history_omo::omo_session_folder(cwd));
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return Vec::new();
    };
    let mut candidates: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("jsonl"))
        .filter(|path| {
            use std::io::BufRead as _;

            let Ok(file) = File::open(path) else {
                return false;
            };
            // The folder slug is lossy. Only an exact cwd in the session header establishes
            // ownership; bound the header read just like the transcript window read.
            let mut reader = std::io::BufReader::new(file.take(REFERENCE_HISTORY_MAX_BYTES as u64));
            let mut first_line = String::new();
            reader.read_line(&mut first_line).is_ok()
                && super::history_omo::omo_session_header(&first_line, cwd).is_some()
        })
        .collect();
    candidates.sort();
    candidates
}

/// The bytes of one stream, and the stream offset they begin at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceHistoryWindow {
    /// The absolute offset in the stream the bytes begin at. `0` for a whole-store read.
    pub start: u64,
    pub bytes: Vec<u8>,
}

/// Read one bounded window of a store. Synchronous by design: every caller runs it through
/// [`crate::ipc::run_blocking`].
pub fn read_reference_history_window(
    path: &Path,
    kind: ReferenceNativeHistoryKind,
) -> Result<ReferenceHistoryWindow, ScopeError> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        history_error(
            ScopeErrorCode::NotFound,
            format!("{}: {error}", path.display()),
        )
    })?;
    if !metadata.is_file() {
        return Err(history_error(
            ScopeErrorCode::NotFound,
            format!("{} is not a file", path.display()),
        ));
    }
    let length = metadata.len();

    // pi's store is an entry tree: the active branch is the path from the last entry written back
    // to its root, and that path cannot be projected from a tail window. The whole file is read,
    // and a file past the branch cap is refused rather than projected from bytes nobody saw.
    if kind == ReferenceNativeHistoryKind::Pi {
        if length > REFERENCE_HISTORY_MAX_PI_BYTES as u64 {
            return Err(history_error(
                ScopeErrorCode::PayloadTooLarge,
                format!(
                    "{} is {length} bytes, past the {} byte pi read cap",
                    path.display(),
                    REFERENCE_HISTORY_MAX_PI_BYTES
                ),
            ));
        }
        let bytes = std::fs::read(path).map_err(|error| {
            history_error(
                ScopeErrorCode::NotFound,
                format!("{}: {error}", path.display()),
            )
        })?;
        return Ok(ReferenceHistoryWindow { start: 0, bytes });
    }

    let cap = REFERENCE_HISTORY_MAX_BYTES as u64;
    if length <= cap {
        let bytes = std::fs::read(path).map_err(|error| {
            history_error(
                ScopeErrorCode::NotFound,
                format!("{}: {error}", path.display()),
            )
        })?;
        return Ok(ReferenceHistoryWindow { start: 0, bytes });
    }

    // Read one byte before the window too, so a window that begins exactly on a record boundary
    // keeps that record instead of discarding it as a torn head.
    let start = length - cap - 1;
    let mut file = File::open(path).map_err(|error| {
        history_error(
            ScopeErrorCode::NotFound,
            format!("{}: {error}", path.display()),
        )
    })?;
    file.seek(SeekFrom::Start(start)).map_err(|error| {
        history_error(
            ScopeErrorCode::NotFound,
            format!("{}: {error}", path.display()),
        )
    })?;
    let mut bytes: Vec<u8> = Vec::with_capacity(cap as usize + 1);
    file.take(cap + 1).read_to_end(&mut bytes).map_err(|error| {
        history_error(
            ScopeErrorCode::NotFound,
            format!("{}: {error}", path.display()),
        )
    })?;

    // Drop everything through the first line break: when the byte before the window is one, that
    // is only that byte; otherwise it is the torn head of a record, which the parser must never
    // see. A window with no line break at all holds nothing that can be read.
    match bytes.iter().position(|byte| *byte == b'\n') {
        Some(index) => Ok(ReferenceHistoryWindow {
            start: start + index as u64 + 1,
            bytes: bytes.split_off(index + 1),
        }),
        None => Ok(ReferenceHistoryWindow {
            start: length,
            bytes: Vec::new(),
        }),
    }
}

/// The last `max` bytes of a store, read without holding the rest.
///
/// Synchronous by design: every caller runs it through [`crate::ipc::run_blocking`].
pub fn read_reference_history_tail<R: Read + Seek>(
    reader: &mut R,
    max: usize,
) -> std::io::Result<Vec<u8>> {
    let length = reader.seek(SeekFrom::End(0))?;
    let start = length.saturating_sub(max as u64);
    reader.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity((length - start) as usize);
    reader.by_ref().take(max as u64).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// [`read_reference_history_tail`] over a file on the owning host.
pub fn read_reference_history_tail_file(path: &Path, max: usize) -> std::io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    read_reference_history_tail(&mut file, max)
}

/// Does this raw line open a turn?
///
/// Each family's own `opensTurn` predicate decides, so a page never starts mid-turn. omp, gjc and
/// pi are read through the pinned reader that reads omo's record shape
/// (`transcript-records.ts` `parseOmpTranscript`), so they share omo's boundary predicate.
pub fn reference_history_line_opens_turn(
    kind: ReferenceNativeHistoryKind,
    line: &str,
) -> bool {
    match kind {
        ReferenceNativeHistoryKind::Claude => super::history_claude::opens_claude_turn(line),
        ReferenceNativeHistoryKind::Codex => super::history_parse::codex_line_opens_turn(line),
        ReferenceNativeHistoryKind::Omo => super::history_omo::omo_line_opens_turn(line),
        ReferenceNativeHistoryKind::Omp
        | ReferenceNativeHistoryKind::Gjc
        | ReferenceNativeHistoryKind::Pi => super::history_omo::omo_line_opens_turn(line),
        ReferenceNativeHistoryKind::Unavailable => false,
    }
}

/// The byte offsets of the lines that open a turn, ascending.
///
/// Offsets are taken over the raw bytes (never over a lossily decoded string) so a cursor minted
/// from one of them names a real position in the stream the caller holds.
pub fn reference_history_turn_starts(kind: ReferenceNativeHistoryKind, bytes: &[u8]) -> Vec<usize> {
    let mut starts: Vec<usize> = Vec::new();
    let mut line_start = 0usize;
    while line_start < bytes.len() {
        let line_end = bytes[line_start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| line_start + index)
            .unwrap_or(bytes.len());
        let mut line = &bytes[line_start..line_end];
        if line.last() == Some(&b'\r') {
            line = &line[..line.len() - 1];
        }
        if !line.is_empty() {
            let text = String::from_utf8_lossy(line);
            if reference_history_line_opens_turn(kind, text.trim()) {
                starts.push(line_start);
            }
        }
        line_start = line_end + 1;
    }
    starts
}

/// A position in pi's branch view as its offset in the file.
fn pi_view_to_file(segments: &[super::history_pi::PiTranscriptSegment], view_offset: usize) -> usize {
    let mut base = 0usize;
    for segment in segments {
        let len = segment.end - segment.start;
        if view_offset < base + len {
            return segment.start + (view_offset - base);
        }
        base += len;
    }
    segments.last().map_or(view_offset, |segment| segment.end)
}

/// A file offset as its position in pi's branch view, or `None` when the active branch no longer
/// holds it (the entry it named was left behind by `/tree`).
fn pi_file_to_view(segments: &[super::history_pi::PiTranscriptSegment], file_offset: usize) -> Option<usize> {
    let mut base = 0usize;
    for segment in segments {
        if (segment.start..segment.end).contains(&file_offset) {
            return Some(base + (file_offset - segment.start));
        }
        base += segment.end - segment.start;
    }
    (segments.last().map(|segment| segment.end) == Some(file_offset)).then_some(base)
}

/// The bytes of pi's projected active branch, concatenated in order.
fn branch_bytes(bytes: &[u8], segments: &[super::history_pi::PiTranscriptSegment]) -> Vec<u8> {
    let mut branch: Vec<u8> = Vec::new();
    for segment in segments {
        if let Some(slice) = bytes.get(segment.start..segment.end) {
            branch.extend_from_slice(slice);
        }
    }
    branch
}

fn parse_reference_history_turns(
    kind: ReferenceNativeHistoryKind,
    bytes: &[u8],
) -> Result<Vec<ReferenceTurn>, ScopeError> {
    match kind {
        // pi's byte-level entry point is the branch projection plus the record loop; the text it
        // is handed here is already the projected branch.
        ReferenceNativeHistoryKind::Pi => {
            super::history_pi::parse_pi_history(kind, &String::from_utf8_lossy(bytes))
                .map_err(|error| history_error(ScopeErrorCode::Unsupported, error))
        }
        _ => super::history_parse::dispatch_reference_history(kind, bytes)
            .map_err(|error| history_error(ScopeErrorCode::Unsupported, error)),
    }
}

/// The stable fingerprint of one turn, over its canonical wire JSON.
fn reference_turn_fingerprint(turn: &ReferenceTurn) -> String {
    let mut hasher = Sha256::new();
    match serde_json::to_vec(turn) {
        Ok(json) => hasher.update(&json),
        // A turn that cannot be serialized still gets a fingerprint, so it is never mistaken for a
        // neighbour: the debug rendering is stable for a stable value.
        Err(_) => hasher.update(format!("{turn:?}").as_bytes()),
    }
    format!("{:x}", hasher.finalize())
}

/// The genesis generation of a stream whose provider session identity is not known.
///
/// Stable for as long as the stream's own first turn is unchanged, so an append cannot rotate it
/// and a rewritten head cannot fail to.
pub fn reference_history_genesis_generation(
    stream_id: &str,
    first: Option<&ReferenceTurn>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(stream_id.as_bytes());
    if let Some(turn) = first {
        hasher.update(b":first:");
        hasher.update(reference_turn_fingerprint(turn).as_bytes());
    }
    let digest = format!("{:x}", hasher.finalize());
    format!("{stream_id}:{}", &digest[..16])
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReferenceStreamState {
    provider_session_id: Option<String>,
    fingerprints: Vec<String>,
    incarnation: u64,
    /// The stream offset the observed turns were read from (`0` for a whole-store read).
    window_start: u64,
    /// When this stream was last observed, for least-recently-observed eviction.
    last_seen: u64,
}

#[derive(Debug, Default)]
struct ReferenceTrackerInner {
    streams: HashMap<String, ReferenceStreamState>,
    clock: u64,
}

/// Does `current` continue `previous` after its head moved forward? True when some non-empty
/// suffix of the previous turns is exactly the start of the current ones. The last previous turn
/// may still be growing, so it is only required to still be there.
fn reference_history_head_shifted(previous: &[String], current: &[String]) -> bool {
    (1..previous.len()).any(|drop| {
        let kept = &previous[drop..];
        let closed = kept.len() - 1;
        current.len() >= kept.len() && current[..closed] == kept[..closed]
    })
}

/// Tracks a stream's incarnation so `generation` changes exactly when the answer could have.
///
/// The invariants, each one a case the fence must get right:
///
/// 1. **Pagination-independent** — the token is computed over the whole stream, never a page, so
///    asking for an older page cannot rotate it.
/// 2. **Append preserves** — new turns extending the known prefix leave the token identical, so a
///    growing transcript does not make the client discard a page it just rendered.
/// 3. **Prefix rewrite rotates** — a rewritten existing turn (even with an identical first turn)
///    rotates it.
/// 4. **Truncation rotates** — a stream shorter than the known one rotates it.
/// 5. **Head removal rotates** — dropping the first turn shifts the chain and rotates it.
/// 6. **Provider change rotates** — a different provider session identity rotates it.
///
/// A bounded window whose head moved forward (the store outgrew the read cap and the oldest turns
/// fell out of the window) is an append, not a head removal: the turns it still holds continue the
/// known ones, so the token is kept. At most [`REFERENCE_HISTORY_MAX_TRACKED_STREAMS`] streams are
/// remembered; the least recently observed one is forgotten first.
#[derive(Debug, Default)]
pub struct ReferenceHistoryGenerationTracker {
    inner: Mutex<ReferenceTrackerInner>,
}

impl ReferenceHistoryGenerationTracker {
    /// A tracker with no observed stream.
    pub fn new() -> Self {
        Self::default()
    }

    /// Observe a stream's whole turn list and answer its current generation.
    pub fn observe(
        &self,
        stream_id: &str,
        provider_session_id: Option<&str>,
        turns: &[ReferenceTurn],
    ) -> String {
        self.observe_window(stream_id, provider_session_id, 0, turns)
    }

    /// Observe the turns of a stream read from `window_start` onward.
    pub fn observe_window(
        &self,
        stream_id: &str,
        provider_session_id: Option<&str>,
        window_start: u64,
        turns: &[ReferenceTurn],
    ) -> String {
        let fingerprints: Vec<String> =
            turns.iter().map(reference_turn_fingerprint).collect();
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.clock += 1;
        let now = inner.clock;
        let streams = &mut inner.streams;

        let Some(state) = streams.get_mut(stream_id) else {
            if streams.len() >= REFERENCE_HISTORY_MAX_TRACKED_STREAMS {
                let oldest = streams
                    .iter()
                    .min_by_key(|(_, state)| state.last_seen)
                    .map(|(id, _)| id.clone());
                if let Some(oldest) = oldest {
                    streams.remove(&oldest);
                }
            }
            let incarnation = 1;
            streams.insert(
                stream_id.to_string(),
                ReferenceStreamState {
                    provider_session_id: provider_session_id.map(str::to_string),
                    fingerprints,
                    incarnation,
                    window_start,
                    last_seen: now,
                },
            );
            return format!("{stream_id}:inc-{incarnation}");
        };
        state.last_seen = now;

        if state.provider_session_id.as_deref() != provider_session_id {
            state.provider_session_id = provider_session_id.map(str::to_string);
            state.fingerprints = fingerprints;
            state.window_start = window_start;
            state.incarnation += 1;
            return format!("{stream_id}:inc-{}", state.incarnation);
        }

        let truncated = fingerprints.len() < state.fingerprints.len();
        // The newest observed turn is still open: an agent appends records to it until it settles,
        // so its growth is an append, not a rewrite.
        let closed = state.fingerprints.len().saturating_sub(1);
        let rewritten = state
            .fingerprints
            .iter()
            .take(closed)
            .zip(fingerprints.iter())
            .any(|(previous, current)| previous != current);
        let head_moved = window_start > state.window_start
            && reference_history_head_shifted(&state.fingerprints, &fingerprints);
        if (truncated || rewritten) && !head_moved {
            state.incarnation += 1;
        }
        state.fingerprints = fingerprints;
        state.window_start = window_start;
        format!("{stream_id}:inc-{}", state.incarnation)
    }
}

static REFERENCE_HISTORY_GENERATIONS: OnceLock<ReferenceHistoryGenerationTracker> = OnceLock::new();

/// How many streams the process-wide tracker remembers before it forgets the least recently
/// observed one.
pub const REFERENCE_HISTORY_MAX_TRACKED_STREAMS: usize = 1024;

impl ReferenceHistoryGenerationTracker {
    /// How many streams the tracker currently remembers.
    pub fn tracked_stream_count(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .streams
            .len()
    }
}

/// The process-wide tracker the page reader uses.
pub fn global_reference_history_generation_tracker() -> &'static ReferenceHistoryGenerationTracker {
    REFERENCE_HISTORY_GENERATIONS.get_or_init(ReferenceHistoryGenerationTracker::new)
}

/// The generation of one stream's whole turn list.
///
/// A reader-identified stream is tracked by incarnation, so an append cannot rotate the token; a
/// stream with no identified provider session falls back to its genesis digest.
pub fn reference_history_generation(
    stream_id: &str,
    provider_session_id: Option<&str>,
    turns: &[ReferenceTurn],
) -> String {
    match provider_session_id {
        Some(_) => {
            global_reference_history_generation_tracker().observe(stream_id, provider_session_id, turns)
        }
        None => reference_history_genesis_generation(stream_id, turns.first()),
    }
}

/// The generation of a stream read as a bounded window starting at `window_start`.
///
/// Every native read is tracked by incarnation, whether or not a provider session is known and
/// whether the store was read whole or as a tail window: a tail window's first turn moves with
/// every append, so a genesis digest over it would rotate on every poll, and switching methods
/// when a growing store passes the read cap would rotate the token on a plain append.
fn reference_history_window_generation(
    stream_id: &str,
    provider_session_id: Option<&str>,
    window_start: u64,
    turns: &[ReferenceTurn],
) -> String {
    global_reference_history_generation_tracker().observe_window(
        stream_id,
        provider_session_id,
        window_start,
        turns,
    )
}

/// The byte offset just past the last reset in a stream, if any: an omp-shaped `context_clear`
/// or a Claude `/clear` envelope.
///
/// Every turn before it was cleared, so no page boundary may lie before it.
fn reference_history_reset_end(kind: ReferenceNativeHistoryKind, bytes: &[u8]) -> Option<usize> {
    let marker: &[u8] = match kind {
        ReferenceNativeHistoryKind::Omo
        | ReferenceNativeHistoryKind::Omp
        | ReferenceNativeHistoryKind::Gjc
        | ReferenceNativeHistoryKind::Pi => b"context_clear",
        ReferenceNativeHistoryKind::Claude => b"/clear",
        _ => return None,
    };
    let mut reset = None;
    let mut line_start = 0usize;
    while line_start < bytes.len() {
        let line_end = bytes[line_start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| line_start + index)
            .unwrap_or(bytes.len());
        let line = &bytes[line_start..line_end];
        if line.windows(marker.len()).any(|window| window == marker) {
            let cleared = serde_json::from_slice::<serde_json::Value>(line)
                .map(|entry| match kind {
                    ReferenceNativeHistoryKind::Claude => entry
                        .as_object()
                        .is_some_and(super::history_claude::is_claude_context_clear),
                    _ => super::history_omo::is_omo_context_clear(&entry),
                })
                .unwrap_or(false);
            if cleared {
                reset = Some(line_end);
            }
        }
        line_start = line_end + 1;
    }
    reset
}

/// The task titles OmO's `task` calls recorded in these bytes, so a page inherits the titles its
/// older pages named (as `conversation.ts` carries `taskTitles` across pages).
fn reference_history_task_titles(bytes: &[u8]) -> HashMap<String, String> {
    let mut titles = HashMap::new();
    for line in String::from_utf8_lossy(bytes).split('\n') {
        if !line.contains("\"task\"") {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(message) = super::history_omo::omo_pi_message(&entry) {
            super::history_omo::omo_task_titles(&message, &mut titles);
        }
    }
    titles
}

/// The page size a caller's `limit` resolves to.
///
/// `0` is refused rather than silently raised: a caller that asks for nothing is a caller whose
/// request was not understood. Anything past [`REFERENCE_HISTORY_MAX_LIMIT`] is clamped, because
/// the page bound exists to keep a polled response bounded.
pub fn normalize_reference_history_limit(limit: usize) -> Result<usize, ScopeError> {
    if limit == 0 {
        return Err(history_error(
            ScopeErrorCode::InvalidRequest,
            "a history page must ask for at least one turn boundary",
        ));
    }
    Ok(limit.min(REFERENCE_HISTORY_MAX_LIMIT))
}

/// Serve one page from a store whose bytes are already in hand.
///
/// `window_start` is the stream offset `bytes` begins at (`0` for a whole-store read); a cursor
/// below it names a position this reader no longer holds and is refused rather than re-anchored.
pub fn reference_history_page_from_window(
    stream: &ReferenceHistoryStream,
    identity: &ReferenceHistoryIdentity,
    window_start: u64,
    bytes: &[u8],
    limit: usize,
    cursor: Option<&ReferenceHistoryCursor>,
) -> Result<ReferenceHistoryPage, ScopeError> {
    ensure_reference_history_owner(stream, identity)?;

    let limit = normalize_reference_history_limit(limit)?;

    match stream.availability {
        ReferenceHistoryAvailability::Scrollback => return Ok(scrollback_page(stream)),
        ReferenceHistoryAvailability::NotStarted => return Ok(not_started_page(stream)),
        ReferenceHistoryAvailability::Native => {}
    }

    // pi's stream is its projected active branch; every other family's stream is its own bytes.
    // pi's cursor offsets are file offsets, not view offsets: a view offset names a different
    // entry once `/tree` moves the branch, so it is mapped through the segments both ways.
    let pi_segments = match stream.kind {
        ReferenceNativeHistoryKind::Pi => match super::history_pi::pi_transcript_segments(bytes) {
            Some(segments) => Some(segments),
            None => {
                return Ok(unreadable_page(stream, super::history_pi::PI_BRANCH_UNREADABLE));
            }
        },
        _ => None,
    };
    let view: std::borrow::Cow<'_, [u8]> = match &pi_segments {
        Some(segments) => std::borrow::Cow::Owned(branch_bytes(bytes, segments)),
        None => std::borrow::Cow::Borrowed(bytes),
    };

    let mut starts = reference_history_turn_starts(stream.kind, &view);
    // Taken before the reset filter: the whole-stream generation below starts at this boundary.
    let first_start = starts.first().copied();
    // A reset clears every turn before it: no page may start before it, so no cursor can reach
    // the cleared conversation.
    let reset = reference_history_reset_end(stream.kind, &view);
    if let Some(reset) = reset {
        starts.retain(|start| *start > reset);
    }
    let stream_length = window_start + view.len() as u64;

    let end = match cursor {
        Some(cursor) => {
            if !cursor.matches_stream(&stream.stream_id) {
                return Err(history_error(
                    ScopeErrorCode::TargetExpired,
                    format!(
                        "the cursor belongs to stream `{}`, not `{}`",
                        cursor.stream_id, stream.stream_id
                    ),
                ));
            }
            if let Some(segments) = &pi_segments {
                let mapped = cursor
                    .offset
                    .checked_sub(window_start)
                    .and_then(|offset| pi_file_to_view(segments, offset as usize));
                match mapped {
                    Some(view_offset) => window_start + view_offset as u64,
                    None => {
                        return Err(history_error(
                            ScopeErrorCode::TargetExpired,
                            format!(
                                "the cursor names offset {}, which is not on the active pi branch",
                                cursor.offset
                            ),
                        ));
                    }
                }
            } else {
                if cursor.offset > stream_length {
                    return Err(history_error(
                        ScopeErrorCode::TargetExpired,
                        format!(
                            "the stream is {stream_length} bytes; the cursor names offset {}",
                            cursor.offset
                        ),
                    ));
                }
                if cursor.offset < window_start {
                    return Err(history_error(
                        ScopeErrorCode::TargetExpired,
                        format!(
                            "the cursor names offset {}, below the readable window at {window_start}",
                            cursor.offset
                        ),
                    ));
                }
                cursor.offset
            }
        }
        None => stream_length,
    };

    let end_relative = (end - window_start) as usize;
    let end_index = starts.partition_point(|start| *start < end_relative);
    let start_index = end_index.saturating_sub(limit);

    // The oldest page also carries what precedes the first boundary: the records after a reset
    // (or the head of a whole store) that no prompt opened still belong to the conversation. A
    // tail window's head is a torn older turn, so there it starts at the first boundary.
    let page_start = if start_index == 0 {
        match reset {
            Some(reset) => (reset + 1).min(end_relative),
            None if window_start == 0 => 0,
            // A window no prompt opens is one long turn in progress: its records ARE the live
            // turn, so they are parsed rather than dropped (the torn head line was already cut by
            // the window reader, and the parsers skip unopened tool results).
            None => starts.first().copied().unwrap_or(0).min(end_relative),
        }
    } else {
        starts[start_index]
    };
    let page_bytes: &[u8] = if page_start < end_relative {
        &view[page_start..end_relative]
    } else {
        &[]
    };
    let turns = match stream.kind {
        // A task's title is recorded where it was started; a page that only holds its result
        // inherits the titles every older byte named.
        ReferenceNativeHistoryKind::Omo | ReferenceNativeHistoryKind::Omp
            if page_start < end_relative =>
        {
            let mut titles = reference_history_task_titles(&view[..page_start]);
            let text = String::from_utf8_lossy(page_bytes);
            if stream.kind == ReferenceNativeHistoryKind::Omo {
                super::history_omo::parse_omo_transcript(&text, usize::MAX, false, &mut titles)
            } else {
                super::history_omp::parse_omp_transcript_with_titles(&text, titles)
            }
        }
        _ => parse_reference_history_turns(stream.kind, page_bytes)?,
    };

    // The generation is computed over the whole stream, never the page: asking for an older page
    // must not look like a change to the client's fence.
    // A tail window begins mid-stream, so what precedes its first boundary is a torn piece of an
    // older turn and is left out.
    let whole_from = if window_start > 0 {
        first_start.unwrap_or(0)
    } else {
        0
    };
    let whole = parse_reference_history_turns(stream.kind, &view[whole_from..])?;
    let generation = reference_history_window_generation(
        &stream.stream_id,
        identity.provider_session_id(),
        window_start,
        &whole,
    );

    let next_cursor = (start_index > 0).then(|| ReferenceHistoryCursor {
        stream_id: stream.stream_id.clone(),
        offset: window_start
            + match &pi_segments {
                Some(segments) => pi_view_to_file(segments, starts[start_index]),
                None => starts[start_index],
            } as u64,
    });
    // A tail window's oldest page is not the conversation's beginning: the turns before the window
    // were never read, so the page says so rather than passing its first turn off as the start. A
    // reset inside the window is the conversation's beginning, so nothing older is missing.
    let unavailable_reason = (window_start > 0 && start_index == 0 && reset.is_none()).then(|| {
        format!(
            "older history lies beyond the {REFERENCE_HISTORY_MAX_BYTES} byte read cap; the {window_start} \
             bytes before this window were not read"
        )
    });

    Ok(ReferenceHistoryPage {
        source: stream.source,
        availability: ReferenceHistoryAvailability::Native,
        turns,
        cursor: next_cursor,
        has_more: start_index > 0,
        generation,
        unavailable_reason,
    })
}

/// Serve one page from bytes already in memory, as a whole-store read.
pub fn reference_history_page_from_bytes(
    stream: &ReferenceHistoryStream,
    identity: &ReferenceHistoryIdentity,
    bytes: &[u8],
    limit: usize,
    cursor: Option<&ReferenceHistoryCursor>,
) -> Result<ReferenceHistoryPage, ScopeError> {
    reference_history_page_from_window(stream, identity, 0, bytes, limit, cursor)
}

/// Serve one page from a paired host's answer, which leads with the store's true size.
///
/// The size line is what lets the reader say honestly that it is holding a tail: a cursor below
/// the window it names is refused rather than answered from bytes nobody read.
pub fn reference_history_page_from_remote_bytes(
    stream: &ReferenceHistoryStream,
    identity: &ReferenceHistoryIdentity,
    bytes: &[u8],
    limit: usize,
    cursor: Option<&ReferenceHistoryCursor>,
) -> Result<ReferenceHistoryPage, ScopeError> {
    let (total, body) = crate::agent_transcript::split_remote_size_line(bytes).ok_or_else(|| {
        history_error(
            ScopeErrorCode::InvalidRequest,
            "the paired reader's answer carries no size line",
        )
    })?;
    let window_start = (total as u64).saturating_sub(body.len() as u64);
    reference_history_page_from_window(stream, identity, window_start, body, limit, cursor)
}

/// Read one page of a local store, off the async reactor.
///
/// The read is synchronous file I/O and the paging parses the whole window, so both are handed
/// to [`crate::ipc::run_blocking`].
pub async fn read_reference_history_page(
    stream: &ReferenceHistoryStream,
    identity: &ReferenceHistoryIdentity,
    limit: usize,
    cursor: Option<&ReferenceHistoryCursor>,
) -> Result<ReferenceHistoryPage, ScopeError> {
    ensure_reference_history_owner(stream, identity)?;

    let Some(path) = stream.file_path() else {
        return reference_history_page_from_window(stream, identity, 0, &[], limit, cursor);
    };
    let path = path.to_path_buf();
    let kind = stream.kind;
    let stream = stream.clone();
    let identity = identity.clone();
    let cursor = cursor.cloned();
    crate::ipc::run_blocking(move || {
        let page = (|| -> Result<ReferenceHistoryPage, ScopeError> {
            // A pi store past the branch cap is disclosed like a branch past it: the pane shows
            // its own output rather than an error that leaves it with nothing.
            let window = match read_reference_history_window(&path, kind) {
                Err(error)
                    if kind == ReferenceNativeHistoryKind::Pi
                        && error.code == ScopeErrorCode::PayloadTooLarge =>
                {
                    return Ok(unreadable_page(&stream, super::history_pi::PI_BRANCH_UNREADABLE));
                }
                other => other?,
            };
            reference_history_page_from_window(
                &stream,
                &identity,
                window.start,
                &window.bytes,
                limit,
                cursor.as_ref(),
            )
        })();
        Ok::<_, crate::ipc::error::IpcError>(page)
    })
    .await
    .map_err(|error| {
        history_error(
            ScopeErrorCode::Timeout,
            format!("the transcript read did not finish: {}", error.message),
        )
    })?
}

fn scrollback_page(stream: &ReferenceHistoryStream) -> ReferenceHistoryPage {
    ReferenceHistoryPage {
        source: ReferenceHistorySource::Scrollback,
        availability: ReferenceHistoryAvailability::Scrollback,
        turns: Vec::new(),
        // Same-pane output has no older page: it is the pane's own screen, not a stream.
        cursor: None,
        has_more: false,
        generation: reference_history_generation(&stream.stream_id, None, &[]),
        unavailable_reason: Some(
            stream
                .unavailable_reason
                .clone()
                .unwrap_or_else(|| REFERENCE_SCROLLBACK_DISCLOSURE.to_string()),
        ),
    }
}

fn not_started_page(stream: &ReferenceHistoryStream) -> ReferenceHistoryPage {
    ReferenceHistoryPage {
        source: stream.source,
        availability: ReferenceHistoryAvailability::NotStarted,
        turns: Vec::new(),
        cursor: None,
        has_more: false,
        generation: reference_history_generation(&stream.stream_id, None, &[]),
        unavailable_reason: Some(
            stream
                .unavailable_reason
                .clone()
                .unwrap_or_else(|| REFERENCE_SCROLLBACK_DISCLOSURE.to_string()),
        ),
    }
}

fn unreadable_page(stream: &ReferenceHistoryStream, reason: &str) -> ReferenceHistoryPage {
    ReferenceHistoryPage {
        source: ReferenceHistorySource::Scrollback,
        availability: ReferenceHistoryAvailability::Scrollback,
        turns: Vec::new(),
        cursor: None,
        has_more: false,
        generation: reference_history_generation(&stream.stream_id, None, &[]),
        unavailable_reason: Some(format!(
            "{REFERENCE_HISTORY_UNREADABLE_REASON} ({reason}); showing this pane's own output"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::reference_chat::types::{
        ReferencePart, ReferenceTargetRef, ReferenceTurnRole,
    };
    use crate::scoped_contracts::{Epoch, TargetRef};

    /// An omo-shaped store: three human turns, each with the assistant's answer after it. Every
    /// user record carries an array content block, which is the shape the families' own
    /// `opensTurn` accepts as a turn boundary.
    const THREE_TURNS: &str = concat!(
        r#"{"type":"session","id":"sess-a","cwd":"/w","timestamp":"2026-10-06T09:00:00.000Z"}"#,
        "\n",
        r#"{"type":"message","id":"m1","timestamp":"2026-10-06T09:00:01.000Z","message":{"role":"user","content":[{"type":"text","text":"first"}]}}"#,
        "\n",
        r#"{"type":"message","id":"m2","timestamp":"2026-10-06T09:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"one"}]}}"#,
        "\n",
        r#"{"type":"message","id":"m3","timestamp":"2026-10-06T09:00:03.000Z","message":{"role":"user","content":[{"type":"text","text":"second"}]}}"#,
        "\n",
        r#"{"type":"message","id":"m4","timestamp":"2026-10-06T09:00:04.000Z","message":{"role":"assistant","content":[{"type":"text","text":"two"}]}}"#,
        "\n",
        r#"{"type":"message","id":"m5","timestamp":"2026-10-06T09:00:05.000Z","message":{"role":"user","content":[{"type":"text","text":"third"}]}}"#,
        "\n",
        r#"{"type":"message","id":"m6","timestamp":"2026-10-06T09:00:06.000Z","message":{"role":"assistant","content":[{"type":"text","text":"three"}]}}"#,
        "\n",
    );

    /// A home directory that cleans itself up, so a failing assertion leaves nothing behind.
    struct TempHome(PathBuf);

    impl TempHome {
        fn new() -> Self {
            let dir = std::env::temp_dir()
                .join(format!("ferryx-ref-history-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).expect("a temp home is creatable");
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        /// Write a store under the `.omo` session root, in the folder a cwd slugs to.
        fn write_session(&self, folder: &str, file: &str, body: &str) -> PathBuf {
            let dir = self.0.join(".omo").join("agent").join("sessions").join(folder);
            std::fs::create_dir_all(&dir).expect("a session folder is creatable");
            let path = dir.join(file);
            std::fs::write(&path, body).expect("a session file is writable");
            path
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn target(backend: &str) -> ReferenceTargetRef {
        ReferenceTargetRef::without_provider_session(TargetRef {
            host_id: "host-a".into(),
            owner_id: "owner-a".into(),
            epoch: Epoch(11),
            backend_session_id: backend.into(),
        })
    }

    fn identity(registry: &str, backend: &str) -> ReferenceHistoryIdentity {
        ReferenceHistoryIdentity::new(target(backend), registry)
    }

    fn bound_identity(registry: &str, backend: &str, provider: &str) -> ReferenceHistoryIdentity {
        ReferenceHistoryIdentity::new(
            ReferenceTargetRef::with_provider_session(
                TargetRef {
                    host_id: "host-a".into(),
                    owner_id: "owner-a".into(),
                    epoch: Epoch(11),
                    backend_session_id: backend.into(),
                },
                provider,
            ),
            registry,
        )
    }

    fn first_text(turn: &ReferenceTurn) -> String {
        turn.parts
            .iter()
            .find_map(|part| match part {
                ReferencePart::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    fn omo_stream(home: &TempHome, identity: &ReferenceHistoryIdentity) -> ReferenceHistoryStream {
        resolve_reference_history_stream(home.path(), identity, &[])
            .expect("the store resolves for its own target")
    }

    fn full_turns() -> Vec<ReferenceTurn> {
        super::super::history_parse::dispatch_reference_history(
            ReferenceNativeHistoryKind::Omo,
            THREE_TURNS.as_bytes(),
        )
        .expect("the omo reader parses its own records")
    }

    #[test]
    fn the_registry_label_decides_the_reader_and_an_unknown_one_is_never_native() {
        let home = TempHome::new();
        for (registry, kind) in [
            ("claude", ReferenceNativeHistoryKind::Claude),
            ("codex", ReferenceNativeHistoryKind::Codex),
            ("omo", ReferenceNativeHistoryKind::Omo),
            ("omp", ReferenceNativeHistoryKind::Omp),
            ("gjc", ReferenceNativeHistoryKind::Gjc),
            ("pi", ReferenceNativeHistoryKind::Pi),
        ] {
            assert_eq!(identity(registry, "sess-1").native_kind(), kind, "{registry}");
        }
        for registry in ["opencode", "mimo-code", "cursor-agent", ""] {
            let scrollback = resolve_reference_history_stream(
                home.path(),
                &identity(registry, "sess-1"),
                &[],
            )
            .expect("a pane with no reader is still a legitimate result");
            assert_eq!(scrollback.kind, ReferenceNativeHistoryKind::Unavailable);
            assert_eq!(scrollback.availability, ReferenceHistoryAvailability::Scrollback);
            assert!(!scrollback.is_native());
            assert_eq!(scrollback.location, ReferenceHistoryLocation::Scrollback);
            assert!(scrollback.file_path().is_none());
            assert!(scrollback.unavailable_reason.is_some());
        }
    }

    #[test]
    fn resolve_binds_the_exact_provider_transcript_and_refuses_another_sessions_file() {
        let home = TempHome::new();
        let own = home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        let other = home.write_session("--w--", "2026-10-06_bbb.jsonl", THREE_TURNS);

        let identity = bound_identity("omo", "sess-1", "aaa").with_provider_transcript_path(&own);
        let stream = omo_stream(&home, &identity);
        assert_eq!(stream.file_path(), Some(own.as_path()));
        assert_eq!(stream.availability, ReferenceHistoryAvailability::Native);
        assert_eq!(stream.source, ReferenceHistorySource::OmoTranscript);

        let wrong = bound_identity("omo", "sess-1", "aaa").with_provider_transcript_path(&other);
        let refusal = resolve_reference_history_stream(home.path(), &wrong, &[])
            .expect_err("another session's store is refused");
        assert_eq!(refusal.code, ScopeErrorCode::Forbidden);
    }

    #[test]
    fn resolve_refuses_a_provider_identity_or_path_that_is_not_usable() {
        let home = TempHome::new();
        let traversal = bound_identity("omo", "sess-1", "../etc/passwd");
        let refusal = resolve_reference_history_stream(home.path(), &traversal, &[])
            .expect_err("a traversal id is not a session id");
        assert_eq!(refusal.code, ScopeErrorCode::Forbidden);

        let relative = bound_identity("omo", "sess-1", "aaa")
            .with_provider_transcript_path("relative/2026-10-06_aaa.jsonl");
        let refusal = resolve_reference_history_stream(home.path(), &relative, &[])
            .expect_err("a relative transcript path is not usable");
        assert_eq!(refusal.code, ScopeErrorCode::Forbidden);
    }

    #[test]
    fn resolve_binds_a_cwd_only_when_this_pane_owns_it_alone() {
        let home = TempHome::new();
        let only = home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        // No identity-exact hit: the pane's session id matches no store's file name.
        let identity = identity("omo", "sess-z").with_cwd("/w");
        let stream = omo_stream(&home, &identity);
        assert_eq!(stream.file_path(), Some(only.as_path()));
    }

    // rs-history-r4-1
    #[test]
    fn resolve_binds_a_windows_cwd_to_the_folder_omo_names_for_it() {
        let home = TempHome::new();
        let transcript = THREE_TURNS.replacen(
            r#""cwd":"/w""#,
            r#""cwd":"C:\\Users\\me\\app""#,
            1,
        );
        let only = home.write_session("--C--Users-me-app--", "2026-10-06_aaa.jsonl", &transcript);
        let identity = identity("omo", "sess-z").with_cwd(r"C:\Users\me\app");
        let stream = resolve_reference_history_stream(home.path(), &identity, &[])
            .expect("a Windows cwd resolves to OmO's own folder for it");
        assert_eq!(stream.file_path(), Some(only.as_path()));
    }

    // rs-history-r4-2
    #[test]
    fn the_oldest_page_of_a_tail_window_discloses_the_history_past_the_read_cap() {
        let home = TempHome::new();
        home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        let identity = bound_identity("omo", "sess-1", "aaa");
        let stream = omo_stream(&home, &identity);

        // A whole-store read reaches the conversation's real beginning and says nothing.
        let starts = reference_history_turn_starts(ReferenceNativeHistoryKind::Omo, THREE_TURNS.as_bytes());
        let whole_oldest = reference_history_page_from_bytes(&stream, &identity, THREE_TURNS.as_bytes(), 3, None)
            .expect("the whole store is one page");
        assert!(whole_oldest.cursor.is_none());
        assert_eq!(whole_oldest.unavailable_reason, None);

        // A tail window that begins inside the first turn holds only turns two and three.
        let window_start = starts[1] - 10;
        let window = &THREE_TURNS.as_bytes()[window_start..];
        let newest = reference_history_page_from_window(&stream, &identity, window_start as u64, window, 1, None)
            .expect("the newest page of the window is served");
        assert_eq!(newest.unavailable_reason, None, "a page with an older page in the window says nothing");
        let cursor = newest.cursor.expect("the window holds an older turn");
        let oldest =
            reference_history_page_from_window(&stream, &identity, window_start as u64, window, 1, Some(&cursor))
                .expect("the window's oldest page is served");
        assert_eq!(first_text(&oldest.turns[0]), "second");
        assert!(oldest.cursor.is_none(), "no byte below the window is reachable");
        assert!(!oldest.has_more);
        assert_eq!(oldest.availability, ReferenceHistoryAvailability::Native);
        assert!(
            oldest.unavailable_reason.as_deref().is_some_and(|reason| reason.contains("read cap")),
            "the window's oldest page must disclose the older history it cannot reach: {:?}",
            oldest.unavailable_reason
        );
    }

    // rs-history-r7-1
    #[test]
    fn a_tail_window_without_a_turn_boundary_still_serves_the_turn_it_read() {
        let home = TempHome::new();
        home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        let identity = bound_identity("omo", "sess-1", "aaa");
        let stream = omo_stream(&home, &identity);

        // The window opens on the last line, an assistant record: one long turn that no prompt in
        // the window opened (the torn head line was already dropped by the window reader).
        let window_start = THREE_TURNS.rfind(r#"{"type":"message","id":"m6""#).expect("m6 is in the fixture");
        let window = &THREE_TURNS.as_bytes()[window_start..];
        assert!(
            reference_history_turn_starts(ReferenceNativeHistoryKind::Omo, window).is_empty(),
            "the fixture window must hold no turn boundary"
        );
        let newest = reference_history_page_from_window(&stream, &identity, window_start as u64, window, 1, None)
            .expect("the newest page of the window is served");
        assert!(!newest.turns.is_empty(), "the live turn the window read must be shown: {:?}", newest.turns);
        assert!(newest.cursor.is_none());
        assert!(newest.unavailable_reason.as_deref().is_some_and(|reason| reason.contains("read cap")));
    }

    #[test]
    fn resolve_refuses_a_cwd_two_live_sessions_share() {
        let home = TempHome::new();
        home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        let identity = identity("omo", "sess-z").with_cwd("/w");
        let siblings = [ReferenceHistorySibling::new("sess-other", Some("/w".into()))];
        let refusal = resolve_reference_history_stream(home.path(), &identity, &siblings)
            .expect_err("a shared cwd has no single owner");
        assert_eq!(refusal.code, ScopeErrorCode::ControlConflict);
        assert!(refusal.message.contains("sess-other"));

        let elsewhere = [ReferenceHistorySibling::new("sess-other", Some("/other".into()))];
        assert!(resolve_reference_history_stream(home.path(), &identity, &elsewhere).is_ok());
    }

    #[test]
    fn resolve_refuses_a_cwd_that_holds_more_than_one_candidate() {
        let home = TempHome::new();
        home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        home.write_session("--w--", "2026-10-06_bbb.jsonl", THREE_TURNS);
        let identity = identity("omo", "sess-z").with_cwd("/w");
        let refusal = resolve_reference_history_stream(home.path(), &identity, &[])
            .expect_err("two candidates and no identity match is an ambiguous owner");
        assert_eq!(refusal.code, ScopeErrorCode::ControlConflict);
        assert!(refusal.message.contains('2'));
    }

    #[test]
    fn cwd_fallback_refuses_a_transcript_from_a_colliding_cwd_slug() {
        let home = TempHome::new();
        let cwd = "/work/a/b";
        let other_cwd = "/work/a-b";
        let folder = super::super::history_omo::omo_session_folder(cwd);
        assert_eq!(folder, super::super::history_omo::omo_session_folder(other_cwd));
        let body = serde_json::json!({"type": "session", "id": "old", "cwd": other_cwd});
        home.write_session(&folder, "old.jsonl", &format!("{body}\n"));
        let identity = identity("omo", "sess-live").with_cwd(cwd);

        let refusal = resolve_reference_history_stream(home.path(), &identity, &[])
            .expect_err("a colliding slug is not exact cwd ownership");

        assert_eq!(refusal.code, ScopeErrorCode::NotFound);
    }

    #[test]
    fn cwd_fallback_selects_only_the_exact_header_among_colliding_slugs() {
        let home = TempHome::new();
        let cwd = "/work/a/b";
        let folder = super::super::history_omo::omo_session_folder(cwd);
        let other = serde_json::json!({"type": "session", "id": "old", "cwd": "/work/a-b"});
        let own = serde_json::json!({"type": "session", "id": "own", "cwd": cwd});
        home.write_session(&folder, "old.jsonl", &format!("{other}\n"));
        let path = home.write_session(&folder, "own.jsonl", &format!("{own}\n"));
        let identity = identity("omo", "sess-live").with_cwd(cwd);

        let stream = resolve_reference_history_stream(home.path(), &identity, &[])
            .expect("the exact cwd header identifies the only candidate");

        assert_eq!(stream.file_path(), Some(path.as_path()));
    }

    #[test]
    fn cwd_fallback_refuses_missing_malformed_and_non_session_headers() {
        for body in ["", "not json\n", r#"{"type":"message","id":"old","cwd":"/w"}"#] {
            let home = TempHome::new();
            home.write_session("--w--", "old.jsonl", body);
            let identity = identity("omo", "sess-live").with_cwd("/w");

            let refusal = resolve_reference_history_stream(home.path(), &identity, &[])
                .expect_err("a transcript without a valid cwd header has no proven owner");

            assert_eq!(refusal.code, ScopeErrorCode::NotFound);
        }
    }

    #[test]
    fn resolve_reports_not_started_when_the_identity_is_known_and_nothing_was_written() {
        let home = TempHome::new();
        let identity = bound_identity("omo", "sess-1", "sess-never-written");
        let stream = omo_stream(&home, &identity);
        assert_eq!(stream.availability, ReferenceHistoryAvailability::NotStarted);
        assert_eq!(stream.location, ReferenceHistoryLocation::NotStarted);
        assert!(stream.file_path().is_none());
        assert!(!stream.is_native());
    }

    #[test]
    fn resolve_refuses_a_pane_nothing_identifies() {
        let home = TempHome::new();
        let refusal = resolve_reference_history_stream(home.path(), &identity("omo", "sess-1"), &[])
            .expect_err("no identity and no cwd is not a store");
        assert_eq!(refusal.code, ScopeErrorCode::NotFound);
    }

    #[test]
    fn the_owner_check_refuses_a_rotated_target_and_a_foreign_provider() {
        let home = TempHome::new();
        home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        let identity = bound_identity("omo", "sess-1", "aaa");
        let stream = omo_stream(&home, &identity);
        assert!(ensure_reference_history_owner(&stream, &identity).is_ok());

        let rotated = ReferenceHistoryIdentity::new(
            ReferenceTargetRef::with_provider_session(
                TargetRef { epoch: Epoch(12), ..identity.target.target.clone() },
                "aaa",
            ),
            "omo",
        );
        assert_eq!(
            ensure_reference_history_owner(&stream, &rotated).unwrap_err().code,
            ScopeErrorCode::TargetExpired
        );

        let foreign = bound_identity("omo", "sess-1", "bbb");
        assert_eq!(
            ensure_reference_history_owner(&stream, &foreign).unwrap_err().code,
            ScopeErrorCode::Forbidden
        );

        let other_reader = bound_identity("codex", "sess-1", "aaa");
        assert_eq!(
            ensure_reference_history_owner(&stream, &other_reader).unwrap_err().code,
            ScopeErrorCode::Forbidden
        );
    }

    #[test]
    fn turn_starts_land_on_the_families_own_turn_predicates() {
        let user = r#"{"type":"message","message":{"role":"user","content":[{"type":"text","text":"hi"}]}}"#;
        let assistant =
            r#"{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#;
        let claude_user = r#"{"type":"user","message":{"content":[{"type":"text","text":"hi"}]}}"#;
        let claude_assistant = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"}]}}"#;
        let codex_start = r#"{"type":"event_msg","payload":{"type":"task_started"}}"#;
        let codex_other = r#"{"type":"event_msg","payload":{"type":"agent_message"}}"#;

        for kind in [
            ReferenceNativeHistoryKind::Omo,
            ReferenceNativeHistoryKind::Omp,
            ReferenceNativeHistoryKind::Gjc,
            ReferenceNativeHistoryKind::Pi,
        ] {
            assert!(reference_history_line_opens_turn(kind, user), "{}", kind.as_str());
            assert!(!reference_history_line_opens_turn(kind, assistant), "{}", kind.as_str());
        }
        assert!(reference_history_line_opens_turn(ReferenceNativeHistoryKind::Claude, claude_user));
        assert!(!reference_history_line_opens_turn(
            ReferenceNativeHistoryKind::Claude,
            claude_assistant
        ));
        assert!(reference_history_line_opens_turn(ReferenceNativeHistoryKind::Codex, codex_start));
        assert!(!reference_history_line_opens_turn(ReferenceNativeHistoryKind::Codex, codex_other));
        assert!(!reference_history_line_opens_turn(
            ReferenceNativeHistoryKind::Unavailable,
            user
        ));

        // The offsets are real positions in the bytes, and they are the three user records.
        let starts = reference_history_turn_starts(ReferenceNativeHistoryKind::Omo, THREE_TURNS.as_bytes());
        assert_eq!(starts.len(), 3);
        let lines: Vec<&str> = THREE_TURNS.lines().collect();
        for (index, start) in starts.iter().enumerate() {
            let expected: usize = lines[..index * 2 + 1].iter().map(|line| line.len() + 1).sum();
            assert_eq!(*start, expected, "boundary {index}");
            assert!(lines[index * 2 + 1].contains(r#""role":"user""#));
        }
    }

    #[test]
    fn a_page_carries_the_newest_turns_and_the_cursor_for_the_page_before() {
        let home = TempHome::new();
        home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        let identity = bound_identity("omo", "sess-1", "aaa");
        let stream = omo_stream(&home, &identity);

        let page =
            reference_history_page_from_bytes(&stream, &identity, THREE_TURNS.as_bytes(), 2, None)
                .expect("the newest page is served");
        assert_eq!(page.availability, ReferenceHistoryAvailability::Native);
        assert_eq!(page.source, ReferenceHistorySource::OmoTranscript);
        assert!(page.is_native());
        assert_eq!(page.disclosure(), None);
        assert_eq!(page.unavailable_reason, None);
        assert!(page.has_more);

        let starts = reference_history_turn_starts(ReferenceNativeHistoryKind::Omo, THREE_TURNS.as_bytes());
        let cursor = page.cursor.expect("an older page exists");
        assert_eq!(cursor.stream_id, stream.stream_id);
        assert_eq!(cursor.offset, starts[1] as u64);

        // The page never splits a turn: it starts on the boundary it names.
        assert_eq!(page.turns[0].role, ReferenceTurnRole::User);
        assert_eq!(first_text(&page.turns[0]), "second");
        assert_eq!(page.turns.len(), 4, "the boundary's turn and what followed it");
    }

    #[test]
    fn paging_the_whole_stream_reassembles_it_once_in_order() {
        let home = TempHome::new();
        home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        let identity = bound_identity("omo", "sess-1", "aaa");
        let stream = omo_stream(&home, &identity);

        let mut collected: Vec<ReferenceTurn> = Vec::new();
        let mut cursor: Option<ReferenceHistoryCursor> = None;
        let mut pages = 0;
        loop {
            let page = reference_history_page_from_bytes(
                &stream,
                &identity,
                THREE_TURNS.as_bytes(),
                1,
                cursor.as_ref(),
            )
            .expect("every page is served");
            let mut oldest_first = page.turns;
            oldest_first.extend(collected);
            collected = oldest_first;
            pages += 1;
            cursor = page.cursor;
            assert!(pages <= 4, "paging must terminate");
            if cursor.is_none() {
                break;
            }
        }

        assert_eq!(pages, 3, "one page per human turn");
        assert_eq!(collected, full_turns(), "no turn is duplicated and none is dropped");
    }

    #[test]
    fn a_cursor_minted_against_another_stream_is_refused() {
        let home = TempHome::new();
        home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        let identity = bound_identity("omo", "sess-1", "aaa");
        let stream = omo_stream(&home, &identity);

        let foreign = ReferenceHistoryCursor { stream_id: "omo:0123456789abcdef".into(), offset: 10 };
        let refusal = reference_history_page_from_bytes(
            &stream,
            &identity,
            THREE_TURNS.as_bytes(),
            2,
            Some(&foreign),
        )
        .expect_err("a cursor from another stream is never re-anchored");
        assert_eq!(refusal.code, ScopeErrorCode::TargetExpired);
        assert!(refusal.message.contains(&stream.stream_id));
    }

    #[test]
    fn a_cursor_past_the_stream_or_before_the_window_is_refused() {
        let home = TempHome::new();
        home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        let identity = bound_identity("omo", "sess-1", "aaa");
        let stream = omo_stream(&home, &identity);

        let rotated = ReferenceHistoryCursor {
            stream_id: stream.stream_id.clone(),
            offset: THREE_TURNS.len() as u64 + 1,
        };
        assert_eq!(
            reference_history_page_from_bytes(&stream, &identity, THREE_TURNS.as_bytes(), 2, Some(&rotated))
                .unwrap_err()
                .code,
            ScopeErrorCode::TargetExpired
        );

        let below = ReferenceHistoryCursor { stream_id: stream.stream_id.clone(), offset: 4 };
        assert_eq!(
            reference_history_page_from_window(
                &stream,
                &identity,
                64,
                THREE_TURNS.as_bytes(),
                2,
                Some(&below)
            )
            .unwrap_err()
            .code,
            ScopeErrorCode::TargetExpired
        );
    }

    #[test]
    fn the_limit_is_bounded_and_zero_is_refused() {
        assert_eq!(normalize_reference_history_limit(0).unwrap_err().code, ScopeErrorCode::InvalidRequest);
        assert_eq!(normalize_reference_history_limit(1).unwrap(), 1);
        assert_eq!(normalize_reference_history_limit(REFERENCE_HISTORY_MAX_LIMIT).unwrap(), REFERENCE_HISTORY_MAX_LIMIT);
        assert_eq!(
            normalize_reference_history_limit(REFERENCE_HISTORY_MAX_LIMIT * 4).unwrap(),
            REFERENCE_HISTORY_MAX_LIMIT
        );
    }

    #[test]
    fn scrollback_and_not_started_pages_carry_no_older_cursor() {
        let home = TempHome::new();
        let scrollback = resolve_reference_history_stream(home.path(), &identity("opencode", "s1"), &[])
            .expect("a pane with no reader resolves to its own output");
        let page = reference_history_page_from_bytes(&scrollback, &identity("opencode", "s1"), b"", 50, None)
            .expect("a scrollback page is served");
        assert_eq!(page.availability, ReferenceHistoryAvailability::Scrollback);
        assert_eq!(page.source, ReferenceHistorySource::Scrollback);
        assert!(page.turns.is_empty());
        assert!(page.cursor.is_none());
        assert!(!page.has_more);
        assert!(page.disclosure().is_some());
        assert!(page.unavailable_reason.is_some());
        assert!(!page.can_reach_provider_read(&scrollback.owner));

        let unwritten = bound_identity("omo", "sess-1", "sess-never-written");
        let stream = omo_stream(&home, &unwritten);
        let page = reference_history_page_from_bytes(&stream, &unwritten, b"", 50, None)
            .expect("an unwritten session is served");
        assert_eq!(page.availability, ReferenceHistoryAvailability::NotStarted);
        assert!(page.turns.is_empty());
        assert!(page.cursor.is_none());
        assert!(!page.has_more);
        assert!(page.disclosure().is_some());
    }

    fn pi_entry(id: &str, parent: &str, role: &str, text: &str) -> String {
        let content = if role == "user" {
            serde_json::json!(text)
        } else {
            serde_json::json!([{ "type": "text", "text": text }])
        };
        let mut line = serde_json::json!({
            "type": "message",
            "id": id,
            "parentId": parent,
            "timestamp": "2026-10-06T10:00:01.000Z",
            "message": { "role": role, "content": content, "stopReason": "stop" },
        })
        .to_string();
        line.push('\n');
        line
    }

    #[test]
    fn a_pi_cursor_minted_before_a_tree_is_refused_not_reanchored_onto_the_new_branch() {
        let home = TempHome::new();
        let mut store = String::from(
            "{\"type\":\"session\",\"version\":\"0.87.1\",\"cwd\":\"/w\",\"timestamp\":\"2026-10-06T10:00:00.000Z\"}\n",
        );
        store += &pi_entry("u1", "session", "user", "one");
        store += &pi_entry("a1", "u1", "assistant", "answer one");
        store += &pi_entry("u2", "a1", "user", "two");
        store += &pi_entry("a2", "u2", "assistant", "answer two");
        store += &pi_entry("u3", "a2", "user", "three");
        store += &pi_entry("a3", "u3", "assistant", "answer three");
        home.write_session("--w--", "2026-10-06_aaa.jsonl", &store);
        let identity = bound_identity("pi", "sess-1", "aaa");
        let stream = omo_stream(&home, &identity);
        assert_eq!(stream.kind, ReferenceNativeHistoryKind::Pi);

        let newest = reference_history_page_from_bytes(&stream, &identity, store.as_bytes(), 1, None)
            .expect("the newest page is served");
        let cursor = newest.cursor.expect("an older page exists");
        // The cursor still works while the branch it was minted on is the live one.
        let older = reference_history_page_from_bytes(&stream, &identity, store.as_bytes(), 1, Some(&cursor))
            .expect("an unchanged branch keeps its cursor");
        assert_eq!(first_text(&older.turns[0]), "two");

        // `/tree` back to a1, then a new exchange long enough that the new branch view reaches
        // past the old cursor's offset.
        store += &pi_entry("u4", "a1", "user", &"four ".repeat(80));
        store += &pi_entry("a4", "u4", "assistant", &"answer four ".repeat(80));
        let after = reference_history_page_from_bytes(&stream, &identity, store.as_bytes(), 1, Some(&cursor));
        match after {
            Err(refusal) => assert_eq!(refusal.code, ScopeErrorCode::TargetExpired),
            Ok(page) => panic!(
                "a cursor from the abandoned branch was re-anchored onto the new one: {:?}",
                page.turns.iter().map(first_text).collect::<Vec<_>>()
            ),
        }

        // A cursor minted on the new branch pages it exactly.
        let newest = reference_history_page_from_bytes(&stream, &identity, store.as_bytes(), 1, None)
            .expect("the new branch is served");
        assert_eq!(first_text(&newest.turns[0]), "four ".repeat(80));
        let cursor = newest.cursor.expect("the new branch has older turns");
        let older = reference_history_page_from_bytes(&stream, &identity, store.as_bytes(), 1, Some(&cursor))
            .expect("the live branch's own cursor is honoured");
        assert_eq!(first_text(&older.turns[0]), "one");
        assert!(!older.has_more);
    }

    #[test]
    fn a_pi_tree_that_cannot_be_projected_is_disclosed_rather_than_invented() {
        let home = TempHome::new();
        home.write_session("--w--", "2026-10-06_aaa.jsonl", "not an entry tree at all\n");
        let identity = bound_identity("pi", "sess-1", "aaa");
        let stream = omo_stream(&home, &identity);
        let page = reference_history_page_from_bytes(&stream, &identity, b"not an entry tree at all\n", 10, None)
            .expect("an unprojectable tree is disclosed, not an error");
        assert_eq!(page.availability, ReferenceHistoryAvailability::Scrollback);
        assert_eq!(page.source, ReferenceHistorySource::Scrollback);
        assert!(page.turns.is_empty(), "a branch nobody could walk is never approximated");
        assert!(
            page.unavailable_reason
                .as_deref()
                .is_some_and(|reason| reason.contains(super::super::history_pi::PI_BRANCH_UNREADABLE)),
            "{:?}",
            page.unavailable_reason
        );
    }

    #[test]
    fn a_remote_reader_reports_the_window_its_size_line_names() {
        let home = TempHome::new();
        home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        let identity = bound_identity("omo", "sess-1", "aaa");
        let stream = omo_stream(&home, &identity);

        let whole = format!("{}\n{THREE_TURNS}", THREE_TURNS.len());
        let from_remote =
            reference_history_page_from_remote_bytes(&stream, &identity, whole.as_bytes(), 2, None)
                .expect("a whole-store answer is served");
        let from_bytes =
            reference_history_page_from_bytes(&stream, &identity, THREE_TURNS.as_bytes(), 2, None)
                .expect("the same page from bytes");
        assert_eq!(from_remote, from_bytes);

        let tail_start = THREE_TURNS.len() - 120;
        let tail = format!("{}\n{}", THREE_TURNS.len(), &THREE_TURNS[tail_start..]);
        let page = reference_history_page_from_remote_bytes(&stream, &identity, tail.as_bytes(), 2, None)
            .expect("a tail answer is served");
        assert!(!page.has_more, "the reader holds no older bytes to page from");
        let below = ReferenceHistoryCursor { stream_id: stream.stream_id.clone(), offset: 4 };
        assert_eq!(
            reference_history_page_from_remote_bytes(&stream, &identity, tail.as_bytes(), 2, Some(&below))
                .unwrap_err()
                .code,
            ScopeErrorCode::TargetExpired
        );

        let sizeless = reference_history_page_from_remote_bytes(&stream, &identity, b"", 2, None)
            .expect_err("an answer with no size line is not a page");
        assert_eq!(sizeless.code, ScopeErrorCode::InvalidRequest);
    }

    #[test]
    fn a_bounded_window_drops_the_torn_first_line() {
        let home = TempHome::new();
        let path = home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        let window = read_reference_history_window(&path, ReferenceNativeHistoryKind::Omo)
            .expect("a small store is read whole");
        assert_eq!(window.start, 0);
        assert_eq!(window.bytes, THREE_TURNS.as_bytes());

        let missing = read_reference_history_window(&path.with_extension("gone"), ReferenceNativeHistoryKind::Omo)
            .expect_err("a store that is not there is not readable");
        assert_eq!(missing.code, ScopeErrorCode::NotFound);
    }

    #[test]
    fn an_append_does_not_rotate_the_generation_and_a_truncation_does() {
        let tracker = ReferenceHistoryGenerationTracker::new();
        let stream_id = "omo:0123456789abcdef";
        let turns = full_turns();

        let first = tracker.observe(stream_id, Some("provider-1"), &turns);
        assert_eq!(
            tracker.observe(stream_id, Some("provider-1"), &turns),
            first,
            "observing the same stream twice is not a change"
        );

        // The page reader hands the whole stream over every time, so a page cannot look like a
        // change to the client's fence.
        let mut appended = turns.clone();
        appended.push(turns[0].clone());
        assert_eq!(
            tracker.observe(stream_id, Some("provider-1"), &appended),
            first,
            "an append extends the known prefix and keeps the incarnation"
        );

        assert_ne!(
            tracker.observe(stream_id, Some("provider-1"), &turns[..2]),
            first,
            "a shorter stream is a truncation, and a truncation rotates"
        );
    }

    #[test]
    fn a_prefix_rewrite_a_truncation_a_head_removal_and_a_provider_change_rotate() {
        let stream_id = "omo:0123456789abcdef";
        let turns = full_turns();
        let base =
            ReferenceHistoryGenerationTracker::new().observe(stream_id, Some("provider-1"), &turns);

        let mut rewritten = turns.clone();
        rewritten[1].parts = vec![ReferencePart::Text { text: "rewritten".into(), phase: None }];
        let rewrite = ReferenceHistoryGenerationTracker::new();
        assert_eq!(rewrite.observe(stream_id, Some("provider-1"), &turns), base);
        assert_ne!(
            rewrite.observe(stream_id, Some("provider-1"), &rewritten),
            base,
            "a rewritten existing turn rotates"
        );

        let mut truncated = turns.clone();
        truncated.pop();
        let truncation = ReferenceHistoryGenerationTracker::new();
        assert_eq!(truncation.observe(stream_id, Some("provider-1"), &turns), base);
        assert_ne!(
            truncation.observe(stream_id, Some("provider-1"), &truncated),
            base,
            "a shorter stream rotates"
        );

        let without_head = turns[1..].to_vec();
        let head_removal = ReferenceHistoryGenerationTracker::new();
        assert_eq!(head_removal.observe(stream_id, Some("provider-1"), &turns), base);
        assert_ne!(
            head_removal.observe(stream_id, Some("provider-1"), &without_head),
            base,
            "dropping the first turn shifts the chain and rotates"
        );

        let provider_change = ReferenceHistoryGenerationTracker::new();
        assert_eq!(provider_change.observe(stream_id, Some("provider-1"), &turns), base);
        assert_ne!(
            provider_change.observe(stream_id, Some("provider-2"), &turns),
            base,
            "another provider session is another conversation"
        );
    }

    #[test]
    fn a_stream_without_a_provider_identity_keeps_a_genesis_generation() {
        let turns = full_turns();
        let stream_id = "omo:0123456789abcdef";
        let genesis = reference_history_genesis_generation(stream_id, turns.first());
        assert_eq!(genesis, reference_history_genesis_generation(stream_id, turns.first()));
        assert!(genesis.starts_with(stream_id));

        let rewritten = ReferencePart::Text { text: "different".into(), phase: None };
        let mut other = turns[0].clone();
        other.parts = vec![rewritten];
        assert_ne!(genesis, reference_history_genesis_generation(stream_id, Some(&other)));
        assert!(reference_history_genesis_generation(stream_id, None).starts_with(stream_id));
    }

    #[test]
    fn a_native_page_reaches_provider_read_only_with_an_identified_provider_session() {
        let home = TempHome::new();
        home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        let bound = bound_identity("omo", "sess-1", "aaa");
        let stream = omo_stream(&home, &bound);
        let page = reference_history_page_from_bytes(&stream, &bound, THREE_TURNS.as_bytes(), 5, None)
            .expect("the page is served");
        assert!(page.can_reach_provider_read(&stream.owner));

        // The same store, with no provider identity: `transcript_path_for_session` matches a
        // store by the session id in its own file name, so the backend session id that resolves
        // this store is the id the store is named with (`aaa`) — the daemon's backend session id
        // is the agent's own session id in the common case.
        let unbound = identity("omo", "aaa");
        let stream = resolve_reference_history_stream(home.path(), &unbound, &[])
            .expect("the backend session id resolves the same store");
        let page = reference_history_page_from_bytes(&stream, &unbound, THREE_TURNS.as_bytes(), 5, None)
            .expect("the page is served");
        assert!(page.is_native());
        assert!(!page.can_reach_provider_read(&stream.owner), "an unbound target stops at accepted");
    }

    fn omo_line(role: &str, text: &str) -> String {
        serde_json::json!({
            "type": "message",
            "message": {"role": role, "content": [{"type": "text", "text": text}]}
        })
        .to_string()
    }

    fn joined(lines: &[String]) -> String {
        lines.iter().map(|line| format!("{line}\n")).collect()
    }

    // rs-history-core-1
    #[test]
    fn a_claude_page_keeps_every_turn_its_boundaries_hold() {
        let home = TempHome::new();
        let mut body = String::new();
        for index in 1..=150 {
            body.push_str(
                &serde_json::json!({
                    "type": "user",
                    "message": {"role": "user", "content": format!("prompt {index}")}
                })
                .to_string(),
            );
            body.push('\n');
            body.push_str(
                &serde_json::json!({
                    "type": "assistant",
                    "message": {"role": "assistant", "content": [{"type": "text", "text": format!("answer {index}")}]}
                })
                .to_string(),
            );
            body.push('\n');
        }
        let path = home.path().join("aaa.jsonl");
        std::fs::write(&path, &body).expect("a claude store is writable");
        let identity =
            bound_identity("claude", "sess-1", "aaa").with_provider_transcript_path(&path);
        let stream = resolve_reference_history_stream(home.path(), &identity, &[])
            .expect("the named claude store resolves");
        assert_eq!(stream.kind, ReferenceNativeHistoryKind::Claude);

        let page = reference_history_page_from_bytes(&stream, &identity, body.as_bytes(), 200, None)
            .expect("the page is served");
        assert_eq!(page.turns.len(), 300, "every turn the page's boundaries hold is served");
        assert_eq!(first_text(&page.turns[0]), "prompt 1");
        assert!(!page.has_more);
    }

    // rs-history-r5-1
    #[test]
    fn the_backend_id_never_binds_an_older_store_when_the_current_session_is_known() {
        let home = TempHome::new();
        let old = home.write_session("--w--", "2026-10-06_sess-b.jsonl", THREE_TURNS);

        // The pane's backend id names the first session's store; the current one is unwritten.
        let provider = bound_identity("omo", "sess-b", "sess-x");
        let stream = resolve_reference_history_stream(home.path(), &provider, &[])
            .expect("a known, unwritten provider session resolves");
        assert_eq!(
            stream.availability,
            ReferenceHistoryAvailability::NotStarted,
            "bound {:?} for an unwritten provider session",
            stream.file_path()
        );

        let agent = identity("omo", "sess-b").with_agent_session_id("sess-x");
        let stream = resolve_reference_history_stream(home.path(), &agent, &[])
            .expect("a known, unwritten agent session resolves");
        assert_eq!(stream.availability, ReferenceHistoryAvailability::NotStarted);

        // Another reader's pane whose backend id matches an `.omo` file never reads it.
        let codex = identity("codex", "sess-b");
        match resolve_reference_history_stream(home.path(), &codex, &[]) {
            Ok(stream) => assert_ne!(
                stream.file_path(),
                Some(old.as_path()),
                "a codex pane must not read an omo store"
            ),
            Err(error) => assert_eq!(error.code, ScopeErrorCode::NotFound),
        }

        // The common case still binds: nothing but the backend id identifies the omo pane.
        let backend_only = identity("omo", "sess-b");
        assert_eq!(omo_stream(&home, &backend_only).file_path(), Some(old.as_path()));
    }

    // rs-history-core-2
    #[test]
    fn the_cwd_fallback_never_binds_a_store_when_the_session_is_known_or_the_reader_differs() {
        let home = TempHome::new();
        let old = home.write_session("--w--", "2026-10-06_old.jsonl", THREE_TURNS);

        let known = identity("omo", "sess-z").with_cwd("/w").with_agent_session_id("new-sess");
        let stream = resolve_reference_history_stream(home.path(), &known, &[])
            .expect("a known, unwritten session resolves");
        assert_eq!(
            stream.availability,
            ReferenceHistoryAvailability::NotStarted,
            "bound {:?} instead of reporting the unwritten session",
            stream.file_path()
        );

        let named = identity("omo", "sess-z")
            .with_cwd("/w")
            .with_provider_transcript_path(home.path().join("not-written-yet.jsonl"));
        let stream = resolve_reference_history_stream(home.path(), &named, &[])
            .expect("a named, unwritten store resolves");
        assert_eq!(stream.availability, ReferenceHistoryAvailability::NotStarted);

        let claude = identity("claude", "sess-z").with_cwd("/w");
        match resolve_reference_history_stream(home.path(), &claude, &[]) {
            Ok(stream) => assert_ne!(
                stream.file_path(),
                Some(old.as_path()),
                "a claude pane must not read an omo store"
            ),
            Err(error) => assert_eq!(error.code, ScopeErrorCode::NotFound),
        }
    }

    // rs-history-core-4
    #[test]
    fn a_window_whose_head_moves_with_an_append_keeps_the_generation() {
        let home = TempHome::new();
        let lines: Vec<String> = (0..11)
            .map(|index| omo_line(if index % 2 == 0 { "user" } else { "assistant" }, &format!("line {index}")))
            .collect();
        let first = joined(&lines[..10]);
        let second = joined(&lines[1..11]);
        let shifted = 1000 + lines[0].len() as u64 + 1;
        let mut rewritten_lines = lines[1..11].to_vec();
        rewritten_lines[3] = omo_line("user", "rewritten");
        let rewritten = joined(&rewritten_lines);
        home.write_session("--w--", "2026-10-06_aaa.jsonl", &first);

        for identity in [bound_identity("omo", "sess-1", "aaa"), identity("omo", "aaa")] {
            let stream = omo_stream(&home, &identity);
            let before =
                reference_history_page_from_window(&stream, &identity, 1000, first.as_bytes(), 50, None)
                    .expect("the first window is served")
                    .generation;
            let after =
                reference_history_page_from_window(&stream, &identity, shifted, second.as_bytes(), 50, None)
                    .expect("the shifted window is served")
                    .generation;
            assert_eq!(after, before, "an append that moves the window head is not a change");
            let rotated =
                reference_history_page_from_window(&stream, &identity, shifted, rewritten.as_bytes(), 50, None)
                    .expect("the rewritten window is served")
                    .generation;
            assert_ne!(rotated, before, "a rewrite inside the window still rotates");
        }
    }

    // rs-history-core-5
    #[test]
    fn the_generation_tracker_forgets_streams_past_its_bound() {
        let tracker = ReferenceHistoryGenerationTracker::new();
        for index in 0..10_000u32 {
            tracker.observe(&format!("omo:{index:016x}"), Some("provider-1"), &[]);
        }
        assert!(
            tracker.tracked_stream_count() <= REFERENCE_HISTORY_MAX_TRACKED_STREAMS,
            "the tracker holds {} streams",
            tracker.tracked_stream_count()
        );
        let newest = format!("omo:{:016x}", 9_999u32);
        assert_eq!(
            tracker.observe(&newest, Some("provider-1"), &[]),
            format!("{newest}:inc-1"),
            "the most recently observed stream is still tracked"
        );
    }

    /// A store of any size whose bytes are generated on demand, counting what was read.
    struct SyntheticStore {
        length: u64,
        position: u64,
        delivered: u64,
    }

    impl SyntheticStore {
        fn byte_at(position: u64) -> u8 {
            if position % 97 == 96 {
                b'\n'
            } else {
                b'a' + (position % 26) as u8
            }
        }
    }

    impl Read for SyntheticStore {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let remaining = self.length.saturating_sub(self.position);
            let count = (buffer.len() as u64).min(remaining) as usize;
            for (index, slot) in buffer[..count].iter_mut().enumerate() {
                *slot = Self::byte_at(self.position + index as u64);
            }
            self.position += count as u64;
            self.delivered += count as u64;
            Ok(count)
        }
    }

    impl Seek for SyntheticStore {
        fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
            let next = match position {
                SeekFrom::Start(offset) => offset as i128,
                SeekFrom::End(delta) => self.length as i128 + delta as i128,
                SeekFrom::Current(delta) => self.position as i128 + delta as i128,
            };
            if next < 0 {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "before the start"));
            }
            self.position = next as u64;
            Ok(self.position)
        }
    }

    // rs-history-core-6
    #[test]
    fn the_prompt_session_tail_reads_only_the_bytes_it_keeps() {
        let max = 4 * 1024 * 1024;
        let length = 50 * 1024 * 1024u64;
        let mut store = SyntheticStore { length, position: 0, delivered: 0 };
        let tail = read_reference_history_tail(&mut store, max).expect("the tail is readable");
        assert_eq!(tail.len(), max);
        assert_eq!(tail[0], SyntheticStore::byte_at(length - max as u64));
        assert_eq!(tail[max - 1], SyntheticStore::byte_at(length - 1));
        assert!(store.delivered <= max as u64, "read {} bytes to keep {max}", store.delivered);

        let mut small = SyntheticStore { length: 10, position: 0, delivered: 0 };
        let whole = read_reference_history_tail(&mut small, max).expect("a small store is read whole");
        assert_eq!(whole.len(), 10);
    }

    // rs-history-other-3
    #[test]
    fn a_context_clear_ends_paging_so_no_older_page_serves_the_cleared_conversation() {
        let home = TempHome::new();
        let body = joined(&[
            omo_line("user", "A"),
            omo_line("assistant", "answer A"),
            omo_line("user", "A2"),
            omo_line("assistant", "answer A2"),
            r#"{"type":"custom","customType":"context_clear"}"#.to_string(),
            omo_line("user", "B"),
            omo_line("assistant", "answer B"),
        ]);
        home.write_session("--w--", "2026-10-06_aaa.jsonl", &body);
        for registry in ["omo", "omp", "gjc"] {
            let identity = bound_identity(registry, "sess-1", "aaa");
            let stream = omo_stream(&home, &identity);
            let page = reference_history_page_from_bytes(&stream, &identity, body.as_bytes(), 1, None)
                .expect("the newest page is served");
            assert_eq!(first_text(&page.turns[0]), "B", "{registry}");
            let mut served: Vec<String> = page.turns.iter().map(first_text).collect();
            let mut cursor = page.cursor.clone();
            let mut pages = 0;
            while let Some(older) = cursor.take() {
                pages += 1;
                assert!(pages < 5, "{registry}: paging must terminate");
                let older =
                    reference_history_page_from_bytes(&stream, &identity, body.as_bytes(), 1, Some(&older))
                        .expect("the older page is served");
                served.extend(older.turns.iter().map(first_text));
                cursor = older.cursor;
            }
            assert_eq!(
                served,
                vec!["B".to_string(), "answer B".to_string()],
                "{registry}: the cleared conversation came back"
            );
            assert!(!page.has_more, "{registry}: nothing before the reset is reachable");
        }
    }

    // rs-history-r6-1
    #[test]
    fn the_oldest_page_keeps_records_after_a_reset_that_no_prompt_opened() {
        let home = TempHome::new();
        let body = joined(&[
            omo_line("user", "A"),
            omo_line("assistant", "answer A"),
            r#"{"type":"custom","customType":"context_clear"}"#.to_string(),
            omo_line("assistant", "done"),
        ]);
        home.write_session("--w--", "2026-10-06_aaa.jsonl", &body);
        let identity = bound_identity("omo", "sess-1", "aaa");
        let stream = omo_stream(&home, &identity);
        let page = reference_history_page_from_bytes(&stream, &identity, body.as_bytes(), 10, None)
            .expect("the page is served");
        let texts: Vec<String> = page.turns.iter().map(first_text).collect();
        assert_eq!(texts, vec!["done".to_string()], "the answer after the reset is served");
        assert!(!page.has_more, "nothing before the reset is reachable");

        // A whole store whose first record precedes any turn opener keeps that record too.
        let body = joined(&[omo_line("assistant", "hello"), omo_line("user", "B")]);
        let page = reference_history_page_from_bytes(&stream, &identity, body.as_bytes(), 10, None)
            .expect("the page is served");
        let texts: Vec<String> = page.turns.iter().map(first_text).collect();
        assert_eq!(texts, vec!["hello".to_string(), "B".to_string()]);
    }

    // rs-history-other-4
    #[test]
    fn a_task_title_from_an_earlier_page_still_names_its_result() {
        let home = TempHome::new();
        let body = joined(&[
            omo_line("user", "spawn a worker"),
            serde_json::json!({"type": "message", "message": {"role": "assistant", "stopReason": "toolUse", "content": [{"type": "toolCall", "id": "c9", "name": "task", "arguments": {"task_summary": "audit parser", "prompt": "audit"}}]}}).to_string(),
            serde_json::json!({"type": "message", "message": {"role": "toolResult", "toolName": "task", "toolCallId": "c9", "content": [{"type": "text", "text": "started"}], "details": {"task_id": "st-1", "task_summary": "audit parser"}}}).to_string(),
            omo_line("user", "next"),
            serde_json::json!({"type": "custom_message", "customType": "omo-senpi:wake", "display": false, "details": [{"customType": "senpi-task.completion", "details": [{"task_id": "st-1", "status": "completed", "name": "worker-a", "final_response": "clean"}]}]}).to_string(),
        ]);
        home.write_session("--w--", "2026-10-06_aaa.jsonl", &body);
        for registry in ["omo", "omp"] {
            let identity = bound_identity(registry, "sess-1", "aaa");
            let stream = omo_stream(&home, &identity);
            let page = reference_history_page_from_bytes(&stream, &identity, body.as_bytes(), 1, None)
                .expect("the newest page is served");
            let title = page
                .turns
                .iter()
                .flat_map(|turn| turn.parts.iter())
                .find_map(|part| match part {
                    ReferencePart::TaskResult { tasks } => Some(tasks[0].title.clone()),
                    _ => None,
                })
                .expect("the wake is on the newest page");
            assert_eq!(title, "audit parser", "{registry}");
        }
    }

    #[test]
    fn the_cwd_fallback_never_binds_an_omo_store_to_an_omp_or_gjc_pane() {
        let home = TempHome::new();
        home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        for registry in ["omp", "gjc"] {
            let identity = identity(registry, "sess-z").with_cwd("/w");
            let refusal = resolve_reference_history_stream(home.path(), &identity, &[])
                .expect_err("an omo store in the cwd is not this reader's store");
            assert_eq!(refusal.code, ScopeErrorCode::NotFound, "{registry}");
        }
    }

    #[test]
    fn a_claude_clear_fences_paging_so_no_older_page_reaches_the_cleared_conversation() {
        let home = TempHome::new();
        let user = |text: &str| {
            serde_json::json!({"type": "user", "message": {"role": "user", "content": text}})
                .to_string()
        };
        let assistant = serde_json::json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": "ok"}]}}).to_string();
        let mut lines = Vec::new();
        for index in 0..5 {
            lines.push(user(&format!("cleared {index}")));
            lines.push(assistant.clone());
        }
        lines.push(user("<command-name>/clear</command-name>"));
        for index in 0..2 {
            lines.push(user(&format!("kept {index}")));
            lines.push(assistant.clone());
        }
        let body = format!("{}\n", lines.join("\n"));
        let dir = home.path().join("claude");
        std::fs::create_dir_all(&dir).expect("a claude folder is creatable");
        let path = dir.join("aaa.jsonl");
        std::fs::write(&path, &body).expect("a claude store is writable");

        let identity = bound_identity("claude", "sess-1", "aaa").with_provider_transcript_path(&path);
        let stream = omo_stream(&home, &identity);
        assert_eq!(stream.kind, ReferenceNativeHistoryKind::Claude);
        let page = reference_history_page_from_bytes(&stream, &identity, body.as_bytes(), 4, None)
            .expect("the newest page is served");
        assert_eq!(page.turns.len(), 4, "the two kept prompts and their answers");
        assert!(!page.has_more, "nothing before the clear may be paged in");
        assert!(page.cursor.is_none());
    }

    /// The open turn of an omo store: a prompt and an answer that called a tool and has not yet
    /// recorded its result. `grown` adds the tool result and the answer's closing text.
    fn open_turn_lines(prompt: &str, grown: bool) -> Vec<String> {
        let mut lines = vec![
            omo_line("user", prompt),
            serde_json::json!({"type": "message", "message": {"role": "assistant", "stopReason": "toolUse", "content": [{"type": "text", "text": "looking"}, {"type": "toolCall", "id": "t1", "name": "read", "arguments": {"path": "a"}}]}}).to_string(),
        ];
        if grown {
            lines.push(serde_json::json!({"type": "message", "message": {"role": "toolResult", "toolName": "read", "toolCallId": "t1", "content": [{"type": "text", "text": "contents"}]}}).to_string());
            lines.push(omo_line("assistant", "done"));
        }
        lines
    }

    // rs-history-r15-1
    #[test]
    fn the_open_tail_turn_growing_keeps_the_generation() {
        // A whole-store read: the newest turn grows in place, no existing record is rewritten.
        let home = TempHome::new();
        home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        let identity = bound_identity("omo", "sess-1", "aaa");
        let stream = omo_stream(&home, &identity);
        let closed = vec![omo_line("user", "U0"), omo_line("assistant", "A0")];
        let mut open_lines = closed.clone();
        open_lines.extend(open_turn_lines("U", false));
        let mut grown_lines = closed.clone();
        grown_lines.extend(open_turn_lines("U", true));
        let open = joined(&open_lines);
        let grown = joined(&grown_lines);
        let before = reference_history_page_from_bytes(&stream, &identity, open.as_bytes(), 10, None)
            .expect("the open turn is served");
        let after = reference_history_page_from_bytes(&stream, &identity, grown.as_bytes(), 10, None)
            .expect("the grown turn is served");
        assert_eq!(after.turns.len(), before.turns.len(), "the fixture grows a turn, it does not add one");
        assert_eq!(after.generation, before.generation, "growth of the open tail turn is an append");
        let mut rewritten_lines = grown_lines.clone();
        rewritten_lines[0] = omo_line("user", "rewritten");
        let rewritten = joined(&rewritten_lines);
        let rotated = reference_history_page_from_bytes(&stream, &identity, rewritten.as_bytes(), 10, None)
            .expect("the rewritten store is served");
        assert_ne!(rotated.generation, before.generation, "a rewritten closed turn still rotates");

        // A tail window whose head moves forward while its newest turn grows.
        let home = TempHome::new();
        home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        let stream = omo_stream(&home, &identity);
        let head = vec![omo_line("user", "U0"), omo_line("assistant", "A0")];
        let mut first_lines = head.clone();
        first_lines.extend(open_turn_lines("U", false));
        let mut second_lines = head[1..].to_vec();
        second_lines.extend(open_turn_lines("U", true));
        let first = joined(&first_lines);
        let second = joined(&second_lines);
        let shifted = 1000 + head[0].len() as u64 + 1;
        let before = reference_history_page_from_window(&stream, &identity, 1000, first.as_bytes(), 10, None)
            .expect("the first window is served")
            .generation;
        let after = reference_history_page_from_window(&stream, &identity, shifted, second.as_bytes(), 10, None)
            .expect("the shifted window is served")
            .generation;
        assert_eq!(after, before, "a moved head with a growing tail turn is an append");
    }

    // rs-history-r14-2
    #[test]
    fn a_store_growing_past_the_read_cap_keeps_the_generation() {
        let home = TempHome::new();
        home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        let identity = identity("omo", "aaa");
        let stream = omo_stream(&home, &identity);
        assert!(identity.provider_session_id().is_none(), "the stream has no provider session id");
        let mut lines = vec![
            omo_line("user", "U0"),
            omo_line("assistant", "A0"),
            omo_line("user", "U1"),
            omo_line("assistant", "A1"),
        ];
        let whole = joined(&lines);
        let before = reference_history_page_from_window(&stream, &identity, 0, whole.as_bytes(), 10, None)
            .expect("the whole store is served")
            .generation;
        lines.push(omo_line("user", "U2"));
        lines.push(omo_line("assistant", "A2"));
        let appended = joined(&lines);
        // The store passed the cap: the window now starts inside the first record.
        let window_start = 5usize;
        let after = reference_history_page_from_window(
            &stream,
            &identity,
            window_start as u64,
            &appended.as_bytes()[window_start..],
            10,
            None,
        )
        .expect("the tail window is served")
        .generation;
        assert_eq!(after, before, "an append that pushes the store past the read cap is not a change");
    }

    // rs-history-r15-2
    #[test]
    fn a_bounded_window_starting_on_a_line_boundary_keeps_its_first_record() {
        let home = TempHome::new();
        let cap = REFERENCE_HISTORY_MAX_BYTES;
        let base = joined(&[omo_line("user", "boundary prompt"), omo_line("assistant", "")]).len();
        let rest = joined(&[omo_line("user", "boundary prompt"), omo_line("assistant", &"p".repeat(cap - base))]);
        assert_eq!(rest.len(), cap, "the fixture tail is exactly the read cap");
        let head = format!("{}\n", "x".repeat(16));
        let path = home.write_session("--w--", "2026-10-06_aaa.jsonl", &format!("{head}{rest}"));
        let window = read_reference_history_window(&path, ReferenceNativeHistoryKind::Omo)
            .expect("the bounded window is read");
        assert_eq!(window.start, head.len() as u64, "the window starts on the record boundary");
        assert!(window.bytes == rest.as_bytes(), "the record the window starts on is kept whole");

        // A window that starts mid-record still drops the torn head.
        let torn = format!("{head}y{rest}");
        std::fs::write(&path, &torn).expect("the store is rewritable");
        let window = read_reference_history_window(&path, ReferenceNativeHistoryKind::Omo)
            .expect("the bounded window is read");
        let first_line = rest.find('\n').expect("the tail has a line") + 1;
        assert_eq!(window.start, (head.len() + 1 + first_line) as u64);
        assert!(window.bytes == rest.as_bytes()[first_line..], "the torn head line is dropped");
    }

    // rs-history-r13-3
    #[test]
    fn a_reset_inside_a_tail_window_fences_the_unread_history_disclosure() {
        let home = TempHome::new();
        home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        let identity = bound_identity("omo", "sess-1", "aaa");
        let stream = omo_stream(&home, &identity);
        let window = joined(&[
            omo_line("assistant", "tail of an older turn"),
            r#"{"type":"custom","customType":"context_clear"}"#.to_string(),
            omo_line("user", "B"),
            omo_line("assistant", "answer B"),
        ]);
        let page = reference_history_page_from_window(&stream, &identity, 1000, window.as_bytes(), 10, None)
            .expect("the oldest page of the window is served");
        assert_eq!(first_text(&page.turns[0]), "B");
        assert!(!page.has_more);
        assert_eq!(
            page.unavailable_reason, None,
            "nothing before the reset belongs to the conversation, so no older history is unread"
        );
    }

    #[tokio::test]
    async fn a_pi_store_past_the_read_cap_is_disclosed_as_scrollback_not_an_error() {
        let home = TempHome::new();
        let path = home.write_session("--w--", "2026-10-06_aaa.jsonl", "");
        std::fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_len(REFERENCE_HISTORY_MAX_PI_BYTES as u64 + 1))
            .expect("a sparse oversized store is creatable");
        let identity = bound_identity("pi", "sess-1", "aaa");
        let stream = omo_stream(&home, &identity);
        assert_eq!(stream.file_path(), Some(path.as_path()));

        let page = read_reference_history_page(&stream, &identity, 10, None)
            .await
            .expect("an oversized pi store is disclosed, not an error");
        assert_eq!(page.availability, ReferenceHistoryAvailability::Scrollback);
        assert!(page.turns.is_empty());
        assert!(
            page.unavailable_reason
                .as_deref()
                .is_some_and(|reason| reason.contains(super::super::history_pi::PI_BRANCH_UNREADABLE)),
            "{:?}",
            page.unavailable_reason
        );
    }

    #[tokio::test]
    async fn a_stream_read_off_the_reactor_matches_the_byte_reader() {
        let home = TempHome::new();
        let path = home.write_session("--w--", "2026-10-06_aaa.jsonl", THREE_TURNS);
        let identity = bound_identity("omo", "sess-1", "aaa");
        let stream = omo_stream(&home, &identity);
        assert_eq!(stream.file_path(), Some(path.as_path()));

        let from_file = read_reference_history_page(&stream, &identity, 2, None)
            .await
            .expect("the store is read through run_blocking");
        let from_bytes =
            reference_history_page_from_bytes(&stream, &identity, THREE_TURNS.as_bytes(), 2, None)
                .expect("the same page from bytes");
        assert_eq!(from_file, from_bytes);
    }
}
