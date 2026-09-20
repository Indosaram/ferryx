//! Positive process evidence only. Sampling frequency is not a state timeout.
use super::PtySession;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Foreground {
    Agent(u32),
    Shell,
    Other,
}

/// The positive process evidence an observation carries about agent ownership of a PTY.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AgentProcessEdge {
    /// An agent process was observed where none was before: screen inference is meaningful again.
    Observed,
    /// The shell owns the foreground again after an agent ran: the agent's work is over.
    Released,
}

#[derive(Default)]
pub(crate) struct ProcessTransition {
    /// Set while an agent has been observed and no shell prompt has been seen since.
    agent_observed: bool,
}

impl ProcessTransition {
    pub(crate) fn observe(&mut self, current: Option<Foreground>) -> Option<AgentProcessEdge> {
        // Unknown ownership is not evidence in either direction.
        let current = current?;
        match current {
            Foreground::Agent(_) if !self.agent_observed => {
                self.agent_observed = true;
                Some(AgentProcessEdge::Observed)
            }
            Foreground::Agent(_) => None,
            // An agent rarely hands the terminal straight back to the prompt: it exits, the shell
            // runs whatever came next in the command line, and only then prompts. Releasing only
            // on an immediate agent -> shell edge therefore misses real exits and pins the pane
            // to its last activity forever. The shell owning the foreground is the evidence.
            Foreground::Shell if self.agent_observed => {
                self.agent_observed = false;
                Some(AgentProcessEdge::Released)
            }
            // A non-agent, non-shell foreground says nothing about whether the agent came back.
            Foreground::Shell | Foreground::Other => None,
        }
    }
}

fn is_agent(command: &str) -> bool {
    let mut parts = command.split_whitespace();
    let Some(program) = parts.next() else {
        return false;
    };
    let executable = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .trim_matches('"')
        .trim_end_matches(".exe");
    let candidate = if matches!(
        executable,
        "node" | "bun" | "python" | "python3" | "sh" | "bash"
    ) {
        // Interpreter entry points, not arbitrary arguments such as `echo omo`.
        parts.next().unwrap_or("")
    } else {
        program
    };
    {
        let part = candidate.trim_matches('"');
        let name = part.rsplit(['/', '\\']).next().unwrap_or(part);
        matches!(
            name.trim_end_matches(".exe").trim_end_matches(".js"),
            "claude"
                | "codex"
                | "omo"
                | "opencode"
                | "cursor-agent"
                | "agent"
                | "gjc"
                | "grok"
                | "kimi"
                | "copilot"
                | "cline"
                | "antigravity"
        )
    }
}

fn is_shell(command: &str) -> bool {
    let name = command
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim_start_matches('-');
    matches!(
        name.trim_end_matches(".exe"),
        "sh" | "bash"
            | "zsh"
            | "fish"
            | "dash"
            | "ksh"
            | "tcsh"
            | "csh"
            | "pwsh"
            | "powershell"
            | "cmd"
            | "nu"
    )
}

pub(crate) fn inspect(session: &PtySession) -> std::io::Result<Option<Foreground>> {
    platform::inspect(session)
}

#[cfg(unix)]
mod platform {
    use super::*;
    pub(super) fn inspect(session: &PtySession) -> std::io::Result<Option<Foreground>> {
        let Some(shell) = session.pid() else {
            return Ok(None);
        };
        let Some(group) = session.foreground_process_group()? else {
            return Ok(None);
        };
        // ps is available on both macOS and Linux. Do not assume /proc exists.
        let output = std::process::Command::new("ps")
            .args(["-axo", "pid=,ppid=,pgid=,args="])
            .output()?;
        if !output.status.success() {
            return Err(std::io::Error::other("foreground process snapshot failed"));
        }
        // Ownership can change while ps takes its snapshot. A mixed snapshot is
        // unknown, not an Other edge that would erase the observed agent.
        if session.foreground_process_group()? != Some(group) {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut shell_foreground = false;
        for line in text.lines() {
            let mut fields = line.split_whitespace();
            let pid = fields.next().and_then(|s| s.parse::<u32>().ok());
            let parent = fields.next().and_then(|s| s.parse::<u32>().ok());
            let pgid = fields.next().and_then(|s| s.parse::<u32>().ok());
            let command = fields.collect::<Vec<_>>().join(" ");
            if pgid == Some(group) && is_agent(&command) {
                return Ok(pid.map(Foreground::Agent));
            }
            // A suspended/background agent is still alive: do not call shell ownership exit.
            if parent == Some(shell) && is_agent(&command) {
                return Ok(pid.map(Foreground::Agent));
            }
            if pid == Some(shell) && pgid == Some(group) && is_shell(&command) {
                shell_foreground = true;
            }
        }
        Ok(Some(if shell_foreground {
            Foreground::Shell
        } else {
            Foreground::Other
        }))
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    #[derive(serde::Deserialize)]
    struct Process {
        #[serde(rename = "ProcessId")]
        pid: u32,
        #[serde(rename = "ParentProcessId")]
        parent: u32,
        #[serde(rename = "Name")]
        name: String,
        #[serde(rename = "CommandLine")]
        command: Option<String>,
    }
    pub(super) fn inspect(session: &PtySession) -> std::io::Result<Option<Foreground>> {
        let Some(shell) = session.pid() else {
            return Ok(None);
        };
        // ConPTY has no tcgetpgrp. A complete descendant snapshot supplies positive
        // child-exit evidence; shell-with-no-descendants is the conservative fallback.
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "ConvertTo-Json -Compress -InputObject @(Get-CimInstance Win32_Process -ErrorAction Stop | Select-Object ProcessId,ParentProcessId,Name,CommandLine)"])
            .output()?;
        if !output.status.success() {
            return Err(std::io::Error::other("ConPTY process snapshot failed"));
        }
        let processes: Vec<Process> =
            serde_json::from_slice(&output.stdout).map_err(std::io::Error::other)?;
        if !processes
            .iter()
            .any(|p| p.pid == shell && is_shell(&p.name))
        {
            return Ok(None);
        }
        let mut descendants = vec![shell];
        loop {
            let before = descendants.len();
            for p in &processes {
                if descendants.contains(&p.parent) && !descendants.contains(&p.pid) {
                    descendants.push(p.pid);
                }
            }
            if descendants.len() == before {
                break;
            }
        }
        for p in &processes {
            if p.pid != shell
                && descendants.contains(&p.pid)
                && (is_agent(&p.name) || p.command.as_deref().is_some_and(is_agent))
            {
                return Ok(Some(Foreground::Agent(p.pid)));
            }
        }
        Ok(Some(if descendants.len() == 1 {
            Foreground::Shell
        } else {
            Foreground::Other
        }))
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    use super::*;
    pub(super) fn inspect(_: &PtySession) -> std::io::Result<Option<Foreground>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn real_pty_agent_exit_releases_without_further_agent_output() {
        use crate::daemon::agent_state::{AgentState, AgentStateHub};
        use portable_pty::CommandBuilder;
        use std::sync::Arc;
        use std::time::Duration;

        async fn output_until(rx: &mut tokio::sync::mpsc::Receiver<Vec<u8>>, marker: &str) {
            tokio::time::timeout(Duration::from_secs(5), async {
                let mut output = Vec::new();
                while let Some(chunk) = rx.recv().await {
                    output.extend(chunk);
                    if String::from_utf8_lossy(&output).contains(marker) {
                        return;
                    }
                }
                panic!("PTY closed before {marker}");
            })
            .await
            .expect("PTY protocol marker");
        }

        let (directory, script) = crate::ipc::run_blocking(|| {
            let directory = tempfile::tempdir().unwrap();
            let script = directory.path().join("omo");
            std::fs::write(&script, "printf 'AGENT_READY>'\nread answer\n").unwrap();
            Ok((directory, script))
        })
        .await
        .unwrap();
        let manager = crate::terminal::PtyManager::new();
        let mut command = CommandBuilder::new("/bin/bash");
        command.args(["--noprofile", "--norc", "-i"]);
        let (id, mut output) = manager.spawn(command, 80, 24).unwrap();
        let session = manager.get_session(&id).unwrap();
        // The output receiver exists before any action. Each marker is a protocol
        // handshake, not a timing delay; the agent then blocks silently in read.
        session
            .write_input(b"stty -echo; PS1='SHELL_READY>'\n")
            .unwrap();
        output_until(&mut output, "SHELL_READY>").await;
        session
            .write_input(format!("/bin/sh '{}'\n", script.display()).as_bytes())
            .unwrap();
        output_until(&mut output, "AGENT_READY>").await;
        let hub = Arc::new(AgentStateHub::default());
        hub.publish_canonical(AgentState {
            session_id: id.clone(),
            state: "working".into(),
            agent: Some("omo".into()),
            provider_session: None,
            origin: crate::daemon::protocol::AgentStateOrigin::Agent,
        });
        let mut states = hub.subscribe(&id);
        let mut transition = ProcessTransition::default();
        for _ in 0..2 {
            let session = Arc::clone(&session);
            let observed = crate::ipc::run_blocking(move || Ok(inspect(&session).unwrap()))
                .await
                .unwrap();
            assert!(
                matches!(observed, Some(Foreground::Agent(_))),
                "{observed:?}"
            );
            assert_ne!(
                transition.observe(observed),
                Some(AgentProcessEdge::Released)
            );
        }
        assert!(matches!(
            states.receiver.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
        session.write_input(b"done\n").unwrap();
        output_until(&mut output, "SHELL_READY>").await;
        let inspect_session = Arc::clone(&session);
        let observed = crate::ipc::run_blocking(move || Ok(inspect(&inspect_session).unwrap()))
            .await
            .unwrap();
        assert_eq!(observed, Some(Foreground::Shell));
        assert_eq!(
            transition.observe(observed),
            Some(AgentProcessEdge::Released)
        );
        hub.release_foreground(&id);
        let released = tokio::time::timeout(Duration::from_secs(1), states.receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(released.state.state, "idle");
        session.write_input(b"exit\n").unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while output.recv().await.is_some() {}
        })
        .await
        .unwrap();
        drop(directory);
    }

    #[test]
    fn agent_identity_uses_executable_or_interpreter_entry_point() {
        assert!(is_agent("/usr/bin/node /opt/omo.js"));
        assert!(is_agent("/bin/sh /tmp/omo"));
        assert!(is_agent("codex --resume"));
        assert!(!is_agent("echo omo"));
        assert!(!is_agent("/bin/sh -c omo"));
        assert!(!is_agent("ps -axo pid=,args="));
    }

    #[test]
    fn agent_identity_handles_windows_shaped_commands() {
        // The ConPTY path has no tcgetpgrp, so classification is its ONLY evidence:
        // a regression in separator, .exe or quote handling would silently disable
        // Windows detection with every Unix test still green.
        assert!(is_agent(r"C:\tools\codex.exe --resume"));
        assert!(is_agent(r"C:\tools\node.exe C:\agents\omo.js"));
        assert!(is_agent(r"bun.exe C:\a\claude.js"));
        assert!(is_agent("\"C:\\tools\\codex.exe\" --resume"));
        // Windows shells are not agents, and their arguments must not be promoted.
        assert!(!is_agent(r"C:\Windows\System32\cmd.exe"));
        assert!(!is_agent("cmd.exe /c omo"));
        assert!(!is_agent(
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"
        ));

        assert!(is_shell("powershell.exe"));
        assert!(is_shell("pwsh.exe"));
        assert!(is_shell(r"C:\Windows\System32\cmd.exe"));
        assert!(!is_shell(r"C:\tools\codex.exe"));
    }

    #[test]
    fn unknown_and_unrelated_processes_do_not_release() {
        let mut observer = ProcessTransition::default();
        assert_eq!(observer.observe(Some(Foreground::Shell)), None);
        assert_eq!(
            observer.observe(Some(Foreground::Agent(42))),
            Some(AgentProcessEdge::Observed)
        );
        assert_eq!(observer.observe(None), None);
        assert_eq!(observer.observe(Some(Foreground::Other)), None);
        assert_eq!(
            observer.observe(Some(Foreground::Shell)),
            Some(AgentProcessEdge::Released),
            "an unknown or unrelated foreground must not consume the pending agent exit"
        );
    }

    #[test]
    fn agent_exit_through_an_unrelated_command_still_releases() {
        // `omo && ls` and `omo; git status` are ordinary usage: the agent exits, the shell runs
        // the next command, and only then prompts. Requiring an immediate agent -> shell edge
        // silently drops the exit and the pane stays "working" with no agent alive.
        let mut observer = ProcessTransition::default();
        assert_eq!(
            observer.observe(Some(Foreground::Agent(42))),
            Some(AgentProcessEdge::Observed)
        );
        assert_eq!(observer.observe(Some(Foreground::Other)), None);
        assert_eq!(
            observer.observe(Some(Foreground::Shell)),
            Some(AgentProcessEdge::Released)
        );
        assert_eq!(
            observer.observe(Some(Foreground::Shell)),
            None,
            "one release per exit"
        );
    }

    #[test]
    fn a_new_agent_after_a_release_is_reported_as_fresh_evidence() {
        // Screen inference is disarmed while the daemon owns the release; the next agent launch
        // is what re-arms it, so the edge has to be observable.
        let mut observer = ProcessTransition::default();
        assert_eq!(
            observer.observe(Some(Foreground::Agent(7))),
            Some(AgentProcessEdge::Observed)
        );
        assert_eq!(
            observer.observe(Some(Foreground::Agent(7))),
            None,
            "a still-running agent is not a new edge"
        );
        assert_eq!(
            observer.observe(Some(Foreground::Shell)),
            Some(AgentProcessEdge::Released)
        );
        assert_eq!(
            observer.observe(Some(Foreground::Agent(9))),
            Some(AgentProcessEdge::Observed)
        );
    }
}
