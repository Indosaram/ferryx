use std::io::Write;
use std::sync::Arc;

pub mod agent_hook;
pub mod api_catalog;
pub mod billing;
pub mod direct_trust;
pub mod supervisor_engine;
pub mod supervisor_goal;
pub mod supervisor_dispatch;
pub mod supervisor_request;

pub use billing::AccountCliError;
pub use direct_trust::{
    parse_direct_trust_cli, run_direct_trust_cli, DirectTrustCliCommand, DIRECT_TRUST_USAGE,
};

use crate::browser::BrowserAutomationAction;
use crate::ipc::browser_cli::{send_browser_cli_request, BrowserCliRequest, BrowserCliResponse};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Gui,
    Daemon,
    /// `--daemon-supervisor`: the Windows watchdog the per-user scheduled task runs.
    DaemonSupervisor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFileCliCommand {
    pub path: String,
    pub line: Option<u32>,
    pub col: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserCliCommand {
    List,
    Open {
        url: String,
        workspace_id: Option<String>,
        worktree_path: Option<String>,
    },
    Navigate {
        browser_id: String,
        url: String,
    },
    Back {
        browser_id: String,
    },
    Forward {
        browser_id: String,
    },
    Reload {
        browser_id: String,
    },
    Close {
        browser_id: String,
    },
    Identify,
    Url {
        browser_id: String,
    },
    Title {
        browser_id: String,
    },
    Snapshot {
        browser_id: String,
    },
    Click {
        browser_id: String,
        generation: u64,
        reference: String,
    },
    Fill {
        browser_id: String,
        generation: u64,
        reference: String,
        value: String,
    },
    Keypress {
        browser_id: String,
        generation: u64,
        key: String,
    },
    Dblclick {
        browser_id: String,
        generation: u64,
        reference: String,
    },
    Hover {
        browser_id: String,
        generation: u64,
        reference: String,
    },
    Check {
        browser_id: String,
        generation: u64,
        reference: String,
    },
    Uncheck {
        browser_id: String,
        generation: u64,
        reference: String,
    },
    ScrollIntoView {
        browser_id: String,
        generation: u64,
        reference: String,
    },
    Select {
        browser_id: String,
        generation: u64,
        reference: String,
        value: String,
    },
    Type {
        browser_id: String,
        generation: u64,
        reference: Option<String>,
        text: String,
    },
    Scroll {
        browser_id: String,
        generation: u64,
        reference: Option<String>,
        x: Option<i64>,
        y: Option<i64>,
    },
    Eval {
        browser_id: String,
        script: String,
    },
    Wait {
        browser_id: String,
        condition: BrowserWaitCondition,
        timeout_ms: Option<u64>,
    },
    Screenshot {
        browser_id: String,
        out_path: String,
    },
    Console {
        browser_id: String,
        errors_only: bool,
        clear: bool,
    },
    Errors {
        browser_id: String,
        clear: bool,
    },
    Focus {
        browser_id: String,
        /// Element focus target. `None` keeps the row-1 webview focus path; `Some`
        /// maps to `BrowserAutomationAction::Focus` on the referenced element.
        generation: Option<u64>,
        reference: Option<String>,
    },
    Cookies {
        browser_id: String,
        action: CookieCliAction,
    },
    Storage {
        browser_id: String,
        kind: StorageCliKind,
        action: StorageCliAction,
    },
    Get {
        browser_id: String,
        subselector: BrowserGetSubselector,
    },
    Is {
        browser_id: String,
        subselector: BrowserIsSubselector,
    },
    Find {
        browser_id: String,
        subselector: BrowserFindSubselector,
    },
    Dialog {
        browser_id: String,
        action: DialogCliAction,
    },
    Download {
        action: DownloadCliAction,
    },
    Highlight {
        browser_id: String,
        selector: String,
        duration_ms: Option<u64>,
    },
    Tab {
        action: TabCliAction,
    },
    State {
        browser_id: String,
        action: StateCliAction,
    },
}

pub use crate::browser::model::BrowserWaitCondition;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserGetSubselector {
    Title,
    Url,
    Text { selector: String },
    Html { selector: String },
    Value { selector: String },
    Attr { selector: String, attribute: String },
    Count { selector: String },
    Box { selector: String },
    Styles { selector: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserIsSubselector {
    Visible { selector: String },
    Enabled { selector: String },
    Checked { selector: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserFindSubselector {
    Role { role: String },
    Text { text: String },
    Label { label: String },
    Placeholder { text: String },
    Alt { alt: String },
    Title { title: String },
    TestId { testid: String },
    First { selector: String },
    Last { selector: String },
    Nth { selector: String, index: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CookieCliAction {
    Get,
    Set {
        name: String,
        value: String,
        domain: Option<String>,
        path: Option<String>,
    },
    Clear {
        name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageCliKind {
    Local,
    Session,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageCliAction {
    Get { key: Option<String> },
    Set { key: String, value: String },
    Clear {
        key: Option<String>,
        all: bool,
        url: Option<String>,
        domain: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabCliAction {
    List,
    New {
        url: Option<String>,
        workspace_id: Option<String>,
        worktree_path: Option<String>,
    },
    Switch {
        target: String,
    },
    Close {
        target: Option<String>,
        browser_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateCliAction {
    Save { path: String },
    Load { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogCliAction {
    Accept { prompt_text: Option<String> },
    Dismiss,
    List,
    Policy { policy: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadCliAction {
    Start {
        browser_id: String,
        url: String,
        path: String,
    },
    List,
    Cancel { id: String },
}

/// Dialog policies the guest bridge actually implements (`browser/guest.rs`,
/// `recordDialog`): only `auto-accept` is special-cased and every other value
/// falls through to the auto-dismiss branch. Anything else is therefore rejected
/// at parse time instead of silently behaving like `auto-dismiss`.
const DIALOG_POLICIES: [&str; 2] = ["auto-dismiss", "auto-accept"];

fn parse_dialog_policy(value: &str) -> Result<String, String> {
    let normalized = value.trim();
    if DIALOG_POLICIES.contains(&normalized) {
        Ok(normalized.to_string())
    } else {
        Err(format!(
            "invalid dialog policy `{value}`: expected `auto-dismiss` or `auto-accept`"
        ))
    }
}

fn parse_required_generation(args: &[String]) -> Result<u64, String> {
    required_option(args, "--generation")?
        .parse()
        .map_err(|_| "--generation must be an unsigned integer".to_string())
}

fn parse_optional_i64_option(args: &[String], name: &str) -> Result<Option<i64>, String> {
    optional_option(args, name)?
        .map(|value| {
            value
                .parse::<i64>()
                .map_err(|_| format!("{name} must be an integer"))
        })
        .transpose()
}

const BROWSER_USAGE: &str =
    "expected `ferryx browser <list|open|navigate|goto|back|forward|reload|close|identify|url|title|snapshot|click|dblclick|hover|check|uncheck|scroll-into-view|select|type|scroll|fill|keypress|eval|wait|screenshot|highlight|console|errors|focus|cookies|storage|get|is|find|dialog|download|tab|state>`";

fn required_option(args: &[String], name: &str) -> Result<String, String> {
    let index = args
        .iter()
        .position(|arg| arg == name)
        .ok_or_else(|| format!("missing required option {name}"))?;
    args.get(index + 1)
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .ok_or_else(|| format!("missing value for {name}"))
}

fn optional_option(args: &[String], name: &str) -> Result<Option<String>, String> {
    let Some(index) = args.iter().position(|arg| arg == name) else {
        return Ok(None);
    };
    args.get(index + 1)
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .map(Some)
        .ok_or_else(|| format!("missing value for {name}"))
}

pub fn parse_browser_cli<I, T>(args: I) -> Result<BrowserCliCommand, String>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    if args.get(1).is_none_or(|arg| arg != "browser") {
        return Err(BROWSER_USAGE.into());
    }
    match args.get(2).map(String::as_str) {
        Some("list") => Ok(BrowserCliCommand::List),
        Some("open") => {
            let url = required_option(&args, "--url")?;
            let workspace_id = optional_option(&args, "--workspace")?.or_else(|| {
                std::env::var("FERRYX_WORKSPACE_ID")
                    .ok()
                    .filter(|v| !v.trim().is_empty())
            });
            let worktree_path = optional_option(&args, "--worktree-path")?.or_else(|| {
                std::env::var("FERRYX_WORKTREE_PATH")
                    .ok()
                    .filter(|v| !v.trim().is_empty())
            });
            Ok(BrowserCliCommand::Open {
                url,
                workspace_id,
                worktree_path,
            })
        }
        Some("navigate") | Some("goto") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let url = required_option(&args, "--url")?;
            Ok(BrowserCliCommand::Navigate { browser_id, url })
        }
        Some("back") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Back { browser_id })
        }
        Some("forward") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Forward { browser_id })
        }
        Some("reload") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Reload { browser_id })
        }
        Some("close") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Close { browser_id })
        }
        Some("identify") => Ok(BrowserCliCommand::Identify),
        Some("url") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Url { browser_id })
        }
        Some("title") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Title { browser_id })
        }
        Some("snapshot") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Snapshot { browser_id })
        }
        Some("click") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Click {
                browser_id,
                generation: required_option(&args, "--generation")?
                    .parse()
                    .map_err(|_| "--generation must be an unsigned integer")?,
                reference: required_option(&args, "--ref")?,
            })
        }
        Some("fill") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Fill {
                browser_id,
                generation: required_option(&args, "--generation")?
                    .parse()
                    .map_err(|_| "--generation must be an unsigned integer")?,
                reference: required_option(&args, "--ref")?,
                value: required_option(&args, "--value")?,
            })
        }
        Some("keypress") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Keypress {
                browser_id,
                generation: required_option(&args, "--generation")?
                    .parse()
                    .map_err(|_| "--generation must be an unsigned integer")?,
                key: required_option(&args, "--key")?,
            })
        }
        Some("dblclick") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Dblclick {
                browser_id,
                generation: parse_required_generation(&args)?,
                reference: required_option(&args, "--ref")?,
            })
        }
        Some("hover") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Hover {
                browser_id,
                generation: parse_required_generation(&args)?,
                reference: required_option(&args, "--ref")?,
            })
        }
        Some("check") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Check {
                browser_id,
                generation: parse_required_generation(&args)?,
                reference: required_option(&args, "--ref")?,
            })
        }
        Some("uncheck") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Uncheck {
                browser_id,
                generation: parse_required_generation(&args)?,
                reference: required_option(&args, "--ref")?,
            })
        }
        Some("scroll-into-view") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::ScrollIntoView {
                browser_id,
                generation: parse_required_generation(&args)?,
                reference: required_option(&args, "--ref")?,
            })
        }
        Some("select") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Select {
                browser_id,
                generation: parse_required_generation(&args)?,
                reference: required_option(&args, "--ref")?,
                value: required_option(&args, "--value")?,
            })
        }
        Some("type") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Type {
                browser_id,
                generation: parse_required_generation(&args)?,
                reference: optional_option(&args, "--ref")?,
                text: required_option(&args, "--text")?,
            })
        }
        Some("scroll") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Scroll {
                browser_id,
                generation: parse_required_generation(&args)?,
                reference: optional_option(&args, "--ref")?,
                x: parse_optional_i64_option(&args, "--x")?,
                y: parse_optional_i64_option(&args, "--y")?,
            })
        }
        Some("eval") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let script = required_option(&args, "--script")?;
            Ok(BrowserCliCommand::Eval { browser_id, script })
        }
        Some("wait") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let timeout_ms = optional_option(&args, "--timeout-ms")?
                .map(|v| {
                    v.parse::<u64>()
                        .map_err(|_| "--timeout-ms must be an unsigned integer")
                })
                .transpose()?;
            let condition = if let Some(selector) = optional_option(&args, "--selector")? {
                BrowserWaitCondition::Selector { selector }
            } else if let Some(text) = optional_option(&args, "--text")? {
                BrowserWaitCondition::Text { text }
            } else if let Some(fragment) = optional_option(&args, "--url-contains")? {
                BrowserWaitCondition::UrlContains { fragment }
            } else if let Some(state) = optional_option(&args, "--load-state")? {
                BrowserWaitCondition::LoadState { state }
            } else if let Some(script) = optional_option(&args, "--function")? {
                BrowserWaitCondition::Function { script }
            } else {
                return Err("missing wait condition: expected --selector, --text, --url-contains, --load-state, or --function".into());
            };
            Ok(BrowserCliCommand::Wait {
                browser_id,
                condition,
                timeout_ms,
            })
        }
        Some("screenshot") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let out_path = required_option(&args, "--out")?;
            Ok(BrowserCliCommand::Screenshot {
                browser_id,
                out_path,
            })
        }
        Some("console") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let errors_only = args.iter().any(|arg| arg == "--errors");
            let clear = args.iter().any(|arg| arg == "--clear");
            Ok(BrowserCliCommand::Console {
                browser_id,
                errors_only,
                clear,
            })
        }
        Some("errors") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let clear = args.iter().any(|arg| arg == "--clear");
            Ok(BrowserCliCommand::Errors { browser_id, clear })
        }
        Some("focus") => {
            let browser_id = required_option(&args, "--browser-id")?;
            // Without `--generation`/`--ref` this stays the webview focus path from
            // row 1. The element form needs BOTH flags: a half-specified pair is
            // rejected here so it can never silently focus the webview instead of
            // the element the caller named.
            let generation = optional_option(&args, "--generation")?
                .map(|value| {
                    value
                        .parse::<u64>()
                        .map_err(|_| "--generation must be an unsigned integer".to_string())
                })
                .transpose()?;
            let reference = optional_option(&args, "--ref")?;
            match (generation, reference) {
                (Some(generation), Some(reference)) => Ok(BrowserCliCommand::Focus {
                    browser_id,
                    generation: Some(generation),
                    reference: Some(reference),
                }),
                (None, None) => Ok(BrowserCliCommand::Focus {
                    browser_id,
                    generation: None,
                    reference: None,
                }),
                _ => Err(
                    "focus needs both --generation and --ref to focus an element (omit both to focus the webview)"
                        .to_string(),
                ),
            }
        }
        Some("highlight") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let selector = required_option(&args, "--selector")?;
            let duration_ms = optional_option(&args, "--duration-ms")?
                .map(|value| {
                    value
                        .parse::<u64>()
                        .map_err(|_| "--duration-ms must be an unsigned integer".to_string())
                })
                .transpose()?;
            Ok(BrowserCliCommand::Highlight {
                browser_id,
                selector,
                duration_ms,
            })
        }
        Some("cookies") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let mut positional = Vec::new();
            let mut i = 3;
            while i < args.len() {
                let arg = &args[i];
                if arg == "--browser-id" || arg == "--domain" || arg == "--path" {
                    i += 2;
                } else if arg.starts_with("--") {
                    i += 1;
                } else {
                    positional.push(arg.clone());
                    i += 1;
                }
            }
            let action = match positional.first().map(String::as_str) {
                Some("get") => CookieCliAction::Get,
                Some("set") => {
                    let name = positional
                        .get(1)
                        .cloned()
                        .ok_or("missing cookie name for set")?;
                    let value = positional
                        .get(2)
                        .cloned()
                        .ok_or("missing cookie value for set")?;
                    let domain = optional_option(&args, "--domain")?;
                    let path = optional_option(&args, "--path")?;
                    CookieCliAction::Set {
                        name,
                        value,
                        domain,
                        path,
                    }
                }
                Some("clear") => {
                    let name = positional
                        .get(1)
                        .cloned()
                        .ok_or("missing cookie name for clear")?;
                    CookieCliAction::Clear { name }
                }
                _ => return Err("expected cookies <get|set|clear>".into()),
            };
            Ok(BrowserCliCommand::Cookies { browser_id, action })
        }
        Some("storage") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let has_all = args.iter().any(|arg| arg == "--all");
            let url = optional_option(&args, "--url")?;
            let domain = optional_option(&args, "--domain")?;
            let mut positional = Vec::new();
            let mut i = 3;
            while i < args.len() {
                let arg = &args[i];
                if arg == "--browser-id" || arg == "--url" || arg == "--domain" || arg == "--key" || arg == "--value" {
                    i += 2;
                } else if arg.starts_with("--") {
                    i += 1;
                } else {
                    positional.push(arg.clone());
                    i += 1;
                }
            }
            let kind = match positional.first().map(String::as_str) {
                Some("local") => StorageCliKind::Local,
                Some("session") => StorageCliKind::Session,
                _ => return Err("expected storage kind `local` or `session`".into()),
            };
            let action = match positional.get(1).map(String::as_str) {
                Some("get") => StorageCliAction::Get {
                    key: positional.get(2).cloned().or(optional_option(&args, "--key")?),
                },
                Some("set") => {
                    let key = positional
                        .get(2)
                        .cloned()
                        .or(optional_option(&args, "--key")?)
                        .ok_or("missing key for storage set")?;
                    let value = positional
                        .get(3)
                        .cloned()
                        .or(optional_option(&args, "--value")?)
                        .ok_or("missing value for storage set")?;
                    StorageCliAction::Set { key, value }
                }
                Some("clear") => {
                    let key = positional.get(2).cloned().or(optional_option(&args, "--key")?);
                    if has_all && key.is_some() {
                        return Err("cannot specify both key and --all for storage clear".into());
                    }
                    StorageCliAction::Clear {
                        key,
                        all: has_all,
                        url,
                        domain,
                    }
                }
                _ => return Err("expected storage action `get`, `set`, or `clear`".into()),
            };
            Ok(BrowserCliCommand::Storage {
                browser_id,
                kind,
                action,
            })
        }
        Some("get") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let mut positional = Vec::new();
            let mut i = 3;
            while i < args.len() {
                let arg = &args[i];
                if arg == "--browser-id"
                    || arg == "--selector"
                    || arg == "--attr"
                    || arg == "--name"
                {
                    i += 2;
                } else if arg.starts_with("--") {
                    i += 1;
                } else {
                    positional.push(arg.clone());
                    i += 1;
                }
            }
            let subselector = match positional.first().map(String::as_str) {
                Some("title") => BrowserGetSubselector::Title,
                Some("url") => BrowserGetSubselector::Url,
                Some("text") => {
                    let selector = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing selector for get text")?;
                    BrowserGetSubselector::Text { selector }
                }
                Some("html") => {
                    let selector = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing selector for get html")?;
                    BrowserGetSubselector::Html { selector }
                }
                Some("value") => {
                    let selector = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing selector for get value")?;
                    BrowserGetSubselector::Value { selector }
                }
                Some("attr") => {
                    let selector = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing selector for get attr")?;
                    let attribute = positional
                        .get(2)
                        .cloned()
                        .or(optional_option(&args, "--attr")?)
                        .or(optional_option(&args, "--name")?)
                        .ok_or("missing attribute name for get attr")?;
                    BrowserGetSubselector::Attr { selector, attribute }
                }
                Some("count") => {
                    let selector = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing selector for get count")?;
                    BrowserGetSubselector::Count { selector }
                }
                Some("box") => {
                    let selector = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing selector for get box")?;
                    BrowserGetSubselector::Box { selector }
                }
                Some("styles") => {
                    let selector = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing selector for get styles")?;
                    BrowserGetSubselector::Styles { selector }
                }
                _ => {
                    return Err(
                        "expected get <title|url|text|html|value|attr|count|box|styles>".into(),
                    )
                }
            };
            Ok(BrowserCliCommand::Get {
                browser_id,
                subselector,
            })
        }
        Some("is") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let mut positional = Vec::new();
            let mut i = 3;
            while i < args.len() {
                let arg = &args[i];
                if arg == "--browser-id" || arg == "--selector" {
                    i += 2;
                } else if arg.starts_with("--") {
                    i += 1;
                } else {
                    positional.push(arg.clone());
                    i += 1;
                }
            }
            let subselector = match positional.first().map(String::as_str) {
                Some("visible") => {
                    let selector = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing selector for is visible")?;
                    BrowserIsSubselector::Visible { selector }
                }
                Some("enabled") => {
                    let selector = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing selector for is enabled")?;
                    BrowserIsSubselector::Enabled { selector }
                }
                Some("checked") => {
                    let selector = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing selector for is checked")?;
                    BrowserIsSubselector::Checked { selector }
                }
                _ => return Err("expected is <visible|enabled|checked>".into()),
            };
            Ok(BrowserCliCommand::Is {
                browser_id,
                subselector,
            })
        }
        Some("find") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let mut positional = Vec::new();
            let mut i = 3;
            while i < args.len() {
                let arg = &args[i];
                if arg == "--browser-id"
                    || arg == "--selector"
                    || arg == "--role"
                    || arg == "--text"
                    || arg == "--label"
                    || arg == "--placeholder"
                    || arg == "--alt"
                    || arg == "--title"
                    || arg == "--testid"
                    || arg == "--id"
                    || arg == "--value"
                    || arg == "--index"
                    || arg == "--nth"
                {
                    i += 2;
                } else if arg.starts_with("--") {
                    i += 1;
                } else {
                    positional.push(arg.clone());
                    i += 1;
                }
            }
            let subselector = match positional.first().map(String::as_str) {
                Some("role") => {
                    let role = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--role")?)
                        .or(optional_option(&args, "--value")?)
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing role for find role")?;
                    BrowserFindSubselector::Role { role }
                }
                Some("text") => {
                    let text = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--text")?)
                        .or(optional_option(&args, "--value")?)
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing text for find text")?;
                    BrowserFindSubselector::Text { text }
                }
                Some("label") => {
                    let label = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--label")?)
                        .or(optional_option(&args, "--value")?)
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing label for find label")?;
                    BrowserFindSubselector::Label { label }
                }
                Some("placeholder") => {
                    let text = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--placeholder")?)
                        .or(optional_option(&args, "--value")?)
                        .or(optional_option(&args, "--text")?)
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing placeholder for find placeholder")?;
                    BrowserFindSubselector::Placeholder { text }
                }
                Some("alt") => {
                    let alt = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--alt")?)
                        .or(optional_option(&args, "--value")?)
                        .or(optional_option(&args, "--text")?)
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing alt for find alt")?;
                    BrowserFindSubselector::Alt { alt }
                }
                Some("title") => {
                    let title = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--title")?)
                        .or(optional_option(&args, "--value")?)
                        .or(optional_option(&args, "--text")?)
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing title for find title")?;
                    BrowserFindSubselector::Title { title }
                }
                Some("testid") => {
                    let testid = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--testid")?)
                        .or(optional_option(&args, "--id")?)
                        .or(optional_option(&args, "--value")?)
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing testid for find testid")?;
                    BrowserFindSubselector::TestId { testid }
                }
                Some("first") => {
                    let selector = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing selector for find first")?;
                    BrowserFindSubselector::First { selector }
                }
                Some("last") => {
                    let selector = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing selector for find last")?;
                    BrowserFindSubselector::Last { selector }
                }
                Some("nth") => {
                    let selector = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--selector")?)
                        .ok_or("missing selector for find nth")?;
                    let raw_index = positional
                        .get(2)
                        .cloned()
                        .or(optional_option(&args, "--index")?)
                        .or(optional_option(&args, "--nth")?)
                        .ok_or("missing index for find nth")?;
                    let index: usize = raw_index
                        .parse()
                        .map_err(|_| "index for find nth must be a non-negative integer")?;
                    BrowserFindSubselector::Nth { selector, index }
                }
                _ => {
                    return Err(
                        "expected find <role|text|label|placeholder|alt|title|testid|first|last|nth>".into(),
                    )
                }
            };
            Ok(BrowserCliCommand::Find {
                browser_id,
                subselector,
            })
        }
        Some("dialog") => {
            let browser_id = required_option(&args, "--browser-id")?;
            // `--browser-id` may precede the sub-verb, so collect the positionals
            // instead of indexing a fixed slot.
            let mut positional = Vec::new();
            let mut i = 3;
            while i < args.len() {
                let arg = &args[i];
                if arg == "--browser-id" || arg == "--prompt-text" || arg == "--value" {
                    i += 2;
                } else if arg.starts_with("--") {
                    i += 1;
                } else {
                    positional.push(arg.clone());
                    i += 1;
                }
            }
            let action = match positional.first().map(String::as_str) {
                Some("accept") => DialogCliAction::Accept {
                    prompt_text: optional_option(&args, "--prompt-text")?,
                },
                Some("dismiss") => DialogCliAction::Dismiss,
                Some("list") => DialogCliAction::List,
                Some("policy") => {
                    let raw = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--value")?)
                        .ok_or("missing policy value for dialog policy")?;
                    DialogCliAction::Policy {
                        policy: parse_dialog_policy(&raw)?,
                    }
                }
                _ => return Err("expected dialog <accept|dismiss|list|policy>".into()),
            };
            Ok(BrowserCliCommand::Dialog { browser_id, action })
        }
        Some("download") => {
            let mut positional = Vec::new();
            let mut i = 3;
            while i < args.len() {
                let arg = &args[i];
                if arg == "--browser-id" || arg == "--url" || arg == "--path" || arg == "--id" {
                    i += 2;
                } else if arg.starts_with("--") {
                    i += 1;
                } else {
                    positional.push(arg.clone());
                    i += 1;
                }
            }
            let action = match positional.first().map(String::as_str) {
                Some("list") => DownloadCliAction::List,
                Some("cancel") => DownloadCliAction::Cancel {
                    id: required_option(&args, "--id")?,
                },
                _ => DownloadCliAction::Start {
                    browser_id: required_option(&args, "--browser-id")?,
                    url: required_option(&args, "--url")?,
                    path: required_option(&args, "--path")?,
                },
            };
            Ok(BrowserCliCommand::Download { action })
        }
        Some("tab") => {
            let mut positional = Vec::new();
            let mut i = 3;
            while i < args.len() {
                let arg = &args[i];
                if arg == "--url"
                    || arg == "--workspace"
                    || arg == "--worktree-path"
                    || arg == "--browser-id"
                    || arg == "--index"
                    || arg == "--target"
                {
                    i += 2;
                } else if arg.starts_with("--") {
                    i += 1;
                } else {
                    positional.push(arg.clone());
                    i += 1;
                }
            }
            let sub_verb = positional.first().map(String::as_str);
            let action = match sub_verb {
                Some("list") => TabCliAction::List,
                Some("new") => {
                    let url = positional.get(1).cloned().or(optional_option(&args, "--url")?);
                    let workspace_id = optional_option(&args, "--workspace")?.or_else(|| {
                        std::env::var("FERRYX_WORKSPACE_ID")
                            .ok()
                            .filter(|v| !v.trim().is_empty())
                    });
                    let worktree_path = optional_option(&args, "--worktree-path")?.or_else(|| {
                        std::env::var("FERRYX_WORKTREE_PATH")
                            .ok()
                            .filter(|v| !v.trim().is_empty())
                    });
                    TabCliAction::New {
                        url,
                        workspace_id,
                        worktree_path,
                    }
                }
                Some("switch") => {
                    let target = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--target")?)
                        .or(optional_option(&args, "--index")?)
                        .or(optional_option(&args, "--browser-id")?)
                        .ok_or("missing tab index or browser id for tab switch")?;
                    TabCliAction::Switch { target }
                }
                Some("close") => {
                    let target = positional.get(1).cloned();
                    let browser_id = optional_option(&args, "--browser-id")?;
                    TabCliAction::Close { target, browser_id }
                }
                Some(other) => {
                    return Err(format!(
                        "unknown tab sub-verb `{other}`: expected tab <list|new|switch|close>"
                    ))
                }
                None => return Err("expected tab <list|new|switch|close>".into()),
            };
            Ok(BrowserCliCommand::Tab { action })
        }
        Some("state") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let mut positional = Vec::new();
            let mut i = 3;
            while i < args.len() {
                let arg = &args[i];
                if arg == "--browser-id" || arg == "--out" || arg == "--in" || arg == "--file" {
                    i += 2;
                } else if arg.starts_with("--") {
                    i += 1;
                } else {
                    positional.push(arg.clone());
                    i += 1;
                }
            }
            let sub_verb = positional.first().map(String::as_str);
            let action = match sub_verb {
                Some("save") => {
                    let path = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--out")?)
                        .or(optional_option(&args, "--file")?)
                        .ok_or("missing file path for state save")?;
                    StateCliAction::Save { path }
                }
                Some("load") => {
                    let path = positional
                        .get(1)
                        .cloned()
                        .or(optional_option(&args, "--in")?)
                        .or(optional_option(&args, "--file")?)
                        .ok_or("missing file path for state load")?;
                    StateCliAction::Load { path }
                }
                Some(other) => {
                    return Err(format!(
                        "unknown state sub-verb `{other}`: expected state <save|load>"
                    ))
                }
                None => return Err("expected state <save|load>".into()),
            };
            Ok(BrowserCliCommand::State { browser_id, action })
        }
        _ => Err(BROWSER_USAGE.into()),
    }
}

pub fn build_dom_get_script(subselector: &BrowserGetSubselector) -> String {
    match subselector {
        BrowserGetSubselector::Title => {
            "(() => { return JSON.stringify(document.title); })()".into()
        }
        BrowserGetSubselector::Url => {
            "(() => { return JSON.stringify(location.href); })()".into()
        }
        BrowserGetSubselector::Text { selector } => {
            let sel = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
            format!(
                r#"(() => {{
  const el = document.querySelector({sel});
  if (!el) return JSON.stringify({{ error: "BROWSER_NOT_FOUND", message: "element not found" }});
  return JSON.stringify(el.innerText || el.textContent || "");
}})()"#
            )
        }
        BrowserGetSubselector::Html { selector } => {
            let sel = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
            format!(
                r#"(() => {{
  const el = document.querySelector({sel});
  if (!el) return JSON.stringify({{ error: "BROWSER_NOT_FOUND", message: "element not found" }});
  return JSON.stringify(el.outerHTML);
}})()"#
            )
        }
        BrowserGetSubselector::Value { selector } => {
            let sel = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
            format!(
                r#"(() => {{
  const el = document.querySelector({sel});
  if (!el) return JSON.stringify({{ error: "BROWSER_NOT_FOUND", message: "element not found" }});
  return JSON.stringify(el.value !== undefined ? el.value : null);
}})()"#
            )
        }
        BrowserGetSubselector::Attr {
            selector,
            attribute,
        } => {
            let sel = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
            let attr = serde_json::to_string(attribute).unwrap_or_else(|_| "\"\"".into());
            format!(
                r#"(() => {{
  const el = document.querySelector({sel});
  if (!el) return JSON.stringify({{ error: "BROWSER_NOT_FOUND", message: "element not found" }});
  return JSON.stringify(el.getAttribute({attr}));
}})()"#
            )
        }
        BrowserGetSubselector::Count { selector } => {
            let sel = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
            format!(
                r#"(() => {{
  const list = document.querySelectorAll({sel});
  return JSON.stringify(list.length);
}})()"#
            )
        }
        BrowserGetSubselector::Box { selector } => {
            let sel = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
            format!(
                r#"(() => {{
  const el = document.querySelector({sel});
  if (!el) return JSON.stringify({{ error: "BROWSER_NOT_FOUND", message: "element not found" }});
  const r = el.getBoundingClientRect();
  return JSON.stringify({{ x: r.x, y: r.y, width: r.width, height: r.height, top: r.top, right: r.right, bottom: r.bottom, left: r.left }});
}})()"#
            )
        }
        BrowserGetSubselector::Styles { selector } => {
            let sel = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
            format!(
                r#"(() => {{
  const el = document.querySelector({sel});
  if (!el) return JSON.stringify({{ error: "BROWSER_NOT_FOUND", message: "element not found" }});
  const cs = window.getComputedStyle(el);
  const dump = {{}};
  for (let i = 0; i < cs.length; i++) {{
    const prop = cs[i];
    dump[prop] = cs.getPropertyValue(prop);
  }}
  return JSON.stringify(dump);
}})()"#
            )
        }
    }
}

pub fn build_dom_is_script(subselector: &BrowserIsSubselector) -> String {
    match subselector {
        BrowserIsSubselector::Visible { selector } => {
            let sel = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
            format!(
                r#"(() => {{
  const el = document.querySelector({sel});
  if (!el) return JSON.stringify({{ error: "BROWSER_NOT_FOUND", message: "element not found" }});
  const style = window.getComputedStyle(el);
  const isVisible = !(style.display === "none" || style.visibility === "hidden" || style.opacity === "0" || (el.offsetParent === null && el.tagName !== "BODY"));
  return JSON.stringify(isVisible);
}})()"#
            )
        }
        BrowserIsSubselector::Enabled { selector } => {
            let sel = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
            format!(
                r#"(() => {{
  const el = document.querySelector({sel});
  if (!el) return JSON.stringify({{ error: "BROWSER_NOT_FOUND", message: "element not found" }});
  return JSON.stringify(!el.disabled);
}})()"#
            )
        }
        BrowserIsSubselector::Checked { selector } => {
            let sel = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
            format!(
                r#"(() => {{
  const el = document.querySelector({sel});
  if (!el) return JSON.stringify({{ error: "BROWSER_NOT_FOUND", message: "element not found" }});
  return JSON.stringify(Boolean(el.checked));
}})()"#
            )
        }
    }
}

pub fn build_dom_find_script(subselector: &BrowserFindSubselector) -> String {
    match subselector {
        BrowserFindSubselector::Role { role } => {
            let role_json = serde_json::to_string(role).unwrap_or_else(|_| "\"\"".into());
            format!(
                r##"(() => {{
  const target = {role_json}.toLowerCase();
  const candidates = Array.from(document.querySelectorAll(
    'a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]'
  ));
  const refFor = (el) => {{
    let idx = candidates.indexOf(el);
    if (idx < 0) {{
      const parent = el.closest('a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]');
      if (parent) idx = candidates.indexOf(parent);
    }}
    if (idx >= 0) return 'e' + (idx + 1);
    const all = Array.from(document.querySelectorAll('*'));
    const allIdx = all.indexOf(el);
    return 'e' + (candidates.length + (allIdx >= 0 ? allIdx + 1 : 1));
  }};
  const roleFor = (el) => (el.getAttribute('role') || el.tagName.toLowerCase()).toLowerCase();
  const elements = Array.from(document.querySelectorAll('*')).filter((el) => roleFor(el) === target);
  const refs = [];
  for (const el of elements) {{
    const r = refFor(el);
    if (r && !refs.includes(r)) refs.push(r);
  }}
  return JSON.stringify(refs);
}})()"##
            )
        }
        BrowserFindSubselector::Text { text } => {
            let text_json = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into());
            format!(
                r##"(() => {{
  const query = {text_json};
  const candidates = Array.from(document.querySelectorAll(
    'a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]'
  ));
  const refFor = (el) => {{
    let idx = candidates.indexOf(el);
    if (idx < 0) {{
      const parent = el.closest('a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]');
      if (parent) idx = candidates.indexOf(parent);
    }}
    if (idx >= 0) return 'e' + (idx + 1);
    const all = Array.from(document.querySelectorAll('*'));
    const allIdx = all.indexOf(el);
    return 'e' + (candidates.length + (allIdx >= 0 ? allIdx + 1 : 1));
  }};
  const matching = candidates.filter((el) => {{
    const content = (el.innerText || el.textContent || '').trim();
    const val = el.value !== undefined ? String(el.value) : '';
    const aria = el.getAttribute('aria-label') || '';
    const title = el.getAttribute('title') || '';
    const alt = el.getAttribute('alt') || '';
    return content.includes(query) || val.includes(query) || aria.includes(query) || title.includes(query) || alt.includes(query);
  }});
  const refs = [];
  for (const el of matching) {{
    const r = refFor(el);
    if (r && !refs.includes(r)) refs.push(r);
  }}
  if (refs.length === 0) {{
    const all = Array.from(document.querySelectorAll('*')).filter((el) => {{
      const content = (el.innerText || el.textContent || '').trim();
      return content.includes(query);
    }});
    for (const el of all) {{
      const r = refFor(el);
      if (r && !refs.includes(r)) refs.push(r);
    }}
  }}
  return JSON.stringify(refs);
}})()"##
            )
        }
        BrowserFindSubselector::Label { label } => {
            let label_json = serde_json::to_string(label).unwrap_or_else(|_| "\"\"".into());
            format!(
                r##"(() => {{
  const query = {label_json};
  const candidates = Array.from(document.querySelectorAll(
    'a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]'
  ));
  const refFor = (el) => {{
    let idx = candidates.indexOf(el);
    if (idx < 0) {{
      const parent = el.closest('a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]');
      if (parent) idx = candidates.indexOf(parent);
    }}
    if (idx >= 0) return 'e' + (idx + 1);
    const all = Array.from(document.querySelectorAll('*'));
    const allIdx = all.indexOf(el);
    return 'e' + (candidates.length + (allIdx >= 0 ? allIdx + 1 : 1));
  }};
  const labelFor = (el) => {{
    const aria = el.getAttribute('aria-label');
    if (aria) return aria;
    const labelledby = el.getAttribute('aria-labelledby');
    if (labelledby) {{
      const target = document.getElementById(labelledby);
      if (target) return (target.innerText || target.textContent || '').trim();
    }}
    if (el.id) {{
      const escape = window.CSS && CSS.escape ? CSS.escape(el.id) : el.id;
      const lbl = document.querySelector('label[for="' + escape + '"]');
      if (lbl) return (lbl.innerText || lbl.textContent || '').trim();
    }}
    const parentLabel = el.closest('label');
    if (parentLabel) return (parentLabel.innerText || parentLabel.textContent || '').trim();
    return '';
  }};
  const matching = Array.from(document.querySelectorAll('*')).filter((el) => {{
    const lbl = labelFor(el);
    return lbl && lbl.includes(query);
  }});
  const refs = [];
  for (const el of matching) {{
    const r = refFor(el);
    if (r && !refs.includes(r)) refs.push(r);
  }}
  return JSON.stringify(refs);
}})()"##
            )
        }
        BrowserFindSubselector::Placeholder { text } => {
            let text_json = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into());
            format!(
                r##"(() => {{
  const query = {text_json};
  const candidates = Array.from(document.querySelectorAll(
    'a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]'
  ));
  const refFor = (el) => {{
    let idx = candidates.indexOf(el);
    if (idx < 0) {{
      const parent = el.closest('a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]');
      if (parent) idx = candidates.indexOf(parent);
    }}
    if (idx >= 0) return 'e' + (idx + 1);
    const all = Array.from(document.querySelectorAll('*'));
    const allIdx = all.indexOf(el);
    return 'e' + (candidates.length + (allIdx >= 0 ? allIdx + 1 : 1));
  }};
  const matching = Array.from(document.querySelectorAll('[placeholder]')).filter((el) => {{
    const ph = el.getAttribute('placeholder') || '';
    return ph.includes(query);
  }});
  const refs = [];
  for (const el of matching) {{
    const r = refFor(el);
    if (r && !refs.includes(r)) refs.push(r);
  }}
  return JSON.stringify(refs);
}})()"##
            )
        }
        BrowserFindSubselector::Alt { alt } => {
            let alt_json = serde_json::to_string(alt).unwrap_or_else(|_| "\"\"".into());
            format!(
                r##"(() => {{
  const query = {alt_json};
  const candidates = Array.from(document.querySelectorAll(
    'a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]'
  ));
  const refFor = (el) => {{
    let idx = candidates.indexOf(el);
    if (idx < 0) {{
      const parent = el.closest('a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]');
      if (parent) idx = candidates.indexOf(parent);
    }}
    if (idx >= 0) return 'e' + (idx + 1);
    const all = Array.from(document.querySelectorAll('*'));
    const allIdx = all.indexOf(el);
    return 'e' + (candidates.length + (allIdx >= 0 ? allIdx + 1 : 1));
  }};
  const matching = Array.from(document.querySelectorAll('[alt]')).filter((el) => {{
    const a = el.getAttribute('alt') || '';
    return a.includes(query);
  }});
  const refs = [];
  for (const el of matching) {{
    const r = refFor(el);
    if (r && !refs.includes(r)) refs.push(r);
  }}
  return JSON.stringify(refs);
}})()"##
            )
        }
        BrowserFindSubselector::Title { title } => {
            let title_json = serde_json::to_string(title).unwrap_or_else(|_| "\"\"".into());
            format!(
                r##"(() => {{
  const query = {title_json};
  const candidates = Array.from(document.querySelectorAll(
    'a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]'
  ));
  const refFor = (el) => {{
    let idx = candidates.indexOf(el);
    if (idx < 0) {{
      const parent = el.closest('a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]');
      if (parent) idx = candidates.indexOf(parent);
    }}
    if (idx >= 0) return 'e' + (idx + 1);
    const all = Array.from(document.querySelectorAll('*'));
    const allIdx = all.indexOf(el);
    return 'e' + (candidates.length + (allIdx >= 0 ? allIdx + 1 : 1));
  }};
  const matching = Array.from(document.querySelectorAll('[title]')).filter((el) => {{
    const t = el.getAttribute('title') || '';
    return t.includes(query);
  }});
  const refs = [];
  for (const el of matching) {{
    const r = refFor(el);
    if (r && !refs.includes(r)) refs.push(r);
  }}
  return JSON.stringify(refs);
}})()"##
            )
        }
        BrowserFindSubselector::TestId { testid } => {
            let testid_json = serde_json::to_string(testid).unwrap_or_else(|_| "\"\"".into());
            format!(
                r##"(() => {{
  const query = {testid_json};
  const candidates = Array.from(document.querySelectorAll(
    'a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]'
  ));
  const refFor = (el) => {{
    let idx = candidates.indexOf(el);
    if (idx < 0) {{
      const parent = el.closest('a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]');
      if (parent) idx = candidates.indexOf(parent);
    }}
    if (idx >= 0) return 'e' + (idx + 1);
    const all = Array.from(document.querySelectorAll('*'));
    const allIdx = all.indexOf(el);
    return 'e' + (candidates.length + (allIdx >= 0 ? allIdx + 1 : 1));
  }};
  const matching = Array.from(document.querySelectorAll(
    '[data-testid], [data-test-id], [data-test]'
  )).filter((el) => {{
    const tid = el.getAttribute('data-testid') || el.getAttribute('data-test-id') || el.getAttribute('data-test') || '';
    return tid === query || tid.includes(query);
  }});
  const refs = [];
  for (const el of matching) {{
    const r = refFor(el);
    if (r && !refs.includes(r)) refs.push(r);
  }}
  return JSON.stringify(refs);
}})()"##
            )
        }
        BrowserFindSubselector::First { selector } => {
            let sel_json = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
            format!(
                r##"(() => {{
  const candidates = Array.from(document.querySelectorAll(
    'a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]'
  ));
  const refFor = (el) => {{
    let idx = candidates.indexOf(el);
    if (idx < 0) {{
      const parent = el.closest('a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]');
      if (parent) idx = candidates.indexOf(parent);
    }}
    if (idx >= 0) return 'e' + (idx + 1);
    const all = Array.from(document.querySelectorAll('*'));
    const allIdx = all.indexOf(el);
    return 'e' + (candidates.length + (allIdx >= 0 ? allIdx + 1 : 1));
  }};
  const el = document.querySelector({sel_json});
  if (!el) return JSON.stringify([]);
  const r = refFor(el);
  return JSON.stringify(r ? [r] : []);
}})()"##
            )
        }
        BrowserFindSubselector::Last { selector } => {
            let sel_json = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
            format!(
                r##"(() => {{
  const candidates = Array.from(document.querySelectorAll(
    'a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]'
  ));
  const refFor = (el) => {{
    let idx = candidates.indexOf(el);
    if (idx < 0) {{
      const parent = el.closest('a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]');
      if (parent) idx = candidates.indexOf(parent);
    }}
    if (idx >= 0) return 'e' + (idx + 1);
    const all = Array.from(document.querySelectorAll('*'));
    const allIdx = all.indexOf(el);
    return 'e' + (candidates.length + (allIdx >= 0 ? allIdx + 1 : 1));
  }};
  const list = document.querySelectorAll({sel_json});
  if (list.length === 0) return JSON.stringify([]);
  const el = list[list.length - 1];
  const r = refFor(el);
  return JSON.stringify(r ? [r] : []);
}})()"##
            )
        }
        BrowserFindSubselector::Nth { selector, index } => {
            let sel_json = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".into());
            let idx_val = *index;
            format!(
                r##"(() => {{
  const candidates = Array.from(document.querySelectorAll(
    'a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]'
  ));
  const refFor = (el) => {{
    let idx = candidates.indexOf(el);
    if (idx < 0) {{
      const parent = el.closest('a[href], button, input, select, textarea, [role="button"], [role="link"], [contenteditable="true"]');
      if (parent) idx = candidates.indexOf(parent);
    }}
    if (idx >= 0) return 'e' + (idx + 1);
    const all = Array.from(document.querySelectorAll('*'));
    const allIdx = all.indexOf(el);
    return 'e' + (candidates.length + (allIdx >= 0 ? allIdx + 1 : 1));
  }};
  const list = document.querySelectorAll({sel_json});
  const index = {idx_val};
  if (index >= list.length) return JSON.stringify([]);
  const el = list[index];
  const r = refFor(el);
  return JSON.stringify(r ? [r] : []);
}})()"##
            )
        }
    }
}

fn browser_cli_request(command: BrowserCliCommand) -> BrowserCliRequest {
    match command {
        BrowserCliCommand::List => BrowserCliRequest::List,
        BrowserCliCommand::Open {
            url,
            workspace_id,
            worktree_path,
        } => BrowserCliRequest::Open {
            url,
            workspace_id,
            worktree_path,
        },
        BrowserCliCommand::Navigate { browser_id, url } => {
            BrowserCliRequest::Navigate { browser_id, url }
        }
        BrowserCliCommand::Back { browser_id } => BrowserCliRequest::Back { browser_id },
        BrowserCliCommand::Forward { browser_id } => BrowserCliRequest::Forward { browser_id },
        BrowserCliCommand::Reload { browser_id } => BrowserCliRequest::Reload { browser_id },
        BrowserCliCommand::Close { browser_id } => BrowserCliRequest::Close { browser_id },
        BrowserCliCommand::Identify => BrowserCliRequest::Identify,
        BrowserCliCommand::Url { browser_id } => BrowserCliRequest::Snapshot { browser_id },
        BrowserCliCommand::Title { browser_id } => BrowserCliRequest::Snapshot { browser_id },
        BrowserCliCommand::Snapshot { browser_id } => BrowserCliRequest::Snapshot { browser_id },
        BrowserCliCommand::Click {
            browser_id,
            generation,
            reference,
        } => BrowserCliRequest::Act {
            request: crate::browser::BrowserAutomationRequest {
                browser_id,
                generation,
                action: BrowserAutomationAction::Click { reference },
            },
        },
        BrowserCliCommand::Fill {
            browser_id,
            generation,
            reference,
            value,
        } => BrowserCliRequest::Act {
            request: crate::browser::BrowserAutomationRequest {
                browser_id,
                generation,
                action: BrowserAutomationAction::Fill { reference, value },
            },
        },
        BrowserCliCommand::Keypress {
            browser_id,
            generation,
            key,
        } => BrowserCliRequest::Act {
            request: crate::browser::BrowserAutomationRequest {
                browser_id,
                generation,
                action: BrowserAutomationAction::Keypress { key },
            },
        },
        BrowserCliCommand::Dblclick {
            browser_id,
            generation,
            reference,
        } => BrowserCliRequest::Act {
            request: crate::browser::BrowserAutomationRequest {
                browser_id,
                generation,
                action: BrowserAutomationAction::Dblclick { reference },
            },
        },
        BrowserCliCommand::Hover {
            browser_id,
            generation,
            reference,
        } => BrowserCliRequest::Act {
            request: crate::browser::BrowserAutomationRequest {
                browser_id,
                generation,
                action: BrowserAutomationAction::Hover { reference },
            },
        },
        BrowserCliCommand::Check {
            browser_id,
            generation,
            reference,
        } => BrowserCliRequest::Act {
            request: crate::browser::BrowserAutomationRequest {
                browser_id,
                generation,
                action: BrowserAutomationAction::Check { reference },
            },
        },
        BrowserCliCommand::Uncheck {
            browser_id,
            generation,
            reference,
        } => BrowserCliRequest::Act {
            request: crate::browser::BrowserAutomationRequest {
                browser_id,
                generation,
                action: BrowserAutomationAction::Uncheck { reference },
            },
        },
        BrowserCliCommand::ScrollIntoView {
            browser_id,
            generation,
            reference,
        } => BrowserCliRequest::Act {
            request: crate::browser::BrowserAutomationRequest {
                browser_id,
                generation,
                action: BrowserAutomationAction::ScrollIntoView { reference },
            },
        },
        BrowserCliCommand::Select {
            browser_id,
            generation,
            reference,
            value,
        } => BrowserCliRequest::Act {
            request: crate::browser::BrowserAutomationRequest {
                browser_id,
                generation,
                action: BrowserAutomationAction::Select { reference, value },
            },
        },
        BrowserCliCommand::Type {
            browser_id,
            generation,
            reference,
            text,
        } => BrowserCliRequest::Act {
            request: crate::browser::BrowserAutomationRequest {
                browser_id,
                generation,
                action: BrowserAutomationAction::Type { reference, text },
            },
        },
        BrowserCliCommand::Scroll {
            browser_id,
            generation,
            reference,
            x,
            y,
        } => BrowserCliRequest::Act {
            request: crate::browser::BrowserAutomationRequest {
                browser_id,
                generation,
                action: BrowserAutomationAction::Scroll { reference, x, y },
            },
        },
        BrowserCliCommand::Eval { browser_id, script } => {
            BrowserCliRequest::Eval { browser_id, script }
        }
        BrowserCliCommand::Wait {
            browser_id,
            condition,
            timeout_ms,
        } => BrowserCliRequest::Wait {
            browser_id,
            // `--timeout-ms` used to be dropped here, which silently ran every wait
            // against the server's 15 s default. The server already honours the
            // wrapped condition (`ipc/browser.rs` wait_browser_session).
            condition: match timeout_ms {
                Some(timeout_ms) => BrowserWaitCondition::WithTimeout {
                    inner: Box::new(condition),
                    timeout_ms,
                },
                None => condition,
            },
        },
        BrowserCliCommand::Screenshot {
            browser_id,
            out_path,
        } => BrowserCliRequest::Screenshot {
            browser_id,
            out_path,
        },
        BrowserCliCommand::Console {
            browser_id,
            errors_only,
            clear,
        } => BrowserCliRequest::Console {
            browser_id,
            errors_only: Some(errors_only),
            clear: Some(clear),
        },
        BrowserCliCommand::Errors { browser_id, clear } => BrowserCliRequest::Console {
            browser_id,
            errors_only: Some(true),
            clear: Some(clear),
        },
        BrowserCliCommand::Focus {
            browser_id,
            generation,
            reference,
        } => match (generation, reference) {
            (Some(generation), Some(reference)) => BrowserCliRequest::Act {
                request: crate::browser::BrowserAutomationRequest {
                    browser_id,
                    generation,
                    action: BrowserAutomationAction::Focus { reference },
                },
            },
            // `parse_browser_cli` rejects a half-specified element focus, so anything
            // reaching here without both flags is the row-1 webview focus path.
            _ => BrowserCliRequest::Focus { browser_id },
        },
        BrowserCliCommand::Cookies { browser_id, action } => match action {
            CookieCliAction::Get => BrowserCliRequest::Cookies {
                browser_id,
                action: "get".into(),
                name: None,
                value: None,
                domain: None,
                path: None,
            },
            CookieCliAction::Set {
                name,
                value,
                domain,
                path,
            } => BrowserCliRequest::Cookies {
                browser_id,
                action: "set".into(),
                name: Some(name),
                value: Some(value),
                domain,
                path,
            },
            CookieCliAction::Clear { name } => BrowserCliRequest::Cookies {
                browser_id,
                action: "clear".into(),
                name: Some(name),
                value: None,
                domain: None,
                path: None,
            },
        },
        BrowserCliCommand::Storage {
            browser_id,
            kind,
            action,
        } => {
            let kind = match kind {
                StorageCliKind::Local => "local",
                StorageCliKind::Session => "session",
            };
            match action {
                StorageCliAction::Get { key } => BrowserCliRequest::Storage {
                    browser_id,
                    kind: kind.into(),
                    action: "get".into(),
                    key,
                    value: None,
                    clear_all: None,
                    url: None,
                    domain: None,
                },
                StorageCliAction::Set { key, value } => BrowserCliRequest::Storage {
                    browser_id,
                    kind: kind.into(),
                    action: "set".into(),
                    key: Some(key),
                    value: Some(value),
                    clear_all: None,
                    url: None,
                    domain: None,
                },
                StorageCliAction::Clear {
                    key,
                    all,
                    url,
                    domain,
                } => BrowserCliRequest::Storage {
                    browser_id,
                    kind: kind.into(),
                    action: "clear".into(),
                    key,
                    value: None,
                    clear_all: Some(all),
                    url,
                    domain,
                },
            }
        }
        BrowserCliCommand::Get {
            browser_id,
            subselector,
        } => BrowserCliRequest::Eval {
            browser_id,
            script: build_dom_get_script(&subselector),
        },
        BrowserCliCommand::Is {
            browser_id,
            subselector,
        } => BrowserCliRequest::Eval {
            browser_id,
            script: build_dom_is_script(&subselector),
        },
        BrowserCliCommand::Find {
            browser_id,
            subselector,
        } => BrowserCliRequest::Eval {
            browser_id,
            script: build_dom_find_script(&subselector),
        },
        BrowserCliCommand::Tab { action } => match action {
            TabCliAction::List => BrowserCliRequest::TabList,
            TabCliAction::New {
                url,
                workspace_id,
                worktree_path,
            } => BrowserCliRequest::TabNew {
                url,
                workspace_id,
                worktree_path,
            },
            TabCliAction::Switch { target } => {
                if let Ok(idx) = target.parse::<usize>() {
                    BrowserCliRequest::TabSwitch {
                        browser_id: None,
                        index: Some(idx),
                    }
                } else {
                    BrowserCliRequest::TabSwitch {
                        browser_id: Some(target),
                        index: None,
                    }
                }
            }
            TabCliAction::Close { target, browser_id } => {
                if let Some(t) = target {
                    if let Ok(idx) = t.parse::<usize>() {
                        BrowserCliRequest::TabClose {
                            browser_id,
                            index: Some(idx),
                        }
                    } else {
                        BrowserCliRequest::TabClose {
                            browser_id: Some(t),
                            index: None,
                        }
                    }
                } else {
                    BrowserCliRequest::TabClose {
                        browser_id,
                        index: None,
                    }
                }
            }
        },
        BrowserCliCommand::State { browser_id, action } => match action {
            StateCliAction::Save { path } => BrowserCliRequest::StateSave {
                browser_id,
                out_path: path,
            },
            StateCliAction::Load { path } => BrowserCliRequest::StateLoad {
                browser_id,
                in_path: path,
            },
        },
        BrowserCliCommand::Dialog { browser_id, action } => match action {
            DialogCliAction::Accept { prompt_text } => BrowserCliRequest::Dialog {
                browser_id,
                action: "accept".into(),
                prompt_text,
                policy: None,
            },
            DialogCliAction::Dismiss => BrowserCliRequest::Dialog {
                browser_id,
                action: "dismiss".into(),
                prompt_text: None,
                policy: None,
            },
            DialogCliAction::List => BrowserCliRequest::Dialog {
                browser_id,
                action: "list".into(),
                prompt_text: None,
                policy: None,
            },
            DialogCliAction::Policy { policy } => BrowserCliRequest::Dialog {
                browser_id,
                action: "policy".into(),
                prompt_text: None,
                policy: Some(policy),
            },
        },
        BrowserCliCommand::Download { action } => match action {
            DownloadCliAction::Start {
                browser_id,
                url,
                path,
            } => BrowserCliRequest::Download {
                browser_id: Some(browser_id),
                url: Some(url),
                path: Some(path),
                id: None,
                action: "start".into(),
            },
            DownloadCliAction::List => BrowserCliRequest::Download {
                browser_id: None,
                url: None,
                path: None,
                id: None,
                action: "list".into(),
            },
            DownloadCliAction::Cancel { id } => BrowserCliRequest::Download {
                browser_id: None,
                url: None,
                path: None,
                id: Some(id),
                action: "cancel".into(),
            },
        },
        BrowserCliCommand::Highlight {
            browser_id,
            selector,
            duration_ms,
        } => BrowserCliRequest::Highlight {
            browser_id,
            selector,
            duration_ms,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairCliCommand {
    List,
    GeneratePin,
    GenerateMachinePin,
    Approve { pin: String },
}

const PAIR_USAGE: &str = "expected `ferryx pair <list|generate|--generate-pin|approve <pin>>`";

/// Parses a pair subcommand (`list`, `generate`, `--generate-pin`, `approve <pin>`)
/// starting at `args[subcommand_index]`. Shared by both `ferryx pair <...>` and
/// `ferryx remote pair <...>`.
fn parse_pair_subcommand(
    args: &[String],
    subcommand_index: usize,
) -> Result<PairCliCommand, String> {
    match args.get(subcommand_index).map(String::as_str) {
        Some("list") => Ok(PairCliCommand::List),
        Some("--generate-pin") | Some("generate") => match &args[subcommand_index + 1..] {
            [] => Ok(PairCliCommand::GeneratePin),
            [flag, scope] if flag == "--access" && scope == "mirror" => {
                Ok(PairCliCommand::GeneratePin)
            }
            [flag, scope] if flag == "--access" && scope == "machine" => {
                Ok(PairCliCommand::GenerateMachinePin)
            }
            _ => Err("expected pair generate [--access mirror|machine]".into()),
        },
        Some("approve") => {
            let pin = args
                .get(subcommand_index + 1)
                .cloned()
                .ok_or_else(|| "missing <pin> for `ferryx pair approve`".to_string())?;
            Ok(PairCliCommand::Approve { pin })
        }
        _ => Err(PAIR_USAGE.into()),
    }
}

pub fn parse_pair_cli<I, T>(args: I) -> Result<PairCliCommand, String>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    if args.get(1).is_none_or(|arg| arg != "pair") {
        return Err(PAIR_USAGE.into());
    }
    parse_pair_subcommand(&args, 2)
}

fn remote_auth_manager() -> Result<crate::remote::AuthManager, String> {
    let data_dir = std::env::var_os("FERRYX_DATA_DIR")
        .map(std::path::PathBuf::from)
        .map(|dir| dir.join("remote"))
        .or_else(|| {
            #[cfg(windows)]
            {
                std::env::var_os("LOCALAPPDATA")
                    .map(|dir| std::path::PathBuf::from(dir).join("Ferryx").join("remote"))
                    .or_else(|| {
                        std::env::var_os("USERPROFILE")
                            .map(|dir| std::path::PathBuf::from(dir).join(".ferryx").join("remote"))
                    })
            }
            #[cfg(not(windows))]
            {
                std::env::var_os("HOME")
                    .map(|dir| std::path::PathBuf::from(dir).join(".ferryx").join("remote"))
            }
        });
    let auth_path = data_dir
        .ok_or_else(|| "Cannot persist pairing state: set FERRYX_DATA_DIR".to_string())?
        .join("remote-auth.json");
    Ok(crate::remote::AuthManager::with_persistence(Some(
        auth_path,
    )))
}

pub const ACCOUNT_LOGIN_REQUIRED: &str = "ACCOUNT_LOGIN_REQUIRED";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairCliOutcome {
    Done,
    AccountLoginRequired,
}

pub fn run_pair_cli(command: PairCliCommand) -> Result<PairCliOutcome, String> {
    let manager = remote_auth_manager()?;
    match command {
        PairCliCommand::List => {
            let devices = manager.list_devices();
            if devices.is_empty() {
                println!("No paired devices");
            } else {
                for device in devices {
                    println!("{}\t{}\t{:?}", device.id, device.name, device.permission);
                }
            }
            Ok(PairCliOutcome::Done)
        }
        PairCliCommand::GeneratePin | PairCliCommand::GenerateMachinePin => {
            Ok(PairCliOutcome::AccountLoginRequired)
        }
        PairCliCommand::Approve { pin } => match manager.approve_pairing_code_cli(&pin) {
            Ok(_device) => {
                println!("Pairing approved for {pin}; ready for remote client exchange");
                Ok(PairCliOutcome::Done)
            }
            Err(error) => Err(format!("Failed to approve pairing: {error}")),
        },
    }
}

pub fn print_browser_cli_error(code: &str, message: impl AsRef<str>) {
    eprintln!(
        "{}",
        serde_json::json!({ "type": "error", "code": code, "message": message.as_ref() })
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteCliCommand {
    Status { json: bool },
    Pair(PairCliCommand),
}

pub fn parse_remote_cli<I, T>(args: I) -> Result<RemoteCliCommand, String>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    if args.get(1).is_none_or(|arg| arg != "remote") {
        return Err("expected `ferryx remote <status|pair>`".into());
    }
    match args.get(2).map(String::as_str) {
        Some("status") => {
            let json = args.iter().skip(3).any(|arg| arg == "--json");
            Ok(RemoteCliCommand::Status { json })
        }
        Some("pair") => parse_pair_subcommand(&args, 3).map(RemoteCliCommand::Pair),
        _ => Err("expected `ferryx remote <status|pair>`".into()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountCliCommand {
    Enroll { code: String, origin: Option<String> },
    Login { email: Option<String>, origin: Option<String> },
    Plan { json: bool, origin: Option<String> },
}

const ACCOUNT_USAGE: &str = "expected `ferryx account <enroll|login|plan>`\n  enroll: `ferryx account enroll --code <code> [--origin <url>]`\n  login:  `ferryx account login [--email <email>] [--origin <url>]`\n  plan:   `ferryx account plan [--json] [--origin <url>]` (reuses an existing account session: FERRYX_ACCOUNT_SESSION_TOKEN + FERRYX_ACCOUNT_SESSION_ORIGIN)";

pub fn parse_account_cli<I, T>(args: I) -> Result<AccountCliCommand, String>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    if args.get(1).is_none_or(|arg| arg != "account") {
        return Err(ACCOUNT_USAGE.into());
    }
    match args.get(2).map(String::as_str) {
        Some("enroll") => {
            let mut code = None;
            let mut origin = None;
            let mut index = 3;
            while index < args.len() {
                match args[index].as_str() {
                    "--code" if code.is_none() => {
                        let value = args.get(index + 1).ok_or("missing value for --code")?;
                        code = Some(value.clone());
                        index += 2;
                    }
                    "--origin" if origin.is_none() => {
                        let value = args.get(index + 1).ok_or("missing value for --origin")?;
                        origin = Some(value.clone());
                        index += 2;
                    }
                    other => return Err(format!("unknown account option `{other}`")),
                }
            }
            let code = code.ok_or_else(|| ACCOUNT_USAGE.to_string())?;
            if code.trim().is_empty() {
                return Err("missing value for --code".into());
            }
            Ok(AccountCliCommand::Enroll { code, origin })
        }
        Some("login") => {
            let mut email = None;
            let mut origin = None;
            let mut index = 3;
            while index < args.len() {
                match args[index].as_str() {
                    "--email" if email.is_none() => {
                        let value = args.get(index + 1).ok_or("missing value for --email")?;
                        email = Some(value.clone());
                        index += 2;
                    }
                    "--origin" if origin.is_none() => {
                        let value = args.get(index + 1).ok_or("missing value for --origin")?;
                        origin = Some(value.clone());
                        index += 2;
                    }
                    other => return Err(format!("unknown account option `{other}`")),
                }
            }
            Ok(AccountCliCommand::Login { email, origin })
        }
        Some("plan") => {
            let mut json = false;
            let mut origin = None;
            let mut index = 3;
            while index < args.len() {
                match args[index].as_str() {
                    "--json" => {
                        json = true;
                        index += 1;
                    }
                    "--origin" if origin.is_none() => {
                        let value = args.get(index + 1).ok_or("missing value for --origin")?;
                        origin = Some(value.clone());
                        index += 2;
                    }
                    other => return Err(format!("unknown account option `{other}`")),
                }
            }
            Ok(AccountCliCommand::Plan { json, origin })
        }
        _ => Err(ACCOUNT_USAGE.into()),
    }
}

fn account_cli_origin(origin: Option<String>) -> Result<String, AccountCliError> {
    match origin {
        Some(value) => crate::account::origin::normalize_account_origin(&value)
            .map_err(|error| AccountCliError::usage(error.to_string())),
        None => crate::account::origin::account_origin()
            .map_err(|error| AccountCliError::usage(error.to_string())),
    }
}

fn account_cli_runtime() -> Result<tokio::runtime::Runtime, AccountCliError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| AccountCliError::usage(error.to_string()))
}

pub fn run_account_cli(command: AccountCliCommand) -> Result<(), AccountCliError> {
    match command {
        AccountCliCommand::Enroll { code, origin } => {
            let origin = account_cli_origin(origin)?;
            let runtime = account_cli_runtime()?;
            let record = runtime
                .block_on(crate::account::enroll_client::enroll_machine(&origin, &code))
                .map_err(|error| billing::from_enroll_error(&origin, &error))?;
            println!("{}", record.account_id);
            println!("{}", record.machine_record_id);
            println!("{}", record.relay_origin);
            println!("{}", record.enrollment_epoch);
            eprintln!(
                "Enrolled; the daemon keeps owning its identity and device tokens. \
                 Existing paired devices and live terminals are unaffected."
            );
            enable_relay_on_this_machine(
                &runtime,
                &record.relay_origin,
                std::time::Duration::ZERO,
                std::time::Duration::ZERO,
            );
            Ok(())
        }
        AccountCliCommand::Login { email, origin } => {
            let email = match email {
                Some(ref em) if !em.trim().is_empty() => em.trim(),
                _ => {
                    return Err(AccountCliError::usage(
                        "error: --email <address> is required for headless machine login.\nUsage: ferryx account login --email user@example.com",
                    ));
                }
            };
            let origin = account_cli_origin(origin)?;
            let runtime = account_cli_runtime()?;
            let record = runtime.block_on(async {
                let auth_resp = crate::account::enroll_client::request_device_auth(&origin, email)
                    .await
                    .map_err(|error| billing::from_enroll_error(&origin, &error))?;
                eprintln!("\n=== Ferryx Machine Login ===");
                eprintln!("A magic authorization link has been sent to {email}.");
                eprintln!("Please open the link in your email to approve this machine.\n");
                eprintln!("Confirmation Code: {}\n", auth_resp.user_code);
                eprintln!("Waiting for authorization...");
                let start = std::time::Instant::now();
                let timeout = std::time::Duration::from_secs(auth_resp.expires_in.max(300));
                let poll_interval = std::time::Duration::from_secs(auth_resp.interval.max(2));

                let enrollment_code = loop {
                    tokio::time::sleep(poll_interval).await;
                    if start.elapsed() > timeout {
                        return Err(AccountCliError::local(
                            "ACCOUNT_LOGIN_TIMEOUT",
                            "authentication timed out waiting for approval",
                        ));
                    }
                    eprint!(".");
                    match crate::account::enroll_client::poll_device_auth(&origin, &auth_resp.device_code).await {
                        Ok(Some(code)) => {
                            eprintln!("\nAuthorization approved!");
                            break code;
                        }
                        Ok(None) => continue,
                        // A dropped connection or a server-side failure (5xx) while waiting is
                        // retried on the next tick; the timeout above bounds it.
                        Err(err) if poll_error_is_transient(&err) => {
                            tracing::debug!(error = %err, "device auth poll failed; retrying");
                            continue;
                        }
                        Err(err) => return Err(billing::from_enroll_error(&origin, &err)),
                    }
                };

                eprintln!("Enrolling machine with account...");
                let record = crate::account::enroll_client::enroll_machine(&origin, &enrollment_code)
                    .await
                    .map_err(|error| billing::from_enroll_error(&origin, &error))?;
                println!("{}", record.account_id);
                println!("{}", record.machine_record_id);
                println!("{}", record.relay_origin);
                println!("{}", record.enrollment_epoch);
                eprintln!(
                    "Enrolled successfully! The daemon keeps owning its identity and device tokens. \
                     Existing paired devices and live terminals are unaffected."
                );
                Ok(record)
            })?;
            enable_relay_on_this_machine(
                &runtime,
                &record.relay_origin,
                std::time::Duration::ZERO,
                std::time::Duration::ZERO,
            );
            Ok(())
        }
        AccountCliCommand::Plan { json, origin } => {
            let origin = account_cli_origin(origin)?;
            let session_token = billing::resolve_session_token(&origin)?;
            let runtime = account_cli_runtime()?;
            let entitlement = runtime.block_on(billing::fetch_entitlement(&origin, &session_token))?;
            if json {
                println!("{}", billing::entitlement_json(&entitlement)?);
            } else {
                print!("{}", billing::render_plan_text(&entitlement));
            }
            Ok(())
        }
    }
}

/// Environment variable carrying an install key to `ferryx-cli up` (never argv).
pub const INSTALL_KEY_ENV: &str = "FERRYX_INSTALL_KEY";

/// `ferryx-cli up` exit codes (machine onboarding contract).
pub const UP_EXIT_OK: i32 = 0;
pub const UP_EXIT_OTHER: i32 = 1;
pub const UP_EXIT_USAGE: i32 = 2;
pub const UP_EXIT_KEY_REJECTED: i32 = 3;
pub const UP_EXIT_DENIED: i32 = 4;
pub const UP_EXIT_APPROVAL_EXPIRED: i32 = 5;
pub const UP_EXIT_PLAN_LIMIT: i32 = 6;

const UP_USAGE: &str = "usage: ferryx-cli up [--key-stdin | --key-env] [--email <hint>] [--origin <url>] [--name <name>] [--no-service] [--json]\n  The install key is read from stdin (--key-stdin) or FERRYX_INSTALL_KEY (--key-env, or implicitly when set); it is never accepted as an argument.\n  Without a key, an approval request is created and its URL and code are printed to stderr.";

const SERVICE_USAGE: &str =
    "usage: ferryx-cli service <install|status|uninstall> [--json]";

/// The `ferryx-cli --version` output line.
pub fn cli_version_line() -> String {
    format!("ferryx-cli {}", env!("CARGO_PKG_VERSION"))
}

/// True when `ferryx-cli up` was asked for help (`--help`/`-h`).
pub fn up_help_requested(args: &[String]) -> bool {
    args.get(1).is_some_and(|arg| arg == "up")
        && args.iter().skip(2).any(|arg| arg == "--help" || arg == "-h")
}

/// Where `ferryx-cli up` takes its install key from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpKeySource {
    /// No flag: use `FERRYX_INSTALL_KEY` when it is set, else the approval flow.
    Default,
    /// `--key-stdin`: exactly one line from stdin.
    Stdin,
    /// `--key-env`: `FERRYX_INSTALL_KEY`, which must be set.
    Env,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpCliCommand {
    pub key_source: UpKeySource,
    pub email: Option<String>,
    pub origin: Option<String>,
    pub name: Option<String>,
    pub no_service: bool,
    pub json: bool,
}

/// A typed `up`/`service` failure: `code` is the structured account or local error code,
/// `message` the full human text, `exit_code` one of the `UP_EXIT_*` values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpCliError {
    pub code: String,
    pub message: String,
    pub exit_code: i32,
}

impl UpCliError {
    pub fn new(code: &str, message: impl Into<String>, exit_code: i32) -> Self {
        Self {
            code: code.to_string(),
            message: format!("{code}: {}", message.into()),
            exit_code,
        }
    }

    pub fn usage(code: &str, message: impl Into<String>) -> Self {
        Self::new(code, message, UP_EXIT_USAGE)
    }

    pub fn other(code: &str, message: impl Into<String>) -> Self {
        Self::new(code, message, UP_EXIT_OTHER)
    }
}

impl std::fmt::Display for UpCliError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

/// Maps a structured account error to the `up` exit code. Branches on the code, never on text.
pub fn up_exit_code(code: &str, status: u16) -> i32 {
    match code {
        "INSTALL_KEY_USED"
        | "INSTALL_KEY_EXPIRED"
        | "ENROLL_CODE_INVALID"
        | "ENROLL_CODE_EXPIRED"
        | "ENROLL_CODE_MACHINE_MISMATCH" => UP_EXIT_KEY_REJECTED,
        "MACHINE_REQUEST_DENIED" => UP_EXIT_DENIED,
        "MACHINE_REQUEST_EXPIRED" => UP_EXIT_APPROVAL_EXPIRED,
        "PLAN_LIMIT_REACHED" | "REMOTE_SUSPENDED" => UP_EXIT_PLAN_LIMIT,
        _ if status == 402 => UP_EXIT_PLAN_LIMIT,
        _ => UP_EXIT_OTHER,
    }
}

fn up_error_from_enroll(
    origin: &str,
    error: &crate::account::enroll_client::EnrollError,
) -> UpCliError {
    UpCliError {
        code: error.code.clone(),
        message: billing::from_enroll_error(origin, error).message,
        exit_code: up_exit_code(&error.code, error.status),
    }
}

fn take_option_value(args: &[String], index: usize, name: &str) -> Result<String, UpCliError> {
    args.get(index + 1)
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .ok_or_else(|| UpCliError::usage("CLI_USAGE", format!("missing value for {name}\n{UP_USAGE}")))
}

pub fn parse_up_cli<I, T>(args: I) -> Result<UpCliCommand, UpCliError>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    if args.get(1).is_none_or(|arg| arg != "up") {
        return Err(UpCliError::usage("CLI_USAGE", UP_USAGE));
    }
    let mut command = UpCliCommand {
        key_source: UpKeySource::Default,
        email: None,
        origin: None,
        name: None,
        no_service: false,
        json: false,
    };
    let mut index = 2;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--key-stdin" | "--key-env" => {
                if command.key_source != UpKeySource::Default {
                    return Err(UpCliError::usage(
                        "CLI_USAGE",
                        format!("--key-stdin and --key-env are mutually exclusive\n{UP_USAGE}"),
                    ));
                }
                command.key_source = if arg == "--key-stdin" {
                    UpKeySource::Stdin
                } else {
                    UpKeySource::Env
                };
                index += 1;
            }
            "--email" if command.email.is_none() => {
                command.email = Some(take_option_value(&args, index, "--email")?);
                index += 2;
            }
            "--origin" if command.origin.is_none() => {
                command.origin = Some(take_option_value(&args, index, "--origin")?);
                index += 2;
            }
            "--name" if command.name.is_none() => {
                command.name = Some(take_option_value(&args, index, "--name")?);
                index += 2;
            }
            "--no-service" => {
                command.no_service = true;
                index += 1;
            }
            "--json" => {
                command.json = true;
                index += 1;
            }
            _ if arg == "--key" || arg.starts_with("--key=") => {
                return Err(UpCliError::usage(
                    "INSTALL_KEY_IN_ARGV",
                    format!(
                        "an install key is never accepted as an argument; pass it on stdin with --key-stdin or in {INSTALL_KEY_ENV} with --key-env"
                    ),
                ));
            }
            other => {
                return Err(UpCliError::usage(
                    "CLI_USAGE",
                    format!("unknown up option `{other}`\n{UP_USAGE}"),
                ))
            }
        }
    }
    Ok(command)
}

/// Resolves the install key for `source`. `read_stdin` is called only for `--key-stdin` and
/// must return one line; `env_key` is the value of `FERRYX_INSTALL_KEY`.
pub fn resolve_up_key<R>(
    source: UpKeySource,
    read_stdin: R,
    env_key: Option<String>,
) -> Result<Option<String>, UpCliError>
where
    R: FnOnce() -> std::io::Result<String>,
{
    let normalized = |value: String| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    };
    match source {
        UpKeySource::Stdin => {
            let line = read_stdin().map_err(|error| {
                UpCliError::usage(
                    "INSTALL_KEY_MISSING",
                    format!("cannot read the install key from stdin: {error}"),
                )
            })?;
            normalized(line).map(Some).ok_or_else(|| {
                UpCliError::usage("INSTALL_KEY_MISSING", "--key-stdin read an empty line")
            })
        }
        UpKeySource::Env => env_key.and_then(normalized).map(Some).ok_or_else(|| {
            UpCliError::usage(
                "INSTALL_KEY_MISSING",
                format!("--key-env needs a non-empty {INSTALL_KEY_ENV}"),
            )
        }),
        UpKeySource::Default => Ok(env_key.and_then(normalized)),
    }
}

fn read_stdin_line() -> std::io::Result<String> {
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line)
}

/// The first poll interval: the server's `interval`, kept within 1..=60 s.
pub fn approval_poll_interval(interval_secs: u64) -> std::time::Duration {
    std::time::Duration::from_secs(interval_secs.clamp(1, 60))
}

/// RFC 8628 `slow_down`: lengthen the interval by 5 s (capped at 60 s).
pub fn slowed_down_interval(current: std::time::Duration) -> std::time::Duration {
    (current + std::time::Duration::from_secs(5)).min(std::time::Duration::from_secs(60))
}

/// Whether an approval poll failed in a way the next poll can recover from: the account service
/// was unreachable or answered with a server error (e.g. a store write that timed out). Client
/// errors such as an expired or unknown device code are final.
pub fn poll_error_is_transient(error: &crate::account::enroll_client::EnrollError) -> bool {
    error.code == "ACCOUNT_UNREACHABLE" || error.status >= 500
}

/// Whether the stored enrollment record belongs to `origin`.
pub fn enrolled_to_origin(
    record: &crate::account::enroll_client::AccountEnrollmentRecord,
    origin: &str,
) -> bool {
    record.account_origin.trim_end_matches('/') == origin.trim_end_matches('/')
}

async fn wait_for_machine_approval(
    origin: &str,
    email: Option<&str>,
    name: Option<&str>,
) -> Result<String, UpCliError> {
    use crate::account::enroll_client::{create_machine_request, poll_machine_request, MachineRequestPoll};

    let request = create_machine_request(origin, email, name)
        .await
        .map_err(|error| up_error_from_enroll(origin, &error))?;
    eprintln!("Approve this machine: {}", request.verification_uri_complete);
    eprintln!("Code: {}", request.user_code);
    eprintln!("Waiting for approval...");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(request.expires_in);
    let mut interval = approval_poll_interval(request.interval);
    loop {
        tokio::time::sleep(interval).await;
        match poll_machine_request(origin, &request.device_code).await {
            Ok(MachineRequestPoll::Pending) => {}
            Ok(MachineRequestPoll::SlowDown) => interval = slowed_down_interval(interval),
            Ok(MachineRequestPoll::Approved { enrollment_code }) => {
                eprintln!("Approved.");
                return Ok(enrollment_code);
            }
            Ok(MachineRequestPoll::Denied) => {
                return Err(UpCliError::new(
                    "MACHINE_REQUEST_DENIED",
                    "the account owner denied this machine",
                    UP_EXIT_DENIED,
                ))
            }
            // A dropped connection while waiting is retried on the next tick; the deadline bounds it.
            Err(error) if poll_error_is_transient(&error) => {
                tracing::debug!(%error, "machine request poll failed; retrying");
            }
            Err(error) => return Err(up_error_from_enroll(origin, &error)),
        }
        if std::time::Instant::now() >= deadline {
            return Err(UpCliError::new(
                "MACHINE_REQUEST_EXPIRED",
                "the approval request expired before it was approved",
                UP_EXIT_APPROVAL_EXPIRED,
            ));
        }
    }
}

/// `enrolled` of the `up --json` summary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpEnrolled {
    pub account_id: String,
    pub machine_record_id: String,
    pub relay_origin: String,
    /// What `up` did to the remote gateway mode: `enabled` (the daemon switched to Relay),
    /// `saved` (no daemon answered; Relay was saved for its next boot), `unchanged` (a mode
    /// the user chose was kept) or `failed`.
    pub relay_activation: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay_activation_error: Option<String>,
    pub remote: RemoteLiveStatus,
}

/// Remote gateway state as the running daemon reports it, or the saved config when no daemon
/// answers (`daemon` is then `notRunning` or `unreachable` and nothing live is claimed).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteLiveStatus {
    /// `running`, `notRunning` (no daemon owns the endpoint) or `unreachable` (a daemon owns it
    /// but did not answer the request, e.g. it speaks another protocol version).
    pub daemon: &'static str,
    pub mode: crate::remote::RemoteNetworkMode,
    /// `None` when no daemon answered: the tunnel state is unknown, not down.
    pub relay_connected: Option<bool>,
    /// The relay the gateway connects to in Relay mode.
    pub relay_url: Option<String>,
    /// `config` (saved relay URL) or `default` (built-in relay); `None` outside Relay mode.
    pub relay_url_source: Option<&'static str>,
}

/// How long `up` and `account enroll` wait for the daemon endpoint before saving Relay to the
/// config file the daemon restores at boot.
const RELAY_DAEMON_WAIT: std::time::Duration = std::time::Duration::from_secs(10);
/// How long `up` waits for a just-enabled relay tunnel before reporting its current state.
const RELAY_CONNECT_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// The relay a gateway in `config` connects to, and where that URL came from.
fn effective_relay_url(
    config_mode: crate::remote::RemoteNetworkMode,
    relay_url: Option<&str>,
) -> (Option<String>, Option<&'static str>) {
    if config_mode != crate::remote::RemoteNetworkMode::Relay {
        return (None, None);
    }
    match relay_url.map(str::trim).filter(|url| !url.is_empty()) {
        Some(url) => (Some(url.to_string()), Some("config")),
        None => (
            Some(crate::remote::state::DEFAULT_RELAY_URL.to_string()),
            Some("default"),
        ),
    }
}

fn live_status_from_daemon(status: &crate::daemon::DaemonRemoteStatus) -> RemoteLiveStatus {
    let (relay_url, relay_url_source) = effective_relay_url(status.mode, status.relay_url.as_deref());
    RemoteLiveStatus {
        daemon: "running",
        mode: status.mode,
        relay_connected: status.relay_connected,
        relay_url,
        relay_url_source,
    }
}

/// The `daemon` marker for a status request that `error` failed: `notRunning` only when no daemon
/// owns the endpoint; any other failure (e.g. a retiring daemon's handover timeout) is `unreachable`.
fn daemon_absence(error: &crate::ipc::IpcError) -> &'static str {
    if error.code == crate::ipc::IpcErrorCode::HostUnavailable
        && error.message == crate::daemon::EXISTING_ONLY_NO_DAEMON_MESSAGE
    {
        "notRunning"
    } else {
        "unreachable"
    }
}

fn live_status_from_saved(
    saved: &crate::remote::RemoteGatewayConfig,
    daemon: &'static str,
) -> RemoteLiveStatus {
    let (relay_url, relay_url_source) = effective_relay_url(saved.mode, saved.relay_url.as_deref());
    RemoteLiveStatus {
        daemon,
        mode: saved.mode,
        relay_connected: None,
        relay_url,
        relay_url_source,
    }
}

fn saved_remote_config(config_path: Option<&std::path::Path>) -> crate::remote::RemoteGatewayConfig {
    config_path
        .and_then(crate::remote::state::load_persisted_config)
        .unwrap_or_default()
}

/// The `relayActivationError` of an `unchanged` outcome caused by an unparseable remote config.
const UNREADABLE_REMOTE_CONFIG_NOTE: &str =
    "the saved remote config could not be read, so it was left as is";

/// Outcome of [`enable_relay_after_enroll`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayActivation {
    pub outcome: &'static str,
    pub error: Option<String>,
    pub remote: RemoteLiveStatus,
}

/// Turns the relay on after an enrollment, the way the desktop does on its first pairing: only
/// when the gateway is Off, so a LocalNetwork or Tailscale mode the user chose is never replaced.
/// A running daemon is switched over IPC; when none answers on `client`'s endpoint, Relay is
/// saved to `config_path`, which the daemon restores at boot.
pub async fn enable_relay_after_enroll(
    client: &crate::daemon::DaemonClient,
    config_path: Option<&std::path::Path>,
    relay_origin: &str,
    connect_wait: std::time::Duration,
) -> RelayActivation {
    use crate::remote::{RemoteGatewayConfig, RemoteNetworkMode, REMOTE_GATEWAY_PORT};

    let status = match client.remote_get_status().await {
        Ok(status) => status,
        Err(status_error) => {
            let valid = crate::remote::relay_client::validate_relay_url(
                relay_origin,
                crate::remote::relay_client::is_insecure_relay_allowed(),
            );
            let (outcome, error) = match (config_path, valid) {
                (_, Err(error)) => ("failed", Some(format!("invalid relay origin: {error}"))),
                (Some(path), Ok(_)) => match crate::remote::state::persist_relay_if_off(path, relay_origin) {
                    Ok(true) => ("saved", None),
                    Ok(false) if crate::remote::state::load_persisted_config(path).is_none() => {
                        ("unchanged", Some(UNREADABLE_REMOTE_CONFIG_NOTE.to_string()))
                    }
                    Ok(false) => ("unchanged", None),
                    Err(error) => ("failed", Some(format!("cannot save the remote config: {error}"))),
                },
                (None, Ok(_)) => ("failed", Some("cannot resolve the remote config path".to_string())),
            };
            return RelayActivation {
                outcome,
                error,
                remote: live_status_from_saved(
                    &saved_remote_config(config_path),
                    daemon_absence(&status_error),
                ),
            };
        }
    };
    if status.mode != RemoteNetworkMode::Off {
        return RelayActivation {
            outcome: "unchanged",
            error: None,
            remote: live_status_from_daemon(&status),
        };
    }
    // A daemon that booted from an unreadable config reports Off; switching it to Relay would
    // make it overwrite that file, so the file and the daemon are both left alone.
    if config_path.is_some_and(|path| {
        path.exists() && crate::remote::state::load_persisted_config(path).is_none()
    }) {
        return RelayActivation {
            outcome: "unchanged",
            error: Some(UNREADABLE_REMOTE_CONFIG_NOTE.to_string()),
            remote: live_status_from_daemon(&status),
        };
    }
    let config = RemoteGatewayConfig {
        mode: RemoteNetworkMode::Relay,
        port: REMOTE_GATEWAY_PORT,
        allow_control: status.allow_control,
        relay_url: Some(relay_origin.to_string()),
    };
    if let Err(error) = client.remote_configure(config).await {
        return RelayActivation {
            outcome: "failed",
            error: Some(error.message),
            remote: live_status_from_daemon(&status),
        };
    }
    // The tunnel connects in the background; report it once it is up or the wait ends.
    let deadline = tokio::time::Instant::now() + connect_wait;
    let latest = loop {
        match client.remote_get_status().await {
            Ok(status) => {
                if status.relay_connected == Some(true) || tokio::time::Instant::now() >= deadline {
                    break Ok(status);
                }
            }
            Err(error) => break Err(daemon_absence(&error)),
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    };
    RelayActivation {
        outcome: "enabled",
        error: None,
        remote: match latest {
            Ok(status) => live_status_from_daemon(&status),
            Err(daemon) => live_status_from_saved(&saved_remote_config(config_path), daemon),
        },
    }
}

/// [`enable_relay_after_enroll`] against this machine's daemon and config file.
fn enable_relay_on_this_machine(
    runtime: &tokio::runtime::Runtime,
    relay_origin: &str,
    daemon_wait: std::time::Duration,
    connect_wait: std::time::Duration,
) -> RelayActivation {
    let client = crate::daemon::DaemonClient::existing_only(daemon_wait);
    let config_path = crate::remote::state::remote_config_path();
    let activation = runtime.block_on(enable_relay_after_enroll(
        &client,
        config_path.as_deref(),
        relay_origin,
        connect_wait,
    ));
    eprintln!("{}", relay_activation_message(&activation, relay_origin));
    activation
}

/// The human line `up` and `account enroll` print for `activation`.
fn relay_activation_message(activation: &RelayActivation, relay_origin: &str) -> String {
    let remote = &activation.remote;
    match (activation.outcome, &activation.error) {
        ("enabled", _) => format!("Relay enabled through {relay_origin}."),
        ("saved", _) if remote.daemon == "unreachable" => format!(
            "A daemon owns the endpoint but did not answer; relay mode through {relay_origin} was saved \
             and applies when that daemon restarts."
        ),
        ("saved", _) => format!(
            "No daemon answered; relay mode through {relay_origin} was saved and starts with the daemon."
        ),
        ("unchanged", Some(note)) => format!("Remote access was left unchanged: {note}."),
        ("unchanged", None) if remote.mode == crate::remote::RemoteNetworkMode::Relay => {
            match remote.relay_url.as_deref() {
                Some(url) if url.trim_end_matches('/') != relay_origin.trim_end_matches('/') => format!(
                    "Relay mode already uses {url}; it was not switched to {relay_origin}."
                ),
                _ if remote.daemon == "running" => {
                    "Relay mode is already on; it is left unchanged.".to_string()
                }
                _ => "Relay mode is already saved for the next daemon start; it is left unchanged."
                    .to_string(),
            }
        }
        ("unchanged", None) => format!(
            "Remote access mode {:?} was chosen before; it is left unchanged.",
            remote.mode
        ),
        (_, error) => format!(
            "Could not enable the relay: {}",
            error.as_deref().unwrap_or("unknown error")
        ),
    }
}

/// How the daemon is kept running (`service` of the `up --json` summary and `service --json`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceReport {
    pub supervised: bool,
    /// `systemd-user`, `launchd`, `schtasks`, `detached` or `none`.
    pub backend: &'static str,
    pub installed: bool,
    pub linger: Option<bool>,
    pub pid: Option<u32>,
    pub reason: Option<String>,
}

/// The single JSON line `ferryx-cli up --json` prints last on stdout.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpSummary {
    pub cli_path: String,
    pub version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enrolled: Option<UpEnrolled>,
    pub service: ServiceReport,
}

pub fn run_up_cli(command: UpCliCommand) -> Result<UpSummary, UpCliError> {
    let key = resolve_up_key(
        command.key_source,
        read_stdin_line,
        std::env::var(INSTALL_KEY_ENV).ok(),
    )?;
    // Nothing started from here on (the supervised unit, the detached daemon) may inherit the key.
    // The process is still single-threaded at this point.
    std::env::remove_var(INSTALL_KEY_ENV);

    let origin = account_cli_origin(command.origin.clone())
        .map_err(|error| UpCliError::usage("ACCOUNT_ORIGIN_INVALID", error.message))?;

    // 1. Identity first, so the approval request and the enrollment present the same machine.
    let identity_dir = crate::remote::auth::canonical_remote_dir().ok_or_else(|| {
        UpCliError::other("ACCOUNT_IDENTITY_DIR_UNRESOLVED", "cannot resolve the identity directory")
    })?;
    crate::remote::auth::load_or_generate_machine_identity(&identity_dir)
        .map_err(|error| UpCliError::other("ACCOUNT_IDENTITY_UNAVAILABLE", error))?;

    let runtime = account_cli_runtime()
        .map_err(|error| UpCliError::other("CLI_RUNTIME_FAILED", error.message))?;

    // 2. Already enrolled to this origin: keep it.
    let existing = crate::account::enroll_client::load_enrollment_record()
        .filter(|record| enrolled_to_origin(record, &origin));
    let record = match existing {
        Some(record) => {
            eprintln!("Already enrolled with {origin}; skipping enrollment.");
            record
        }
        None => runtime.block_on(async {
            // 3. A key enrolls directly; 4. otherwise the owner approves a machine request.
            let enrollment_code = match key {
                Some(key) => key,
                None => {
                    wait_for_machine_approval(
                        &origin,
                        command.email.as_deref(),
                        command.name.as_deref(),
                    )
                    .await?
                }
            };
            crate::account::enroll_client::enroll_machine_named(
                &origin,
                &enrollment_code,
                command.name.as_deref(),
            )
            .await
            .map_err(|error| up_error_from_enroll(&origin, &error))
        })?,
    };

    // 5. Keep the daemon running.
    let service = if command.no_service {
        let mut status = service_status(&runtime);
        if status.reason.is_none() {
            status.reason = Some("service install skipped (--no-service)".to_string());
        }
        status
    } else {
        install_service(&runtime)
    };

    // 6. Turn the relay on so the machine is reachable without a desktop. An unsupervised or
    // skipped service gets one endpoint attempt; Relay is then saved for the daemon's boot.
    let daemon_wait = if service.installed && !command.no_service {
        RELAY_DAEMON_WAIT
    } else {
        std::time::Duration::ZERO
    };
    let activation =
        enable_relay_on_this_machine(&runtime, &record.relay_origin, daemon_wait, RELAY_CONNECT_WAIT);

    Ok(UpSummary {
        cli_path: std::env::current_exe()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        version: env!("CARGO_PKG_VERSION"),
        enrolled: Some(UpEnrolled {
            account_id: record.account_id,
            machine_record_id: record.machine_record_id,
            relay_origin: record.relay_origin,
            relay_activation: activation.outcome,
            relay_activation_error: activation.error,
            remote: activation.remote,
        }),
        service,
    })
}

fn print_up_cli_error(json: bool, error: &UpCliError) {
    if json {
        eprintln!(
            "{}",
            serde_json::json!({ "type": "error", "code": error.code, "message": error.message })
        );
    } else {
        eprintln!("{error}");
    }
}

/// Exit code of `up` after a successful run: a daemon that could not be started is a failure
/// even though enrollment succeeded.
pub fn up_summary_exit_code(summary: &UpSummary, no_service: bool) -> i32 {
    if no_service || summary.service.installed {
        UP_EXIT_OK
    } else {
        UP_EXIT_OTHER
    }
}

/// Entry for `ferryx-cli up`: parses, runs, prints, and returns the process exit code.
pub fn up_cli_main(args: &[String]) -> i32 {
    if up_help_requested(args) {
        println!("{UP_USAGE}");
        return UP_EXIT_OK;
    }
    let json = args.iter().skip(2).any(|arg| arg == "--json");
    let command = match parse_up_cli(args) {
        Ok(command) => command,
        Err(error) => {
            print_up_cli_error(json, &error);
            return error.exit_code;
        }
    };
    let no_service = command.no_service;
    match run_up_cli(command) {
        Ok(summary) => {
            if json {
                match serde_json::to_string(&summary) {
                    Ok(line) => println!("{line}"),
                    Err(error) => {
                        print_up_cli_error(json, &UpCliError::other("CLI_OUTPUT_FAILED", error.to_string()));
                        return UP_EXIT_OTHER;
                    }
                }
            } else {
                if let Some(enrolled) = &summary.enrolled {
                    println!(
                        "Enrolled: account {} machine {} relay {}",
                        enrolled.account_id, enrolled.machine_record_id, enrolled.relay_origin
                    );
                    println!(
                        "remote relayActivation={} {}",
                        enrolled.relay_activation,
                        render_remote_live_status(&enrolled.remote)
                    );
                }
                println!("{}", render_service_report(&summary.service));
            }
            let code = up_summary_exit_code(&summary, no_service);
            if code != UP_EXIT_OK {
                eprintln!(
                    "SERVICE_INSTALL_FAILED: {}",
                    summary.service.reason.as_deref().unwrap_or("the daemon is not running")
                );
            }
            code
        }
        Err(error) => {
            print_up_cli_error(json, &error);
            error.exit_code
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceCliAction {
    Install,
    Status,
    Uninstall,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceCliCommand {
    pub action: ServiceCliAction,
    pub json: bool,
}

pub fn parse_service_cli<I, T>(args: I) -> Result<ServiceCliCommand, UpCliError>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    if args.get(1).is_none_or(|arg| arg != "service") {
        return Err(UpCliError::usage("CLI_USAGE", SERVICE_USAGE));
    }
    let action = match args.get(2).map(String::as_str) {
        Some("install") => ServiceCliAction::Install,
        Some("status") => ServiceCliAction::Status,
        Some("uninstall") => ServiceCliAction::Uninstall,
        _ => return Err(UpCliError::usage("CLI_USAGE", SERVICE_USAGE)),
    };
    let mut json = false;
    for arg in &args[3..] {
        match arg.as_str() {
            "--json" => json = true,
            other => {
                return Err(UpCliError::usage(
                    "CLI_USAGE",
                    format!("unknown service option `{other}`\n{SERVICE_USAGE}"),
                ))
            }
        }
    }
    Ok(ServiceCliCommand { action, json })
}

/// The OS service manager this target's supervisor facade drives.
pub fn native_service_backend() -> &'static str {
    if cfg!(target_os = "macos") {
        "launchd"
    } else if cfg!(windows) {
        "schtasks"
    } else if cfg!(target_os = "linux") {
        "systemd-user"
    } else {
        "none"
    }
}

fn supervisor_unavailable_reason() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Some(reason) = crate::daemon::supervisor_linux::availability().clone() {
            return reason;
        }
    }
    "no daemon supervisor is supported in this runtime".to_string()
}

/// Pid of the daemon answering on this machine's canonical endpoint. Read-only; on targets
/// without a handshake probe ([`crate::daemon::endpoint_owner_pid`]) it is always `None`.
fn answering_daemon_pid(runtime: &tokio::runtime::Runtime) -> Option<u32> {
    runtime.block_on(crate::daemon::endpoint_owner_pid())
}

#[cfg(target_os = "linux")]
fn login_user() -> Option<String> {
    std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("LOGNAME").ok())
        .filter(|user| !user.is_empty())
}

/// Linux: `loginctl enable-linger $USER` so the user manager (and a supervised daemon) outlives
/// the SSH session. `None` on other targets.
#[cfg(target_os = "linux")]
fn enable_linger() -> Option<bool> {
    let Some(user) = login_user() else {
        return Some(false);
    };
    Some(
        std::process::Command::new("loginctl")
            .args(["enable-linger", &user])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success()),
    )
}

#[cfg(not(target_os = "linux"))]
fn enable_linger() -> Option<bool> {
    None
}

#[cfg(target_os = "linux")]
fn linger_status() -> Option<bool> {
    let user = login_user()?;
    let output = std::process::Command::new("loginctl")
        .args(["show-user", &user, "-p", "Linger", "--value"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim() == "yes")
}

#[cfg(not(target_os = "linux"))]
fn linger_status() -> Option<bool> {
    None
}

/// `<data dir>/daemon.log` for the detached fallback daemon's stdout and stderr.
fn detached_daemon_log_path() -> Option<std::path::PathBuf> {
    crate::remote::auth::canonical_remote_dir()
        .and_then(|dir| dir.parent().map(|data| data.join("daemon.log")))
}

/// Starts `<current exe> --daemon` in its own session (unix `setsid`, SIGHUP ignored; Windows
/// detached, no console), stdio to `<data dir>/daemon.log`. Returns the child pid.
fn spawn_detached_daemon() -> Result<u32, String> {
    let executable =
        std::env::current_exe().map_err(|error| format!("cannot resolve the current executable: {error}"))?;
    let log_path = detached_daemon_log_path().ok_or("cannot resolve the Ferryx data directory")?;
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let log = options
        .open(&log_path)
        .map_err(|error| format!("cannot open {}: {error}", log_path.display()))?;
    let log_err = log
        .try_clone()
        .map_err(|error| format!("cannot open {}: {error}", log_path.display()))?;
    let mut command = std::process::Command::new(executable);
    command
        .arg("--daemon")
        .env_remove(INSTALL_KEY_ENV)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(log_err));
    crate::util::detach_launched_child(&mut command);
    let child = command
        .spawn()
        .map_err(|error| format!("cannot start the daemon: {error}"))?;
    let pid = child.id();
    // The daemon outlives this short-lived CLI on purpose; init reaps it after this process exits.
    drop(child);
    Ok(pid)
}

/// Waits (bounded) for a daemon to answer on the endpoint after a detached start.
fn wait_for_answering_daemon(
    runtime: &tokio::runtime::Runtime,
    timeout: std::time::Duration,
) -> Option<u32> {
    if !cfg!(unix) {
        return None;
    }
    runtime.block_on(async {
        let until = std::time::Instant::now() + timeout;
        loop {
            if let Some(pid) = crate::daemon::endpoint_owner_pid().await {
                return Some(pid);
            }
            if std::time::Instant::now() >= until {
                return None;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    })
}

fn detached_fallback(
    runtime: &tokio::runtime::Runtime,
    linger: Option<bool>,
    reason: String,
) -> ServiceReport {
    // A daemon already answers: never start a second one and never touch the running one.
    if let Some(pid) = answering_daemon_pid(runtime) {
        return ServiceReport {
            supervised: false,
            backend: "detached",
            installed: true,
            linger,
            pid: Some(pid),
            reason: Some(reason),
        };
    }
    match spawn_detached_daemon() {
        Ok(child_pid) => ServiceReport {
            supervised: false,
            backend: "detached",
            installed: true,
            linger,
            pid: wait_for_answering_daemon(runtime, std::time::Duration::from_secs(10))
                .or(Some(child_pid)),
            reason: Some(reason),
        },
        Err(error) => ServiceReport {
            supervised: false,
            backend: "detached",
            installed: false,
            linger,
            pid: None,
            reason: Some(format!("{reason}; detached start failed: {error}")),
        },
    }
}

/// `service install`: the OS supervisor when this runtime supports one (started only for an
/// absent owner, under owner admission), else a detached `--daemon`. Never stops or replaces a
/// running daemon.
pub fn install_service(runtime: &tokio::runtime::Runtime) -> ServiceReport {
    let linger = enable_linger();
    let reason = if crate::daemon::supervisor::supported() {
        // `ensure_if_absent` is `ensure()` behind owner admission: it starts the supervised job
        // only when no daemon owns the endpoint and registers it for login.
        match crate::daemon::supervisor::ensure_if_absent() {
            Ok(_) if crate::daemon::supervisor::installed() => {
                return ServiceReport {
                    supervised: true,
                    backend: native_service_backend(),
                    installed: true,
                    linger,
                    pid: answering_daemon_pid(runtime),
                    reason: None,
                };
            }
            Ok(_) => "the supervisor job was not registered".to_string(),
            Err(error) => format!("supervisor install failed: {error}"),
        }
    } else {
        supervisor_unavailable_reason()
    };
    detached_fallback(runtime, linger, reason)
}

/// `service status`: read-only.
pub fn service_status(runtime: &tokio::runtime::Runtime) -> ServiceReport {
    let supported = crate::daemon::supervisor::supported();
    let unit = supported && crate::daemon::supervisor::installed();
    let pid = answering_daemon_pid(runtime);
    ServiceReport {
        supervised: unit,
        backend: if unit {
            native_service_backend()
        } else if pid.is_some() {
            "detached"
        } else {
            "none"
        },
        installed: unit || pid.is_some(),
        linger: linger_status(),
        pid,
        reason: (!supported).then(supervisor_unavailable_reason),
    }
}

/// `service uninstall`: removes the supervisor job only when no daemon answers. With a live
/// daemon only login autostart is removed, so its terminals survive; a detached daemon is never
/// stopped either.
pub fn uninstall_service(runtime: &tokio::runtime::Runtime) -> Result<ServiceReport, UpCliError> {
    if !crate::daemon::supervisor::supported() {
        let mut report = service_status(runtime);
        report.reason = Some(format!(
            "{}; nothing to uninstall, and a running daemon is left running",
            supervisor_unavailable_reason()
        ));
        return Ok(report);
    }
    if let Some(pid) = answering_daemon_pid(runtime) {
        crate::daemon::supervisor::remove_autostart()
            .map_err(|error| UpCliError::other("SERVICE_UNINSTALL_FAILED", error))?;
        let mut report = service_status(runtime);
        report.reason = Some(format!(
            "daemon pid {pid} is running; login autostart was removed and the running daemon was left untouched"
        ));
        return Ok(report);
    }
    crate::daemon::supervisor::uninstall()
        .map_err(|error| UpCliError::other("SERVICE_UNINSTALL_FAILED", error))?;
    Ok(service_status(runtime))
}

pub fn render_service_report(report: &ServiceReport) -> String {
    let mut text = format!(
        "service backend={} supervised={} installed={}",
        report.backend, report.supervised, report.installed
    );
    if let Some(linger) = report.linger {
        text.push_str(&format!(" linger={linger}"));
    }
    if let Some(pid) = report.pid {
        text.push_str(&format!(" pid={pid}"));
    }
    if let Some(reason) = &report.reason {
        text.push_str(&format!(" reason=\"{reason}\""));
    }
    text
}

pub fn run_service_cli(command: &ServiceCliCommand) -> Result<ServiceReport, UpCliError> {
    let runtime = account_cli_runtime()
        .map_err(|error| UpCliError::other("CLI_RUNTIME_FAILED", error.message))?;
    match command.action {
        ServiceCliAction::Install => Ok(install_service(&runtime)),
        ServiceCliAction::Status => Ok(service_status(&runtime)),
        ServiceCliAction::Uninstall => uninstall_service(&runtime),
    }
}

/// Entry for `ferryx-cli service`: returns the process exit code.
pub fn service_cli_main(args: &[String]) -> i32 {
    let json = args.iter().skip(2).any(|arg| arg == "--json");
    let result = parse_service_cli(args).and_then(|command| {
        run_service_cli(&command).map(|report| (command.action, report))
    });
    match result {
        Ok((action, report)) => {
            if json {
                match serde_json::to_string(&report) {
                    Ok(line) => println!("{line}"),
                    Err(error) => {
                        print_up_cli_error(json, &UpCliError::other("CLI_OUTPUT_FAILED", error.to_string()));
                        return UP_EXIT_OTHER;
                    }
                }
            } else {
                println!("{}", render_service_report(&report));
            }
            if action == ServiceCliAction::Install && !report.installed {
                UP_EXIT_OTHER
            } else {
                UP_EXIT_OK
            }
        }
        Err(error) => {
            print_up_cli_error(json, &error);
            error.exit_code
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RemoteStatusOutput {
    /// `ok` when the daemon answered; `daemonNotRunning` or `daemonUnreachable` when only the
    /// saved config was read.
    status: &'static str,
    port: u16,
    #[serde(flatten)]
    remote: RemoteLiveStatus,
}

/// Asks the running daemon for the live gateway state (mode, tunnel, effective relay). When no
/// daemon answers, reports the saved `remote-config.json` and says whether the daemon is not
/// running or did not answer, instead of claiming a live state.
async fn remote_status_output(
    client: &crate::daemon::DaemonClient,
    config_path: Option<&std::path::Path>,
) -> RemoteStatusOutput {
    match client.remote_get_status().await {
        Ok(status) => RemoteStatusOutput {
            status: "ok",
            port: status.port,
            remote: live_status_from_daemon(&status),
        },
        Err(error) => {
            let saved = saved_remote_config(config_path);
            let remote = live_status_from_saved(&saved, daemon_absence(&error));
            RemoteStatusOutput {
                status: if remote.daemon == "notRunning" {
                    "daemonNotRunning"
                } else {
                    "daemonUnreachable"
                },
                port: saved.port,
                remote,
            }
        }
    }
}

fn render_remote_live_status(remote: &RemoteLiveStatus) -> String {
    let mode = serde_json::to_value(remote.mode)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default();
    let mut text = format!("daemon={} mode={mode}", remote.daemon);
    match remote.relay_connected {
        Some(connected) => text.push_str(&format!(" relayConnected={connected}")),
        None => text.push_str(" relayConnected=unknown"),
    }
    if let (Some(url), Some(source)) = (&remote.relay_url, remote.relay_url_source) {
        text.push_str(&format!(" relayUrl={url} relayUrlSource={source}"));
    }
    text
}

pub fn run_remote_cli(command: RemoteCliCommand) -> Result<(), String> {
    match command {
        RemoteCliCommand::Status { json } => {
            let runtime = account_cli_runtime().map_err(|error| error.message)?;
            let client = crate::daemon::DaemonClient::existing_only(std::time::Duration::ZERO);
            let config_path = crate::remote::state::remote_config_path();
            let output = runtime.block_on(remote_status_output(&client, config_path.as_deref()));
            if json {
                println!(
                    "{}",
                    serde_json::to_string(&output).map_err(|error| error.to_string())?
                );
            } else {
                println!(
                    "status={} port={} {}",
                    output.status,
                    output.port,
                    render_remote_live_status(&output.remote)
                );
            }
            Ok(())
        }
        RemoteCliCommand::Pair(pair_command) => run_pair_cli(pair_command).map(|_| ()),
    }
}

fn format_browser_cli_response(
    command: &BrowserCliCommand,
    response: BrowserCliResponse,
) -> Result<String, String> {
    match response {
        BrowserCliResponse::List { sessions } => {
            serde_json::to_string(&sessions).map_err(|error| error.to_string())
        }
        BrowserCliResponse::Snapshot { snapshot } => match command {
            BrowserCliCommand::Url { .. } => Ok(snapshot.url),
            BrowserCliCommand::Title { .. } => Ok(snapshot.title),
            _ => serde_json::to_string(&snapshot).map_err(|error| error.to_string()),
        },
        BrowserCliResponse::Acted => Ok(serde_json::json!({ "type": "acted" }).to_string()),
        BrowserCliResponse::Opened { browser, adoption } => {
            serde_json::to_string(&serde_json::json!({
                "type": "opened",
                "browser": browser,
                "adoption": adoption
            }))
            .map_err(|error| error.to_string())
        }
        BrowserCliResponse::Navigated => Ok(serde_json::json!({ "type": "navigated" }).to_string()),
        BrowserCliResponse::Closed => Ok(serde_json::json!({ "type": "closed" }).to_string()),
        BrowserCliResponse::Identified { browser } => {
            serde_json::to_string(&browser).map_err(|error| error.to_string())
        }
        BrowserCliResponse::Evaluated { result, truncated } => {
            if truncated {
                eprintln!("(truncated)");
            }
            Ok(result.unwrap_or_default())
        }
        BrowserCliResponse::Waited => Ok("waited".into()),
        BrowserCliResponse::Focused => Ok("focused".into()),
        BrowserCliResponse::ScreenshotSaved { path } => Ok(path),
        BrowserCliResponse::ConsoleEntries { entries } => {
            serde_json::to_string(&entries).map_err(|error| error.to_string())
        }
        BrowserCliResponse::CookieEntries { cookies } => {
            serde_json::to_string(&cookies).map_err(|error| error.to_string())
        }
        BrowserCliResponse::StorageValue { value } => {
            serde_json::to_string(&value).map_err(|error| error.to_string())
        }
        BrowserCliResponse::TabSwitched { browser, index } => {
            serde_json::to_string(&serde_json::json!({
                "type": "tabSwitched",
                "index": index,
                "browser": browser
            }))
            .map_err(|error| error.to_string())
        }
        BrowserCliResponse::StateSaved { path } => {
            serde_json::to_string(&serde_json::json!({
                "type": "stateSaved",
                "path": path
            }))
            .map_err(|error| error.to_string())
        }
        BrowserCliResponse::StateLoaded => {
            serde_json::to_string(&serde_json::json!({
                "type": "stateLoaded"
            }))
            .map_err(|error| error.to_string())
        }
        BrowserCliResponse::DialogHandled { dialog } => {
            serde_json::to_string(&serde_json::json!({
                "type": "dialogHandled",
                "dialog": dialog
            }))
            .map_err(|error| error.to_string())
        }
        BrowserCliResponse::DialogEntries { dialogs } => {
            serde_json::to_string(&serde_json::json!({
                "type": "dialogEntries",
                "dialogs": dialogs
            }))
            .map_err(|error| error.to_string())
        }
        BrowserCliResponse::DialogPolicySet { policy } => {
            serde_json::to_string(&serde_json::json!({
                "type": "dialogPolicySet",
                "policy": policy
            }))
            .map_err(|error| error.to_string())
        }
        BrowserCliResponse::DownloadStarted { download } => {
            serde_json::to_string(&serde_json::json!({
                "type": "downloadStarted",
                "download": download
            }))
            .map_err(|error| error.to_string())
        }
        BrowserCliResponse::Downloads { downloads } => {
            // The row-12 acceptance line asks for a JSON array of `DownloadRecord`,
            // and this is also what the sibling list responses in this match print
            // (`List` -> sessions, `ConsoleEntries` -> entries, `CookieEntries` ->
            // cookies all serialize the bare array).
            serde_json::to_string(&downloads).map_err(|error| error.to_string())
        }
        BrowserCliResponse::DownloadCancelled { id, cancelled } => {
            serde_json::to_string(&serde_json::json!({
                "type": "downloadCancelled",
                "id": id,
                "cancelled": cancelled
            }))
            .map_err(|error| error.to_string())
        }
        BrowserCliResponse::Highlighted { highlight } => {
            serde_json::to_string(&serde_json::json!({
                "type": "highlighted",
                "highlight": highlight
            }))
            .map_err(|error| error.to_string())
        }
        BrowserCliResponse::FileOpened { path } => Ok(format!("opened {path}")),
        BrowserCliResponse::Error { code, message, .. } => {
            print_browser_cli_error(&code, &message);
            Err(message)
        }
        #[allow(unreachable_patterns)]
        _ => Ok(String::new()),
    }
}

pub const OPEN_CLI_USAGE: &str = "usage: ferryx open <path>[:line[:col]]";

fn is_single_drive_letter(s: &str) -> bool {
    s.len() == 1 && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
}

fn split_path_line_col(target: &str) -> (&str, Option<u32>, Option<u32>) {
    if let Some((rest, last_segment)) = target.rsplit_once(':') {
        if !last_segment.is_empty() && last_segment.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(last_num) = last_segment.parse::<u32>() {
                if let Some((prefix, mid_segment)) = rest.rsplit_once(':') {
                    if !mid_segment.is_empty() && mid_segment.chars().all(|c| c.is_ascii_digit()) {
                        if let Ok(mid_num) = mid_segment.parse::<u32>() {
                            if !prefix.is_empty() && !is_single_drive_letter(prefix) {
                                return (prefix, Some(mid_num), Some(last_num));
                            } else {
                                return (target, None, None);
                            }
                        }
                    }
                }
                if !rest.is_empty() && !is_single_drive_letter(rest) {
                    return (rest, Some(last_num), None);
                } else {
                    return (target, None, None);
                }
            }
        }
    }
    (target, None, None)
}

pub fn parse_open_cli<I, T>(args: I, cwd: &std::path::Path) -> Result<OpenFileCliCommand, String>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();

    if args.len() != 3 || args.get(1).map(String::as_str) != Some("open") {
        return Err(OPEN_CLI_USAGE.to_string());
    }

    let target = &args[2];
    let (parsed_path, line, col) = split_path_line_col(target);

    let path_obj = std::path::Path::new(parsed_path);
    let resolved = if path_obj.is_absolute() {
        path_obj.to_path_buf()
    } else {
        cwd.join(path_obj)
    };

    let metadata = match std::fs::metadata(&resolved) {
        Ok(m) if m.is_file() => m,
        _ => return Err(format!("file not found: {}", resolved.display())),
    };
    let _ = metadata;

    let canonical = resolved
        .canonicalize()
        .map_err(|e| format!("file not found: {}: {e}", resolved.display()))?;

    let path = canonical
        .to_str()
        .map(str::to_string)
        .unwrap_or_else(|| canonical.to_string_lossy().to_string());

    Ok(OpenFileCliCommand { path, line, col })
}

/// `send_browser_cli_request` reads the reply under `tokio::time::timeout`, so timers are required.
fn browser_cli_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|error| error.to_string())
}

pub fn run_open_cli(command: OpenFileCliCommand) -> Result<(), String> {
    let runtime = browser_cli_runtime()?;
    let request = BrowserCliRequest::OpenFile {
        path: command.path,
        line: command.line,
        col: command.col,
    };
    let response = runtime
        .block_on(send_browser_cli_request(request))
        .map_err(|error| error.to_string())?;
    match response {
        BrowserCliResponse::FileOpened { path } => {
            println!("opened {path}");
            Ok(())
        }
        BrowserCliResponse::Error { code, message, .. } => Err(format!("{code}: {message}")),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

pub fn run_browser_cli(command: BrowserCliCommand) -> Result<(), String> {
    let runtime = browser_cli_runtime()?;
    let cwd = std::env::current_dir()
        .map_err(|error| format!("failed to read current directory: {error}"))?;
    let home = cli_home_dir();
    let request = absolutize_browser_cli_paths(
        browser_cli_request(command.clone()),
        &cwd,
        home.as_deref(),
    );
    let response = runtime
        .block_on(send_browser_cli_request(request))
        .map_err(|error| error.to_string())?;
    let output = format_browser_cli_response(&command, response)?;
    println!("{output}");
    Ok(())
}

/// File paths in a browser CLI request are resolved by the GUI process, whose cwd is
/// unrelated to the caller's, so relative paths are anchored to the CLI's cwd here.
fn absolutize_browser_cli_paths(
    request: BrowserCliRequest,
    cwd: &std::path::Path,
    home: Option<&std::path::Path>,
) -> BrowserCliRequest {
    let anchor = |path: String| anchor_cli_path(path, cwd, home);
    match request {
        BrowserCliRequest::Screenshot {
            browser_id,
            out_path,
        } => BrowserCliRequest::Screenshot {
            browser_id,
            out_path: anchor(out_path),
        },
        BrowserCliRequest::StateSave {
            browser_id,
            out_path,
        } => BrowserCliRequest::StateSave {
            browser_id,
            out_path: anchor(out_path),
        },
        BrowserCliRequest::StateLoad {
            browser_id,
            in_path,
        } => BrowserCliRequest::StateLoad {
            browser_id,
            in_path: anchor(in_path),
        },
        BrowserCliRequest::Download {
            browser_id,
            url,
            path,
            id,
            action,
        } => BrowserCliRequest::Download {
            browser_id,
            url,
            path: path.map(anchor),
            id,
            action,
        },
        other => other,
    }
}

/// Expands `~` with the CLI user's home and joins relative paths onto `cwd`. Blank
/// paths pass through so the GUI keeps reporting its own validation error.
fn anchor_cli_path(
    path: String,
    cwd: &std::path::Path,
    home: Option<&std::path::Path>,
) -> String {
    anchor_cli_path_with(path, cwd, home, cfg!(windows))
}

fn anchor_cli_path_with(
    path: String,
    cwd: &std::path::Path,
    home: Option<&std::path::Path>,
    windows: bool,
) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return path;
    }
    let home_rest = trimmed
        .strip_prefix("~/")
        .or_else(|| windows.then(|| trimmed.strip_prefix("~\\")).flatten());
    let expanded = match (home_rest, home) {
        (Some(rest), Some(home)) => home.join(rest),
        (None, Some(home)) if trimmed == "~" => home.to_path_buf(),
        _ => std::path::PathBuf::from(trimmed),
    };
    if expanded.is_relative() {
        cwd.join(expanded).to_string_lossy().into_owned()
    } else {
        expanded.to_string_lossy().into_owned()
    }
}

fn cli_home_dir() -> Option<std::path::PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .filter(|home| !home.is_empty())
        .map(std::path::PathBuf::from)
}

pub fn parse_launch_mode<I, T>(args: I) -> LaunchMode
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    for arg in args {
        if arg.as_ref() == crate::daemon::supervisor_windows::SUPERVISOR_ARG {
            return LaunchMode::DaemonSupervisor;
        }
        if arg.as_ref() == "--daemon" {
            return LaunchMode::Daemon;
        }
    }
    LaunchMode::Gui
}

/// Whether `args` (the full command line) is a plain app launch rather than a command.
///
/// Only the bare executable opens the window. macOS may still append a `-psn_<id>` process serial
/// number when Launch Services starts the bundle; that is a launch, not a command.
pub fn is_bare_gui_launch<T: AsRef<str>>(args: &[T]) -> bool {
    args.iter().skip(1).all(|arg| arg.as_ref().starts_with("-psn_"))
}

/// Runs the `--daemon-supervisor` watchdog and returns its exit code.
///
/// `args` is the full command line. The task forwards the isolation variables as `--env KEY=VALUE`
/// because a scheduled task has no environment block; they are applied here, while the process is
/// still single-threaded, so the watchdog and every daemon it spawns use the same runtime dir.
pub fn run_daemon_supervisor(args: &[String]) -> i32 {
    let forwarded = match crate::daemon::supervisor_windows::parse_supervisor_args(
        args.get(1..).unwrap_or_default(),
    ) {
        Some(Ok(forwarded)) => forwarded,
        Some(Err(error)) => {
            eprintln!("Ferryx daemon supervisor error: {error}");
            return 2;
        }
        None => {
            eprintln!(
                "Ferryx daemon supervisor error: {} must be the first argument",
                crate::daemon::supervisor_windows::SUPERVISOR_ARG
            );
            return 2;
        }
    };
    for (key, value) in forwarded {
        std::env::set_var(key, value);
    }
    #[cfg(windows)]
    {
        crate::daemon::supervisor_windows::run_watchdog()
    }
    #[cfg(not(windows))]
    {
        eprintln!("Ferryx daemon supervisor error: the watchdog only runs on Windows");
        2
    }
}

/// Parses `--handover-from <path>`.
///
/// Returns `Ok(None)` when the flag is absent (a legitimate cold start) and an
/// error when the flag is present but its value is missing or is itself an
/// option. Treating a malformed flag as "no handover" would silently cold-start
/// the daemon and abandon every live PTY session the handover existed to carry
/// across, while still looking like a successful start to the caller.
pub fn parse_handover_from<I, T>(args: I) -> Result<Option<std::path::PathBuf>, String>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();
    let Some(pos) = args.iter().position(|a| a == "--handover-from") else {
        return Ok(None);
    };
    match args.get(pos + 1) {
        Some(value) if !value.starts_with("--") => Ok(Some(std::path::PathBuf::from(value))),
        _ => Err("missing value for --handover-from".to_string()),
    }
}

pub fn run_daemon_headless(
    handover_from: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let announce_readiness = handover_from.is_none();
    // Install before anything else: a panic during startup is exactly the death that left no
    // trace on 2026-09-25, and the desktop discards this process's stderr.
    crate::daemon::logging::install_panic_recorder();
    // Before any thread exists: keeps NOTIFY_SOCKET away from every PTY child.
    #[cfg(unix)]
    crate::daemon::supervisor_linux::capture_notify_socket();
    #[cfg(target_os = "linux")]
    let supervisor_unavailable = crate::daemon::supervisor_linux::availability().clone();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let logging = match crate::daemon::logging::DaemonLogging::start().await {
            Ok(logging) => Some(logging),
            Err(error) => {
                crate::daemon::logging::report_failure(&error);
                None
            }
        };
        #[cfg(target_os = "linux")]
        if let Some(reason) = supervisor_unavailable {
            tracing::warn!("crash-restart unavailable: {reason}");
        }
        #[cfg(test)]
        if std::env::var("FERRYX_LOGGING_FIXTURE").as_deref() == Ok("1") {
            let hub = crate::daemon::agent_state::AgentStateHub::default();
            hub.release_manual("logging-fixture-session");
            hub.release_foreground("logging-fixture-session");
            // A record from a different target than the agent-state diagnostics: the daemon's
            // handover and lifecycle records are the ones an incident needs, so the sink must not
            // be restricted to one target.
            tracing::info!(
                target: "ferryx_lib::daemon::server",
                marker = "handover-probe",
                "fixture lifecycle record"
            );
            if let Some(logging) = logging {
                logging.finish().await?;
            }
            return Ok(());
        }
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        #[cfg(test)]
        let lifecycle_fixture = std::env::var("FERRYX_LOGGING_FIXTURE").is_ok_and(|v| v != "1");
        #[cfg(not(test))]
        let lifecycle_fixture = false;
        let mut hub_server: Option<Arc<crate::daemon::server::DaemonServer>> = None;
        let server_task = if lifecycle_fixture {
            tokio::spawn(async move {
                let _ = ready_tx.send(());
                crate::daemon::agent_state::AgentStateHub::default()
                    .release_manual("failure-fixture");
                use tokio::io::AsyncReadExt;
                let mut byte = [0];
                tokio::io::stdin()
                    .read_exact(&mut byte)
                    .await
                    .map_err(|e| e.to_string())?;
                println!("FERRYX_PRIMARY_SERVICE_ALIVE");
                Ok(())
            })
        } else {
            let server = Arc::new(crate::daemon::server::DaemonServer::new());
            hub_server = Some(Arc::clone(&server));
            let server_clone = Arc::clone(&server);
            let server_task = tokio::spawn(async move {
                server_clone
                    .run_server_with_handover_and_readiness(handover_from, Some(ready_tx))
                    .await
            });

            let registry = server.workspace_registry().clone();
            tokio::task::spawn_blocking(move || {
                if let Ok(initial) = crate::ipc::project::initial_project(&registry) {
                    tracing::info!(
                        workspace_id = %initial.workspace_id,
                        repo_root = %initial.repo_root.display(),
                        "Registered startup workspace for headless daemon"
                    );
                }
            });

            // Own the termination signals. Without a handler the default disposition ends this
            // process instantly and no code of ours runs, which is how a daemon death once left
            // every PTY gone and nothing at all in the log to explain it.
            #[cfg(unix)]
            {
                let signal_server = Arc::clone(&server);
                tokio::spawn(async move {
                    let Some(signal) = crate::daemon::logging::next_shutdown_signal().await else {
                        return;
                    };
                    crate::daemon::logging::append_record(
                        &crate::daemon::logging::signal_record(signal),
                    );
                    signal_server.shutdown_gracefully("termination signal").await;
                    std::process::exit(0);
                });
            }

            server_task
        };

        // Wait for server to bind listener and initialize before emitting readiness signal
        match ready_rx.await {
            Ok(()) => {
                if announce_readiness {
                    println!("FERRYX_DAEMON_READY");
                    let _ = std::io::stdout().flush();
                    #[cfg(unix)]
                    crate::daemon::supervisor_linux::notify_ready();
                }
                // The hub starts after readiness so it cannot delay FERRYX_DAEMON_READY. A
                // `--handover-from` successor announces nothing on stdout, but it still owns the
                // hub from here on: the predecessor's hub tasks end when it retires, and the
                // persisted hub_stopped flag decides whether Slack opens again. A hub failure is
                // logged and the daemon keeps serving panes.
                if let Some(server) = hub_server {
                    tokio::spawn(async move {
                        if let Err(error) = crate::hub_runtime::start(server).await {
                            tracing::warn!(%error, "ferryx hub did not start");
                        }
                    });
                }
            }
            Err(_) => {
                // If ready_tx dropped, server_task must have returned an error
            }
        }

        let completed = server_task.await;
        let mut result = match completed {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e.into()),
            Err(e) => Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
        };

        // Losing the instance-lock race is not a failure: another process already owns this endpoint
        // and serves every session behind it. Reporting it as an error would make the supervisor
        // relaunch a process that can never win, so say why and exit clean.
        if result.is_err() {
            if let Some(owner_pid) = crate::daemon::endpoint_owner_pid().await {
                if owner_pid != std::process::id() {
                    if let Err(error) = &result {
                        tracing::warn!(
                            owner_pid,
                            %error,
                            "Another daemon already serves this endpoint; exiting without taking over"
                        );
                    }
                    // A `Type=notify` unit whose process exits before `READY=1` counts as failed
                    // and `Restart=on-failure` would relaunch it in a loop; report ready so this
                    // clean exit stays a clean, unrestarted stop.
                    #[cfg(unix)]
                    crate::daemon::supervisor_linux::notify_ready();
                    result = Ok(());
                }
            }
        }
        if let Some(logging) = logging {
            if let Err(error) = logging.finish().await {
                crate::daemon::logging::report_failure(&error);
            }
        }
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn up_args(extra: &[&str]) -> Vec<String> {
        ["ferryx-cli", "up"]
            .iter()
            .chain(extra.iter())
            .map(|arg| arg.to_string())
            .collect()
    }

    #[test]
    fn browser_cli_runtime_supports_the_request_timeout() {
        let runtime = browser_cli_runtime().expect("runtime");
        let result = runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), async { 7 }).await
        });
        assert_eq!(result.expect("ready future must not time out"), 7);
    }

    #[test]
    fn up_parses_every_flag() {
        let command = parse_up_cli(up_args(&[
            "--key-env",
            "--email",
            "me@example.com",
            "--origin",
            "https://relay.example",
            "--name",
            "box",
            "--no-service",
            "--json",
        ]))
        .expect("parse");
        assert_eq!(
            command,
            UpCliCommand {
                key_source: UpKeySource::Env,
                email: Some("me@example.com".into()),
                origin: Some("https://relay.example".into()),
                name: Some("box".into()),
                no_service: true,
                json: true,
            }
        );
        let bare = parse_up_cli(up_args(&[])).expect("bare up");
        assert_eq!(bare.key_source, UpKeySource::Default);
        assert!(!bare.no_service && !bare.json);
        assert_eq!(
            parse_up_cli(up_args(&["--key-stdin"])).expect("stdin").key_source,
            UpKeySource::Stdin
        );
    }

    #[test]
    fn up_refuses_keys_in_argv_and_bad_usage_with_exit_2() {
        for args in [
            vec!["--key", "fxk_secret"],
            vec!["--key=fxk_secret"],
            vec!["--key-stdin", "--key-env"],
            vec!["--email"],
            vec!["--origin", "--json"],
            vec!["--bogus"],
        ] {
            let error = parse_up_cli(up_args(&args)).expect_err("must refuse");
            assert_eq!(error.exit_code, UP_EXIT_USAGE, "{args:?}");
            assert!(!error.message.contains("fxk_secret"), "the key must never be echoed");
        }
        assert_eq!(
            parse_up_cli(up_args(&["--key", "fxk_secret"])).unwrap_err().code,
            "INSTALL_KEY_IN_ARGV"
        );
        assert!(parse_up_cli(["ferryx-cli", "account"]).is_err());
    }

    #[test]
    fn up_help_is_recognized_and_version_line_names_the_binary() {
        assert!(up_help_requested(&up_args(&["--help"])));
        assert!(up_help_requested(&up_args(&["-h"])));
        assert!(up_help_requested(&up_args(&["--json", "--help"])));
        assert!(!up_help_requested(&up_args(&["--json"])));
        assert!(!up_help_requested(&["ferryx-cli".to_string(), "--help".to_string()]));
        assert_eq!(
            cli_version_line(),
            format!("ferryx-cli {}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn up_key_resolution_reads_stdin_or_env() {
        let no_stdin = || -> std::io::Result<String> { panic!("stdin must not be read") };
        assert_eq!(
            resolve_up_key(UpKeySource::Stdin, || Ok("fxk_abc\n".to_string()), None),
            Ok(Some("fxk_abc".into()))
        );
        assert_eq!(
            resolve_up_key(UpKeySource::Stdin, || Ok("\n".to_string()), None)
                .unwrap_err()
                .exit_code,
            UP_EXIT_USAGE
        );
        assert_eq!(
            resolve_up_key(UpKeySource::Env, no_stdin, Some(" fxk_env ".into())),
            Ok(Some("fxk_env".into()))
        );
        assert_eq!(
            resolve_up_key(UpKeySource::Env, no_stdin, None).unwrap_err().code,
            "INSTALL_KEY_MISSING"
        );
        assert_eq!(
            resolve_up_key(UpKeySource::Default, no_stdin, Some("fxk_implicit".into())),
            Ok(Some("fxk_implicit".into()))
        );
        assert_eq!(resolve_up_key(UpKeySource::Default, no_stdin, Some("  ".into())), Ok(None));
        assert_eq!(resolve_up_key(UpKeySource::Default, no_stdin, None), Ok(None));
    }

    #[test]
    fn up_exit_codes_follow_structured_error_codes() {
        for code in [
            "INSTALL_KEY_USED",
            "INSTALL_KEY_EXPIRED",
            "ENROLL_CODE_INVALID",
            "ENROLL_CODE_EXPIRED",
            "ENROLL_CODE_MACHINE_MISMATCH",
        ] {
            assert_eq!(up_exit_code(code, 401), UP_EXIT_KEY_REJECTED, "{code}");
        }
        assert_eq!(up_exit_code("MACHINE_REQUEST_DENIED", 0), UP_EXIT_DENIED);
        assert_eq!(up_exit_code("MACHINE_REQUEST_EXPIRED", 400), UP_EXIT_APPROVAL_EXPIRED);
        assert_eq!(up_exit_code("PLAN_LIMIT_REACHED", 402), UP_EXIT_PLAN_LIMIT);
        assert_eq!(up_exit_code("SOMETHING_NEW", 402), UP_EXIT_PLAN_LIMIT);
        assert_eq!(up_exit_code("ACCOUNT_MACHINE_CLAIMED", 409), UP_EXIT_OTHER);
        assert_eq!(up_exit_code("ACCOUNT_UNREACHABLE", 0), UP_EXIT_OTHER);

        let used = crate::account::enroll_client::EnrollError {
            code: "INSTALL_KEY_USED".into(),
            message: "install key already used".into(),
            status: 409,
            details: None,
        };
        let mapped = up_error_from_enroll("https://relay.example", &used);
        assert_eq!(mapped.exit_code, UP_EXIT_KEY_REJECTED);
        assert_eq!(mapped.code, "INSTALL_KEY_USED");
    }

    #[test]
    fn approval_polling_honors_interval_and_slow_down() {
        assert_eq!(approval_poll_interval(3), std::time::Duration::from_secs(3));
        assert_eq!(approval_poll_interval(0), std::time::Duration::from_secs(1));
        assert_eq!(
            slowed_down_interval(std::time::Duration::from_secs(3)),
            std::time::Duration::from_secs(8)
        );
        assert_eq!(
            slowed_down_interval(std::time::Duration::from_secs(58)),
            std::time::Duration::from_secs(60)
        );
    }

    #[test]
    fn approval_polling_retries_unreachable_and_server_errors_only() {
        let error = |code: &str, status: u16| crate::account::enroll_client::EnrollError {
            code: code.into(),
            message: String::new(),
            status,
            details: None,
        };
        assert!(poll_error_is_transient(&error("ACCOUNT_UNREACHABLE", 0)));
        assert!(poll_error_is_transient(&error("INTERNAL_ERROR", 500)));
        assert!(poll_error_is_transient(&error("ACCOUNT_HTTP_502", 502)));
        assert!(!poll_error_is_transient(&error("DEVICE_CODE_EXPIRED", 400)));
        assert!(!poll_error_is_transient(&error("MACHINE_REQUEST_INVALID", 400)));
    }

    #[test]
    fn enrolled_origin_match_ignores_trailing_slash() {
        let record = crate::account::enroll_client::AccountEnrollmentRecord {
            account_id: "a".into(),
            machine_record_id: "m".into(),
            account_origin: "https://relay.example".into(),
            relay_origin: "https://relay.example".into(),
            enrollment_epoch: "1".into(),
            enrolled_at: 1,
        };
        assert!(enrolled_to_origin(&record, "https://relay.example/"));
        assert!(!enrolled_to_origin(&record, "https://other.example"));
    }

    #[test]
    fn up_summary_json_matches_the_contract_shape() {
        let service = ServiceReport {
            supervised: false,
            backend: "detached",
            installed: true,
            linger: None,
            pid: Some(42),
            reason: Some("systemd is not the init system".into()),
        };
        let summary = UpSummary {
            cli_path: "/home/u/.local/bin/ferryx-cli".into(),
            version: "1.0.0",
            enrolled: Some(UpEnrolled {
                account_id: "acct".into(),
                machine_record_id: "rec".into(),
                relay_origin: "https://relay.example".into(),
                relay_activation: "enabled",
                relay_activation_error: None,
                remote: RemoteLiveStatus {
                    daemon: "running",
                    mode: crate::remote::RemoteNetworkMode::Relay,
                    relay_connected: Some(true),
                    relay_url: Some("https://relay.example".into()),
                    relay_url_source: Some("config"),
                },
            }),
            service: service.clone(),
        };
        let value = serde_json::to_value(&summary).expect("json");
        assert_eq!(
            value,
            serde_json::json!({
                "cliPath": "/home/u/.local/bin/ferryx-cli",
                "version": "1.0.0",
                "enrolled": {
                    "accountId": "acct",
                    "machineRecordId": "rec",
                    "relayOrigin": "https://relay.example",
                    "relayActivation": "enabled",
                    "remote": {"daemon": "running", "mode": "relay", "relayConnected": true, "relayUrl": "https://relay.example", "relayUrlSource": "config"}
                },
                "service": {"supervised": false, "backend": "detached", "installed": true, "linger": null, "pid": 42, "reason": "systemd is not the init system"}
            })
        );
        let unenrolled = UpSummary {
            enrolled: None,
            ..summary
        };
        let value = serde_json::to_value(&unenrolled).expect("json");
        assert!(value.get("enrolled").is_none(), "enrolled is omitted when nothing is enrolled");
        assert_eq!(up_summary_exit_code(&unenrolled, false), UP_EXIT_OK);
        let not_running = UpSummary {
            service: ServiceReport {
                installed: false,
                ..service
            },
            ..unenrolled
        };
        assert_eq!(up_summary_exit_code(&not_running, false), UP_EXIT_OTHER);
        assert_eq!(up_summary_exit_code(&not_running, true), UP_EXIT_OK);
    }

    /// A daemon serving `config_path` on a private test socket, and an existing-only client of it.
    #[cfg(unix)]
    async fn up_test_daemon(
        dir: &std::path::Path,
    ) -> (
        crate::daemon::DaemonClient,
        Arc<crate::daemon::server::DaemonServer>,
        tokio::task::JoinHandle<()>,
    ) {
        let socket_path = dir.join("daemon.sock");
        let listener = tokio::net::UnixListener::bind(&socket_path).unwrap();
        let server = Arc::new(crate::daemon::server::DaemonServer::new_with_paths(
            Some(dir.join("remote-config.json")),
            Some(dir.join("remote-auth.json")),
        ));
        let serving = Arc::clone(&server);
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(Arc::clone(&serving).handle_client(stream));
            }
        });
        let client = crate::daemon::DaemonClient::existing_only_at(
            socket_path,
            std::time::Duration::from_secs(5),
        );
        (client, server, task)
    }

    /// A relay origin nothing listens on: the gateway enters Relay mode and its tunnel retries.
    const UP_TEST_RELAY: &str = "http://127.0.0.1:9";

    #[cfg(unix)]
    #[tokio::test]
    async fn up_enables_relay_on_a_running_daemon_and_persists_it() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("remote-config.json");
        let (client, server, task) = up_test_daemon(dir.path()).await;

        let activation = enable_relay_after_enroll(
            &client,
            Some(&config_path),
            UP_TEST_RELAY,
            std::time::Duration::ZERO,
        )
        .await;

        assert_eq!(activation.outcome, "enabled", "{:?}", activation.error);
        assert_eq!(activation.remote.daemon, "running");
        assert_eq!(activation.remote.mode, crate::remote::RemoteNetworkMode::Relay);
        assert_eq!(activation.remote.relay_url.as_deref(), Some(UP_TEST_RELAY));
        assert_eq!(activation.remote.relay_url_source, Some("config"));
        let saved = crate::remote::state::load_persisted_config(&config_path).expect("saved config");
        assert_eq!(saved.mode, crate::remote::RemoteNetworkMode::Relay);
        assert_eq!(saved.relay_url.as_deref(), Some(UP_TEST_RELAY));

        let status = remote_status_output(&client, Some(&config_path)).await;
        let value = serde_json::to_value(&status).unwrap();
        assert_eq!(value["status"], "ok");
        assert_eq!(value["daemon"], "running");
        assert_eq!(value["mode"], "relay");
        assert!(value["relayConnected"].is_boolean(), "{value}");
        assert_eq!(value["relayUrl"], UP_TEST_RELAY);
        assert_eq!(value["relayUrlSource"], "config");

        server
            .handle_remote_configure(crate::remote::RemoteGatewayConfig::default())
            .await
            .unwrap();
        task.abort();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn up_leaves_a_user_chosen_mode_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("remote-config.json");
        let (client, server, task) = up_test_daemon(dir.path()).await;
        server
            .handle_remote_configure(crate::remote::RemoteGatewayConfig {
                mode: crate::remote::RemoteNetworkMode::LocalNetwork,
                ..Default::default()
            })
            .await
            .expect("enable local network");

        let activation = enable_relay_after_enroll(
            &client,
            Some(&config_path),
            UP_TEST_RELAY,
            std::time::Duration::ZERO,
        )
        .await;

        assert_eq!(activation.outcome, "unchanged");
        assert_eq!(activation.remote.mode, crate::remote::RemoteNetworkMode::LocalNetwork);
        assert_eq!(
            server.remote_state().config.read().mode,
            crate::remote::RemoteNetworkMode::LocalNetwork
        );
        let saved = crate::remote::state::load_persisted_config(&config_path).expect("saved config");
        assert_eq!(saved.mode, crate::remote::RemoteNetworkMode::LocalNetwork);
        assert_eq!(saved.relay_url, None);

        server
            .handle_remote_configure(crate::remote::RemoteGatewayConfig::default())
            .await
            .unwrap();
        task.abort();
    }

    #[tokio::test]
    async fn up_without_a_daemon_saves_relay_for_the_next_boot_and_status_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("remote-config.json");
        let client = crate::daemon::DaemonClient::existing_only_at(
            dir.path().join("absent.sock"),
            std::time::Duration::ZERO,
        );

        let activation =
            enable_relay_after_enroll(&client, Some(&config_path), UP_TEST_RELAY, std::time::Duration::ZERO)
                .await;

        assert_eq!(activation.outcome, "saved", "{:?}", activation.error);
        assert_eq!(activation.remote.daemon, "notRunning");
        assert_eq!(activation.remote.relay_connected, None);
        let saved = crate::remote::state::load_persisted_config(&config_path).expect("saved config");
        assert_eq!(saved.mode, crate::remote::RemoteNetworkMode::Relay);
        assert_eq!(saved.relay_url.as_deref(), Some(UP_TEST_RELAY));

        let value = serde_json::to_value(remote_status_output(&client, Some(&config_path)).await).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "status": "daemonNotRunning",
                "port": crate::remote::REMOTE_GATEWAY_PORT,
                "daemon": "notRunning",
                "mode": "relay",
                "relayConnected": null,
                "relayUrl": UP_TEST_RELAY,
                "relayUrlSource": "config"
            })
        );

        // A saved non-Off mode is kept, and a second run is a no-op.
        crate::remote::state::persist_relay_if_off(&config_path, "http://127.0.0.1:10").unwrap();
        assert_eq!(
            crate::remote::state::load_persisted_config(&config_path).unwrap().relay_url.as_deref(),
            Some(UP_TEST_RELAY)
        );
    }

    #[tokio::test]
    async fn up_without_a_daemon_leaves_an_unreadable_config_alone() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("remote-config.json");
        std::fs::write(&config_path, b"{not json").unwrap();
        let client = crate::daemon::DaemonClient::existing_only_at(
            dir.path().join("absent.sock"),
            std::time::Duration::ZERO,
        );

        let activation =
            enable_relay_after_enroll(&client, Some(&config_path), UP_TEST_RELAY, std::time::Duration::ZERO)
                .await;

        assert_eq!(activation.outcome, "unchanged");
        assert!(activation.error.is_some());
        assert_eq!(activation.remote.daemon, "notRunning");
        assert_eq!(std::fs::read(&config_path).unwrap(), b"{not json");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn up_leaves_a_running_daemon_and_its_unreadable_config_alone() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("remote-config.json");
        std::fs::write(&config_path, b"{not json").unwrap();
        let (client, server, task) = up_test_daemon(dir.path()).await;

        let activation = enable_relay_after_enroll(
            &client,
            Some(&config_path),
            UP_TEST_RELAY,
            std::time::Duration::ZERO,
        )
        .await;

        assert_eq!(activation.outcome, "unchanged");
        assert_eq!(activation.error.as_deref(), Some(UNREADABLE_REMOTE_CONFIG_NOTE));
        assert_eq!(activation.remote.daemon, "running");
        assert_eq!(activation.remote.mode, crate::remote::RemoteNetworkMode::Off);
        assert_eq!(
            server.remote_state().config.read().mode,
            crate::remote::RemoteNetworkMode::Off
        );
        assert_eq!(std::fs::read(&config_path).unwrap(), b"{not json");
        task.abort();
    }

    #[test]
    fn relay_already_on_message_tells_live_saved_and_other_relay_apart() {
        let origin = "https://relay.example";
        let activation = |daemon: &'static str, relay_url: &str| RelayActivation {
            outcome: "unchanged",
            error: None,
            remote: RemoteLiveStatus {
                daemon,
                mode: crate::remote::RemoteNetworkMode::Relay,
                relay_connected: None,
                relay_url: Some(relay_url.to_string()),
                relay_url_source: Some("config"),
            },
        };
        let live = relay_activation_message(&activation("running", origin), origin);
        let saved = relay_activation_message(&activation("notRunning", origin), origin);
        let unreachable = relay_activation_message(&activation("unreachable", origin), origin);
        let other = relay_activation_message(&activation("running", "http://127.0.0.1:9"), origin);

        assert_ne!(live, saved);
        assert_eq!(saved, unreachable);
        assert!(other.contains("http://127.0.0.1:9") && other.contains(origin), "{other}");
        assert_ne!(other, live);
        assert_eq!(
            relay_activation_message(&activation("running", "https://relay.example/"), origin),
            live
        );
    }

    #[test]
    fn daemon_absence_is_not_running_only_when_no_daemon_owns_the_endpoint() {
        use crate::ipc::{IpcError, IpcErrorCode};
        let no_daemon = IpcError::new(
            IpcErrorCode::HostUnavailable,
            crate::daemon::EXISTING_ONLY_NO_DAEMON_MESSAGE,
        );
        assert_eq!(daemon_absence(&no_daemon), "notRunning");
        let handover = IpcError::new(IpcErrorCode::HostUnavailable, "Daemon handover handshake timed out");
        assert_eq!(daemon_absence(&handover), "unreachable");
        let mismatch = IpcError::new(IpcErrorCode::DaemonUpdatePending, "another protocol");
        assert_eq!(daemon_absence(&mismatch), "unreachable");
    }

    #[test]
    fn service_parses_actions_and_json() {
        assert_eq!(
            parse_service_cli(["ferryx-cli", "service", "install", "--json"]).expect("install"),
            ServiceCliCommand {
                action: ServiceCliAction::Install,
                json: true
            }
        );
        assert_eq!(
            parse_service_cli(["ferryx-cli", "service", "status"]).expect("status").action,
            ServiceCliAction::Status
        );
        assert_eq!(
            parse_service_cli(["ferryx-cli", "service", "uninstall"]).expect("uninstall").action,
            ServiceCliAction::Uninstall
        );
        for args in [
            vec!["ferryx-cli", "service"],
            vec!["ferryx-cli", "service", "restart"],
            vec!["ferryx-cli", "service", "status", "--force"],
        ] {
            assert_eq!(parse_service_cli(&args).unwrap_err().exit_code, UP_EXIT_USAGE, "{args:?}");
        }
    }

    #[test]
    fn native_backend_names_the_contract_values() {
        assert!(["systemd-user", "launchd", "schtasks", "none"].contains(&native_service_backend()));
    }

    #[test]
    fn headless_logging_child() {
        if std::env::var_os("FERRYX_LOGGING_FIXTURE").is_some() {
            run_daemon_headless(None).expect("headless initialization and shutdown");
        }
    }

    #[test]
    fn headless_release_reasons_reach_private_bounded_sink() {
        // Given: a separate process with all writable locations inside this worktree.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
        std::fs::create_dir_all(&root).unwrap();
        let fixture = tempfile::tempdir_in(root).unwrap();
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap());
        child
            .args([
                "--exact",
                "cli::tests::headless_logging_child",
                "--nocapture",
            ])
            .env("FERRYX_LOGGING_FIXTURE", "1")
            .current_dir(fixture.path())
            .kill_on_drop(true);
        for key in [
            "FERRYX_DATA_DIR",
            "FERRYX_RUNTIME_DIR",
            "HOME",
            "USERPROFILE",
            "LOCALAPPDATA",
            "APPDATA",
            "TMPDIR",
            "TMP",
            "TEMP",
        ] {
            child.env(key, fixture.path());
        }
        // When: production headless initialization emits both real release operations and exits.
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let output = runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(20), child.output())
                .await
                .expect("bounded child exit")
                .unwrap()
        });
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        // Then: both structured reasons and their session reach the daemon-owned file.
        let path = fixture.path().join("logs/daemon.log");
        let text = std::fs::read_to_string(&path).expect("headless release log must exist");
        for reason in ["manual_reset", "foreground_agent_to_shell"] {
            assert!(
                text.lines().any(
                    |line| line.contains("session_id=\"logging-fixture-session\"")
                        && line.contains(&format!("reason=\"{reason}\""))
                ),
                "missing {reason}: {text}"
            );
        }
        assert!(text.len() <= 1024 * 1024);
        assert!(
            text.lines().any(|line| line.contains("ferryx_lib::daemon::server")
                && line.contains("marker=\"handover-probe\"")),
            "daemon log still drops records from targets other than agent-state: {text}"
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("reason="));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("reason="));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn headless_primary_service_survives_logging_failures() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        for mode in ["init_failure", "write_failure"] {
            // Given: isolated production CLI lifecycle with a gated primary task.
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
            std::fs::create_dir_all(&root).unwrap();
            let fixture = tempfile::tempdir_in(root).unwrap();
            if mode == "init_failure" {
                std::fs::write(fixture.path().join("logs"), b"not a directory").unwrap();
            }
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(20), async {
                    let mut command =
                        tokio::process::Command::new(std::env::current_exe().unwrap());
                    command
                        .args([
                            "--exact",
                            "cli::tests::headless_logging_child",
                            "--nocapture",
                        ])
                        .env("FERRYX_LOGGING_FIXTURE", mode)
                        .current_dir(fixture.path())
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped())
                        .kill_on_drop(true);
                    for key in [
                        "FERRYX_DATA_DIR",
                        "FERRYX_RUNTIME_DIR",
                        "HOME",
                        "USERPROFILE",
                        "LOCALAPPDATA",
                        "APPDATA",
                        "TMPDIR",
                        "TMP",
                        "TEMP",
                    ] {
                        command.env(key, fixture.path());
                    }
                    let mut child = command.spawn().unwrap();
                    let mut errors = BufReader::new(child.stderr.take().unwrap()).lines();
                    // When: the actual CLI reports sink failure, release the primary task.
                    let report = errors.next_line().await.unwrap().unwrap_or_default();
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = stdin.write_all(b"x").await;
                    }
                    let output = child.wait_with_output().await.unwrap();
                    // Then: readiness and a post-failure service action both survive.
                    assert!(output.status.success(), "{mode}: {report}");
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    assert!(stdout.contains("FERRYX_DAEMON_READY"), "{mode}: {stdout}");
                    assert!(
                        stdout.contains("FERRYX_PRIMARY_SERVICE_ALIVE"),
                        "{mode}: {stdout}"
                    );
                    assert!(
                        report.contains("FERRYX_DAEMON_LOGGING_DISABLED"),
                        "{report}"
                    );
                    assert!(
                        errors.next_line().await.unwrap().is_none(),
                        "only one failure report"
                    );
                })
                .await
                .expect("bounded lifecycle fixture exit");
            });
        }
    }

    #[test]
    fn browser_list_cli_parses_without_browser_id() {
        let args = vec!["ferryx", "browser", "list"];
        assert_eq!(
            parse_browser_cli(&args).expect("parse browser list"),
            BrowserCliCommand::List
        );
    }

    #[test]
    fn open_cli_parses_absolute_file() {
        let temp_file = tempfile::NamedTempFile::new().expect("create temp file");
        let path = temp_file.path().canonicalize().expect("canonicalize");
        let path_str = path.to_str().expect("path utf8");
        let dummy_cwd = std::path::Path::new("/dummy");

        let parsed = parse_open_cli(["ferryx", "open", path_str], dummy_cwd).expect("parse open");
        assert_eq!(parsed.path, path_str);
        assert_eq!(parsed.line, None);
        assert_eq!(parsed.col, None);
    }

    #[test]
    fn open_cli_parses_file_with_line() {
        let temp_file = tempfile::NamedTempFile::new().expect("create temp file");
        let path = temp_file.path().canonicalize().expect("canonicalize");
        let path_str = path.to_str().expect("path utf8");
        let target = format!("{path_str}:12");
        let dummy_cwd = std::path::Path::new("/dummy");

        let parsed = parse_open_cli(["ferryx", "open", &target], dummy_cwd).expect("parse open");
        assert_eq!(parsed.path, path_str);
        assert_eq!(parsed.line, Some(12));
        assert_eq!(parsed.col, None);
    }

    #[test]
    fn open_cli_parses_file_with_line_and_col() {
        let temp_file = tempfile::NamedTempFile::new().expect("create temp file");
        let path = temp_file.path().canonicalize().expect("canonicalize");
        let path_str = path.to_str().expect("path utf8");
        let target = format!("{path_str}:12:3");
        let dummy_cwd = std::path::Path::new("/dummy");

        let parsed = parse_open_cli(["ferryx", "open", &target], dummy_cwd).expect("parse open");
        assert_eq!(parsed.path, path_str);
        assert_eq!(parsed.line, Some(12));
        assert_eq!(parsed.col, Some(3));
    }

    #[test]
    fn open_cli_resolves_relative_path_against_cwd() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let file_name = "example.txt";
        let file_path = temp_dir.path().join(file_name);
        std::fs::write(&file_path, "hello ferryx").expect("write test file");
        let expected_canonical = file_path.canonicalize().expect("canonicalize");
        let expected_str = expected_canonical.to_str().expect("path utf8");

        let parsed = parse_open_cli(["ferryx", "open", file_name], temp_dir.path())
            .expect("parse open relative");
        assert_eq!(parsed.path, expected_str);
        assert_eq!(parsed.line, None);
        assert_eq!(parsed.col, None);

        let target = format!("{file_name}:42:7");
        let parsed_line_col = parse_open_cli(["ferryx", "open", &target], temp_dir.path())
            .expect("parse open relative with line and col");
        assert_eq!(parsed_line_col.path, expected_str);
        assert_eq!(parsed_line_col.line, Some(42));
        assert_eq!(parsed_line_col.col, Some(7));
    }

    #[test]
    fn open_cli_missing_file_errors_with_file_not_found() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let err = parse_open_cli(
            ["ferryx", "open", "does_not_exist_file.txt"],
            temp_dir.path(),
        )
        .unwrap_err();
        assert!(
            err.contains("file not found"),
            "expected 'file not found' in '{err}'"
        );
    }

    #[test]
    fn open_cli_directory_errors_with_file_not_found() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let subdir = temp_dir.path().join("subdir");
        std::fs::create_dir(&subdir).expect("create subdir");
        let err = parse_open_cli(["ferryx", "open", "subdir"], temp_dir.path()).unwrap_err();
        assert!(
            err.contains("file not found"),
            "expected 'file not found' in '{err}'"
        );
    }

    #[test]
    fn open_cli_missing_arg_errors_with_usage() {
        let dummy_cwd = std::path::Path::new("/dummy");
        let err_missing = parse_open_cli(["ferryx", "open"], dummy_cwd).unwrap_err();
        assert!(
            err_missing.contains("usage"),
            "expected 'usage' in '{err_missing}'"
        );

        let err_no_open = parse_open_cli(["ferryx"], dummy_cwd).unwrap_err();
        assert!(
            err_no_open.contains("usage"),
            "expected 'usage' in '{err_no_open}'"
        );

        let err_extra =
            parse_open_cli(["ferryx", "open", "file.txt", "extra"], dummy_cwd).unwrap_err();
        assert!(
            err_extra.contains("usage"),
            "expected 'usage' in '{err_extra}'"
        );
    }

    #[test]
    fn open_cli_target_suffix_parsing_rules() {
        assert_eq!(
            split_path_line_col("C:\\path\\file.rs"),
            ("C:\\path\\file.rs", None, None)
        );
        assert_eq!(
            split_path_line_col("C:\\path\\file.rs:10"),
            ("C:\\path\\file.rs", Some(10), None)
        );
        assert_eq!(
            split_path_line_col("C:\\path\\file.rs:10:5"),
            ("C:\\path\\file.rs", Some(10), Some(5))
        );
        assert_eq!(split_path_line_col("C:10"), ("C:10", None, None));
        assert_eq!(split_path_line_col("C:10:5"), ("C:10:5", None, None));
        assert_eq!(split_path_line_col(":10"), (":10", None, None));
        assert_eq!(split_path_line_col(":10:5"), (":10:5", None, None));
    }

    #[test]
    fn test_parse_launch_mode_default_is_gui() {
        let args = vec!["ferryx".to_string()];
        assert_eq!(parse_launch_mode(&args), LaunchMode::Gui);
    }

    #[test]
    fn test_parse_launch_mode_detects_daemon_flag() {
        let args = vec!["ferryx".to_string(), "--daemon".to_string()];
        assert_eq!(parse_launch_mode(&args), LaunchMode::Daemon);
    }

    #[test]
    fn test_parse_launch_mode_ignores_other_flags() {
        let args = vec![
            "ferryx".to_string(),
            "--verbose".to_string(),
            "--other".to_string(),
        ];
        assert_eq!(parse_launch_mode(&args), LaunchMode::Gui);
    }

    #[test]
    fn test_parse_launch_mode_detects_daemon_supervisor_flag() {
        let args = vec!["ferryx".to_string(), "--daemon-supervisor".to_string()];
        assert_eq!(parse_launch_mode(&args), LaunchMode::DaemonSupervisor);
        let malformed = vec!["ferryx".to_string(), "--daemon-supervisor".to_string(), "--bogus".to_string()];
        assert_eq!(run_daemon_supervisor(&malformed), 2);
    }

    #[test]
    fn only_a_bare_launch_opens_the_gui() {
        assert!(is_bare_gui_launch(&["ferryx"]));
        assert!(is_bare_gui_launch(&["ferryx", "-psn_0_12345"]));
        assert!(!is_bare_gui_launch(&["ferryx", "agents", "--help"]));
        assert!(!is_bare_gui_launch(&["ferryx", "--version"]));
        assert!(!is_bare_gui_launch(&["ferryx", "--help"]));
        assert!(!is_bare_gui_launch(&["ferryx", "-psn_0_1", "agents"]));
    }

    #[test]
    fn browser_snapshot_cli_requires_browser_id() {
        let args = vec!["ferryx", "browser", "snapshot"];
        assert!(parse_browser_cli(&args).is_err());
    }

    #[test]
    fn browser_click_cli_preserves_generation_and_reference() {
        let args = vec![
            "ferryx",
            "browser",
            "click",
            "--browser-id",
            "browser-1",
            "--generation",
            "7",
            "--ref",
            "e3",
        ];

        assert_eq!(
            parse_browser_cli(&args).expect("parse browser click"),
            BrowserCliCommand::Click {
                browser_id: "browser-1".into(),
                generation: 7,
                reference: "e3".into(),
            }
        );
    }

    #[test]
    fn browser_fill_cli_preserves_generation_reference_and_value() {
        let args = vec![
            "ferryx",
            "browser",
            "fill",
            "--browser-id",
            "browser-1",
            "--generation",
            "3",
            "--ref",
            "e2",
            "--value",
            "hello world",
        ];

        assert_eq!(
            parse_browser_cli(&args).expect("parse browser fill"),
            BrowserCliCommand::Fill {
                browser_id: "browser-1".into(),
                generation: 3,
                reference: "e2".into(),
                value: "hello world".into(),
            }
        );
    }

    #[test]
    fn browser_keypress_cli_preserves_generation_and_key() {
        let args = vec![
            "ferryx",
            "browser",
            "keypress",
            "--browser-id",
            "browser-1",
            "--generation",
            "3",
            "--key",
            "Enter",
        ];

        assert_eq!(
            parse_browser_cli(&args).expect("parse browser keypress"),
            BrowserCliCommand::Keypress {
                browser_id: "browser-1".into(),
                generation: 3,
                key: "Enter".into(),
            }
        );

        let modifier_args = vec![
            "ferryx",
            "browser",
            "keypress",
            "--browser-id",
            "browser-1",
            "--generation",
            "3",
            "--key",
            "Meta+ArrowLeft",
        ];

        assert_eq!(
            parse_browser_cli(&modifier_args).expect("parse browser keypress with modifiers"),
            BrowserCliCommand::Keypress {
                browser_id: "browser-1".into(),
                generation: 3,
                key: "Meta+ArrowLeft".into(),
            }
        );
    }

    #[test]
    fn handover_from_rejects_a_missing_or_option_like_value() {
        // `--handover-from` with no value must be an ERROR, not a silent cold start.
        // A handover exists to transplant live PTY sessions from a retiring daemon;
        // starting fresh instead abandons every session the caller meant to preserve,
        // and the caller sees a daemon that looks healthy.
        assert!(
            parse_handover_from(&["--daemon".to_string(), "--handover-from".to_string()]).is_err()
        );
        // A following flag is not a path.
        assert!(
            parse_handover_from(&["--handover-from".to_string(), "--daemon".to_string()]).is_err()
        );
        // Absent entirely is a legitimate cold start.
        assert_eq!(
            parse_handover_from(&["--daemon".to_string()]).expect("absent is ok"),
            None
        );
        // A real value still parses.
        assert_eq!(
            parse_handover_from(&[
                "--handover-from".to_string(),
                "/tmp/ferryx-handover.json".to_string()
            ])
            .expect("value parses"),
            Some(std::path::PathBuf::from("/tmp/ferryx-handover.json"))
        );
    }

    static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn browser_open_cli_happy_path() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let prev_ws = std::env::var("FERRYX_WORKSPACE_ID").ok();
        let prev_wt = std::env::var("FERRYX_WORKTREE_PATH").ok();
        std::env::remove_var("FERRYX_WORKSPACE_ID");
        std::env::remove_var("FERRYX_WORKTREE_PATH");

        let args = vec!["ferryx", "browser", "open", "--url", "https://example.com"];
        let parsed = parse_browser_cli(&args);

        let full_args = vec![
            "ferryx",
            "browser",
            "open",
            "--url",
            "https://example.com",
            "--workspace",
            "ws-123",
            "--worktree-path",
            "/path/to/worktree",
        ];
        let parsed_full = parse_browser_cli(&full_args);

        if let Some(ws) = prev_ws {
            std::env::set_var("FERRYX_WORKSPACE_ID", ws);
        }
        if let Some(wt) = prev_wt {
            std::env::set_var("FERRYX_WORKTREE_PATH", wt);
        }

        assert_eq!(
            parsed.expect("parse browser open"),
            BrowserCliCommand::Open {
                url: "https://example.com".into(),
                workspace_id: None,
                worktree_path: None,
            }
        );

        assert_eq!(
            parsed_full.expect("parse browser open with optional flags"),
            BrowserCliCommand::Open {
                url: "https://example.com".into(),
                workspace_id: Some("ws-123".into()),
                worktree_path: Some("/path/to/worktree".into()),
            }
        );
    }

    #[test]
    fn browser_open_cli_missing_options() {
        let args_no_url = vec!["ferryx", "browser", "open"];
        assert!(parse_browser_cli(&args_no_url).is_err());

        let args_empty_url = vec!["ferryx", "browser", "open", "--url"];
        assert!(parse_browser_cli(&args_empty_url).is_err());

        let args_empty_ws = vec![
            "ferryx",
            "browser",
            "open",
            "--url",
            "https://example.com",
            "--workspace",
        ];
        assert!(parse_browser_cli(&args_empty_ws).is_err());

        let args_empty_wt = vec![
            "ferryx",
            "browser",
            "open",
            "--url",
            "https://example.com",
            "--worktree-path",
        ];
        assert!(parse_browser_cli(&args_empty_wt).is_err());
    }

    #[test]
    fn browser_navigate_cli_happy_and_missing() {
        let happy = vec![
            "ferryx",
            "browser",
            "navigate",
            "--browser-id",
            "b-1",
            "--url",
            "https://example.com",
        ];
        assert_eq!(
            parse_browser_cli(&happy).expect("parse browser navigate"),
            BrowserCliCommand::Navigate {
                browser_id: "b-1".into(),
                url: "https://example.com".into(),
            }
        );

        let missing_url = vec!["ferryx", "browser", "navigate", "--browser-id", "b-1"];
        assert!(parse_browser_cli(&missing_url).is_err());

        let missing_id = vec![
            "ferryx",
            "browser",
            "navigate",
            "--url",
            "https://example.com",
        ];
        assert!(parse_browser_cli(&missing_id).is_err());

        let happy_goto = vec![
            "ferryx",
            "browser",
            "goto",
            "--browser-id",
            "b-1",
            "--url",
            "https://example.com",
        ];
        assert_eq!(
            parse_browser_cli(&happy_goto).expect("parse browser goto"),
            BrowserCliCommand::Navigate {
                browser_id: "b-1".into(),
                url: "https://example.com".into(),
            }
        );

        let missing_goto_url = vec!["ferryx", "browser", "goto", "--browser-id", "b-1"];
        assert!(parse_browser_cli(&missing_goto_url).is_err());
    }

    #[test]
    fn browser_nav_history_and_reload_cli_parsing() {
        let happy_back = vec!["ferryx", "browser", "back", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&happy_back).expect("parse browser back"),
            BrowserCliCommand::Back {
                browser_id: "b-1".into(),
            }
        );
        assert!(parse_browser_cli(["ferryx", "browser", "back"]).is_err());

        let happy_forward = vec!["ferryx", "browser", "forward", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&happy_forward).expect("parse browser forward"),
            BrowserCliCommand::Forward {
                browser_id: "b-1".into(),
            }
        );
        assert!(parse_browser_cli(["ferryx", "browser", "forward"]).is_err());

        let happy_reload = vec!["ferryx", "browser", "reload", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&happy_reload).expect("parse browser reload"),
            BrowserCliCommand::Reload {
                browser_id: "b-1".into(),
            }
        );
        assert!(parse_browser_cli(["ferryx", "browser", "reload"]).is_err());
    }

    #[test]
    fn browser_close_cli_happy_and_missing() {
        let happy = vec!["ferryx", "browser", "close", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&happy).expect("parse browser close"),
            BrowserCliCommand::Close {
                browser_id: "b-1".into(),
            }
        );

        let missing_id = vec!["ferryx", "browser", "close"];
        assert!(parse_browser_cli(&missing_id).is_err());
    }

    #[test]
    fn browser_identify_cli_happy() {
        let happy = vec!["ferryx", "browser", "identify"];
        assert_eq!(
            parse_browser_cli(&happy).expect("parse browser identify"),
            BrowserCliCommand::Identify
        );
    }

    #[test]
    fn browser_url_cli_happy_and_missing() {
        let happy = vec!["ferryx", "browser", "url", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&happy).expect("parse browser url"),
            BrowserCliCommand::Url {
                browser_id: "b-1".into(),
            }
        );

        let missing_id = vec!["ferryx", "browser", "url"];
        assert!(parse_browser_cli(&missing_id).is_err());
    }

    #[test]
    fn browser_title_cli_happy_and_missing() {
        let happy = vec!["ferryx", "browser", "title", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&happy).expect("parse browser title"),
            BrowserCliCommand::Title {
                browser_id: "b-1".into(),
            }
        );

        let missing_id = vec!["ferryx", "browser", "title"];
        assert!(parse_browser_cli(&missing_id).is_err());
    }

    #[test]
    fn browser_cli_unknown_subcommand_and_usage() {
        let err = parse_browser_cli(["ferryx", "browser", "unknown"]).unwrap_err();
        assert_eq!(
            err,
            "expected `ferryx browser <list|open|navigate|goto|back|forward|reload|close|identify|url|title|snapshot|click|dblclick|hover|check|uncheck|scroll-into-view|select|type|scroll|fill|keypress|eval|wait|screenshot|highlight|console|errors|focus|cookies|storage|get|is|find|dialog|download|tab|state>`"
        );

        let err_no_browser = parse_browser_cli(["ferryx", "other"]).unwrap_err();
        assert_eq!(
            err_no_browser,
            "expected `ferryx browser <list|open|navigate|goto|back|forward|reload|close|identify|url|title|snapshot|click|dblclick|hover|check|uncheck|scroll-into-view|select|type|scroll|fill|keypress|eval|wait|screenshot|highlight|console|errors|focus|cookies|storage|get|is|find|dialog|download|tab|state>`"
        );
    }

    #[test]
    fn browser_cli_request_mappings_phase3() {
        assert_eq!(
            browser_cli_request(BrowserCliCommand::Eval {
                browser_id: "b-1".into(),
                script: "1+1".into(),
            }),
            BrowserCliRequest::Eval {
                browser_id: "b-1".into(),
                script: "1+1".into(),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Wait {
                browser_id: "b-1".into(),
                condition: BrowserWaitCondition::Selector {
                    selector: "#submit".into(),
                },
                timeout_ms: Some(5000),
            }),
            BrowserCliRequest::Wait {
                browser_id: "b-1".into(),
                condition: BrowserWaitCondition::WithTimeout {
                    inner: Box::new(BrowserWaitCondition::Selector {
                        selector: "#submit".into(),
                    }),
                    timeout_ms: 5000,
                },
            }
        );

        // Without `--timeout-ms` the condition must reach the server untouched, so
        // the server's own default timeout stays in charge.
        assert_eq!(
            browser_cli_request(BrowserCliCommand::Wait {
                browser_id: "b-1".into(),
                condition: BrowserWaitCondition::Selector {
                    selector: "#submit".into(),
                },
                timeout_ms: None,
            }),
            BrowserCliRequest::Wait {
                browser_id: "b-1".into(),
                condition: BrowserWaitCondition::Selector {
                    selector: "#submit".into(),
                },
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Screenshot {
                browser_id: "b-1".into(),
                out_path: "/tmp/shot.png".into(),
            }),
            BrowserCliRequest::Screenshot {
                browser_id: "b-1".into(),
                out_path: "/tmp/shot.png".into(),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Console {
                browser_id: "b-1".into(),
                errors_only: true,
                clear: true,
            }),
            BrowserCliRequest::Console {
                browser_id: "b-1".into(),
                errors_only: Some(true),
                clear: Some(true),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Errors {
                browser_id: "b-1".into(),
                clear: true,
            }),
            BrowserCliRequest::Console {
                browser_id: "b-1".into(),
                errors_only: Some(true),
                clear: Some(true),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Focus {
                browser_id: "b-1".into(),
                generation: None,
                reference: None,
            }),
            BrowserCliRequest::Focus {
                browser_id: "b-1".into(),
            }
        );

        // `focus --generation G --ref R` is the element form (row 10) and must not
        // reuse the webview focus request.
        assert_eq!(
            browser_cli_request(BrowserCliCommand::Focus {
                browser_id: "b-1".into(),
                generation: Some(5),
                reference: Some("e7".into()),
            }),
            BrowserCliRequest::Act {
                request: crate::browser::BrowserAutomationRequest {
                    browser_id: "b-1".into(),
                    generation: 5,
                    action: BrowserAutomationAction::Focus {
                        reference: "e7".into(),
                    },
                },
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Cookies {
                browser_id: "b-1".into(),
                action: CookieCliAction::Get,
            }),
            BrowserCliRequest::Cookies {
                browser_id: "b-1".into(),
                action: "get".into(),
                name: None,
                value: None,
                domain: None,
                path: None,
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Cookies {
                browser_id: "b-1".into(),
                action: CookieCliAction::Set {
                    name: "session".into(),
                    value: "xyz".into(),
                    domain: Some("example.com".into()),
                    path: Some("/".into()),
                },
            }),
            BrowserCliRequest::Cookies {
                browser_id: "b-1".into(),
                action: "set".into(),
                name: Some("session".into()),
                value: Some("xyz".into()),
                domain: Some("example.com".into()),
                path: Some("/".into()),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Cookies {
                browser_id: "b-1".into(),
                action: CookieCliAction::Clear {
                    name: "session".into(),
                },
            }),
            BrowserCliRequest::Cookies {
                browser_id: "b-1".into(),
                action: "clear".into(),
                name: Some("session".into()),
                value: None,
                domain: None,
                path: None,
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Storage {
                browser_id: "b-1".into(),
                kind: StorageCliKind::Local,
                action: StorageCliAction::Get {
                    key: Some("token".into()),
                },
            }),
            BrowserCliRequest::Storage {
                browser_id: "b-1".into(),
                kind: "local".into(),
                action: "get".into(),
                key: Some("token".into()),
                value: None,
                clear_all: None,
                url: None,
                domain: None,
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Storage {
                browser_id: "b-1".into(),
                kind: StorageCliKind::Local,
                action: StorageCliAction::Set {
                    key: "theme".into(),
                    value: "dark".into(),
                },
            }),
            BrowserCliRequest::Storage {
                browser_id: "b-1".into(),
                kind: "local".into(),
                action: "set".into(),
                key: Some("theme".into()),
                value: Some("dark".into()),
                clear_all: None,
                url: None,
                domain: None,
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Storage {
                browser_id: "b-1".into(),
                kind: StorageCliKind::Session,
                action: StorageCliAction::Clear {
                    key: Some("token".into()),
                    all: false,
                    url: None,
                    domain: None,
                },
            }),
            BrowserCliRequest::Storage {
                browser_id: "b-1".into(),
                kind: "session".into(),
                action: "clear".into(),
                key: Some("token".into()),
                value: None,
                clear_all: Some(false),
                url: None,
                domain: None,
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Storage {
                browser_id: "b-1".into(),
                kind: StorageCliKind::Session,
                action: StorageCliAction::Clear {
                    key: None,
                    all: true,
                    url: None,
                    domain: None,
                },
            }),
            BrowserCliRequest::Storage {
                browser_id: "b-1".into(),
                kind: "session".into(),
                action: "clear".into(),
                key: None,
                value: None,
                clear_all: Some(true),
                url: None,
                domain: None,
            }
        );
    }

    #[test]
    fn browser_cli_request_mappings() {
        assert_eq!(
            browser_cli_request(BrowserCliCommand::List),
            BrowserCliRequest::List
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Open {
                url: "https://example.com".into(),
                workspace_id: Some("ws-1".into()),
                worktree_path: Some("/tree".into()),
            }),
            BrowserCliRequest::Open {
                url: "https://example.com".into(),
                workspace_id: Some("ws-1".into()),
                worktree_path: Some("/tree".into()),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Navigate {
                browser_id: "b-1".into(),
                url: "https://example.com".into(),
            }),
            BrowserCliRequest::Navigate {
                browser_id: "b-1".into(),
                url: "https://example.com".into(),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Back {
                browser_id: "b-1".into(),
            }),
            BrowserCliRequest::Back {
                browser_id: "b-1".into(),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Forward {
                browser_id: "b-1".into(),
            }),
            BrowserCliRequest::Forward {
                browser_id: "b-1".into(),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Reload {
                browser_id: "b-1".into(),
            }),
            BrowserCliRequest::Reload {
                browser_id: "b-1".into(),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Close {
                browser_id: "b-1".into(),
            }),
            BrowserCliRequest::Close {
                browser_id: "b-1".into(),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Identify),
            BrowserCliRequest::Identify
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Url {
                browser_id: "b-1".into(),
            }),
            BrowserCliRequest::Snapshot {
                browser_id: "b-1".into(),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Title {
                browser_id: "b-1".into(),
            }),
            BrowserCliRequest::Snapshot {
                browser_id: "b-1".into(),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Snapshot {
                browser_id: "b-1".into(),
            }),
            BrowserCliRequest::Snapshot {
                browser_id: "b-1".into(),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Click {
                browser_id: "b-1".into(),
                generation: 2,
                reference: "e-btn".into(),
            }),
            BrowserCliRequest::Act {
                request: crate::browser::BrowserAutomationRequest {
                    browser_id: "b-1".into(),
                    generation: 2,
                    action: BrowserAutomationAction::Click {
                        reference: "e-btn".into(),
                    },
                },
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Fill {
                browser_id: "b-1".into(),
                generation: 3,
                reference: "e-input".into(),
                value: "typed text".into(),
            }),
            BrowserCliRequest::Act {
                request: crate::browser::BrowserAutomationRequest {
                    browser_id: "b-1".into(),
                    generation: 3,
                    action: BrowserAutomationAction::Fill {
                        reference: "e-input".into(),
                        value: "typed text".into(),
                    },
                },
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Keypress {
                browser_id: "b-1".into(),
                generation: 4,
                key: "Enter".into(),
            }),
            BrowserCliRequest::Act {
                request: crate::browser::BrowserAutomationRequest {
                    browser_id: "b-1".into(),
                    generation: 4,
                    action: BrowserAutomationAction::Keypress {
                        key: "Enter".into(),
                    },
                },
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Title,
            }),
            BrowserCliRequest::Eval {
                browser_id: "b-1".into(),
                script: build_dom_get_script(&BrowserGetSubselector::Title),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Url,
            }),
            BrowserCliRequest::Eval {
                browser_id: "b-1".into(),
                script: build_dom_get_script(&BrowserGetSubselector::Url),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Text {
                    selector: "#heading".into(),
                },
            }),
            BrowserCliRequest::Eval {
                browser_id: "b-1".into(),
                script: build_dom_get_script(&BrowserGetSubselector::Text {
                    selector: "#heading".into(),
                }),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Html {
                    selector: "div.card".into(),
                },
            }),
            BrowserCliRequest::Eval {
                browser_id: "b-1".into(),
                script: build_dom_get_script(&BrowserGetSubselector::Html {
                    selector: "div.card".into(),
                }),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Value {
                    selector: "input#name".into(),
                },
            }),
            BrowserCliRequest::Eval {
                browser_id: "b-1".into(),
                script: build_dom_get_script(&BrowserGetSubselector::Value {
                    selector: "input#name".into(),
                }),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Attr {
                    selector: "a#link".into(),
                    attribute: "href".into(),
                },
            }),
            BrowserCliRequest::Eval {
                browser_id: "b-1".into(),
                script: build_dom_get_script(&BrowserGetSubselector::Attr {
                    selector: "a#link".into(),
                    attribute: "href".into(),
                }),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Count {
                    selector: "li.item".into(),
                },
            }),
            BrowserCliRequest::Eval {
                browser_id: "b-1".into(),
                script: build_dom_get_script(&BrowserGetSubselector::Count {
                    selector: "li.item".into(),
                }),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Box {
                    selector: "button#submit".into(),
                },
            }),
            BrowserCliRequest::Eval {
                browser_id: "b-1".into(),
                script: build_dom_get_script(&BrowserGetSubselector::Box {
                    selector: "button#submit".into(),
                }),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Styles {
                    selector: "body".into(),
                },
            }),
            BrowserCliRequest::Eval {
                browser_id: "b-1".into(),
                script: build_dom_get_script(&BrowserGetSubselector::Styles {
                    selector: "body".into(),
                }),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Is {
                browser_id: "b-1".into(),
                subselector: BrowserIsSubselector::Visible {
                    selector: "#modal".into(),
                },
            }),
            BrowserCliRequest::Eval {
                browser_id: "b-1".into(),
                script: build_dom_is_script(&BrowserIsSubselector::Visible {
                    selector: "#modal".into(),
                }),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Is {
                browser_id: "b-1".into(),
                subselector: BrowserIsSubselector::Enabled {
                    selector: "#btn".into(),
                },
            }),
            BrowserCliRequest::Eval {
                browser_id: "b-1".into(),
                script: build_dom_is_script(&BrowserIsSubselector::Enabled {
                    selector: "#btn".into(),
                }),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Is {
                browser_id: "b-1".into(),
                subselector: BrowserIsSubselector::Checked {
                    selector: "input#accept".into(),
                },
            }),
            BrowserCliRequest::Eval {
                browser_id: "b-1".into(),
                script: build_dom_is_script(&BrowserIsSubselector::Checked {
                    selector: "input#accept".into(),
                }),
            }
        );

        for subselector in [
            BrowserFindSubselector::Role {
                role: "button".into(),
            },
            BrowserFindSubselector::Text {
                text: "Sign in".into(),
            },
            BrowserFindSubselector::Label {
                label: "Username".into(),
            },
            BrowserFindSubselector::Placeholder {
                text: "Search...".into(),
            },
            BrowserFindSubselector::Alt {
                alt: "Logo".into(),
            },
            BrowserFindSubselector::Title {
                title: "Close".into(),
            },
            BrowserFindSubselector::TestId {
                testid: "submit-btn".into(),
            },
            BrowserFindSubselector::First {
                selector: "button".into(),
            },
            BrowserFindSubselector::Last {
                selector: "button".into(),
            },
            BrowserFindSubselector::Nth {
                selector: "button".into(),
                index: 2,
            },
        ] {
            assert_eq!(
                browser_cli_request(BrowserCliCommand::Find {
                    browser_id: "b-1".into(),
                    subselector: subselector.clone(),
                }),
                BrowserCliRequest::Eval {
                    browser_id: "b-1".into(),
                    script: build_dom_find_script(&subselector),
                }
            );
        }
    }

    #[test]
    fn browser_formatter_prints_only_url_or_title_from_snapshot() {
        let snapshot = crate::browser::BrowserAutomationSnapshot {
            browser_id: "b-1".into(),
            generation: 1,
            url: "https://example.com/page".into(),
            title: "Example Title".into(),
            elements: vec![],
        };
        let response = BrowserCliResponse::Snapshot {
            snapshot: snapshot.clone(),
        };

        let url_output = format_browser_cli_response(
            &BrowserCliCommand::Url {
                browser_id: "b-1".into(),
            },
            response.clone(),
        )
        .expect("format url");
        assert_eq!(url_output, "https://example.com/page");

        let title_output = format_browser_cli_response(
            &BrowserCliCommand::Title {
                browser_id: "b-1".into(),
            },
            response.clone(),
        )
        .expect("format title");
        assert_eq!(title_output, "Example Title");

        let snapshot_output = format_browser_cli_response(
            &BrowserCliCommand::Snapshot {
                browser_id: "b-1".into(),
            },
            response,
        )
        .expect("format snapshot");
        assert_eq!(snapshot_output, serde_json::to_string(&snapshot).unwrap());
    }

    #[test]
    fn browser_eval_cli_parsing() {
        let args = vec![
            "ferryx",
            "browser",
            "eval",
            "--browser-id",
            "b-1",
            "--script",
            "document.title",
        ];
        assert_eq!(
            parse_browser_cli(&args).expect("parse eval"),
            BrowserCliCommand::Eval {
                browser_id: "b-1".into(),
                script: "document.title".into(),
            }
        );
    }

    #[test]
    fn browser_wait_cli_parsing() {
        let args_sel = vec![
            "ferryx",
            "browser",
            "wait",
            "--browser-id",
            "b-1",
            "--selector",
            "#submit",
            "--timeout-ms",
            "5000",
        ];
        assert_eq!(
            parse_browser_cli(&args_sel).expect("parse wait selector"),
            BrowserCliCommand::Wait {
                browser_id: "b-1".into(),
                condition: BrowserWaitCondition::Selector {
                    selector: "#submit".into(),
                },
                timeout_ms: Some(5000),
            }
        );

        let args_text = vec![
            "ferryx",
            "browser",
            "wait",
            "--browser-id",
            "b-1",
            "--text",
            "Welcome",
        ];
        assert_eq!(
            parse_browser_cli(&args_text).expect("parse wait text"),
            BrowserCliCommand::Wait {
                browser_id: "b-1".into(),
                condition: BrowserWaitCondition::Text {
                    text: "Welcome".into(),
                },
                timeout_ms: None,
            }
        );

        let args_url = vec![
            "ferryx",
            "browser",
            "wait",
            "--browser-id",
            "b-1",
            "--url-contains",
            "/dashboard",
        ];
        assert_eq!(
            parse_browser_cli(&args_url).expect("parse wait url-contains"),
            BrowserCliCommand::Wait {
                browser_id: "b-1".into(),
                condition: BrowserWaitCondition::UrlContains {
                    fragment: "/dashboard".into(),
                },
                timeout_ms: None,
            }
        );

        let args_load = vec![
            "ferryx",
            "browser",
            "wait",
            "--browser-id",
            "b-1",
            "--load-state",
            "complete",
        ];
        assert_eq!(
            parse_browser_cli(&args_load).expect("parse wait load-state"),
            BrowserCliCommand::Wait {
                browser_id: "b-1".into(),
                condition: BrowserWaitCondition::LoadState {
                    state: "complete".into(),
                },
                timeout_ms: None,
            }
        );

        let args_func = vec![
            "ferryx",
            "browser",
            "wait",
            "--browser-id",
            "b-1",
            "--function",
            "window.ready === true",
        ];
        assert_eq!(
            parse_browser_cli(&args_func).expect("parse wait function"),
            BrowserCliCommand::Wait {
                browser_id: "b-1".into(),
                condition: BrowserWaitCondition::Function {
                    script: "window.ready === true".into(),
                },
                timeout_ms: None,
            }
        );
    }

    #[test]
    fn browser_screenshot_cli_parsing() {
        let args = vec![
            "ferryx",
            "browser",
            "screenshot",
            "--browser-id",
            "b-1",
            "--out",
            "/tmp/shot.png",
        ];
        assert_eq!(
            parse_browser_cli(&args).expect("parse screenshot"),
            BrowserCliCommand::Screenshot {
                browser_id: "b-1".into(),
                out_path: "/tmp/shot.png".into(),
            }
        );
    }

    #[test]
    fn browser_console_and_errors_cli_parsing() {
        let args_console = vec![
            "ferryx",
            "browser",
            "console",
            "--browser-id",
            "b-1",
            "--errors",
            "--clear",
        ];
        assert_eq!(
            parse_browser_cli(&args_console).expect("parse console"),
            BrowserCliCommand::Console {
                browser_id: "b-1".into(),
                errors_only: true,
                clear: true,
            }
        );

        let args_errors = vec![
            "ferryx",
            "browser",
            "errors",
            "--browser-id",
            "b-1",
            "--clear",
        ];
        assert_eq!(
            parse_browser_cli(&args_errors).expect("parse errors"),
            BrowserCliCommand::Errors {
                browser_id: "b-1".into(),
                clear: true,
            }
        );
    }

    #[test]
    fn browser_focus_cli_parsing() {
        let args = vec!["ferryx", "browser", "focus", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&args).expect("parse focus"),
            BrowserCliCommand::Focus {
                browser_id: "b-1".into(),
                generation: None,
                reference: None,
            }
        );

        let element_args = vec![
            "ferryx",
            "browser",
            "focus",
            "--browser-id",
            "b-1",
            "--generation",
            "4",
            "--ref",
            "e2",
        ];
        assert_eq!(
            parse_browser_cli(&element_args).expect("parse element focus"),
            BrowserCliCommand::Focus {
                browser_id: "b-1".into(),
                generation: Some(4),
                reference: Some("e2".into()),
            }
        );

        // Adversarial: a half-specified element focus must not silently degrade to
        // focusing the webview instead of the element the caller named.
        let ref_only = vec![
            "ferryx",
            "browser",
            "focus",
            "--browser-id",
            "b-1",
            "--ref",
            "e2",
        ];
        assert!(parse_browser_cli(&ref_only).is_err());

        let generation_only = vec![
            "ferryx",
            "browser",
            "focus",
            "--browser-id",
            "b-1",
            "--generation",
            "4",
        ];
        assert!(parse_browser_cli(&generation_only).is_err());

        let bad_generation = vec![
            "ferryx",
            "browser",
            "focus",
            "--browser-id",
            "b-1",
            "--generation",
            "nope",
            "--ref",
            "e2",
        ];
        assert_eq!(
            parse_browser_cli(&bad_generation).unwrap_err(),
            "--generation must be an unsigned integer"
        );
    }

    #[test]
    fn browser_cookies_cli_parsing() {
        let args_get = vec!["ferryx", "browser", "cookies", "--browser-id", "b-1", "get"];
        assert_eq!(
            parse_browser_cli(&args_get).expect("parse cookies get"),
            BrowserCliCommand::Cookies {
                browser_id: "b-1".into(),
                action: CookieCliAction::Get,
            }
        );

        let args_set = vec![
            "ferryx",
            "browser",
            "cookies",
            "--browser-id",
            "b-1",
            "set",
            "session",
            "xyz",
            "--domain",
            "example.com",
            "--path",
            "/",
        ];
        assert_eq!(
            parse_browser_cli(&args_set).expect("parse cookies set"),
            BrowserCliCommand::Cookies {
                browser_id: "b-1".into(),
                action: CookieCliAction::Set {
                    name: "session".into(),
                    value: "xyz".into(),
                    domain: Some("example.com".into()),
                    path: Some("/".into()),
                },
            }
        );

        let args_clear = vec![
            "ferryx",
            "browser",
            "cookies",
            "--browser-id",
            "b-1",
            "clear",
            "session",
        ];
        assert_eq!(
            parse_browser_cli(&args_clear).expect("parse cookies clear"),
            BrowserCliCommand::Cookies {
                browser_id: "b-1".into(),
                action: CookieCliAction::Clear {
                    name: "session".into(),
                },
            }
        );
    }

    #[test]
    fn browser_storage_cli_parsing() {
        let args_get = vec![
            "ferryx",
            "browser",
            "storage",
            "--browser-id",
            "b-1",
            "local",
            "get",
            "token",
        ];
        assert_eq!(
            parse_browser_cli(&args_get).expect("parse storage local get"),
            BrowserCliCommand::Storage {
                browser_id: "b-1".into(),
                kind: StorageCliKind::Local,
                action: StorageCliAction::Get {
                    key: Some("token".into()),
                },
            }
        );

        let args_set = vec![
            "ferryx",
            "browser",
            "storage",
            "--browser-id",
            "b-1",
            "session",
            "set",
            "user",
            "alice",
        ];
        assert_eq!(
            parse_browser_cli(&args_set).expect("parse storage session set"),
            BrowserCliCommand::Storage {
                browser_id: "b-1".into(),
                kind: StorageCliKind::Session,
                action: StorageCliAction::Set {
                    key: "user".into(),
                    value: "alice".into(),
                },
            }
        );

        let args_clear = vec![
            "ferryx",
            "browser",
            "storage",
            "--browser-id",
            "b-1",
            "local",
            "clear",
        ];
        assert_eq!(
            parse_browser_cli(&args_clear).expect("parse storage local clear"),
            BrowserCliCommand::Storage {
                browser_id: "b-1".into(),
                kind: StorageCliKind::Local,
                action: StorageCliAction::Clear {
                    key: None,
                    all: false,
                    url: None,
                    domain: None,
                },
            }
        );
    }

    #[test]
    fn browser_get_cli_parsing() {
        let happy_title = vec!["ferryx", "browser", "get", "title", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&happy_title).expect("parse get title"),
            BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Title,
            }
        );

        let happy_url = vec!["ferryx", "browser", "get", "url", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&happy_url).expect("parse get url"),
            BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Url,
            }
        );

        let happy_text_pos = vec![
            "ferryx",
            "browser",
            "get",
            "text",
            "#heading",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_text_pos).expect("parse get text positional"),
            BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Text {
                    selector: "#heading".into(),
                },
            }
        );

        let happy_text_flag = vec![
            "ferryx",
            "browser",
            "get",
            "text",
            "--browser-id",
            "b-1",
            "--selector",
            "#heading",
        ];
        assert_eq!(
            parse_browser_cli(&happy_text_flag).expect("parse get text flag"),
            BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Text {
                    selector: "#heading".into(),
                },
            }
        );

        let happy_html = vec![
            "ferryx",
            "browser",
            "get",
            "html",
            "div.content",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_html).expect("parse get html"),
            BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Html {
                    selector: "div.content".into(),
                },
            }
        );

        let happy_value = vec![
            "ferryx",
            "browser",
            "get",
            "value",
            "input[name=email]",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_value).expect("parse get value"),
            BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Value {
                    selector: "input[name=email]".into(),
                },
            }
        );

        let happy_attr_pos = vec![
            "ferryx",
            "browser",
            "get",
            "attr",
            "a.link",
            "href",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_attr_pos).expect("parse get attr positional"),
            BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Attr {
                    selector: "a.link".into(),
                    attribute: "href".into(),
                },
            }
        );

        let happy_attr_flag = vec![
            "ferryx",
            "browser",
            "get",
            "attr",
            "--browser-id",
            "b-1",
            "--selector",
            "a.link",
            "--attr",
            "href",
        ];
        assert_eq!(
            parse_browser_cli(&happy_attr_flag).expect("parse get attr flag"),
            BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Attr {
                    selector: "a.link".into(),
                    attribute: "href".into(),
                },
            }
        );

        let happy_count = vec![
            "ferryx",
            "browser",
            "get",
            "count",
            "li.item",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_count).expect("parse get count"),
            BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Count {
                    selector: "li.item".into(),
                },
            }
        );

        let happy_box = vec![
            "ferryx",
            "browser",
            "get",
            "box",
            "#btn",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_box).expect("parse get box"),
            BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Box {
                    selector: "#btn".into(),
                },
            }
        );

        let happy_styles = vec![
            "ferryx",
            "browser",
            "get",
            "styles",
            "body",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_styles).expect("parse get styles"),
            BrowserCliCommand::Get {
                browser_id: "b-1".into(),
                subselector: BrowserGetSubselector::Styles {
                    selector: "body".into(),
                },
            }
        );
    }

    #[test]
    fn browser_is_cli_parsing() {
        let happy_visible_pos = vec![
            "ferryx",
            "browser",
            "is",
            "visible",
            "#modal",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_visible_pos).expect("parse is visible positional"),
            BrowserCliCommand::Is {
                browser_id: "b-1".into(),
                subselector: BrowserIsSubselector::Visible {
                    selector: "#modal".into(),
                },
            }
        );

        let happy_visible_flag = vec![
            "ferryx",
            "browser",
            "is",
            "visible",
            "--browser-id",
            "b-1",
            "--selector",
            "#modal",
        ];
        assert_eq!(
            parse_browser_cli(&happy_visible_flag).expect("parse is visible flag"),
            BrowserCliCommand::Is {
                browser_id: "b-1".into(),
                subselector: BrowserIsSubselector::Visible {
                    selector: "#modal".into(),
                },
            }
        );

        let happy_enabled = vec![
            "ferryx",
            "browser",
            "is",
            "enabled",
            "#submit-btn",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_enabled).expect("parse is enabled"),
            BrowserCliCommand::Is {
                browser_id: "b-1".into(),
                subselector: BrowserIsSubselector::Enabled {
                    selector: "#submit-btn".into(),
                },
            }
        );

        let happy_checked = vec![
            "ferryx",
            "browser",
            "is",
            "checked",
            "input#agree",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_checked).expect("parse is checked"),
            BrowserCliCommand::Is {
                browser_id: "b-1".into(),
                subselector: BrowserIsSubselector::Checked {
                    selector: "input#agree".into(),
                },
            }
        );
    }

    #[test]
    fn browser_get_and_is_cli_error_handling() {
        // Unknown sub-selector returns a clean error, not a panic.
        let unknown_get = vec!["ferryx", "browser", "get", "bogus", "--browser-id", "b-1"];
        let err_get = parse_browser_cli(&unknown_get).unwrap_err();
        assert_eq!(
            err_get,
            "expected get <title|url|text|html|value|attr|count|box|styles>"
        );

        let unknown_is = vec!["ferryx", "browser", "is", "bogus", "--browser-id", "b-1"];
        let err_is = parse_browser_cli(&unknown_is).unwrap_err();
        assert_eq!(err_is, "expected is <visible|enabled|checked>");

        // Missing sub-selector
        let empty_get = vec!["ferryx", "browser", "get", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&empty_get).unwrap_err(),
            "expected get <title|url|text|html|value|attr|count|box|styles>"
        );

        let empty_is = vec!["ferryx", "browser", "is", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&empty_is).unwrap_err(),
            "expected is <visible|enabled|checked>"
        );

        // Missing browser-id
        let no_id_get = vec!["ferryx", "browser", "get", "title"];
        assert!(parse_browser_cli(&no_id_get).is_err());

        let no_id_is = vec!["ferryx", "browser", "is", "visible", "#modal"];
        assert!(parse_browser_cli(&no_id_is).is_err());

        // Missing selector for sub-selectors that require one
        let no_sel_text = vec!["ferryx", "browser", "get", "text", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&no_sel_text).unwrap_err(),
            "missing selector for get text"
        );

        let no_sel_is = vec!["ferryx", "browser", "is", "visible", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&no_sel_is).unwrap_err(),
            "missing selector for is visible"
        );

        // Missing attribute for get attr
        let no_attr = vec![
            "ferryx",
            "browser",
            "get",
            "attr",
            "a.link",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&no_attr).unwrap_err(),
            "missing attribute name for get attr"
        );
    }

    #[test]
    fn browser_find_cli_parsing() {
        let happy_role_pos = vec![
            "ferryx",
            "browser",
            "find",
            "role",
            "button",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_role_pos).expect("parse find role positional"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::Role {
                    role: "button".into(),
                },
            }
        );

        let happy_role_flag = vec![
            "ferryx",
            "browser",
            "find",
            "role",
            "--browser-id",
            "b-1",
            "--role",
            "button",
        ];
        assert_eq!(
            parse_browser_cli(&happy_role_flag).expect("parse find role flag"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::Role {
                    role: "button".into(),
                },
            }
        );

        let happy_text_pos = vec![
            "ferryx",
            "browser",
            "find",
            "text",
            "Sign in",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_text_pos).expect("parse find text positional"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::Text {
                    text: "Sign in".into(),
                },
            }
        );

        let happy_text_flag = vec![
            "ferryx",
            "browser",
            "find",
            "text",
            "--browser-id",
            "b-1",
            "--text",
            "Sign in",
        ];
        assert_eq!(
            parse_browser_cli(&happy_text_flag).expect("parse find text flag"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::Text {
                    text: "Sign in".into(),
                },
            }
        );

        let happy_label_pos = vec![
            "ferryx",
            "browser",
            "find",
            "label",
            "Username",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_label_pos).expect("parse find label positional"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::Label {
                    label: "Username".into(),
                },
            }
        );

        let happy_label_flag = vec![
            "ferryx",
            "browser",
            "find",
            "label",
            "--browser-id",
            "b-1",
            "--label",
            "Username",
        ];
        assert_eq!(
            parse_browser_cli(&happy_label_flag).expect("parse find label flag"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::Label {
                    label: "Username".into(),
                },
            }
        );

        let happy_placeholder_pos = vec![
            "ferryx",
            "browser",
            "find",
            "placeholder",
            "Search...",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_placeholder_pos).expect("parse find placeholder positional"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::Placeholder {
                    text: "Search...".into(),
                },
            }
        );

        let happy_placeholder_flag = vec![
            "ferryx",
            "browser",
            "find",
            "placeholder",
            "--browser-id",
            "b-1",
            "--placeholder",
            "Search...",
        ];
        assert_eq!(
            parse_browser_cli(&happy_placeholder_flag).expect("parse find placeholder flag"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::Placeholder {
                    text: "Search...".into(),
                },
            }
        );

        let happy_alt_pos = vec![
            "ferryx",
            "browser",
            "find",
            "alt",
            "Logo",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_alt_pos).expect("parse find alt positional"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::Alt {
                    alt: "Logo".into(),
                },
            }
        );

        let happy_alt_flag = vec![
            "ferryx",
            "browser",
            "find",
            "alt",
            "--browser-id",
            "b-1",
            "--alt",
            "Logo",
        ];
        assert_eq!(
            parse_browser_cli(&happy_alt_flag).expect("parse find alt flag"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::Alt {
                    alt: "Logo".into(),
                },
            }
        );

        let happy_title_pos = vec![
            "ferryx",
            "browser",
            "find",
            "title",
            "Close",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_title_pos).expect("parse find title positional"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::Title {
                    title: "Close".into(),
                },
            }
        );

        let happy_title_flag = vec![
            "ferryx",
            "browser",
            "find",
            "title",
            "--browser-id",
            "b-1",
            "--title",
            "Close",
        ];
        assert_eq!(
            parse_browser_cli(&happy_title_flag).expect("parse find title flag"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::Title {
                    title: "Close".into(),
                },
            }
        );

        let happy_testid_pos = vec![
            "ferryx",
            "browser",
            "find",
            "testid",
            "submit-btn",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_testid_pos).expect("parse find testid positional"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::TestId {
                    testid: "submit-btn".into(),
                },
            }
        );

        let happy_testid_flag = vec![
            "ferryx",
            "browser",
            "find",
            "testid",
            "--browser-id",
            "b-1",
            "--testid",
            "submit-btn",
        ];
        assert_eq!(
            parse_browser_cli(&happy_testid_flag).expect("parse find testid flag"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::TestId {
                    testid: "submit-btn".into(),
                },
            }
        );

        let happy_first_pos = vec![
            "ferryx",
            "browser",
            "find",
            "first",
            "button.submit",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_first_pos).expect("parse find first positional"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::First {
                    selector: "button.submit".into(),
                },
            }
        );

        let happy_first_flag = vec![
            "ferryx",
            "browser",
            "find",
            "first",
            "--browser-id",
            "b-1",
            "--selector",
            "button.submit",
        ];
        assert_eq!(
            parse_browser_cli(&happy_first_flag).expect("parse find first flag"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::First {
                    selector: "button.submit".into(),
                },
            }
        );

        let happy_last_pos = vec![
            "ferryx",
            "browser",
            "find",
            "last",
            "div.card",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_last_pos).expect("parse find last positional"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::Last {
                    selector: "div.card".into(),
                },
            }
        );

        let happy_last_flag = vec![
            "ferryx",
            "browser",
            "find",
            "last",
            "--browser-id",
            "b-1",
            "--selector",
            "div.card",
        ];
        assert_eq!(
            parse_browser_cli(&happy_last_flag).expect("parse find last flag"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::Last {
                    selector: "div.card".into(),
                },
            }
        );

        let happy_nth_pos = vec![
            "ferryx",
            "browser",
            "find",
            "nth",
            "li.item",
            "3",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&happy_nth_pos).expect("parse find nth positional"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::Nth {
                    selector: "li.item".into(),
                    index: 3,
                },
            }
        );

        let happy_nth_flag = vec![
            "ferryx",
            "browser",
            "find",
            "nth",
            "--browser-id",
            "b-1",
            "--selector",
            "li.item",
            "--index",
            "3",
        ];
        assert_eq!(
            parse_browser_cli(&happy_nth_flag).expect("parse find nth flag"),
            BrowserCliCommand::Find {
                browser_id: "b-1".into(),
                subselector: BrowserFindSubselector::Nth {
                    selector: "li.item".into(),
                    index: 3,
                },
            }
        );
    }

    #[test]
    fn browser_find_cli_error_handling() {
        // Unknown sub-selector returns a clean error, not a panic.
        let unknown_find = vec!["ferryx", "browser", "find", "bogus", "--browser-id", "b-1"];
        let err_find = parse_browser_cli(&unknown_find).unwrap_err();
        assert_eq!(
            err_find,
            "expected find <role|text|label|placeholder|alt|title|testid|first|last|nth>"
        );

        // Missing sub-selector
        let empty_find = vec!["ferryx", "browser", "find", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&empty_find).unwrap_err(),
            "expected find <role|text|label|placeholder|alt|title|testid|first|last|nth>"
        );

        // Missing browser-id
        let no_id_find = vec!["ferryx", "browser", "find", "text", "Sign in"];
        assert!(parse_browser_cli(&no_id_find).is_err());

        // Missing argument for sub-selectors that require one
        let no_role = vec!["ferryx", "browser", "find", "role", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&no_role).unwrap_err(),
            "missing role for find role"
        );

        let no_text = vec!["ferryx", "browser", "find", "text", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&no_text).unwrap_err(),
            "missing text for find text"
        );

        let no_label = vec!["ferryx", "browser", "find", "label", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&no_label).unwrap_err(),
            "missing label for find label"
        );

        let no_ph = vec![
            "ferryx",
            "browser",
            "find",
            "placeholder",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&no_ph).unwrap_err(),
            "missing placeholder for find placeholder"
        );

        let no_alt = vec!["ferryx", "browser", "find", "alt", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&no_alt).unwrap_err(),
            "missing alt for find alt"
        );

        let no_title = vec!["ferryx", "browser", "find", "title", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&no_title).unwrap_err(),
            "missing title for find title"
        );

        let no_testid = vec![
            "ferryx",
            "browser",
            "find",
            "testid",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&no_testid).unwrap_err(),
            "missing testid for find testid"
        );

        let no_sel_first = vec!["ferryx", "browser", "find", "first", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&no_sel_first).unwrap_err(),
            "missing selector for find first"
        );

        let no_sel_last = vec!["ferryx", "browser", "find", "last", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&no_sel_last).unwrap_err(),
            "missing selector for find last"
        );

        let no_sel_nth = vec!["ferryx", "browser", "find", "nth", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&no_sel_nth).unwrap_err(),
            "missing selector for find nth"
        );

        // Missing index for nth
        let no_idx_nth = vec![
            "ferryx",
            "browser",
            "find",
            "nth",
            "button",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&no_idx_nth).unwrap_err(),
            "missing index for find nth"
        );

        // Non-numeric index for nth
        let bad_idx_nth = vec![
            "ferryx",
            "browser",
            "find",
            "nth",
            "button",
            "not-a-number",
            "--browser-id",
            "b-1",
        ];
        assert_eq!(
            parse_browser_cli(&bad_idx_nth).unwrap_err(),
            "index for find nth must be a non-negative integer"
        );
    }

    #[test]
    fn browser_find_script_builder_deterministic() {
        for (subselector, expected_token) in [
            (
                BrowserFindSubselector::Role {
                    role: "button".into(),
                },
                "\"button\"",
            ),
            (
                BrowserFindSubselector::Text {
                    text: "Sign in".into(),
                },
                "\"Sign in\"",
            ),
            (
                BrowserFindSubselector::Label {
                    label: "Username".into(),
                },
                "\"Username\"",
            ),
            (
                BrowserFindSubselector::Placeholder {
                    text: "Search...".into(),
                },
                "\"Search...\"",
            ),
            (
                BrowserFindSubselector::Alt {
                    alt: "Logo".into(),
                },
                "\"Logo\"",
            ),
            (
                BrowserFindSubselector::Title {
                    title: "Close".into(),
                },
                "\"Close\"",
            ),
            (
                BrowserFindSubselector::TestId {
                    testid: "submit-btn".into(),
                },
                "\"submit-btn\"",
            ),
            (
                BrowserFindSubselector::First {
                    selector: "button.submit".into(),
                },
                "\"button.submit\"",
            ),
            (
                BrowserFindSubselector::Last {
                    selector: "div.card".into(),
                },
                "\"div.card\"",
            ),
            (
                BrowserFindSubselector::Nth {
                    selector: "li.item".into(),
                    index: 3,
                },
                "\"li.item\"",
            ),
        ] {
            let script = build_dom_find_script(&subselector);
            assert!(script.starts_with("(() => {"));
            assert!(script.ends_with("})()"));
            assert!(script.contains("return JSON.stringify"));
            assert!(
                script.contains(expected_token),
                "script missing {expected_token}: {script}"
            );
        }
    }

    #[test]
    fn browser_tab_cli_parsing_coverage() {
        // `tab new` inherits FERRYX_WORKSPACE_ID / FERRYX_WORKTREE_PATH when the matching
        // flags are absent, so clear both for the parse and restore them afterwards.
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let prev_ws = std::env::var("FERRYX_WORKSPACE_ID").ok();
        let prev_wt = std::env::var("FERRYX_WORKTREE_PATH").ok();
        std::env::remove_var("FERRYX_WORKSPACE_ID");
        std::env::remove_var("FERRYX_WORKTREE_PATH");

        let list_args = vec!["ferryx", "browser", "tab", "list"];
        assert_eq!(
            parse_browser_cli(&list_args).expect("parse tab list"),
            BrowserCliCommand::Tab {
                action: TabCliAction::List
            }
        );

        let new_args = vec!["ferryx", "browser", "tab", "new", "https://example.com/app"];
        let parsed_new = parse_browser_cli(&new_args);

        let new_opt_args = vec![
            "ferryx", "browser", "tab", "new", "--url", "https://example.com",
            "--workspace", "ws-1", "--worktree-path", "/wt/path"
        ];
        let parsed_new_opt = parse_browser_cli(&new_opt_args);

        if let Some(ws) = prev_ws {
            std::env::set_var("FERRYX_WORKSPACE_ID", ws);
        }
        if let Some(wt) = prev_wt {
            std::env::set_var("FERRYX_WORKTREE_PATH", wt);
        }

        assert_eq!(
            parsed_new.expect("parse tab new positional"),
            BrowserCliCommand::Tab {
                action: TabCliAction::New {
                    url: Some("https://example.com/app".into()),
                    workspace_id: None,
                    worktree_path: None,
                }
            }
        );

        assert_eq!(
            parsed_new_opt.expect("parse tab new options"),
            BrowserCliCommand::Tab {
                action: TabCliAction::New {
                    url: Some("https://example.com".into()),
                    workspace_id: Some("ws-1".into()),
                    worktree_path: Some("/wt/path".into()),
                }
            }
        );

        let switch_idx = vec!["ferryx", "browser", "tab", "switch", "1"];
        assert_eq!(
            parse_browser_cli(&switch_idx).expect("parse tab switch 1"),
            BrowserCliCommand::Tab {
                action: TabCliAction::Switch {
                    target: "1".into(),
                }
            }
        );

        let switch_id = vec!["ferryx", "browser", "tab", "switch", "browser-xyz"];
        assert_eq!(
            parse_browser_cli(&switch_id).expect("parse tab switch id"),
            BrowserCliCommand::Tab {
                action: TabCliAction::Switch {
                    target: "browser-xyz".into(),
                }
            }
        );

        let close_args = vec!["ferryx", "browser", "tab", "close", "2"];
        assert_eq!(
            parse_browser_cli(&close_args).expect("parse tab close 2"),
            BrowserCliCommand::Tab {
                action: TabCliAction::Close {
                    target: Some("2".into()),
                    browser_id: None,
                }
            }
        );

        // Adversarial: unknown tab sub-verb
        let bad_sub = vec!["ferryx", "browser", "tab", "invalid_verb"];
        let err = parse_browser_cli(&bad_sub).expect_err("unknown tab sub-verb");
        assert!(err.contains("unknown tab sub-verb"));
        assert!(err.contains("expected tab <list|new|switch|close>"));

        // Adversarial: tab switch missing target
        let no_target = vec!["ferryx", "browser", "tab", "switch"];
        assert!(parse_browser_cli(&no_target).is_err());
    }

    #[test]
    fn browser_storage_cli_extended_and_adversarial() {
        let clear_all = vec![
            "ferryx", "browser", "storage", "local", "clear", "--browser-id", "b-1", "--all"
        ];
        assert_eq!(
            parse_browser_cli(&clear_all).expect("parse storage clear --all"),
            BrowserCliCommand::Storage {
                browser_id: "b-1".into(),
                kind: StorageCliKind::Local,
                action: StorageCliAction::Clear {
                    key: None,
                    all: true,
                    url: None,
                    domain: None,
                }
            }
        );

        let clear_scope = vec![
            "ferryx", "browser", "storage", "session", "clear", "--browser-id", "b-1",
            "--url", "https://example.com", "--domain", "example.com"
        ];
        assert_eq!(
            parse_browser_cli(&clear_scope).expect("parse storage clear with scope"),
            BrowserCliCommand::Storage {
                browser_id: "b-1".into(),
                kind: StorageCliKind::Session,
                action: StorageCliAction::Clear {
                    key: None,
                    all: false,
                    url: Some("https://example.com".into()),
                    domain: Some("example.com".into()),
                }
            }
        );

        // Adversarial: clear --all combined with specific key
        let bad_clear = vec![
            "ferryx", "browser", "storage", "local", "clear", "mykey", "--browser-id", "b-1", "--all"
        ];
        let err = parse_browser_cli(&bad_clear).expect_err("cannot specify both key and --all");
        assert!(err.contains("cannot specify both key and --all"));

        // Adversarial: unknown storage kind
        let bad_kind = vec![
            "ferryx", "browser", "storage", "invalid_kind", "get", "--browser-id", "b-1"
        ];
        let err_k = parse_browser_cli(&bad_kind).expect_err("unknown storage kind");
        assert!(err_k.contains("expected storage kind `local` or `session`"));
    }

    #[test]
    fn browser_state_cli_parsing_coverage() {
        let save_args = vec![
            "ferryx", "browser", "state", "save", "/tmp/state.json", "--browser-id", "b-1"
        ];
        assert_eq!(
            parse_browser_cli(&save_args).expect("parse state save"),
            BrowserCliCommand::State {
                browser_id: "b-1".into(),
                action: StateCliAction::Save {
                    path: "/tmp/state.json".into(),
                }
            }
        );

        let load_args = vec![
            "ferryx", "browser", "state", "load", "--file", "/tmp/state.json", "--browser-id", "b-1"
        ];
        assert_eq!(
            parse_browser_cli(&load_args).expect("parse state load"),
            BrowserCliCommand::State {
                browser_id: "b-1".into(),
                action: StateCliAction::Load {
                    path: "/tmp/state.json".into(),
                }
            }
        );

        // Adversarial: unknown state sub-verb
        let bad_sub = vec![
            "ferryx", "browser", "state", "unknown", "--browser-id", "b-1"
        ];
        let err = parse_browser_cli(&bad_sub).expect_err("unknown state sub-verb");
        assert!(err.contains("unknown state sub-verb"));
        assert!(err.contains("expected state <save|load>"));
    }

    #[test]
    fn browser_cli_request_mapping_for_tabs_and_state() {
        let cmd_tab_switch_idx = BrowserCliCommand::Tab {
            action: TabCliAction::Switch {
                target: "2".into(),
            },
        };
        assert_eq!(
            browser_cli_request(cmd_tab_switch_idx),
            BrowserCliRequest::TabSwitch {
                browser_id: None,
                index: Some(2),
            }
        );

        let cmd_tab_switch_id = BrowserCliCommand::Tab {
            action: TabCliAction::Switch {
                target: "b-target".into(),
            },
        };
        assert_eq!(
            browser_cli_request(cmd_tab_switch_id),
            BrowserCliRequest::TabSwitch {
                browser_id: Some("b-target".into()),
                index: None,
            }
        );

        let cmd_tab_close_idx = BrowserCliCommand::Tab {
            action: TabCliAction::Close {
                target: Some("3".into()),
                browser_id: None,
            },
        };
        assert_eq!(
            browser_cli_request(cmd_tab_close_idx),
            BrowserCliRequest::TabClose {
                browser_id: None,
                index: Some(3),
            }
        );

        let cmd_state_save = BrowserCliCommand::State {
            browser_id: "b-1".into(),
            action: StateCliAction::Save {
                path: "/tmp/out.json".into(),
            },
        };
        assert_eq!(
            browser_cli_request(cmd_state_save),
            BrowserCliRequest::StateSave {
                browser_id: "b-1".into(),
                out_path: "/tmp/out.json".into(),
            }
        );
    }

    #[test]
    fn browser_cli_windows_tilde_backslash_is_expanded() {
        // m6: on Windows `~\\foo` is the native spelling of `~/foo`.
        let cwd = std::path::Path::new("/cli-cwd");
        let home = std::path::Path::new("/cli-home");
        assert_eq!(
            anchor_cli_path_with("~\\shot.png".into(), cwd, Some(home), true),
            home.join("shot.png").to_string_lossy().into_owned()
        );
        assert_eq!(
            anchor_cli_path_with("~/shot.png".into(), cwd, Some(home), true),
            home.join("shot.png").to_string_lossy().into_owned()
        );
        // Elsewhere a backslash is an ordinary filename character.
        assert_eq!(
            anchor_cli_path_with("~\\shot.png".into(), cwd, Some(home), false),
            cwd.join("~\\shot.png").to_string_lossy().into_owned()
        );
    }

    #[test]
    fn browser_cli_relative_paths_are_anchored_to_cli_cwd() {
        // A-CORE-15: the GUI resolves paths against its own cwd, so the CLI must send
        // absolute paths for screenshot --out, state save/load and download --path.
        let cwd = std::env::temp_dir().join("ferryx-cli-cwd");
        let home = std::env::temp_dir().join("ferryx-cli-home");
        let abs = |rel: &str| cwd.join(rel).to_string_lossy().into_owned();
        let map = |request| absolutize_browser_cli_paths(request, &cwd, Some(&home));

        assert_eq!(
            map(BrowserCliRequest::Screenshot {
                browser_id: "b-1".into(),
                out_path: "shots/rel.png".into(),
            }),
            BrowserCliRequest::Screenshot {
                browser_id: "b-1".into(),
                out_path: abs("shots/rel.png"),
            }
        );
        assert_eq!(
            map(BrowserCliRequest::StateSave {
                browser_id: "b-1".into(),
                out_path: "state.json".into(),
            }),
            BrowserCliRequest::StateSave {
                browser_id: "b-1".into(),
                out_path: abs("state.json"),
            }
        );
        assert_eq!(
            map(BrowserCliRequest::StateLoad {
                browser_id: "b-1".into(),
                in_path: "state.json".into(),
            }),
            BrowserCliRequest::StateLoad {
                browser_id: "b-1".into(),
                in_path: abs("state.json"),
            }
        );
        assert_eq!(
            map(BrowserCliRequest::Download {
                browser_id: Some("b-1".into()),
                url: Some("https://example.com/f.zip".into()),
                path: Some("dl/f.zip".into()),
                id: None,
                action: "start".into(),
            }),
            BrowserCliRequest::Download {
                browser_id: Some("b-1".into()),
                url: Some("https://example.com/f.zip".into()),
                path: Some(abs("dl/f.zip")),
                id: None,
                action: "start".into(),
            }
        );

        // `~` is expanded with the CLI user's home, not the GUI's.
        assert_eq!(
            map(BrowserCliRequest::Screenshot {
                browser_id: "b-1".into(),
                out_path: "~/shot.png".into(),
            }),
            BrowserCliRequest::Screenshot {
                browser_id: "b-1".into(),
                out_path: home.join("shot.png").to_string_lossy().into_owned(),
            }
        );

        // Absolute paths and path-free requests pass through untouched.
        let absolute = std::env::temp_dir().join("abs.png").to_string_lossy().into_owned();
        let unchanged = BrowserCliRequest::Screenshot {
            browser_id: "b-1".into(),
            out_path: absolute,
        };
        assert_eq!(map(unchanged.clone()), unchanged);
        assert_eq!(map(BrowserCliRequest::List), BrowserCliRequest::List);
    }

    #[test]
    fn browser_open_cli_env_var_defaults() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let prev_ws = std::env::var("FERRYX_WORKSPACE_ID").ok();
        let prev_wt = std::env::var("FERRYX_WORKTREE_PATH").ok();

        std::env::set_var("FERRYX_WORKSPACE_ID", "ws-from-env");
        std::env::set_var("FERRYX_WORKTREE_PATH", "/path/from/env");

        let args = vec!["ferryx", "browser", "open", "--url", "https://example.com"];
        let parsed = parse_browser_cli(&args).expect("parse browser open with env defaults");

        std::env::remove_var("FERRYX_WORKSPACE_ID");
        std::env::remove_var("FERRYX_WORKTREE_PATH");

        if let Some(ws) = prev_ws {
            std::env::set_var("FERRYX_WORKSPACE_ID", ws);
        }
        if let Some(wt) = prev_wt {
            std::env::set_var("FERRYX_WORKTREE_PATH", wt);
        }

        assert_eq!(
            parsed,
            BrowserCliCommand::Open {
                url: "https://example.com".into(),
                workspace_id: Some("ws-from-env".into()),
                worktree_path: Some("/path/from/env".into()),
            }
        );
    }

    #[test]
    fn pair_generate_requires_account() {
        let dir = tempfile::tempdir().expect("temp");
        let previous = std::env::var_os("FERRYX_DATA_DIR");
        std::env::set_var("FERRYX_DATA_DIR", dir.path());
        let machine = run_pair_cli(PairCliCommand::GenerateMachinePin);
        let mirror = run_pair_cli(PairCliCommand::GeneratePin);
        if let Some(value) = previous {
            std::env::set_var("FERRYX_DATA_DIR", value);
        } else {
            std::env::remove_var("FERRYX_DATA_DIR");
        }
        assert_eq!(machine.expect("machine pin"), PairCliOutcome::AccountLoginRequired);
        assert_eq!(mirror.expect("mirror pin"), PairCliOutcome::AccountLoginRequired);
    }

    #[test]
    fn account_enroll_cli_parses_code_and_optional_origin() {
        assert_eq!(
            parse_account_cli(["ferryx", "account", "enroll", "--code", "code-1"]).expect("parse"),
            AccountCliCommand::Enroll {
                code: "code-1".into(),
                origin: None,
            }
        );
        assert_eq!(
            parse_account_cli([
                "ferryx",
                "account",
                "enroll",
                "--origin",
                "https://account.example",
                "--code",
                "code-2",
            ])
            .expect("parse"),
            AccountCliCommand::Enroll {
                code: "code-2".into(),
                origin: Some("https://account.example".into()),
            }
        );
        assert!(parse_account_cli(["ferryx", "account", "enroll"]).is_err());
        assert!(parse_account_cli(["ferryx", "account", "enroll", "--code"]).is_err());
        assert!(parse_account_cli(["ferryx", "account", "enroll", "--code", "x", "--bogus"]).is_err());
        assert!(parse_account_cli(["ferryx", "pair", "list"]).is_err());
    }

    #[test]
    fn account_login_cli_parses_email_and_optional_origin() {
        assert_eq!(
            parse_account_cli(["ferryx", "account", "login"]).expect("parse"),
            AccountCliCommand::Login {
                email: None,
                origin: None,
            }
        );
        assert_eq!(
            parse_account_cli(["ferryx", "account", "login", "--email", "user@example.com"]).expect("parse"),
            AccountCliCommand::Login {
                email: Some("user@example.com".into()),
                origin: None,
            }
        );
        assert_eq!(
            parse_account_cli([
                "ferryx",
                "account",
                "login",
                "--origin",
                "https://account.example",
                "--email",
                "user@example.com",
            ])
            .expect("parse"),
            AccountCliCommand::Login {
                email: Some("user@example.com".into()),
                origin: Some("https://account.example".into()),
            }
        );
        assert!(parse_account_cli(["ferryx", "account", "login", "--bogus"]).is_err());
    }

    #[test]
    fn account_plan_cli_parses_json_and_optional_origin() {
        assert_eq!(
            parse_account_cli(["ferryx", "account", "plan"]).expect("parse"),
            AccountCliCommand::Plan {
                json: false,
                origin: None,
            }
        );
        assert_eq!(
            parse_account_cli([
                "ferryx",
                "account",
                "plan",
                "--json",
                "--origin",
                "https://account.example",
            ])
            .expect("parse"),
            AccountCliCommand::Plan {
                json: true,
                origin: Some("https://account.example".into()),
            }
        );
        assert_eq!(
            parse_account_cli(["ferryx", "account", "plan", "--json", "--json"]).expect("parse"),
            AccountCliCommand::Plan {
                json: true,
                origin: None,
            }
        );
        assert!(parse_account_cli(["ferryx", "account", "plan", "--origin"]).is_err());
        assert!(parse_account_cli(["ferryx", "account", "plan", "--bogus"]).is_err());
    }

    #[test]
    fn browser_dom_action_cli_parsing_and_mapping() {
        // Row 10 (audit D-4): every action the executor already implements must be
        // reachable from the CLI and must map onto the same Act request shape `click`
        // uses, so generation fencing and the snapshot reference path are unchanged.
        for (verb, expected_action) in [
            (
                vec![
                    "ferryx",
                    "browser",
                    "dblclick",
                    "--browser-id",
                    "b-1",
                    "--generation",
                    "2",
                    "--ref",
                    "e1",
                ],
                BrowserAutomationAction::Dblclick {
                    reference: "e1".into(),
                },
            ),
            (
                vec![
                    "ferryx",
                    "browser",
                    "hover",
                    "--browser-id",
                    "b-1",
                    "--generation",
                    "2",
                    "--ref",
                    "e1",
                ],
                BrowserAutomationAction::Hover {
                    reference: "e1".into(),
                },
            ),
            (
                vec![
                    "ferryx",
                    "browser",
                    "check",
                    "--browser-id",
                    "b-1",
                    "--generation",
                    "2",
                    "--ref",
                    "e1",
                ],
                BrowserAutomationAction::Check {
                    reference: "e1".into(),
                },
            ),
            (
                vec![
                    "ferryx",
                    "browser",
                    "uncheck",
                    "--browser-id",
                    "b-1",
                    "--generation",
                    "2",
                    "--ref",
                    "e1",
                ],
                BrowserAutomationAction::Uncheck {
                    reference: "e1".into(),
                },
            ),
            (
                vec![
                    "ferryx",
                    "browser",
                    "scroll-into-view",
                    "--browser-id",
                    "b-1",
                    "--generation",
                    "2",
                    "--ref",
                    "e1",
                ],
                BrowserAutomationAction::ScrollIntoView {
                    reference: "e1".into(),
                },
            ),
            (
                vec![
                    "ferryx",
                    "browser",
                    "select",
                    "--browser-id",
                    "b-1",
                    "--generation",
                    "2",
                    "--ref",
                    "e1",
                    "--value",
                    "ca",
                ],
                BrowserAutomationAction::Select {
                    reference: "e1".into(),
                    value: "ca".into(),
                },
            ),
            (
                vec![
                    "ferryx",
                    "browser",
                    "type",
                    "--browser-id",
                    "b-1",
                    "--generation",
                    "2",
                    "--ref",
                    "e1",
                    "--text",
                    "hello",
                ],
                BrowserAutomationAction::Type {
                    reference: Some("e1".into()),
                    text: "hello".into(),
                },
            ),
            (
                vec![
                    "ferryx",
                    "browser",
                    "scroll",
                    "--browser-id",
                    "b-1",
                    "--generation",
                    "2",
                    "--ref",
                    "e1",
                    "--x",
                    "-5",
                    "--y",
                    "120",
                ],
                BrowserAutomationAction::Scroll {
                    reference: Some("e1".into()),
                    x: Some(-5),
                    y: Some(120),
                },
            ),
        ] {
            let command = parse_browser_cli(&verb).expect("parse dom action verb");
            assert_eq!(
                browser_cli_request(command),
                BrowserCliRequest::Act {
                    request: crate::browser::BrowserAutomationRequest {
                        browser_id: "b-1".into(),
                        generation: 2,
                        action: expected_action,
                    },
                }
            );
        }

        // `type` and `scroll` may omit `--ref`: they then act on the active element
        // and the window, which is exactly what the executor does for a missing
        // reference.
        let type_without_ref = parse_browser_cli([
            "ferryx",
            "browser",
            "type",
            "--browser-id",
            "b-1",
            "--generation",
            "3",
            "--text",
            "streamed",
        ])
        .expect("parse type without ref");
        assert_eq!(
            browser_cli_request(type_without_ref),
            BrowserCliRequest::Act {
                request: crate::browser::BrowserAutomationRequest {
                    browser_id: "b-1".into(),
                    generation: 3,
                    action: BrowserAutomationAction::Type {
                        reference: None,
                        text: "streamed".into(),
                    },
                },
            }
        );

        let scroll_without_ref = parse_browser_cli([
            "ferryx",
            "browser",
            "scroll",
            "--browser-id",
            "b-1",
            "--generation",
            "3",
            "--y",
            "400",
        ])
        .expect("parse scroll without ref");
        assert_eq!(
            browser_cli_request(scroll_without_ref),
            BrowserCliRequest::Act {
                request: crate::browser::BrowserAutomationRequest {
                    browser_id: "b-1".into(),
                    generation: 3,
                    action: BrowserAutomationAction::Scroll {
                        reference: None,
                        x: None,
                        y: Some(400),
                    },
                },
            }
        );
    }

    #[test]
    fn browser_dom_action_cli_requires_generation_and_reference() {
        // Every DOM verb that names an element must keep the same required flags as
        // `click`, so a caller can never issue an unfenced or target-less action.
        for verb in [
            "dblclick",
            "hover",
            "check",
            "uncheck",
            "scroll-into-view",
        ] {
            assert!(parse_browser_cli(["ferryx", "browser", verb]).is_err());
            assert!(parse_browser_cli([
                "ferryx", "browser", verb, "--browser-id", "b-1", "--generation", "2"
            ])
            .is_err());
            assert!(parse_browser_cli([
                "ferryx", "browser", verb, "--browser-id", "b-1", "--ref", "e1"
            ])
            .is_err());
        }

        assert!(parse_browser_cli([
            "ferryx",
            "browser",
            "select",
            "--browser-id",
            "b-1",
            "--generation",
            "2",
            "--ref",
            "e1"
        ])
        .is_err());

        assert!(parse_browser_cli([
            "ferryx",
            "browser",
            "type",
            "--browser-id",
            "b-1",
            "--generation",
            "2"
        ])
        .is_err());

        assert_eq!(
            parse_browser_cli([
                "ferryx",
                "browser",
                "scroll",
                "--browser-id",
                "b-1",
                "--generation",
                "2",
                "--y",
                "not-a-number"
            ])
            .unwrap_err(),
            "--y must be an integer"
        );
    }

    #[test]
    fn browser_dialog_cli_parsing_and_mapping() {
        // Row 11 (audit D-5): the dialog IPC commands had no caller at all.
        let accept = parse_browser_cli([
            "ferryx",
            "browser",
            "dialog",
            "--browser-id",
            "b-1",
            "accept",
            "--prompt-text",
            "typed answer",
        ])
        .expect("parse dialog accept");
        assert_eq!(
            browser_cli_request(accept),
            BrowserCliRequest::Dialog {
                browser_id: "b-1".into(),
                action: "accept".into(),
                prompt_text: Some("typed answer".into()),
                policy: None,
            }
        );

        let accept_plain = parse_browser_cli([
            "ferryx",
            "browser",
            "dialog",
            "--browser-id",
            "b-1",
            "accept",
        ])
        .expect("parse dialog accept without prompt text");
        assert_eq!(
            browser_cli_request(accept_plain),
            BrowserCliRequest::Dialog {
                browser_id: "b-1".into(),
                action: "accept".into(),
                prompt_text: None,
                policy: None,
            }
        );

        let dismiss = parse_browser_cli([
            "ferryx",
            "browser",
            "dialog",
            "--browser-id",
            "b-1",
            "dismiss",
        ])
        .expect("parse dialog dismiss");
        assert_eq!(
            browser_cli_request(dismiss),
            BrowserCliRequest::Dialog {
                browser_id: "b-1".into(),
                action: "dismiss".into(),
                prompt_text: None,
                policy: None,
            }
        );

        let list = parse_browser_cli(["ferryx", "browser", "dialog", "--browser-id", "b-1", "list"])
            .expect("parse dialog list");
        assert_eq!(
            browser_cli_request(list),
            BrowserCliRequest::Dialog {
                browser_id: "b-1".into(),
                action: "list".into(),
                prompt_text: None,
                policy: None,
            }
        );

        let policy_positional = parse_browser_cli([
            "ferryx",
            "browser",
            "dialog",
            "--browser-id",
            "b-1",
            "policy",
            "auto-accept",
        ])
        .expect("parse dialog policy positional");
        assert_eq!(
            browser_cli_request(policy_positional),
            BrowserCliRequest::Dialog {
                browser_id: "b-1".into(),
                action: "policy".into(),
                prompt_text: None,
                policy: Some("auto-accept".into()),
            }
        );

        let policy_flag = parse_browser_cli([
            "ferryx",
            "browser",
            "dialog",
            "--browser-id",
            "b-1",
            "policy",
            "--value",
            "auto-dismiss",
        ])
        .expect("parse dialog policy flag");
        assert_eq!(
            browser_cli_request(policy_flag),
            BrowserCliRequest::Dialog {
                browser_id: "b-1".into(),
                action: "policy".into(),
                prompt_text: None,
                policy: Some("auto-dismiss".into()),
            }
        );

        // Adversarial: the guest bridge only implements `auto-accept` and
        // `auto-dismiss`; any other value silently behaved like `auto-dismiss`
        // before, so it is rejected at parse time instead.
        assert_eq!(
            parse_browser_cli([
                "ferryx",
                "browser",
                "dialog",
                "--browser-id",
                "b-1",
                "policy",
                "always-accept",
            ])
            .unwrap_err(),
            "invalid dialog policy `always-accept`: expected `auto-dismiss` or `auto-accept`"
        );

        assert_eq!(
            parse_browser_cli(["ferryx", "browser", "dialog", "--browser-id", "b-1"])
                .unwrap_err(),
            "expected dialog <accept|dismiss|list|policy>"
        );

        assert!(parse_browser_cli(["ferryx", "browser", "dialog", "accept"]).is_err());
    }

    #[test]
    fn browser_download_cli_parsing_and_mapping() {
        // Row 12 (audit D-6): the cookie-carrying download path had no caller.
        let start = parse_browser_cli([
            "ferryx",
            "browser",
            "download",
            "--browser-id",
            "b-1",
            "--url",
            "https://example.com/private.pdf",
            "--path",
            "/tmp/private.pdf",
        ])
        .expect("parse download start");
        assert_eq!(
            browser_cli_request(start),
            BrowserCliRequest::Download {
                browser_id: Some("b-1".into()),
                url: Some("https://example.com/private.pdf".into()),
                path: Some("/tmp/private.pdf".into()),
                id: None,
                action: "start".into(),
            }
        );

        let list = parse_browser_cli(["ferryx", "browser", "download", "list"])
            .expect("parse download list");
        assert_eq!(
            browser_cli_request(list),
            BrowserCliRequest::Download {
                browser_id: None,
                url: None,
                path: None,
                id: None,
                action: "list".into(),
            }
        );

        let cancel = parse_browser_cli([
            "ferryx",
            "browser",
            "download",
            "cancel",
            "--id",
            "download-7",
        ])
        .expect("parse download cancel");
        assert_eq!(
            browser_cli_request(cancel),
            BrowserCliRequest::Download {
                browser_id: None,
                url: None,
                path: None,
                id: Some("download-7".into()),
                action: "cancel".into(),
            }
        );

        // Adversarial: a start without its target path, and a cancel without an id,
        // must fail loudly rather than downloading nowhere or cancelling nothing.
        assert!(parse_browser_cli([
            "ferryx",
            "browser",
            "download",
            "--browser-id",
            "b-1",
            "--url",
            "https://example.com/a.pdf"
        ])
        .is_err());
        assert!(parse_browser_cli(["ferryx", "browser", "download", "cancel"]).is_err());
        assert!(parse_browser_cli(["ferryx", "browser", "download"]).is_err());
    }

    #[test]
    fn browser_highlight_cli_parsing_and_mapping() {
        // Row 14 (audit D-7): `cmd_browser_highlight` had no caller.
        let default_duration = parse_browser_cli([
            "ferryx",
            "browser",
            "highlight",
            "--browser-id",
            "b-1",
            "--selector",
            "#submit",
        ])
        .expect("parse highlight");
        assert_eq!(
            browser_cli_request(default_duration),
            BrowserCliRequest::Highlight {
                browser_id: "b-1".into(),
                selector: "#submit".into(),
                duration_ms: None,
            }
        );

        let explicit_duration = parse_browser_cli([
            "ferryx",
            "browser",
            "highlight",
            "--browser-id",
            "b-1",
            "--selector",
            "#submit",
            "--duration-ms",
            "250",
        ])
        .expect("parse highlight with duration");
        assert_eq!(
            browser_cli_request(explicit_duration),
            BrowserCliRequest::Highlight {
                browser_id: "b-1".into(),
                selector: "#submit".into(),
                duration_ms: Some(250),
            }
        );

        assert!(parse_browser_cli(["ferryx", "browser", "highlight", "--browser-id", "b-1"])
            .is_err());
        assert_eq!(
            parse_browser_cli([
                "ferryx",
                "browser",
                "highlight",
                "--browser-id",
                "b-1",
                "--selector",
                "#submit",
                "--duration-ms",
                "soon"
            ])
            .unwrap_err(),
            "--duration-ms must be an unsigned integer"
        );
    }

    #[test]
    fn browser_formatter_prints_new_response_shapes() {
        // Row 12: `download list` must print a JSON array of `DownloadRecord`, not a
        // wrapper object — this is also what the sibling list responses in the same
        // match print. RED mutation: restore the
        // `json!({ "type": "downloads", "downloads": downloads })` wrapper in
        // `format_browser_cli_response` and the array assertion below fails.
        let record = crate::browser::download::DownloadRecord {
            id: "d-1".into(),
            url: "https://example.com/a.pdf".into(),
            file_path: "/tmp/a.pdf".into(),
            status: crate::browser::download::DownloadStatus::Completed,
            total_bytes: Some(10),
            received_bytes: 10,
            error: None,
            created_at_ms: 1,
            updated_at_ms: 2,
        };
        let list_output = format_browser_cli_response(
            &BrowserCliCommand::Download {
                action: DownloadCliAction::List,
            },
            BrowserCliResponse::Downloads {
                downloads: vec![record.clone()],
            },
        )
        .expect("format download list");
        assert_eq!(
            list_output,
            serde_json::to_string(&vec![record.clone()]).expect("serialize download array")
        );
        assert!(
            list_output.starts_with('['),
            "download list must be a JSON array: {list_output}"
        );

        // Every other new response carries a `type` field, exactly like its
        // neighbours in this match. RED mutation: delete one print arm (e.g. the
        // `DialogPolicySet` arm) — the response then falls through to the
        // `_ => Ok(String::new())` fallback and `serde_json::from_str` panics here.
        for (response, expected_type) in [
            (
                BrowserCliResponse::DialogHandled {
                    dialog: crate::browser::model::BrowserDialogEntry {
                        id: "d-1".into(),
                        r#type: "confirm".into(),
                        message: "Leave?".into(),
                        default_value: None,
                        at_ms: 1,
                        handled: true,
                        action: "accept".into(),
                        result: Some("true".into()),
                    },
                },
                "dialogHandled",
            ),
            (
                BrowserCliResponse::DialogEntries { dialogs: vec![] },
                "dialogEntries",
            ),
            (
                BrowserCliResponse::DialogPolicySet {
                    policy: "auto-accept".into(),
                },
                "dialogPolicySet",
            ),
            (
                BrowserCliResponse::DownloadStarted {
                    download: record.clone(),
                },
                "downloadStarted",
            ),
            (
                BrowserCliResponse::DownloadCancelled {
                    id: "d-1".into(),
                    cancelled: true,
                },
                "downloadCancelled",
            ),
            (
                BrowserCliResponse::Highlighted {
                    highlight: crate::browser::picker::BrowserHighlightResult {
                        selector: "#a".into(),
                        tag_name: "div".into(),
                        rect: crate::browser::picker::ElementRect {
                            x: 1.0,
                            y: 2.0,
                            width: 3.0,
                            height: 4.0,
                        },
                    },
                },
                "highlighted",
            ),
        ] {
            let output = format_browser_cli_response(&BrowserCliCommand::Identify, response)
                .expect("format new response");
            let parsed: serde_json::Value =
                serde_json::from_str(&output).expect("new response must be JSON");
            assert_eq!(parsed["type"], expected_type, "{output}");
        }
    }

    #[test]
    fn browser_cookie_header_from_session_cookies() {
        // Row 12 security fix (audit D-6). RED mutation: make
        // `cookie_header_from_cookies` skip cookies whose `http_only()` is
        // `Some(true)` — the assertion below fails, because the login session
        // cookie is exactly the HttpOnly cookie `document.cookie` never exposed.
        let mut session = tauri::webview::Cookie::new("session", "abc123");
        session.set_http_only(true);
        session.set_secure(true);
        let plain = tauri::webview::Cookie::new("theme", "dark");
        assert_eq!(
            crate::ipc::browser::cookie_header_from_cookies(&[session, plain]).as_deref(),
            Some("session=abc123; theme=dark")
        );

        // RED mutation: drop the emptiness guard so the builder always returns
        // `Some(..)` — this assertion then sees `Some("")` instead of `None` and
        // the download path would send an empty `Cookie:` header.
        assert_eq!(crate::ipc::browser::cookie_header_from_cookies(&[]), None);
    }
}
