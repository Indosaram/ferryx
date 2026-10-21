//! Done-signal grammar ported from omo-gateway. Checks run locally: there is no SSH runner.
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum SignalParseError {
    #[error("signals list is empty")]
    Empty,
    #[error("unknown signal kind: {0}")]
    UnknownKind(String),
    #[error("uncheckable command: {0}")]
    UncheckableCommand(String),
    #[error("malformed JSON value: {0}")]
    MalformedJson(String),
    #[error("invalid regex: {0}")]
    InvalidRegex(String),
    #[error("invalid syntax: {0}")]
    InvalidSyntax(String),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DoneSignal {
    Merged {
        owner_repo: String,
        pr_number: u64,
    },
    Closed {
        owner_repo: String,
        issue_number: u64,
    },
    Published {
        package: String,
        version: String,
    },
    Command {
        argv: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stdout_regex: Option<String>,
    },
    File {
        path: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sha256: Option<String>,
    },
    Url {
        url: String,
        status: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalResult {
    pub signal: String,
    pub passed: bool,
    pub detail: String,
}

/// Removes a leading `done =` prefix if present.
pub fn strip_done_prefix(raw: &str) -> &str {
    let trimmed = raw.trim();
    let lower = trimmed.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("done") {
        let rest_trimmed = rest.trim_start();
        if rest_trimmed.starts_with('=') {
            let offset = trimmed.len() - rest_trimmed.len() + 1;
            return trimmed[offset..].trim();
        }
    }
    trimmed
}

pub fn parse_signals(input: &str) -> Result<Vec<DoneSignal>, SignalParseError> {
    let trimmed = strip_done_prefix(input).trim();
    if trimmed.is_empty() {
        return Err(SignalParseError::Empty);
    }
    let mut signals = Vec::new();
    for segment in split_top_level_semicolons(trimmed)? {
        signals.push(parse_single_signal(&segment)?);
    }
    Ok(signals)
}

fn split_top_level_semicolons(input: &str) -> Result<Vec<String>, SignalParseError> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut escaped = false;
    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            current.push(ch);
            escaped = true;
            continue;
        }
        if in_quote {
            current.push(ch);
            if ch == '"' {
                in_quote = false;
            }
            continue;
        }
        match ch {
            '"' => {
                in_quote = true;
                current.push(ch);
            }
            '[' => {
                bracket_depth += 1;
                current.push(ch);
            }
            ']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                current.push(ch);
            }
            '{' => {
                brace_depth += 1;
                current.push(ch);
            }
            '}' => {
                brace_depth = brace_depth.saturating_sub(1);
                current.push(ch);
            }
            ';' if bracket_depth == 0 && brace_depth == 0 => {
                let piece = current.trim();
                if piece.is_empty() {
                    return Err(SignalParseError::InvalidSyntax(
                        "empty signal before semicolon".into(),
                    ));
                }
                parts.push(piece.to_owned());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let piece = current.trim();
    if !piece.is_empty() {
        parts.push(piece.to_owned());
    }
    if parts.is_empty() {
        return Err(SignalParseError::Empty);
    }
    Ok(parts)
}

fn parse_single_signal(raw: &str) -> Result<DoneSignal, SignalParseError> {
    let (kind, rest) = match raw.trim().split_once(char::is_whitespace) {
        Some((kind, rest)) => (kind.trim(), rest.trim()),
        None => (raw.trim(), ""),
    };
    match kind {
        "merged" => parse_merged(rest),
        "closed" => parse_closed(rest),
        "published" => parse_published(rest),
        "command" => parse_command(rest),
        "file" => parse_file(rest),
        "url" => parse_url(rest),
        unknown => Err(SignalParseError::UnknownKind(unknown.to_owned())),
    }
}

fn parse_github_target(rest: &str, kind: &str) -> Result<(String, u64), SignalParseError> {
    let trimmed = rest.trim();
    let (repo_part, num_str) = if let Some((repo, number)) = trimmed.split_once('#') {
        (repo, number)
    } else if let Some((repo, number)) = trimmed.split_once("/pull/") {
        (repo, number)
    } else if let Some((repo, number)) = trimmed.split_once("/issues/") {
        (repo, number)
    } else {
        return Err(SignalParseError::InvalidSyntax(format!(
            "missing '#', '/pull/' or '/issues/' in {kind} signal: {rest}"
        )));
    };
    let (owner, repo) = repo_part.split_once('/').ok_or_else(|| {
        SignalParseError::InvalidSyntax(format!(
            "missing '/' in owner/repo for {kind} signal: {repo_part}"
        ))
    })?;
    let number: u64 = num_str.trim().parse().map_err(|error| {
        SignalParseError::InvalidSyntax(format!(
            "invalid {kind} number '{}': {error}",
            num_str.trim()
        ))
    })?;
    if owner.trim().is_empty() || repo.trim().is_empty() || number == 0 {
        return Err(SignalParseError::InvalidSyntax(format!(
            "invalid {kind} target: {rest}"
        )));
    }
    Ok((format!("{}/{}", owner.trim(), repo.trim()), number))
}

fn parse_merged(rest: &str) -> Result<DoneSignal, SignalParseError> {
    let (owner_repo, pr_number) = parse_github_target(rest, "merged")?;
    Ok(DoneSignal::Merged {
        owner_repo,
        pr_number,
    })
}

fn parse_closed(rest: &str) -> Result<DoneSignal, SignalParseError> {
    let (owner_repo, issue_number) = parse_github_target(rest, "closed")?;
    Ok(DoneSignal::Closed {
        owner_repo,
        issue_number,
    })
}

fn parse_published(rest: &str) -> Result<DoneSignal, SignalParseError> {
    let (package, version) = rest.split_once('@').ok_or_else(|| {
        SignalParseError::InvalidSyntax(format!("missing '@' in published signal: {rest}"))
    })?;
    if package.trim().is_empty() || version.trim().is_empty() {
        return Err(SignalParseError::InvalidSyntax(format!(
            "invalid published signal: {rest}"
        )));
    }
    Ok(DoneSignal::Published {
        package: package.trim().to_owned(),
        version: version.trim().to_owned(),
    })
}

fn parse_command(rest: &str) -> Result<DoneSignal, SignalParseError> {
    if !rest.starts_with('[') {
        return Err(SignalParseError::InvalidSyntax(format!(
            "command argv must be a JSON array starting with '[': {rest}"
        )));
    }
    let value: serde_json::Value =
        serde_json::from_str(rest.split_whitespace().next().unwrap_or(rest))
            .or_else(|_| serde_json::from_str(rest))
            .map_err(|error| {
                SignalParseError::InvalidSyntax(format!(
                    "malformed command argv JSON array: {error}"
                ))
            })?;
    let argv = match value {
        serde_json::Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                serde_json::Value::String(text) => Ok(text),
                _ => Err(SignalParseError::InvalidSyntax(
                    "command argv elements must be strings".into(),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err(SignalParseError::InvalidSyntax(
                "command argv must be a JSON array".into(),
            ))
        }
    };
    if argv.is_empty()
        || argv
            .iter()
            .any(|arg| arg == "true" || arg == ":" || arg == "/bin/true" || arg == "/usr/bin/true")
            && argv.len() == 1
    {
        return Err(SignalParseError::UncheckableCommand(format!(
            "uncheckable command: {argv:?}"
        )));
    }
    if argv.first().is_some_and(|arg| arg == "true" || arg == ":") {
        return Err(SignalParseError::UncheckableCommand(format!(
            "uncheckable command: {argv:?}"
        )));
    }
    Ok(DoneSignal::Command {
        argv,
        stdout_regex: None,
    })
}

fn parse_file(rest: &str) -> Result<DoneSignal, SignalParseError> {
    let rest = rest.trim();
    if rest.is_empty() {
        return Err(SignalParseError::InvalidSyntax(
            "missing path for file signal".into(),
        ));
    }
    let (path, sha) = if let Some(quoted) = rest.strip_prefix('"') {
        let end = quoted.find('"').ok_or_else(|| {
            SignalParseError::InvalidSyntax("unterminated quote in file path".into())
        })?;
        (quoted[..end].to_owned(), quoted[end + 1..].trim())
    } else {
        match rest.split_once(char::is_whitespace) {
            Some((path, rest)) => (path.to_owned(), rest.trim()),
            None => (rest.to_owned(), ""),
        }
    };
    let mut sha256 = None;
    if let Some(hex) = sha.strip_prefix("sha256=") {
        let hex = hex.trim();
        if hex.len() != 64 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(SignalParseError::InvalidSyntax(
                "sha256= takes 64 hex characters".into(),
            ));
        }
        sha256 = Some(hex.to_ascii_lowercase());
    } else if !sha.is_empty() {
        return Err(SignalParseError::InvalidSyntax(format!(
            "unexpected file option: {sha}"
        )));
    }
    Ok(DoneSignal::File {
        path: PathBuf::from(path),
        sha256,
    })
}

fn parse_url(rest: &str) -> Result<DoneSignal, SignalParseError> {
    let url = rest.split_whitespace().next().unwrap_or("").trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(SignalParseError::InvalidSyntax(format!(
            "invalid URL: {rest}"
        )));
    }
    Ok(DoneSignal::Url {
        url: url.to_owned(),
        status: 200,
    })
}

/// Runs file and command signals on this machine. Merged, closed, published, and url
/// signals are reported as not checked: the hub does not call GitHub or the network for them.
pub async fn check_signals(signals: &[DoneSignal], cwd: &Path) -> Vec<SignalResult> {
    let mut results = Vec::new();
    for signal in signals {
        results.push(check_one(signal, cwd).await);
    }
    results
}

async fn check_one(signal: &DoneSignal, cwd: &Path) -> SignalResult {
    match signal {
        DoneSignal::File { path, sha256 } => {
            let full = if path.is_absolute() {
                path.clone()
            } else {
                cwd.join(path)
            };
            match std::fs::read(&full) {
                Err(error) => SignalResult {
                    signal: format!("file {}", path.display()),
                    passed: false,
                    detail: error.to_string(),
                },
                Ok(bytes) => {
                    let passed = match sha256 {
                        Some(expected) => {
                            use sha2::{Digest, Sha256};
                            let digest = Sha256::digest(&bytes);
                            let actual = digest
                                .iter()
                                .map(|byte| format!("{byte:02x}"))
                                .collect::<String>();
                            &actual == expected
                        }
                        None => true,
                    };
                    SignalResult {
                        signal: format!("file {}", path.display()),
                        passed,
                        detail: full.display().to_string(),
                    }
                }
            }
        }
        DoneSignal::Command { argv, stdout_regex } => {
            let mut command = tokio::process::Command::new(&argv[0]);
            command.args(&argv[1..]).current_dir(cwd);
            match tokio::time::timeout(Duration::from_secs(20), command.output()).await {
                Ok(Ok(output)) if output.status.success() => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let passed = match stdout_regex {
                        Some(pattern) => {
                            regex::Regex::new(pattern).is_ok_and(|re| re.is_match(&stdout))
                        }
                        None => true,
                    };
                    SignalResult {
                        signal: format!("command {argv:?}"),
                        passed,
                        detail: stdout.trim().to_owned(),
                    }
                }
                Ok(Ok(output)) => SignalResult {
                    signal: format!("command {argv:?}"),
                    passed: false,
                    detail: format!("exit {}", output.status),
                },
                Ok(Err(error)) => SignalResult {
                    signal: format!("command {argv:?}"),
                    passed: false,
                    detail: error.to_string(),
                },
                Err(_) => SignalResult {
                    signal: format!("command {argv:?}"),
                    passed: false,
                    detail: "timed out".into(),
                },
            }
        }
        other => SignalResult {
            signal: format!("{other:?}"),
            passed: false,
            detail: "not checked by the in-process hub".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_file_signal_and_rejects_an_uncheckable_true_command() {
        let signals = parse_signals("done = file \"/tmp/marker\"; merged acme/app#12").unwrap();
        assert!(matches!(signals[0], DoneSignal::File { .. }));
        assert!(matches!(
            signals[1],
            DoneSignal::Merged { pr_number: 12, .. }
        ));
        let error = parse_signals(r#"command ["true"]"#).unwrap_err();
        assert!(matches!(error, SignalParseError::UncheckableCommand(_)));
    }

    #[tokio::test]
    async fn a_file_signal_passes_only_when_the_file_exists() {
        let dir = std::env::temp_dir().join(format!("ferryx-hub-sig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("marker"), "ok").unwrap();
        let signals = parse_signals("file marker").unwrap();
        let results = check_signals(&signals, &dir).await;
        assert!(results[0].passed, "{results:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
