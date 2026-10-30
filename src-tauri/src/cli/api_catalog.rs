//! `ferryx-cli api list | describe <group>`: a static, machine-readable catalogue of the
//! user-facing top-level command groups, so agents do not have to parse usage text.
//!
//! Each entry mirrors its group's real parser. A group that is invoked without a sub-verb
//! (`open`, `up`, `mcp`) has one subcommand with the empty name `""`. Multi-word sub-verbs
//! (`goal set`, `cron model`) are named with a single space between the words.
use serde_json::{json, Value};

/// Every user-facing top-level group `bin/cli.rs` dispatches on `args[1]`.
///
/// Deliberately excluded (not command groups): `--version`/`-V`/`version` (prints the
/// version line), `--daemon` (headless daemon), the Windows `--daemon-supervisor` watchdog,
/// and the internal ssh supervisor / askpass helper modes entered through environment.
pub const CLI_GROUPS: &[&str] = &[
    "agents",
    "supervisor",
    "hub",
    "mcp",
    "open",
    "up",
    "service",
    "account",
    "browser",
    "pair",
    "direct-trust",
    "remote",
    "api",
];

/// A positional argument.
pub struct Arg {
    pub name: &'static str,
    pub kind: &'static str,
    pub required: bool,
}

/// A `--flag`; `takes_value` is false for a boolean switch.
pub struct Flag {
    pub name: &'static str,
    pub takes_value: bool,
    pub required: bool,
}

pub struct Subcommand {
    pub name: &'static str,
    pub args: &'static [Arg],
    pub flags: &'static [Flag],
}

pub struct Command {
    pub name: &'static str,
    pub summary: &'static str,
    /// What stdout carries on success.
    pub output: &'static str,
    pub subcommands: &'static [Subcommand],
    pub exit_codes: &'static [(&'static str, &'static str)],
}

macro_rules! arg {
    ($name:expr, $kind:expr) => {
        Arg { name: $name, kind: $kind, required: true }
    };
}

macro_rules! opt_arg {
    ($name:expr, $kind:expr) => {
        Arg { name: $name, kind: $kind, required: false }
    };
}

/// An optional flag that takes a value.
macro_rules! value {
    ($name:expr) => {
        Flag { name: $name, takes_value: true, required: false }
    };
}

/// A required flag that takes a value.
macro_rules! required {
    ($name:expr) => {
        Flag { name: $name, takes_value: true, required: true }
    };
}

/// A boolean switch.
macro_rules! switch {
    ($name:expr) => {
        Flag { name: $name, takes_value: false, required: false }
    };
}

macro_rules! sub {
    ($name:expr, $args:expr, $flags:expr $(,)?) => {
        Subcommand { name: $name, args: $args, flags: $flags }
    };
}

const JSON: Flag = switch!("--json");
const HOST: Flag = value!("--host");
const REQUEST_ID: Flag = value!("--request-id");
const SESSION: Arg = arg!("session", "session-id");
const LANE: Arg = arg!("lane", "string");
const REVISION_ID: Arg = arg!("id", "integer");
const BROWSER_ID: Flag = required!("--browser-id");
const GENERATION: Flag = required!("--generation");
const REF: Flag = required!("--ref");
const PIN: Arg = arg!("pin", "string");
const ACCESS: Flag = value!("--access");

const DAEMON_JSON_EXIT_CODES: &[(&str, &str)] = &[
    ("0", "ok"),
    ("2", "invalid request (usage error, never contacts the daemon)"),
    ("3", "not found (no daemon, unknown session or target expired)"),
    ("4", "refused (unauthorized, conflict or unsupported)"),
    ("5", "other failure (timeout, outcome unknown, internal)"),
];

const OK_OR_FAILURE: &[(&str, &str)] = &[("0", "ok"), ("1", "failure, including usage errors")];

pub const CATALOG: &[Command] = &[
    Command {
        name: "agents",
        summary: "List, read, drive and wait on agent panes on this machine or a paired host",
        output: "json",
        subcommands: &[
            sub!("list", &[], &[HOST, JSON]),
            sub!("read", &[SESSION], &[HOST, JSON]),
            sub!("send", &[SESSION, arg!("text", "string")], &[REQUEST_ID, HOST, JSON]),
            sub!(
                "answer",
                &[SESSION],
                &[
                    required!("--prompt"),
                    required!("--revision"),
                    value!("--option"),
                    value!("--options"),
                    value!("--text"),
                    REQUEST_ID,
                    HOST,
                    JSON,
                ],
            ),
            sub!("stop", &[SESSION], &[REQUEST_ID, HOST, JSON]),
            sub!(
                "wait",
                &[SESSION],
                &[required!("--until"), value!("--after"), value!("--revision"), value!("--timeout-ms"), HOST, JSON],
            ),
            sub!("events", &[], &[value!("--after"), HOST, JSON]),
        ],
        exit_codes: DAEMON_JSON_EXIT_CODES,
    },
    Command {
        name: "supervisor",
        summary: "Manage supervisor goals, project setup, work dispatch and the supervisor engine",
        output: "json",
        subcommands: &[
            sub!(
                "goal set",
                &[LANE],
                &[value!("--requirement"), value!("--requirements-json"), value!("--mode"), value!("--turn-limit"), JSON],
            ),
            sub!("goal show", &[LANE], &[JSON]),
            sub!("goal mode", &[LANE, arg!("mode", "off|supervised|full")], &[JSON]),
            sub!("goal approve", &[LANE], &[JSON]),
            sub!("goal discard", &[LANE], &[JSON]),
            sub!("goal accept-revision", &[REVISION_ID], &[JSON]),
            sub!("goal reject-revision", &[REVISION_ID], &[JSON]),
            sub!(
                "setup",
                &[],
                &[
                    required!("--owner"),
                    required!("--project"),
                    required!("--repo-identity"),
                    required!("--host"),
                    required!("--git-common-dir"),
                    switch!("--remote"),
                    JSON,
                ],
            ),
            sub!("submit", &[], &[required!("--request-file"), HOST, value!("--session"), JSON]),
            sub!("dispatch status", &[], &[required!("--goal"), JSON]),
            sub!("request", &[], &[required!("--json")]),
            sub!("engine status", &[], &[JSON]),
            sub!("engine assess", &[], &[value!("--prompt"), value!("--goal"), JSON]),
        ],
        exit_codes: DAEMON_JSON_EXIT_CODES,
    },
    Command {
        name: "hub",
        summary: "Control the agent hub: lifecycle, omo migration, env import and cron jobs",
        output: "text",
        subcommands: &[
            sub!(
                "migrate",
                &[],
                &[value!("--from-omo"), value!("--cwd"), value!("--imported-from"), switch!("--dry-run")],
            ),
            sub!("stop", &[], &[]),
            sub!("start", &[], &[]),
            sub!("status", &[], &[switch!("--check")]),
            sub!("cutover", &[], &[]),
            sub!("rollback", &[], &[]),
            sub!("intake", &[], &[]),
            sub!("env import", &[], &[value!("--from-plist")]),
            sub!("env keys", &[], &[]),
            sub!("cron list", &[], &[]),
            sub!("cron model", &[arg!("job-id", "string"), opt_arg!("model", "provider/model")], &[switch!("--clear")]),
            sub!("cron approve-rerun", &[arg!("problem-id", "integer")], &[]),
        ],
        exit_codes: &[("0", "ok"), ("1", "failure or unhealthy status"), ("2", "usage error")],
    },
    Command {
        name: "mcp",
        summary: "Serve the Ferryx MCP server over stdio",
        output: "mcp-stdio",
        subcommands: &[sub!("", &[], &[])],
        exit_codes: &[("0", "server exited"), ("1", "startup failure")],
    },
    Command {
        name: "open",
        summary: "Open a file in the running Ferryx desktop app",
        output: "text",
        subcommands: &[sub!("", &[arg!("target", "path[:line[:col]]")], &[])],
        exit_codes: &[("0", "ok"), ("2", "failure (json error on stderr)")],
    },
    Command {
        name: "up",
        summary: "Enroll this machine with the Ferryx account and install the background service",
        output: "text or json (--json)",
        subcommands: &[sub!(
            "",
            &[],
            &[
                switch!("--key-stdin"),
                switch!("--key-env"),
                value!("--email"),
                value!("--origin"),
                value!("--name"),
                switch!("--no-service"),
                JSON,
                switch!("--help"),
            ],
        )],
        exit_codes: &[
            ("0", "ok"),
            ("1", "other failure (including service not installed)"),
            ("2", "usage error"),
            ("3", "install key or enroll code rejected"),
            ("4", "machine request denied"),
            ("5", "approval request expired"),
            ("6", "plan limit reached or remote suspended"),
        ],
    },
    Command {
        name: "service",
        summary: "Install, inspect or remove the OS background service for the daemon",
        output: "text or json (--json)",
        subcommands: &[sub!("install", &[], &[JSON]), sub!("status", &[], &[JSON]), sub!("uninstall", &[], &[JSON])],
        exit_codes: &[("0", "ok"), ("1", "failure (including install not completed)"), ("2", "usage error")],
    },
    Command {
        name: "account",
        summary: "Enroll with a code, log in, or show the account plan",
        output: "text or json (plan --json)",
        subcommands: &[
            sub!("enroll", &[], &[required!("--code"), value!("--origin")]),
            sub!("login", &[], &[value!("--email"), value!("--origin")]),
            sub!("plan", &[], &[JSON, value!("--origin")]),
        ],
        exit_codes: &[("0", "ok"), ("1", "failure, including usage errors"), ("3", "plan blocked")],
    },
    Command {
        name: "browser",
        summary: "Drive Ferryx browser panes: navigation, DOM actions, queries, tabs and state",
        output: "json",
        subcommands: &[
            sub!("list", &[], &[]),
            sub!("open", &[], &[required!("--url"), value!("--workspace"), value!("--worktree-path")]),
            sub!("navigate", &[], &[BROWSER_ID, required!("--url")]),
            sub!("goto", &[], &[BROWSER_ID, required!("--url")]),
            sub!("back", &[], &[BROWSER_ID]),
            sub!("forward", &[], &[BROWSER_ID]),
            sub!("reload", &[], &[BROWSER_ID]),
            sub!("close", &[], &[BROWSER_ID]),
            sub!("identify", &[], &[]),
            sub!("url", &[], &[BROWSER_ID]),
            sub!("title", &[], &[BROWSER_ID]),
            sub!("snapshot", &[], &[BROWSER_ID]),
            sub!("click", &[], &[BROWSER_ID, GENERATION, REF]),
            sub!("dblclick", &[], &[BROWSER_ID, GENERATION, REF]),
            sub!("hover", &[], &[BROWSER_ID, GENERATION, REF]),
            sub!("check", &[], &[BROWSER_ID, GENERATION, REF]),
            sub!("uncheck", &[], &[BROWSER_ID, GENERATION, REF]),
            sub!("scroll-into-view", &[], &[BROWSER_ID, GENERATION, REF]),
            sub!("select", &[], &[BROWSER_ID, GENERATION, REF, required!("--value")]),
            sub!("type", &[], &[BROWSER_ID, GENERATION, value!("--ref"), required!("--text")]),
            sub!("scroll", &[], &[BROWSER_ID, GENERATION, value!("--ref"), value!("--x"), value!("--y")]),
            sub!("fill", &[], &[BROWSER_ID, GENERATION, REF, required!("--value")]),
            sub!("keypress", &[], &[BROWSER_ID, GENERATION, required!("--key")]),
            sub!("eval", &[], &[BROWSER_ID, required!("--script")]),
            sub!(
                "wait",
                &[],
                &[
                    BROWSER_ID,
                    value!("--timeout-ms"),
                    value!("--selector"),
                    value!("--text"),
                    value!("--url-contains"),
                    value!("--load-state"),
                    value!("--function"),
                ],
            ),
            sub!("screenshot", &[], &[BROWSER_ID, required!("--out")]),
            sub!("highlight", &[], &[BROWSER_ID, required!("--selector"), value!("--duration-ms")]),
            sub!("console", &[], &[BROWSER_ID, switch!("--errors"), switch!("--clear")]),
            sub!("errors", &[], &[BROWSER_ID, switch!("--clear")]),
            sub!("focus", &[], &[BROWSER_ID, value!("--generation"), value!("--ref")]),
            sub!(
                "cookies",
                &[arg!("action", "get|set|clear"), opt_arg!("name", "string"), opt_arg!("value", "string")],
                &[BROWSER_ID, value!("--domain"), value!("--path")],
            ),
            sub!(
                "storage",
                &[
                    arg!("kind", "local|session"),
                    arg!("action", "get|set|clear"),
                    opt_arg!("key", "string"),
                    opt_arg!("value", "string"),
                ],
                &[BROWSER_ID, value!("--key"), value!("--value"), value!("--url"), value!("--domain"), switch!("--all")],
            ),
            sub!(
                "get",
                &[
                    arg!("what", "title|url|text|html|value|attr|count|box|styles"),
                    opt_arg!("selector", "css-selector"),
                    opt_arg!("attr", "string"),
                ],
                &[BROWSER_ID, value!("--selector"), value!("--attr"), value!("--name")],
            ),
            sub!(
                "is",
                &[arg!("state", "visible|enabled|checked"), opt_arg!("selector", "css-selector")],
                &[BROWSER_ID, value!("--selector")],
            ),
            sub!(
                "find",
                &[
                    arg!("by", "role|text|label|placeholder|alt|title|testid|first|last|nth"),
                    opt_arg!("value", "string"),
                    opt_arg!("index", "integer"),
                ],
                &[
                    BROWSER_ID,
                    value!("--selector"),
                    value!("--role"),
                    value!("--text"),
                    value!("--label"),
                    value!("--placeholder"),
                    value!("--alt"),
                    value!("--title"),
                    value!("--testid"),
                    value!("--id"),
                    value!("--value"),
                    value!("--index"),
                    value!("--nth"),
                ],
            ),
            sub!(
                "dialog",
                &[arg!("action", "accept|dismiss|list|policy"), opt_arg!("policy", "auto-dismiss|auto-accept")],
                &[BROWSER_ID, value!("--prompt-text"), value!("--value")],
            ),
            sub!(
                "download",
                &[opt_arg!("action", "list|cancel")],
                &[value!("--browser-id"), value!("--url"), value!("--path"), value!("--id")],
            ),
            sub!(
                "tab",
                &[arg!("action", "list|new|switch|close"), opt_arg!("target", "string")],
                &[
                    value!("--url"),
                    value!("--workspace"),
                    value!("--worktree-path"),
                    value!("--browser-id"),
                    value!("--index"),
                    value!("--target"),
                ],
            ),
            sub!(
                "state",
                &[arg!("action", "save|load"), opt_arg!("path", "path")],
                &[BROWSER_ID, value!("--out"), value!("--in"), value!("--file")],
            ),
        ],
        exit_codes: &[("0", "ok"), ("2", "invalid request or browser unavailable (json error on stderr)")],
    },
    Command {
        name: "pair",
        summary: "List paired devices, generate a pairing PIN, or approve a PIN",
        output: "text",
        subcommands: &[
            sub!("list", &[], &[]),
            sub!("generate", &[], &[ACCESS]),
            sub!("approve", &[PIN], &[]),
        ],
        exit_codes: &[("0", "ok"), ("1", "failure, including usage errors"), ("2", "account login required")],
    },
    Command {
        name: "direct-trust",
        summary: "Show this machine's direct-connection identity or pin/revoke a peer key",
        output: "text",
        subcommands: &[
            sub!("identity", &[], &[]),
            sub!(
                "provision",
                &[arg!("machineId", "string"), arg!("publicKeyBase64", "base64")],
                &[switch!("--replace")],
            ),
            sub!("revoke", &[arg!("machineId", "string")], &[]),
        ],
        exit_codes: OK_OR_FAILURE,
    },
    Command {
        name: "remote",
        summary: "Show remote access status or pair a device",
        output: "text or json (status --json)",
        subcommands: &[
            sub!("status", &[], &[JSON]),
            sub!("pair list", &[], &[]),
            sub!("pair generate", &[], &[ACCESS]),
            sub!("pair approve", &[PIN], &[]),
        ],
        exit_codes: OK_OR_FAILURE,
    },
    Command {
        name: "api",
        summary: "Describe the ferryx-cli command groups as JSON",
        output: "json",
        subcommands: &[sub!("list", &[], &[]), sub!("describe", &[arg!("name", "command-group")], &[])],
        exit_codes: &[("0", "ok"), ("2", "unknown command group or usage error")],
    },
];

const API_USAGE: &str = "usage: ferryx-cli api list | ferryx-cli api describe <command>";

/// `{"commands":[{"name","summary"}]}`.
pub fn list_json() -> Value {
    let commands: Vec<Value> = CATALOG
        .iter()
        .map(|command| json!({ "name": command.name, "summary": command.summary }))
        .collect();
    json!({ "commands": commands })
}

/// The full entry for one group, or an error message naming the known groups.
pub fn describe_json(name: &str) -> Result<Value, String> {
    let command = CATALOG
        .iter()
        .find(|command| command.name == name)
        .ok_or_else(|| format!("unknown command `{name}`; known: {}", CLI_GROUPS.join(", ")))?;
    let subcommands: Vec<Value> = command
        .subcommands
        .iter()
        .map(|sub| {
            let args: Vec<Value> = sub
                .args
                .iter()
                .map(|arg| json!({ "name": arg.name, "type": arg.kind, "required": arg.required }))
                .collect();
            let flags: Vec<Value> = sub
                .flags
                .iter()
                .map(|flag| json!({ "name": flag.name, "takesValue": flag.takes_value, "required": flag.required }))
                .collect();
            json!({ "name": sub.name, "args": args, "flags": flags })
        })
        .collect();
    let exit_codes: serde_json::Map<String, Value> = command
        .exit_codes
        .iter()
        .map(|(code, meaning)| ((*code).to_owned(), Value::from(*meaning)))
        .collect();
    Ok(json!({
        "name": command.name,
        "summary": command.summary,
        "output": command.output,
        "subcommands": subcommands,
        "exitCodes": exit_codes,
    }))
}

/// Entry for `ferryx-cli api ...`; returns the process exit code.
pub fn run_api_cli(args: &[String]) -> i32 {
    let words: Vec<&str> = args.iter().skip(2).map(String::as_str).collect();
    match words.as_slice() {
        ["list"] => {
            println!("{}", list_json());
            0
        }
        ["describe", name] => match describe_json(name) {
            Ok(entry) => {
                println!("{entry}");
                0
            }
            Err(message) => {
                eprintln!("{message}");
                2
            }
        },
        _ => {
            eprintln!("{API_USAGE}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn every_cli_group_is_described() {
        for group in CLI_GROUPS {
            let entry = describe_json(group).unwrap_or_else(|error| panic!("{group}: {error}"));
            assert_eq!(entry["name"], *group);
            assert!(
                entry["subcommands"].as_array().is_some_and(|subs| !subs.is_empty()),
                "{group} has no subcommands"
            );
            assert!(entry["exitCodes"]["0"].is_string(), "{group} documents no success exit code");
        }
        for command in CATALOG {
            assert!(CLI_GROUPS.contains(&command.name), "{} is catalogued but not a CLI group", command.name);
        }
    }

    #[test]
    fn api_list_has_one_parsable_entry_per_group() {
        let parsed: Value = serde_json::from_str(&list_json().to_string()).unwrap();
        let commands = parsed["commands"].as_array().unwrap();
        assert_eq!(commands.len(), CLI_GROUPS.len());
        for (command, group) in commands.iter().zip(CLI_GROUPS) {
            assert_eq!(command["name"], *group);
            assert!(command["summary"].as_str().is_some_and(|summary| !summary.is_empty()));
        }
    }

    #[test]
    fn describe_agents_carries_args_flags_and_exit_codes() {
        let entry = describe_json("agents").unwrap();
        let wait = entry["subcommands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|sub| sub["name"] == "wait")
            .unwrap();
        assert_eq!(wait["args"][0], json!({ "name": "session", "type": "session-id", "required": true }));
        assert!(wait["flags"]
            .as_array()
            .unwrap()
            .contains(&json!({ "name": "--until", "takesValue": true, "required": true })));
        assert_eq!(entry["exitCodes"].as_object().unwrap().len(), 5);
    }

    #[test]
    fn describe_unknown_is_an_error_with_exit_2() {
        assert!(describe_json("nope").is_err());
        assert_eq!(run_api_cli(&argv(&["ferryx-cli", "api", "describe", "nope"])), 2);
        assert_eq!(run_api_cli(&argv(&["ferryx-cli", "api"])), 2);
        assert_eq!(run_api_cli(&argv(&["ferryx-cli", "api", "list"])), 0);
    }
}
