// allow: SIZE_OK — Shell resolution and agent resume argv mapping
use crate::daemon::protocol::{AgentProviderSession, AgentProviderSessionKey, TerminalStartup};
use portable_pty::CommandBuilder;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentResumeError {
    #[error("Unsupported agent type: {0}")]
    UnsupportedAgent(String),
    #[error("Invalid provider session id: {0}")]
    InvalidSessionId(String),
    #[error(
        "Invalid provider session key for agent {agent}: expected {expected:?}, got {actual:?}"
    )]
    InvalidKey {
        agent: String,
        expected: AgentProviderSessionKey,
        actual: AgentProviderSessionKey,
    },
    #[error("Transcript path is required for agent {0}")]
    MissingTranscriptPath(String),
    #[error("Invalid transcript path: {0}")]
    InvalidTranscriptPath(String),
}

fn has_unsafe_control_chars(s: &str) -> bool {
    s.chars().any(|c| (c as u32) <= 0x1f || (c as u32) == 0x7f)
}

pub fn validate_agent_session_id(id: &str) -> Result<String, AgentResumeError> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err(AgentResumeError::InvalidSessionId(
            "id cannot be empty".to_string(),
        ));
    }
    if trimmed.len() > 512 {
        return Err(AgentResumeError::InvalidSessionId(
            "id exceeds max length of 512 characters".to_string(),
        ));
    }
    if trimmed.starts_with('-') {
        return Err(AgentResumeError::InvalidSessionId(
            "id cannot start with a dash".to_string(),
        ));
    }
    if trimmed.eq_ignore_ascii_case("latest") {
        return Err(AgentResumeError::InvalidSessionId(
            "id cannot be 'latest'".to_string(),
        ));
    }
    if has_unsafe_control_chars(trimmed) {
        return Err(AgentResumeError::InvalidSessionId(
            "id contains control characters".to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

pub fn validate_transcript_path(path: Option<&str>) -> Result<Option<String>, AgentResumeError> {
    let Some(p) = path else {
        return Ok(None);
    };
    let trimmed = p.trim();
    if trimmed.is_empty() {
        return Err(AgentResumeError::InvalidTranscriptPath(
            "transcript path cannot be empty".to_string(),
        ));
    }
    if trimmed.len() > 4096 {
        return Err(AgentResumeError::InvalidTranscriptPath(
            "transcript path exceeds max length of 4096 characters".to_string(),
        ));
    }
    if trimmed.starts_with('-') {
        return Err(AgentResumeError::InvalidTranscriptPath(
            "transcript path cannot start with a dash".to_string(),
        ));
    }
    if has_unsafe_control_chars(trimmed) {
        return Err(AgentResumeError::InvalidTranscriptPath(
            "transcript path contains control characters".to_string(),
        ));
    }
    let is_absolute = trimmed.starts_with('/')
        || (trimmed.len() >= 3
            && trimmed.as_bytes()[0].is_ascii_alphabetic()
            && trimmed.as_bytes()[1] == b':'
            && matches!(trimmed.as_bytes()[2], b'\\' | b'/'))
        || trimmed.starts_with("\\\\");
    if !is_absolute {
        return Err(AgentResumeError::InvalidTranscriptPath(
            "transcript path must be absolute".to_string(),
        ));
    }
    Ok(Some(trimmed.to_string()))
}

pub fn resolve_agent_resume_plan(
    agent_type: &str,
    provider_session: &AgentProviderSession,
) -> Result<ShellCommandPlan, AgentResumeError> {
    let norm_agent = agent_type.trim().to_ascii_lowercase();
    let validated_id = validate_agent_session_id(&provider_session.id)?;
    let validated_transcript =
        validate_transcript_path(provider_session.transcript_path.as_deref())?;

    let ensure_key = |expected: AgentProviderSessionKey| -> Result<(), AgentResumeError> {
        if provider_session.key != expected {
            Err(AgentResumeError::InvalidKey {
                agent: norm_agent.clone(),
                expected,
                actual: provider_session.key,
            })
        } else {
            Ok(())
        }
    };

    match norm_agent.as_str() {
        "claude" => {
            ensure_key(AgentProviderSessionKey::SessionId)?;
            Ok(ShellCommandPlan {
                program: "claude".to_string(),
                args: vec!["--resume".to_string(), validated_id],
            })
        }
        "codex" => {
            ensure_key(AgentProviderSessionKey::SessionId)?;
            Ok(ShellCommandPlan {
                program: "codex".to_string(),
                args: vec!["resume".to_string(), validated_id],
            })
        }
        "gemini" => {
            ensure_key(AgentProviderSessionKey::SessionId)?;
            Ok(ShellCommandPlan {
                program: "gemini".to_string(),
                args: vec!["--resume".to_string(), validated_id],
            })
        }
        "antigravity" => {
            ensure_key(AgentProviderSessionKey::ConversationId)?;
            Ok(ShellCommandPlan {
                program: "agy".to_string(),
                args: vec!["--conversation".to_string(), validated_id],
            })
        }
        "opencode" => {
            ensure_key(AgentProviderSessionKey::SessionId)?;
            Ok(ShellCommandPlan {
                program: "opencode".to_string(),
                args: vec!["--session".to_string(), validated_id],
            })
        }
        "pi" => {
            ensure_key(AgentProviderSessionKey::SessionId)?;
            let transcript = validated_transcript
                .ok_or_else(|| AgentResumeError::MissingTranscriptPath("pi".to_string()))?;
            Ok(ShellCommandPlan {
                program: "pi".to_string(),
                args: vec!["--session".to_string(), transcript],
            })
        }
        "prime-agent" => {
            ensure_key(AgentProviderSessionKey::SessionId)?;
            let transcript = validated_transcript.ok_or_else(|| {
                AgentResumeError::MissingTranscriptPath("prime-agent".to_string())
            })?;
            Ok(ShellCommandPlan {
                program: "prime-agent".to_string(),
                args: vec!["--resume".to_string(), transcript],
            })
        }
        "mimo-code" => {
            ensure_key(AgentProviderSessionKey::SessionId)?;
            Ok(ShellCommandPlan {
                program: "mimo".to_string(),
                args: vec!["--session".to_string(), validated_id],
            })
        }
        "droid" => {
            ensure_key(AgentProviderSessionKey::SessionId)?;
            Ok(ShellCommandPlan {
                program: "droid".to_string(),
                args: vec!["--resume".to_string(), validated_id],
            })
        }
        "grok" => {
            ensure_key(AgentProviderSessionKey::SessionId)?;
            Ok(ShellCommandPlan {
                program: "grok".to_string(),
                args: vec!["--resume".to_string(), validated_id],
            })
        }
        "devin" => {
            ensure_key(AgentProviderSessionKey::SessionId)?;
            Ok(ShellCommandPlan {
                program: "devin".to_string(),
                args: vec!["--resume".to_string(), validated_id],
            })
        }
        "omp" => {
            ensure_key(AgentProviderSessionKey::SessionId)?;
            let target = validated_transcript.unwrap_or(validated_id);
            Ok(ShellCommandPlan {
                program: "omp".to_string(),
                args: vec!["--resume".to_string(), target],
            })
        }
        "omo" => {
            ensure_key(AgentProviderSessionKey::SessionId)?;
            Ok(ShellCommandPlan {
                program: "omo".to_string(),
                args: vec!["--session".to_string(), validated_id],
            })
        }
        "kimi" => {
            ensure_key(AgentProviderSessionKey::SessionId)?;
            Ok(ShellCommandPlan {
                program: "kimi".to_string(),
                args: vec!["--session".to_string(), validated_id],
            })
        }
        "gjc" => {
            ensure_key(AgentProviderSessionKey::SessionId)?;
            Ok(ShellCommandPlan {
                program: "gjc".to_string(),
                args: vec!["--resume".to_string(), validated_id],
            })
        }
        "copilot" => {
            ensure_key(AgentProviderSessionKey::SessionId)?;
            Ok(ShellCommandPlan {
                program: "copilot".to_string(),
                args: vec!["--resume".to_string(), validated_id],
            })
        }
        "cursor" | "cursor-agent" => {
            ensure_key(AgentProviderSessionKey::SessionId)?;
            Ok(ShellCommandPlan {
                program: "cursor-agent".to_string(),
                args: vec!["--resume".to_string(), validated_id],
            })
        }
        #[cfg(all(test, unix))]
        "ferryx-test-agent" => {
            ensure_key(AgentProviderSessionKey::SessionId)?;
            Ok(ShellCommandPlan {
                program: "/bin/cat".to_string(),
                args: Vec::new(),
            })
        }
        unsupported => Err(AgentResumeError::UnsupportedAgent(unsupported.to_string())),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetPlatform {
    Windows,
    MacOS,
    Linux,
}

impl TargetPlatform {
    pub const CURRENT: TargetPlatform = if cfg!(windows) {
        TargetPlatform::Windows
    } else if cfg!(target_os = "macos") {
        TargetPlatform::MacOS
    } else {
        TargetPlatform::Linux
    };

    #[allow(non_upper_case_globals)]
    pub const Current: TargetPlatform = Self::CURRENT;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellCommandPlan {
    pub program: String,
    pub args: Vec<String>,
}

pub fn resolve_shell_command_pure<P, E>(
    preference: Option<&str>,
    platform: TargetPlatform,
    is_executable_on_path: P,
    get_env: E,
) -> ShellCommandPlan
where
    P: Fn(&str) -> bool,
    E: Fn(&str) -> Option<String>,
{
    let clean_pref = preference
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("default"));

    match platform {
        TargetPlatform::Windows => match clean_pref {
            Some("pwsh") | Some("pwsh.exe") => ShellCommandPlan {
                program: "pwsh.exe".to_string(),
                args: Vec::new(),
            },
            Some("powershell") | Some("powershell.exe") => ShellCommandPlan {
                program: "powershell.exe".to_string(),
                args: Vec::new(),
            },
            Some("cmd") | Some("cmd.exe") => ShellCommandPlan {
                program: "cmd.exe".to_string(),
                args: Vec::new(),
            },
            Some("wsl") | Some("wsl.exe") => ShellCommandPlan {
                program: "wsl.exe".to_string(),
                args: Vec::new(),
            },
            Some(custom) => ShellCommandPlan {
                program: custom.to_string(),
                args: Vec::new(),
            },
            None => {
                let program = if is_executable_on_path("pwsh.exe") || is_executable_on_path("pwsh")
                {
                    "pwsh.exe"
                } else {
                    "powershell.exe"
                };
                ShellCommandPlan {
                    program: program.to_string(),
                    args: Vec::new(),
                }
            }
        },
        TargetPlatform::MacOS => match clean_pref {
            Some(custom) => ShellCommandPlan {
                program: custom.to_string(),
                args: Vec::new(),
            },
            None => {
                let program = get_env("SHELL")
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "/bin/zsh".to_string());
                ShellCommandPlan {
                    program,
                    // Login shells source ~/.zprofile, where Homebrew's brew shellenv and
                    // similar PATH setup live. Without this, GUI-launched daemons pass a
                    // minimal PATH and panes lose /opt/homebrew/bin entirely (starship,
                    // brew, etc. "command not found"). Matches Ghostty's login default.
                    args: vec!["-l".to_string()],
                }
            }
        },
        TargetPlatform::Linux => match clean_pref {
            Some(custom) => ShellCommandPlan {
                program: custom.to_string(),
                args: Vec::new(),
            },
            None => {
                let program = get_env("SHELL")
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "/bin/bash".to_string());
                ShellCommandPlan {
                    program,
                    args: vec!["-l".to_string()],
                }
            }
        },
    }
}

fn is_on_path(exe: &str) -> bool {
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let full = dir.join(exe);
            if full.is_file() {
                return true;
            }
        }
    }
    false
}

pub fn resolve_startup_command_pure<P, E>(
    preference: Option<&str>,
    startup: Option<&TerminalStartup>,
    platform: TargetPlatform,
    is_executable_on_path: P,
    get_env: E,
) -> Result<ShellCommandPlan, AgentResumeError>
where
    P: Fn(&str) -> bool,
    E: Fn(&str) -> Option<String>,
{
    match startup {
        Some(TerminalStartup::AgentResume {
            agent_type,
            provider_session,
        }) => resolve_agent_resume_plan(agent_type, provider_session),
        None => Ok(resolve_shell_command_pure(
            preference,
            platform,
            is_executable_on_path,
            get_env,
        )),
    }
}

pub fn resolve_startup_command(
    preference: Option<&str>,
    startup: Option<&TerminalStartup>,
) -> Result<CommandBuilder, AgentResumeError> {
    let plan = resolve_startup_command_pure(
        preference,
        startup,
        TargetPlatform::CURRENT,
        is_on_path,
        |var| std::env::var(var).ok(),
    )?;
    let mut cmd = CommandBuilder::new(&plan.program);
    for arg in &plan.args {
        cmd.arg(arg);
    }
    Ok(cmd)
}

pub fn resolve_shell_command(preference: Option<&str>) -> CommandBuilder {
    let plan = resolve_shell_command_pure(preference, TargetPlatform::CURRENT, is_on_path, |var| {
        std::env::var(var).ok()
    });
    let mut cmd = CommandBuilder::new(&plan.program);
    for arg in &plan.args {
        cmd.arg(arg);
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_windows_default_pwsh_present() {
        let plan = resolve_shell_command_pure(
            None,
            TargetPlatform::Windows,
            |exe| exe == "pwsh.exe",
            |_| None,
        );
        assert_eq!(
            plan,
            ShellCommandPlan {
                program: "pwsh.exe".to_string(),
                args: vec![],
            }
        );
    }

    #[test]
    fn test_windows_default_pwsh_absent() {
        let plan = resolve_shell_command_pure(None, TargetPlatform::Windows, |_| false, |_| None);
        assert_eq!(
            plan,
            ShellCommandPlan {
                program: "powershell.exe".to_string(),
                args: vec![],
            }
        );
    }

    #[test]
    fn test_windows_explicit_choices() {
        for (pref, expected) in [
            ("pwsh", "pwsh.exe"),
            ("powershell", "powershell.exe"),
            ("cmd", "cmd.exe"),
            ("wsl", "wsl.exe"),
        ] {
            let plan = resolve_shell_command_pure(
                Some(pref),
                TargetPlatform::Windows,
                |_| false,
                |_| None,
            );
            assert_eq!(
                plan,
                ShellCommandPlan {
                    program: expected.to_string(),
                    args: vec![],
                },
                "failed for preference: {}",
                pref
            );
        }
    }

    #[test]
    fn test_custom_path_passthrough() {
        let custom_win = "C:\\Tools\\nu.exe";
        let plan_win = resolve_shell_command_pure(
            Some(custom_win),
            TargetPlatform::Windows,
            |_| false,
            |_| None,
        );
        assert_eq!(
            plan_win,
            ShellCommandPlan {
                program: custom_win.to_string(),
                args: vec![],
            }
        );

        let custom_unix = "/usr/local/bin/fish";
        let plan_mac = resolve_shell_command_pure(
            Some(custom_unix),
            TargetPlatform::MacOS,
            |_| false,
            |_| None,
        );
        assert_eq!(
            plan_mac,
            ShellCommandPlan {
                program: custom_unix.to_string(),
                args: vec![],
            }
        );
    }

    #[test]
    fn test_macos_default_with_shell_env() {
        let plan = resolve_shell_command_pure(
            None,
            TargetPlatform::MacOS,
            |_| false,
            |var| {
                if var == "SHELL" {
                    Some("/opt/homebrew/bin/fish".to_string())
                } else {
                    None
                }
            },
        );
        assert_eq!(
            plan,
            ShellCommandPlan {
                program: "/opt/homebrew/bin/fish".to_string(),
                args: vec!["-l".to_string()],
            }
        );
    }

    #[test]
    fn test_macos_default_without_shell_env() {
        let plan = resolve_shell_command_pure(None, TargetPlatform::MacOS, |_| false, |_| None);
        assert_eq!(
            plan,
            ShellCommandPlan {
                program: "/bin/zsh".to_string(),
                args: vec!["-l".to_string()],
            }
        );
    }

    #[test]
    fn test_custom_shell_keeps_no_login_flag() {
        let plan = resolve_shell_command_pure(
            Some("/usr/local/bin/nu"),
            TargetPlatform::MacOS,
            |_| true,
            |_| None,
        );
        assert_eq!(
            plan,
            ShellCommandPlan {
                program: "/bin/zsh".to_string(),
                args: vec!["-l".to_string()],
            }
        );
    }

    #[test]
    fn test_custom_shell_keeps_no_login_flag() {
        let plan = resolve_shell_command_pure(
            Some("/usr/local/bin/nu"),
            TargetPlatform::MacOS,
            |_| true,
            |_| None,
        );
        assert_eq!(
            plan,
            ShellCommandPlan {
                program: "/usr/local/bin/nu".to_string(),
                args: vec![],
            }
        );
    }

    #[test]
    fn test_linux_default() {
        let with_env = resolve_shell_command_pure(
            None,
            TargetPlatform::Linux,
            |_| false,
            |var| {
                if var == "SHELL" {
                    Some("/bin/zsh".into())
                } else {
                    None
                }
            },
        );
        assert_eq!(
            with_env,
            ShellCommandPlan {
                program: "/bin/zsh".to_string(),
                args: vec!["-l".to_string()],
            }
        );

        let without_env =
            resolve_shell_command_pure(None, TargetPlatform::Linux, |_| false, |_| None);
        assert_eq!(
            without_env,
            ShellCommandPlan {
                program: "/bin/bash".to_string(),
                args: vec!["-l".to_string()],
            }
        );
    }

    #[test]
    fn test_empty_and_whitespace_preference_treated_as_default() {
        for pref in [
            Some(""),
            Some("   "),
            Some("\t\n"),
            Some("default"),
            Some("DEFAULT"),
        ] {
            let plan_mac =
                resolve_shell_command_pure(pref, TargetPlatform::MacOS, |_| false, |_| None);
            assert_eq!(
                plan_mac,
                ShellCommandPlan {
                    program: "/bin/zsh".to_string(),
                    args: vec!["-l".to_string()],
                },
                "failed for macos pref: {:?}",
                pref
            );

            let plan_win = resolve_shell_command_pure(
                pref,
                TargetPlatform::Windows,
                |exe| exe == "pwsh.exe",
                |_| None,
            );
            assert_eq!(
                plan_win,
                ShellCommandPlan {
                    program: "pwsh.exe".to_string(),
                    args: vec![],
                },
                "failed for windows pref: {:?}",
                pref
            );
        }
    }

    #[test]
    fn test_resolve_agent_resume_all_supported_mappings() {
        let cases = [
            (
                "claude",
                AgentProviderSessionKey::SessionId,
                "sess-1",
                None,
                "claude",
                vec!["--resume", "sess-1"],
            ),
            (
                "codex",
                AgentProviderSessionKey::SessionId,
                "sess-2",
                None,
                "codex",
                vec!["resume", "sess-2"],
            ),
            (
                "gemini",
                AgentProviderSessionKey::SessionId,
                "sess-3",
                None,
                "gemini",
                vec!["--resume", "sess-3"],
            ),
            (
                "antigravity",
                AgentProviderSessionKey::ConversationId,
                "conv-4",
                None,
                "agy",
                vec!["--conversation", "conv-4"],
            ),
            (
                "opencode",
                AgentProviderSessionKey::SessionId,
                "sess-5",
                None,
                "opencode",
                vec!["--session", "sess-5"],
            ),
            (
                "pi",
                AgentProviderSessionKey::SessionId,
                "sess-6",
                Some("/path/to/pi.json"),
                "pi",
                vec!["--session", "/path/to/pi.json"],
            ),
            (
                "prime-agent",
                AgentProviderSessionKey::SessionId,
                "sess-7",
                Some("/path/to/prime.json"),
                "prime-agent",
                vec!["--resume", "/path/to/prime.json"],
            ),
            (
                "mimo-code",
                AgentProviderSessionKey::SessionId,
                "sess-8",
                None,
                "mimo",
                vec!["--session", "sess-8"],
            ),
            (
                "droid",
                AgentProviderSessionKey::SessionId,
                "sess-9",
                None,
                "droid",
                vec!["--resume", "sess-9"],
            ),
            (
                "grok",
                AgentProviderSessionKey::SessionId,
                "sess-10",
                None,
                "grok",
                vec!["--resume", "sess-10"],
            ),
            (
                "devin",
                AgentProviderSessionKey::SessionId,
                "sess-11",
                None,
                "devin",
                vec!["--resume", "sess-11"],
            ),
            (
                "omp",
                AgentProviderSessionKey::SessionId,
                "sess-12",
                None,
                "omp",
                vec!["--resume", "sess-12"],
            ),
            (
                "omp",
                AgentProviderSessionKey::SessionId,
                "sess-12",
                Some("/custom/omp.json"),
                "omp",
                vec!["--resume", "/custom/omp.json"],
            ),
            (
                "omo",
                AgentProviderSessionKey::SessionId,
                "sess-13",
                None,
                "omo",
                vec!["--session", "sess-13"],
            ),
            (
                "kimi",
                AgentProviderSessionKey::SessionId,
                "sess-14",
                None,
                "kimi",
                vec!["--session", "sess-14"],
            ),
            (
                "gjc",
                AgentProviderSessionKey::SessionId,
                "1f9d2a6b9c0d1234",
                None,
                "gjc",
                vec!["--resume", "1f9d2a6b9c0d1234"],
            ),
            (
                "copilot",
                AgentProviderSessionKey::SessionId,
                "sess-15",
                None,
                "copilot",
                vec!["--resume", "sess-15"],
            ),
            (
                "cursor",
                AgentProviderSessionKey::SessionId,
                "sess-16",
                None,
                "cursor-agent",
                vec!["--resume", "sess-16"],
            ),
            (
                "cursor-agent",
                AgentProviderSessionKey::SessionId,
                "sess-17",
                None,
                "cursor-agent",
                vec!["--resume", "sess-17"],
            ),
        ];

        for (agent, key, id, transcript, expected_bin, expected_args) in cases {
            let session = AgentProviderSession {
                key,
                id: id.to_string(),
                transcript_path: transcript.map(|s| s.to_string()),
            };
            let plan = resolve_agent_resume_plan(agent, &session)
                .unwrap_or_else(|err| panic!("Failed to resolve {agent}: {err}"));
            assert_eq!(plan.program, expected_bin, "binary mismatch for {agent}");
            let expected_args_owned: Vec<String> =
                expected_args.into_iter().map(|s| s.to_string()).collect();
            assert_eq!(plan.args, expected_args_owned, "argv mismatch for {agent}");
        }
    }

    #[test]
    fn test_resolve_agent_resume_omo_never_uses_session_id_flag() {
        let session = AgentProviderSession {
            key: AgentProviderSessionKey::SessionId,
            id: "omo-sess-123".to_string(),
            transcript_path: None,
        };
        let plan = resolve_agent_resume_plan("omo", &session).expect("resolve omo");
        assert_eq!(plan.program, "omo");
        assert_eq!(
            plan.args,
            vec!["--session".to_string(), "omo-sess-123".to_string()]
        );
        assert!(!plan.args.iter().any(|a| a == "--session-id"));
    }

    #[test]
    fn test_agent_resume_validation_rejects_leading_dash() {
        let session = AgentProviderSession {
            key: AgentProviderSessionKey::SessionId,
            id: "--new".to_string(),
            transcript_path: None,
        };
        let err =
            resolve_agent_resume_plan("claude", &session).expect_err("must reject leading dash");
        assert!(matches!(err, AgentResumeError::InvalidSessionId(_)));
    }

    #[test]
    fn test_agent_resume_validates_bounded_absolute_transcript_paths() {
        assert_eq!(
            validate_transcript_path(Some("/tmp/run.json")).unwrap(),
            Some("/tmp/run.json".to_string())
        );
        assert_eq!(
            validate_transcript_path(Some("C:\\sessions\\run.json")).unwrap(),
            Some("C:\\sessions\\run.json".to_string())
        );
        for invalid in ["relative/run.json", "--session", "\u{7f}/tmp/run.json"] {
            assert!(
                validate_transcript_path(Some(invalid)).is_err(),
                "accepted {invalid:?}"
            );
        }
        let oversized = format!("/{}", "x".repeat(4096));
        assert!(validate_transcript_path(Some(&oversized)).is_err());
    }

    #[test]
    fn test_agent_resume_validation_rejects_latest() {
        for val in ["latest", "LATEST", "Latest", "  latest  "] {
            let session = AgentProviderSession {
                key: AgentProviderSessionKey::SessionId,
                id: val.to_string(),
                transcript_path: None,
            };
            let err =
                resolve_agent_resume_plan("claude", &session).expect_err("must reject latest");
            assert!(matches!(err, AgentResumeError::InvalidSessionId(_)));
        }
    }

    #[test]
    fn test_agent_resume_validation_rejects_control_characters() {
        let session = AgentProviderSession {
            key: AgentProviderSessionKey::SessionId,
            id: "session\x00id".to_string(),
            transcript_path: None,
        };
        let err =
            resolve_agent_resume_plan("claude", &session).expect_err("must reject control chars");
        assert!(matches!(err, AgentResumeError::InvalidSessionId(_)));

        let session2 = AgentProviderSession {
            key: AgentProviderSessionKey::SessionId,
            id: "session\x1b[31mid".to_string(),
            transcript_path: None,
        };
        let err2 =
            resolve_agent_resume_plan("claude", &session2).expect_err("must reject ESC char");
        assert!(matches!(err2, AgentResumeError::InvalidSessionId(_)));
    }

    #[test]
    fn test_agent_resume_validation_rejects_empty_or_too_long() {
        let session_empty = AgentProviderSession {
            key: AgentProviderSessionKey::SessionId,
            id: "   ".to_string(),
            transcript_path: None,
        };
        let err_empty =
            resolve_agent_resume_plan("claude", &session_empty).expect_err("must reject empty");
        assert!(matches!(err_empty, AgentResumeError::InvalidSessionId(_)));

        let long_id = "a".repeat(513);
        let session_long = AgentProviderSession {
            key: AgentProviderSessionKey::SessionId,
            id: long_id,
            transcript_path: None,
        };
        let err_long =
            resolve_agent_resume_plan("claude", &session_long).expect_err("must reject >512 chars");
        assert!(matches!(err_long, AgentResumeError::InvalidSessionId(_)));
    }

    #[test]
    fn test_agent_resume_validation_rejects_wrong_key() {
        let session_claude_wrong = AgentProviderSession {
            key: AgentProviderSessionKey::ConversationId,
            id: "sess-1".to_string(),
            transcript_path: None,
        };
        let err1 = resolve_agent_resume_plan("claude", &session_claude_wrong)
            .expect_err("claude requires session_id");
        assert!(matches!(err1, AgentResumeError::InvalidKey { .. }));

        let session_agy_wrong = AgentProviderSession {
            key: AgentProviderSessionKey::SessionId,
            id: "conv-1".to_string(),
            transcript_path: None,
        };
        let err2 = resolve_agent_resume_plan("antigravity", &session_agy_wrong)
            .expect_err("antigravity requires conversation_id");
        assert!(matches!(err2, AgentResumeError::InvalidKey { .. }));
    }

    #[test]
    fn test_agent_resume_validation_rejects_missing_transcript_for_pi_and_prime() {
        let session_pi_no_transcript = AgentProviderSession {
            key: AgentProviderSessionKey::SessionId,
            id: "pi-1".to_string(),
            transcript_path: None,
        };
        let err_pi = resolve_agent_resume_plan("pi", &session_pi_no_transcript)
            .expect_err("pi requires transcript");
        assert!(matches!(err_pi, AgentResumeError::MissingTranscriptPath(_)));

        let session_prime_no_transcript = AgentProviderSession {
            key: AgentProviderSessionKey::SessionId,
            id: "prime-1".to_string(),
            transcript_path: None,
        };
        let err_prime = resolve_agent_resume_plan("prime-agent", &session_prime_no_transcript)
            .expect_err("prime requires transcript");
        assert!(matches!(
            err_prime,
            AgentResumeError::MissingTranscriptPath(_)
        ));
    }

    #[test]
    fn test_agent_resume_validation_rejects_unsupported_agent() {
        let session = AgentProviderSession {
            key: AgentProviderSessionKey::SessionId,
            id: "sess-1".to_string(),
            transcript_path: None,
        };
        let err =
            resolve_agent_resume_plan("unsupported-llm", &session).expect_err("unsupported agent");
        assert!(matches!(err, AgentResumeError::UnsupportedAgent(_)));
    }

    #[test]
    fn test_qa_happy_claude_resume_creates_command_builder_without_executing() {
        let startup = TerminalStartup::AgentResume {
            agent_type: "claude".to_string(),
            provider_session: AgentProviderSession {
                key: AgentProviderSessionKey::SessionId,
                id: "sess-claude-xyz".to_string(),
                transcript_path: None,
            },
        };

        // Assert pure plan first
        let plan = resolve_startup_command_pure(
            None,
            Some(&startup),
            TargetPlatform::MacOS,
            |_| true,
            |_| None,
        )
        .expect("resolve claude startup plan");

        assert_eq!(plan.program, "claude");
        assert_eq!(
            plan.args,
            vec!["--resume".to_string(), "sess-claude-xyz".to_string()]
        );

        // Assert CommandBuilder construction without executing Claude
        let cmd =
            resolve_startup_command(None, Some(&startup)).expect("resolve startup command builder");
        let _ = cmd; // CommandBuilder constructed without executing the process
    }

    #[test]
    fn test_resolve_startup_command_pure_fallback_to_shell() {
        let plan =
            resolve_startup_command_pure(None, None, TargetPlatform::MacOS, |_| false, |_| None)
                .expect("shell startup fallback");
        assert_eq!(plan.program, "/bin/zsh");
        assert_eq!(plan.args, vec!["-l".to_string()]);
    }
}
