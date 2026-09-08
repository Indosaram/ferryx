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
                for profile in profiles {
                    let profile = profile?;
                    if profile.file_type()?.is_dir() {
                        roots.push(profile.path().join("agent/sessions"));
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    roots.sort();
    roots.dedup();
    resolve_omo_cwd(provider_session, &roots)
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
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                for file in fs::read_dir(entry.path())? {
                    files.push(file?.path());
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
}
