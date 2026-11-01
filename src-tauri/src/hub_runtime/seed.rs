//! Seeds the owner's supervisor memory once, after setup opted in: the owner's own words from
//! recent agent sessions (Claude, Codex, omo) and the owner-authored global agent instruction
//! files. Everything goes through memory_import::import_sessions, which drops hub-driven
//! sessions, wrappers and secrets and makes a rerun ingest nothing new. Only counts are logged.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ferryx_hub::meta::mhc_supervisor::OwnerMemory;
use ferryx_hub::supervisor::memory_import::{self, ImportBounds, ImportReport, ImportSession, ImportTurn};
use ferryx_hub::supervisor::SupervisorError;

use crate::agent_history::history::History;
use crate::scoped_contracts::CanonicalProvider;

/// Providers whose sessions the owner typed into.
const SESSION_PROVIDERS: [CanonicalProvider; 3] = [CanonicalProvider::Claude, CanonicalProvider::Codex, CanonicalProvider::Omo];

/// Largest instruction file read.
const MAX_MEMORY_FILE_BYTES: u64 = 64 * 1024;

/// Page size of one history read (the history reader's maximum).
const PAGE: usize = 100;

/// Owner-authored global instruction files: (tool, path under home). Agent-written memory
/// (for example ~/.omo/memory) is never read.
fn memory_files(home: &Path) -> Vec<(&'static str, PathBuf)> {
    vec![
        ("claude", home.join(".claude").join("CLAUDE.md")),
        ("codex", home.join(".codex").join("AGENTS.md")),
        ("opencode", home.join(".config").join("opencode").join("AGENTS.md")),
        ("omo", home.join(".omo").join("agent").join("AGENTS.md")),
    ]
}

/// Non-empty blank-line-separated paragraphs of text, at most max, each once. A paragraph's
/// ordinal (and so its import source id) is a hash of its own text rather than its position,
/// so inserting, removing or moving one paragraph does not give every other paragraph a new
/// id and make owner ingest learn the whole file again. A repeated paragraph keeps its first
/// occurrence only, since both copies would share one id.
fn paragraphs(text: &str, max: usize) -> Vec<ImportTurn> {
    let normalized = text.replace("\r\n", "\n");
    let mut seen = std::collections::HashSet::new();
    normalized
        .split("\n\n")
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .map(|paragraph| ImportTurn { ordinal: paragraph_id(paragraph), text: paragraph.to_owned() })
        .filter(|turn| seen.insert(turn.ordinal))
        .take(max)
        .collect()
}

/// The leading bytes of the paragraph's SHA-256 as a number.
fn paragraph_id(paragraph: &str) -> usize {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(paragraph.as_bytes());
    let mut head = [0u8; std::mem::size_of::<usize>()];
    head.copy_from_slice(&digest[..head.len()]);
    usize::from_be_bytes(head)
}

/// One import session per readable instruction file (provider memory, id <tool>:<file>).
fn memory_file_sessions(home: &Path, max_paragraphs: usize) -> Vec<ImportSession> {
    use std::io::Read;
    let mut sessions = Vec::new();
    for (tool, path) in memory_files(home) {
        let Ok(file) = std::fs::File::open(&path) else { continue };
        let mut bytes = Vec::new();
        if file.take(MAX_MEMORY_FILE_BYTES).read_to_end(&mut bytes).is_err() {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        let turns = paragraphs(&text, max_paragraphs);
        if turns.is_empty() {
            continue;
        }
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        sessions.push(ImportSession {
            provider: "memory".into(),
            session_id: format!("{tool}:{name}"),
            cwd: None,
            turns,
        });
    }
    sessions
}

/// The owner turns of the most recent sessions across SESSION_PROVIDERS, newest first.
fn recent_sessions(roots: Vec<(CanonicalProvider, PathBuf)>, max_sessions: usize) -> Vec<ImportSession> {
    let roots: Vec<_> = roots.into_iter().filter(|(provider, _)| SESSION_PROVIDERS.contains(provider)).collect();
    let mut history = History::new(roots);
    let mut entries = Vec::new();
    for provider in SESSION_PROVIDERS {
        let wire = crate::agent_history::history::provider_wire(provider);
        let mut cursor: Option<String> = None;
        while let Ok(page) = history.search(wire, None, "", cursor.as_deref(), PAGE) {
            entries.extend(page.items.into_iter().map(|entry| (wire, entry)));
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
    }
    entries.sort_by_key(|(_, entry)| std::cmp::Reverse(entry.modified_ms));
    let mut sessions = Vec::new();
    for (wire, entry) in entries.into_iter().take(max_sessions) {
        let mut turns = Vec::new();
        let mut cursor: Option<String> = None;
        while let Ok(page) = history.read(&entry.entry_key, cursor.as_deref(), PAGE) {
            turns.extend(
                page.items
                    .into_iter()
                    .filter(|message| message.role == "user")
                    .map(|message| ImportTurn { ordinal: message.ordinal, text: message.text }),
            );
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        sessions.push(ImportSession {
            provider: wire.to_owned(),
            session_id: entry.provider_session.id.clone(),
            cwd: (!entry.cwd.is_empty()).then(|| entry.cwd.clone()),
            turns,
        });
    }
    sessions
}

/// Seeds owner memory for principal from the agent stores under home. File reads run on the
/// blocking pool.
pub(crate) async fn seed_owner_memory(
    pool: &ferryx_hub::goals::SqlitePool,
    memory: Arc<dyn OwnerMemory>,
    principal: &str,
    home: PathBuf,
) -> Result<ImportReport, SupervisorError> {
    seed_with_roots(pool, memory, principal, home.clone(), crate::ipc::agent_history::provider_history_roots(&home)).await
}

/// The result of the blocking session read. A read that panicked or was cancelled is an error,
/// not an empty read: importing nothing would still record a completed run, and setup would
/// treat memory as seeded although no session was looked at.
fn read_join<T>(joined: Result<T, tokio::task::JoinError>) -> Result<T, SupervisorError> {
    joined.map_err(|error| {
        tracing::warn!(%error, "reading agent sessions for owner memory failed");
        SupervisorError::Invalid(format!("reading agent sessions failed: {error}"))
    })
}

pub(crate) async fn seed_with_roots(
    pool: &ferryx_hub::goals::SqlitePool,
    memory: Arc<dyn OwnerMemory>,
    principal: &str,
    home: PathBuf,
    roots: Vec<(CanonicalProvider, PathBuf)>,
) -> Result<ImportReport, SupervisorError> {
    let bounds = ImportBounds::default();
    let file_bounds = ImportBounds {
        max_turns_per_session: bounds.max_turns_per_session * 3,
        ..bounds
    };
    let joined = tokio::task::spawn_blocking(move || {
        (
            memory_file_sessions(&home, file_bounds.max_turns_per_session),
            recent_sessions(roots, bounds.max_sessions),
        )
    })
    .await;
    let (files, sessions) = read_join(joined)?;
    let from_files = memory_import::import_sessions(pool, memory.as_ref(), principal, files, file_bounds).await?;
    let from_sessions = memory_import::import_sessions(pool, memory.as_ref(), principal, sessions, bounds).await?;
    let report = ImportReport {
        sessions: from_files.sessions + from_sessions.sessions,
        turns_ingested: from_files.turns_ingested + from_sessions.turns_ingested,
        duplicates: from_files.duplicates + from_sessions.duplicates,
        skipped: from_files.skipped + from_sessions.skipped,
        failed: from_files.failed + from_sessions.failed,
    };
    tracing::info!(
        sessions = report.sessions,
        ingested = report.turns_ingested,
        duplicates = report.duplicates,
        skipped = report.skipped,
        failed = report.failed,
        "owner memory seeded"
    );
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ferryx_hub::meta::mhc_fault::SupervisorFault;
    use ferryx_hub::meta::mhc_supervisor::{IngestOutcome, MemoryRequestError};
    use serde_json::Value;

    #[derive(Default)]
    struct Learner {
        seen: std::sync::Mutex<std::collections::HashSet<String>>,
    }

    #[async_trait]
    impl OwnerMemory for Learner {
        async fn memory_request(&self, name: &str, _data: Value) -> Result<Value, MemoryRequestError> {
            panic!("seeding sends no memory request ({name})")
        }
        async fn ingest_owner_text(&self, source: &str, _text: &str) -> Result<IngestOutcome, SupervisorFault> {
            let fresh = self.seen.lock().unwrap().insert(source.to_owned());
            Ok(if fresh { IngestOutcome::Learned { reply: String::new() } } else { IngestOutcome::Duplicate })
        }
    }

    #[test]
    fn instruction_files_split_into_bounded_paragraphs() {
        let turns = paragraphs("one\r\n\r\n\n two \n\nthree\n\nfour", 3);
        let texts: Vec<_> = turns.iter().map(|turn| turn.text.as_str()).collect();
        assert_eq!(texts, vec!["one", "two", "three"]);
        // The id follows the text, not its position; a repeated paragraph is kept once.
        let moved = paragraphs("zero\n\nthree\n\none\n\nthree", 10);
        let moved: Vec<_> = moved.iter().map(|turn| (turn.ordinal, turn.text.as_str())).collect();
        assert_eq!(moved, vec![(paragraph_id("zero"), "zero"), (turns[2].ordinal, "three"), (turns[0].ordinal, "one")]);
        assert_ne!(turns[0].ordinal, turns[1].ordinal);
    }

    #[tokio::test]
    async fn a_failed_session_read_is_an_error_not_an_empty_import() {
        let joined: Result<(), _> = tokio::spawn(async { panic!("boom") }).await;
        let error = read_join(joined).unwrap_err();
        assert_eq!(error.code(), "INVALID_REQUEST");
    }

    #[tokio::test]
    async fn an_inserted_paragraph_keeps_the_ids_of_the_others() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        let file = claude.join("CLAUDE.md");
        std::fs::write(&file, "Prefer small focused commits.\n\nAnswer in Korean when I write Korean.\n").unwrap();
        let pool = ferryx_hub::storage::memory().await.unwrap();
        ferryx_hub::supervisor::provision_identity(&pool, "local:tester").await.unwrap();
        let memory: Arc<dyn OwnerMemory> = Arc::new(Learner::default());
        let first = seed_with_roots(&pool, memory.clone(), "local:tester", home.path().to_path_buf(), Vec::new())
            .await
            .unwrap();
        assert_eq!(first.turns_ingested, 2);

        std::fs::write(
            &file,
            "Run clippy before every commit.\n\nPrefer small focused commits.\n\nAnswer in Korean when I write Korean.\n",
        )
        .unwrap();
        let second = seed_with_roots(&pool, memory, "local:tester", home.path().to_path_buf(), Vec::new())
            .await
            .unwrap();
        assert_eq!((second.turns_ingested, second.failed), (1, 0), "{second:?}");
    }

    #[tokio::test]
    async fn owner_instruction_files_seed_memory_once_without_secrets_or_agent_memory() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join("CLAUDE.md"),
            "Prefer small focused commits.\n\nAnswer in Korean when I write Korean.\n\nMy key is sk-test-0123456789abcdef\n",
        )
        .unwrap();
        let agent_memory = home.path().join(".omo").join("memory");
        std::fs::create_dir_all(&agent_memory).unwrap();
        std::fs::write(agent_memory.join("AGENTS.md"), "agent-derived note\n").unwrap();

        let pool = ferryx_hub::storage::memory().await.unwrap();
        ferryx_hub::supervisor::provision_identity(&pool, "local:tester").await.unwrap();
        let memory: Arc<dyn OwnerMemory> = Arc::new(Learner::default());
        let first = seed_with_roots(&pool, memory.clone(), "local:tester", home.path().to_path_buf(), Vec::new())
            .await
            .unwrap();
        assert_eq!((first.sessions, first.turns_ingested, first.failed), (1, 2, 0));
        let again = seed_with_roots(&pool, memory, "local:tester", home.path().to_path_buf(), Vec::new())
            .await
            .unwrap();
        assert_eq!(again.turns_ingested, 0);
    }
}
