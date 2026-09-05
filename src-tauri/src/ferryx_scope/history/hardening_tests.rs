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
    assert_eq!(h.search("pi",None,"",None,100).unwrap_err(),HistoryError::Unsupported);
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
