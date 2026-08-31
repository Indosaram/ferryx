use portable_pty::CommandBuilder;

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
                let program = if is_executable_on_path("pwsh.exe") || is_executable_on_path("pwsh") {
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

pub fn resolve_shell_command(preference: Option<&str>) -> CommandBuilder {
    let plan = resolve_shell_command_pure(
        preference,
        TargetPlatform::CURRENT,
        is_on_path,
        |var| std::env::var(var).ok(),
    );
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
        let plan = resolve_shell_command_pure(
            None,
            TargetPlatform::Windows,
            |_| false,
            |_| None,
        );
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
        let plan = resolve_shell_command_pure(
            None,
            TargetPlatform::MacOS,
            |_| false,
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
            |var| if var == "SHELL" { Some("/bin/zsh".into()) } else { None },
        );
        assert_eq!(
            with_env,
            ShellCommandPlan {
                program: "/bin/zsh".to_string(),
                args: vec!["-l".to_string()],
            }
        );

        let without_env = resolve_shell_command_pure(
            None,
            TargetPlatform::Linux,
            |_| false,
            |_| None,
        );
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
        for pref in [Some(""), Some("   "), Some("\t\n"), Some("default"), Some("DEFAULT")] {
            let plan_mac = resolve_shell_command_pure(
                pref,
                TargetPlatform::MacOS,
                |_| false,
                |_| None,
            );
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
}
