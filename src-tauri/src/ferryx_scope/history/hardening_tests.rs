use super::history::*;
use crate::scoped_contracts::CanonicalProvider;
use serde_json::json;
use std::{fs,io::Write};
#[test]
fn claude_active_branch_excludes_abandoned_sibling() {
    let root = tempfile::tempdir().unwrap(); let cwd = root.path().to_string_lossy().into_owned();
    let rows = [("a",None,"user","question"),("b",Some("a"),"assistant","abandoned-sentinel"),("c",Some("a"),"assistant","active-sentinel")];
    let text = rows.iter().map(|(id,parent,role,text)| format!("{}\n",json!({"type":role,"uuid":id,"parentUuid":parent,"sessionId":"63ba88ec-b3cb-4a70-8a32-f5d1ad07e749","cwd":cwd,"version":"2.1.251","message":{"role":role,"content":[{"type":"text","text":text}]}}))).collect::<String>();
    fs::write(root.path().join("claude.jsonl"),text).unwrap();
    let mut h = History::new(vec![(CanonicalProvider::Claude,root.path().into())]);
    assert_eq!(h.search("claude", None,"abandoned-sentinel",None,100).unwrap().items.len(),0,"abandoned branch must not be presented as current conversation");
    let p = h.search("claude",None,"active-sentinel",None,100).unwrap(); assert_eq!(p.items.len(),1);
    let messages = h.read(&p.items[0].entry_key,None,100).unwrap().items;
    assert_eq!(messages.iter().map(|m|m.id.as_deref().unwrap()).collect::<Vec<_>>(),vec!["a","c"]);
}
/// Real Claude files chain messages through attachment and system records, so the active
/// branch walk must follow every record's uuid. Typed turns in both content shapes are read;
/// a tool_result-only user record and a meta (skill body) record are not turns.
#[test]
fn claude_turns_chained_through_attachment_and_system_records_are_read() {
    let root = tempfile::tempdir().unwrap(); let cwd = root.path().to_string_lossy().into_owned();
    let session = "63ba88ec-b3cb-4a70-8a32-f5d1ad07e749";
    let rec = |kind: &str, id: &str, parent: Option<&str>, extra: serde_json::Value| {
        let mut v = json!({"type":kind,"uuid":id,"parentUuid":parent,"sessionId":session,"cwd":cwd,"version":"2.1.251"});
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        format!("{v}\n")
    };
    let text = [
        format!("{}\n", json!({"type":"file-history-snapshot","messageId":"x"})),
        rec("user","u1",None,json!({"message":{"role":"user","content":"typed-string-sentinel"}})),
        rec("attachment","a1",Some("u1"),json!({"attachment":{"type":"todo"}})),
        rec("assistant","m1",Some("a1"),json!({"message":{"role":"assistant","content":[{"type":"text","text":"first answer"}]}})),
        rec("system","s1",Some("m1"),json!({"subtype":"informational"})),
        rec("user","u2",Some("s1"),json!({"isMeta":true,"message":{"role":"user","content":[{"type":"text","text":"Base directory for this skill"}]}})),
        rec("user","u3",Some("u2"),json!({"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"tool output"}]}})),
        rec("user","u4",Some("u3"),json!({"message":{"role":"user","content":[{"type":"text","text":"typed-array-sentinel"}]}})),
        rec("attachment","a2",Some("u4"),json!({"attachment":{"type":"todo"}})),
        rec("assistant","m2",Some("a2"),json!({"message":{"role":"assistant","content":[{"type":"text","text":"last answer"}]}})),
        format!("{}\n", json!({"type":"last-prompt","sessionId":session})),
    ].concat();
    fs::write(root.path().join("claude.jsonl"),text).unwrap();
    let mut h = History::new(vec![(CanonicalProvider::Claude,root.path().into())]);
    let p = h.search("claude",None,"typed-string-sentinel",None,100).unwrap(); assert_eq!(p.items.len(),1);
    let messages = h.read(&p.items[0].entry_key,None,100).unwrap().items;
    let read: Vec<_> = messages.iter().map(|m| (m.ordinal,m.role.as_str(),m.text.as_str())).collect();
    assert_eq!(read, vec![(1,"user","typed-string-sentinel"),(3,"assistant","first answer"),(7,"user","typed-array-sentinel"),(9,"assistant","last answer")]);
}
fn fixture() -> (tempfile::TempDir,History) {
    let root = tempfile::tempdir().unwrap();
    let header = json!({"type":"session_meta","payload":{"id":"019ec598-c800-7000-8000-000000000001","cwd":root.path(),"cli_version":"0.153.2"}});
    let msg = json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"sentinel"}]}});
    fs::write(root.path().join("log.jsonl"),format!("{header}\n{msg}\n{msg}\n")).unwrap();
    let h = History::new(vec![(CanonicalProvider::Codex,root.path().into())]); (root,h)
}
#[test]
fn paging_append_corrupt_partial_deleted_and_unsupported() {
    let (root,mut h) = fixture();
    let p = h.search("codex",None,"sentinel",None,100).unwrap(); let key = &p.items[0].entry_key;
    let first = h.read(key,None,1).unwrap(); assert!(first.next_cursor.is_some());
    assert_eq!(h.read(key,first.next_cursor.as_deref(),1).unwrap().items.len(),1);
    fs::OpenOptions::new().append(true).open(root.path().join("log.jsonl")).unwrap().write_all(b"{bad}\n{partial").unwrap();
    assert_eq!(h.read(key,first.next_cursor.as_deref(),1).unwrap_err(),HistoryError::SourceChanged);
    let p = h.search("codex",None,"sentinel",None,100).unwrap(); assert!(p.partial); assert!(p.warnings.contains(&"CORRUPT_RECORD".into())); assert!(p.warnings.contains(&"PARTIAL_RECORD".into()));
    fs::remove_file(root.path().join("log.jsonl")).unwrap(); assert_eq!(h.read(&p.items[0].entry_key,None,100).unwrap_err(),HistoryError::NotFound);
    // aider keeps one per-repo markdown log with no session id, so it has no history provider.
    assert_eq!(h.search("aider",None,"",None,100).unwrap_err(),HistoryError::Unsupported);
    assert_eq!(h.search("",None,"",None,100).unwrap_err(),HistoryError::Unsupported);
    assert_eq!(h.search("codex",None,"",None,101).unwrap_err(),HistoryError::InvalidRequest);
}
#[cfg(unix)]
#[test]
fn symlink_roots_and_files_are_rejected() {
    use std::os::unix::fs::symlink;
    let (root,mut h) = fixture(); let outside = tempfile::tempdir().unwrap();
    symlink(root.path().join("log.jsonl"),outside.path().join("escape.jsonl")).unwrap();
    let mut escaped = History::new(vec![(CanonicalProvider::Codex,outside.path().into())]);
    let p = escaped.search("codex",None,"",None,100).unwrap(); assert!(p.items.is_empty()); assert!(p.partial);
    let p = h.search("codex",None,"sentinel",None,100).unwrap();
    fs::remove_file(root.path().join("log.jsonl")).unwrap();
    symlink(outside.path().join("missing"),root.path().join("log.jsonl")).unwrap();
    assert_eq!(h.read(&p.items[0].entry_key,None,100).unwrap_err(),HistoryError::OutsideRoot);
    symlink(root.path(),outside.path().join("root-link")).unwrap();
    let mut h = History::new(vec![(CanonicalProvider::Codex,outside.path().join("root-link"))]);
    let p = h.search("codex",None,"",None,100).unwrap();
    assert!(p.warnings.contains(&"ROOT_SYMLINK_REJECTED".into()),"configured symlink root must not silently redirect");
}

/// The multi-document readers (cline, grok, antigravity) open a second file next to the
/// one the walk found. That sibling goes through the same containment guard, so a symlink planted
/// in a provider store cannot be used to read an arbitrary file.
#[cfg(unix)]
#[test]
fn symlinked_sibling_transcript_is_not_followed_out_of_the_root() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap(); let outside = tempfile::tempdir().unwrap();
    let cwd = root.path().to_string_lossy().into_owned();
    let session = root.path().join("1792653340197_ys4yu"); fs::create_dir_all(&session).unwrap();
    fs::write(session.join("1792653340197_ys4yu.json"),json!({"version":1,"session_id":"1792653340197_ys4yu","cwd":cwd,"provider":"cline"}).to_string()).unwrap();
    fs::write(outside.path().join("stolen.json"),json!({"sessionId":"1792653340197_ys4yu","messages":[{"id":"m1","role":"user","content":[{"type":"text","text":"escaped-sibling-sentinel"}]}]}).to_string()).unwrap();
    symlink(outside.path().join("stolen.json"),session.join("1792653340197_ys4yu.messages.json")).unwrap();
    let mut h = History::new(vec![(CanonicalProvider::Cline,root.path().into())]);
    assert_eq!(h.search("cline",None,"escaped-sibling-sentinel",None,100).unwrap().items.len(),0,"a symlinked sibling document must not be read");
    let p = h.search("cline",None,"",None,100).unwrap();
    assert!(p.warnings.contains(&"TRANSCRIPT_UNAVAILABLE".into()),"refusing the sibling must be reported, not hidden");
    assert!(h.read(&p.items[0].entry_key,None,100).unwrap().items.is_empty());
}

#[cfg(unix)]
#[test]
fn symlinked_file_inside_root_is_not_searched() {
    use std::os::unix::fs::symlink;
    let (root, mut h) = fixture();
    let outside = tempfile::tempdir().unwrap();
    let header = json!({"type":"session_meta","payload":{"id":"019ec598-c800-7000-8000-000000000002","cwd":outside.path(),"cli_version":"0.153.2"}});
    let msg = json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"escaped-symlink-sentinel"}]}});
    fs::write(outside.path().join("target.jsonl"), format!("{header}\n{msg}\n")).unwrap();
    symlink(outside.path().join("target.jsonl"), root.path().join("symlinked.jsonl")).unwrap();
    let p = h.search("codex", None, "escaped-symlink-sentinel", None, 100).unwrap();
    assert_eq!(p.items.len(), 0, "symlinked file inside root must not be traversed");
}
