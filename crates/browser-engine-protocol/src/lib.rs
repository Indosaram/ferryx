//! Line protocol between Ferryx and its Chromium (CEF) engine process.
//!
//! One engine process serves every tab of one browser profile, so cookies, logins and
//! extensions are shared by those tabs. Ferryx writes [`Command`] lines to the engine's stdin
//! and reads [`Event`] lines from its stdout. Each line is space separated; the last field of a
//! line that carries a URL or a title takes the rest of the line. Event lines
//! start with [`EVENT_PREFIX`] so anything else Chromium prints to stdout is ignored.
//!
//! Tab ids are Ferryx browser ids (ASCII letters, digits, `:`, `.`, `_`, `-`).

/// Marks an engine stdout line as a protocol event.
pub const EVENT_PREFIX: &str = "@ferryx ";

/// A pane rectangle: pixels inside the Ferryx window on Windows, screen points from the
/// top-left on macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Ferryx to engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Opens a tab's browser window at `rect`, shown or hidden.
    Open { tab: String, rect: Rect, visible: bool, url: String },
    /// Makes a window the engine opened on its own (see [`Event::Popup`]) the window of `tab`.
    Adopt { tab: String, popup: u32 },
    Bounds { tab: String, rect: Rect },
    Show { tab: String },
    Hide { tab: String },
    /// Puts every shown pane window back directly above the Ferryx window (macOS).
    Raise,
    Close { tab: String },
    Navigate { tab: String, url: String },
    Back { tab: String },
    Forward { tab: String },
    Reload { tab: String, ignore_cache: bool },
}

/// Engine to Ferryx.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The tab's window exists; `handle` is its native window (HWND on Windows, 0 on macOS).
    Created { tab: String, handle: u64 },
    /// The engine opened a window on its own (a new tab, a popup); Ferryx answers with
    /// [`Command::Adopt`] once it made a tab for it.
    Popup { popup: u32, handle: u64, url: String },
    Address { tab: String, url: String },
    Title { tab: String, title: String },
    Loading { tab: String, loading: bool, can_go_back: bool, can_go_forward: bool },
    /// The tab's browser closed inside the engine (the page, a shortcut, a crash).
    Closed { tab: String },
}

fn one_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

fn flag(value: bool) -> &'static str {
    if value {
        "1"
    } else {
        "0"
    }
}

fn parse_flag(value: &str) -> Option<bool> {
    match value {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// Splits off `count` space-separated fields and returns them with the rest of the line.
fn fields(line: &str, count: usize) -> Option<(Vec<&str>, &str)> {
    let mut parts = Vec::with_capacity(count);
    let mut rest = line;
    for _ in 0..count {
        let (head, tail) = match rest.split_once(' ') {
            Some(split) => split,
            None => (rest, ""),
        };
        if head.is_empty() {
            return None;
        }
        parts.push(head);
        rest = tail;
    }
    Some((parts, rest))
}

fn parse_rect(parts: &[&str]) -> Option<Rect> {
    Some(Rect {
        x: parts.first()?.parse().ok()?,
        y: parts.get(1)?.parse().ok()?,
        width: parts.get(2)?.parse().ok()?,
        height: parts.get(3)?.parse().ok()?,
    })
}

fn rect_fields(rect: &Rect) -> String {
    format!("{} {} {} {}", rect.x, rect.y, rect.width, rect.height)
}

impl Command {
    pub fn to_line(&self) -> String {
        match self {
            Self::Open { tab, rect, visible, url } => {
                format!("open {tab} {} {} {}", rect_fields(rect), flag(*visible), one_line(url))
            }
            Self::Adopt { tab, popup } => format!("adopt {tab} {popup}"),
            Self::Bounds { tab, rect } => format!("bounds {tab} {}", rect_fields(rect)),
            Self::Show { tab } => format!("show {tab}"),
            Self::Hide { tab } => format!("hide {tab}"),
            Self::Raise => "raise".to_string(),
            Self::Close { tab } => format!("close {tab}"),
            Self::Navigate { tab, url } => format!("navigate {tab} {}", one_line(url)),
            Self::Back { tab } => format!("back {tab}"),
            Self::Forward { tab } => format!("forward {tab}"),
            Self::Reload { tab, ignore_cache } => format!("reload {tab} {}", flag(*ignore_cache)),
        }
    }

    pub fn parse(line: &str) -> Option<Self> {
        let (verb, rest) = line.trim_end().split_once(' ').unwrap_or((line.trim_end(), ""));
        let tab_only = |rest: &str| fields(rest, 1).map(|(parts, _)| parts[0].to_string());
        match verb {
            "open" => {
                let (parts, url) = fields(rest, 6)?;
                Some(Self::Open {
                    tab: parts[0].to_string(),
                    rect: parse_rect(&parts[1..5])?,
                    visible: parse_flag(parts[5])?,
                    url: url.to_string(),
                })
            }
            "adopt" => {
                let (parts, _) = fields(rest, 2)?;
                Some(Self::Adopt { tab: parts[0].to_string(), popup: parts[1].parse().ok()? })
            }
            "bounds" => {
                let (parts, _) = fields(rest, 5)?;
                Some(Self::Bounds { tab: parts[0].to_string(), rect: parse_rect(&parts[1..5])? })
            }
            "show" => Some(Self::Show { tab: tab_only(rest)? }),
            "hide" => Some(Self::Hide { tab: tab_only(rest)? }),
            "raise" => Some(Self::Raise),
            "close" => Some(Self::Close { tab: tab_only(rest)? }),
            "navigate" => {
                let (parts, url) = fields(rest, 1)?;
                Some(Self::Navigate { tab: parts[0].to_string(), url: url.to_string() })
            }
            "back" => Some(Self::Back { tab: tab_only(rest)? }),
            "forward" => Some(Self::Forward { tab: tab_only(rest)? }),
            "reload" => {
                let (parts, _) = fields(rest, 2)?;
                Some(Self::Reload { tab: parts[0].to_string(), ignore_cache: parse_flag(parts[1])? })
            }
            _ => None,
        }
    }
}

impl Event {
    pub fn to_line(&self) -> String {
        let body = match self {
            Self::Created { tab, handle } => format!("created {tab} {handle}"),
            Self::Popup { popup, handle, url } => format!("popup {popup} {handle} {}", one_line(url)),
            Self::Address { tab, url } => format!("address {tab} {}", one_line(url)),
            Self::Title { tab, title } => format!("title {tab} {}", one_line(title)),
            Self::Loading { tab, loading, can_go_back, can_go_forward } => format!(
                "loading {tab} {} {} {}",
                flag(*loading),
                flag(*can_go_back),
                flag(*can_go_forward)
            ),
            Self::Closed { tab } => format!("closed {tab}"),
        };
        format!("{EVENT_PREFIX}{body}")
    }

    /// Parses an engine stdout line; `None` for lines that are not protocol events.
    pub fn parse(line: &str) -> Option<Self> {
        let body = line.trim_end_matches(['\r', '\n']).strip_prefix(EVENT_PREFIX)?;
        let (verb, rest) = body.split_once(' ').unwrap_or((body, ""));
        match verb {
            "created" => {
                let (parts, _) = fields(rest, 2)?;
                Some(Self::Created { tab: parts[0].to_string(), handle: parts[1].parse().ok()? })
            }
            "popup" => {
                let (parts, url) = fields(rest, 2)?;
                Some(Self::Popup {
                    popup: parts[0].parse().ok()?,
                    handle: parts[1].parse().ok()?,
                    url: url.to_string(),
                })
            }
            "address" => {
                let (parts, url) = fields(rest, 1)?;
                Some(Self::Address { tab: parts[0].to_string(), url: url.to_string() })
            }
            "title" => {
                let (parts, title) = fields(rest, 1)?;
                Some(Self::Title { tab: parts[0].to_string(), title: title.to_string() })
            }
            "loading" => {
                let (parts, _) = fields(rest, 4)?;
                Some(Self::Loading {
                    tab: parts[0].to_string(),
                    loading: parse_flag(parts[1])?,
                    can_go_back: parse_flag(parts[2])?,
                    can_go_forward: parse_flag(parts[3])?,
                })
            }
            "closed" => Some(Self::Closed { tab: fields(rest, 1)?.0[0].to_string() }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Rect {
        Rect { x: -4, y: 20, width: 800, height: 600 }
    }

    #[test]
    fn every_command_survives_a_round_trip() {
        let tab = "3ae2a12b-a92f:x.y_z".to_string();
        let commands = [
            Command::Open {
                tab: tab.clone(),
                rect: rect(),
                visible: true,
                url: "https://example.com/a b?q=1".into(),
            },
            Command::Adopt { tab: tab.clone(), popup: 7 },
            Command::Bounds { tab: tab.clone(), rect: rect() },
            Command::Show { tab: tab.clone() },
            Command::Hide { tab: tab.clone() },
            Command::Raise,
            Command::Close { tab: tab.clone() },
            Command::Navigate { tab: tab.clone(), url: "https://example.com/".into() },
            Command::Back { tab: tab.clone() },
            Command::Forward { tab: tab.clone() },
            Command::Reload { tab: tab.clone(), ignore_cache: true },
            Command::Raise,
        ];
        for command in commands {
            assert_eq!(Command::parse(&command.to_line()), Some(command));
        }
    }

    #[test]
    fn every_event_survives_a_round_trip() {
        let tab = "b1".to_string();
        let events = [
            Event::Created { tab: tab.clone(), handle: 132_456 },
            Event::Popup { popup: 3, handle: 0, url: "chrome://newtab/".into() },
            Event::Address { tab: tab.clone(), url: "https://a.test/".into() },
            Event::Title { tab: tab.clone(), title: "Inbox (3) - mail".into() },
            Event::Loading { tab: tab.clone(), loading: false, can_go_back: true, can_go_forward: false },
            Event::Closed { tab },
        ];
        for event in events {
            assert_eq!(Event::parse(&event.to_line()), Some(event));
        }
    }

    #[test]
    fn an_empty_title_and_a_line_break_in_a_title_stay_on_one_line() {
        let empty = Event::Title { tab: "b1".into(), title: String::new() };
        assert_eq!(Event::parse(&empty.to_line()), Some(empty));
        let broken = Event::Title { tab: "b1".into(), title: "a\nb".into() };
        assert!(!broken.to_line().contains('\n'));
        assert_eq!(
            Event::parse(&broken.to_line()),
            Some(Event::Title { tab: "b1".into(), title: "a b".into() })
        );
    }

    #[test]
    fn lines_without_the_prefix_or_with_bad_fields_are_ignored() {
        assert_eq!(Event::parse("launch browser process"), None);
        assert_eq!(Event::parse("@ferryx created b1 not-a-number"), None);
        assert_eq!(Event::parse("@ferryx loading b1 1 2 0"), None);
        assert_eq!(Event::parse("@ferryx unknown b1"), None);
        assert_eq!(Command::parse("open b1 0 0 800"), None);
        assert_eq!(Command::parse("show"), None);
        assert_eq!(Command::parse("dance b1"), None);
    }

    #[test]
    fn a_crlf_line_from_a_windows_pipe_parses() {
        assert_eq!(
            Event::parse("@ferryx closed b1\r\n"),
            Some(Event::Closed { tab: "b1".into() })
        );
        assert_eq!(Command::parse("hide b1\r"), Some(Command::Hide { tab: "b1".into() }));
    }
}
