//! `ferryx-cli integrations`: what Ferryx installed into each coding agent on this machine, and
//! the verbs that install or remove it.
//!
//! - `status [--json]` lists every agent Ferryx detects panes of (the shipped detection manifests):
//!   whether its config directory exists, its hook entries, its skill and its state extension.
//! - `skill install|uninstall <agent|--all>` and `skill status <agent>` manage the Ferryx skill.
//! - `hooks install|uninstall <agent|--all>` manage the hook entries.
//!
//! There is no second installer here: every write goes through `agent_hook_install`, with the
//! executable of this process as the reporter, and keeps its ownership rules (nothing is written
//! for an agent whose config directory is absent, a user-edited skill or hook entry is never
//! overwritten or deleted). An uninstall also records a per-agent opt-out in the agent integration
//! setting, so the daemon's boot install does not put the entries back; an install clears it.
//!
//! Every verb but `status` (without `--json`) prints one JSON object on stdout. Errors are one
//! JSON object `{"type":"error","code","message"}` on stderr.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::agent_detect::engine::SHIPPED_MANIFESTS;
use crate::agent_detect::manifest::load_manifest_from_str;
use crate::daemon::agent_extension::{
    agent_integration_path, extension_files_with_env, is_ferryx_extension_bytes, read_opt_outs,
    set_agent_opt_out, AgentIntegration, OptOutKind,
};
use crate::daemon::agent_hook_install::{
    hook_status, hook_targets_with_env, install_hooks_into, remove_hooks_from, skill_state,
    skill_targets_with_env, HookFileTarget, SkillState, SkillTarget,
};

pub const INTEGRATIONS_USAGE: &str = "usage: ferryx-cli integrations status [--json] | \
     integrations skill install|uninstall <agent|--all> | integrations skill status <agent> | \
     integrations hooks install|uninstall <agent|--all>";

/// What an agent without a skill mechanism is told to run instead (the catalogue entry).
pub const SKILL_FALLBACK_COMMAND: &str = "ferryx-cli api describe agents";

/// Exit codes: 0 ok, 1 an install or removal failed, 2 usage error or unknown agent.
pub const EXIT_FAILURE: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

/// One detected agent: its manifest id and the names that also select it.
#[derive(Debug, Clone)]
pub struct KnownAgent {
    pub id: String,
    pub aliases: Vec<String>,
}

/// Every agent in the shipped detection manifests, in manifest order.
pub fn known_agents() -> Vec<KnownAgent> {
    SHIPPED_MANIFESTS
        .iter()
        .filter_map(|raw| load_manifest_from_str(raw).ok())
        .map(|manifest| KnownAgent { id: manifest.id, aliases: manifest.aliases })
        .collect()
}

/// Everything the verbs read or write, resolved once from the environment.
pub struct Context {
    pub agents: Vec<KnownAgent>,
    pub hooks: Vec<HookFileTarget>,
    pub skills: Vec<SkillTarget>,
    /// (agent, gate directory, extension or plugin file).
    pub extensions: Vec<(&'static str, PathBuf, PathBuf)>,
    /// The agent integration setting file; `None` when no data directory resolves.
    pub setting_path: Option<PathBuf>,
    /// The reporter the hook entries name: this executable.
    pub exe: Option<PathBuf>,
}

impl Context {
    pub fn from_process() -> Self {
        Self::with_env(
            |key| std::env::var_os(key),
            std::env::current_exe().ok(),
            agent_integration_path(),
        )
    }

    pub fn with_env(
        get_env: impl Fn(&str) -> Option<OsString>,
        exe: Option<PathBuf>,
        setting_path: Option<PathBuf>,
    ) -> Self {
        Self {
            agents: known_agents(),
            hooks: hook_targets_with_env(&get_env),
            skills: skill_targets_with_env(&get_env),
            extensions: extension_files_with_env(&get_env),
            setting_path,
            exe,
        }
    }

    fn resolve(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.agents
            .iter()
            .find(|agent| agent.id == name || agent.aliases.iter().any(|alias| *alias == name))
            .map(|agent| agent.id.as_str())
    }

    fn hook(&self, agent: &str) -> Option<&HookFileTarget> {
        self.hooks.iter().find(|target| target.agent == agent)
    }

    fn skill(&self, agent: &str) -> Option<&SkillTarget> {
        self.skills.iter().find(|target| target.agent == agent)
    }
}

/// Entry for `ferryx-cli integrations ...`; returns the process exit code.
pub fn run_integrations_cli(args: &[String]) -> i32 {
    let words: Vec<&str> = args.iter().skip(2).map(String::as_str).collect();
    let (code, out, err) = run_with(&Context::from_process(), &words);
    if let Some(out) = out {
        println!("{out}");
    }
    if let Some(err) = err {
        eprintln!("{err}");
    }
    code
}

fn error(code: &str, message: impl Into<String>) -> String {
    json!({ "type": "error", "code": code, "message": message.into() }).to_string()
}

fn usage() -> (i32, Option<String>, Option<String>) {
    (EXIT_USAGE, None, Some(error("USAGE", INTEGRATIONS_USAGE)))
}

/// Runs one invocation (the words after `integrations`) and returns the exit code with what
/// goes to stdout and stderr.
pub fn run_with(ctx: &Context, words: &[&str]) -> (i32, Option<String>, Option<String>) {
    match words {
        ["status"] => (0, Some(status_text(&status_json(ctx))), None),
        ["status", "--json"] => (0, Some(status_json(ctx).to_string()), None),
        ["skill", "status", name] => match ctx.resolve(name) {
            Some(agent) => (0, Some(json!({ "agent": agent, "skill": skill_json(ctx, agent) }).to_string()), None),
            None => unknown_agent(name),
        },
        ["skill", verb @ ("install" | "uninstall"), target] => {
            mutate(ctx, Kind::Skill, *verb == "install", target)
        }
        ["hooks", verb @ ("install" | "uninstall"), target] => {
            mutate(ctx, Kind::Hooks, *verb == "install", target)
        }
        _ => usage(),
    }
}

fn unknown_agent(name: &str) -> (i32, Option<String>, Option<String>) {
    (
        EXIT_USAGE,
        None,
        Some(error("UNKNOWN_AGENT", format!("unknown agent `{name}`; see `ferryx-cli integrations status`"))),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Skill,
    Hooks,
}

/// The agents a verb acts on: one named agent, or every agent that has a target of that kind.
fn selected<'a>(ctx: &'a Context, kind: Kind, target: &str) -> Result<Vec<&'a str>, String> {
    if target == "--all" {
        return Ok(match kind {
            Kind::Skill => ctx.skills.iter().map(|skill| skill.agent).collect(),
            Kind::Hooks => ctx.hooks.iter().map(|hook| hook.agent).collect(),
        });
    }
    ctx.resolve(target).map(|agent| vec![agent]).ok_or_else(|| target.to_owned())
}

fn mutate(ctx: &Context, kind: Kind, install: bool, target: &str) -> (i32, Option<String>, Option<String>) {
    let agents = match selected(ctx, kind, target) {
        Ok(agents) => agents,
        Err(name) => return unknown_agent(&name),
    };
    let mut results = Vec::new();
    let mut any_failed = false;
    for agent in agents {
        let result = match kind {
            Kind::Skill => skill_change(ctx, agent, install),
            Kind::Hooks => hook_change(ctx, agent, install),
        };
        any_failed |= result.get("result").and_then(Value::as_str) == Some("failed");
        results.push(result);
    }
    let action = format!(
        "{} {}",
        if kind == Kind::Skill { "skill" } else { "hooks" },
        if install { "install" } else { "uninstall" }
    );
    let out = json!({ "action": action, "results": results }).to_string();
    if any_failed {
        (EXIT_FAILURE, Some(out), Some(error("INTEGRATION_FAILED", format!("{action} failed for at least one agent"))))
    } else {
        (0, Some(out), None)
    }
}

/// Records (uninstall) or clears (install) the agent's opt-out so the boot install agrees with
/// what the owner asked for. Returns the error text when the setting could not be written.
fn record_opt_out(ctx: &Context, kind: OptOutKind, agent: &str, disabled: bool) -> Option<String> {
    let path = ctx.setting_path.as_deref()?;
    set_agent_opt_out(path, kind, agent, disabled).err().map(|error| error.to_string())
}

fn failed(agent: &str, path: &Path, message: String) -> Value {
    json!({ "agent": agent, "result": "failed", "path": path.to_string_lossy(), "error": message })
}

fn skill_change(ctx: &Context, agent: &str, install: bool) -> Value {
    let Some(skill) = ctx.skill(agent) else {
        return json!({ "agent": agent, "result": "unsupported", "skill": "unsupported", "fallback": SKILL_FALLBACK_COMMAND });
    };
    let path = skill.file.to_string_lossy().into_owned();
    if let Some(message) = record_opt_out(ctx, OptOutKind::Skill, agent, !install) {
        return failed(agent, &skill.file, message);
    }
    let outcome = if install {
        if !skill.agent_dir.is_dir() {
            return json!({ "agent": agent, "result": "not-detected", "path": path });
        }
        skill.install().map(|wrote| if wrote { "installed" } else { "unchanged" })
    } else {
        skill.remove().map(|removed| if removed { "removed" } else { "unchanged" })
    };
    match outcome {
        Ok(result) => {
            // An unchanged file the user owns is said so: Ferryx neither wrote nor removed it.
            let result = match skill_state(&skill.file) {
                Ok(SkillState::UserEdited) => "user-edited",
                _ => result,
            };
            json!({ "agent": agent, "result": result, "path": path })
        }
        Err(error) => failed(agent, &skill.file, error.to_string()),
    }
}

fn hook_change(ctx: &Context, agent: &str, install: bool) -> Value {
    let Some(hook) = ctx.hook(agent) else {
        return json!({ "agent": agent, "result": "unsupported", "hooks": "unsupported" });
    };
    let path = hook.file.to_string_lossy().into_owned();
    if let Some(message) = record_opt_out(ctx, OptOutKind::Hooks, agent, !install) {
        return failed(agent, &hook.file, message);
    }
    let outcome = if install {
        if !hook.agent_dir.is_dir() {
            return json!({ "agent": agent, "result": "not-detected", "path": path });
        }
        let Some(exe) = ctx.exe.as_deref() else {
            return failed(agent, &hook.file, "the ferryx-cli executable path is unknown".into());
        };
        install_hooks_into(hook, exe).map(|wrote| if wrote { "installed" } else { "unchanged" })
    } else {
        remove_hooks_from(hook).map(|removed| if removed > 0 { "removed" } else { "unchanged" })
    };
    match outcome {
        Ok(result) => json!({ "agent": agent, "result": result, "path": path }),
        Err(error) => failed(agent, &hook.file, error.to_string()),
    }
}

fn skill_json(ctx: &Context, agent: &str) -> Value {
    let Some(skill) = ctx.skill(agent) else {
        return json!({ "state": "unsupported", "fallback": SKILL_FALLBACK_COMMAND });
    };
    let state = match skill_state(&skill.file) {
        Ok(SkillState::Absent) => "absent",
        Ok(SkillState::Managed) => "installed",
        Ok(SkillState::UserEdited) => "user-edited",
        Err(_) => "unreadable",
    };
    json!({ "state": state, "path": skill.file.to_string_lossy() })
}

/// `{"integrationEnabled", "agents":[{id, detected, hookSupported, hookInstalled, hookStale,
/// hookEnabled, skill, skillInstalled, skillUserEdited, skillEnabled, extensionSupported,
/// extensionInstalled, integrationEnabled}]}`. Reads only.
pub fn status_json(ctx: &Context) -> Value {
    let enabled = AgentIntegration::load(ctx.setting_path.clone()).enabled();
    let opt_outs = read_opt_outs(ctx.setting_path.as_deref());
    let agents: Vec<Value> = ctx
        .agents
        .iter()
        .map(|known| {
            let agent = known.id.as_str();
            let hook = ctx.hook(agent);
            let skill = ctx.skill(agent);
            let extensions: Vec<_> = ctx.extensions.iter().filter(|(name, _, _)| *name == agent).collect();
            let detected = hook.is_some_and(|hook| hook.agent_dir.is_dir())
                || skill.is_some_and(|skill| skill.agent_dir.is_dir())
                || extensions.iter().any(|(_, gate, _)| gate.is_dir());
            let hooks_now = hook.and_then(|hook| hook_status(hook).ok());
            let skill_entry = skill_json(ctx, agent);
            let skill_now = skill_entry["state"].as_str().unwrap_or("unsupported").to_owned();
            let extension_installed = extensions.iter().any(|(_, _, file)| {
                std::fs::read(file).is_ok_and(|bytes| is_ferryx_extension_bytes(&bytes))
            });
            let hook_enabled = !opt_outs.hooks.iter().any(|name| name == agent);
            let skill_enabled = !opt_outs.skills.iter().any(|name| name == agent);
            let mut entry = json!({
                "id": agent,
                "detected": detected,
                "hookSupported": hook.is_some(),
                "hookInstalled": hooks_now.is_some_and(|status| status.installed),
                "hookStale": hooks_now.is_some_and(|status| status.stale),
                "hookEnabled": hook_enabled,
                "skill": skill_now.as_str(),
                "skillInstalled": skill_now == "installed",
                "skillUserEdited": skill_now == "user-edited",
                "skillEnabled": skill_enabled,
                "extensionSupported": !extensions.is_empty(),
                "extensionInstalled": extension_installed,
                "integrationEnabled": enabled && hook_enabled,
            });
            if let Some(path) = skill_entry.get("path") {
                entry["skillPath"] = path.clone();
            }
            if skill_now == "unsupported" {
                entry["skillFallback"] = Value::from(SKILL_FALLBACK_COMMAND);
            }
            entry
        })
        .collect();
    json!({ "integrationEnabled": enabled, "agents": agents })
}

fn status_text(status: &Value) -> String {
    let mut lines = vec![format!(
        "agent integration: {}",
        if status["integrationEnabled"] == true { "on" } else { "off" }
    )];
    for agent in status["agents"].as_array().into_iter().flatten() {
        let hooks = if agent["hookSupported"] != true {
            "unsupported"
        } else if agent["hookStale"] == true {
            "stale"
        } else if agent["hookInstalled"] == true {
            "installed"
        } else {
            "absent"
        };
        let mut line = format!(
            "{:<12} {:<12} hooks: {:<11} skill: {:<11} extension: {}",
            agent["id"].as_str().unwrap_or_default(),
            if agent["detected"] == true { "detected" } else { "not-detected" },
            hooks,
            agent["skill"].as_str().unwrap_or_default(),
            if agent["extensionSupported"] != true {
                "unsupported"
            } else if agent["extensionInstalled"] == true {
                "installed"
            } else {
                "absent"
            },
        );
        if agent["skill"] == "unsupported" {
            line.push_str(&format!(" (use `{SKILL_FALLBACK_COMMAND}`)"));
        }
        lines.push(line);
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::agent_hook_install::{is_ferryx_skill, SKILL_SOURCE};

    struct Home {
        dir: tempfile::TempDir,
    }

    impl Home {
        fn new(agent_dirs: &[&str]) -> Self {
            let dir = tempfile::Builder::new().prefix("ferryx-integrations-").tempdir().unwrap();
            for agent_dir in agent_dirs {
                std::fs::create_dir_all(dir.path().join(agent_dir)).unwrap();
            }
            std::fs::create_dir_all(dir.path().join("bin")).unwrap();
            std::fs::write(dir.path().join("bin").join("ferryx-cli"), b"").unwrap();
            Self { dir }
        }

        fn path(&self) -> &Path {
            self.dir.path()
        }

        fn setting(&self) -> PathBuf {
            self.path().join("data").join("remote").join("agent-integration.json")
        }

        fn ctx(&self) -> Context {
            let home = self.path().as_os_str().to_os_string();
            Context::with_env(
                move |key| (key == "HOME").then(|| home.clone()),
                Some(self.path().join("bin").join("ferryx-cli")),
                Some(self.setting()),
            )
        }

        fn run(&self, line: &str) -> (i32, Value) {
            let words: Vec<&str> = line.split_whitespace().collect();
            let (code, out, err) = run_with(&self.ctx(), &words);
            let parsed = out
                .or(err)
                .map(|text| serde_json::from_str(&text).unwrap_or(Value::String(text)))
                .unwrap_or(Value::Null);
            (code, parsed)
        }

        /// Every regular file under the home except the setting, with its bytes.
        fn files(&self) -> Vec<(PathBuf, Vec<u8>)> {
            fn walk(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
                for entry in std::fs::read_dir(dir).unwrap() {
                    let path = entry.unwrap().path();
                    if path.is_dir() {
                        walk(&path, out);
                    } else {
                        out.push((path.clone(), std::fs::read(&path).unwrap()));
                    }
                }
            }
            let mut out = Vec::new();
            walk(self.path(), &mut out);
            out.retain(|(path, _)| !path.starts_with(self.path().join("data")));
            out.sort();
            out
        }
    }

    fn agent<'a>(status: &'a Value, id: &str) -> &'a Value {
        status["agents"].as_array().unwrap().iter().find(|entry| entry["id"] == id).unwrap()
    }

    #[test]
    fn status_json_has_one_entry_per_manifest_with_every_field() {
        let home = Home::new(&[".claude", ".omp"]);
        let (code, status) = home.run("status --json");
        assert_eq!(code, 0);
        assert_eq!(status["integrationEnabled"], true);
        let agents = status["agents"].as_array().unwrap();
        assert_eq!(agents.len(), SHIPPED_MANIFESTS.len());
        for entry in agents {
            for field in [
                "detected",
                "hookSupported",
                "hookInstalled",
                "hookStale",
                "hookEnabled",
                "skillInstalled",
                "skillUserEdited",
                "skillEnabled",
                "extensionSupported",
                "extensionInstalled",
                "integrationEnabled",
            ] {
                assert!(entry[field].is_boolean(), "{}: {field} in {entry}", entry["id"]);
            }
            assert!(entry["skill"].is_string(), "{entry}");
        }
        let claude = agent(&status, "claude");
        assert_eq!(claude["detected"], true);
        assert_eq!(claude["hookSupported"], true);
        assert_eq!(claude["skill"], "absent");
        let omp = agent(&status, "omp");
        assert_eq!((omp["detected"].clone(), omp["hookSupported"].clone()), (json!(true), json!(false)));
        assert_eq!(omp["extensionSupported"], true);
        assert_eq!(agent(&status, "codex")["detected"], false);
        // aider has no skill mechanism Ferryx writes to: the fallback is named instead.
        let aider = agent(&status, "aider");
        assert_eq!(aider["skill"], "unsupported");
        assert_eq!(aider["skillFallback"], SKILL_FALLBACK_COMMAND);
        let (code, text) = home.run("status");
        assert_eq!(code, 0);
        assert!(text.as_str().unwrap().contains("claude"), "{text}");
    }

    #[test]
    fn skill_install_writes_only_detected_agents_and_is_idempotent() {
        let home = Home::new(&[".claude", ".codex", ".omp"]);
        let (code, out) = home.run("skill install --all");
        assert_eq!(code, 0, "{out}");
        for (id, rel) in [
            ("claude", ".claude/skills/ferryx/SKILL.md"),
            ("codex", ".agents/skills/ferryx/SKILL.md"),
            ("omp", ".omp/agent/skills/ferryx/SKILL.md"),
        ] {
            assert_eq!(std::fs::read_to_string(home.path().join(rel)).unwrap(), SKILL_SOURCE, "{id}");
        }
        for absent in [".grok", ".gemini", ".cursor"] {
            assert!(!home.path().join(absent).exists(), "{absent} was created");
        }
        let (_, status) = home.run("status --json");
        assert_eq!(agent(&status, "claude")["skillInstalled"], true);
        assert_eq!(agent(&status, "omp")["skillInstalled"], true);

        let before = home.files();
        let (code, again) = home.run("skill install --all");
        assert_eq!(code, 0);
        assert!(
            again["results"].as_array().unwrap().iter().all(|r| r["result"] != "installed"),
            "{again}"
        );
        assert_eq!(home.files(), before, "a second install changes no file");
    }

    #[test]
    fn hooks_install_is_idempotent_and_uninstall_is_complete() {
        let home = Home::new(&[".claude", ".codex", ".cursor", ".cline", ".omp"]);
        let (code, out) = home.run("hooks install --all");
        assert_eq!(code, 0, "{out}");
        let (_, status) = home.run("status --json");
        for id in ["claude", "codex", "cursor", "cline"] {
            assert_eq!(agent(&status, id)["hookInstalled"], true, "{id}");
            assert_eq!(agent(&status, id)["hookStale"], false, "{id}");
        }
        let before = home.files();
        assert_eq!(home.run("hooks install --all").0, 0);
        assert_eq!(home.files(), before, "a second install changes no file");
        // omp runs no hooks.
        let (code, omp) = home.run("hooks install omp");
        assert_eq!(code, 0);
        assert_eq!(omp["results"][0]["result"], "unsupported");

        assert_eq!(home.run("skill install --all").0, 0);
        assert_eq!(home.run("hooks uninstall --all").0, 0);
        assert_eq!(home.run("skill uninstall --all").0, 0);
        let (_, status) = home.run("status --json");
        for entry in status["agents"].as_array().unwrap() {
            assert_eq!(entry["hookInstalled"], false, "{entry}");
            assert_eq!(entry["skillInstalled"], false, "{entry}");
        }
        for (path, bytes) in home.files() {
            let text = String::from_utf8_lossy(&bytes);
            assert!(!is_ferryx_skill(&text), "{} is still a managed skill", path.display());
            assert!(!text.contains(" agent-hook "), "{} still has a Ferryx hook: {text}", path.display());
        }
    }

    #[test]
    fn a_stale_reporter_is_reported() {
        let home = Home::new(&[".claude"]);
        let target = home.ctx().hooks.into_iter().find(|t| t.agent == "claude").unwrap();
        // Entries naming an executable that no longer exists, as after an update moved it.
        install_hooks_into(&target, &home.path().join("gone").join("ferryx-cli")).unwrap();
        let (_, status) = home.run("status --json");
        assert_eq!(agent(&status, "claude")["hookInstalled"], true);
        assert_eq!(agent(&status, "claude")["hookStale"], true);
        assert_eq!(home.run("hooks install claude").0, 0);
        let (_, status) = home.run("status --json");
        assert_eq!(agent(&status, "claude")["hookStale"], false);
    }

    #[test]
    fn a_user_edited_skill_survives_install_and_uninstall() {
        let home = Home::new(&[".claude"]);
        assert_eq!(home.run("skill install claude").0, 0);
        let skill = home.path().join(".claude/skills/ferryx/SKILL.md");
        // The user takes the skill over by deleting the marker, and adds a line of their own.
        let mine = format!("{}my own line\n", SKILL_SOURCE.replace("ferryx-managed: true\n", ""));
        std::fs::write(&skill, &mine).unwrap();
        let (_, status) = home.run("skill status claude");
        assert_eq!(status["skill"]["state"], "user-edited");

        let (code, out) = home.run("skill install claude");
        assert_eq!((code, out["results"][0]["result"].clone()), (0, json!("user-edited")));
        let (code, out) = home.run("skill uninstall claude");
        assert_eq!((code, out["results"][0]["result"].clone()), (0, json!("user-edited")));
        assert_eq!(std::fs::read_to_string(&skill).unwrap(), mine);
        let (_, status) = home.run("status --json");
        assert_eq!(agent(&status, "claude")["skillUserEdited"], true);
    }

    #[test]
    fn hooks_uninstall_then_boot_install_leaves_no_entry() {
        let home = Home::new(&[".claude", ".codex"]);
        assert_eq!(home.run("hooks install --all").0, 0);
        assert_eq!(home.run("hooks uninstall claude").0, 0);
        let (_, status) = home.run("status --json");
        assert_eq!(agent(&status, "claude")["hookEnabled"], false);
        assert_eq!(agent(&status, "claude")["integrationEnabled"], false);

        // What the daemon does on boot (server.rs -> install_agent_state_extension_if_enabled).
        let ctx = home.ctx();
        let setting = AgentIntegration::load(Some(home.setting()));
        assert!(crate::daemon::agent_extension::install_all_if_enabled(
            &setting,
            Vec::new(),
            Vec::new(),
            &ctx.hooks,
            &ctx.skills,
            ctx.exe.as_deref(),
        ));
        let claude = ctx.hooks.iter().find(|t| t.agent == "claude").unwrap();
        assert!(!hook_status(claude).unwrap().installed, "the boot install put claude's hooks back");
        let codex = ctx.hooks.iter().find(|t| t.agent == "codex").unwrap();
        assert!(hook_status(codex).unwrap().installed, "other agents keep theirs");

        // `hooks install` turns it back on for the next boot too.
        assert_eq!(home.run("hooks install claude").0, 0);
        assert!(read_opt_outs(Some(&home.setting())).hooks.is_empty());
    }

    #[test]
    fn unknown_agents_and_bad_usage_fail_with_exit_2() {
        let home = Home::new(&[]);
        let (code, err) = home.run("skill install nosuchagent");
        assert_eq!(code, EXIT_USAGE);
        assert_eq!(err["code"], "UNKNOWN_AGENT");
        assert_eq!(home.run("hooks uninstall nosuchagent").1["code"], "UNKNOWN_AGENT");
        assert_eq!(home.run("skill status nosuchagent").1["code"], "UNKNOWN_AGENT");
        for line in ["", "skill", "skill install", "hooks remove claude", "status --yaml", "skill install a b"] {
            let (code, err) = home.run(line);
            assert_eq!((code, err["code"].clone()), (EXIT_USAGE, json!("USAGE")), "{line:?}");
        }
        // Aliases resolve to the manifest id, case-insensitively.
        let (code, out) = home.run("skill status AGY");
        assert_eq!((code, out["agent"].clone()), (0, json!("antigravity")));
    }

    #[test]
    fn an_agent_without_a_skill_mechanism_gets_the_fallback() {
        let home = Home::new(&[]);
        let (code, out) = home.run("skill install aider");
        assert_eq!(code, 0);
        assert_eq!(out["results"][0]["skill"], "unsupported");
        assert_eq!(out["results"][0]["fallback"], SKILL_FALLBACK_COMMAND);
        assert!(crate::cli::api_catalog::describe_json("agents").is_ok(), "the fallback must exist");
    }
}
