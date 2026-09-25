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

/// Splits a command line into tokens, honoring double quotes.
///
/// A Windows command line is quote-delimited whenever a path contains a space, and
/// `split_whitespace` then hands back fragments (`"C:\Program`, `Files\nodejs\node.exe"`)
/// that name neither an interpreter nor an agent. Backslashes are path separators here,
/// never escapes, so only the quote character is special.
fn command_tokens(command: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = None;
    let mut quoted = false;
    for (index, character) in command.char_indices() {
        match character {
            '"' => {
                quoted = !quoted;
                if start.is_none() {
                    start = Some(index);
                }
            }
            ' ' | '\t' if !quoted => {
                if let Some(begin) = start.take() {
                    tokens.push(&command[begin..index]);
                }
            }
            _ => {
                if start.is_none() {
                    start = Some(index);
                }
            }
        }
    }
    if let Some(begin) = start {
        tokens.push(&command[begin..]);
    }
    tokens
}

/// Strips a trailing `suffix` whatever its casing, still returning a slice of the input.
///
/// Windows command lines and `Win32_Process.Name` values carry whatever casing the install
/// used (`Claude.EXE`), so a case-sensitive strip leaves the suffix on the name and the name
/// lists below stop matching. No allocation, so this stays free on the per-tick path.
fn strip_suffix_ignore_ascii_case<'a>(value: &'a str, suffix: &str) -> &'a str {
    let mut value = value;
    while let Some(start) = value.len().checked_sub(suffix.len()) {
        match value.get(start..) {
            Some(tail) if tail.eq_ignore_ascii_case(suffix) => value = &value[..start],
            _ => break,
        }
    }
    value
}

/// Membership in a literal name list, compared without regard to case. The interpreter list
/// and the agent list both go through here, so the rule lives in one place.
fn is_named(name: &str, known: &[&str]) -> bool {
    known.iter().any(|candidate| name.eq_ignore_ascii_case(candidate))
}

/// The final name component of a token: surrounding quotes, either path separator and a
/// trailing `.exe` in any casing all normalize away.
fn executable_name(token: &str) -> &str {
    let name = token
        .trim_matches('"')
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("");
    strip_suffix_ignore_ascii_case(name, ".exe")
}

/// The name an agent is recognized by: [`executable_name`] plus an interpreter's `.js`
/// entry point, again in any casing.
fn agent_name(token: &str) -> &str {
    strip_suffix_ignore_ascii_case(executable_name(token), ".js")
}

/// The trailing whitespace-delimited segment of a command line: where an interpreter's entry
/// point still sits when the argument list itself left no token behind.
fn last_segment(command: &str) -> Option<&str> {
    command.rsplit([' ', '\t']).next()
}

/// Interpreters whose entry point, not the binary itself, names the agent.
const INTERPRETERS: [&str; 6] = ["node", "bun", "python", "python3", "sh", "bash"];

/// Agent executable or entry-point names, lowercase because the comparison ignores case.
const AGENT_NAMES: [&str; 12] = [
    "claude",
    "codex",
    "omo",
    "opencode",
    "cursor-agent",
    "agent",
    "gjc",
    "grok",
    "kimi",
    "copilot",
    "cline",
    "antigravity",
];

fn is_agent(command: &str) -> bool {
    let tokens = command_tokens(command);
    let Some(program) = tokens.first().copied() else {
        return false;
    };
    let candidate = if is_named(executable_name(program), &INTERPRETERS) {
        // Interpreter entry points, not arbitrary arguments such as `echo omo`. The script is
        // the argument that follows the program; a command line that carries none still ends
        // in its entry point, so the last path segment of the line is the fallback.
        tokens
            .get(1)
            .copied()
            .or_else(|| last_segment(command))
            .unwrap_or("")
    } else {
        program
    };
    is_named(agent_name(candidate), &AGENT_NAMES)
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

/// One process listing shared by every session a tick observes.
///
/// A Windows observation is decided from the whole `Win32_Process` table, and taking that
/// sample is one `powershell.exe` + `Get-CimInstance` enumeration costing hundreds of
/// milliseconds. The daemon observes every live session once per tick, so a per-session
/// sample multiplies that cost by the pane count: capture once, then hand the same snapshot
/// to every [`inspect_with_snapshot`] call in the tick.
pub(crate) struct ProcessSnapshot {
    /// Unix decides ownership from the session's own foreground process group, so it shares
    /// nothing between sessions and this listing stays empty there.
    #[cfg(windows)]
    processes: Vec<platform::Process>,
}

/// Capture the process listing one tick's observations share.
///
/// A failed capture is unknown ownership for every session in that tick: report no
/// foreground evidence rather than an absent agent.
pub(crate) fn capture_process_snapshot() -> std::io::Result<ProcessSnapshot> {
    #[cfg(windows)]
    {
        Ok(ProcessSnapshot {
            processes: platform::sample()?,
        })
    }
    #[cfg(not(windows))]
    {
        Ok(ProcessSnapshot {})
    }
}

pub(crate) fn inspect(session: &PtySession) -> std::io::Result<Option<Foreground>> {
    inspect_with_snapshot(session, &capture_process_snapshot()?)
}

/// Observe one session against a process listing already captured for this tick.
pub(crate) fn inspect_with_snapshot(
    session: &PtySession,
    snapshot: &ProcessSnapshot,
) -> std::io::Result<Option<Foreground>> {
    platform::inspect_with_snapshot(session, snapshot)
}

#[cfg(unix)]
mod platform {
    use super::*;
    pub(super) fn inspect_with_snapshot(
        session: &PtySession,
        // Ownership comes from this session's own foreground process group, so the tick's
        // shared listing is unused here and `ps` stays per-session as it always was.
        _: &ProcessSnapshot,
    ) -> std::io::Result<Option<Foreground>> {
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
    pub(super) struct Process {
        #[serde(rename = "ProcessId")]
        pid: u32,
        #[serde(rename = "ParentProcessId")]
        parent: u32,
        #[serde(rename = "Name")]
        name: String,
        #[serde(rename = "CommandLine")]
        command: Option<String>,
    }

    /// The complete `Win32_Process` listing. ConPTY has no tcgetpgrp, so ownership is decided
    /// from the whole process table; taking that sample is one `powershell.exe` launch, which
    /// is why a tick takes it once and shares it with every session it observes.
    pub(super) fn sample() -> std::io::Result<Vec<Process>> {
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "ConvertTo-Json -Compress -InputObject @(Get-CimInstance Win32_Process -ErrorAction Stop | Select-Object ProcessId,ParentProcessId,Name,CommandLine)"])
            .output()?;
        if !output.status.success() {
            return Err(std::io::Error::other("ConPTY process snapshot failed"));
        }
        serde_json::from_slice(&output.stdout).map_err(std::io::Error::other)
    }

    pub(super) fn inspect_with_snapshot(
        session: &PtySession,
        snapshot: &ProcessSnapshot,
    ) -> std::io::Result<Option<Foreground>> {
        let Some(shell) = session.pid() else {
            return Ok(None);
        };
        // A complete descendant snapshot supplies positive child-exit evidence;
        // shell-with-no-descendants is the conservative fallback.
        if !snapshot
            .processes
            .iter()
            .any(|p| p.pid == shell && is_shell(&p.name))
        {
            return Ok(None);
        }
        let mut descendants = vec![shell];
        loop {
            let before = descendants.len();
            for p in &snapshot.processes {
                if descendants.contains(&p.parent) && !descendants.contains(&p.pid) {
                    descendants.push(p.pid);
                }
            }
            if descendants.len() == before {
                break;
            }
        }
        for p in &snapshot.processes {
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
    pub(super) fn inspect_with_snapshot(
        _: &PtySession,
        _: &ProcessSnapshot,
    ) -> std::io::Result<Option<Foreground>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Waits for a marker the pane itself emits, so no assertion rests on a delay.
    #[cfg(unix)]
    async fn output_until(rx: &mut tokio::sync::mpsc::Receiver<Vec<u8>>, marker: &str) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
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

    #[cfg(unix)]
    #[tokio::test]
    async fn real_pty_agent_exit_releases_without_further_agent_output() {
        use crate::daemon::agent_state::{AgentState, AgentStateHub};
        use portable_pty::CommandBuilder;
        use std::sync::Arc;
        use std::time::Duration;

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
    fn executable_names_strip_the_suffix_whatever_its_casing() {
        // The pure helper every Windows classification rests on: a mixed-case `.EXE` has to
        // come off exactly as the lowercase one does, and the result stays a slice of the
        // token rather than an allocation on the per-tick path.
        assert_eq!(executable_name(r"C:\tools\Claude.EXE"), "Claude");
        assert_eq!(executable_name(r"C:\tools\claude.exe"), "claude");
        assert_eq!(executable_name(r#""C:\Program Files\Copilot.Exe""#), "Copilot");
        assert_eq!(executable_name(r"C:\Windows\System32\Notepad.EXE"), "Notepad");
        assert_eq!(executable_name("node.EXE"), "node");
        // The interpreter entry point is stripped the same way, so `OMO.JS` still names omo.
        assert_eq!(agent_name(r"C:\agents\OMO.JS"), "OMO");
        assert_eq!(agent_name(r"C:\agents\omo.js"), "omo");
    }

    #[test]
    fn agent_identity_ignores_windows_name_casing() {
        // Windows command lines and the `Win32_Process.Name` field carry whatever casing the
        // install used, so a case-sensitive suffix strip or name comparison silently disables
        // detection on those hosts while every Unix test stays green.
        assert!(is_agent(r"C:\tools\Claude.EXE --resume"));
        assert!(is_agent(r"C:\tools\CoDeX.eXe --resume"));
        assert!(is_agent(r"C:\Users\dev\AppData\Local\Programs\Copilot.Exe"));
        assert!(is_agent(r"C:\tools\NODE.EXE C:\agents\omo.js"));
        assert!(is_agent(r"C:\tools\NODE.EXE C:\agents\OMO.JS"));
        assert!(is_agent(r#""C:\Program Files\Claude\Claude.EXE" --resume"#));
        assert!(is_agent(
            r#""C:\Program Files\nodejs\NODE.EXE" "C:\agents\OMO.JS""#
        ));
        // An unrelated binary stays unrelated in either casing, and a mixed-case interpreter
        // running a non-agent entry point stays unobserved.
        assert!(!is_agent(r"C:\Windows\System32\Notepad.EXE"));
        assert!(!is_agent(r"C:\Windows\System32\notepad.exe"));
        assert!(!is_agent("Notepad.EXE"));
        assert!(!is_agent(r"C:\tools\NODE.EXE C:\Program Files\app\CLI.JS"));
    }

    #[test]
    fn agent_identity_reads_a_quoted_spaced_windows_command_line() {
        // A Windows command line is quote-delimited whenever an install path contains a space,
        // so the program is the leading quoted segment and not the first whitespace token
        // (`"C:\Program`, executable `Program`). A regression here leaves every agent pane on
        // such a host permanently unobserved while every Unix test stays green.
        assert!(is_agent(
            r#""C:\Program Files\nodejs\node.exe" "C:\agents\omo.js" --resume"#
        ));
        assert!(is_agent(
            r#""C:\Program Files\nodejs\node.exe" "C:\Program Files\agents\claude.js""#
        ));
        // A quoted interpreter running a non-agent entry point stays unobserved.
        assert!(!is_agent(
            r#""C:\Program Files\nodejs\node.exe" "C:\Program Files\app\cli.js""#
        ));
    }

    #[test]
    fn agent_identity_reads_unquoted_command_lines_and_bare_binaries() {
        assert!(is_agent("/usr/bin/node /opt/omo.js --inspect"));
        assert!(is_agent("codex"));
        assert!(!is_agent("/bin/zsh"));
        assert!(!is_agent("powershell.exe"));
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

    /// The observer tick captures the process listing once and classifies every live session
    /// against that one snapshot. Two panes with different foreground evidence are what prove
    /// the shared listing still answers per session; a single-session test cannot.
    #[cfg(unix)]
    #[tokio::test]
    async fn one_tick_snapshot_classifies_every_session() {
        use portable_pty::CommandBuilder;
        use std::sync::Arc;

        let (directory, script) = crate::ipc::run_blocking(|| {
            let directory = tempfile::tempdir().unwrap();
            let script = directory.path().join("omo");
            std::fs::write(&script, "printf 'AGENT_READY>'\nread answer\n").unwrap();
            Ok((directory, script))
        })
        .await
        .unwrap();
        let manager = crate::terminal::PtyManager::new();
        let mut agent_command = CommandBuilder::new("/bin/bash");
        agent_command.args(["--noprofile", "--norc", "-i"]);
        let (agent_id, mut agent_output) = manager.spawn(agent_command, 80, 24).unwrap();
        let agent = manager.get_session(&agent_id).unwrap();
        let mut shell_command = CommandBuilder::new("/bin/bash");
        shell_command.args(["--noprofile", "--norc", "-i"]);
        let (shell_id, mut shell_output) = manager.spawn(shell_command, 80, 24).unwrap();
        let shell = manager.get_session(&shell_id).unwrap();
        // Each marker is a protocol handshake, not a timing delay: the shell owns the
        // foreground exactly once it has printed its prompt.
        for (session, output) in [(&agent, &mut agent_output), (&shell, &mut shell_output)] {
            session
                .write_input(b"stty -echo; PS1='SHELL_READY>'\n")
                .unwrap();
            output_until(output, "SHELL_READY>").await;
        }
        agent
            .write_input(format!("/bin/sh '{}'\n", script.display()).as_bytes())
            .unwrap();
        output_until(&mut agent_output, "AGENT_READY>").await;
        let snapshot = crate::ipc::run_blocking(|| Ok(capture_process_snapshot().unwrap()))
            .await
            .unwrap();
        let classify = |session: &Arc<crate::terminal::PtySession>| {
            inspect_with_snapshot(session, &snapshot).unwrap()
        };
        assert!(
            matches!(classify(&agent), Some(Foreground::Agent(_))),
            "the tick's listing must still see the agent in front of its own pane"
        );
        assert_eq!(classify(&shell), Some(Foreground::Shell));
        // The same listing keeps serving the rest of the tick's sessions.
        assert!(matches!(classify(&agent), Some(Foreground::Agent(_))));
        assert_eq!(classify(&shell), Some(Foreground::Shell));
        // Teardown exists only so no child outlives the test. Both sessions stay in the
        // manager's registry, so their output channels are not required to close: shut the
        // sessions down through the manager and drop the receivers without draining them.
        agent.write_input(b"exit\n").unwrap();
        shell.write_input(b"exit\n").unwrap();
        manager.close_session(&agent_id).await.unwrap();
        manager.close_session(&shell_id).await.unwrap();
        drop((agent_output, shell_output));
        drop(directory);
    }
}
