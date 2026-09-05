//! Read-only provider history. Invoke synchronous operations on a blocking worker.
use crate::daemon::protocol::{AgentProviderSession, AgentProviderSessionKey, TerminalStartup};
use crate::scoped_contracts::{CanonicalProvider, ConversationClaimKey, ConversationOwner};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io::Read, path::{Path, PathBuf}};
use sha2::{Digest, Sha256};
use serde_json::Value;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HistoryError { Unsupported, InvalidRequest, NotFound, SourceChanged, OutsideRoot, Io, ProviderOwned, InvalidIdentity }
pub type Result<T> = std::result::Result<T, HistoryError>;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry { pub entry_key: String, pub provider: CanonicalProvider, pub provider_session: AgentProviderSession, pub cwd: String, pub version: Option<String>, pub parent_id: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message { pub ordinal: usize, pub role: String, pub text: String, pub id: Option<String>, pub parent_id: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> { pub items: Vec<T>, pub next_cursor: Option<String>, pub partial: bool, pub warnings: Vec<String> }
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_RECORD: usize = 1024 * 1024;
const MAX_FILES: usize = 1000;
struct Source { root: PathBuf, path: PathBuf, hash: String, entry: Entry }
pub struct History { roots: Vec<(CanonicalProvider, PathBuf)>, sources: HashMap<String, Source> }
impl History {
    pub fn new(roots: Vec<(CanonicalProvider, PathBuf)>) -> Self { Self { roots, sources: HashMap::new() } }
    pub fn search(&mut self, provider: &str, cwd: Option<&str>, query: &str, cursor: Option<&str>, limit: usize) -> Result<Page<Entry>> {
        let provider = match provider { "codex" => CanonicalProvider::Codex, "claude" => CanonicalProvider::Claude, _ => return Err(HistoryError::Unsupported) };
        if query.len() > 4096 { return Err(HistoryError::InvalidRequest); }
        let mut warnings = vec![]; let mut files = vec![]; let mut visited = 0;
        for (_, root) in self.roots.iter().filter(|(p,_)| *p == provider) {
            match root.canonicalize() { Ok(root) => walk(&root, &root, &mut files, &mut visited, &mut warnings), Err(_) => warnings.push("ROOT_UNAVAILABLE".into()) }
        }
        files.sort();
        let mut budget = MAX_BYTES; let mut items = vec![]; let mut fingerprints = String::new();
        let mut sources = HashMap::new();
        for (root,path) in files {
            let bytes = match load(&root, &path, budget) { Ok(b) => b, Err(e) => {warnings.push(format!("{e:?}")); continue;} };
            budget -= bytes.len();
            let hash = digest(&bytes); fingerprints.push_str(&hash);
            let (mut entry, messages, issues) = match parse(provider, &bytes) { Ok(v) => v, Err(e) => {warnings.push(format!("{e:?}")); continue;} };
            warnings.extend(issues);
            let key = digest(format!("{:?}:{}:{hash}",provider,path.display()).as_bytes()); entry.entry_key = key.clone();
            if cwd.is_none_or(|cwd| cwd == entry.cwd) && (query.is_empty() || messages.iter().any(|m| m.text.to_lowercase().contains(&query.to_lowercase()))) { items.push(entry.clone()); }
            sources.insert(key, Source { root, path, hash, entry });
        }
        let stamp = digest(format!("{provider:?}:{cwd:?}:{query}:{fingerprints}").as_bytes());
        let page = page(items, cursor, limit, &stamp, warnings)?;
        // Bound server-side locators; refreshing invalidates references no longer in this scan.
        self.sources = sources;
        Ok(page)
    }
    pub fn read(&self, key: &str, cursor: Option<&str>, limit: usize) -> Result<Page<Message>> {
        let s = self.sources.get(key).ok_or(HistoryError::NotFound)?;
        let bytes = self.validate(s)?;
        let (_, messages, warnings) = parse(s.entry.provider, &bytes)?;
        page(messages, cursor, limit, &s.hash, warnings)
    }
    fn validate(&self, s: &Source) -> Result<Vec<u8>> {
        let bytes = load(&s.root, &s.path, MAX_BYTES)?;
        if digest(&bytes) != s.hash { return Err(HistoryError::SourceChanged); }
        Ok(bytes)
    }
    pub fn resume<T>(&self, key: &str, registered_cwd: &str, host: &str, owner: ConversationOwner, claims: &impl ResumeBoundary<T>) -> Result<T> {
        let s = self.sources.get(key).ok_or(HistoryError::NotFound)?;
        self.validate(s)?;
        if s.entry.cwd != registered_cwd || !Path::new(registered_cwd).is_dir() { return Err(HistoryError::OutsideRoot); }
        let agent_type = match s.entry.provider {CanonicalProvider::Codex => "codex", CanonicalProvider::Claude => "claude"};
        crate::terminal::shell::resolve_agent_resume_plan(agent_type, &s.entry.provider_session).map_err(|_| HistoryError::InvalidIdentity)?;
        claims.claim_and_spawn(ConversationClaimKey { host_id: host.into(), provider: s.entry.provider, conversation_id: s.entry.provider_session.id.clone() }, owner, &s.entry.cwd, TerminalStartup::AgentResume {agent_type: agent_type.into(), provider_session: s.entry.provider_session.clone()})
    }
}
fn digest(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn walk(root: &Path, dir: &Path, files: &mut Vec<(PathBuf,PathBuf)>, visited: &mut usize, warnings: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) { Ok(v) => v, Err(_) => {warnings.push("DIRECTORY_UNAVAILABLE".into()); return;} };
    for entry in entries {
        if *visited >= MAX_FILES { warnings.push("SCAN_LIMIT".into()); break; } *visited += 1;
        let entry = match entry {Ok(v) => v, Err(_) => {warnings.push("ENTRY_UNAVAILABLE".into()); continue;}};
        let kind = match entry.file_type() {Ok(v) => v, Err(_) => {warnings.push("ENTRY_UNAVAILABLE".into()); continue;}};
        if kind.is_symlink() { warnings.push("SYMLINK_REJECTED".into()); }
        else if kind.is_dir() { walk(root,&entry.path(),files,visited,warnings); }
        else if kind.is_file() && entry.path().extension().is_some_and(|s| s == "jsonl") { files.push((root.into(),entry.path())); }
    }
}
fn load(root: &Path, path: &Path, budget: usize) -> Result<Vec<u8>> {
    // Reject every symlink component, not only final filename.
    let relative = path.strip_prefix(root).map_err(|_| HistoryError::OutsideRoot)?;
    let mut checked = root.to_path_buf();
    for part in relative.components() { checked.push(part); if fs::symlink_metadata(&checked).map_err(|_|HistoryError::NotFound)?.file_type().is_symlink() {return Err(HistoryError::OutsideRoot);} }
    if !path.canonicalize().map_err(|_|HistoryError::NotFound)?.starts_with(root) {return Err(HistoryError::OutsideRoot);}
    let file = fs::File::open(path).map_err(|_|HistoryError::Io)?;
    let before = file.metadata().map_err(|_|HistoryError::Io)?;
    if !before.is_file() || before.len() > budget as u64 {return Err(HistoryError::InvalidRequest);}
    let mut bytes = Vec::new(); file.take(budget as u64 + 1).read_to_end(&mut bytes).map_err(|_|HistoryError::Io)?;
    let after = fs::metadata(path).map_err(|_|HistoryError::NotFound)?;
    if bytes.len() > budget || before.len() != after.len() || before.modified().ok() != after.modified().ok() {return Err(HistoryError::SourceChanged);}
    Ok(bytes)
}
fn field(v: &Value, key: &str) -> Option<String> { v.get(key).and_then(Value::as_str).map(str::to_owned) }
fn parse(provider: CanonicalProvider, bytes: &[u8]) -> Result<(Entry,Vec<Message>,Vec<String>)> {
    let mut identity: Option<(String,String)> = None; let mut version = None; let mut parent_id = None; let mut messages = vec![]; let mut warnings = vec![];
    for (ordinal,line) in bytes.split_inclusive(|b| *b == b'\n').enumerate() {
        if line.len() > MAX_RECORD {warnings.push("RECORD_LIMIT".into()); continue;}
        if !line.ends_with(b"\n") {warnings.push("PARTIAL_RECORD".into()); continue;}
        let v: Value = match serde_json::from_slice(line) {Ok(v) => v, Err(_) => {warnings.push("CORRUPT_RECORD".into()); continue;}};
        let kind = v["type"].as_str().unwrap_or("");
        let metadata = match provider { CanonicalProvider::Codex if kind == "session_meta" => Some((&v["payload"],"id","cli_version")), CanonicalProvider::Claude if kind == "user" || kind == "assistant" => Some((&v,"sessionId","version")), _ => None };
        if let Some((meta,id_key,version_key)) = metadata {
            if let (Some(id),Some(cwd)) = (field(meta,id_key),field(meta,"cwd")) {
                if crate::terminal::shell::validate_agent_session_id(&id).ok().as_deref() != Some(&id) || !Path::new(&cwd).is_absolute() {return Err(HistoryError::InvalidIdentity);}
                if identity.as_ref().is_some_and(|old| old != &(id.clone(),cwd.clone())) {return Err(HistoryError::InvalidIdentity);}
                identity = Some((id,cwd)); version = field(meta,version_key).or(version); parent_id = field(meta,"forked_from_id").or(parent_id);
            }
        }
        let message = match provider {CanonicalProvider::Codex if kind == "response_item" && v["payload"]["type"] == "message" => Some(&v["payload"]), CanonicalProvider::Claude if kind == "user" || kind == "assistant" => Some(&v["message"]), _ => None};
        if let Some(m) = message {
            let content = &m["content"];
            let text = if let Some(text) = content.as_str() {text.to_owned()} else {content.as_array().map(|parts| parts.iter().filter(|p| matches!(p["type"].as_str(),Some("text"|"input_text"|"output_text"))).filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_default()};
            if !text.is_empty() {messages.push(Message {ordinal, role: field(m,"role").unwrap_or_else(||kind.into()), text, id: field(&v,"uuid").or_else(||field(m,"id")), parent_id: field(&v,"parentUuid")});}
        }
    }
    let (id,cwd) = identity.ok_or(HistoryError::InvalidIdentity)?;
    Ok((Entry {entry_key:String::new(),provider,provider_session:AgentProviderSession {key:AgentProviderSessionKey::SessionId,id,transcript_path:None},cwd,version,parent_id},messages,warnings))
}
fn page<T: Clone>(items: Vec<T>, cursor: Option<&str>, limit: usize, stamp: &str, warnings: Vec<String>) -> Result<Page<T>> {
    if limit == 0 || limit > 100 { return Err(HistoryError::InvalidRequest); }
    let offset = match cursor { None => 0, Some(cursor) => {
        if cursor.len() > 512 {return Err(HistoryError::InvalidRequest);}
        let decoded = URL_SAFE_NO_PAD.decode(cursor).map_err(|_|HistoryError::InvalidRequest)?;
        let (version,hash,offset): (u8,String,usize) = serde_json::from_slice(&decoded).map_err(|_|HistoryError::InvalidRequest)?;
        if version != 1 || hash != stamp {return Err(HistoryError::SourceChanged);} offset
    }};
    if offset > items.len() {return Err(HistoryError::InvalidRequest);}
    let end = (offset + limit).min(items.len());
    let next_cursor = (end < items.len()).then(|| URL_SAFE_NO_PAD.encode(serde_json::to_vec(&(1,stamp,end)).expect("cursor tuple serializes")));
    Ok(Page {items:items[offset..end].to_vec(),next_cursor,partial:!warnings.is_empty(),warnings})
}
/// Implement with the ONE shared registry: claim + daemon spawn under its atomic fence;
/// roll back on spawn failure, retain ownership until confirmed exit. Never shell text.
pub trait ResumeBoundary<T> {
    fn claim_and_spawn(&self, key: ConversationClaimKey, owner: ConversationOwner, cwd: &str, startup: TerminalStartup) -> Result<T>;
}
