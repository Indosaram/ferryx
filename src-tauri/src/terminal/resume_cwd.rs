use crate::daemon::protocol::{AgentProviderSession, TerminalStartup};
use serde::Deserialize;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Read};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct SessionHeader {
    #[serde(rename = "type")]
    kind: String,
    id: String,
    cwd: PathBuf,
}

pub(crate) fn resolve_agent_resume_cwd(
    startup: Option<&TerminalStartup>,
) -> io::Result<Option<PathBuf>> {
    let Some(TerminalStartup::AgentResume {
        agent_type,
        provider_session,
    }) = startup
    else {
        return Ok(None);
    };
    if !agent_type.trim().eq_ignore_ascii_case("omo") {
        return Ok(None);
    }
    if provider_session.transcript_path.is_some() {
        return resolve_omo_cwd(provider_session, &[]);
    }

    let mut roots = Vec::new();
    for key in ["OMO_CODING_AGENT_DIR", "SENPI_CODING_AGENT_DIR"] {
        if let Some(dir) = std::env::var_os(key).filter(|v| !v.is_empty()) {
            roots.push(PathBuf::from(dir).join("sessions"));
        }
    }
    for key in [
        "OMO_CODING_AGENT_SESSION_DIR",
        "SENPI_CODING_AGENT_SESSION_DIR",
    ] {
        if let Some(dir) = std::env::var_os(key).filter(|v| !v.is_empty()) {
            roots.push(PathBuf::from(dir));
        }
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let config = PathBuf::from(home).join(".omo");
        roots.push(config.join("agent/sessions"));
        roots.push(config.join("sessions"));
        match fs::read_dir(config.join("profiles")) {
            Ok(profiles) => {
                for profile in profiles.flatten() {
                    if profile.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        roots.push(profile.path().join("agent/sessions"));
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => {}
        }
    }
    roots.sort();
    roots.dedup();
    resolve_omo_cwd(provider_session, &roots)
}

/// The ONE list of agents a machine may publish (`session_metadata_provider`) and resume
/// (`spawn_machine`). Each has a process-bound discovery adapter: it answers from the pane's own
/// agent process (its open session file or its resume argv). Providers whose adapter falls back to
/// newest-transcript/CWD guessing (antigravity, opencode, pi) stay out.
pub(crate) const MACHINE_RESUME_AGENTS: [&str; 13] = [
    "omo", "claude", "codex", "copilot", "cursor", "cursor-agent", "kimi", "gjc", "omp", "grok",
    "droid", "cline", "crush",
];

/// Admitted agents whose store records no working directory, so a remote spawn can never prove
/// that the session belongs to the target cwd. They are still published, but a spawn is refused
/// with `AGENT_RESUME_UNSUPPORTED`. copilot: `.omo-parity/facts/copilot-store-cwd-2026-10-22.md`.
pub(crate) const MACHINE_RESUME_NO_CWD_PROOF: [&str; 1] = ["copilot"];

pub(crate) fn machine_admits_agent(agent: &str) -> bool {
    MACHINE_RESUME_AGENTS.contains(&agent)
}

/// Whether a remote spawn may resume `agent` at all; the cwd proof is still required after this.
pub(crate) fn machine_spawn_admits_agent(agent: &str) -> bool {
    machine_admits_agent(agent) && !MACHINE_RESUME_NO_CWD_PROOF.contains(&agent)
}

/// Proves that a remote resume of `provider` targets the session's own project: the provider
/// store record for the id must name `requested` as its cwd. Reads the user's own stores, so call
/// it on a blocking worker. Errors are machine error codes.
pub(crate) fn verify_machine_resume_cwd(
    agent: &str,
    provider: &AgentProviderSession,
    requested: &Path,
) -> Result<PathBuf, &'static str> {
    let resolved = if agent == "omo" && machine_spawn_admits_agent(agent) && provider.transcript_path.is_none() {
        // omo keeps its richer root search (env overrides, profiles).
        let startup = TerminalStartup::AgentResume { agent_type: agent.into(), provider_session: provider.clone() };
        resolve_agent_resume_cwd(Some(&startup))
            .map_err(|_| "AGENT_TRANSCRIPT_UNVERIFIED")
            .and_then(|cwd| cwd.ok_or("AGENT_TRANSCRIPT_UNVERIFIED"))
    } else {
        let home = crate::ipc::file_link::home_dir().ok_or("AGENT_TRANSCRIPT_UNVERIFIED")?;
        resolve_machine_resume_cwd_in(agent, provider, &crate::ipc::agent_history::provider_history_roots(&home))
    };
    matching_cwd(resolved?, requested)
}

/// `verify_machine_resume_cwd` against explicit store roots (the injection seam for tests).
#[cfg(test)]
pub(crate) fn verify_machine_resume_cwd_in(
    agent: &str,
    provider: &AgentProviderSession,
    roots: &[(crate::scoped_contracts::CanonicalProvider, PathBuf)],
    requested: &Path,
) -> Result<PathBuf, &'static str> {
    matching_cwd(resolve_machine_resume_cwd_in(agent, provider, roots)?, requested)
}

fn resolve_machine_resume_cwd_in(
    agent: &str,
    provider: &AgentProviderSession,
    roots: &[(crate::scoped_contracts::CanonicalProvider, PathBuf)],
) -> Result<PathBuf, &'static str> {
    use crate::agent_history::history::{provider_from_wire, session_cwd as store_session_cwd};
    use crate::scoped_contracts::CanonicalProvider;
    if !machine_spawn_admits_agent(agent) || provider.transcript_path.is_some() {
        return Err("AGENT_RESUME_UNSUPPORTED");
    }
    let wire = if agent == "cursor-agent" { "cursor" } else { agent };
    let kind = provider_from_wire(wire).ok_or("AGENT_RESUME_UNSUPPORTED")?;
    if kind == CanonicalProvider::Omo {
        let omo_roots: Vec<PathBuf> = roots.iter().filter(|(p, _)| *p == kind).map(|(_, root)| root.clone()).collect();
        return resolve_omo_cwd(provider, &omo_roots)
            .map_err(|_| "AGENT_TRANSCRIPT_UNVERIFIED")?
            .ok_or("AGENT_TRANSCRIPT_UNVERIFIED");
    }
    store_session_cwd(roots, kind, &provider.id).map_err(|_| "AGENT_TRANSCRIPT_UNVERIFIED")
}

fn matching_cwd(recorded: PathBuf, requested: &Path) -> Result<PathBuf, &'static str> {
    let recorded = fs::canonicalize(recorded).map_err(|_| "AGENT_TRANSCRIPT_UNVERIFIED")?;
    let requested = fs::canonicalize(requested).map_err(|_| "INVALID_PATH")?;
    if recorded != requested {
        return Err("AGENT_CWD_MISMATCH");
    }
    Ok(recorded)
}

fn session_cwd(path: &Path, expected_id: &str) -> io::Result<PathBuf> {
    #[cfg(not(unix))]
    if !fs::metadata(path)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Session transcript must be a regular file",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NONBLOCK);
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Session transcript must be a regular file",
        ));
    }
    let mut header_line = String::new();
    BufReader::new(file.take(65537)).read_line(&mut header_line)?;
    if header_line.len() > 65536 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Session header exceeds 64 KiB",
        ));
    }
    let header: SessionHeader = serde_json::from_str(&header_line)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if header.kind != "session" || header.id != expected_id || !header.cwd.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Session header must match the requested id and contain an absolute CWD",
        ));
    }
    Ok(header.cwd)
}

fn resolve_omo_cwd(
    provider: &AgentProviderSession,
    session_roots: &[PathBuf],
) -> io::Result<Option<PathBuf>> {
    if let Some(path) = &provider.transcript_path {
        return session_cwd(Path::new(path), provider.id.trim()).map(Some);
    }
    let suffix = format!("_{}.jsonl", provider.id.trim());
    let mut found = None;
    for root in session_roots {
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        let mut files = Vec::new();
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if let Ok(sub_entries) = fs::read_dir(entry.path()) {
                    for file in sub_entries.flatten() {
                        files.push(file.path());
                    }
                }
            } else {
                files.push(entry.path());
            }
        }
        for path in files {
            if !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(&suffix))
            {
                continue;
            }
            let cwd = session_cwd(&path, provider.id.trim())?;
            if found.as_ref().is_some_and(|previous| previous != &cwd) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Conflicting project directories for the requested session id",
                ));
            }
            found = Some(cwd);
        }
    }
    found.map(Some).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Cannot find the requested OMO session's original project directory",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::protocol::AgentProviderSessionKey;
    use std::fs;
    use std::path::Path;

    const SESSION_ID: &str = "01a07b36-eb98-70e4-a432-c631ea4af1fe";

    fn transcript(root: &Path, id: &str, cwd: &Path) -> PathBuf {
        let project = root.join("encoded-project");
        fs::create_dir_all(&project).unwrap();
        let path = project.join(format!("2026-09-07T09-33-11-960Z_{id}.jsonl"));
        let header = serde_json::json!({"type": "session", "id": id, "cwd": cwd});
        fs::write(&path, format!("{header}\n{{\"type\":\"message\"}}\n")).unwrap();
        path
    }

    fn provider(path: Option<&Path>) -> AgentProviderSession {
        AgentProviderSession {
            key: AgentProviderSessionKey::SessionId,
            id: SESSION_ID.to_string(),
            transcript_path: path.map(|p| p.to_string_lossy().into_owned()),
        }
    }

    #[test]
    fn restores_nested_project_from_reported_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().join("project/vuev.net");
        let path = transcript(dir.path(), SESSION_ID, &cwd);
        assert_eq!(
            resolve_omo_cwd(&provider(Some(&path)), &[]).unwrap(),
            Some(cwd)
        );
    }

    #[test]
    fn repairs_legacy_id_only_state_from_exact_session_header() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().join("project/vuev.net");
        transcript(dir.path(), SESSION_ID, &cwd);
        transcript(dir.path(), "another-session", &dir.path().join("unrelated"));
        assert_eq!(
            resolve_omo_cwd(&provider(None), &[dir.path().to_path_buf()]).unwrap(),
            Some(cwd)
        );
    }

    #[test]
    fn rejects_transcript_belonging_to_another_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = transcript(dir.path(), "another-session", dir.path());
        assert!(resolve_omo_cwd(&provider(Some(&path)), &[]).is_err());
    }

    #[test]
    fn rejects_missing_legacy_session_instead_of_using_pane_root() {
        let dir = tempfile::tempdir().unwrap();
        let error = resolve_omo_cwd(&provider(None), &[dir.path().to_path_buf()]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn rejects_relative_project_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = transcript(dir.path(), SESSION_ID, Path::new("relative/project"));
        assert!(resolve_omo_cwd(&provider(Some(&path)), &[]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_fifo_transcripts() {
        use std::io::Write;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        assert!(std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success());
        let mut stream = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        writeln!(
            stream,
            "{}",
            serde_json::json!({
                "type": "session", "id": SESSION_ID, "cwd": dir.path(),
            })
        )
        .unwrap();
        assert!(resolve_omo_cwd(&provider(Some(&path)), &[]).is_err());
    }

    #[test]
    fn rejects_conflicting_project_directories_for_same_id() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        transcript(first.path(), SESSION_ID, first.path());
        transcript(second.path(), SESSION_ID, second.path());
        assert!(resolve_omo_cwd(
            &provider(None),
            &[first.path().to_path_buf(), second.path().to_path_buf()],
        )
        .is_err());
    }

    #[test]
    fn non_omo_startups_keep_their_existing_directory_contract() {
        assert_eq!(resolve_agent_resume_cwd(None).unwrap(), None);
        let startup = TerminalStartup::AgentResume {
            agent_type: "claude".to_string(),
            provider_session: provider(None),
        };
        assert_eq!(resolve_agent_resume_cwd(Some(&startup)).unwrap(), None);
    }

    #[test]
    fn resolved_resume_directory_is_used_by_a_real_process() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().join("project/vuev.net");
        fs::create_dir_all(&cwd).unwrap();
        let cwd = fs::canonicalize(cwd).unwrap();
        let path = transcript(dir.path(), SESSION_ID, &cwd);
        let startup = TerminalStartup::AgentResume {
            agent_type: "omo".to_string(),
            provider_session: provider(Some(&path)),
        };
        let resolved = resolve_agent_resume_cwd(Some(&startup)).unwrap().unwrap();
        #[cfg(windows)]
        let mut command = {
            let mut command = std::process::Command::new("cmd.exe");
            command.args(["/D", "/C", "cd"]);
            command
        };
        #[cfg(not(windows))]
        let mut command = std::process::Command::new("/bin/pwd");
        let output = command.current_dir(&resolved).output().unwrap();
        assert!(output.status.success());
        let printed = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
        assert_eq!(fs::canonicalize(printed).unwrap(), cwd);
    }

    #[cfg(unix)]
    #[test]
    fn regression_local_pty_create_7_resolve_omo_cwd_ignores_unreadable_entries() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let unreadable_dir = dir.path().join("unreadable_dir");
        fs::create_dir(&unreadable_dir).unwrap();
        fs::set_permissions(&unreadable_dir, fs::Permissions::from_mode(0o000)).unwrap();

        let cwd = dir.path().join("project/vuev.net");
        transcript(dir.path(), SESSION_ID, &cwd);

        let result = resolve_omo_cwd(
            &provider(None),
            &[unreadable_dir.clone(), dir.path().to_path_buf()],
        );
        let _ = fs::set_permissions(&unreadable_dir, fs::Permissions::from_mode(0o755));
        assert_eq!(result.unwrap(), Some(cwd));
    }

    // Gap C27: a remote machine spawn resumes a non-omo agent only in the cwd its own store
    // records for the session. Each store below copies the shape the history readers parse
    // (`ferryx_scope/history/fixtures`, `ferryx_scope/history/tests.rs`).
    use crate::scoped_contracts::CanonicalProvider;

    const UNKNOWN_ID: &str = "0f0f0f0f-0000-4000-8000-0000000000ff";

    fn stored(id: &str) -> AgentProviderSession {
        AgentProviderSession {
            key: AgentProviderSessionKey::SessionId,
            id: id.to_string(),
            transcript_path: None,
        }
    }

    fn put(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    /// A history fixture with its `"__CWD__"` placeholder replaced by `cwd` as a JSON string.
    fn fill(body: &str, cwd: &Path) -> String {
        body.replace("\"__CWD__\"", &serde_json::to_string(&cwd.to_string_lossy()).unwrap())
    }

    /// The C27 contract over a store at `root` that records session `id` in `project`: the
    /// recorded cwd admits the spawn, any other directory is a mismatch, and an id the store
    /// does not hold is unverified.
    fn assert_spawn_contract(agent: &str, kind: CanonicalProvider, root: &Path, id: &str, project: &Path) {
        let roots = [(kind, root.to_path_buf())];
        let other = tempfile::tempdir().unwrap();
        assert_eq!(
            verify_machine_resume_cwd_in(agent, &stored(id), &roots, project),
            Ok(fs::canonicalize(project).unwrap()),
            "{agent}: the store's own cwd must admit the spawn"
        );
        assert_eq!(
            verify_machine_resume_cwd_in(agent, &stored(id), &roots, other.path()),
            Err("AGENT_CWD_MISMATCH"),
            "{agent}: another directory must be refused"
        );
        assert_eq!(
            verify_machine_resume_cwd_in(agent, &stored(UNKNOWN_ID), &roots, project),
            Err("AGENT_TRANSCRIPT_UNVERIFIED"),
            "{agent}: an id the store does not hold must be refused"
        );
    }

    #[test]
    fn machine_resume_claude_requires_its_store_cwd() {
        let (root, project) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let id = "63ba88ec-b3cb-4a70-8a32-f5d1ad07e749";
        let record = serde_json::json!({"type":"user","uuid":"a","parentUuid":null,"sessionId":id,"cwd":project.path(),"version":"2.1.251","message":{"role":"user","content":[{"type":"text","text":"hi"}]}});
        put(&root.path().join("-encoded-project").join(format!("{id}.jsonl")), &format!("{record}\n"));
        assert_spawn_contract("claude", CanonicalProvider::Claude, root.path(), id, project.path());
    }

    #[test]
    fn machine_resume_codex_requires_its_store_cwd() {
        let (root, project) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let id = "019ec598-c800-7000-8000-000000000001";
        let meta = serde_json::json!({"type":"session_meta","payload":{"id":id,"cwd":project.path(),"cli_version":"0.153.2"}});
        put(&root.path().join("2026/10/22").join(format!("rollout-2026-10-22T00-00-00-{id}.jsonl")), &format!("{meta}\n"));
        assert_spawn_contract("codex", CanonicalProvider::Codex, root.path(), id, project.path());
    }

    #[test]
    fn machine_resume_omo_omp_gjc_require_their_session_header_cwd() {
        for (agent, kind, body, id) in [
            ("omo", CanonicalProvider::Omo, include_str!("../ferryx_scope/history/fixtures/omo/session.jsonl"), "01a16281-1b66-764d-87a3-4ea9321c5bd8"),
            ("omp", CanonicalProvider::Omp, include_str!("../ferryx_scope/history/fixtures/omp/session.jsonl"), "01a16270-e358-7635-88e4-1b2f84460057"),
            ("gjc", CanonicalProvider::Gjc, include_str!("../ferryx_scope/history/fixtures/gjc/session.jsonl"), "01a16274-abd4-7581-8f9c-22214661ef6a"),
        ] {
            let (root, project) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
            put(&root.path().join("--slug--").join(format!("2026-10-22T07-26-30-758Z_{id}.jsonl")), &fill(body, project.path()));
            assert_spawn_contract(agent, kind, root.path(), id, project.path());
        }
    }

    #[test]
    fn machine_resume_kimi_requires_the_kimi_json_cwd_of_its_md5_folder() {
        use md5::{Digest as _, Md5};
        let (home, project) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let id = "cb0ab2dd-f2e4-4a49-9c8b-843d866bb8f2";
        let cwd = project.path().to_string_lossy().into_owned();
        let root = home.path().join(".kimi").join("sessions");
        put(
            &root.join(format!("{:x}", Md5::digest(cwd.as_bytes()))).join(id).join("context.jsonl"),
            include_str!("../ferryx_scope/history/fixtures/kimi/context.jsonl"),
        );
        put(&home.path().join(".kimi").join("kimi.json"), &serde_json::json!({"work_dirs":[{"path":cwd,"kaos":"local","last_session_id":id}]}).to_string());
        assert_spawn_contract("kimi", CanonicalProvider::Kimi, &root, id, project.path());
    }

    /// A minimal cursor `store.db`: the hex JSON `meta` row names the root blob, and the root
    /// tree node carries the workspace as field 21 { field 1: path } with no child entries.
    #[cfg(unix)]
    fn write_cursor_store(root: &Path, id: &str, cwd: &str) {
        use md5::{Digest as _, Md5};
        fn varint(out: &mut Vec<u8>, mut value: usize) {
            loop {
                let byte = (value & 0x7f) as u8;
                value >>= 7;
                if value == 0 {
                    out.push(byte);
                    return;
                }
                out.push(byte | 0x80);
            }
        }
        let mut workspace = vec![0x0a];
        varint(&mut workspace, cwd.len());
        workspace.extend_from_slice(cwd.as_bytes());
        let mut node = vec![0xaa, 0x01];
        varint(&mut node, workspace.len());
        node.extend(workspace);
        let meta = serde_json::json!({"agentId": id, "latestRootBlobId": "root"}).to_string();
        let encoded = meta.bytes().map(|byte| format!("{byte:02x}")).collect::<String>();
        let store = root.join(format!("{:x}", Md5::digest(cwd.as_bytes()))).join(id);
        fs::create_dir_all(&store).unwrap();
        let connection = rusqlite::Connection::open(store.join("store.db")).unwrap();
        connection.execute_batch("CREATE TABLE blobs (id TEXT PRIMARY KEY, data BLOB); CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);").unwrap();
        connection.execute("INSERT INTO meta VALUES ('0', ?1)", rusqlite::params![encoded]).unwrap();
        connection.execute("INSERT INTO blobs VALUES ('root', ?1)", rusqlite::params![node]).unwrap();
    }

    /// cursor's reader only trusts a `/`-rooted workspace path, so this store is unix-only.
    #[cfg(unix)]
    #[test]
    fn machine_resume_cursor_and_cursor_agent_require_the_root_node_workspace() {
        for agent in ["cursor", "cursor-agent"] {
            let (root, project) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
            let id = "41ae0fb1-b55f-4592-a462-343915618791";
            write_cursor_store(root.path(), id, &project.path().to_string_lossy());
            assert_spawn_contract(agent, CanonicalProvider::Cursor, root.path(), id, project.path());
        }
    }

    #[test]
    fn machine_resume_grok_requires_its_summary_cwd() {
        let (root, project) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let id = "01a16274-a688-7e73-b6fc-434bc7781be4";
        put(&root.path().join("%2Ftmp%2Fproj").join(id).join("summary.json"), &fill(include_str!("../ferryx_scope/history/fixtures/grok/summary.json"), project.path()));
        assert_spawn_contract("grok", CanonicalProvider::Grok, root.path(), id, project.path());
    }

    #[test]
    fn machine_resume_droid_requires_its_session_start_cwd() {
        let (root, project) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let id = "d08d895d-058f-4f86-aa21-48d7dbdcaa9f";
        put(&root.path().join("-tmp-work").join(format!("{id}.jsonl")), &fill(include_str!("../ferryx_scope/history/fixtures/droid/session.jsonl"), project.path()));
        assert_spawn_contract("droid", CanonicalProvider::Droid, root.path(), id, project.path());
    }

    #[test]
    fn machine_resume_cline_requires_its_metadata_cwd() {
        let (root, project) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let id = "1792653340197_ys4yu";
        put(&root.path().join(id).join(format!("{id}.json")), &fill(include_str!("../ferryx_scope/history/fixtures/cline/session.json"), project.path()));
        assert_spawn_contract("cline", CanonicalProvider::Cline, root.path(), id, project.path());
    }

    #[test]
    fn machine_resume_crush_requires_its_project_index_cwd() {
        let (share, project) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let id = "3df112d9-0000-4000-8000-000000000001";
        let data_dir = project.path().join(".crush");
        fs::create_dir_all(&data_dir).unwrap();
        put(&share.path().join("projects.json"), &serde_json::json!({"projects":[{"path":project.path(),"data_dir":data_dir,"last_accessed":"2026-10-22T16:06:02+09:00"}]}).to_string());
        let connection = rusqlite::Connection::open(data_dir.join("crush.db")).unwrap();
        connection.execute_batch("CREATE TABLE sessions (id TEXT PRIMARY KEY, parent_session_id TEXT, title TEXT, created_at INTEGER, updated_at INTEGER); CREATE TABLE messages (id TEXT PRIMARY KEY, session_id TEXT, role TEXT, parts TEXT, created_at INTEGER);").unwrap();
        connection.execute("INSERT INTO sessions VALUES (?1, NULL, 'redacted-title', 1, 2)", rusqlite::params![id]).unwrap();
        drop(connection);
        assert_spawn_contract("crush", CanonicalProvider::Crush, share.path(), id, project.path());
    }

    #[test]
    fn machine_resume_copilot_is_published_but_never_spawned() {
        let (root, project) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let id = "5793c06c-5bee-4b34-883b-589051286cfd";
        put(&root.path().join(format!("{id}.jsonl")), include_str!("../ferryx_scope/history/fixtures/copilot/session.jsonl"));
        assert!(machine_admits_agent("copilot"), "copilot sessions are still published");
        assert!(!machine_spawn_admits_agent("copilot"));
        assert_eq!(
            verify_machine_resume_cwd_in("copilot", &stored(id), &[(CanonicalProvider::Copilot, root.path().to_path_buf())], project.path()),
            Err("AGENT_RESUME_UNSUPPORTED"),
            "copilot's store records no cwd, so no spawn can be proven"
        );
    }

    #[test]
    fn machine_resume_refuses_agents_without_process_bound_discovery() {
        let project = tempfile::tempdir().unwrap();
        for agent in ["antigravity", "opencode", "pi", "aider", ""] {
            assert!(!machine_spawn_admits_agent(agent), "{agent}");
            assert_eq!(
                verify_machine_resume_cwd_in(agent, &stored(SESSION_ID), &[], project.path()),
                Err("AGENT_RESUME_UNSUPPORTED"),
                "{agent} must stay unsupported"
            );
        }
    }

    #[test]
    fn machine_resume_refuses_a_transcript_path_for_admitted_agents() {
        let (root, project) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let id = "01a16270-e358-7635-88e4-1b2f84460057";
        let path = root.path().join("--slug--").join(format!("2026-10-22T07-09-30-231Z_{id}.jsonl"));
        put(&path, &fill(include_str!("../ferryx_scope/history/fixtures/omp/session.jsonl"), project.path()));
        let mut provider = stored(id);
        provider.transcript_path = Some(path.to_string_lossy().into_owned());
        for agent in ["omp", "claude", "omo"] {
            assert_eq!(
                verify_machine_resume_cwd_in(agent, &provider, &[(CanonicalProvider::Omp, root.path().to_path_buf())], project.path()),
                Err("AGENT_RESUME_UNSUPPORTED"),
                "{agent}: a caller-supplied transcript path is never trusted on a machine spawn"
            );
        }
    }
}
