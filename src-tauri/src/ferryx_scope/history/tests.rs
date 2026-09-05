use super::history::*;
use crate::{daemon::protocol::TerminalStartup, scoped_contracts::*};
use serde_json::json;
use std::{fs, sync::Mutex};

fn fixture() -> (tempfile::TempDir, History, String) {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_string_lossy().into_owned();
    let logs = root.path().join("logs"); fs::create_dir(&logs).unwrap();
    for (file, id, text) in [("older.jsonl", "019ec598-c800-7000-8000-000000000001", "older-message-sentinel"), ("newest.jsonl", "019ec598-c800-7000-8000-000000000002", "newest only")] {
        fs::write(logs.join(file), format!("{}\n{}\n", json!({"type":"session_meta","payload":{"id":id,"cwd":cwd,"cli_version":"0.153.2"}}), json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":text}]}}))).unwrap();
    }
    let h = History::new(vec![(CanonicalProvider::Codex, logs)]); (root, h, cwd)
}
#[test]
fn older_message_search_and_exact_identity() {
    let (_root, mut h, cwd) = fixture();
    let p = h.search("codex", Some(&cwd), "older-message-sentinel", None, 100).unwrap();
    assert_eq!(p.items.len(), 1, "older parsed message must be searchable");
    assert_eq!(p.items[0].provider_session.id, "019ec598-c800-7000-8000-000000000001");
    assert_eq!(h.read(&p.items[0].entry_key, None, 100).unwrap().items[0].text, "older-message-sentinel");
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
    let TerminalStartup::AgentResume { agent_type, provider_session } = startup;
    let plan = crate::terminal::shell::resolve_agent_resume_plan(&agent_type, &provider_session).unwrap();
    assert_eq!(plan.args, vec!["resume", "019ec598-c800-7000-8000-000000000001"]);
    assert_eq!(h.resume(&p.items[0].entry_key, &cwd, "local", owner, &b), Err(HistoryError::ProviderOwned));
    assert_eq!(b.0.lock().unwrap().len(),1);
}
