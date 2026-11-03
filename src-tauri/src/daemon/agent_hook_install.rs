//! Installs the `ferryx agent-hook` reporter into the agents that run a shell command per
//! lifecycle hook.
//!
//! Five config shapes were measured, one per [`HookShape`]: claude's group arrays (claude, codex
//! and grok), agy's object of named hook groups, crush's named entry list, cursor's bare command
//! list, and cline's directory of one executable script per event.
//!
//! These files belong to the user, so they are merged into, never replaced: each hook event gets
//! one Ferryx-owned matcher group appended to its array, and every other group (the user's own,
//! orca's, herdr's) is left exactly as found. A group is Ferryx-owned only when it has precisely
//! the shape Ferryx writes - `{"hooks":[{"type":"command","command":"<ferryx> agent-hook <agent> <event>"}]}`
//! with nothing added - where `<ferryx>` is a quoted path to a `ferryx` executable. The executable
//! path is the one part allowed to differ, because the daemon runs from a generation-keyed staged
//! copy whose path changes on every update; an install replaces an older generation's entry, and a
//! removal deletes it. A group the user edited in any other way is no longer ours and survives both.
//!
//! Alongside the hooks, claude and codex get the Ferryx agent skill (`SKILL.md`, see
//! [`SKILL_FILES`]). It is Ferryx's while its frontmatter says `ferryx-managed: true`; a skill file
//! without that line belongs to the user and is never overwritten or deleted.

use serde_json::{Map, Value};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::cli::agent_hook::{AGENT_HOOK_SUBCOMMAND, HOOK_AGENTS};

/// How one agent spells the hook entries Ferryx writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookShape {
    /// `{"hooks": {"<Event>": [{"hooks": [{"type": "command", "command": …}]}]}}`, the Claude
    /// schema that codex and grok also read (facts/local-1.md, remote-grok.md section 5).
    ClaudeGroups,
    /// An object keyed by hook-group name, one group per owner:
    /// `{"<group>": {"enabled": true, "<Event>": [{"type": "command", "command": …, "timeout": …}]}}`
    /// (agy, facts/local-2.md).
    GroupKeyedObject,
    /// `{"hooks": {"<Event>": [{"name": …, "matcher": …, "command": …, "timeout": …}]}}`
    /// (crush, facts/remote-crush.md section 4a).
    NamedEntries,
    /// `{"version": 1, "hooks": {"<event>": [{"command": …}]}}` (cursor, facts/local-3.md:193 and
    /// cursor-hooks-live-2026-10-23.md).
    CommandEntries,
    /// A directory holding one executable script per event, named after it (cline,
    /// facts/local-3.md). Here `file` is that directory, not a file.
    EventScripts,
}

/// One agent settings file Ferryx merges its hook entries into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookFileTarget {
    pub agent: &'static str,
    /// The agent's own config directory: an install happens only where the agent already has one.
    pub agent_dir: PathBuf,
    /// What Ferryx writes: the agent's hook config file, or for [`HookShape::EventScripts`] the
    /// directory Ferryx writes one script per event into.
    pub file: PathBuf,
    pub events: &'static [&'static str],
    pub shape: HookShape,
    /// Where the Ferryx agent skill goes for this agent, when it reads one ([`SKILL_FILES`]).
    pub skill: Option<PathBuf>,
}

/// Each hook-style agent's config directory, the path Ferryx writes inside it, and that file's
/// shape, all as measured in `.omo-parity/facts/`.
///
/// - claude reads `hooks` from `settings.json`, and codex the same object from `hooks.json`
///   (local-1.md).
/// - grok loads every file in `~/.grok/hooks/` and trusts it unconditionally, so Ferryx owns one
///   file of its own there instead of merging into a shared one (remote-grok.md section 5).
/// - agy reads `~/.gemini/config/hooks.json` (local-2.md). The directory gate is `~/.gemini`,
///   which gemini also owns; only the agy group inside the file is ever touched.
/// - cline's default hook directory is `~/.cline/hooks` (local-3.md).
/// - crush's config directory is `$XDG_CONFIG_HOME/crush`, falling back to `~/.config/crush`;
///   the `.config/` prefix below is what marks that (explore/detect.md:79).
/// - cursor-agent reads user hooks from `~/.cursor/hooks.json` (local-3.md:193, and the live probe
///   in cursor-hooks-live-2026-10-23.md, which fired from that file).
pub const HOOK_FILES: [(&str, &str, &str, HookShape); 7] = [
    ("claude", ".claude", "settings.json", HookShape::ClaudeGroups),
    ("codex", ".codex", "hooks.json", HookShape::ClaudeGroups),
    ("grok", ".grok", "hooks/ferryx.json", HookShape::ClaudeGroups),
    ("antigravity", ".gemini", "config/hooks.json", HookShape::GroupKeyedObject),
    ("cline", ".cline", "hooks", HookShape::EventScripts),
    ("crush", ".config/crush", "crush.json", HookShape::NamedEntries),
    ("cursor", ".cursor", "hooks.json", HookShape::CommandEntries),
];

/// The Ferryx agent skill, embedded so the headless `ferryx-cli` needs no resource directory.
pub(crate) const SKILL_SOURCE: &str = include_str!("../../resources/agent-skills/ferryx/SKILL.md");

/// Each skill-reading agent and where its user skill lives, `/`-separated and relative to HOME:
/// claude reads `~/.claude/skills`, codex reads the shared user skill directory `~/.agents/skills`.
/// The skill is installed only where that agent's own config directory exists.
pub const SKILL_FILES: [(&str, &str); 2] = [
    ("claude", ".claude/skills/ferryx/SKILL.md"),
    ("codex", ".agents/skills/ferryx/SKILL.md"),
];

/// Whether a skill file is Ferryx's: its YAML frontmatter carries `ferryx-managed: true`.
pub(crate) fn is_ferryx_skill(text: &str) -> bool {
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return false;
    }
    lines
        .map(str::trim_end)
        .take_while(|line| *line != "---")
        .any(|line| line.trim() == "ferryx-managed: true")
}

pub fn hook_targets() -> Vec<HookFileTarget> {
    hook_targets_with_env(|key| std::env::var_os(key))
}

pub(crate) fn hook_targets_with_env(get_env: impl Fn(&str) -> Option<OsString>) -> Vec<HookFileTarget> {
    let Some(home) = get_env("HOME")
        .filter(|value| !value.is_empty())
        .or_else(|| get_env("USERPROFILE").filter(|value| !value.is_empty()))
        .map(PathBuf::from)
    else {
        return Vec::new();
    };
    let config_home = get_env("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    HOOK_FILES
        .iter()
        .filter_map(|(agent, dir, file, shape)| {
            let events = HOOK_AGENTS.iter().find(|(name, _)| name == agent)?.1;
            let agent_dir = match dir.strip_prefix(".config/") {
                Some(rest) => config_home.join(rest),
                None => home.join(dir),
            };
            let skill = SKILL_FILES
                .iter()
                .find(|(name, _)| name == agent)
                .map(|(_, rel)| rel.split('/').fold(home.clone(), |path, part| path.join(part)));
            Some(HookFileTarget {
                agent,
                file: agent_dir.join(file),
                agent_dir,
                events,
                shape: *shape,
                skill,
            })
        })
        .collect()
}

/// Quotes the executable for the shell the agent runs hook commands through on a host of the
/// given platform. It is not necessarily this one: the SSH integration writes hook commands for a
/// remote host from the desktop.
fn quote_executable_for(path: &str, windows: bool) -> String {
    if windows {
        format!("\"{path}\"")
    } else {
        format!("'{}'", path.replace('\'', "'\\''"))
    }
}

/// The exact command Ferryx installs for one event.
pub fn hook_command(exe: &Path, agent: &str, event: &str) -> String {
    hook_command_for(&exe.to_string_lossy(), cfg!(windows), agent, event)
}

/// [`hook_command`] for an executable on a host of the given platform.
pub(crate) fn hook_command_for(exe: &str, windows: bool, agent: &str, event: &str) -> String {
    format!("{} {AGENT_HOOK_SUBCOMMAND} {agent} {event}", quote_executable_for(exe, windows))
}

/// The reporter a hook command names: its executable and the platform of the host it runs on.
/// Locally that is this daemon's own executable; for an SSH host it is the staged remote helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reporter {
    pub exe: String,
    pub windows: bool,
}

impl Reporter {
    pub(crate) fn local(exe: &Path) -> Self {
        Self { exe: exe.to_string_lossy().into_owned(), windows: cfg!(windows) }
    }

    pub(crate) fn command(&self, agent: &str, event: &str) -> String {
        hook_command_for(&self.exe, self.windows, agent, event)
    }
}

fn unquote_executable(quoted: &str) -> Option<String> {
    if let Some(inner) = quoted.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) {
        return (!inner.contains('"')).then(|| inner.to_string());
    }
    let inner = quoted.strip_prefix('\'')?.strip_suffix('\'')?;
    let unescaped = inner.replace("'\\''", "'");
    (!unescaped.is_empty()).then_some(unescaped)
}

/// The event a Ferryx-installed command reports for `agent`, or `None` when the command is not one
/// Ferryx writes.
fn ferryx_command_event<'a>(command: &'a str, agent: &str) -> Option<&'a str> {
    let marker = format!(" {AGENT_HOOK_SUBCOMMAND} {agent} ");
    let split = command.rfind(&marker)?;
    let event = &command[split + marker.len()..];
    if event.is_empty() || !event.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let exe = unquote_executable(&command[..split])?;
    // Split on both separators: the desktop recognises entries it wrote for a remote host of the
    // other platform, and `Path` only knows this one's.
    let name = exe.rsplit(['/', '\\']).next()?;
    // `ferryx-remote-helper` is the reporter the SSH integration installs on a remote host
    // (`ssh/remote_hooks.rs`); it runs the same `agent-hook` code.
    matches!(
        name,
        "ferryx" | "ferryx.exe" | "ferryx-remote-helper" | "ferryx-remote-helper.exe"
    )
    .then_some(event)
}

/// The command of a matcher group that has exactly the shape Ferryx writes and nothing more.
fn ferryx_group_command<'a>(group: &'a Value, agent: &str) -> Option<&'a str> {
    let group = group.as_object()?;
    if group.len() != 1 {
        return None;
    }
    let [entry] = group.get("hooks")?.as_array()?.as_slice() else {
        return None;
    };
    let entry = entry.as_object()?;
    if entry.len() != 2 || entry.get("type")?.as_str()? != "command" {
        return None;
    }
    let command = entry.get("command")?.as_str()?;
    ferryx_command_event(command, agent).map(|_| command)
}

fn ferryx_group(command: String) -> Value {
    let mut entry = Map::new();
    entry.insert("type".into(), Value::from("command"));
    entry.insert("command".into(), Value::from(command));
    let mut group = Map::new();
    group.insert("hooks".into(), Value::Array(vec![Value::Object(entry)]));
    Value::Object(group)
}

fn hooks_object(root: &mut Value, create: bool) -> Result<Option<&mut Map<String, Value>>, String> {
    let Some(root) = root.as_object_mut() else {
        return Err("the settings file is not a JSON object".into());
    };
    if !root.contains_key("hooks") {
        if !create {
            return Ok(None);
        }
        root.insert("hooks".into(), Value::Object(Map::new()));
    }
    match root.get_mut("hooks") {
        Some(Value::Object(hooks)) => Ok(Some(hooks)),
        _ => Err("the settings file's \"hooks\" is not a JSON object".into()),
    }
}

/// Recognises one Ferryx-owned entry in an event array and returns the command it carries.
type OwnedEntry = for<'a> fn(&'a Value, &str) -> Option<&'a str>;
/// Builds the entry Ferryx writes for one command.
type BuildEntry = fn(String) -> Value;
/// Writes one agent's entries into its parsed config file.
type MergeShape = fn(&mut Value, &str, &[&str], &Reporter) -> Result<(), String>;
/// Deletes one agent's entries from its parsed config file, returning how many went.
type StripShape = fn(&mut Value, &str) -> Result<usize, String>;

/// Brings `root` to exactly one current Ferryx entry per event, touching nothing else.
fn merge_entries(
    root: &mut Value,
    agent: &str,
    events: &[&str],
    reporter: &Reporter,
    owned: OwnedEntry,
    build: BuildEntry,
) -> Result<(), String> {
    let hooks = hooks_object(root, true)?.expect("created on demand");
    for event in events {
        if !matches!(hooks.get(*event), None | Some(Value::Array(_))) {
            return Err(format!("the settings file's \"hooks.{event}\" is not an array"));
        }
    }
    // Stale entries go first: another generation's executable, or an event no longer installed.
    for (event, groups) in hooks.iter_mut() {
        let Value::Array(groups) = groups else { continue };
        let current = reporter.command(agent, event);
        let wanted = events.contains(&event.as_str());
        let mut kept_current = false;
        groups.retain(|group| match owned(group, agent) {
            None => true,
            Some(command) if wanted && command == current && !kept_current => {
                kept_current = true;
                true
            }
            Some(_) => false,
        });
    }
    for event in events {
        let current = reporter.command(agent, event);
        let groups = hooks
            .entry(event.to_string())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .expect("checked above");
        if !groups.iter().any(|group| owned(group, agent) == Some(current.as_str())) {
            groups.push(build(current));
        }
    }
    Ok(())
}

pub(crate) fn merge_hooks(
    root: &mut Value,
    agent: &str,
    events: &[&str],
    reporter: &Reporter,
) -> Result<(), String> {
    merge_entries(root, agent, events, reporter, ferryx_group_command, ferryx_group)
}

/// Deletes every Ferryx-owned entry for `agent` and returns how many went. An event array that
/// only held Ferryx entries is removed with them; every other key is kept.
fn strip_entries(root: &mut Value, agent: &str, owned: OwnedEntry) -> Result<usize, String> {
    let Some(hooks) = hooks_object(root, false)? else {
        return Ok(0);
    };
    let mut removed = 0;
    let mut emptied = Vec::new();
    for (event, groups) in hooks.iter_mut() {
        let Value::Array(groups) = groups else { continue };
        let before = groups.len();
        groups.retain(|group| owned(group, agent).is_none());
        if groups.len() < before {
            removed += before - groups.len();
            if groups.is_empty() {
                emptied.push(event.clone());
            }
        }
    }
    // `retain` keeps the user's key order, where a per-key remove may swap it.
    hooks.retain(|event, _| !emptied.contains(event));
    Ok(removed)
}

pub(crate) fn strip_hooks(root: &mut Value, agent: &str) -> Result<usize, String> {
    strip_entries(root, agent, ferryx_group_command)
}

/// The name Ferryx puts on the entries it owns in the shapes that name them.
pub(crate) const FERRYX_ENTRY_NAME: &str = "ferryx-agent-state";
/// The reporter connects, writes one line and exits, with its own one-second cap, so the agent's
/// turn never waits on it for longer than this.
const HOOK_TIMEOUT_SECONDS: u64 = 5;

// --- agy: an object of named hook groups (HookShape::GroupKeyedObject) ------------------------

fn group_object_entry(command: String) -> Value {
    let mut entry = Map::new();
    entry.insert("type".into(), Value::from("command"));
    entry.insert("command".into(), Value::from(command));
    entry.insert("timeout".into(), Value::from(HOOK_TIMEOUT_SECONDS));
    Value::Object(entry)
}

fn ferryx_group_object(agent: &str, events: &[&str], reporter: &Reporter) -> Value {
    let mut group = Map::new();
    group.insert("enabled".into(), Value::Bool(true));
    for event in events {
        group.insert(
            (*event).to_string(),
            Value::Array(vec![group_object_entry(reporter.command(agent, event))]),
        );
    }
    Value::Object(group)
}

/// Whether the group under Ferryx's key still has exactly the shape Ferryx writes. Only the
/// executable path and the event set may differ, so an older generation's group is still ours.
fn is_ferryx_group_object(group: &Value, agent: &str) -> bool {
    let Some(group) = group.as_object() else { return false };
    let mut events = 0;
    for (key, value) in group {
        if key == "enabled" {
            if value != &Value::Bool(true) {
                return false;
            }
            continue;
        }
        let Some(entries) = value.as_array() else { return false };
        let [entry] = entries.as_slice() else { return false };
        let Some(entry) = entry.as_object() else { return false };
        if entry.len() != 3 || entry.get("type").and_then(Value::as_str) != Some("command") {
            return false;
        }
        let Some(command) = entry.get("command").and_then(Value::as_str) else { return false };
        // The command must report the very event it is filed under.
        if ferryx_command_event(command, agent) != Some(key.as_str()) {
            return false;
        }
        events += 1;
    }
    events > 0
}

/// Replaces Ferryx's own group and nothing else. A group the user edited is no longer ours, so it
/// survives an install untouched rather than being overwritten.
pub(crate) fn merge_group_object(
    root: &mut Value,
    agent: &str,
    events: &[&str],
    reporter: &Reporter,
) -> Result<(), String> {
    let Some(root) = root.as_object_mut() else {
        return Err("the hooks file is not a JSON object".into());
    };
    if root.get(FERRYX_ENTRY_NAME).is_some_and(|group| !is_ferryx_group_object(group, agent)) {
        return Ok(());
    }
    root.insert(FERRYX_ENTRY_NAME.into(), ferryx_group_object(agent, events, reporter));
    Ok(())
}

pub(crate) fn strip_group_object(root: &mut Value, agent: &str) -> Result<usize, String> {
    let Some(root) = root.as_object_mut() else {
        return Err("the hooks file is not a JSON object".into());
    };
    let Some(group) = root.get(FERRYX_ENTRY_NAME) else { return Ok(0) };
    if !is_ferryx_group_object(group, agent) {
        return Ok(0);
    }
    let removed = group
        .as_object()
        .map_or(0, |group| group.keys().filter(|key| *key != "enabled").count());
    root.remove(FERRYX_ENTRY_NAME);
    Ok(removed)
}

// --- cursor: a bare command list per event (HookShape::CommandEntries) ------------------------

/// The schema version cursor's hooks file carries (`{"hooks":{},"version":1}`, local-3.md:193).
const CURSOR_HOOKS_VERSION: u64 = 1;

fn command_entry(command: String) -> Value {
    let mut entry = Map::new();
    entry.insert("command".into(), Value::from(command));
    Value::Object(entry)
}

/// An entry is ours only when it is exactly `{"command": <reporter command>}`.
fn ferryx_command_entry<'a>(entry: &'a Value, agent: &str) -> Option<&'a str> {
    let entry = entry.as_object()?;
    if entry.len() != 1 {
        return None;
    }
    let command = entry.get("command")?.as_str()?;
    ferryx_command_event(command, agent).map(|_| command)
}

/// Merges beside the user's entries. A file without `version` gets cursor's `1`; an existing
/// `version` is never changed.
pub(crate) fn merge_command_entries(
    root: &mut Value,
    agent: &str,
    events: &[&str],
    reporter: &Reporter,
) -> Result<(), String> {
    merge_entries(root, agent, events, reporter, ferryx_command_entry, command_entry)?;
    if let Some(object) = root.as_object_mut() {
        object
            .entry("version")
            .or_insert_with(|| Value::from(CURSOR_HOOKS_VERSION));
    }
    Ok(())
}

pub(crate) fn strip_command_entries(root: &mut Value, agent: &str) -> Result<usize, String> {
    strip_entries(root, agent, ferryx_command_entry)
}

// --- crush: a list of named entries per event (HookShape::NamedEntries) -----------------------

fn named_entry(command: String) -> Value {
    let mut entry = Map::new();
    entry.insert("name".into(), Value::from(FERRYX_ENTRY_NAME));
    // An empty matcher means every tool, which is what a state report needs.
    entry.insert("matcher".into(), Value::from(""));
    entry.insert("command".into(), Value::from(command));
    entry.insert("timeout".into(), Value::from(HOOK_TIMEOUT_SECONDS));
    Value::Object(entry)
}

fn ferryx_named_entry<'a>(entry: &'a Value, agent: &str) -> Option<&'a str> {
    let entry = entry.as_object()?;
    if entry.len() != 4 || entry.get("name")?.as_str()? != FERRYX_ENTRY_NAME {
        return None;
    }
    if !entry.get("matcher")?.as_str()?.is_empty() {
        return None;
    }
    let command = entry.get("command")?.as_str()?;
    ferryx_command_event(command, agent).map(|_| command)
}

pub(crate) fn merge_named_entries(
    root: &mut Value,
    agent: &str,
    events: &[&str],
    reporter: &Reporter,
) -> Result<(), String> {
    merge_entries(root, agent, events, reporter, ferryx_named_entry, named_entry)
}

/// Unlike the claude shape, an emptied `hooks` object goes too: crush users keep other settings in
/// the same file, and `hooks` is a key Ferryx may have created.
pub(crate) fn strip_named_entries(root: &mut Value, agent: &str) -> Result<usize, String> {
    let removed = strip_entries(root, agent, ferryx_named_entry)?;
    if removed > 0 {
        if let Some(object) = root.as_object_mut() {
            if object.get("hooks").and_then(Value::as_object).is_some_and(Map::is_empty) {
                object.remove("hooks");
            }
        }
    }
    Ok(removed)
}

/// The file actually written: a symlinked settings file (a dotfiles checkout) is updated in place
/// at its target rather than replaced by a regular file.
fn resolve_write_path(file: &Path) -> std::io::Result<PathBuf> {
    match std::fs::symlink_metadata(file) {
        Ok(metadata) if metadata.file_type().is_symlink() => std::fs::canonicalize(file),
        _ => Ok(file.to_path_buf()),
    }
}

fn read_settings(path: &Path) -> std::io::Result<Option<Value>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    parse_settings(&text).map(Some).map_err(invalid)
}

/// An empty file counts as an empty object, as the agents themselves treat it.
fn parse_settings(text: &str) -> Result<Value, String> {
    if text.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_str(text).map_err(|error| error.to_string())
}

/// The bytes Ferryx writes for a settings file: pretty JSON and a final newline.
fn render_settings(value: &Value) -> Result<String, String> {
    let mut text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    text.push('\n');
    Ok(text)
}

fn write_settings(path: &Path, value: &Value) -> std::io::Result<()> {
    let text = render_settings(value).map_err(invalid)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "settings.json".into());
    let tmp = path.with_file_name(format!(".{name}.ferryx.tmp"));
    std::fs::write(&tmp, text)?;
    if let Ok(metadata) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, metadata.permissions());
    }
    std::fs::rename(&tmp, path)
}

fn invalid(message: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

// --- cline: one executable script per event (HookShape::EventScripts) -------------------------

/// The two lines that mark a script as Ferryx's, so a user's own hook for the same event is never
/// mistaken for one of ours. They are the lines the bundled extension payload carries too.
const SCRIPT_MARKER: &str =
    "# installed by ferryx\n# managed by ferryx; reinstalling or updating the integration overwrites this file.\n";

/// Cline matches a hook file by basename against its event enum, and runs at most one per event.
fn script_file_name(event: &str) -> String {
    script_file_name_for(event, cfg!(windows))
}

pub(crate) fn script_file_name_for(event: &str, windows: bool) -> String {
    format!("{event}{}", if windows { ".ps1" } else { ".sh" })
}

pub(crate) fn hook_script(exe: &Path, agent: &str, event: &str) -> String {
    hook_script_for(&Reporter::local(exe), agent, event)
}

pub(crate) fn hook_script_for(reporter: &Reporter, agent: &str, event: &str) -> String {
    let command = reporter.command(agent, event);
    if reporter.windows {
        format!("{SCRIPT_MARKER}& {command}\n")
    } else {
        format!("#!/bin/sh\n{SCRIPT_MARKER}exec {command}\n")
    }
}

/// The event a Ferryx-written script reports, or `None` for a script that is not one of ours.
pub(crate) fn ferryx_script_event<'a>(body: &'a str, agent: &str) -> Option<&'a str> {
    if !body.contains(SCRIPT_MARKER) {
        return None;
    }
    let last = body.strip_suffix('\n')?.lines().next_back()?;
    let command = last.strip_prefix("exec ").or_else(|| last.strip_prefix("& "))?;
    ferryx_command_event(command, agent)
}

fn write_script(path: &Path, body: &str) -> std::io::Result<()> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "hook".into());
    let tmp = path.with_file_name(format!(".{name}.ferryx.tmp"));
    std::fs::write(&tmp, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Cline only runs a hook file that is executable (facts/local-3.md).
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, path)
}

fn install_event_scripts(target: &HookFileTarget, exe: &Path) -> std::io::Result<bool> {
    let mut wrote = false;
    for event in target.events {
        let path = target.file.join(script_file_name(event));
        let body = hook_script(exe, target.agent, event);
        match std::fs::read_to_string(&path) {
            Ok(existing) if existing == body => continue,
            // Cline runs one script per event, so a file Ferryx did not write is the user's own
            // hook for that event. Overwriting it would silently delete their hook.
            Ok(existing) if ferryx_script_event(&existing, target.agent).is_none() => continue,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        std::fs::create_dir_all(&target.file)?;
        write_script(&path, &body)?;
        wrote = true;
    }
    Ok(wrote)
}

fn remove_event_scripts(target: &HookFileTarget) -> std::io::Result<usize> {
    let mut removed = 0;
    for event in target.events {
        let path = target.file.join(script_file_name(event));
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => {}
            // A symlink or a directory under one of our names is not something Ferryx wrote.
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        }
        let Ok(body) = std::fs::read_to_string(&path) else { continue };
        if ferryx_script_event(&body, target.agent).is_none() {
            continue;
        }
        std::fs::remove_file(&path)?;
        removed += 1;
    }
    Ok(removed)
}

/// Merges `target`'s Ferryx hook entries. Returns whether the file was written; an agent with no
/// config directory is not installed and is skipped. Blocking: performs disk I/O.
pub fn install_hooks_into(target: &HookFileTarget, exe: &Path) -> std::io::Result<bool> {
    if !target.agent_dir.is_dir() {
        return Ok(false);
    }
    if target.shape == HookShape::EventScripts {
        return install_event_scripts(target, exe);
    }
    let path = resolve_write_path(&target.file)?;
    let original = read_settings(&path)?;
    let mut merged = original.clone().unwrap_or_else(|| Value::Object(Map::new()));
    let merge = merge_for(target.shape).expect("event scripts handled above");
    merge(&mut merged, target.agent, target.events, &Reporter::local(exe)).map_err(invalid)?;
    if original.as_ref() == Some(&merged) {
        return Ok(false);
    }
    // grok and agy keep their hook file in a subdirectory of the config directory that gates the
    // install, and that subdirectory need not exist yet.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_settings(&path, &merged)?;
    Ok(true)
}

/// Deletes `target`'s Ferryx hook entries and returns how many went. Blocking: performs disk I/O.
pub fn remove_hooks_from(target: &HookFileTarget) -> std::io::Result<usize> {
    if target.shape == HookShape::EventScripts {
        return remove_event_scripts(target);
    }
    let path = resolve_write_path(&target.file)?;
    let Some(mut settings) = read_settings(&path)? else {
        return Ok(0);
    };
    let strip = strip_for(target.shape).expect("event scripts handled above");
    let removed = strip(&mut settings, target.agent).map_err(invalid)?;
    if removed > 0 {
        write_settings(&path, &settings)?;
    }
    Ok(removed)
}

pub(crate) fn install_hook_targets(targets: &[HookFileTarget], exe: &Path) {
    for target in targets {
        match install_hooks_into(target, exe) {
            Ok(true) => tracing::info!(agent = target.agent, file = %target.file.display(), "Installed Ferryx agent hooks"),
            Ok(false) => {}
            Err(error) => tracing::warn!(agent = target.agent, file = %target.file.display(), %error, "Failed to install Ferryx agent hooks"),
        }
        match install_skill_into(target) {
            Ok(true) => tracing::info!(agent = target.agent, "Installed the Ferryx agent skill"),
            Ok(false) => {}
            Err(error) => tracing::warn!(agent = target.agent, %error, "Failed to install the Ferryx agent skill"),
        }
    }
}

/// Writes the Ferryx skill for `target` and returns whether it wrote. Nothing is written for an
/// agent without a skill path or without its config directory, and a file that is not a managed
/// Ferryx skill (the user's own, or one they took over by deleting the marker) is left as is.
/// Blocking: performs disk I/O.
pub fn install_skill_into(target: &HookFileTarget) -> std::io::Result<bool> {
    let Some(path) = &target.skill else { return Ok(false) };
    if !target.agent_dir.is_dir() {
        return Ok(false);
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => return Ok(false),
        Ok(_) => {
            let existing = std::fs::read_to_string(path)?;
            if existing == SKILL_SOURCE || !is_ferryx_skill(&existing) {
                return Ok(false);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_skill_file(path)?;
    Ok(true)
}

/// Writes [`SKILL_SOURCE`] at `path` through a temporary file of its own, created exclusively
/// under a random name beside it. A file or link someone else left at a guessable name is never
/// written through or deleted, and the temporary file is removed again if the rename fails.
fn write_skill_file(path: &Path) -> std::io::Result<()> {
    use std::io::Write;
    let mut builder = tempfile::Builder::new();
    builder.prefix(".SKILL.md.ferryx.").suffix(".tmp");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o644));
    }
    let mut tmp = builder.tempfile_in(path.parent().unwrap_or_else(|| Path::new(".")))?;
    tmp.write_all(SKILL_SOURCE.as_bytes())?;
    tmp.persist(path)?;
    Ok(())
}

/// Deletes the skill file for `target` when it is a managed Ferryx skill, and its `ferryx`
/// directory when that is left empty. Returns whether the file went. Blocking: performs disk I/O.
pub fn remove_skill_from(target: &HookFileTarget) -> std::io::Result<bool> {
    let Some(path) = &target.skill else { return Ok(false) };
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => {}
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    }
    if !is_ferryx_skill(&std::fs::read_to_string(path)?) {
        return Ok(false);
    }
    std::fs::remove_file(path)?;
    if let Some(parent) = path.parent() {
        // Fails, and is meant to, when the directory still holds anything else.
        let _ = std::fs::remove_dir(parent);
    }
    Ok(true)
}

fn merge_for(shape: HookShape) -> Option<MergeShape> {
    match shape {
        HookShape::ClaudeGroups => Some(merge_hooks),
        HookShape::GroupKeyedObject => Some(merge_group_object),
        HookShape::NamedEntries => Some(merge_named_entries),
        HookShape::CommandEntries => Some(merge_command_entries),
        HookShape::EventScripts => None,
    }
}

fn strip_for(shape: HookShape) -> Option<StripShape> {
    match shape {
        HookShape::ClaudeGroups => Some(strip_hooks),
        HookShape::GroupKeyedObject => Some(strip_group_object),
        HookShape::NamedEntries => Some(strip_named_entries),
        HookShape::CommandEntries => Some(strip_command_entries),
        HookShape::EventScripts => None,
    }
}

/// The text an install leaves in a settings file whose current text is `existing` (`None` when
/// the file is absent), or `None` when the file must stay as it is. The same merge
/// [`install_hooks_into`] applies, on text, so the SSH integration can run it for a remote file.
pub(crate) fn merged_settings_text(
    shape: HookShape,
    agent: &str,
    events: &[&str],
    reporter: &Reporter,
    existing: Option<&str>,
) -> Result<Option<String>, String> {
    let merge = merge_for(shape).ok_or("event scripts are not a settings file")?;
    let original = existing.map(parse_settings).transpose()?;
    let mut merged = original.clone().unwrap_or_else(|| Value::Object(Map::new()));
    merge(&mut merged, agent, events, reporter)?;
    if original.as_ref() == Some(&merged) {
        return Ok(None);
    }
    render_settings(&merged).map(Some)
}

/// The text a removal leaves in a settings file, or `None` when it holds no Ferryx entry for
/// `agent` and must stay byte-for-byte as it is. The same strip [`remove_hooks_from`] applies.
pub(crate) fn stripped_settings_text(
    shape: HookShape,
    agent: &str,
    existing: &str,
) -> Result<Option<String>, String> {
    let strip = strip_for(shape).ok_or("event scripts are not a settings file")?;
    let mut settings = parse_settings(existing)?;
    if strip(&mut settings, agent)? == 0 {
        return Ok(None);
    }
    render_settings(&settings).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home() -> tempfile::TempDir {
        tempfile::Builder::new().prefix("ferryx-hooks-").tempdir().expect("temp home")
    }

    fn targets(home: &Path) -> Vec<HookFileTarget> {
        let home = home.as_os_str().to_os_string();
        hook_targets_with_env(move |key| (key == "HOME").then(|| home.clone()))
    }

    fn target(home: &Path, agent: &str) -> HookFileTarget {
        targets(home).into_iter().find(|target| target.agent == agent).unwrap()
    }

    fn exe() -> PathBuf {
        PathBuf::from("/Applications/Ferryx.app/Contents/Helpers/Ferryx Daemon.app/Contents/MacOS/ferryx")
    }

    fn read(path: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn targets_follow_the_measured_hook_files() {
        let home = temp_home();
        let root = home.path();
        let found = targets(root);
        let expected: Vec<(&str, PathBuf, HookShape)> = vec![
            ("claude", root.join(".claude/settings.json"), HookShape::ClaudeGroups),
            ("codex", root.join(".codex/hooks.json"), HookShape::ClaudeGroups),
            ("grok", root.join(".grok/hooks/ferryx.json"), HookShape::ClaudeGroups),
            ("antigravity", root.join(".gemini/config/hooks.json"), HookShape::GroupKeyedObject),
            ("cline", root.join(".cline/hooks"), HookShape::EventScripts),
            ("crush", root.join(".config/crush/crush.json"), HookShape::NamedEntries),
            ("cursor", root.join(".cursor/hooks.json"), HookShape::CommandEntries),
        ];
        assert_eq!(
            found.iter().map(|t| (t.agent, t.file.clone(), t.shape)).collect::<Vec<_>>(),
            expected
        );
        // Every agent with a reporter table must have somewhere to be installed, and nothing is
        // installed for an agent the reporter does not report for.
        for (agent, events) in crate::cli::agent_hook::HOOK_AGENTS {
            let target = found
                .iter()
                .find(|target| target.agent == agent)
                .unwrap_or_else(|| panic!("{agent} has no install target"));
            assert_eq!(target.events, events, "{agent}");
        }
        let profile = root.as_os_str().to_os_string();
        let via_profile =
            hook_targets_with_env(move |key| (key == "USERPROFILE").then(|| profile.clone()));
        assert_eq!(via_profile, found);
        assert!(hook_targets_with_env(|_| None).is_empty());
    }

    #[test]
    fn crush_follows_the_xdg_config_home() {
        let home = temp_home();
        let xdg = home.path().join("elsewhere");
        let (h, x) = (home.path().as_os_str().to_os_string(), xdg.as_os_str().to_os_string());
        let found = hook_targets_with_env(move |key| match key {
            "HOME" => Some(h.clone()),
            "XDG_CONFIG_HOME" => Some(x.clone()),
            _ => None,
        });
        let crush = found.iter().find(|target| target.agent == "crush").unwrap();
        assert_eq!(crush.agent_dir, xdg.join("crush"));
        assert_eq!(crush.file, xdg.join("crush").join("crush.json"));
        // Only crush is XDG-rooted; the rest stay under HOME.
        for target in found.iter().filter(|target| target.agent != "crush") {
            assert!(target.agent_dir.starts_with(home.path()), "{}", target.agent_dir.display());
        }
    }

    #[test]
    fn install_creates_one_entry_per_event_and_is_idempotent() {
        for agent in ["claude", "codex", "grok"] {
            let home = temp_home();
            let target = target(home.path(), agent);
            std::fs::create_dir_all(&target.agent_dir).unwrap();
            assert!(install_hooks_into(&target, &exe()).unwrap());
            let settings = read(&target.file);
            let hooks = settings["hooks"].as_object().unwrap();
            assert_eq!(hooks.len(), target.events.len(), "{agent}: {settings}");
            for event in target.events {
                assert_eq!(
                    hooks[*event],
                    serde_json::json!([{"hooks": [{"type": "command", "command": hook_command(&exe(), agent, event)}]}]),
                    "{agent} {event}"
                );
            }
            let bytes = std::fs::read(&target.file).unwrap();
            assert!(!install_hooks_into(&target, &exe()).unwrap(), "a second install is a no-op");
            assert_eq!(std::fs::read(&target.file).unwrap(), bytes);
        }
    }

    #[test]
    fn install_skips_an_agent_that_is_not_installed() {
        let home = temp_home();
        let target = target(home.path(), "claude");
        assert!(!install_hooks_into(&target, &exe()).unwrap());
        assert!(!target.agent_dir.exists() && !target.file.exists());
    }

    fn user_settings() -> Value {
        serde_json::json!({
            "model": "opus",
            "hooks": {
                "Stop": [{"hooks": [{"type": "command", "command": "orca-hook stop"}]}],
                "Notification": [{"matcher": "", "hooks": [{"type": "command", "command": "herdr notify"}]}],
                "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "./guard.sh"}]}]
            },
            "permissions": {"allow": ["Bash(ls:*)"]}
        })
    }

    #[test]
    fn install_and_removal_preserve_every_foreign_hook() {
        let home = temp_home();
        let target = target(home.path(), "claude");
        std::fs::create_dir_all(&target.agent_dir).unwrap();
        std::fs::write(&target.file, serde_json::to_string(&user_settings()).unwrap()).unwrap();

        assert!(install_hooks_into(&target, &exe()).unwrap());
        let installed = read(&target.file);
        assert_eq!(installed["model"], "opus");
        assert_eq!(installed["permissions"], user_settings()["permissions"]);
        assert_eq!(installed["hooks"]["Notification"], user_settings()["hooks"]["Notification"]);
        let stop = installed["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0], user_settings()["hooks"]["Stop"][0], "the user's group stays first");
        assert_eq!(ferryx_group_command(&stop[1], "claude"), Some(hook_command(&exe(), "claude", "Stop").as_str()));
        assert!(!install_hooks_into(&target, &exe()).unwrap());

        assert_eq!(remove_hooks_from(&target).unwrap(), target.events.len());
        assert_eq!(read(&target.file), user_settings(), "removal restores the user's settings exactly");
        assert_eq!(remove_hooks_from(&target).unwrap(), 0, "a second removal finds nothing");
    }

    #[test]
    fn removal_of_a_fresh_install_leaves_no_ferryx_entries() {
        let home = temp_home();
        let target = target(home.path(), "codex");
        std::fs::create_dir_all(&target.agent_dir).unwrap();
        std::fs::write(&target.file, "{\"hooks\": {}}").unwrap();
        install_hooks_into(&target, &exe()).unwrap();
        assert_eq!(remove_hooks_from(&target).unwrap(), target.events.len());
        assert_eq!(read(&target.file), serde_json::json!({"hooks": {}}));
    }

    #[test]
    fn a_new_executable_replaces_the_previous_generations_entries() {
        let home = temp_home();
        let target = target(home.path(), "codex");
        std::fs::create_dir_all(&target.agent_dir).unwrap();
        let old = PathBuf::from("/Users/me/Library/Application Support/Ferryx/runtime/aaaa/ferryx");
        install_hooks_into(&target, &old).unwrap();
        assert!(install_hooks_into(&target, &exe()).unwrap());
        let settings = read(&target.file);
        for event in target.events {
            let groups = settings["hooks"][*event].as_array().unwrap();
            assert_eq!(groups.len(), 1, "{event}: {settings}");
            assert_eq!(ferryx_group_command(&groups[0], "codex"), Some(hook_command(&exe(), "codex", event).as_str()));
        }
    }

    #[test]
    fn an_edited_ferryx_entry_is_no_longer_ours() {
        let home = temp_home();
        let target = target(home.path(), "claude");
        std::fs::create_dir_all(&target.agent_dir).unwrap();
        let edited = serde_json::json!({"hooks": {"Stop": [
            {"hooks": [{"type": "command", "command": hook_command(&exe(), "claude", "Stop"), "timeout": 5}]},
            {"matcher": "x", "hooks": [{"type": "command", "command": hook_command(&exe(), "claude", "Stop")}]},
            {"hooks": [{"type": "command", "command": "'/usr/local/bin/other' agent-hook claude Stop"}]}
        ]}});
        std::fs::write(&target.file, serde_json::to_string(&edited).unwrap()).unwrap();
        assert_eq!(remove_hooks_from(&target).unwrap(), 0);
        assert_eq!(read(&target.file), edited);
    }

    #[test]
    fn a_malformed_settings_file_is_never_rewritten() {
        let home = temp_home();
        let target = target(home.path(), "claude");
        std::fs::create_dir_all(&target.agent_dir).unwrap();
        for body in ["{ not json", "[]", "{\"hooks\": []}", "{\"hooks\": {\"Stop\": {}}}"] {
            std::fs::write(&target.file, body).unwrap();
            assert!(install_hooks_into(&target, &exe()).is_err(), "{body}");
            assert_eq!(std::fs::read_to_string(&target.file).unwrap(), body);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_settings_file_is_updated_at_its_target() {
        let home = temp_home();
        let target = target(home.path(), "claude");
        std::fs::create_dir_all(&target.agent_dir).unwrap();
        let real = home.path().join("dotfiles-settings.json");
        std::fs::write(&real, "{\"model\": \"opus\"}").unwrap();
        std::os::unix::fs::symlink(&real, &target.file).unwrap();
        assert!(install_hooks_into(&target, &exe()).unwrap());
        assert!(std::fs::symlink_metadata(&target.file).unwrap().file_type().is_symlink());
        assert_eq!(read(&real)["model"], "opus");
        assert!(read(&real)["hooks"]["Stop"].is_array());
    }

    #[test]
    fn installed_commands_round_trip_through_recognition() {
        for path in [exe(), PathBuf::from("/tmp/it's here/ferryx")] {
            let command = hook_command(&path, "claude", "Stop");
            assert_eq!(ferryx_command_event(&command, "claude"), Some("Stop"), "{command}");
            assert_eq!(ferryx_command_event(&command, "codex"), None);
        }
        #[cfg(windows)]
        assert_eq!(ferryx_command_event("\"C:\\x\\ferryx.exe\" agent-hook codex Stop", "codex"), Some("Stop"));
        assert_eq!(ferryx_command_event("'/bin/ferryx' agent-hook claude Stop; rm -rf /", "claude"), None);
        assert_eq!(ferryx_command_event("ferryx agent-hook claude Stop", "claude"), None);
    }

    /// Grok's file lives in a subdirectory of the directory that gates the install, so the install
    /// has to create it.
    #[test]
    fn the_grok_hook_file_is_created_in_its_own_subdirectory() {
        let home = temp_home();
        let target = target(home.path(), "grok");
        std::fs::create_dir_all(&target.agent_dir).unwrap();
        assert!(!target.file.parent().unwrap().exists());
        assert!(install_hooks_into(&target, &exe()).unwrap());
        assert_eq!(read(&target.file)["hooks"].as_object().unwrap().len(), target.events.len());
        assert_eq!(remove_hooks_from(&target).unwrap(), target.events.len());
        // Ferryx owns this whole file, so removing its entries leaves it empty rather than
        // restoring someone else's content.
        assert_eq!(read(&target.file), serde_json::json!({"hooks": {}}));
    }

    fn agy_user_hooks() -> Value {
        // The two groups measured in a real `~/.gemini/config/hooks.json` (facts/local-2.md).
        serde_json::json!({
            "cmux-notify-stop": {"enabled": true, "Stop": [{"type": "command", "command": "cmux notify", "timeout": 5}]},
            "orca-status": {"PreInvocation": [{"type": "command", "command": "orca status working", "timeout": 5}]}
        })
    }

    #[test]
    fn the_agy_group_is_the_only_one_an_install_touches() {
        let home = temp_home();
        let target = target(home.path(), "antigravity");
        std::fs::create_dir_all(&target.agent_dir).unwrap();
        std::fs::write(
            home.path().join(".gemini").join("settings.json"),
            "{\"model\": \"fast\"}",
        )
        .unwrap();
        std::fs::create_dir_all(target.file.parent().unwrap()).unwrap();
        std::fs::write(&target.file, serde_json::to_string(&agy_user_hooks()).unwrap()).unwrap();

        assert!(install_hooks_into(&target, &exe()).unwrap());
        let installed = read(&target.file);
        assert_eq!(installed["cmux-notify-stop"], agy_user_hooks()["cmux-notify-stop"]);
        assert_eq!(installed["orca-status"], agy_user_hooks()["orca-status"]);
        let ours = &installed[FERRYX_ENTRY_NAME];
        assert_eq!(ours["enabled"], true);
        for event in target.events {
            assert_eq!(
                ours[*event],
                serde_json::json!([{
                    "type": "command",
                    "command": hook_command(&exe(), "antigravity", event),
                    "timeout": HOOK_TIMEOUT_SECONDS
                }]),
                "{event}"
            );
        }
        assert!(!install_hooks_into(&target, &exe()).unwrap(), "a second install is a no-op");

        // A new daemon generation replaces our group rather than stacking a second one.
        let newer = PathBuf::from("/Users/me/Library/Application Support/Ferryx/runtime/bbbb/ferryx");
        assert!(install_hooks_into(&target, &newer).unwrap());
        assert_eq!(read(&target.file).as_object().unwrap().len(), 3);

        assert_eq!(remove_hooks_from(&target).unwrap(), target.events.len());
        assert_eq!(read(&target.file), agy_user_hooks(), "removal restores the file exactly");
        assert_eq!(remove_hooks_from(&target).unwrap(), 0);
        // gemini's own settings file in the same directory is never read or written.
        assert_eq!(
            std::fs::read_to_string(home.path().join(".gemini").join("settings.json")).unwrap(),
            "{\"model\": \"fast\"}"
        );
    }

    #[test]
    fn an_edited_agy_group_belongs_to_the_user() {
        let home = temp_home();
        let target = target(home.path(), "antigravity");
        std::fs::create_dir_all(target.file.parent().unwrap()).unwrap();
        for edited in [
            // A command that is not the reporter's.
            serde_json::json!({FERRYX_ENTRY_NAME: {"enabled": true, "Stop": [{"type": "command", "command": "mine.sh", "timeout": 5}]}}),
            // Our command, but filed under an event it does not report.
            serde_json::json!({FERRYX_ENTRY_NAME: {"enabled": true, "PreToolUse": [{"type": "command", "command": hook_command(&exe(), "antigravity", "Stop"), "timeout": 5}]}}),
            // Disabled by the user.
            serde_json::json!({FERRYX_ENTRY_NAME: {"enabled": false, "Stop": [{"type": "command", "command": hook_command(&exe(), "antigravity", "Stop"), "timeout": 5}]}}),
            // Two commands where Ferryx writes one.
            serde_json::json!({FERRYX_ENTRY_NAME: {"Stop": [{"type": "command", "command": hook_command(&exe(), "antigravity", "Stop"), "timeout": 5}, {"type": "command", "command": "also.sh"}]}}),
        ] {
            std::fs::write(&target.file, serde_json::to_string(&edited).unwrap()).unwrap();
            assert!(!install_hooks_into(&target, &exe()).unwrap(), "{edited}");
            assert_eq!(remove_hooks_from(&target).unwrap(), 0, "{edited}");
            assert_eq!(read(&target.file), edited);
        }
    }

    fn crush_user_config() -> Value {
        serde_json::json!({
            "providers": {"openai-compat": {"base_url": "http://127.0.0.1:18977/v1"}},
            "hooks": {"PreToolUse": [{"name": "parity-probe", "matcher": "bash", "command": "./probe.sh", "timeout": 30}]}
        })
    }

    #[test]
    fn the_crush_entry_merges_beside_the_users_own_hooks() {
        let home = temp_home();
        let target = target(home.path(), "crush");
        std::fs::create_dir_all(&target.agent_dir).unwrap();
        std::fs::write(&target.file, serde_json::to_string(&crush_user_config()).unwrap()).unwrap();

        assert!(install_hooks_into(&target, &exe()).unwrap());
        let installed = read(&target.file);
        assert_eq!(installed["providers"], crush_user_config()["providers"]);
        let entries = installed["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], crush_user_config()["hooks"]["PreToolUse"][0], "the user's stays");
        assert_eq!(
            entries[1],
            serde_json::json!({
                "name": FERRYX_ENTRY_NAME,
                "matcher": "",
                "command": hook_command(&exe(), "crush", "PreToolUse"),
                "timeout": HOOK_TIMEOUT_SECONDS
            })
        );
        assert!(!install_hooks_into(&target, &exe()).unwrap());
        assert_eq!(remove_hooks_from(&target).unwrap(), 1);
        assert_eq!(read(&target.file), crush_user_config(), "removal restores the config exactly");
        assert_eq!(remove_hooks_from(&target).unwrap(), 0);
    }

    /// A crush config with no hooks at all must come back without the `hooks` key Ferryx added.
    #[test]
    fn removing_the_crush_entry_takes_the_hooks_key_it_created() {
        let home = temp_home();
        let target = target(home.path(), "crush");
        std::fs::create_dir_all(&target.agent_dir).unwrap();
        let original = serde_json::json!({"options": {"debug": false}});
        std::fs::write(&target.file, serde_json::to_string(&original).unwrap()).unwrap();
        assert!(install_hooks_into(&target, &exe()).unwrap());
        assert!(read(&target.file)["hooks"]["PreToolUse"].is_array());
        assert_eq!(remove_hooks_from(&target).unwrap(), 1);
        assert_eq!(read(&target.file), original);
    }

    #[cfg(unix)]
    #[test]
    fn cline_gets_one_executable_script_per_event() {
        use std::os::unix::fs::PermissionsExt;
        let home = temp_home();
        let target = target(home.path(), "cline");
        std::fs::create_dir_all(&target.agent_dir).unwrap();
        assert!(install_hooks_into(&target, &exe()).unwrap());
        for event in target.events {
            let path = target.file.join(format!("{event}.sh"));
            let body = std::fs::read_to_string(&path).unwrap();
            assert_eq!(body, hook_script(&exe(), "cline", event), "{event}");
            assert!(body.contains(&hook_command(&exe(), "cline", event)), "{event}");
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "cline only runs an executable hook: {event}");
        }
        // Ferryx writes nothing beyond its own scripts.
        assert_eq!(std::fs::read_dir(&target.file).unwrap().count(), target.events.len());
        assert!(!install_hooks_into(&target, &exe()).unwrap(), "a second install is a no-op");

        let newer = PathBuf::from("/Users/me/Library/Application Support/Ferryx/runtime/bbbb/ferryx");
        assert!(install_hooks_into(&target, &newer).unwrap());
        assert_eq!(std::fs::read_dir(&target.file).unwrap().count(), target.events.len());

        assert_eq!(remove_hooks_from(&target).unwrap(), target.events.len());
        assert_eq!(std::fs::read_dir(&target.file).unwrap().count(), 0, "every script went");
        assert_eq!(remove_hooks_from(&target).unwrap(), 0);
    }

    /// Cline runs one script per event, so a user's own hook must never be replaced or deleted.
    #[test]
    fn a_cline_script_ferryx_did_not_write_survives_install_and_removal() {
        let home = temp_home();
        let target = target(home.path(), "cline");
        std::fs::create_dir_all(&target.file).unwrap();
        let mine = target.file.join(script_file_name("TaskStart"));
        let body = "#!/bin/sh\necho mine\n";
        std::fs::write(&mine, body).unwrap();

        assert!(install_hooks_into(&target, &exe()).unwrap());
        assert_eq!(std::fs::read_to_string(&mine).unwrap(), body, "the user's hook is untouched");
        assert_eq!(remove_hooks_from(&target).unwrap(), target.events.len() - 1);
        assert_eq!(std::fs::read_to_string(&mine).unwrap(), body);
        assert_eq!(std::fs::read_dir(&target.file).unwrap().count(), 1);
    }

    #[test]
    fn installed_scripts_round_trip_through_recognition() {
        let script = hook_script(&exe(), "cline", "TaskStart");
        assert_eq!(ferryx_script_event(&script, "cline"), Some("TaskStart"));
        assert_eq!(ferryx_script_event(&script, "claude"), None);
        // The marker alone is not enough, and neither is the command alone.
        assert_eq!(ferryx_script_event(&format!("{SCRIPT_MARKER}echo hi\n"), "cline"), None);
        let command = hook_command(&exe(), "cline", "TaskStart");
        assert_eq!(ferryx_script_event(&format!("#!/bin/sh\nexec {command}\n"), "cline"), None);
        // A trailing line of the user's own makes the script theirs.
        assert_eq!(ferryx_script_event(&format!("{script}echo extra\n"), "cline"), None);
    }

    /// A `~/.cursor/hooks.json` holding the user's own hooks, in the shape the live probe used
    /// (cursor-hooks-live-2026-10-23.md).
    fn cursor_user_hooks() -> Value {
        serde_json::json!({
            "version": 1,
            "hooks": {
                "stop": [{"command": "./audit-stop.sh"}],
                "afterFileEdit": [{"command": "./format.sh"}]
            }
        })
    }

    #[test]
    fn the_cursor_entries_merge_beside_the_users_own_hooks() {
        let home = temp_home();
        let target = target(home.path(), "cursor");
        assert_eq!(target.file, home.path().join(".cursor").join("hooks.json"));
        std::fs::create_dir_all(&target.agent_dir).unwrap();
        std::fs::write(&target.file, serde_json::to_string(&cursor_user_hooks()).unwrap()).unwrap();

        assert!(install_hooks_into(&target, &exe()).unwrap());
        let installed = read(&target.file);
        assert_eq!(installed["version"], 1, "the schema version is kept");
        assert_eq!(installed["hooks"]["afterFileEdit"], cursor_user_hooks()["hooks"]["afterFileEdit"]);
        let stop = installed["hooks"]["stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0], cursor_user_hooks()["hooks"]["stop"][0], "the user's entry stays first");
        for event in target.events {
            let entries = installed["hooks"][*event].as_array().unwrap();
            assert_eq!(
                entries.last().unwrap(),
                &serde_json::json!({"command": hook_command(&exe(), "cursor", event)}),
                "{event}"
            );
        }
        let bytes = std::fs::read(&target.file).unwrap();
        assert!(!install_hooks_into(&target, &exe()).unwrap(), "a second install is a no-op");
        assert_eq!(std::fs::read(&target.file).unwrap(), bytes);

        // A new daemon generation replaces our entries rather than stacking a second set.
        let newer = PathBuf::from("/Users/me/Library/Application Support/Ferryx/runtime/bbbb/ferryx");
        assert!(install_hooks_into(&target, &newer).unwrap());
        assert_eq!(read(&target.file)["hooks"]["stop"].as_array().unwrap().len(), 2);

        assert_eq!(remove_hooks_from(&target).unwrap(), target.events.len());
        assert_eq!(read(&target.file), cursor_user_hooks(), "removal restores the file exactly");
        assert_eq!(remove_hooks_from(&target).unwrap(), 0);
    }

    #[test]
    fn a_fresh_cursor_hooks_file_gets_cursors_version_and_edited_entries_stay() {
        let home = temp_home();
        let target = target(home.path(), "cursor");
        // No ~/.cursor means cursor is not installed, so nothing is written.
        assert!(!install_hooks_into(&target, &exe()).unwrap());
        assert!(!target.agent_dir.exists());

        std::fs::create_dir_all(&target.agent_dir).unwrap();
        assert!(install_hooks_into(&target, &exe()).unwrap());
        let installed = read(&target.file);
        assert_eq!(installed["version"], 1);
        assert_eq!(installed["hooks"].as_object().unwrap().len(), target.events.len());
        assert_eq!(remove_hooks_from(&target).unwrap(), target.events.len());
        assert_eq!(read(&target.file), serde_json::json!({"version": 1, "hooks": {}}));

        // An entry with a key added, or another agent's reporter command, is not ours.
        let edited = serde_json::json!({"version": 1, "hooks": {"stop": [
            {"command": hook_command(&exe(), "cursor", "stop"), "timeout": 5},
            {"command": hook_command(&exe(), "claude", "Stop")}
        ]}});
        std::fs::write(&target.file, serde_json::to_string(&edited).unwrap()).unwrap();
        assert_eq!(remove_hooks_from(&target).unwrap(), 0);
        assert_eq!(read(&target.file), edited);
    }

    /// Every agent the reporter reports for must be installable, and the command written for it
    /// must be the one `ferryx agent-hook` answers to.
    #[test]
    fn every_reported_agent_has_an_install_target_and_a_recognised_command() {
        let home = temp_home();
        for (agent, events) in crate::cli::agent_hook::HOOK_AGENTS {
            let target = target(home.path(), agent);
            assert_eq!(target.events, events, "{agent}");
            for event in events {
                let command = hook_command(&exe(), agent, event);
                assert_eq!(ferryx_command_event(&command, agent), Some(*event), "{command}");
                assert!(
                    crate::cli::agent_hook::state_for(agent, event).is_some(),
                    "{agent} {event} is installed but reports nothing"
                );
            }
        }
    }

    #[test]
    fn skill_paths_are_claudes_and_the_shared_agents_dir_only() {
        let home = temp_home();
        for target in targets(home.path()) {
            let expected = match target.agent {
                "claude" => Some(home.path().join(".claude/skills/ferryx/SKILL.md")),
                "codex" => Some(home.path().join(".agents/skills/ferryx/SKILL.md")),
                _ => None,
            };
            assert_eq!(target.skill, expected, "{}", target.agent);
        }
        assert!(is_ferryx_skill(SKILL_SOURCE), "the shipped skill carries the marker");
        let mut lines = SKILL_SOURCE.lines().map(str::trim_end);
        assert_eq!(lines.next(), Some("---"));
        assert_eq!(lines.next(), Some("name: ferryx"));
    }

    #[test]
    fn skill_install_creates_the_file_and_reinstall_is_idempotent() {
        for agent in ["claude", "codex"] {
            let home = temp_home();
            let target = target(home.path(), agent);
            let skill = target.skill.clone().unwrap();
            assert!(!install_skill_into(&target).unwrap(), "{agent}: no config dir, no skill");
            assert!(!skill.exists());

            std::fs::create_dir_all(&target.agent_dir).unwrap();
            install_hook_targets(std::slice::from_ref(&target), &exe());
            assert_eq!(std::fs::read_to_string(&skill).unwrap(), SKILL_SOURCE, "{agent}");
            assert!(!install_skill_into(&target).unwrap(), "{agent}: a second install is a no-op");
            assert_eq!(std::fs::read_dir(skill.parent().unwrap()).unwrap().count(), 1, "no temp file is left");

            // An older managed version is replaced.
            std::fs::write(&skill, "---\nname: ferryx\nferryx-managed: true\n---\nold\n").unwrap();
            assert!(install_skill_into(&target).unwrap());
            assert_eq!(std::fs::read_to_string(&skill).unwrap(), SKILL_SOURCE);
        }
    }

    #[test]
    fn skill_removal_deletes_the_managed_file() {
        let home = temp_home();
        let target = target(home.path(), "claude");
        std::fs::create_dir_all(&target.agent_dir).unwrap();
        assert!(install_skill_into(&target).unwrap());
        let skill = target.skill.clone().unwrap();
        assert!(remove_skill_from(&target).unwrap());
        assert!(!skill.exists());
        assert!(!skill.parent().unwrap().exists(), "the emptied ferryx directory goes too");
        assert!(skill.parent().unwrap().parent().unwrap().is_dir(), "the skills directory stays");
        assert!(!remove_skill_from(&target).unwrap(), "a second removal finds nothing");
    }

    /// A link someone left at the old fixed temp name is neither written through nor removed.
    #[cfg(unix)]
    #[test]
    fn skill_install_never_writes_through_a_planted_temp_link() {
        let home = temp_home();
        let target = target(home.path(), "claude");
        let skill = target.skill.clone().unwrap();
        std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
        let victim = home.path().join("victim.txt");
        std::fs::write(&victim, "keep\n").unwrap();
        let planted = skill.with_file_name(".SKILL.md.ferryx.tmp");
        std::os::unix::fs::symlink(&victim, &planted).unwrap();

        assert!(install_skill_into(&target).unwrap());
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep\n");
        assert!(std::fs::symlink_metadata(&planted).unwrap().file_type().is_symlink());
        assert!(std::fs::symlink_metadata(&skill).unwrap().file_type().is_file());
        assert_eq!(std::fs::read_to_string(&skill).unwrap(), SKILL_SOURCE);
        assert_eq!(std::fs::read_dir(skill.parent().unwrap()).unwrap().count(), 2, "no temp file is left");
    }

    /// A rename that fails takes its temporary file with it and touches nothing else.
    #[test]
    fn a_failed_skill_rename_leaves_no_temp_file() {
        let home = temp_home();
        let dir = home.path().join("ferryx");
        let blocker = dir.join("SKILL.md");
        std::fs::create_dir_all(&blocker).unwrap();
        std::fs::write(blocker.join("keep.txt"), "keep\n").unwrap();
        assert!(write_skill_file(&blocker).is_err());
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(left, vec![std::ffi::OsString::from("SKILL.md")]);
        assert_eq!(std::fs::read_to_string(blocker.join("keep.txt")).unwrap(), "keep\n");
    }

    /// A Windows link under the skill's name - a file symlink or a directory symlink, both
    /// reparse points - is never written through, replaced or deleted, by install or removal.
    #[cfg(windows)]
    #[test]
    fn a_reparse_point_under_the_skill_name_survives_install_and_removal() {
        for directory in [false, true] {
            let home = temp_home();
            let target = target(home.path(), "claude");
            std::fs::create_dir_all(&target.agent_dir).unwrap();
            let skill = target.skill.clone().unwrap();
            std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
            let outside = home.path().join("outside");
            std::fs::create_dir_all(&outside).unwrap();
            let victim = outside.join("SKILL.md");
            // An outdated managed skill elsewhere: following the link would replace or delete it.
            let victim_before =
                SKILL_SOURCE.replace("ferryx-managed: true", "ferryx-managed: true\nold: 1");
            assert!(is_ferryx_skill(&victim_before));
            std::fs::write(&victim, &victim_before).unwrap();
            if directory {
                std::os::windows::fs::symlink_dir(&outside, &skill)
                    .expect("directory symlink fixture requires symlink privilege");
            } else {
                std::os::windows::fs::symlink_file(&victim, &skill)
                    .expect("file symlink fixture requires symlink privilege");
            }
            assert!(std::fs::symlink_metadata(&skill).unwrap().file_type().is_symlink());

            assert!(!install_skill_into(&target).unwrap(), "directory={directory}");
            assert!(!remove_skill_from(&target).unwrap(), "directory={directory}");

            assert!(std::fs::symlink_metadata(&skill).unwrap().file_type().is_symlink());
            assert_eq!(std::fs::read_to_string(&victim).unwrap(), victim_before);
            assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1, "directory={directory}");
        }
    }

    #[test]
    fn skill_edited_by_the_user_survives_install_and_removal() {
        let home = temp_home();
        let target = target(home.path(), "codex");
        std::fs::create_dir_all(&target.agent_dir).unwrap();
        let skill = target.skill.clone().unwrap();
        std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
        let mine: String = SKILL_SOURCE
            .lines()
            .filter(|line| line.trim() != "ferryx-managed: true")
            .map(|line| format!("{line}\n"))
            .collect();
        assert!(!is_ferryx_skill(&mine));
        std::fs::write(&skill, &mine).unwrap();

        assert!(!install_skill_into(&target).unwrap());
        assert_eq!(std::fs::read_to_string(&skill).unwrap(), mine);
        assert!(!remove_skill_from(&target).unwrap());
        assert_eq!(std::fs::read_to_string(&skill).unwrap(), mine);

        // The marker counts only inside the frontmatter.
        assert!(!is_ferryx_skill("# notes\nferryx-managed: true\n"));
        assert!(!is_ferryx_skill("---\nname: x\n---\nferryx-managed: true\n"));
    }
}
