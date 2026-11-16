//! The owner's agent memory (`~/.omo/memory`) as an owner-memory source. Only Markdown files
//! under `agents/*/repo` are read, never written; `.git`, `transient-runs` and `runtime`
//! directories and symlinks are skipped. Two classes:
//!
//! - core memory (`system/**/*.md`, `people/**/card.md`, `people/**/observations.md`) goes to
//!   owner ingest through [`memory_import`]: one import session per distinct file content
//!   (provider `omo-memory`, id `omo-memory:<sha256>`; identical files of several agents are
//!   sent once), one paragraph per turn, frontmatter and credential-shaped paragraphs dropped.
//!   An unchanged file is not sent again, a changed one is. At most [`ingest_budget`] new turns
//!   are sent per pass, so a large backlog drains over passes;
//! - notes and reference files (`notes/**`, `reference/**`) are never sent to the engine: they
//!   are indexed for retrieval ([`ferryx_hub::supervisor::reference_docs`]).
//!
//! A pass runs on every engine transition to Ready and hourly while the generation runs, only
//! when an owner exists and the owner turned memory import on.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ferryx_hub::meta::mhc_supervisor::{HealthState, OwnerMemory, SupervisorHealth};
use ferryx_hub::supervisor::memory_import::{self, ImportBounds, ImportReport, ImportSession};
use ferryx_hub::supervisor::reference_docs::{self, IndexReport, ReferenceDoc};
use ferryx_hub::supervisor::SupervisorError;
use serde_json::json;

/// Import provider of core memory files.
const PROVIDER: &str = memory_import::OMO_MEMORY_PROVIDER;

/// Largest file read.
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// Leading body bytes a reference doc's keywords are drawn from.
const INDEX_BODY_BYTES: usize = 2 * 1024;

/// Directory names never entered.
const SKIPPED_DIRS: [&str; 3] = [".git", "transient-runs", "runtime"];

/// Env key of the per-pass owner-ingest turn budget, and its default.
const BUDGET_KEY: &str = "FERRYX_HUB_OMO_MEMORY_INGEST_BUDGET";
const DEFAULT_BUDGET: usize = 200;

/// Time between passes while the engine stays Ready.
const PASS_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Bounds of one core-memory import: many small files, each up to 30 paragraphs.
const BOUNDS: ImportBounds = ImportBounds {
    max_sessions: 400,
    max_turns_per_session: 30,
    max_chars: 2000,
};

/// What one scan of the memory tree found.
#[derive(Debug, Default)]
struct Scan {
    sessions: Vec<ImportSession>,
    docs: Vec<ReferenceDoc>,
    /// Core files read, before identical contents were merged.
    core_files: usize,
}

/// What one pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OmoMemoryReport {
    pub ingest: ImportReport,
    pub index: IndexReport,
}

/// The per-pass owner-ingest turn budget: `FERRYX_HUB_OMO_MEMORY_INGEST_BUDGET` when it is a
/// number, else 200.
fn ingest_budget() -> usize {
    ferryx_hub::hub_env::var(BUDGET_KEY)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(DEFAULT_BUDGET)
}

/// Where a file under `agents/<agent>/repo` belongs, by its path relative to the repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Core,
    Reference,
}

fn class_of(relative: &Path) -> Option<Class> {
    if relative.extension().and_then(|ext| ext.to_str()) != Some("md") {
        return None;
    }
    let first = relative.components().next()?.as_os_str().to_str()?;
    let name = relative.file_name()?.to_str()?;
    match first {
        "system" => Some(Class::Core),
        "people" if name == "card.md" || name == "observations.md" => Some(Class::Core),
        "notes" | "reference" => Some(Class::Reference),
        _ => None,
    }
}

/// Every regular `.md` file under `dir`, depth first in name order, skipping [`SKIPPED_DIRS`]
/// and symlinks (a link could lead outside the repo).
fn markdown_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let Ok(kind) = entry.file_type() else { continue };
        let path = entry.path();
        if kind.is_dir() {
            let name = entry.file_name();
            if !SKIPPED_DIRS.iter().any(|skip| name.to_str() == Some(*skip)) {
                markdown_files(&path, out);
            }
        } else if kind.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

fn read_capped(path: &Path) -> Option<String> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// `(frontmatter, body)`: the leading `---` block, when the text opens with one.
fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    let Some(rest) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) else {
        return (None, text);
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return (Some(&rest[..offset]), &rest[offset + line.len()..]);
        }
        offset += line.len();
    }
    (None, text)
}

fn description(frontmatter: Option<&str>) -> String {
    frontmatter
        .into_iter()
        .flat_map(str::lines)
        .find_map(|line| line.strip_prefix("description:"))
        .map(|value| value.trim().trim_matches(|c| c == '"' || c == '\'').to_owned())
        .unwrap_or_default()
}

/// `text` without the lines that look like a credential.
fn without_secret_lines(text: &str) -> String {
    text.lines()
        .filter(|line| !memory_import::looks_like_secret(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn head(text: &str, max_bytes: usize) -> &str {
    let mut end = text.len().min(max_bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Reads `<home>/.omo/memory/agents/*/repo`. Core files are merged by content; the first path
/// in name order keeps a shared content.
fn scan(home: &Path) -> Scan {
    let root = home.join(".omo").join("memory");
    let mut scan = Scan::default();
    let Ok(agents) = std::fs::read_dir(root.join("agents")) else { return scan };
    let mut agents: Vec<_> = agents.flatten().collect();
    agents.sort_by_key(|entry| entry.file_name());
    let mut seen = std::collections::HashSet::new();
    for agent in agents {
        let repo = agent.path().join("repo");
        let is_dir = agent.file_type().is_ok_and(|kind| kind.is_dir())
            && std::fs::symlink_metadata(&repo).is_ok_and(|meta| meta.is_dir());
        if !is_dir {
            continue;
        }
        let agent_name = agent.file_name().to_string_lossy().into_owned();
        let mut files = Vec::new();
        markdown_files(&repo, &mut files);
        for path in files {
            let Ok(relative) = path.strip_prefix(&repo) else { continue };
            let Some(class) = class_of(relative) else { continue };
            let Some(text) = read_capped(&path) else { continue };
            let text = text.replace("\r\n", "\n");
            match class {
                Class::Core => {
                    scan.core_files += 1;
                    let sha = ferryx_hub::usage::sha256_hex(&text);
                    if !seen.insert(sha.clone()) {
                        continue;
                    }
                    let (_, body) = split_frontmatter(&text);
                    let turns = super::seed::paragraphs(body, BOUNDS.max_turns_per_session);
                    if turns.is_empty() {
                        continue;
                    }
                    scan.sessions.push(ImportSession {
                        provider: PROVIDER.to_owned(),
                        session_id: format!("{PROVIDER}:{sha}"),
                        cwd: None,
                        turns,
                    });
                }
                Class::Reference => {
                    let (frontmatter, body) = split_frontmatter(&text);
                    let description = without_secret_lines(&description(frontmatter));
                    let shown = Path::new("agents").join(&agent_name).join("repo").join(relative);
                    let shown = shown.to_string_lossy().into_owned();
                    let keyed = without_secret_lines(&format!("{shown}\n{description}\n{}", head(body, INDEX_BODY_BYTES)));
                    scan.docs.push(ReferenceDoc {
                        path: shown,
                        agent: agent_name.clone(),
                        description,
                        body_sha256: ferryx_hub::usage::sha256_hex(&text),
                        keywords: reference_docs::keywords(&keyed),
                    });
                }
            }
        }
    }
    scan
}

/// One pass: indexes the reference files and sends at most `budget` new core-memory turns to
/// owner ingest, attributed to [`memory_import::OMO_MEMORY_CAUSE`]; the pass summary (report,
/// engine calls and token sums) is recorded in `hub_meta`. Only counts are logged.
pub(crate) async fn seed_omo_memory(
    pool: &ferryx_hub::goals::SqlitePool,
    memory: Arc<dyn OwnerMemory>,
    principal: &str,
    home: PathBuf,
    budget: usize,
) -> Result<OmoMemoryReport, SupervisorError> {
    let started_at = ferryx_hub::storage::now_rfc3339();
    let scan = tokio::task::spawn_blocking(move || scan(&home)).await.map_err(|error| {
        tracing::warn!(%error, "reading the agent memory failed");
        SupervisorError::Invalid(format!("reading the agent memory failed: {error}"))
    })?;
    let index = reference_docs::index_reference_docs(pool, &scan.docs).await?;
    let distinct = scan.sessions.len();
    let ingest = ferryx_hub::usage::with_cause(
        ferryx_hub::usage::CallCause::new(memory_import::OMO_MEMORY_CAUSE),
        memory_import::import_sessions_budgeted(pool, memory.as_ref(), principal, scan.sessions, BOUNDS, Some(budget)),
    )
    .await?;
    let extra = json!({
        "coreFiles": scan.core_files,
        "distinctCoreFiles": distinct,
        "referenceDocs": scan.docs.len(),
        "indexed": index,
        "budget": budget,
    });
    let pass = memory_import::record_pass(
        pool,
        memory_import::OMO_MEMORY_PASS_KEY,
        memory_import::OMO_MEMORY_CAUSE,
        &started_at,
        &ingest,
        extra,
    )
    .await?;
    tracing::info!(
        core_files = scan.core_files,
        distinct,
        sessions = ingest.sessions,
        ingested = ingest.turns_ingested,
        duplicates = ingest.duplicates,
        skipped = ingest.skipped,
        failed = ingest.failed,
        indexed = index.indexed,
        unchanged = index.unchanged,
        input_tokens = ?pass["inputTokens"].as_i64(),
        output_tokens = ?pass["outputTokens"].as_i64(),
        "agent memory pass"
    );
    Ok(OmoMemoryReport { ingest, index })
}

/// Runs a pass when an owner exists and memory import is on; otherwise nothing.
pub(crate) async fn pass_if_wanted(
    engine: &ferryx_hub::meta::mhc_supervisor::MhcSupervisor,
    pool: &ferryx_hub::goals::SqlitePool,
    home: PathBuf,
) {
    let status = match ferryx_hub::supervisor::setup::setup_status(pool, Default::default()).await {
        Ok(status) => status,
        Err(error) => {
            tracing::warn!(code = error.code(), "setup status was not read for the agent memory pass");
            return;
        }
    };
    let (Some(owner), true) = (status.owner, status.memory_import.enabled) else { return };
    let memory: Arc<dyn OwnerMemory> = Arc::new(engine.clone());
    match seed_omo_memory(pool, memory, &owner.owner_principal, home, ingest_budget()).await {
        Ok(report) => tracing::debug!(
            ingested = report.ingest.turns_ingested,
            duplicates = report.ingest.duplicates,
            indexed = report.index.indexed,
            "agent memory pass finished"
        ),
        Err(error) => tracing::warn!(code = error.code(), "agent memory pass failed"),
    }
}

/// Runs `pass` on every transition of `health` to Ready and every [`PASS_INTERVAL`] while it
/// stays Ready. Returns when the channel closes (the generation ended).
pub(crate) async fn on_ready_and_hourly<F, Fut>(mut health: tokio::sync::watch::Receiver<SupervisorHealth>, mut pass: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut was_ready = false;
    let mut hourly = tokio::time::interval(PASS_INTERVAL);
    hourly.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    hourly.tick().await;
    loop {
        let ready = health.borrow_and_update().state == HealthState::Ready;
        if ready && !was_ready {
            pass().await;
            hourly.reset();
        }
        was_ready = ready;
        tokio::select! {
            changed = health.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            _ = hourly.tick() => {
                if was_ready {
                    pass().await;
                }
            }
        }
    }
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
            panic!("the agent memory pass sends no memory request ({name})")
        }
        async fn ingest_owner_text(&self, source: &str, text: &str) -> Result<IngestOutcome, SupervisorFault> {
            self.texts.lock().unwrap().push((source.to_owned(), text.to_owned()));
            let fresh = self.seen.lock().unwrap().insert(source.to_owned());
            Ok(if fresh { IngestOutcome::Learned { reply: String::new() } } else { IngestOutcome::Duplicate })
        }
    }

    impl Learner {
        fn sent(&self) -> Vec<String> {
            self.texts.lock().unwrap().iter().map(|(_, text)| text.clone()).collect()
        }
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    async fn store() -> ferryx_hub::goals::SqlitePool {
        let pool = ferryx_hub::storage::memory().await.unwrap();
        ferryx_hub::supervisor::provision_identity(&pool, "local:tester").await.unwrap();
        pool
    }

    /// A memory tree in a temp home, never the real one.
    fn tree() -> tempfile::TempDir {
        let home = tempfile::tempdir().unwrap();
        let memory = home.path().join(".omo").join("memory");
        let a = memory.join("agents").join("alpha").join("repo");
        let b = memory.join("agents").join("beta").join("repo");
        write(
            &a.join("system").join("human.md"),
            "---\ndescription: Person - Human\n---\nPrefer small focused commits.\n\nOPENAI_API_KEY=sk-test-0123456789abcdef\n",
        );
        // The same content under another agent is sent once.
        write(
            &b.join("system").join("human.md"),
            "---\ndescription: Person - Human\n---\nPrefer small focused commits.\n\nOPENAI_API_KEY=sk-test-0123456789abcdef\n",
        );
        write(&a.join("system").join("human").join("preferences.md"), "Answer in Korean when I write Korean.\n");
        write(&a.join("people").join("kim").join("card.md"), "Kim reviews the mobile builds.\n");
        write(&a.join("people").join("kim").join("misc.md"), "never read: not a card or observations\n");
        write(&a.join("skills").join("x.md"), "never read: skills\n");
        write(
            &a.join("notes").join("ferryx").join("build-host.md"),
            "---\ndescription: Ferryx builds run on maho-mac\n---\ncargo test only on maho-mac.\nGITHUB_TOKEN=ghp_abcdefghijklmnop\n",
        );
        write(&b.join("reference").join("cooking.md"), "---\ndescription: Recipes\n---\npasta\n");
        write(&a.join(".git").join("notes").join("x.md"), "never read: git\n");
        write(&a.join("transient-runs").join("system").join("t.md"), "never read: transient\n");
        write(&a.join("runtime").join("system").join("r.md"), "never read: runtime\n");
        // Outside agents/*/repo.
        write(&memory.join("reference").join("root.md"), "never read: outside agents\n");
        write(&memory.join("agents").join("alpha").join("system").join("x.md"), "never read: outside repo\n");
        home
    }

    #[tokio::test]
    async fn core_memory_is_ingested_once_and_notes_are_only_indexed() {
        let home = tree();
        let pool = store().await;
        let learner = Arc::new(Learner::default());
        let first = seed_omo_memory(&pool, learner.clone(), "local:tester", home.path().to_path_buf(), 200).await.unwrap();
        assert_eq!((first.ingest.sessions, first.ingest.turns_ingested, first.ingest.failed), (3, 3, 0), "{first:?}");
        assert_eq!(first.index, IndexReport { indexed: 2, unchanged: 0 });
        let sent = learner.sent().join("\n");
        for wanted in ["Prefer small focused commits.", "Answer in Korean when I write Korean.", "Kim reviews the mobile builds."] {
            assert!(sent.contains(wanted), "{wanted} missing from {sent}");
        }
        for unwanted in ["never read", "sk-test", "ghp_", "description:", "maho-mac", "pasta"] {
            assert!(!sent.contains(unwanted), "{unwanted} was sent: {sent}");
        }
        assert!(learner.texts.lock().unwrap().iter().all(|(source, _)| source.starts_with("import:omo-memory:omo-memory:")));

        let hits = reference_docs::search_reference(&pool, "maho-mac build", 5).await.unwrap();
        assert_eq!(hits[0].path, "agents/alpha/repo/notes/ferryx/build-host.md");
        assert_eq!(hits[0].description, "Ferryx builds run on maho-mac");
        assert!(reference_docs::search_reference(&pool, "ghp_abcdefghijklmnop", 5).await.unwrap().is_empty());
        assert!(reference_docs::search_reference(&pool, "never", 5).await.unwrap().is_empty());

        // Unchanged: nothing is sent or rewritten.
        let again = seed_omo_memory(&pool, learner.clone(), "local:tester", home.path().to_path_buf(), 200).await.unwrap();
        assert_eq!((again.ingest.turns_ingested, again.ingest.skipped), (0, 3), "{again:?}");
        assert_eq!(again.index, IndexReport { indexed: 0, unchanged: 2 });
        let sent_before = learner.texts.lock().unwrap().len();
        assert_eq!(sent_before, 3);

        // A changed file is learned again; a changed note is re-indexed.
        let repo = home.path().join(".omo/memory/agents/alpha/repo");
        write(&repo.join("people/kim/card.md"), "Kim reviews the mobile builds.\n\nKim prefers Slack threads.\n");
        write(&repo.join("notes/ferryx/build-host.md"), "---\ndescription: Builds\n---\nmaho-mac only\n");
        let changed = seed_omo_memory(&pool, learner.clone(), "local:tester", home.path().to_path_buf(), 200).await.unwrap();
        assert_eq!(changed.ingest.turns_ingested, 2, "{changed:?}");
        assert_eq!(changed.index, IndexReport { indexed: 1, unchanged: 1 });
        assert!(learner.sent().join("\n").contains("Kim prefers Slack threads."));

        // The agent-memory pass never counts as the session seed having run.
        assert_eq!(memory_import::import_totals(&pool).await.unwrap().last_at, None);

        let pass = memory_import::last_pass(&pool, memory_import::OMO_MEMORY_PASS_KEY).await.unwrap().unwrap();
        assert_eq!(pass["report"]["turnsIngested"], 2);
        assert_eq!(pass["coreFiles"], 4);
        assert_eq!(pass["distinctCoreFiles"], 3);
    }

    #[tokio::test]
    async fn the_budget_drains_a_backlog_over_passes() {
        let home = tempfile::tempdir().unwrap();
        let repo = home.path().join(".omo/memory/agents/alpha/repo");
        write(&repo.join("system/a.md"), "one\n\ntwo\n\nthree\n");
        write(&repo.join("system/b.md"), "four\n");
        let pool = store().await;
        let learner = Arc::new(Learner::default());
        let mut drained = Vec::new();
        for _ in 0..4 {
            let pass = seed_omo_memory(&pool, learner.clone(), "local:tester", home.path().to_path_buf(), 2).await.unwrap();
            drained.push(pass.ingest.turns_ingested);
        }
        assert_eq!(drained, [2, 2, 0, 0]);
        let sent = learner.sent();
        assert_eq!(sent.len(), 4, "every paragraph is sent exactly once: {sent:?}");
        for paragraph in ["one", "two", "three", "four"] {
            assert_eq!(sent.iter().filter(|text| text.contains(&format!("\n{paragraph}\n"))).count(), 1, "{paragraph}: {sent:?}");
        }
    }

    #[test]
    fn frontmatter_is_split_off() {
        let (frontmatter, body) = split_frontmatter("---\ndescription: \"Explicit facts\"\nkind: x\n---\nbody\n");
        assert_eq!(description(frontmatter), "Explicit facts");
        assert_eq!(body, "body\n");
        assert_eq!(split_frontmatter("no frontmatter\n"), (None, "no frontmatter\n"));
        assert_eq!(split_frontmatter("---\nunterminated\n"), (None, "---\nunterminated\n"));
    }

    fn health(state: HealthState) -> SupervisorHealth {
        SupervisorHealth {
            state,
            generation: 1,
            pid: None,
            capabilities: Vec::new(),
            server_version: None,
            fault: None,
            detail: None,
            queued_decisions: 0,
        }
    }

    /// A pass runs on every transition to Ready and once per interval while Ready, never while
    /// not Ready, and the loop ends with its channel.
    #[tokio::test(start_paused = true)]
    async fn passes_run_on_every_ready_and_hourly_while_ready() {
        let (sender, receiver) = tokio::sync::watch::channel(health(HealthState::Stopped));
        let (pass_tx, mut pass_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let task = tokio::spawn(on_ready_and_hourly(receiver, move || {
            let pass_tx = pass_tx.clone();
            async move {
                let _ = pass_tx.send(());
            }
        }));
        let next = |rx: &mut tokio::sync::mpsc::UnboundedReceiver<()>| rx.try_recv().is_ok();
        tokio::task::yield_now().await;
        assert!(!next(&mut pass_rx), "no pass before Ready");

        let bound = PASS_INTERVAL * 2;
        sender.send(health(HealthState::Ready)).unwrap();
        tokio::time::timeout(bound, pass_rx.recv()).await.expect("a pass on Ready").unwrap();
        let started = tokio::time::Instant::now();
        tokio::time::timeout(bound, pass_rx.recv()).await.expect("an hourly pass").unwrap();
        assert_eq!(started.elapsed(), PASS_INTERVAL, "the next pass waits one interval");

        sender.send(health(HealthState::Exited)).unwrap();
        tokio::task::yield_now().await;
        tokio::time::advance(PASS_INTERVAL * 3).await;
        tokio::task::yield_now().await;
        assert!(!next(&mut pass_rx), "no pass while not Ready");

        sender.send(health(HealthState::Ready)).unwrap();
        tokio::time::timeout(bound, pass_rx.recv()).await.expect("a pass on the next Ready").unwrap();
        drop(sender);
        tokio::time::timeout(Duration::from_secs(10), task).await.expect("loop ends with its channel").unwrap();
    }
}
