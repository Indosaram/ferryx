//! Which agent *owns* a pane, decided from the process table rather than from what the agent
//! reports about itself.
//!
//! A pane's agent can report its own identity over the wire, and that report is what a reload
//! replays. When the report is wrong the pane keeps the wrong label forever, so ownership is
//! re-derived here from the process that the pane's PTY actually runs: `argv[0]` names the program,
//! and a shell or an interpreter never does (its command text is data, not an entry point).
//!
//! Two properties matter and are both tested:
//!
//! - **Anchoring.** Only `argv[0]` (or, for an interpreter, `argv[1]`) is ever evidence. A path
//!   mentioned later on the command line is an argument, and matching it would let
//!   `python wrapper.py --config /x/omo.js` claim to be omo.
//! - **Own-name precedence.** A program's own name beats a compatibility alias, and the scan over
//!   `command_names` is exhaustive *before* any `launch_variants` list is consulted. `mhc` is both
//!   maho's command name and omo's launcher variant; a variant-first scan would answer omo.

use crate::agent_detect::registry;

/// What a single process's argv identifies it as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classified {
    Primary(&'static str),
    LauncherVariant(&'static str),
}

/// The owner of a pane, as proved by its process tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerProof {
    Mhc,
    Other(&'static str),
    None,
}

/// Interpreters whose `argv[1]` is the real entry point. A name is only ever evidence *after* the
/// interpreter prefix, so this list decides where phase 2 starts, never who the agent is.
const INTERPRETERS: &[&str] = &[
    "bun", "node", "node.exe", "deno", "python", "py", "python.exe", "python3", "python3.exe",
];

/// Shells never classify: their command text is data the operator typed, so a shell whose argument
/// happens to name an agent does not make that agent the pane's owner.
const SHELLS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "dash",
    "ksh",
    "fish",
    "cmd",
    "cmd.exe",
    "powershell",
    "powershell.exe",
    "pwsh",
    "pwsh.exe",
];

/// Lowercase and unify separators so `node.exe C:\Users\x\omo.js` compares like its Unix form.
fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('\\', "/")
}

fn basename(token: &str) -> &str {
    let normalized = token.rsplit('/').next().unwrap_or(token);
    normalized.strip_suffix(".exe").unwrap_or(normalized)
}

fn is_interpreter(name: &str) -> bool {
    INTERPRETERS.contains(&name)
        || name
            .strip_prefix("python3.")
            .is_some_and(|patch| !patch.is_empty() && patch.bytes().all(|b| b.is_ascii_digit()))
}

fn is_shell(name: &str) -> bool {
    SHELLS.contains(&name)
}

/// Split a command line into tokens, honouring quotes so a quoted path with spaces stays one
/// token. Only the first two are consumed, but the split must be right for them.
fn tokenize(args: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for ch in args.chars() {
        match quote {
            Some(active) if ch == active => quote = None,
            Some(_) => current.push(ch),
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
                started = true;
            }
            None if ch.is_whitespace() => {
                if started {
                    tokens.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            None => {
                current.push(ch);
                started = true;
            }
        }
        if tokens.len() == 2 {
            break;
        }
    }
    if started && tokens.len() < 2 {
        tokens.push(current);
    }
    tokens
}

/// Rule 3: classify one program name, by the registry alone. `command_names` is exhausted before
/// any `launch_variants` list is read, so a program's own name always wins over a compatibility
/// alias.
fn classify_name(name: &str) -> Option<Classified> {
    let normalized = normalize(name);
    let name = basename(&normalized);
    if name.is_empty() || is_shell(name) {
        return None;
    }
    for spec in registry::AGENTS {
        if spec
            .command_names
            .iter()
            .any(|candidate| *candidate == name)
        {
            return Some(Classified::Primary(spec.id));
        }
    }
    for spec in registry::AGENTS {
        if spec
            .launch_variants
            .iter()
            .any(|(_, binary)| basename(&normalize(binary)) == name)
        {
            return Some(Classified::LauncherVariant(spec.id));
        }
    }
    None
}

/// Rule 5: classify an interpreter's entry token. The token is matched by basename through rule 3
/// first, then by the registry's `argv_markers`, and the marker match is a **suffix** on the whole
/// entry token - never a `contains` over the command line.
fn classify_entry(token: &str) -> Option<Classified> {
    let normalized = normalize(token);
    if let Some(classified) = classify_name(&normalized) {
        return Some(classified);
    }
    for spec in registry::AGENTS {
        if spec
            .argv_markers
            .iter()
            .any(|marker| normalized.ends_with(&normalize(marker)))
        {
            return Some(Classified::Primary(spec.id));
        }
    }
    None
}

/// Classify one process's command line. `None` means this process names no agent.
pub fn classify_agent_process(args: &str) -> Option<Classified> {
    let tokens = tokenize(args);
    let first = tokens.first()?;
    let normalized = normalize(first);
    let name = basename(&normalized);
    if name.is_empty() || is_shell(name) {
        return None;
    }
    if !is_interpreter(name) {
        return classify_name(name);
    }
    // Phase 2: the entry point is `argv[1]` and only `argv[1]`. A leading `-` means inline code or
    // options, where "the first non-flag token" would select a flag's value and invent an identity.
    let entry = tokens.get(1)?;
    if entry.starts_with('-') {
        return None;
    }
    classify_entry(entry)
}

/// Classify a pane's owner from a process table, breadth-first from the PTY root.
///
/// The first level that yields any classification decides: a single primary wins, a launcher
/// variant is remembered but not decisive (a launcher can host the real agent below it), and two
/// distinct primaries at the same level mean no proof at all. Deeper levels never override a
/// primary found above them, so an `mhc` child can never relabel a genuine `omo` parent.
pub fn owner_proof(entries: &[(u32, u32, String)], pty_pid: u32) -> OwnerProof {
    use std::collections::{HashMap, HashSet, VecDeque};

    let mut children = HashMap::<u32, Vec<u32>>::new();
    let mut args_by_pid = HashMap::<u32, &str>::new();
    for (pid, ppid, args) in entries {
        children.entry(*ppid).or_default().push(*pid);
        args_by_pid.insert(*pid, args.as_str());
    }

    // The PTY root is level 0. A root that is itself the agent counts: a pane can run `mhc`
    // directly, with no shell in between.
    let mut level = VecDeque::from([pty_pid]);
    let mut visited = HashSet::from([pty_pid]);
    let mut remembered: Option<Classified> = None;
    while !level.is_empty() {
        let mut primaries: Vec<&'static str> = Vec::new();
        let mut next = VecDeque::new();
        for pid in level {
            if let Some(args) = args_by_pid.get(&pid) {
                match classify_agent_process(args) {
                    Some(Classified::Primary(id)) if !primaries.contains(&id) => primaries.push(id),
                    Some(Classified::Primary(_)) => {}
                    Some(variant @ Classified::LauncherVariant(_)) => {
                        remembered.get_or_insert(variant);
                    }
                    None => {}
                }
            }
            if let Some(descendants) = children.get(&pid) {
                for child in descendants {
                    if visited.insert(*child) {
                        next.push_back(*child);
                    }
                }
            }
        }
        if primaries.len() == 1 {
            let id = primaries[0];
            return if id == "maho" {
                OwnerProof::Mhc
            } else {
                OwnerProof::Other(id)
            };
        }
        // Two distinct primaries at the winning level: the pane has no single owner.
        if primaries.len() > 1 {
            return OwnerProof::None;
        }
        level = next;
    }
    // Only launcher variants were ever seen. A variant names the program that hosts the agent, and
    // the only variant in the table is maho's `mhc`, so it proves maho.
    match remembered {
        Some(Classified::LauncherVariant("maho")) | Some(Classified::Primary("maho")) => {
            OwnerProof::Mhc
        }
        Some(Classified::LauncherVariant(other)) | Some(Classified::Primary(other)) => {
            OwnerProof::Other(other)
        }
        None => OwnerProof::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[(u32, u32, &str)]) -> Vec<(u32, u32, String)> {
        rows.iter()
            .map(|(pid, ppid, args)| (*pid, *ppid, (*args).to_string()))
            .collect()
    }

    #[test]
    fn mhc_as_argv0_is_maho_not_an_omo_variant() {
        // `mhc` is in omo's launch_variants AND maho's command_names. Own-name precedence decides.
        assert_eq!(
            classify_agent_process("mhc --session abc"),
            Some(Classified::Primary("maho"))
        );
        assert_eq!(
            classify_agent_process("/usr/local/bin/mhc"),
            Some(Classified::Primary("maho"))
        );
        assert_eq!(
            classify_agent_process("MHC.EXE"),
            Some(Classified::Primary("maho"))
        );
    }

    #[test]
    fn interpreter_entry_is_argv1_only() {
        assert_eq!(
            classify_agent_process("node /usr/local/bin/mhc --resume x"),
            Some(Classified::Primary("maho"))
        );
        assert_eq!(
            classify_agent_process("node /x/omo.js"),
            Some(Classified::Primary("omo"))
        );
        // A path in a later argument is data, not an entry point.
        assert_eq!(
            classify_agent_process("python wrapper.py --config /tmp/omo.js"),
            None
        );
        // Inline code and option-first entry points name nothing.
        assert_eq!(classify_agent_process("node -e 'x'"), None);
        assert_eq!(classify_agent_process("node --require /x/omo.js /a.js"), None);
    }

    #[test]
    fn marker_match_is_a_suffix_not_a_contains() {
        assert_eq!(
            classify_agent_process("node /x/pi-coding-agent"),
            Some(Classified::Primary("pi"))
        );
        assert_eq!(classify_agent_process("/x/pi-coding-agent/helper.js"), None);
        assert_eq!(
            classify_agent_process("python wrapper.py --config /x/pi-coding-agent"),
            None
        );
        assert_eq!(
            classify_agent_process("node /x/gjc.js"),
            Some(Classified::Primary("gjc"))
        );
        assert_eq!(
            classify_agent_process("python x.py --config /x/gjc.js"),
            None
        );
    }

    #[test]
    fn shells_never_classify() {
        assert_eq!(classify_agent_process("zsh"), None);
        assert_eq!(classify_agent_process("bash -c 'mhc'"), None);
        assert_eq!(classify_agent_process("-zsh"), None);
    }

    #[test]
    fn quoted_paths_with_spaces_stay_one_token() {
        assert_eq!(
            classify_agent_process("\"/Applications/My App/bin/mhc\" --session x"),
            Some(Classified::Primary("maho"))
        );
    }

    #[test]
    fn root_pane_running_mhc_directly_is_maho() {
        let entries = table(&[(100, 1, "mhc --session abc")]);
        assert_eq!(owner_proof(&entries, 100), OwnerProof::Mhc);
    }

    #[test]
    fn shell_between_pty_and_mhc_still_proves_maho() {
        let entries = table(&[(100, 1, "zsh"), (200, 100, "mhc --session abc")]);
        assert_eq!(owner_proof(&entries, 100), OwnerProof::Mhc);
    }

    #[test]
    fn omo_parent_with_mhc_child_stays_omo() {
        // Different levels: the higher primary wins, so an mhc descendant cannot relabel omo.
        let entries = table(&[(100, 1, "node /x/omo.js"), (200, 100, "mhc --session abc")]);
        assert_eq!(owner_proof(&entries, 100), OwnerProof::Other("omo"));
    }

    #[test]
    fn two_primaries_at_the_same_level_prove_nothing() {
        let entries = table(&[
            (100, 1, "zsh"),
            (200, 100, "node /x/omo.js"),
            (300, 100, "mhc --session abc"),
        ]);
        assert_eq!(owner_proof(&entries, 100), OwnerProof::None);
    }

    #[test]
    fn a_wrapper_above_mhc_still_proves_maho() {
        // No primary at the pane's own level; the child still names `mhc`, which is maho.
        let entries = table(&[(100, 1, "zsh"), (200, 100, "mhc")]);
        assert_eq!(owner_proof(&entries, 100), OwnerProof::Mhc);
    }

    #[test]
    fn empty_table_or_lost_pid_proves_nothing() {
        assert_eq!(owner_proof(&[], 100), OwnerProof::None);
        assert_eq!(owner_proof(&table(&[(200, 100, "mhc")]), 999), OwnerProof::None);
    }
}
