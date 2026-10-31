//! Positive process evidence only. Sampling frequency is not a state timeout.
use super::PtySession;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Foreground {
    /// An agent process owns the PTY. `agent` is the canonical agent name the process was
    /// recognized by (see [`agent_identity`]), so a consumer can pick that agent's discovery
    /// adapter without re-reading the process table.
    Agent { pid: u32, agent: &'static str },
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
            Foreground::Agent { .. } if !self.agent_observed => {
                self.agent_observed = true;
                Some(AgentProcessEdge::Observed)
            }
            Foreground::Agent { .. } => None,
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

/// One session's classification against a tick's process listing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Observation {
    pub(crate) foreground: Foreground,
    /// The shell owns the foreground and the listing shows no other process that may belong to
    /// the pane: on Unix no child of the shell, no other member of the foreground group and no
    /// other process whose controlling terminal is the pane's; on Windows no descendant and no
    /// process orphaned since the shell started that may have come from this pane (see the
    /// Windows `Lineage`). Any program other than the shell that wrote to the pane before the
    /// listing was taken has exited.
    pub(crate) shell_alone: bool,
}

/// Per session, whether the newest OSC 7501 summary change is a transient record (`working`,
/// `blocked` or `idle`) whose program has not been seen gone yet, and the foreground process
/// group it was reported from.
///
/// [`ProcessTransition`] releases only after it has sampled an agent, and a reporting program can
/// live shorter than one observation tick, or be no agent at all. Its records then outlive it and
/// pin the pane to its last report. Two kinds of positive process evidence end a report instead:
///
/// - the shell owns the foreground from a group other than the one the report came from;
/// - a process listing requested after the report was applied shows the shell alone
///   ([`Observation::shell_alone`]), so no program other than the shell that could have written
///   it is still running.
///
/// The second covers ConPTY, which has no process group at all. A lone shell is no evidence
/// against the shell itself, so it does not release a report published while the shell's own
/// group held the foreground (or whose group could not be read) on its own: that may be a builtin
/// or a sourced function (a `read` prompt that reported `blocked`). A reporter that exited before
/// its bytes were parsed is attributed to the shell's group the same way; what tells it apart is
/// the foreground the observer sampled last ([`Self::sampled`], every observer tick) before the
/// report. When that sample was a group other than the report's own and other than the shell's
/// (learned from the observer's Shell observations), a child held the terminal and has handed it
/// back since: the report is taken to be that child's, and a later lone-shell listing (which
/// proves every child and every other foreground-group member gone) releases it. Each report also
/// counts as a sample of its own group, so only the first report after the hand-back carries it.
///
/// The trade-off: a report the shell itself writes after a sampled child exited, before the next
/// tick samples the shell's group again, is released by the next lone-shell listing although its
/// writer is alive. An interactive `child; printf ...` (a function or alias that runs a sampled
/// child and then reports `blocked` before a `read`) almost always lands there, so its transient
/// status is cleared while the shell waits; done and error records are never dropped this way,
/// and a script's own lines report from the script's group, not the shell's. A report from the
/// shell with no other group sampled
/// since the previous sample or report is never released this way, nor is one before the shell's
/// group has been learned. A reporter that lived shorter than one tick and was never sampled
/// stays, as do the shell's own reports, until a prompt start, a clear or a newer report.
/// Neither path ever releases on elapsed time or on unknown ownership.
#[derive(Default)]
pub(crate) struct ProtocolReporters {
    reports: parking_lot::Mutex<HashMap<String, Report>>,
    /// Per session, the foreground groups the observer has seen. Locked after `reports` when
    /// both are held.
    groups: parking_lot::Mutex<HashMap<String, SeenGroups>>,
}

/// One session's newest applied summary change.
struct Report {
    revision: u64,
    /// The change was transient and no release has consumed it yet.
    pending: bool,
    /// The foreground group the transient change was reported from, when known (Unix only).
    group: Option<u32>,
    /// The last foreground sampled before the report was a group other than the report's and
    /// the shell's: the report most likely came from that child, which has handed back since.
    departed: bool,
}

/// One session's foreground group evidence (Unix only).
#[derive(Default)]
struct SeenGroups {
    /// The newest known foreground group, from an observer sample or a report.
    last: Option<u32>,
    /// The group the shell owned the foreground from when last observed as Shell.
    shell: Option<u32>,
}

impl ProtocolReporters {
    /// The observer sampled the session's foreground `group`. Unknown (`None`) keeps the last
    /// known group.
    pub(crate) fn sampled(&self, session_id: &str, group: Option<u32>) {
        if let Some(group) = group {
            self.groups
                .lock()
                .entry(session_id.to_string())
                .or_default()
                .last = Some(group);
        }
    }

    /// A session's summary changed. A transient summary stays pending until its program is seen
    /// gone and remembers the foreground `group` it was reported from; anything else (`done`,
    /// `error`, a clear) leaves nothing to release.
    ///
    /// Changes are published outside the mirror lock and may arrive out of order; one whose
    /// `revision` is not newer than the last applied is ignored, so a stale clear cannot erase
    /// the group of a newer transient report (nor a stale report revive a cleared one).
    pub(crate) fn reported(
        &self,
        session_id: &str,
        revision: u64,
        transient: bool,
        group: Option<u32>,
    ) {
        let mut reports = self.reports.lock();
        if reports
            .get(session_id)
            .is_some_and(|report| report.revision >= revision)
        {
            return;
        }
        let mut groups = self.groups.lock();
        let seen = groups.entry(session_id.to_string()).or_default();
        let departed = match (seen.last, seen.shell) {
            (Some(last), Some(shell)) => transient && Some(last) != group && last != shell,
            _ => false,
        };
        if group.is_some() {
            seen.last = group;
        }
        reports.insert(
            session_id.to_string(),
            Report {
                revision,
                pending: transient,
                group: group.filter(|_| transient),
                departed,
            },
        );
    }

    /// A feed applied a transient record under `revision` from the foreground `group`, whether
    /// or not it changed the summary. A record that changed no summary (a repeated report, or
    /// one hidden behind the root record) never reaches [`Self::reported`], yet an exit decision
    /// gathered before it was applied keeps it; it stays pending under its own revision so a
    /// later observation that proves its program gone releases it. Delivered before the same
    /// feed's summary change, which carries the same revision and is then ignored.
    pub(crate) fn transient_applied(&self, session_id: &str, revision: u64, group: Option<u32>) {
        self.reported(session_id, revision, true, group);
    }

    /// The revision of the session's newest applied change, 0 before any. The observer reads it
    /// before it requests a process listing and hands it back with that listing's evidence.
    pub(crate) fn newest(&self, session_id: &str) -> u64 {
        self.reports
            .lock()
            .get(session_id)
            .map_or(0, |report| report.revision)
    }

    /// Whether `current`, observed while `group` owned the foreground, proves the program behind
    /// the pending report gone. `Some` once per report, carrying the newest revision whose
    /// records that evidence covers: the mirror release drops only records applied by then, so a
    /// report parsed after the decision (or one that never reached this map because it did not
    /// change the summary) survives it.
    ///
    /// `alone_after` is `Some(newest)` when the same listing showed the shell alone and was
    /// requested while `newest` (see [`Self::newest`]) was the session's newest revision. Only a
    /// report applied by then was written by a process that existed before the listing, so only
    /// such a report is released by it; a report applied later may come from a program the
    /// listing never saw.
    ///
    /// `live_group` re-reads the foreground group under the same lock a report takes: a program
    /// that started and reported after `current` was observed owns the foreground now, so its
    /// fresh report is never mistaken for one the shell already took back. ConPTY has no group,
    /// so there both sides are `None` and the revision bound is the guard.
    pub(crate) fn foreground_returned(
        &self,
        session_id: &str,
        current: Option<Foreground>,
        group: Option<u32>,
        alone_after: Option<u64>,
        live_group: impl FnOnce() -> Option<u32>,
    ) -> Option<u64> {
        if current != Some(Foreground::Shell) {
            return None;
        }
        if let Some(group) = group {
            self.groups
                .lock()
                .entry(session_id.to_string())
                .or_default()
                .shell = Some(group);
        }
        let mut reports = self.reports.lock();
        let report = reports.get_mut(session_id).filter(|report| report.pending)?;
        let left_group = group.is_some_and(|group| report.group.is_some_and(|from| from != group));
        // Whether the report's writer can be told apart from the shell: with process groups only
        // a report from another group, or one that followed a child's hand-back, can; ConPTY has
        // no group to attribute it by.
        let not_the_shell = match (report.group, group) {
            _ if report.departed => true,
            (Some(from), Some(now)) => from != now,
            (None, Some(_)) => false,
            (_, None) => true,
        };
        // A lone-shell listing covers every record applied before it was requested; the group
        // edge covers the report it judged.
        let alone_through =
            alone_after.filter(|newest| not_the_shell && report.revision <= *newest);
        if (left_group || alone_through.is_some()) && live_group() == group {
            // Keep the revision: an older change still in flight must not re-install the group.
            report.pending = false;
            report.group = None;
            report.departed = false;
            return Some(alone_through.map_or(report.revision, |newest| {
                newest.max(report.revision)
            }));
        }
        None
    }

    /// Keeps only the sessions `live` accepts.
    pub(crate) fn retain(&self, live: impl Fn(&str) -> bool) {
        self.reports.lock().retain(|id, _| live(id));
        self.groups.lock().retain(|id, _| live(id));
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

/// A listed interpreter, or a versioned CPython binary (`python3.12`) that a console-script
/// shebang such as aider's resolves to.
fn is_interpreter(name: &str) -> bool {
    is_named(name, &INTERPRETERS)
        || (name.len() > 8
            && name
                .get(..8)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("python3."))
            && name[8..].bytes().all(|byte| byte.is_ascii_digit()))
}

/// Interpreter flags whose argument is inline code, not an entry point: what follows them
/// (`sh -c omo`) is never the script that names the agent.
const INLINE_CODE_FLAGS: [&str; 3] = ["-c", "-e", "--eval"];

/// Entry-point basenames too generic to name an agent on their own (`dist/bundle/cli.js`).
const GENERIC_ENTRY_POINTS: [&str; 3] = ["cli", "index", "main"];

/// Package directories that identify the agent behind a generic entry point. Order matters:
/// omo runs the senpi bundle with an `omo-ai` extension, so `omo-ai` must win over senpi.
const PACKAGE_AGENTS: [(&str, &str); 4] = [
    ("omo-ai", "omo"),
    ("@code-yeongyu/senpi", "pi"),
    ("oh-my-pi", "omp"),
    ("omp", "omp"),
];

/// The agent a generic entry point belongs to: the first package directory that appears as
/// whole path segments anywhere in the argument list, with either path separator.
fn package_agent(tokens: &[&str]) -> Option<&'static str> {
    PACKAGE_AGENTS.iter().find_map(|&(package, agent)| {
        let expected: Vec<&str> = package.split('/').collect();
        tokens
            .iter()
            .any(|token| {
                let segments: Vec<&str> = token.trim_matches('"').split(['/', '\\']).collect();
                segments.windows(expected.len()).any(|window| {
                    window
                        .iter()
                        .zip(&expected)
                        .all(|(segment, wanted)| segment.eq_ignore_ascii_case(wanted))
                })
            })
            .then_some(agent)
    })
}

/// Agent executable or entry-point names, lowercase because the comparison ignores case.
const AGENT_NAMES: [&str; 18] = [
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
    "agy",
    "pi",
    "omp",
    "aider",
    "crush",
    "droid",
];

/// The agent name a launcher binary is recognized by, mapped to the agent id the rest of Ferryx
/// uses (discovery, resume, machine admission). cursor's binary is `agent` or `cursor-agent`,
/// and antigravity's is `agy`.
fn canonical_agent_name(matched: &'static str) -> &'static str {
    match matched {
        "agent" => "cursor-agent",
        "agy" => "antigravity",
        other => other,
    }
}

#[cfg(test)]
fn is_agent(command: &str) -> bool {
    agent_identity(command).is_some()
}

/// Which agent `command` launches, by its executable or interpreter entry point.
pub(crate) fn agent_identity(command: &str) -> Option<&'static str> {
    let tokens = command_tokens(command);
    let program = tokens.first().copied()?;
    let candidate = if is_interpreter(executable_name(program)) {
        // Interpreter entry points, not arbitrary arguments such as `echo omo`. The script is
        // the first argument after the interpreter's own flags (`node --no-warnings=X agent`);
        // a command line that carries none still ends in its entry point, so the last path
        // segment of the line is the fallback.
        let mut arguments = tokens.iter().skip(1).copied();
        let script = loop {
            match arguments.next() {
                Some(flag) if is_named(flag, &INLINE_CODE_FLAGS) => return None,
                Some(flag) if flag.starts_with('-') => continue,
                other => break other,
            }
        };
        let script = script.or_else(|| last_segment(command)).unwrap_or("");
        if is_named(agent_name(script), &GENERIC_ENTRY_POINTS) {
            // A bundled CLI (`senpi/dist/bundle/cli.js`) is named by its package, not its file.
            return package_agent(&tokens);
        }
        script
    } else {
        program
    };
    let name = agent_name(candidate);
    AGENT_NAMES
        .iter()
        .copied()
        .find(|known| name.eq_ignore_ascii_case(known))
        .map(canonical_agent_name)
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
/// to every [`observe_with_snapshot`] call in the tick.
pub(crate) struct ProcessSnapshot {
    /// On Unix this is one `ps` listing; each session filters it by its own foreground
    /// process group instead of forking `ps` per session.
    #[cfg(any(unix, windows))]
    processes: Vec<platform::Process>,
}

/// Capture the process listing one tick's observations share.
///
/// A failed capture is unknown ownership for every session in that tick: report no
/// foreground evidence rather than an absent agent.
pub(crate) fn capture_process_snapshot() -> std::io::Result<ProcessSnapshot> {
    #[cfg(any(unix, windows))]
    {
        Ok(ProcessSnapshot {
            processes: platform::sample()?,
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        Ok(ProcessSnapshot {})
    }
}

#[cfg(all(test, unix))]
pub(crate) fn inspect(session: &PtySession) -> std::io::Result<Option<Foreground>> {
    inspect_with_snapshot(session, &capture_process_snapshot()?)
}

#[cfg(all(test, unix))]
pub(crate) fn inspect_with_snapshot(
    session: &PtySession,
    snapshot: &ProcessSnapshot,
) -> std::io::Result<Option<Foreground>> {
    Ok(observe_with_snapshot(session, snapshot)?.map(|observed| observed.foreground))
}

/// Observe one session against a process listing already captured for this tick.
pub(crate) fn observe_with_snapshot(
    session: &PtySession,
    snapshot: &ProcessSnapshot,
) -> std::io::Result<Option<Observation>> {
    platform::observe_with_snapshot(session, snapshot)
}

#[cfg(unix)]
mod platform {
    use super::*;
    pub(super) struct Process {
        pid: Option<u32>,
        parent: Option<u32>,
        pgid: Option<u32>,
        /// The controlling terminal as `ps` names it (`pts/3`, `ttys002`; `?` or `??` for none).
        tty: Option<String>,
        command: String,
    }

    /// One `ps` listing for the whole tick. ps is available on both macOS and Linux. Do not
    /// assume /proc exists. Both name the controlling terminal by its path below `/dev`.
    pub(super) fn sample() -> std::io::Result<Vec<Process>> {
        let output = std::process::Command::new("ps")
            .args(["-axo", "pid=,ppid=,pgid=,tty=,args="])
            .output()?;
        if !output.status.success() {
            return Err(std::io::Error::other("foreground process snapshot failed"));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(text
            .lines()
            .map(|line| {
                let mut fields = line.split_whitespace();
                let pid = fields.next().and_then(|s| s.parse::<u32>().ok());
                let parent = fields.next().and_then(|s| s.parse::<u32>().ok());
                let pgid = fields.next().and_then(|s| s.parse::<u32>().ok());
                let tty = fields.next().map(str::to_string);
                let command = fields.collect::<Vec<_>>().join(" ");
                Process {
                    pid,
                    parent,
                    pgid,
                    tty,
                    command,
                }
            })
            .collect())
    }

    pub(super) fn observe_with_snapshot(
        session: &PtySession,
        // The tick's shared listing, filtered by this session's own foreground process group.
        snapshot: &ProcessSnapshot,
    ) -> std::io::Result<Option<Observation>> {
        let Some(shell) = session.pid() else {
            return Ok(None);
        };
        let Some(group) = session.foreground_process_group()? else {
            return Ok(None);
        };
        let tty = session.slave_tty_name();
        let tty = tty
            .as_deref()
            .and_then(|path| path.to_str())
            .and_then(|path| path.strip_prefix("/dev/"));
        let observed = classify(&snapshot.processes, shell, group, tty);
        // Ownership can change while this session is classified. A mixed snapshot is
        // unknown, not an Other edge that would erase the observed agent.
        if session.foreground_process_group()? != Some(group) {
            return Ok(None);
        }
        Ok(observed)
    }

    /// Classifies the pane whose shell is `shell`, whose foreground group is `group` and whose
    /// terminal `ps` names `tty`.
    fn classify(
        processes: &[Process],
        shell: u32,
        group: u32,
        tty: Option<&str>,
    ) -> Option<Observation> {
        let mut shell_foreground = false;
        // The foreground group always has a member; a shared listing taken before the group
        // formed has none, and that mixed snapshot is unknown rather than an Other edge.
        let mut group_listed = false;
        // Any row besides the shell that is its child, shares the foreground group or has the
        // pane as its controlling terminal: a writer reparented away from the shell (its parent
        // exited) keeps the pane as its controlling terminal in a group of its own. A row whose
        // pid did not parse counts too: unreadable is not absent. A pane whose terminal is
        // unknown can never prove its shell alone.
        let mut accompanied = tty.is_none();
        let mut agent = None;
        for process in processes {
            let (pid, parent, pgid, command) =
                (process.pid, process.parent, process.pgid, process.command.as_str());
            group_listed |= pgid == Some(group);
            let on_pane = tty.is_some_and(|tty| process.tty.as_deref() == Some(tty));
            accompanied |= pid != Some(shell)
                && (parent == Some(shell) || pgid == Some(group) || on_pane);
            // A suspended/background agent is still alive: do not call shell ownership exit.
            if pgid == Some(group) || parent == Some(shell) {
                if let Some(name) = agent_identity(command) {
                    agent = Some((pid, name));
                    break;
                }
            }
            if pid == Some(shell) && pgid == Some(group) && is_shell(command) {
                shell_foreground = true;
            }
        }
        if let Some((pid, name)) = agent {
            return pid.map(|pid| Observation {
                foreground: Foreground::Agent { pid, agent: name },
                shell_alone: false,
            });
        }
        if !group_listed {
            return None;
        }
        Some(if shell_foreground {
            Observation {
                foreground: Foreground::Shell,
                shell_alone: !accompanied,
            }
        } else {
            Observation {
                foreground: Foreground::Other,
                shell_alone: false,
            }
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn row(pid: u32, parent: u32, pgid: u32, tty: &str, command: &str) -> Process {
            Process {
                pid: Some(pid),
                parent: Some(parent),
                pgid: Some(pgid),
                tty: Some(tty.to_string()),
                command: command.to_string(),
            }
        }

        #[test]
        fn the_shell_is_alone_only_without_a_child_or_a_foreground_companion() {
            let shell = |alone| {
                Some(Observation {
                    foreground: Foreground::Shell,
                    shell_alone: alone,
                })
            };
            let pane = Some("pts/3");
            let prompt = [row(1, 0, 1, "?", "launchd"), row(100, 1, 100, "pts/3", "-bash")];
            assert_eq!(classify(&prompt, 100, 100, pane), shell(true));
            // A pane whose terminal is unknown cannot prove anything absent.
            assert_eq!(classify(&prompt, 100, 100, None), shell(false));
            // A background job (its own group) is a live child: whatever reported may be it.
            let job = [row(100, 1, 100, "pts/3", "-bash"), row(300, 100, 300, "pts/3", "sleep")];
            assert_eq!(classify(&job, 100, 100, pane), shell(false));
            // A child that has not taken its own group yet, or a zombie not reaped yet.
            let forked = [row(100, 1, 100, "pts/3", "-bash"), row(301, 100, 100, "pts/3", "bash")];
            assert_eq!(classify(&forked, 100, 100, pane), shell(false));
            // A row whose pid did not parse is not evidence of absence.
            let unreadable = [
                row(100, 1, 100, "pts/3", "-bash"),
                Process {
                    pid: None,
                    parent: Some(100),
                    pgid: Some(302),
                    tty: None,
                    command: String::new(),
                },
            ];
            assert_eq!(classify(&unreadable, 100, 100, pane), shell(false));
            // Another program in front is never alone, and a listing without the group is unknown.
            let other = [row(100, 1, 100, "pts/3", "-bash"), row(400, 100, 400, "pts/3", "vim")];
            assert_eq!(
                classify(&other, 100, 400, pane),
                Some(Observation {
                    foreground: Foreground::Other,
                    shell_alone: false,
                })
            );
            assert_eq!(classify(&prompt, 100, 500, pane), None);
        }

        /// Review N2: a writer whose parent exited (reparented to init or a subreaper) in a
        /// background group of its own is neither the shell's child nor in its foreground group,
        /// yet it still has the pane as its controlling terminal and can write to it. Its live
        /// report must survive a refresh that otherwise finds the shell alone.
        #[test]
        fn a_detached_writer_on_the_pane_terminal_keeps_the_shell_accompanied() {
            for pane in ["pts/3", "ttys002"] {
                let mut rows = vec![
                    row(1, 0, 1, "?", "init"),
                    row(100, 1, 100, pane, "-bash"),
                    // Another pane's process and a daemon without a terminal are no company.
                    row(200, 1, 200, "pts/9", "vim"),
                    row(201, 1, 201, "??", "sshd"),
                ];
                let observed = classify(&rows, 100, 100, Some(pane)).expect("shell in front");
                assert!(observed.shell_alone, "{pane}: nothing else is on the pane");
                rows.push(row(500, 1, 500, pane, "reporter --watch"));
                let observed = classify(&rows, 100, 100, Some(pane)).expect("shell in front");
                assert_eq!(
                    observed,
                    Observation {
                        foreground: Foreground::Shell,
                        shell_alone: false,
                    },
                    "{pane}: the reparented writer is still on the pane"
                );
            }
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    #[derive(Clone, serde::Deserialize)]
    pub(super) struct Process {
        #[serde(rename = "ProcessId")]
        pid: u32,
        #[serde(rename = "ParentProcessId")]
        parent: u32,
        #[serde(rename = "Name")]
        name: String,
        #[serde(rename = "CommandLine")]
        command: Option<String>,
        /// The Windows logon session; a process in another one cannot have the pane's console.
        #[serde(rename = "SessionId")]
        session: Option<u32>,
        /// Creation time as a UTC FILETIME, `None` when Windows did not report one.
        #[serde(rename = "Created")]
        created: Option<i64>,
    }

    /// The `Win32_Process` columns [`Process`] reads. `CreationDate` goes out as a FILETIME so
    /// the JSON carries a plain integer whatever the PowerShell version.
    const LISTING: &str = concat!(
        "ConvertTo-Json -Compress -InputObject @(Get-CimInstance Win32_Process -ErrorAction Stop",
        " | Select-Object ProcessId,ParentProcessId,Name,CommandLine,SessionId,",
        "@{n='Created';e={if ($_.CreationDate) { $_.CreationDate.ToFileTimeUtc() }}})"
    );

    /// The complete `Win32_Process` listing. ConPTY has no tcgetpgrp, so ownership is decided
    /// from the whole process table; taking that sample is one `powershell.exe` launch, which
    /// is why a tick takes it once and shares it with every session it observes.
    pub(super) fn sample() -> std::io::Result<Vec<Process>> {
        let output = crate::util::no_window_command("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", LISTING])
            .output()?;
        if !output.status.success() {
            return Err(std::io::Error::other("ConPTY process snapshot failed"));
        }
        serde_json::from_slice(&output.stdout).map_err(std::io::Error::other)
    }

    pub(super) fn observe_with_snapshot(
        session: &PtySession,
        snapshot: &ProcessSnapshot,
    ) -> std::io::Result<Option<Observation>> {
        /// Each pane's [`Lineage`], keyed by its shell's pid. A shell no longer listed has
        /// exited, and its memory with it.
        static LINEAGES: parking_lot::Mutex<Vec<(u32, Lineage)>> =
            parking_lot::Mutex::new(Vec::new());
        let Some(shell) = session.pid() else {
            return Ok(None);
        };
        let mut lineages = LINEAGES.lock();
        lineages.retain(|(pid, _)| snapshot.processes.iter().any(|p| p.pid == *pid));
        let index = match lineages.iter().position(|(pid, _)| *pid == shell) {
            Some(index) => index,
            None => {
                lineages.push((shell, Lineage::default()));
                lineages.len() - 1
            }
        };
        Ok(classify(&snapshot.processes, shell, &mut lineages[index].1))
    }

    /// Whether two creation times may name the same process: unknown matches anything.
    fn may_match(a: Option<i64>, b: Option<i64>) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        }
    }

    /// What earlier listings showed of one pane's shell tree. The process table is the whole
    /// logon session's, so without it an orphan from anywhere (a launcher-style app started in
    /// another pane) would keep every ConPTY shell accompanied.
    #[derive(Default)]
    struct Lineage {
        /// The creation time of the shell this memory belongs to; a different one is a new shell
        /// under a reused pid and starts over.
        shell_created: Option<i64>,
        /// `None` until a listing has classified this pane: without history every possible
        /// orphan counts.
        history: Option<History>,
    }

    #[derive(Default)]
    struct History {
        /// What earlier listings attributed to the pane, with creation times: the shell's
        /// descendants and every possible orphan counted as its company. Kept while the process
        /// is listed or a listed process still names it as its parent, so an orphan of a
        /// remembered middle process, or one counted once, stays attributed to the pane.
        seen: HashMap<u32, Option<i64>>,
        /// Every process of the previous listing whose creation time was known. A process not
        /// in it may have been born and orphaned between listings, so no listing could have
        /// attributed it.
        listed: HashMap<u32, i64>,
    }

    impl Lineage {
        /// Whether `process`, a possible orphan, may have come from this pane: no history yet,
        /// the previous listing did not show it, or an earlier listing attributed it or its
        /// parent to the pane. Unknown creation times count as possible.
        fn may_own(&self, process: &Process) -> bool {
            let Some(history) = &self.history else {
                return true;
            };
            let unattributable = process
                .created
                .is_none_or(|own| history.listed.get(&process.pid) != Some(&own));
            let seen = history
                .seen
                .get(&process.pid)
                .is_some_and(|&created| may_match(created, process.created));
            let born_of_seen = history.seen.get(&process.parent).is_some_and(|&parent| {
                match (parent, process.created) {
                    (Some(parent), Some(own)) => parent <= own,
                    _ => true,
                }
            });
            unattributable || seen || born_of_seen
        }

        /// Remembers this listing's descendants of the shell, the `orphans` it counted as
        /// company and the listing itself.
        fn record(
            &mut self,
            processes: &[Process],
            shell: u32,
            descendants: &[u32],
            orphans: &[&Process],
        ) {
            let history = self.history.get_or_insert_with(History::default);
            for p in processes {
                if p.pid != shell && descendants.contains(&p.pid) {
                    history.seen.insert(p.pid, p.created);
                }
            }
            for p in orphans {
                history.seen.insert(p.pid, p.created);
            }
            history.seen.retain(|pid, created| {
                processes.iter().any(|p| {
                    (p.pid == *pid && may_match(p.created, *created)) || p.parent == *pid
                })
            });
            history.listed = processes
                .iter()
                .filter_map(|p| p.created.map(|created| (p.pid, created)))
                .collect();
        }
    }

    /// Whether `process`, outside the shell's descendant tree, may still be a descendant whose
    /// parent chain broke: its parent exited (not listed, or the pid now names a process created
    /// after it), it runs in the shell's logon session and it started after the shell did.
    /// Anything unknown counts as possible: absence of a parent link is not proof of unrelation.
    fn maybe_orphaned_descendant(
        processes: &[Process],
        shell: &Process,
        process: &Process,
    ) -> bool {
        let same_session = match (process.session, shell.session) {
            (Some(own), Some(pane)) => own == pane,
            _ => true,
        };
        let after_shell = match (process.created, shell.created) {
            (Some(own), Some(started)) => own >= started,
            _ => true,
        };
        let orphaned = match processes.iter().find(|parent| parent.pid == process.parent) {
            None => true,
            Some(parent) => match (parent.created, process.created) {
                (Some(parent), Some(own)) => parent > own,
                _ => true,
            },
        };
        same_session && after_shell && orphaned
    }

    /// Classifies the pane whose shell is `shell` against the whole process table.
    fn classify(
        processes: &[Process],
        shell: u32,
        lineage: &mut Lineage,
    ) -> Option<Observation> {
        // A complete descendant snapshot supplies positive child-exit evidence;
        // shell-with-no-descendants is the conservative fallback.
        let shell_row = processes
            .iter()
            .find(|p| p.pid == shell && is_shell(&p.name))?;
        if lineage.shell_created != shell_row.created {
            *lineage = Lineage {
                shell_created: shell_row.created,
                history: None,
            };
        }
        let mut descendants = vec![shell];
        loop {
            let before = descendants.len();
            for p in processes {
                if descendants.contains(&p.parent) && !descendants.contains(&p.pid) {
                    descendants.push(p.pid);
                }
            }
            if descendants.len() == before {
                break;
            }
        }
        let agent = processes
            .iter()
            .filter(|p| p.pid != shell && descendants.contains(&p.pid))
            .find_map(|p| {
                agent_identity(&p.name)
                    .or_else(|| p.command.as_deref().and_then(agent_identity))
                    .map(|name| (p.pid, name))
            });
        // The shell with no descendant at all owns the foreground. It is alone only when no
        // process could be a descendant whose middle process exited (ConPTY keeps such a
        // grandchild attached to the pane, so it can still write there) and this pane's
        // history may own (see [`Lineage`]).
        let in_front = descendants.len() == 1;
        let orphans: Vec<&Process> = processes
            .iter()
            .filter(|p| {
                !descendants.contains(&p.pid)
                    && maybe_orphaned_descendant(processes, shell_row, p)
                    && lineage.may_own(p)
            })
            .collect();
        lineage.record(processes, shell, &descendants, &orphans);
        if let Some((pid, agent)) = agent {
            return Some(Observation {
                foreground: Foreground::Agent { pid, agent },
                shell_alone: false,
            });
        }
        Some(Observation {
            foreground: if in_front {
                Foreground::Shell
            } else {
                Foreground::Other
            },
            shell_alone: in_front && orphans.is_empty(),
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A row in logon session 1 created at `created`.
        fn at(pid: u32, parent: u32, name: &str, created: i64) -> Process {
            Process {
                pid,
                parent,
                name: name.to_string(),
                command: None,
                session: Some(1),
                created: Some(created),
            }
        }

        /// A row in session 1 whose creation order follows its pid.
        fn row(pid: u32, parent: u32, name: &str) -> Process {
            at(pid, parent, name, i64::from(pid))
        }

        /// A pane's first listing: no history to attribute anything by.
        fn first(processes: &[Process], shell: u32) -> Option<Observation> {
            classify(processes, shell, &mut Lineage::default())
        }

        fn shell(alone: bool) -> Option<Observation> {
            Some(Observation {
                foreground: Foreground::Shell,
                shell_alone: alone,
            })
        }

        #[test]
        fn a_conpty_shell_is_alone_only_without_any_descendant() {
            let prompt = [row(4, 0, "System"), row(100, 50, "pwsh.exe")];
            assert_eq!(
                first(&prompt, 100),
                Some(Observation {
                    foreground: Foreground::Shell,
                    shell_alone: true,
                })
            );
            // A grandchild whose parent is still listed keeps the shell accompanied.
            let busy = [
                row(100, 50, "pwsh.exe"),
                row(200, 100, "cmd.exe"),
                row(300, 200, "reporter.exe"),
            ];
            assert_eq!(
                first(&busy, 100),
                Some(Observation {
                    foreground: Foreground::Other,
                    shell_alone: false,
                })
            );
            // No shell row is unknown, never a lone shell.
            assert_eq!(first(&[row(200, 100, "reporter.exe")], 100), None);
        }

        /// Review N2: ConPTY keeps a grandchild attached to the pane after its middle process
        /// exits, and its parent link then names nothing (or a reused pid). Such a writer is no
        /// descendant any more, yet the shell is not alone.
        #[test]
        fn an_orphaned_grandchild_keeps_the_conpty_shell_accompanied() {
            // explorer.exe outlived its launcher long before the shell started: no company.
            let mut rows = vec![row(30, 20, "explorer.exe"), row(100, 30, "pwsh.exe")];
            // An orphan in another logon session cannot hold the pane's console.
            rows.push(Process {
                session: Some(0),
                ..row(700, 650, "updater.exe")
            });
            assert_eq!(first(&rows, 100), shell(true));
            // cmd.exe (pid 200) started reporter.exe and exited.
            let mut orphaned = rows.clone();
            orphaned.push(row(300, 200, "reporter.exe"));
            assert_eq!(first(&orphaned, 100), shell(false));
            // The middle pid was reused by a process created after the reporter.
            let mut reused = rows.clone();
            reused.push(at(300, 200, "reporter.exe", 250));
            reused.push(at(200, 30, "notepad.exe", 400));
            assert_eq!(first(&reused, 100), shell(false));
            // Unknown creation times prove nothing.
            let mut unknown = rows.clone();
            unknown[1].created = None;
            assert_eq!(first(&unknown, 100), shell(false));
        }

        /// Review follow-up: the table is the whole logon session's. A launcher started in
        /// another pane (`code .`) whose middle process exited is an orphan created after this
        /// pane's shell, yet an earlier listing showed it in the other pane's tree: it must not
        /// keep this shell accompanied, or a transient report here never meets a lone shell.
        #[test]
        fn an_orphan_from_another_pane_leaves_the_conpty_shell_alone() {
            let mut lineage = Lineage::default();
            // This pane's shell 100; another pane's shell 150 runs the launcher cli (400),
            // which started the editor (500).
            let before = [
                row(30, 20, "explorer.exe"),
                row(100, 30, "pwsh.exe"),
                row(150, 30, "pwsh.exe"),
                row(400, 150, "cmd.exe"),
                row(500, 400, "Code.exe"),
            ];
            assert_eq!(classify(&before, 100, &mut lineage), shell(true));
            // The cli exited: the editor is an orphan created after shell 100.
            let after = [
                row(30, 20, "explorer.exe"),
                row(100, 30, "pwsh.exe"),
                row(150, 30, "pwsh.exe"),
                row(500, 400, "Code.exe"),
            ];
            assert_eq!(classify(&after, 100, &mut lineage), shell(true));
            assert_eq!(classify(&after, 100, &mut lineage), shell(true));
            // Without history the same listing stays conservative.
            assert_eq!(first(&after, 100), shell(false));
        }

        /// The pane's own orphans still count: one an earlier listing showed in the shell's
        /// tree, one born and orphaned between two listings, and one whose creation time is
        /// unknown. Each keeps counting on every later listing while it lives.
        #[test]
        fn this_panes_orphans_keep_the_conpty_shell_accompanied() {
            let base = [row(30, 20, "explorer.exe"), row(100, 30, "pwsh.exe")];
            let with = |extra: &[Process]| {
                let mut rows = base.to_vec();
                rows.extend_from_slice(extra);
                rows
            };

            // cmd.exe (200) under the shell started reporter.exe (300), then exited.
            let mut lineage = Lineage::default();
            let busy = with(&[row(200, 100, "cmd.exe"), row(300, 200, "reporter.exe")]);
            assert_eq!(
                classify(&busy, 100, &mut lineage),
                Some(Observation {
                    foreground: Foreground::Other,
                    shell_alone: false,
                })
            );
            let orphaned = with(&[row(300, 200, "reporter.exe")]);
            assert_eq!(classify(&orphaned, 100, &mut lineage), shell(false));
            assert_eq!(classify(&orphaned, 100, &mut lineage), shell(false));
            assert_eq!(classify(&base, 100, &mut lineage), shell(true));

            // Born and orphaned between listings: no listing could attribute it elsewhere.
            let mut lineage = Lineage::default();
            assert_eq!(classify(&base, 100, &mut lineage), shell(true));
            let newborn = with(&[row(310, 290, "reporter.exe")]);
            assert_eq!(classify(&newborn, 100, &mut lineage), shell(false));
            assert_eq!(classify(&newborn, 100, &mut lineage), shell(false));

            // An unknown creation time proves nothing, listing after listing.
            let mut lineage = Lineage::default();
            assert_eq!(classify(&base, 100, &mut lineage), shell(true));
            let unknown = with(&[Process {
                created: None,
                ..row(320, 290, "reporter.exe")
            }]);
            assert_eq!(classify(&unknown, 100, &mut lineage), shell(false));
            assert_eq!(classify(&unknown, 100, &mut lineage), shell(false));
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    use super::*;
    pub(super) fn observe_with_snapshot(
        _: &PtySession,
        _: &ProcessSnapshot,
    ) -> std::io::Result<Option<Observation>> {
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
        // The marker is split in the command so its echo (by the tty before `stty -echo`
        // applies, or by readline) cannot match: only the prompt drawn after the line ran can.
        session
            .write_input(b"stty -echo; PS1='SHELL_''READY>'\n")
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
            detail: None,
            origin: crate::daemon::protocol::AgentStateOrigin::Agent,
            outcome: None,
            kind: None,
        });
        let mut states = hub.subscribe(&id);
        let mut transition = ProcessTransition::default();
        for _ in 0..2 {
            let session = Arc::clone(&session);
            let observed = crate::ipc::run_blocking(move || Ok(inspect(&session).unwrap()))
                .await
                .unwrap();
            assert!(
                matches!(observed, Some(Foreground::Agent { .. })),
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
    fn agent_identity_covers_every_supported_agent_name() {
        for name in ["agy", "pi", "omp", "aider", "crush", "droid"] {
            assert!(is_agent(name), "{name}");
            assert!(is_agent(&format!("/usr/local/bin/{name} --resume")), "{name}");
            assert!(is_agent(&format!("/usr/bin/node /opt/{name}.js")), "{name}");
        }
        assert!(is_agent(r"C:\Users\dev\.local\bin\agy.exe"));
    }

    #[test]
    fn agent_identity_reads_measured_launch_command_lines() {
        // Each argv below was read with `ps -axo args` while the agent ran.
        // aider: a console script under a versioned CPython.
        assert!(is_agent(
            "/Users/x/.local/share/uv/tools/aider-chat/bin/python3.12 /Users/x/.local/bin/aider"
        ));
        // pi: a node-shebang symlink in node_modules/.bin.
        assert!(is_agent("node /Users/x/proj/node_modules/.bin/pi"));
        // omo: the senpi bundle loaded with the omo-ai extension.
        assert!(is_agent(
            "/Users/x/.bun/bin/bun /Users/x/.bun/install/global/node_modules/@code-yeongyu/senpi/dist/bundle/cli.js --extension /Users/x/.bun/install/global/node_modules/omo-ai/plugin --session 01a15ec8"
        ));
        // A generic entry point outside a known package is not an agent.
        assert!(!is_agent("node --inspect /x/server.js"));
        assert!(!is_agent("node /x/app/dist/cli.js --port 3000"));
        assert!(!is_agent("/usr/bin/python3.12 /x/manage.py runserver"));
        assert!(!is_agent("node -e omo"));
    }

    #[test]
    fn agent_identity_reads_measured_parity_launch_argv() {
        // gjc: bun runs the bin symlink, whose basename is `gjc` (.omo-parity/facts/remote-gjc.md:18).
        assert!(is_agent(
            "bun /home/indo/.local/share/mise/installs/node/26.8.1/bin/gjc --model fakelocal/fake-model --tools ask,read,bash"
        ));
        // opencode: native Mach-O, fresh and resumed (.omo-parity/facts/local.md:171).
        assert!(is_agent("opencode"));
        assert!(is_agent("opencode -s ses_e9d6ab0a6ffe947zqzT8h7XHYX"));
        // cursor-agent: the launcher's `exec -a` keeps the symlink path as argv[0] while the
        // real executable is the bundled node (.omo-parity/facts/local-3.md:149-152).
        assert!(is_agent(
            "/Users/indo/.local/bin/cursor-agent --use-system-ca /Users/indo/.local/share/cursor-agent/versions/2026.10.19-6bbf2c4/index.js"
        ));
        assert!(is_agent(
            "/Users/indo/.local/bin/cursor-agent --use-system-ca /Users/indo/.local/share/cursor-agent/versions/2026.10.19-6bbf2c4/index.js --resume 9371cc0a-d5c0-4cca-bcae-cae24632e1b5"
        ));
        // grok: the ~/.grok/bin symlink, fresh and resumed (.omo-parity/facts/remote-grok.md:54-55).
        assert!(is_agent("/home/indo/.grok/bin/grok"));
        assert!(is_agent(
            "/home/indo/.grok/bin/grok --resume 01a16274-a688-7e73-b6fc-434bc7781be4"
        ));
        // kimi: the uv tool's python runs the ~/.local/bin/kimi script (.omo-parity/facts/local-2.md:212,216).
        assert!(is_agent(
            "/Users/indo/.local/share/uv/tools/kimi-cli/bin/python /Users/indo/.local/bin/kimi"
        ));
        assert!(is_agent(
            "/Users/indo/.local/share/uv/tools/kimi-cli/bin/python /Users/indo/.local/bin/kimi -S eca66b4f-a89b-4979-9f13-cbe3f35b5991"
        ));
        // cline: native binary, its resume form, and the hub-daemon child (.omo-parity/facts/local.md:107).
        assert!(is_agent("cline"));
        assert!(is_agent("cline --id 1792653433537_85evt"));
        assert!(is_agent(
            ".../@cline/cli-darwin-arm64/bin/cline --cline-hub-daemon --cwd <repo> --host 127.0.0.1 --port 25463 --pathname /hub"
        ));
        // droid: native ELF, argv[0] `droid` (.omo-parity/facts/remote-droid.md:41).
        assert!(is_agent("droid"));
    }

    #[test]
    fn foreground_agent_carries_the_name_discovery_uses() {
        // Re-explore-4 G2: the observer picks the discovery adapter by this name, so each
        // published family must come back under the id discovery and resume use.
        for (command, agent) in [
            // kimi: uv's python runs ~/.local/bin/kimi (local-2.md:212,216).
            ("/Users/indo/.local/share/uv/tools/kimi-cli/bin/python /Users/indo/.local/bin/kimi", "kimi"),
            ("/Users/indo/.local/share/uv/tools/kimi-cli/bin/python /Users/indo/.local/bin/kimi -S eca66b4f-a89b-4979-9f13-cbe3f35b5991", "kimi"),
            // copilot: the Mach-O binary itself (local-3.md:17, local.md:124).
            ("copilot", "copilot"),
            ("copilot --resume 5793c06c-5bee-4b34-883b-589051286cfd", "copilot"),
            (r"C:\Users\dev\AppData\Local\Programs\Copilot.Exe", "copilot"),
            // gjc: bun runs the bin symlink (remote-gjc.md:18).
            ("bun /home/indo/.local/share/mise/installs/node/26.8.1/bin/gjc --model fakelocal/fake-model", "gjc"),
            ("claude --resume id", "claude"),
            ("/home/indo/.grok/bin/grok", "grok"),
            ("bun /home/indo/.bun/bin/omp", "omp"),
            ("/Users/indo/.local/bin/agent --resume id", "cursor-agent"),
            ("/Users/x/.local/bin/agy", "antigravity"),
        ] {
            assert_eq!(agent_identity(command), Some(agent), "{command}");
        }
        assert_eq!(
            agent_identity("bun /g/node_modules/@code-yeongyu/senpi/dist/bundle/cli.js --extension /g/node_modules/omo-ai/plugin"),
            Some("omo")
        );
        assert_eq!(agent_identity("/bin/zsh"), None);
        assert_eq!(agent_identity("sh -c kimi"), None);
    }

    #[test]
    fn generic_entry_points_resolve_to_their_package_agent() {
        let senpi = "/g/node_modules/@code-yeongyu/senpi/dist/bundle/cli.js";
        assert_eq!(
            package_agent(&command_tokens(&format!(
                "bun {senpi} --extension /g/node_modules/omo-ai/plugin"
            ))),
            Some("omo")
        );
        assert_eq!(
            package_agent(&command_tokens(&format!("node {senpi}"))),
            Some("pi")
        );
        assert_eq!(
            package_agent(&command_tokens("node /g/node_modules/oh-my-pi/dist/cli.js")),
            Some("omp")
        );
        assert_eq!(
            package_agent(&command_tokens(r"node.exe C:\g\node_modules\omp\dist\index.js")),
            Some("omp")
        );
        assert!(is_agent(&format!("node {senpi}")));
        assert!(is_agent("node /g/node_modules/oh-my-pi/dist/cli.js"));
        assert_eq!(package_agent(&command_tokens("node /x/app/dist/cli.js")), None);
    }

    #[test]
    fn unknown_and_unrelated_processes_do_not_release() {
        let mut observer = ProcessTransition::default();
        assert_eq!(observer.observe(Some(Foreground::Shell)), None);
        assert_eq!(
            observer.observe(Some(Foreground::Agent { pid: 42, agent: "omo" })),
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
            observer.observe(Some(Foreground::Agent { pid: 42, agent: "omo" })),
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
            observer.observe(Some(Foreground::Agent { pid: 7, agent: "omo" })),
            Some(AgentProcessEdge::Observed)
        );
        assert_eq!(
            observer.observe(Some(Foreground::Agent { pid: 7, agent: "omo" })),
            None,
            "a still-running agent is not a new edge"
        );
        assert_eq!(
            observer.observe(Some(Foreground::Shell)),
            Some(AgentProcessEdge::Released)
        );
        assert_eq!(
            observer.observe(Some(Foreground::Agent { pid: 9, agent: "omo" })),
            Some(AgentProcessEdge::Observed)
        );
    }

    #[test]
    fn a_reporter_never_sampled_as_an_agent_has_no_release_edge() {
        // The live E2E gap: the reporter lived shorter than one tick (or was no agent), so the
        // observer only ever saw the shell. No Released edge exists; ProtocolReporters covers it.
        let mut observer = ProcessTransition::default();
        assert_eq!(observer.observe(Some(Foreground::Shell)), None);
        assert_eq!(observer.observe(Some(Foreground::Other)), None);
        assert_eq!(observer.observe(Some(Foreground::Shell)), None);
    }

    /// [`ProtocolReporters::foreground_returned`] for any observation; true when nothing is
    /// released.
    fn kept(
        reporters: &ProtocolReporters,
        session_id: &str,
        current: Option<Foreground>,
        group: Option<u32>,
        alone_after: Option<u64>,
        live: Option<u32>,
    ) -> bool {
        reporters
            .foreground_returned(session_id, current, group, alone_after, || live)
            .is_none()
    }

    /// [`ProtocolReporters::foreground_returned`] for a Shell observation of session `s`.
    fn shell_back(
        reporters: &ProtocolReporters,
        group: Option<u32>,
        alone_after: Option<u64>,
        live: Option<u32>,
    ) -> bool {
        reporters
            .foreground_returned("s", Some(Foreground::Shell), group, alone_after, || live)
            .is_some()
    }

    #[test]
    fn the_shell_regaining_the_foreground_from_a_reporter_releases_once() {
        let reporters = ProtocolReporters::default();
        reporters.reported("s", 1, true, Some(200));
        assert!(shell_back(&reporters, Some(100), None, Some(100)));
        assert!(
            !shell_back(&reporters, Some(100), Some(1), Some(100)),
            "one release per report"
        );
        assert!(
            kept(&reporters, "other", Some(Foreground::Shell), Some(100), Some(1), Some(100)),
            "a session that never reported has nothing to release"
        );
    }

    #[test]
    fn a_reporter_still_in_the_foreground_keeps_its_records() {
        let reporters = ProtocolReporters::default();
        reporters.reported("s", 1, true, Some(200));
        let live = Some(200);
        assert!(kept(&reporters, "s", Some(Foreground::Other), Some(200), None, live));
        let agent = Foreground::Agent { pid: 200, agent: "claude" };
        assert!(kept(&reporters, "s", Some(agent), Some(200), None, live));
        assert!(
            kept(&reporters, "s", None, Some(100), Some(1), live),
            "unknown is no evidence"
        );
        assert!(!shell_back(&reporters, None, None, Some(200)));
        // The shell was observed, but the reporter took the foreground again since: not gone.
        assert!(!shell_back(&reporters, Some(100), Some(1), Some(200)));
        assert!(
            shell_back(&reporters, Some(100), None, Some(100)),
            "none of the above consumed the pending release"
        );
    }

    /// Review N2: a report from the shell's own group is the shell's (a builtin, or a sourced
    /// function such as a `read` prompt that reported `blocked`). The shell is alive, so neither
    /// its prompt nor a listing showing it alone is evidence that the writer left.
    #[test]
    fn a_report_from_the_shell_itself_or_a_settled_record_is_not_released() {
        let reporters = ProtocolReporters::default();
        reporters.reported("s", 1, true, Some(100));
        assert!(!shell_back(&reporters, Some(100), None, Some(100)));
        assert!(
            !shell_back(&reporters, Some(100), Some(reporters.newest("s")), Some(100)),
            "a lone shell is still the shell that reported"
        );
        // done/error (or a clear) stay until acknowledged, so there is nothing to release.
        reporters.reported("s", 2, true, Some(200));
        reporters.reported("s", 3, false, Some(200));
        assert!(!shell_back(&reporters, Some(100), Some(3), Some(100)));
        // An unknown reporter group is no group evidence either.
        reporters.reported("s", 4, true, None);
        assert!(!shell_back(&reporters, Some(100), None, Some(100)));
        reporters.reported("gone", 5, true, Some(200));
        reporters.retain(|id| id != "gone");
        assert!(kept(&reporters, "gone", Some(Foreground::Shell), Some(100), Some(5), Some(100)));
    }

    /// Finding 3: reporter bookkeeping receives changes outside the mirror lock. A stale clear
    /// arriving after a newer transient report must not erase the reporter's group, and a stale
    /// report must not revive a group a release already consumed.
    #[test]
    fn reporter_groups_keep_the_newest_change_when_published_out_of_order() {
        let reporters = ProtocolReporters::default();
        reporters.reported("s", 7, true, Some(200));
        reporters.reported("s", 6, false, None);
        assert!(
            shell_back(&reporters, Some(100), None, Some(100)),
            "the newer transient report's group survived the stale clear"
        );
        reporters.reported("s", 7, true, Some(200));
        reporters.reported("s", 5, true, Some(300));
        assert!(
            !shell_back(&reporters, Some(100), Some(7), Some(100)),
            "a release is not re-armed by a change it already superseded"
        );
        reporters.reported("s", 8, true, Some(300));
        assert!(shell_back(&reporters, Some(100), None, Some(100)));
    }

    /// A report whose group is unknown (its marker could not be read when it was published)
    /// cannot be told apart from the shell while the shell's group is in front, so only a
    /// lone-shell listing taken without a group (ConPTY) can end it.
    #[test]
    fn a_report_without_attribution_is_released_only_by_a_lone_shell_without_a_group() {
        let reporters = ProtocolReporters::default();
        reporters.reported("s", 3, true, None);
        let requested = reporters.newest("s");
        assert_eq!(requested, 3);
        assert!(
            !shell_back(&reporters, Some(100), Some(requested), Some(100)),
            "with the shell's group in front, the writer may be the shell itself"
        );
        assert!(
            !shell_back(&reporters, None, None, None),
            "the shell in front with a child left is no exit evidence"
        );
        assert!(
            kept(&reporters, "s", Some(Foreground::Other), None, Some(requested), None),
            "only a Shell observation carries the shell-alone evidence"
        );
        assert!(
            kept(&reporters, "s", None, None, Some(requested), None),
            "unknown ownership is never an exit"
        );
        let shell = Some(Foreground::Shell);
        assert_eq!(
            reporters.foreground_returned("s", shell, None, Some(requested), || None),
            Some(requested)
        );
        assert!(
            !shell_back(&reporters, None, Some(requested), None),
            "one release per report"
        );
        // done/error is sticky: a lone shell never drops a settled record.
        reporters.reported("s", 4, false, None);
        assert!(!shell_back(&reporters, None, Some(4), None));
    }

    /// Review N3: the release carries the bound the mirror may drop through. A lone-shell
    /// listing covers what was applied before it was requested; a group edge covers the report.
    #[test]
    fn a_release_reports_the_newest_revision_its_evidence_covers() {
        let reporters = ProtocolReporters::default();
        reporters.reported("s", 4, true, Some(200));
        let shell = Some(Foreground::Shell);
        let edge = reporters.foreground_returned("s", shell, Some(100), None, || Some(100));
        assert_eq!(edge, Some(4));
        reporters.reported("s", 9, true, None);
        assert_eq!(reporters.foreground_returned("s", shell, None, Some(9), || None), Some(9));
    }

    /// ConPTY has no process group, so the report carries none and neither does the observation.
    /// The shell with no descendant in a listing requested after the report still ends it.
    #[test]
    fn a_windows_reporter_without_a_group_is_released_by_a_lone_shell() {
        let reporters = ProtocolReporters::default();
        reporters.reported("s", 1, true, None);
        assert!(
            !shell_back(&reporters, None, None, None),
            "no group and no shell-alone listing is no evidence"
        );
        assert!(
            kept(&reporters, "s", Some(Foreground::Other), None, Some(1), None),
            "a descendant still running keeps the report"
        );
        let requested = reporters.newest("s");
        assert!(shell_back(&reporters, None, Some(requested), None));
        // The next report is pending again, and only a listing requested after it releases it.
        reporters.reported("s", 2, true, None);
        assert!(!shell_back(&reporters, None, Some(requested), None));
        assert!(shell_back(&reporters, None, Some(reporters.newest("s")), None));
    }

    /// The listing is taken off the observer's lock. A program that starts, reports and is
    /// published after the listing was requested is not in that listing, so the stale lone-shell
    /// evidence must not release its fresh report. Only an unattributed report (ConPTY) can be
    /// released by a lone shell at all; a report from the shell's own group never is.
    #[test]
    fn a_stale_lone_shell_listing_never_releases_a_report_published_after_it() {
        let reporters = ProtocolReporters::default();
        reporters.reported("s", 1, true, None);
        let requested = reporters.newest("s");
        // The listing is in flight; a new program reports in the meantime.
        reporters.reported("s", 2, true, None);
        assert!(
            !shell_back(&reporters, None, Some(requested), None),
            "a listing requested before revision 2 cannot prove its program gone"
        );
        // A stale change arriving late changes nothing either.
        reporters.reported("s", 1, true, None);
        assert!(!shell_back(&reporters, None, Some(requested), None));
        assert!(shell_back(&reporters, None, Some(reporters.newest("s")), None));
        // A report from the shell's own group is never released by a lone shell, fresh or not.
        let reporters = ProtocolReporters::default();
        reporters.reported("s", 1, true, Some(100));
        assert!(!shell_back(&reporters, Some(100), Some(reporters.newest("s")), Some(100)));
        // With a group, a reporter that took the foreground after the listing also blocks it.
        let reporters = ProtocolReporters::default();
        reporters.reported("s", 1, true, Some(100));
        assert!(!shell_back(&reporters, Some(100), Some(1), Some(200)));
    }

    /// A short-lived reporter that exits before its bytes are parsed is attributed to the
    /// shell's group. The observer sampled its group before the report, so once a listing
    /// requested after the report shows the shell alone, the child is proven gone.
    #[test]
    fn a_report_attributed_to_the_shell_after_its_child_exited_is_released_by_a_lone_shell() {
        let reporters = ProtocolReporters::default();
        reporters.sampled("s", Some(100));
        assert!(!shell_back(&reporters, Some(100), None, Some(100)), "nothing pending yet");
        // The child's group held the foreground; it reported and exited before the parse.
        reporters.sampled("s", Some(200));
        reporters.reported("s", 1, true, Some(100));
        assert!(
            !shell_back(&reporters, Some(100), None, Some(100)),
            "the shell's prompt alone is no evidence"
        );
        assert!(
            !shell_back(&reporters, Some(100), Some(1), Some(200)),
            "a program that took the foreground since blocks the release"
        );
        assert_eq!(
            reporters.foreground_returned(
                "s",
                Some(Foreground::Shell),
                Some(100),
                Some(reporters.newest("s")),
                || Some(100),
            ),
            Some(1)
        );
        assert!(!shell_back(&reporters, Some(100), Some(1), Some(100)), "one release per report");
        // An unreadable group at publish after a child's hand-back is the child's too.
        reporters.sampled("s", Some(300));
        reporters.reported("s", 2, true, None);
        assert!(shell_back(&reporters, Some(100), Some(2), Some(100)));
    }

    /// A shell builtin (or a sourced `read` prompt) reports with no other group sampled since
    /// the previous sample: the shell that wrote it is alive, so a lone shell never releases it.
    #[test]
    fn a_shell_builtin_report_without_an_intervening_child_is_not_released() {
        let reporters = ProtocolReporters::default();
        reporters.sampled("s", Some(100));
        assert!(!shell_back(&reporters, Some(100), None, Some(100)));
        reporters.reported("s", 1, true, Some(100));
        assert!(!shell_back(&reporters, Some(100), Some(reporters.newest("s")), Some(100)));
        // A child ran and was sampled, then handed back and was sampled gone before the report.
        reporters.sampled("s", Some(200));
        reporters.sampled("s", Some(100));
        reporters.reported("s", 2, true, Some(100));
        assert!(!shell_back(&reporters, Some(100), Some(2), Some(100)));
        reporters.reported("s", 3, true, None);
        assert!(
            !shell_back(&reporters, Some(100), Some(3), Some(100)),
            "an unreadable group with the shell sampled last is still the shell's"
        );
        // Before the shell's group is known, a foreign sample proves nothing.
        let reporters = ProtocolReporters::default();
        reporters.sampled("s", Some(200));
        reporters.reported("s", 1, true, Some(100));
        assert!(!shell_back(&reporters, Some(100), Some(1), Some(100)));
    }

    /// A transient record that changed no summary re-arms the released entry under its own
    /// revision, so a later listing releases it; one requested before it still does not, and the
    /// summary change of the same feed (same revision) changes nothing.
    #[test]
    fn a_transient_record_that_changed_no_summary_stays_pending() {
        let reporters = ProtocolReporters::default();
        reporters.reported("s", 1, true, None);
        assert!(shell_back(&reporters, None, Some(1), None));
        reporters.transient_applied("s", 2, None);
        assert!(!shell_back(&reporters, None, Some(1), None), "requested before revision 2");
        assert!(shell_back(&reporters, None, Some(2), None));
        assert!(!shell_back(&reporters, None, Some(2), None), "released once");
        // The feed's own summary change arrives after it and is ignored; a stale one too.
        reporters.transient_applied("s", 3, Some(200));
        reporters.reported("s", 3, false, None);
        reporters.transient_applied("s", 2, None);
        assert_eq!(reporters.newest("s"), 3);
        assert!(!shell_back(&reporters, Some(200), None, Some(200)), "still its group");
        assert!(shell_back(&reporters, Some(100), None, Some(100)), "the group left");
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
        // foreground exactly once it has printed its prompt. The marker is split in the
        // command so the echoed command line cannot match it before `stty` has run: while
        // `stty` runs it, not the shell, owns the foreground group, which a listing taken
        // before it forked does not contain.
        for (session, output) in [(&agent, &mut agent_output), (&shell, &mut shell_output)] {
            session
                .write_input(b"stty -echo; PS1='SHELL_''READY>'\n")
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
            matches!(classify(&agent), Some(Foreground::Agent { .. })),
            "the tick's listing must still see the agent in front of its own pane"
        );
        assert_eq!(classify(&shell), Some(Foreground::Shell));
        // The same listing keeps serving the rest of the tick's sessions.
        assert!(matches!(classify(&agent), Some(Foreground::Agent { .. })));
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
