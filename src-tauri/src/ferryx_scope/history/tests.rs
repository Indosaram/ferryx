use super::history::*;
use crate::{daemon::protocol::TerminalStartup, scoped_contracts::*};
use serde_json::json;
use std::{fs, path::Path, sync::Mutex};

fn fixture() -> (tempfile::TempDir, History, String) {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_string_lossy().into_owned();
    let logs = root.path().join("logs"); fs::create_dir(&logs).unwrap();
    for (file, id, text) in [("older.jsonl", "019ec598-c800-7000-8000-000000000001", "older-message-sentinel"), ("newest.jsonl", "019ec598-c800-7000-8000-000000000002", "newest only")] {
        fs::write(logs.join(file), format!("{}\n{}\n", json!({"type":"session_meta","payload":{"id":id,"cwd":cwd,"cli_version":"0.153.2"}}), json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":text}]}}))).unwrap();
    }
    let h = History::new(vec![(CanonicalProvider::Codex, logs)]); (root, h, cwd)
}

/// Machine spawn proves a remote resume through `session_cwd`: the id's own store record names
/// the cwd, and an id no record carries is `NotFound` (gap C27).
#[test]
fn session_cwd_names_the_store_record_cwd_for_one_id() {
    // A real codex store names each rollout after its id; the lookup only decodes such files.
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_string_lossy().into_owned();
    let id = "019ec598-c800-7000-8000-000000000001";
    let day = root.path().join("2026").join("10").join("22"); fs::create_dir_all(&day).unwrap();
    fs::write(day.join(format!("rollout-2026-10-22T16-08-08-{id}.jsonl")), format!("{}\n", json!({"type":"session_meta","payload":{"id":id,"cwd":cwd,"cli_version":"0.153.2"}}))).unwrap();
    let roots = vec![(CanonicalProvider::Codex, root.path().to_path_buf())];
    assert_eq!(session_cwd(&roots, CanonicalProvider::Codex, id).unwrap(), Path::new(&cwd));
    assert!(matches!(session_cwd(&roots, CanonicalProvider::Codex, "019ec598-c800-7000-8000-00000000ffff"), Err(HistoryError::NotFound)));
}
#[test]
fn older_message_search_and_exact_identity() {
    let (_root, mut h, cwd) = fixture();
    let p = h.search("codex", Some(&cwd), "older-message-sentinel", None, 100).unwrap();
    assert_eq!(p.items.len(), 1, "older parsed message must be searchable");
    assert_eq!(p.items[0].provider_session.id, "019ec598-c800-7000-8000-000000000001");
    assert_eq!(h.read(&p.items[0].entry_key, None, 100).unwrap().items[0].text, "older-message-sentinel");
    assert_eq!(h.source_path(&p.items[0].entry_key), Some(Path::new(&cwd).join("logs").join("older.jsonl").as_path()));
    assert_eq!(h.source_path("missing-entry-key"), None);
}
struct NativeBoundary(Mutex<Vec<(ConversationClaimKey, TerminalStartup)>>);
impl ResumeBoundary<TerminalStartup> for NativeBoundary {
    fn claim_and_spawn(&self, key: ConversationClaimKey, _: ConversationOwner, _: &str, startup: TerminalStartup) -> Result<TerminalStartup> {
        let mut claims = self.0.lock().unwrap();
        if claims.iter().any(|(k,_)| k == &key) { return Err(HistoryError::ProviderOwned); }
        claims.push((key,startup.clone())); Ok(startup)
    }
}
#[test]
fn selected_session_exact_typed_resume_and_duplicate_fence() {
    let (_root, mut h, cwd) = fixture();
    let p = h.search("codex", None, "older-message-sentinel", None, 100).unwrap();
    assert_eq!(p.items.len(),1,"exact selected identity must survive discovery");
    let b = NativeBoundary(Mutex::new(vec![]));
    let owner = ConversationOwner::Provisional {request_id:"test".into()};
    let startup = h.resume(&p.items[0].entry_key, &cwd, "local", owner.clone(), &b).unwrap();
    let TerminalStartup::AgentResume { agent_type, provider_session } = startup else { panic!("expected AgentResume"); };
    let plan = crate::terminal::shell::resolve_agent_resume_plan(&agent_type, &provider_session).unwrap();
    assert_eq!(plan.args, vec!["resume", "019ec598-c800-7000-8000-000000000001"]);
    assert_eq!(h.resume(&p.items[0].entry_key, &cwd, "local", owner, &b), Err(HistoryError::ProviderOwned));
    assert_eq!(b.0.lock().unwrap().len(),1);
}

fn write(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().expect("fixture path has a parent")).unwrap();
    fs::write(path, body).unwrap();
}
fn boundary() -> (NativeBoundary, ConversationOwner) {
    (NativeBoundary(Mutex::new(vec![])), ConversationOwner::Provisional { request_id: "test".into() })
}

#[test]
fn every_canonical_provider_has_a_wire_name_the_search_gate_accepts() {
    assert_eq!(PROVIDERS.len(), 15, "every provider with a readable store must be listed");
    for provider in PROVIDERS {
        let wire = provider_wire(provider);
        assert_eq!(provider_from_wire(wire), Some(provider), "{wire} must round-trip through the wire name");
        assert!(History::new(vec![]).search(wire, None, "", None, 100).is_ok(), "{wire} must not be rejected as unsupported");
    }
    for wire in ["omo","pi","omp","gjc","antigravity","opencode","copilot","cursor","kimi","cline","grok","droid","crush"] {
        assert!(provider_from_wire(wire).is_some(), "{wire} history support must stay wired");
    }
}

/// omo, pi, omp and gjc share the senpi transcript shape, so one table drives all four.
/// Fixtures: `fixtures/{omo,omp,gjc}/session.jsonl`.
#[test]
fn senpi_family_lists_reads_and_targets_resume_for_omo_pi_omp_gjc() {
    const OMO: &str = include_str!("fixtures/omo/session.jsonl");
    const OMP: &str = include_str!("fixtures/omp/session.jsonl");
    const GJC: &str = include_str!("fixtures/gjc/session.jsonl");
    for (provider, wire, body, sentinel, id, version, wants_transcript) in [
        (CanonicalProvider::Omo, "omo", OMO, "omo-user-sentinel", "01a16281-1b66-764d-87a3-4ea9321c5bd8", None, false),
        (CanonicalProvider::Pi, "pi", OMO, "omo-user-sentinel", "01a16281-1b66-764d-87a3-4ea9321c5bd8", None, true),
        (CanonicalProvider::Omp, "omp", OMP, "omp-user-sentinel", "01a16270-e358-7635-88e4-1b2f84460057", Some("18.12.6"), true),
        (CanonicalProvider::Gjc, "gjc", GJC, "gjc-user-sentinel", "01a16274-abd4-7581-8f9c-22214661ef6a", None, false),
    ] {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().to_string_lossy().into_owned();
        write(&root.path().join("--slug--").join("2026-10-22T07-26-30-758Z_session.jsonl"), &body.replace("__CWD__", &cwd));
        let mut h = History::new(vec![(provider, root.path().into())]);
        let page = h.search(wire, Some(&cwd), sentinel, None, 100).unwrap();
        assert_eq!(page.items.len(), 1, "{wire} session must be searchable by its message text");
        let entry = page.items[0].clone();
        assert_eq!(entry.provider_session.id, id, "{wire} identity comes from the session header");
        assert_eq!(entry.version.as_deref(), version, "{wire} version comes from runtime.version when present");
        assert_eq!(entry.provider_session.transcript_path.is_some(), wants_transcript, "{wire} resume target shape");
        let messages = h.read(&entry.entry_key, None, 100).unwrap().items;
        assert!(messages.iter().any(|m| m.role == "user" && m.text == sentinel), "{wire} user turn must be readable");
        assert!(messages.iter().any(|m| m.role == "assistant"), "{wire} assistant turn must be readable");
        let plan = crate::terminal::shell::resolve_agent_resume_plan(wire, &entry.provider_session).unwrap();
        assert_eq!(plan.program, wire, "{wire} must resolve its own resume program");
    }
}

/// copilot records carry the role in `type` and never carry a cwd.
/// Fixture: `fixtures/copilot/session.jsonl`.
#[test]
fn copilot_session_lists_without_a_cwd_and_reads_both_roles() {
    let root = tempfile::tempdir().unwrap();
    write(&root.path().join("5793c06c-5bee-4b34-883b-589051286cfd.jsonl"), include_str!("fixtures/copilot/session.jsonl"));
    let mut h = History::new(vec![(CanonicalProvider::Copilot, root.path().into())]);
    let page = h.search("copilot", None, "copilot-user-sentinel", None, 100).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].provider_session.id, "5793c06c-5bee-4b34-883b-589051286cfd");
    assert_eq!(page.items[0].version.as_deref(), Some("0.0.375"));
    assert_eq!(page.items[0].cwd, "", "no copilot record type carries a working directory");
    assert!(page.warnings.contains(&"CWD_UNAVAILABLE".into()));
    let messages = h.read(&page.items[0].entry_key, None, 100).unwrap().items;
    assert_eq!(messages.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(), vec!["user", "assistant"], "the tool-only assistant.message has no text to show");
    assert_eq!(messages[1].text, "copilot-assistant-sentinel");
}

/// droid lists sessions that actually hold a turn; its inner message shape is unmeasured.
/// Fixture: `fixtures/droid/session.jsonl`.
#[test]
fn droid_lists_sessions_with_turns_and_reports_read_as_unsettled() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_string_lossy().into_owned();
    let slug = root.path().join("-tmp-work");
    write(&slug.join("d08d895d-058f-4f86-aa21-48d7dbdcaa9f.jsonl"), &include_str!("fixtures/droid/session.jsonl").replace("__CWD__", &cwd));
    write(&slug.join("d08d895d-058f-4f86-aa21-48d7dbdcaa90.jsonl"), &format!("{}\n", json!({"type":"session_start","id":"d08d895d-058f-4f86-aa21-48d7dbdcaa90","title":"redacted-title","owner":"indo","version":2,"cwd":cwd,"hostId":"6f1d0c1a-0000-4000-8000-000000000001"})));
    let mut h = History::new(vec![(CanonicalProvider::Droid, root.path().into())]);
    let page = h.search("droid", Some(&cwd), "", None, 100).unwrap();
    assert_eq!(page.items.len(), 1, "a session_start-only file is hidden, as droid's own find hides it");
    assert_eq!(page.items[0].provider_session.id, "d08d895d-058f-4f86-aa21-48d7dbdcaa9f");
    assert_eq!(page.items[0].version.as_deref(), Some("2"));
    assert!(page.warnings.contains(&"READ_UNSETTLED".into()), "droid message bodies were never measured");
    assert!(h.read(&page.items[0].entry_key, None, 100).unwrap().items.is_empty());
}

/// kimi's conversation is its `wire.jsonl` event stream, and its cwd is the `kimi.json`
/// `work_dirs[].path` whose md5 names the session's parent folder.
/// Fixtures: `fixtures/kimi/context.jsonl`, `fixtures/kimi/wire.jsonl`.
#[test]
fn kimi_reads_the_wire_stream_and_resolves_its_cwd_from_kimi_json() {
    use md5::{Digest as _, Md5};
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let cwd = workspace.path().to_string_lossy().into_owned();
    let hashed = format!("{:x}", Md5::digest(cwd.as_bytes()));
    let root = home.path().join(".kimi").join("sessions");
    let session = root.join(&hashed).join("cb0ab2dd-f2e4-4a49-9c8b-843d866bb8f2");
    write(&session.join("context.jsonl"), include_str!("fixtures/kimi/context.jsonl"));
    write(&session.join("wire.jsonl"), include_str!("fixtures/kimi/wire.jsonl"));
    write(&home.path().join(".kimi").join("kimi.json"), &json!({"work_dirs":[{"path":"/","kaos":"local","last_session_id":null},{"path":cwd,"kaos":"local","last_session_id":"cb0ab2dd-f2e4-4a49-9c8b-843d866bb8f2"}]}).to_string());
    let mut h = History::new(vec![(CanonicalProvider::Kimi, root)]);
    let page = h.search("kimi", Some(&cwd), "kimi-assistant-sentinel", None, 100).unwrap();
    assert_eq!(page.items.len(), 1, "the workspace filter must accept a kimi entry whose cwd kimi.json names");
    assert_eq!(page.items[0].cwd, cwd);
    assert!(!page.warnings.iter().any(|w| w == "CWD_UNAVAILABLE" || w == "ASSISTANT_RECORDS_UNSETTLED"), "{:?}", page.warnings);
    let messages = h.read(&page.items[0].entry_key, None, 100).unwrap().items;
    assert_eq!(
        messages.iter().map(|m| (m.role.as_str(), m.text.as_str())).collect::<Vec<_>>(),
        vec![("user", "kimi-user-sentinel"), ("assistant", "kimi-assistant-sentinel"), ("tool", "Shell {\"command\": \"ls\"}")],
        "text parts join, think parts and the result are not turns"
    );
    let (b, owner) = boundary();
    let startup = h.resume(&page.items[0].entry_key, &cwd, "local", owner, &b).unwrap();
    let TerminalStartup::AgentResume { agent_type, provider_session } = startup else { panic!("expected AgentResume"); };
    let plan = crate::terminal::shell::resolve_agent_resume_plan(&agent_type, &provider_session).unwrap();
    assert_eq!(plan.args, vec!["--session", "cb0ab2dd-f2e4-4a49-9c8b-843d866bb8f2"]);
}

/// Without a wire stream kimi falls back to the user records of `context.jsonl`, and without a
/// `kimi.json` entry for the md5 folder the cwd stays empty and is reported.
/// Fixture: `fixtures/kimi/context.jsonl`.
#[test]
fn kimi_without_wire_or_kimi_json_reads_context_users_and_reports_no_cwd() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join(".kimi").join("sessions");
    let session = root.join("905465c20d3ded51e5f2764977d4230b").join("eca66b4f-a89b-4979-9f13-cbe3f35b5991");
    write(&session.join("context.jsonl"), include_str!("fixtures/kimi/context.jsonl"));
    let mut h = History::new(vec![(CanonicalProvider::Kimi, root)]);
    let page = h.search("kimi", None, "kimi-user-sentinel", None, 100).unwrap();
    assert_eq!(page.items.len(), 1, "only context.jsonl is listed");
    assert_eq!(page.items[0].provider_session.id, "eca66b4f-a89b-4979-9f13-cbe3f35b5991");
    assert_eq!(page.items[0].cwd, "");
    assert!(page.warnings.contains(&"CWD_UNAVAILABLE".into()));
    let messages = h.read(&page.items[0].entry_key, None, 100).unwrap().items;
    assert_eq!(messages.len(), 1, "_checkpoint markers are not turns");
    assert_eq!((messages[0].role.as_str(), messages[0].text.as_str()), ("user", "kimi-user-sentinel"));
}
/// grok streams each turn as chunks that have to be rejoined.
/// Fixtures: `fixtures/grok/summary.json`, `fixtures/grok/updates.jsonl`.
#[test]
fn grok_rejoins_message_chunks_from_the_update_stream() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_string_lossy().into_owned();
    let session = root.path().join("%2Ftmp%2Fproj").join("01a16274-a688-7e73-b6fc-434bc7781be4");
    write(&session.join("summary.json"), &include_str!("fixtures/grok/summary.json").replace("__CWD__", &cwd));
    write(&session.join("updates.jsonl"), include_str!("fixtures/grok/updates.jsonl"));
    let mut h = History::new(vec![(CanonicalProvider::Grok, root.path().into())]);
    let page = h.search("grok", Some(&cwd), "grok-assistant-sentinel", None, 100).unwrap();
    assert_eq!(page.items.len(), 1, "a reply split across chunks must still be searchable");
    assert_eq!(page.items[0].provider_session.id, "01a16274-a688-7e73-b6fc-434bc7781be4");
    let messages = h.read(&page.items[0].entry_key, None, 100).unwrap().items;
    assert_eq!(messages.iter().map(|m| (m.role.as_str(), m.text.as_str())).collect::<Vec<_>>(), vec![("user", "grok-user-sentinel"), ("assistant", "grok-assistant-sentinel")]);
}

/// cline splits one conversation into a metadata document and a messages document.
/// Fixtures: `fixtures/cline/session.json`, `fixtures/cline/messages.json`.
#[test]
fn cline_reads_the_sibling_messages_document_and_unwraps_user_input() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_string_lossy().into_owned();
    let session = root.path().join("1792653340197_ys4yu");
    write(&session.join("1792653340197_ys4yu.json"), &include_str!("fixtures/cline/session.json").replace("__CWD__", &cwd));
    write(&session.join("1792653340197_ys4yu.messages.json"), include_str!("fixtures/cline/messages.json"));
    let mut h = History::new(vec![(CanonicalProvider::Cline, root.path().into())]);
    let page = h.search("cline", Some(&cwd), "cline-user-sentinel", None, 100).unwrap();
    assert_eq!(page.items.len(), 1, "the messages document must not be listed as a second conversation");
    assert_eq!(page.items[0].provider_session.id, "1792653340197_ys4yu");
    assert_eq!(page.items[0].version.as_deref(), Some("1"));
    let messages = h.read(&page.items[0].entry_key, None, 100).unwrap().items;
    assert_eq!(messages[0].text, "cline-user-sentinel", "the <user_input> wrapper is not part of the prompt");
    assert_eq!(messages[1].text, "cline-assistant-sentinel");
}

/// antigravity lists from its conversation index and reads the generated transcript.
/// Fixture: `fixtures/antigravity/transcript_full.jsonl`.
#[test]
fn antigravity_lists_from_the_summary_index_and_reads_the_transcript() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_string_lossy().into_owned();
    let id = "1908f6de-ef1d-424b-8f89-d66be0db44b5";
    let connection = rusqlite::Connection::open(root.path().join("conversation_summaries.db")).unwrap();
    connection.execute_batch("CREATE TABLE conversation_summaries (conversation_id TEXT PRIMARY KEY, title TEXT, preview TEXT, step_count INTEGER, last_modified_time TEXT, workspace_uris TEXT, parent_conversation_id TEXT);").unwrap();
    connection.execute("INSERT INTO conversation_summaries VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", rusqlite::params![id, "redacted-title", "redacted-title", 6, "2026-10-22 07:06:46.019683+00:00", format!("[\"file://{cwd}\"]"), Option::<String>::None]).unwrap();
    drop(connection);
    write(&root.path().join("brain").join(id).join(".system_generated").join("logs").join("transcript_full.jsonl"), include_str!("fixtures/antigravity/transcript_full.jsonl"));
    let mut h = History::new(vec![(CanonicalProvider::Antigravity, root.path().into())]);
    let page = h.search("antigravity", Some(&cwd), "antigravity-user-sentinel", None, 100).unwrap();
    assert_eq!(page.items.len(), 1, "the workspace_uris file URI must resolve to the cwd");
    assert_eq!(page.items[0].provider_session.id, id);
    let messages = h.read(&page.items[0].entry_key, None, 100).unwrap().items;
    assert_eq!(messages[0].text, "antigravity-user-sentinel", "the <USER_REQUEST> wrapper and trailing metadata are not prompt text");
    assert_eq!(crate::terminal::shell::resolve_agent_resume_plan("antigravity", &page.items[0].provider_session).unwrap().args, vec!["--conversation", id]);
}

/// opencode packs every session into one database; text lives on the message's `text` parts.
/// Fixture: `fixtures/opencode/rows.json`.
#[test]
fn opencode_reads_sessions_out_of_its_shared_sqlite_store() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_string_lossy().into_owned();
    let rows: serde_json::Value = serde_json::from_str(include_str!("fixtures/opencode/rows.json")).unwrap();
    let session = "ses_e9d886f2bffeJp9DsEMPfnB25I";
    let connection = rusqlite::Connection::open(root.path().join("opencode.db")).unwrap();
    connection.execute_batch("CREATE TABLE session (id TEXT PRIMARY KEY, project_id TEXT, parent_id TEXT, slug TEXT, directory TEXT, title TEXT, version TEXT, time_updated INTEGER); CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT); CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, data TEXT);").unwrap();
    connection.execute("INSERT INTO session VALUES (?1, NULL, NULL, 'playful-squid', ?2, 'redacted-title', '1.18.18', 1792653365461)", rusqlite::params![session, cwd]).unwrap();
    connection.execute("INSERT INTO message VALUES ('msg_1', ?1, 1, ?2)", rusqlite::params![session, rows["message"].to_string()]).unwrap();
    connection.execute("INSERT INTO part VALUES ('prt_0001', 'msg_1', ?1, 1, ?2)", rusqlite::params![session, rows["textPart"].to_string()]).unwrap();
    connection.execute("INSERT INTO part VALUES ('prt_0002', 'msg_1', ?1, 2, ?2)", rusqlite::params![session, rows["toolPart"].to_string()]).unwrap();
    drop(connection);
    let mut h = History::new(vec![(CanonicalProvider::Opencode, root.path().into())]);
    let page = h.search("opencode", Some(&cwd), "opencode-user-sentinel", None, 100).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].provider_session.id, session);
    assert_eq!(page.items[0].version.as_deref(), Some("1.18.18"));
    let messages = h.read(&page.items[0].entry_key, None, 100).unwrap().items;
    assert_eq!(messages.len(), 1, "tool parts are not displayed turns");
    assert_eq!((messages[0].role.as_str(), messages[0].text.as_str()), ("user", "opencode-user-sentinel"));
}

/// crush keeps one database per project and an index of them in its share directory.
/// Fixture: `fixtures/crush/rows.json`.
#[test]
fn crush_follows_its_project_index_and_skips_child_sessions() {
    let share = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let cwd = project.path().to_string_lossy().into_owned();
    let data_dir = project.path().join(".crush");
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(share.path().join("projects.json"), json!({"projects":[{"path":cwd,"data_dir":data_dir,"last_accessed":"2026-10-22T16:06:02+09:00"}]}).to_string()).unwrap();
    let parts = serde_json::from_str::<serde_json::Value>(include_str!("fixtures/crush/rows.json")).unwrap()["parts"].to_string();
    let session = "3df112d9-0000-4000-8000-000000000001";
    let connection = rusqlite::Connection::open(data_dir.join("crush.db")).unwrap();
    connection.execute_batch("CREATE TABLE sessions (id TEXT PRIMARY KEY, parent_session_id TEXT, title TEXT, created_at INTEGER, updated_at INTEGER); CREATE TABLE messages (id TEXT PRIMARY KEY, session_id TEXT, role TEXT, parts TEXT, created_at INTEGER);").unwrap();
    connection.execute("INSERT INTO sessions VALUES (?1, NULL, 'redacted-title', 1, 2)", rusqlite::params![session]).unwrap();
    connection.execute("INSERT INTO sessions VALUES ('call_child', ?1, 'redacted-child', 1, 2)", rusqlite::params![session]).unwrap();
    connection.execute("INSERT INTO messages VALUES ('m1', ?1, 'user', ?2, 1)", rusqlite::params![session, parts]).unwrap();
    drop(connection);
    let mut h = History::new(vec![(CanonicalProvider::Crush, share.path().into())]);
    let page = h.search("crush", Some(&cwd), "crush-user-sentinel", None, 100).unwrap();
    assert_eq!(page.items.len(), 1, "sub-agent sessions are not user-resumable conversations");
    assert_eq!(page.items[0].provider_session.id, session);
    assert_eq!(page.items[0].cwd, cwd, "the cwd comes from the project index, not from the database");
    let messages = h.read(&page.items[0].entry_key, None, 100).unwrap().items;
    assert_eq!((messages[0].role.as_str(), messages[0].text.as_str()), ("user", "crush-user-sentinel"));
}

/// antigravity model steps: `PLANNER_RESPONSE` prose is the assistant turn, its `tool_calls` are tool
/// records, and `GENERIC` tool outputs are not turns.
/// Fixture: `fixtures/antigravity/replied_transcript_full.jsonl` (a real local transcript, minimized).
#[test]
fn antigravity_reads_planner_responses_as_assistant_and_tool_records() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_string_lossy().into_owned();
    let id = "06468bac-2c00-4674-9598-71f9e59b9af6";
    let connection = rusqlite::Connection::open(root.path().join("conversation_summaries.db")).unwrap();
    connection.execute_batch("CREATE TABLE conversation_summaries (conversation_id TEXT PRIMARY KEY, title TEXT, preview TEXT, step_count INTEGER, last_modified_time TEXT, workspace_uris TEXT, parent_conversation_id TEXT);").unwrap();
    connection.execute("INSERT INTO conversation_summaries VALUES (?1, 'redacted-title', 'redacted-title', 5, '2026-10-08 00:10:07+00:00', ?2, NULL)", rusqlite::params![id, format!("[\"file://{cwd}\"]")]).unwrap();
    drop(connection);
    write(&root.path().join("brain").join(id).join(".system_generated").join("logs").join("transcript_full.jsonl"), include_str!("fixtures/antigravity/replied_transcript_full.jsonl"));
    let mut h = History::new(vec![(CanonicalProvider::Antigravity, root.path().into())]);
    let page = h.search("antigravity", Some(&cwd), "editorial card slides", None, 100).unwrap();
    assert_eq!(page.items.len(), 1, "assistant prose must be searchable");
    assert!(!page.warnings.iter().any(|w| w == "ASSISTANT_RECORDS_UNSETTLED"), "{:?}", page.warnings);
    let messages = h.read(&page.items[0].entry_key, None, 100).unwrap().items;
    assert_eq!(
        messages.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(),
        vec!["user", "assistant", "user", "tool"],
        "the GENERIC tool output is not a turn"
    );
    assert_eq!(messages[1].text, "Here are 5 concrete photographic search queries tailored for the editorial card slides:");
    assert_eq!(messages[1].id.as_deref(), Some("1"), "the step index names the model step");
    assert!(messages[3].text.starts_with("finish {\"queries\":"), "{}", messages[3].text);
}

/// cursor reads its conversation by walking the root blob tree, and its cwd is the workspace path
/// the root node names, accepted because its md5 is the store's parent folder.
/// Fixture: `fixtures/cursor/store.db`, the real store of a cursor-agent chat run in
/// `/private/tmp/fx-cursor-jFjF` (`~/.cursor/chats/b8670c45df56ad6621fb57097feba1b4/`), reduced to the
/// root node and its children minus the 43 KB `<user_info>` context blob.
#[test]
fn cursor_reads_the_root_blob_tree_and_resolves_its_workspace_cwd() {
    let root = tempfile::tempdir().unwrap();
    let chat = "41ae0fb1-b55f-4592-a462-343915618791";
    let cwd = "/private/tmp/fx-cursor-jFjF";
    write_bytes(&root.path().join("b8670c45df56ad6621fb57097feba1b4").join(chat).join("store.db"), include_bytes!("fixtures/cursor/store.db"));
    let mut h = History::new(vec![(CanonicalProvider::Cursor, root.path().into())]);
    let page = h.search("cursor", Some(cwd), "touch probe.txt", None, 100).unwrap();
    assert_eq!(page.items.len(), 1, "the workspace filter must accept the cwd the root node names");
    assert_eq!(page.items[0].provider_session.id, chat);
    assert_eq!(page.items[0].cwd, cwd);
    assert!(page.warnings.is_empty(), "{:?}", page.warnings);
    let messages = h.read(&page.items[0].entry_key, None, 100).unwrap().items;
    assert_eq!(
        messages.iter().map(|m| (m.role.as_str(), m.text.as_str())).collect::<Vec<_>>(),
        vec![
            ("user", "run the shell command: touch probe.txt"),
            ("assistant", "I'll create `probe.txt` with `touch`."),
            ("tool", "Shell {\"command\":\"touch probe.txt\",\"description\":\"Create empty probe.txt file\"}"),
            ("assistant", "Created `probe.txt`."),
        ],
        "system, missing context and tool-result children are not turns; encrypted reasoning is not shown"
    );
}

/// A store whose folder is not the md5 of the workspace its root names keeps no cwd.
/// Fixture: `fixtures/cursor/store.db`.
#[test]
fn cursor_without_a_matching_md5_folder_or_root_blob_reports_what_is_missing() {
    let root = tempfile::tempdir().unwrap();
    write_bytes(&root.path().join("00000000000000000000000000000000").join("41ae0fb1-b55f-4592-a462-343915618791").join("store.db"), include_bytes!("fixtures/cursor/store.db"));
    let mut h = History::new(vec![(CanonicalProvider::Cursor, root.path().into())]);
    let page = h.search("cursor", None, "", None, 100).unwrap();
    assert_eq!(page.items[0].cwd, "", "a root path that does not hash to the folder name is not trusted");
    assert!(page.warnings.contains(&"CWD_UNAVAILABLE".into()));
    assert_eq!(h.read(&page.items[0].entry_key, None, 100).unwrap().items.len(), 4);
}

fn write_bytes(path: &Path, body: &[u8]) {
    fs::create_dir_all(path.parent().expect("fixture path has a parent")).unwrap();
    fs::write(path, body).unwrap();
}

/// cursor stores its chat metadata hex-encoded; without the root blob there is nothing to walk.
/// Fixture: `fixtures/cursor/meta.json`.
#[test]
fn cursor_lists_a_chat_from_hex_encoded_meta_without_its_blob_tree() {
    let root = tempfile::tempdir().unwrap();
    let chat = "9371cc0a-d5c0-4cca-bcae-cae24632e1b5";
    let store = root.path().join("b4e6d784f9a6bb21558b1cd04080d304").join(chat);
    fs::create_dir_all(&store).unwrap();
    let encoded = include_str!("fixtures/cursor/meta.json").trim().bytes().map(|byte| format!("{byte:02x}")).collect::<String>();
    let connection = rusqlite::Connection::open(store.join("store.db")).unwrap();
    connection.execute_batch("CREATE TABLE blobs (id TEXT PRIMARY KEY, data BLOB); CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);").unwrap();
    connection.execute("INSERT INTO meta VALUES ('0', ?1)", rusqlite::params![encoded]).unwrap();
    drop(connection);
    let mut h = History::new(vec![(CanonicalProvider::Cursor, root.path().into())]);
    let page = h.search("cursor", None, "", None, 100).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].provider_session.id, chat, "the chat id is the agentId inside the meta row");
    assert_eq!(page.items[0].cwd, "", "without the root node only an md5 of the workspace is known");
    assert!(page.warnings.contains(&"TRANSCRIPT_UNAVAILABLE".into()) && page.warnings.contains(&"CWD_UNAVAILABLE".into()), "{:?}", page.warnings);
    assert!(!page.warnings.contains(&"READ_UNSETTLED".into()));
    assert!(h.read(&page.items[0].entry_key, None, 100).unwrap().items.is_empty());
}
