//! Slack `/omo-*` commands: the gateway's twenty, plus the owner's report rules and lessons.
use super::session::SessionKey;

pub const COMMANDS: [(&str, &str); 22] = [
    ("model", "/omo-model <name>"),
    ("reset", "/omo-reset [confirm]"),
    ("stop", "/omo-stop"),
    ("status", "/omo-status"),
    ("tools", "/omo-tools"),
    ("skill", "/omo-skill <list|search|read|run|pending|review|approve|reject> [name|query|id]"),
    ("skills", "/omo-skills [list|search|read|run|pending|review|approve|reject] [name|query|id]"),
    ("memory", "/omo-memory [list|pending|review|approve|reject] [id]"),
    ("cron", "/omo-cron [list]"),
    ("steer", "/omo-steer <guidance>"),
    ("undo", "/omo-undo"),
    ("retry", "/omo-retry"),
    ("compress", "/omo-compress"),
    ("title", "/omo-title [<thread link or timestamp>] <text>"),
    ("thread", "/omo-thread <name> [| first message]"),
    ("deny", "/omo-deny [reason]"),
    ("yolo", "/omo-yolo [on|off]"),
    ("pair", "/omo-pair <code>"),
    (
        "sessions",
        "/omo-sessions overview | read <target> [limit] | send <target> [--mode auto|steer|follow_up] <text> | stop <target> | answer <target> [--request <id>] <text> | watch <target> | unwatch <target>",
    ),
    (
        "huddle",
        "/omo-huddle join <huddle link or channel id> | leave | status | speak <text> | transcript",
    ),
    ("rules", "/omo-rules [list | close <number>]"),
    ("lessons", "/omo-lessons [list | retire <number> | pin <number>]"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlashInvocation {
    pub command: String,
    pub text: String,
    pub user_id: String,
    pub channel_id: String,
    pub team_id: String,
    pub thread_ts: Option<String>,
}

impl SlashInvocation {
    pub fn is_dm(&self) -> bool {
        self.channel_id.starts_with('D')
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionsCommand {
    Overview,
    Read {
        target: String,
        limit: Option<usize>,
    },
    Send {
        target: String,
        text: String,
        mode: String,
    },
    Stop {
        target: String,
    },
    Answer {
        target: String,
        text: String,
        request_id: Option<String>,
    },
    Watch {
        target: String,
    },
    Unwatch {
        target: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HuddleAction {
    Join(String),
    Leave,
    Status,
    Speak(String),
    Transcript,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlackCommand {
    Model(String),
    Reset {
        confirm: Option<bool>,
    },
    Stop,
    Status,
    Tools,
    Skill {
        action: String,
        arg: Option<String>,
    },
    Memory {
        action: String,
        arg: Option<String>,
    },
    Cron {
        action: String,
    },
    Steer(String),
    Undo,
    Retry,
    Compress,
    Title(String),
    Thread {
        name: String,
        message: Option<String>,
    },
    Deny(Option<String>),
    Yolo(Option<String>),
    Pair(String),
    Sessions(SessionsCommand),
    Huddle(HuddleAction),
    Rules(ListAction),
    Lessons(ListAction),
    Help,
}

/// An action on a numbered list the owner sees (`/omo-rules`, `/omo-lessons`). Numbers are
/// 1-based positions in the last `list`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListAction {
    List,
    Close(usize),
    Retire(usize),
    Pin(usize),
}

fn parse_list_action(text: &str, verbs: &[&str]) -> Option<ListAction> {
    let (verb, rest) = split_word(text);
    let verb = verb.to_ascii_lowercase();
    if verb.is_empty() || verb == "list" {
        return rest.is_empty().then_some(ListAction::List);
    }
    if !verbs.contains(&verb.as_str()) {
        return None;
    }
    let number: usize = rest.trim_start_matches('#').parse().ok().filter(|n| *n > 0)?;
    match verb.as_str() {
        "close" => Some(ListAction::Close(number)),
        "retire" => Some(ListAction::Retire(number)),
        "pin" => Some(ListAction::Pin(number)),
        _ => None,
    }
}

pub fn parse_command(command: &str, text: &str) -> Result<SlackCommand, String> {
    let name = command_name(command);
    let text = text.trim();
    let usage = || {
        COMMANDS
            .iter()
            .find(|(command, _)| *command == name)
            .map(|(_, usage)| format!("Usage: `{usage}`"))
            .unwrap_or_else(help_text)
    };
    let required = |value: &str| {
        if value.is_empty() {
            Err(usage())
        } else {
            Ok(value.to_owned())
        }
    };
    let (first, rest) = split_word(text);
    let rest_opt = (!rest.is_empty()).then(|| rest.to_owned());
    match name {
        "model" => required(text).map(SlackCommand::Model),
        "reset" => match text.to_ascii_lowercase().as_str() {
            "" => Ok(SlackCommand::Reset { confirm: None }),
            "confirm" | "confirm:true" | "true" | "yes" | "y" | "1" => Ok(SlackCommand::Reset {
                confirm: Some(true),
            }),
            "confirm:false" | "false" | "no" | "n" | "0" => Ok(SlackCommand::Reset {
                confirm: Some(false),
            }),
            _ => Err(usage()),
        },
        "stop" => Ok(SlackCommand::Stop),
        "status" => Ok(SlackCommand::Status),
        "tools" => Ok(SlackCommand::Tools),
        "skill" => Ok(SlackCommand::Skill {
            action: required(first)?.to_ascii_lowercase(),
            arg: rest_opt,
        }),
        "skills" => Ok(SlackCommand::Skill {
            action: if first.is_empty() {
                "list".into()
            } else {
                first.to_ascii_lowercase()
            },
            arg: rest_opt,
        }),
        "memory" => Ok(SlackCommand::Memory {
            action: if first.is_empty() {
                "list".into()
            } else {
                first.to_ascii_lowercase()
            },
            arg: rest_opt,
        }),
        "cron" => Ok(SlackCommand::Cron {
            action: if first.is_empty() {
                "list".into()
            } else {
                first.to_ascii_lowercase()
            },
        }),
        "steer" => required(text).map(SlackCommand::Steer),
        "undo" => Ok(SlackCommand::Undo),
        "retry" => Ok(SlackCommand::Retry),
        "compress" => Ok(SlackCommand::Compress),
        "title" => required(text).map(SlackCommand::Title),
        "thread" => {
            let (name, message) = match text.split_once('|') {
                Some((name, message)) => (name.trim(), Some(message.trim())),
                None => (text, None),
            };
            Ok(SlackCommand::Thread {
                name: required(name)?,
                message: message
                    .filter(|message| !message.is_empty())
                    .map(str::to_owned),
            })
        }
        "deny" => Ok(SlackCommand::Deny(
            (!text.is_empty()).then(|| text.to_owned()),
        )),
        "yolo" => Ok(SlackCommand::Yolo(
            (!text.is_empty()).then(|| text.to_owned()),
        )),
        "pair" => required(text).map(SlackCommand::Pair),
        "sessions" => parse_sessions(text)
            .map(SlackCommand::Sessions)
            .ok_or_else(usage),
        "huddle" => parse_huddle(text).ok_or_else(usage),
        "rules" => parse_list_action(text, &["close"])
            .map(SlackCommand::Rules)
            .ok_or_else(usage),
        "lessons" => parse_list_action(text, &["retire", "pin"])
            .map(SlackCommand::Lessons)
            .ok_or_else(usage),
        _ => Ok(SlackCommand::Help),
    }
}

pub fn help_text() -> String {
    let mut lines = vec!["*omo-gateway commands*".to_owned()];
    lines.extend(COMMANDS.iter().map(|(_, usage)| format!("• `{usage}`")));
    lines.join("\n")
}

/// DMs are per-user. Channels are shared per team, then stamped with the bot id.
pub fn session_key(invocation: &SlashInvocation, bot_user_id: &str) -> SessionKey {
    let key = if invocation.is_dm() {
        SessionKey::new(
            "slack",
            None::<String>,
            invocation.channel_id.clone(),
            invocation.thread_ts.clone(),
            invocation.user_id.clone(),
        )
    } else {
        SessionKey::new(
            "slack",
            Some(invocation.team_id.clone()),
            invocation.channel_id.clone(),
            invocation.thread_ts.clone(),
            "",
        )
    };
    key.with_bot_id(bot_user_id)
}

pub fn upload_request(
    filename: &str,
    channels: &str,
    thread_ts: Option<&str>,
    title: &str,
) -> serde_json::Value {
    serde_json::json!({
        "method": "files.upload",
        "filename": filename,
        "channels": channels,
        "thread_ts": thread_ts,
        "title": title
    })
}

pub fn lane_status_text(key: &str, status: &str) -> String {
    format!("*{key}* — {status}")
}

/// Messages strictly after `cursor`, compared as Slack timestamps.
pub fn backfill<'a>(cursor: &str, messages: &'a [(String, String)]) -> Vec<&'a (String, String)> {
    let cursor = ts_key(cursor);
    messages
        .iter()
        .filter(|(ts, _)| ts_key(ts) > cursor)
        .collect()
}

fn ts_key(ts: &str) -> (i64, i64) {
    let (whole, frac) = ts.split_once('.').unwrap_or((ts, "0"));
    (whole.parse().unwrap_or(0), frac.parse().unwrap_or(0))
}

fn command_name(command: &str) -> &str {
    command
        .trim()
        .trim_start_matches('/')
        .trim_start_matches("omo-")
}

fn split_word(text: &str) -> (&str, &str) {
    let text = text.trim();
    match text.split_once(char::is_whitespace) {
        Some((first, rest)) => (first, rest.trim()),
        None => (text, ""),
    }
}

fn parse_huddle(text: &str) -> Option<SlackCommand> {
    let (action, rest) = split_word(text);
    let action = match action.to_ascii_lowercase().as_str() {
        "join" if !rest.is_empty() => HuddleAction::Join(rest.to_owned()),
        "leave" if rest.is_empty() => HuddleAction::Leave,
        "status" if rest.is_empty() => HuddleAction::Status,
        "speak" if !rest.is_empty() => HuddleAction::Speak(rest.to_owned()),
        "transcript" if rest.is_empty() => HuddleAction::Transcript,
        _ => return None,
    };
    Some(SlackCommand::Huddle(action))
}

fn parse_sessions(text: &str) -> Option<SessionsCommand> {
    let (sub, rest) = split_word(text);
    let (target, rest) = split_word(rest);
    let needs = |command: SessionsCommand| (!target.is_empty()).then_some(command);
    match sub.to_ascii_lowercase().as_str() {
        "" | "overview" => Some(SessionsCommand::Overview),
        "read" => {
            let limit = match rest {
                "" => None,
                value => Some(value.parse().ok()?),
            };
            needs(SessionsCommand::Read {
                target: target.to_owned(),
                limit,
            })
        }
        "send" => {
            let (mode, text) = take_option(rest, "--mode");
            let mode = mode
                .unwrap_or_else(|| "auto".to_owned())
                .to_ascii_lowercase();
            if !matches!(mode.as_str(), "auto" | "steer" | "follow_up") || text.is_empty() {
                return None;
            }
            needs(SessionsCommand::Send {
                target: target.to_owned(),
                text,
                mode,
            })
        }
        "stop" => needs(SessionsCommand::Stop {
            target: target.to_owned(),
        }),
        "answer" => {
            let (request_id, text) = take_option(rest, "--request");
            if text.is_empty() {
                return None;
            }
            needs(SessionsCommand::Answer {
                target: target.to_owned(),
                text,
                request_id,
            })
        }
        "watch" => needs(SessionsCommand::Watch {
            target: target.to_owned(),
        }),
        "unwatch" => needs(SessionsCommand::Unwatch {
            target: target.to_owned(),
        }),
        _ => None,
    }
}

fn take_option(text: &str, flag: &str) -> (Option<String>, String) {
    let mut parts = text.split_whitespace();
    let mut mode = None;
    let mut kept = Vec::new();
    while let Some(part) = parts.next() {
        if part == flag {
            mode = parts.next().map(str::to_owned);
        } else {
            kept.push(part);
        }
    }
    (mode, kept.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_commands_parse_and_sessions_are_scoped() {
        assert_eq!(COMMANDS.len(), 22);
        assert_eq!(
            parse_command("/omo-rules", "").unwrap(),
            SlackCommand::Rules(ListAction::List)
        );
        assert_eq!(
            parse_command("/omo-rules", "close #2").unwrap(),
            SlackCommand::Rules(ListAction::Close(2))
        );
        assert!(parse_command("/omo-rules", "close 0").is_err());
        assert!(parse_command("/omo-rules", "retire 1").is_err());
        assert_eq!(
            parse_command("/omo-lessons", "pin 3").unwrap(),
            SlackCommand::Lessons(ListAction::Pin(3))
        );
        assert_eq!(
            parse_command("/omo-lessons", "retire 1").unwrap(),
            SlackCommand::Lessons(ListAction::Retire(1))
        );
        assert!(parse_command("/omo-lessons", "close 1").is_err());
        assert!(parse_command("/omo-model", "")
            .unwrap_err()
            .contains("Usage"));
        assert_eq!(
            parse_command("/omo-model", "opus").unwrap(),
            SlackCommand::Model("opus".into())
        );
        assert_eq!(parse_command("omo-stop", "").unwrap(), SlackCommand::Stop);
        assert_eq!(
            parse_command("/omo-yolo", "on").unwrap(),
            SlackCommand::Yolo(Some("on".into()))
        );
        let pair = parse_command("/omo-pair", "").unwrap_err();
        assert!(pair.contains("Usage"));
        assert!(matches!(
            parse_command("/omo-huddle", "join C1").unwrap(),
            SlackCommand::Huddle(HuddleAction::Join(_))
        ));
        assert!(parse_command("/omo-huddle", "join").is_err());
        let send = parse_command("/omo-sessions", "send pane --mode steer hello there").unwrap();
        assert_eq!(
            send,
            SlackCommand::Sessions(SessionsCommand::Send {
                target: "pane".into(),
                text: "hello there".into(),
                mode: "steer".into()
            })
        );
        assert_eq!(
            parse_command("/omo-unknown", "").unwrap(),
            SlackCommand::Help
        );
        let dm = SlashInvocation {
            command: "/omo-status".into(),
            text: String::new(),
            user_id: "U1".into(),
            channel_id: "D1".into(),
            team_id: "T1".into(),
            thread_ts: None,
        };
        let channel = SlashInvocation {
            channel_id: "C1".into(),
            ..dm.clone()
        };
        assert_eq!(session_key(&dm, "B1").user_id, "U1");
        assert!(session_key(&channel, "B1").user_id.is_empty());
        assert_eq!(session_key(&channel, "B1").bot_id.as_deref(), Some("B1"));
        let upload = upload_request("a.txt", "C1", Some("1.2"), "notes");
        assert_eq!(upload["method"], "files.upload");
        let messages = [
            ("1700.0001".to_owned(), "old".to_owned()),
            ("1700.0002".to_owned(), "new".to_owned()),
        ];
        let later = backfill("1700.0001", &messages);
        assert_eq!(later.len(), 1);
        assert!(lane_status_text("lane-a", "working").contains("working"));
    }
}
