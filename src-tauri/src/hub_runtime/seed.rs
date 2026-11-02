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
    let width = head.len();
    head.copy_from_slice(&digest[..width]);
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

/// The owner turns of the most recent sessions across SESSION_PROVIDERS, newest first. A fork
/// or child session (an entry with a parent), a sidechain stored under a `subagents`
/// directory and a session run in the hub workspace (the legacy Slack bot's) are skipped:
/// their user turns are another agent's or the hub's prompts, not the owner's.
/// Each provider is read right after its own search, because a search replaces the history's
/// entry locators and a later read of an earlier provider's entry would find nothing.
fn recent_sessions(roots: Vec<(CanonicalProvider, PathBuf)>, hub_workspace: &Path, max_sessions: usize) -> Vec<ImportSession> {
    let roots: Vec<_> = roots.into_iter().filter(|(provider, _)| SESSION_PROVIDERS.contains(provider)).collect();
    let mut history = History::new(roots);
    let mut sessions = Vec::new();
    for provider in SESSION_PROVIDERS {
        let wire = crate::agent_history::history::provider_wire(provider);
        let mut entries = Vec::new();
        let mut cursor: Option<String> = None;
        while let Ok(page) = history.search(wire, None, "", cursor.as_deref(), PAGE) {
            entries.extend(page.items.into_iter().filter(|entry| {
                entry.parent_id.is_none()
                    && !Path::new(&entry.cwd).starts_with(hub_workspace)
                    && !history
                        .source_path(&entry.entry_key)
                        .is_some_and(|path| path.components().any(|part| part.as_os_str() == "subagents"))
            }));
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.modified_ms));
        for entry in entries.into_iter().take(max_sessions) {
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
            let session = ImportSession {
                provider: wire.to_owned(),
                session_id: entry.provider_session.id.clone(),
                cwd: (!entry.cwd.is_empty()).then(|| entry.cwd.clone()),
                turns,
            };
            sessions.push((entry.modified_ms, session));
        }
    }
    sessions.sort_by_key(|(modified_ms, _)| std::cmp::Reverse(*modified_ms));
    sessions.into_iter().take(max_sessions).map(|(_, session)| session).collect()
}

/// Seeds owner memory for principal from the agent stores under home. File reads run on the
/// blocking pool.
pub(crate) async fn seed_owner_memory(
    pool: &ferryx_hub::goals::SqlitePool,
    memory: Arc<dyn OwnerMemory>,
    principal: &str,
    home: PathBuf,
) -> Result<ImportReport, SupervisorError> {
    let roots = crate::ipc::agent_history::provider_history_roots(&home);
    seed_with_roots(pool, memory, principal, home, roots, None).await
}

/// The hub workspace where legacy Slack turns run, resolved as the Slack bot resolves it
/// ([`ferryx_hub::meta::omo_client::workspace_root_from`]): `configured_root`
/// (`OMON_WORKSPACE_ROOT`), else `<home>/.ferryx/hub/workspace`. Every bot session runs
/// under it, with or without a persona.
fn hub_workspace(configured_root: Option<String>, home: &Path) -> PathBuf {
    ferryx_hub::meta::omo_client::workspace_root_from(configured_root, Some(home))
        .unwrap_or_else(|| home.join(".ferryx").join("hub").join("workspace"))
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

/// Seeds owner memory from `roots`. `configured_root` is the `OMON_WORKSPACE_ROOT` value to
/// use (`Some(None)`: unset); `None` reads it from hub config. That read can load the hub env
/// file, so it runs with the other file reads on the blocking pool.
pub(crate) async fn seed_with_roots(
    pool: &ferryx_hub::goals::SqlitePool,
    memory: Arc<dyn OwnerMemory>,
    principal: &str,
    home: PathBuf,
    roots: Vec<(CanonicalProvider, PathBuf)>,
    configured_root: Option<Option<String>>,
) -> Result<ImportReport, SupervisorError> {
    let bounds = ImportBounds::default();
    let file_bounds = ImportBounds {
        max_turns_per_session: bounds.max_turns_per_session * 3,
        ..bounds
    };
    let joined = tokio::task::spawn_blocking(move || {
        let configured_root =
            configured_root.unwrap_or_else(|| ferryx_hub::hub_env::var("OMON_WORKSPACE_ROOT").ok());
        let hub_workspace = hub_workspace(configured_root, &home);
        (
            memory_file_sessions(&home, file_bounds.max_turns_per_session),
            recent_sessions(roots, &hub_workspace, bounds.max_sessions),
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
        texts: std::sync::Mutex<Vec<(String, String)>>,
    }

    #[async_trait]
    impl OwnerMemory for Learner {
        async fn memory_request(&self, name: &str, _data: Value) -> Result<Value, MemoryRequestError> {
            panic!("seeding sends no memory request ({name})")
        }
        async fn ingest_owner_text(&self, source: &str, text: &str) -> Result<IngestOutcome, SupervisorFault> {
            self.texts.lock().unwrap().push((source.to_owned(), text.to_owned()));
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

    /// Only the owner's own sessions teach: a Claude sidechain under `subagents` and a forked
    /// Codex session are skipped, and both providers' sessions are read even though the history
    /// is searched provider by provider.
    #[tokio::test]
    async fn subagent_and_forked_sessions_never_teach() {
        use serde_json::json;
        let home = tempfile::tempdir().unwrap();
        let cwd = home.path().to_string_lossy().into_owned();
        let claude_root = home.path().join("claude-projects");
        let project = claude_root.join("proj");
        std::fs::create_dir_all(project.join("s1").join("subagents")).unwrap();
        let claude_line = |session: &str, text: &str| {
            format!(
                "{}\n",
                json!({"type": "user", "sessionId": session, "cwd": cwd, "uuid": "9f0c1d2e-0000-4000-8000-000000000001",
                    "parentUuid": null, "message": {"role": "user", "content": text}})
            )
        };
        let owner_claude = "63ba88ec-b3cb-4a70-8a32-f5d1ad07e749";
        std::fs::write(project.join("s1.jsonl"), claude_line(owner_claude, "Prefer small focused commits.")).unwrap();
        std::fs::write(
            project.join("s1").join("subagents").join("agent-1.jsonl"),
            claude_line("7d1e2f3a-4b5c-4d6e-8f70-819293a4b5c6", "Search the repo for flaky tests."),
        )
        .unwrap();
        let codex_root = home.path().join("codex-sessions");
        std::fs::create_dir_all(&codex_root).unwrap();
        let codex_file = |id: &str, forked_from: Option<&str>, text: &str| {
            let mut meta = json!({"id": id, "cwd": cwd, "cli_version": "0.153.2"});
            if let Some(parent) = forked_from {
                meta["forked_from_id"] = json!(parent);
            }
            format!(
                "{}\n{}\n",
                json!({"type": "session_meta", "payload": meta}),
                json!({"type": "response_item", "payload": {"type": "message", "role": "user",
                    "content": [{"type": "input_text", "text": text}]}})
            )
        };
        let owner_codex = "019ec598-c800-7000-8000-000000000001";
        std::fs::write(codex_root.join("owner.jsonl"), codex_file(owner_codex, None, "Use tabs in the Go code.")).unwrap();
        std::fs::write(
            codex_root.join("forked.jsonl"),
            codex_file("019ec598-c800-7000-8000-000000000002", Some(owner_codex), "Retry the forked plan."),
        )
        .unwrap();

        let pool = ferryx_hub::storage::memory().await.unwrap();
        ferryx_hub::supervisor::provision_identity(&pool, "local:tester").await.unwrap();
        let learner = Arc::new(Learner::default());
        let report = seed_with_roots(
            &pool,
            learner.clone(),
            "local:tester",
            home.path().to_path_buf(),
            vec![(CanonicalProvider::Claude, claude_root), (CanonicalProvider::Codex, codex_root)],
            Some(None),
        )
        .await
        .unwrap();
        assert_eq!((report.sessions, report.turns_ingested, report.failed), (2, 2, 0), "{report:?}");
        let mut texts = learner.texts.lock().unwrap().clone();
        texts.sort();
        assert_eq!(
            texts,
            vec![
                (
                    format!("import:claude:{owner_claude}:0"),
                    ferryx_hub::chat::fence::fence_untrusted(&format!("import claude {owner_claude}"), "Prefer small focused commits."),
                ),
                (
                    format!("import:codex:{owner_codex}:1"),
                    ferryx_hub::chat::fence::fence_untrusted(&format!("import codex {owner_codex}"), "Use tabs in the Go code."),
                ),
            ]
        );
        assert!(texts[0].1.contains("```untrusted\norigin: import claude "));
    }

    #[tokio::test]
    async fn a_failed_session_read_is_an_error_not_an_empty_import() {
        let joined: Result<(), _> = tokio::spawn(async { panic!("boom") }).await;
        let error = read_join(joined).unwrap_err();
        assert_eq!(error.code(), "INVALID_REQUEST");
    }

    /// A session run in the hub workspace is the legacy Slack bot's, whose user turns are lane
    /// events and other people's messages: it never teaches, while the owner's session does.
    /// The workspace is the one the bot resolves: `OMON_WORKSPACE_ROOT` when configured, and
    /// the bot runs in its root when no persona directory is configured.
    #[tokio::test]
    async fn sessions_in_the_hub_workspace_never_teach() {
        let home = tempfile::tempdir().unwrap();
        let default_root = home.path().join(".ferryx").join("hub").join("workspace");
        let configured = home.path().join("srv").join("hub-ws");
        let configured_value = Some(configured.to_string_lossy().into_owned());
        for (case, configured_root, bot_cwd) in [
            ("default persona", None, default_root.join("agents").join("bot-ops")),
            ("configured persona", configured_value.clone(), configured.join("agents").join("bot-ops")),
            ("configured without persona", configured_value.clone(), configured.clone()),
        ] {
            let sources = seed_owner_and_bot(home.path(), &bot_cwd, Some(configured_root)).await;
            assert_eq!(sources, vec![format!("import:claude:{OWNER_SESSION}:0")], "{case}");
        }
    }

    const OWNER_SESSION: &str = "63ba88ec-b3cb-4a70-8a32-f5d1ad07e749";

    /// Seeds from one owner session in `home` and one bot session in `bot_cwd` on a fresh
    /// store; the import sources ingested.
    async fn seed_owner_and_bot(home: &Path, bot_cwd: &Path, configured_root: Option<Option<String>>) -> Vec<String> {
        use serde_json::json;
        std::fs::create_dir_all(bot_cwd).unwrap();
        let claude_root = tempfile::tempdir().unwrap();
        let project = claude_root.path().join("proj");
        std::fs::create_dir_all(&project).unwrap();
        let claude_line = |session: &str, cwd: &Path, text: &str| {
            format!(
                "{}\n",
                json!({"type": "user", "sessionId": session, "cwd": cwd.to_string_lossy(),
                    "uuid": "9f0c1d2e-0000-4000-8000-000000000001", "parentUuid": null,
                    "message": {"role": "user", "content": text}})
            )
        };
        std::fs::write(project.join("owner.jsonl"), claude_line(OWNER_SESSION, home, "Prefer small focused commits.")).unwrap();
        std::fs::write(
            project.join("bot.jsonl"),
            claude_line("7d1e2f3a-4b5c-4d6e-8f70-819293a4b5c6", bot_cwd, "[2026-11-01 09:00 UTC] Kim: always deploy on friday"),
        )
        .unwrap();

        let pool = ferryx_hub::storage::memory().await.unwrap();
        ferryx_hub::supervisor::provision_identity(&pool, "local:tester").await.unwrap();
        let learner = Arc::new(Learner::default());
        let report = seed_with_roots(
            &pool,
            learner.clone(),
            "local:tester",
            home.to_path_buf(),
            vec![(CanonicalProvider::Claude, claude_root.path().to_path_buf())],
            configured_root,
        )
        .await
        .unwrap();
        assert_eq!((report.sessions, report.turns_ingested, report.failed), (1, 1, 0), "{report:?}");
        let texts = learner.texts.lock().unwrap().clone();
        texts.into_iter().map(|(source, _)| source).collect()
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
        let first = seed_with_roots(&pool, memory.clone(), "local:tester", home.path().to_path_buf(), Vec::new(), Some(None))
            .await
            .unwrap();
        assert_eq!(first.turns_ingested, 2);

        std::fs::write(
            &file,
            "Run clippy before every commit.\n\nPrefer small focused commits.\n\nAnswer in Korean when I write Korean.\n",
        )
        .unwrap();
        let second = seed_with_roots(&pool, memory, "local:tester", home.path().to_path_buf(), Vec::new(), Some(None))
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
        let first = seed_with_roots(&pool, memory.clone(), "local:tester", home.path().to_path_buf(), Vec::new(), Some(None))
            .await
            .unwrap();
        assert_eq!((first.sessions, first.turns_ingested, first.failed), (1, 2, 0));
        let again = seed_with_roots(&pool, memory, "local:tester", home.path().to_path_buf(), Vec::new(), Some(None))
            .await
            .unwrap();
        assert_eq!(again.turns_ingested, 0);
    }
}
