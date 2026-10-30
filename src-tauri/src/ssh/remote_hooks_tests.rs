use super::*;

const HELPER: &str = "/home/u/.ferryx/versions/2026.1017.1/bin/ferryx-remote-helper";

fn reporter() -> Reporter {
    Reporter { exe: HELPER.into(), windows: false }
}

fn windows_reporter() -> Reporter {
    Reporter { exe: r"C:\Users\u\.ferryx\bin\ferryx-remote-helper.exe".into(), windows: true }
}

#[cfg(unix)]
mod posix {
    use super::*;
    use serde_json::{json, Value};
    use std::io::Write;
    use std::path::Path;
    use std::process::{Command, Stdio};

    /// Each agent's directory on a POSIX host, relative to HOME (crush under the default XDG root).
    fn agent_dir(agent: &str) -> &'static str {
        HOOK_FILES.iter().find(|(name, ..)| *name == agent).map(|(_, dir, ..)| *dir).unwrap()
    }

    fn file_of(home: &Path, agent: &str) -> std::path::PathBuf {
        let (_, dir, file, _) = HOOK_FILES.iter().find(|(name, ..)| *name == agent).unwrap();
        home.join(dir).join(file)
    }

    /// Runs each step the way `run_integration_script` and the read path do on a POSIX host:
    /// `sh -c` with HOME set, the step's bytes on stdin.
    async fn run(home: &Path, step: Step) -> Result<Vec<u8>, IpcError> {
        let (script, input) = match step {
            Step::Read(script) => (script, Vec::new()),
            Step::Write(script, bytes) => (script, bytes),
        };
        let mut child = Command::new("sh")
            .args(["-c", &script])
            .env("HOME", home)
            .env("TMPDIR", home)
            .env_remove("XDG_CONFIG_HOME")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&input).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{script}\n{}", String::from_utf8_lossy(&output.stderr));
        Ok(output.stdout)
    }

    async fn install(home: &Path) -> usize {
        install_with(RemotePlatform::Posix, &reporter(), |step| run(home, step)).await.unwrap()
    }

    async fn remove(home: &Path) -> RemoteRemovalReport {
        let mut report = RemoteRemovalReport::default();
        remove_with(RemotePlatform::Posix, &mut report, |step| run(home, step)).await.unwrap();
        report
    }

    fn home() -> tempfile::TempDir {
        tempfile::Builder::new().prefix("ferryx-remote-hooks-").tempdir().unwrap()
    }

    fn make_all_agent_dirs(home: &Path) {
        for (agent, _) in HOOK_AGENTS {
            std::fs::create_dir_all(home.join(agent_dir(agent))).unwrap();
        }
    }

    fn assert_no_staging_left(home: &Path) {
        for entry in walk(home) {
            let name = entry.file_name().unwrap().to_string_lossy().into_owned();
            assert!(!name.starts_with(".ferryx-hook") && !name.contains(".ferryx.tmp"), "leftover {name}");
        }
    }

    fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                out.extend(walk(&path));
            }
            out.push(path);
        }
        out
    }

    /// Each agent's foreign config, in the shape measured for it and in the bytes Ferryx renders,
    /// so a removal must give back the very same bytes.
    fn foreign(agent: &str) -> Option<Value> {
        Some(match agent {
            "claude" | "codex" => json!({
                "model": "opus",
                "hooks": {
                    "Stop": [{"hooks": [{"type": "command", "command": "orca-hook stop"}]}],
                    "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "./guard.sh"}]}]
                }
            }),
            "grok" => json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "mine"}]}]}}),
            "antigravity" => json!({
                "user-group": {"enabled": true, "Stop": [{"type": "command", "command": "notify.sh", "timeout": 5}]}
            }),
            "crush" => json!({
                "options": {"debug": true},
                "hooks": {"PreToolUse": [{"name": "guard", "matcher": "bash", "command": "./guard.sh", "timeout": 3}]}
            }),
            "cursor" => json!({"version": 1, "hooks": {"stop": [{"command": "./audit.sh"}]}}),
            _ => return None,
        })
    }

    fn rendered(value: &Value) -> String {
        format!("{}\n", serde_json::to_string_pretty(value).unwrap())
    }

    /// Every agent's hooks are installed on the host and point at the staged helper. Dropping one
    /// agent from the remote install (or from the shared tables it reads) fails here for that agent.
    #[tokio::test]
    async fn every_hook_agent_is_installed_on_the_host_with_the_helper_command() {
        let home = home();
        make_all_agent_dirs(home.path());
        assert!(install(home.path()).await > 0);
        for (agent, events) in HOOK_AGENTS {
            let target = file_of(home.path(), agent);
            for event in events.iter() {
                let command = reporter().command(agent, event);
                assert!(command.starts_with(&format!("'{HELPER}' agent-hook {agent} ")), "{command}");
                let text = if agent == "cline" {
                    let script = target.join(format!("{event}.sh"));
                    let mode = std::os::unix::fs::PermissionsExt::mode(
                        &std::fs::metadata(&script).unwrap().permissions(),
                    );
                    assert_eq!(mode & 0o777, 0o755, "{agent} {event}");
                    std::fs::read_to_string(script).unwrap()
                } else {
                    std::fs::read_to_string(&target).unwrap()
                };
                let needle = serde_json::to_string(&command).unwrap();
                assert!(
                    text.contains(&command) || text.contains(needle.trim_matches('"')),
                    "{agent} {event} has no helper command in {}",
                    target.display()
                );
            }
        }
        // A second install finds every file current and writes nothing.
        assert_eq!(install(home.path()).await, 0);
        assert_no_staging_left(home.path());
    }

    #[tokio::test]
    async fn nothing_is_written_for_an_agent_that_is_not_installed() {
        let home = home();
        assert_eq!(install(home.path()).await, 0);
        assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
        let report = remove(home.path()).await;
        assert!(report.removed.is_empty() && report.kept_modified.is_empty());
        assert_eq!(report.absent.len(), HOOK_AGENTS.len());
    }

    /// Per shape: the user's own entries survive the install untouched, and a removal gives back
    /// the file byte-for-byte.
    #[tokio::test]
    async fn foreign_entries_survive_and_removal_restores_the_exact_bytes() {
        let home = home();
        make_all_agent_dirs(home.path());
        let mut originals = Vec::new();
        for (agent, _) in HOOK_AGENTS {
            let Some(value) = foreign(agent) else { continue };
            let file = file_of(home.path(), agent);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, rendered(&value)).unwrap();
            originals.push((agent, file, value));
        }
        let user_script = file_of(home.path(), "cline").join("TaskStart.sh");
        std::fs::create_dir_all(user_script.parent().unwrap()).unwrap();
        std::fs::write(&user_script, "#!/bin/sh\necho mine\n").unwrap();

        install(home.path()).await;
        for (agent, file, value) in &originals {
            let installed: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
            assert_ne!(&installed, value, "{agent} got no entry");
            match *agent {
                "antigravity" => assert_eq!(installed["user-group"], value["user-group"]),
                "crush" => {
                    assert_eq!(installed["options"], value["options"]);
                    assert_eq!(installed["hooks"]["PreToolUse"][0], value["hooks"]["PreToolUse"][0]);
                }
                _ => {
                    for (event, groups) in value["hooks"].as_object().unwrap() {
                        assert_eq!(installed["hooks"][event][0], groups[0], "{agent} {event} stays first");
                    }
                }
            }
        }
        assert_eq!(std::fs::read_to_string(&user_script).unwrap(), "#!/bin/sh\necho mine\n");

        let report = remove(home.path()).await;
        for (agent, file, value) in &originals {
            assert_eq!(std::fs::read_to_string(file).unwrap(), rendered(value), "{agent}");
            assert!(report.removed.contains(&removal_label(agent)), "{agent}: {report:?}");
        }
        // Ferryx's own cline scripts go; the user's script for TaskStart stays.
        assert_eq!(std::fs::read_to_string(&user_script).unwrap(), "#!/bin/sh\necho mine\n");
        let left: Vec<_> = std::fs::read_dir(user_script.parent().unwrap()).unwrap().collect();
        assert_eq!(left.len(), 1);
        assert!(report.removed.contains(&removal_label("cline")));
        assert_no_staging_left(home.path());
    }

    /// A file changed on the host between the read and the write is left alone.
    #[tokio::test]
    async fn a_file_changed_after_the_read_is_not_overwritten() {
        let home = home();
        let file = home.path().join(".claude/settings.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "{}\n").unwrap();
        let target = hook_files(RemotePlatform::Posix).into_iter().find(|f| f.agent == "claude").unwrap();
        let (expect, change) = install_change(&target, &reporter(), Found::File(b"{}\n".to_vec())).unwrap();
        std::fs::write(&file, "{\"model\": \"edited\"}\n").unwrap();
        let output = run(home.path(), write_step(RemotePlatform::Posix, &target, expect, change)).await.unwrap();
        assert_eq!(write_result(&output).unwrap(), "changed");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{\"model\": \"edited\"}\n");
    }

    #[tokio::test]
    async fn a_symlinked_settings_file_is_written_through() {
        let home = home();
        std::fs::create_dir_all(home.path().join(".codex")).unwrap();
        let real = home.path().join("dotfiles.json");
        // The claude-shaped strip keeps an emptied `hooks` object, so start from one to compare bytes.
        std::fs::write(&real, rendered(&json!({"model": "o3", "hooks": {}}))).unwrap();
        std::os::unix::fs::symlink(&real, home.path().join(".codex/hooks.json")).unwrap();
        install(home.path()).await;
        assert!(std::fs::symlink_metadata(home.path().join(".codex/hooks.json")).unwrap().file_type().is_symlink());
        let installed: Value = serde_json::from_slice(&std::fs::read(&real).unwrap()).unwrap();
        assert!(installed["hooks"]["Stop"].is_array());
        remove(home.path()).await;
        assert_eq!(std::fs::read_to_string(&real).unwrap(), rendered(&json!({"model": "o3", "hooks": {}})));
    }

    #[tokio::test]
    async fn crush_follows_xdg_config_home_on_the_host() {
        let home = home();
        let xdg = home.path().join("xdg");
        std::fs::create_dir_all(xdg.join("crush")).unwrap();
        let file = hook_files(RemotePlatform::Posix).into_iter().find(|f| f.agent == "crush").unwrap();
        let script = format!("XDG_CONFIG_HOME={}; {}", direct::quote_posix(&xdg.to_string_lossy()), read_script(RemotePlatform::Posix, &file));
        assert_eq!(parse_found(&run(home.path(), Step::Read(script)).await.unwrap()).unwrap(), Found::Absent);
    }

    /// The skill goes to `~/.claude/skills` and `~/.agents/skills` on the host; a removal deletes
    /// a managed one, and one the user took over (marker deleted) survives both.
    #[tokio::test]
    async fn the_skill_is_installed_on_the_host_and_a_users_copy_is_kept() {
        let home = home();
        make_all_agent_dirs(home.path());
        let claude = home.path().join(".claude/skills/ferryx/SKILL.md");
        let codex = home.path().join(".agents/skills/ferryx/SKILL.md");
        let mine = SKILL_SOURCE.replace("ferryx-managed: true\n", "");
        std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
        std::fs::write(&codex, &mine).unwrap();

        install(home.path()).await;
        assert_eq!(std::fs::read_to_string(&claude).unwrap(), SKILL_SOURCE);
        assert_eq!(std::fs::read_to_string(&codex).unwrap(), mine);
        remove(home.path()).await;
        assert!(!claude.exists());
        assert_eq!(std::fs::read_to_string(&codex).unwrap(), mine);
        assert_no_staging_left(home.path());
    }
}

#[test]
fn reading_a_file_returns_its_exact_bytes() {
    assert_eq!(parse_found(b"file\n{\n}\n").unwrap(), Found::File(b"{\n}\n".to_vec()));
    assert_eq!(parse_found(b"file\n").unwrap(), Found::File(Vec::new()));
    assert_eq!(parse_found(b"nodir\n").unwrap(), Found::NoDir);
    assert!(parse_found(b"garbage").is_err());
}

/// The Windows scripts locate the same files under `$homeRoot` and never use a POSIX tool.
#[test]
fn windows_scripts_locate_the_same_files() {
    let files = hook_files(RemotePlatform::Windows);
    let cline: Vec<_> = files.iter().filter(|f| f.agent == "cline").map(|f| f.rel.clone()).collect();
    assert!(cline.iter().all(|rel| rel.starts_with("hooks/") && rel.ends_with(".ps1")), "{cline:?}");
    for file in &files {
        let read = read_script(RemotePlatform::Windows, file);
        assert!(read.contains("$homeRoot"), "{read}");
        assert!(read.contains(&powershell_data(&file.rel.replace('/', "\\"))), "{read}");
        let write = write_script(RemotePlatform::Windows, file, &Expect::Absent, &Change::Delete);
        assert!(!write.contains("mktemp") && !write.contains("cmp -s"), "{write}");
    }
    let script = hook_script_for(&windows_reporter(), "cline", "TaskStart");
    assert!(script.ends_with("& \"C:\\Users\\u\\.ferryx\\bin\\ferryx-remote-helper.exe\" agent-hook cline TaskStart\n"), "{script}");
    assert_eq!(ferryx_script_event(&script, "cline"), Some("TaskStart"));
}

/// Entries the desktop wrote with the remote helper's path are recognised as Ferryx's, so a later
/// install replaces them and a removal deletes them.
#[test]
fn helper_commands_are_recognised_as_ferryx_entries() {
    for reporter in [reporter(), windows_reporter()] {
        let text = merged_settings_text(HookShape::ClaudeGroups, "claude", &["Stop"], &reporter, Some("{}"))
            .unwrap()
            .unwrap();
        assert_eq!(stripped_settings_text(HookShape::ClaudeGroups, "claude", &text).unwrap().as_deref(), Some("{\n  \"hooks\": {}\n}\n"));
    }
    let other = Reporter { exe: "/usr/bin/not-ferryx".into(), windows: false };
    let text = merged_settings_text(HookShape::ClaudeGroups, "claude", &["Stop"], &other, None).unwrap().unwrap();
    assert_eq!(stripped_settings_text(HookShape::ClaudeGroups, "claude", &text).unwrap(), None);
}

/// Hooks are installed only for a staged helper whose own `--capabilities` lists `agentHookV1`; an
/// older build at the same version path, or garbage output, gets none.
#[test]
fn only_a_helper_advertising_agent_hook_gets_remote_hooks() {
    let current = crate::ssh::helper_runtime::process::capabilities_report();
    assert!(advertises_agent_hook(format!("{current}\n").as_bytes()));
    let older = r#"{"protocol":1,"helperVersion":"2026.1017.1","capabilities":["sshHelperV1","dagStreamingV1","dagSubscribeV1","agentStateV1"]}"#;
    assert!(!advertises_agent_hook(older.as_bytes()));
    assert!(!advertises_agent_hook(b"usage: ferryx-remote-helper\n"));
    assert!(!advertises_agent_hook(b""));
}
