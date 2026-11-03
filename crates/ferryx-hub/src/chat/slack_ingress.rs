//! omo-gateway's Slack ingress enrichment for the omo turn path: forwarded/shared messages,
//! file attachments (downloaded, small text files inlined, audio transcribed), the thread root
//! a reply answers, the channel topic and recent channel context on a mention, and the
//! timestamped prompt omo's `render_user_prompt` built. Ported from omo `slack/ingress.rs`,
//! `discord/adapter.rs` (shared formatters), `security/neutralize.rs` and
//! `voice_transcription.rs`.
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::Value;

const REFERENCED_CONTENT_CAP: usize = 500;
const DEFAULT_CHANNEL_CONTEXT_LIMIT: usize = 10;
const MAX_CHANNEL_CONTEXT_LIMIT: usize = 25;
const MAX_CONTEXT_LINE_CHARS: usize = 200;
/// omo inlined text attachments up to 100 KB.
const MAX_INLINED_ATTACHMENT_BYTES: u64 = 100 * 1024;
const MAX_DOWNLOAD_BYTES: u64 = 50 * 1024 * 1024;

/// One Slack file of a message (or of a message it forwards).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackFile {
    pub id: String,
    pub filename: String,
    pub url: String,
    pub content_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub local_path: Option<PathBuf>,
    pub text_content: Option<String>,
}

fn str_field<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(Value::as_str)
}

fn text_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
}

/// omo `attachments`: files with an id and a private URL.
pub fn files_of(event: &Value) -> Vec<SlackFile> {
    let Some(files) = event.get("files").and_then(Value::as_array) else {
        return Vec::new();
    };
    files
        .iter()
        .filter_map(|file| {
            let id = str_field(file, "id")?;
            let url = str_field(file, "url_private_download")
                .or_else(|| str_field(file, "url_private"))?;
            Some(SlackFile {
                id: id.to_owned(),
                filename: str_field(file, "name").unwrap_or(id).to_owned(),
                url: url.to_owned(),
                content_type: str_field(file, "mimetype").map(str::to_owned),
                size_bytes: file.get("size").and_then(Value::as_u64),
                local_path: None,
                text_content: None,
            })
        })
        .collect()
}

pub fn attachment_summary(files: &[SlackFile]) -> Option<String> {
    match files {
        [] => None,
        [one] => Some(format!("[Attachment: {}]", one.filename)),
        many => Some(format!(
            "[Attachments: {}]",
            many.iter()
                .map(|f| f.filename.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// omo `forwarded`: shared messages and message unfurls become a `[Forwarded]` block; their
/// files travel with the message.
pub fn forwarded(event: &Value) -> (String, Vec<SlackFile>) {
    let Some(items) = event.get("attachments").and_then(Value::as_array) else {
        return (String::new(), Vec::new());
    };
    let mut parts = Vec::new();
    let mut files = Vec::new();
    for item in items {
        let flagged = |name: &str| item.get(name).and_then(Value::as_bool).unwrap_or(false);
        if !flagged("is_share") && !flagged("is_msg_unfurl") {
            continue;
        }
        let item_files = files_of(item);
        let mut part = str_field(item, "text")
            .or_else(|| str_field(item, "fallback"))
            .unwrap_or_default()
            .trim()
            .to_owned();
        if let Some(summary) = attachment_summary(&item_files) {
            part = if part.is_empty() {
                summary
            } else {
                format!("{part} {summary}")
            };
        }
        if let Some(author) =
            str_field(item, "author_name").or_else(|| str_field(item, "author_subname"))
        {
            if !part.is_empty() {
                part = format!("{author}: {part}");
            }
        }
        if !part.is_empty() {
            parts.push(part);
        }
        files.extend(item_files);
    }
    let block = if parts.is_empty() {
        String::new()
    } else {
        format!("[Forwarded]\n{}", parts.join("\n"))
    };
    (block, files)
}

/// The message body omo routes: text without the mention, then the forwarded block; and its
/// files (forwarded ones included, once).
pub fn content_and_files(event: &Value, text: &str) -> (String, Vec<SlackFile>) {
    let mut content = text.trim().to_owned();
    let (block, forward_files) = forwarded(event);
    if !block.is_empty() {
        content = if content.is_empty() {
            block
        } else {
            format!("{content}\n\n{block}")
        };
    }
    let mut files = files_of(event);
    for file in forward_files {
        if !files.iter().any(|existing| existing.id == file.id) {
            files.push(file);
        }
    }
    (content, files)
}

static ANSI_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]|\x1b\].*?\x07|\x1b[PX^_].*?\x1b\\").unwrap()
});
static SENTINEL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<\|.*?\|>|<\||\|>").unwrap());

fn is_invisible_or_control(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'
            | '\u{200C}'
            | '\u{200D}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{FEFF}'
            | '\u{00AD}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{E0000}'..='\u{E007F}'
            | '\u{E0100}'..='\u{E01EF}'
            | '\u{FE00}'..='\u{FE0E}'
            | '\u{034F}'
            | '\u{180E}'
    )
}

/// omo `neutralize_untrusted_inline_text`: one line, no ANSI, invisible characters, model
/// delimiter sentinels or backticks, at most `max` characters.
pub fn neutralize(text: &str, max: usize) -> String {
    let without_ansi = ANSI_RE.replace_all(text, "");
    let visible: String = without_ansi
        .chars()
        .filter(|c| !is_invisible_or_control(*c))
        .collect();
    let without_sentinels = SENTINEL_RE.replace_all(&visible, " ").replace('`', "'");
    let cleaned: String = without_sentinels
        .chars()
        .map(|c| {
            if c < ' ' || c == '\u{007F}' || ('\u{0080}'..='\u{009F}').contains(&c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if max > 0 && collapsed.chars().count() > max {
        if max <= 3 {
            return collapsed.chars().take(max).collect();
        }
        let prefix: String = collapsed.chars().take(max - 3).collect();
        return format!("{prefix}...");
    }
    collapsed
}

pub fn format_channel_context(messages: &[(String, String)]) -> String {
    let lines: Vec<String> = messages
        .iter()
        .filter_map(|(author, content)| {
            let author = neutralize(author, 64);
            let content = neutralize(content, MAX_CONTEXT_LINE_CHARS);
            (!author.is_empty() && !content.is_empty()).then(|| format!("{author}: {content}"))
        })
        .collect();
    if lines.is_empty() {
        String::new()
    } else {
        format!("[Recent channel context]\n{}", lines.join("\n"))
    }
}

pub fn format_channel_topic_context(topic: Option<&str>) -> String {
    match topic.map(str::trim) {
        Some(topic) if !topic.is_empty() => format!("[Channel Topic]\n{topic}"),
        _ => String::new(),
    }
}

pub fn compose_reply_context(author: &str, referenced: &str, body: &str) -> String {
    let capped = if referenced.chars().count() > REFERENCED_CONTENT_CAP {
        format!(
            "{}...",
            referenced
                .chars()
                .take(REFERENCED_CONTENT_CAP)
                .collect::<String>()
        )
    } else {
        referenced.to_owned()
    };
    let clean = capped.trim();
    match (clean.is_empty(), body.is_empty()) {
        (true, true) => format!("> [Replying to @{author}]"),
        (true, false) => format!("> [Replying to @{author}]\n\n{body}"),
        (false, true) => format!("> [Replying to @{author}]: {clean}"),
        (false, false) => format!("> [Replying to @{author}]: {clean}\n\n{body}"),
    }
}

fn prepend(content: &mut String, block: String) {
    if block.is_empty() {
        return;
    }
    *content = if content.trim().is_empty() {
        block
    } else {
        format!("{block}\n\n{content}")
    };
}

/// Orderable key for a Slack `ts` (`secs.micros`); malformed parts sort as zero.
pub fn ts_key(ts: &str) -> (u64, u32) {
    let (secs, frac) = ts.split_once('.').unwrap_or((ts, ""));
    let micros = format!("{frac:0<6}")
        .get(..6)
        .and_then(|d| d.parse().ok())
        .unwrap_or(0);
    (secs.parse().unwrap_or(0), micros)
}

pub fn ts_to_datetime(ts: &str) -> Option<DateTime<Utc>> {
    let (secs, frac) = ts.split_once('.').unwrap_or((ts, ""));
    let secs = secs.parse::<i64>().ok()?;
    let micros = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<6}").get(..6)?.parse::<u32>().ok()?
    };
    DateTime::from_timestamp(secs, micros * 1_000)
}

/// omo's `SLACK_<NAME>` boolean flags (the Discord fallback names are not read: Discord is off).
pub fn flag(name: &str, default: bool) -> bool {
    match crate::hub_env::var(&format!("SLACK_{name}"))
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("1" | "true" | "yes" | "on") => true,
        Some("0" | "false" | "no" | "off") => false,
        _ => default,
    }
}

/// `SLACK_CHANNEL_CONTEXT_LIMIT` (default 10, at most 25).
pub fn channel_context_limit() -> usize {
    crate::hub_env::var("SLACK_CHANNEL_CONTEXT_LIMIT")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_CHANNEL_CONTEXT_LIMIT)
        .min(MAX_CHANNEL_CONTEXT_LIMIT)
}

async fn user_name(user: &str) -> String {
    match super::slack_web::post_form("users.info", &[("user", user)]).await {
        Ok(info) => [
            "/user/profile/display_name",
            "/user/real_name",
            "/user/name",
        ]
        .iter()
        .find_map(|pointer| text_at(&info, pointer))
        .unwrap_or(user)
        .to_owned(),
        Err(_) => user.to_owned(),
    }
}

fn messages_of(value: &Value) -> Vec<Value> {
    value
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// What omo's `enrich` adds before the turn. `root_known`: the thread already has a session.
pub struct Enrich<'a> {
    pub event: &'a Value,
    pub channel: &'a str,
    pub ts: &'a str,
    pub dm: bool,
    pub bot_user: &'a str,
    pub root_known: bool,
}

pub async fn enrich(input: &Enrich<'_>, content: &mut String, files: &mut Vec<SlackFile>) {
    let thread_ts = super::slack_turn::reply_thread_ts(input.event);
    if let (Some(thread_ts), false) = (thread_ts, input.root_known) {
        add_reply_context(input.channel, thread_ts, content, files).await;
    }
    if input.dm {
        return;
    }
    if flag("CHANNEL_TOPIC_CONTEXT", true) {
        match super::slack_web::post_form("conversations.info", &[("channel", input.channel)]).await
        {
            Ok(info) => {
                let topic = text_at(&info, "/channel/topic/value")
                    .or_else(|| text_at(&info, "/channel/purpose/value"));
                prepend(content, format_channel_topic_context(topic));
            }
            Err(error) => {
                tracing::warn!(%error, channel = input.channel, "failed to fetch Slack channel topic")
            }
        }
    }
    let mentioned = !input.bot_user.is_empty()
        && str_field(input.event, "text")
            .is_some_and(|text| text.contains(&format!("<@{}>", input.bot_user)));
    let limit = channel_context_limit();
    if flag("CHANNEL_CONTEXT", true) && mentioned && limit > 0 {
        let fetched = match thread_ts {
            Some(thread) => {
                let limit = (limit + 1).to_string();
                super::slack_web::post_form(
                    "conversations.replies",
                    &[
                        ("channel", input.channel),
                        ("ts", thread),
                        ("limit", &limit),
                    ],
                )
                .await
            }
            None => {
                let limit = limit.to_string();
                super::slack_web::post_form(
                    "conversations.history",
                    &[
                        ("channel", input.channel),
                        ("latest", input.ts),
                        ("limit", &limit),
                    ],
                )
                .await
            }
        };
        match fetched {
            Ok(value) => {
                let mut messages = messages_of(&value);
                messages
                    .retain(|m| str_field(m, "ts").is_some_and(|t| ts_key(t) < ts_key(input.ts)));
                messages.sort_by_key(|m| ts_key(str_field(m, "ts").unwrap_or_default()));
                let skip = messages.len().saturating_sub(limit);
                let mut history = Vec::new();
                for message in messages.into_iter().skip(skip) {
                    let text = str_field(&message, "text").unwrap_or_default();
                    let author = str_field(&message, "user").unwrap_or_default();
                    if text.trim().is_empty() || author.is_empty() || author == input.bot_user {
                        continue;
                    }
                    history.push((user_name(author).await, text.to_owned()));
                }
                prepend(content, format_channel_context(&history));
            }
            Err(error) => {
                tracing::warn!(%error, channel = input.channel, "failed to fetch recent Slack channel context")
            }
        }
    }
}

async fn add_reply_context(
    channel: &str,
    thread_ts: &str,
    content: &mut String,
    files: &mut Vec<SlackFile>,
) {
    let root = match super::slack_web::thread_root(channel, thread_ts).await {
        Ok(root) => root.filter(|m| str_field(m, "ts") == Some(thread_ts)),
        Err(error) => {
            tracing::warn!(%error, channel, thread_ts, "failed to fetch Slack thread root");
            None
        }
    };
    let Some(root) = root else {
        return;
    };
    let parent_files = files_of(&root);
    let mut quoted = str_field(&root, "text")
        .unwrap_or_default()
        .trim()
        .to_owned();
    if let Some(summary) = attachment_summary(&parent_files) {
        quoted = if quoted.is_empty() {
            summary
        } else {
            format!("{quoted} {summary}")
        };
    }
    let author = match str_field(&root, "user") {
        Some(user) => user_name(user).await,
        None => str_field(&root, "username")
            .or_else(|| text_at(&root, "/bot_profile/name"))
            .unwrap_or("unknown")
            .to_owned(),
    };
    *content = compose_reply_context(&author, &quoted, content);
    for file in parent_files {
        if !files.iter().any(|existing| existing.id == file.id) {
            files.push(file);
        }
    }
}

/// omo's `attachment_dir`: `<workspace root>/attachments/slack` (`OMON_WORKSPACE_ROOT`, default
/// `~/.ferryx/hub/workspace`).
pub fn attachment_dir() -> PathBuf {
    crate::hub_env::var("OMON_WORKSPACE_ROOT")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".ferryx/hub/workspace")
        })
        .join("attachments")
        .join("slack")
}

fn safe_component(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | '\0') {
                '_'
            } else {
                c
            }
        })
        .collect();
    match cleaned.as_str() {
        "" | "." | ".." => "_".to_owned(),
        _ => cleaned,
    }
}

/// omo `is_text_attachment`: text and code documents whose content is inlined.
pub fn is_text_attachment(filename: &str, content_type: Option<&str>) -> bool {
    if let Some(ct) = content_type {
        let lower = ct.to_ascii_lowercase();
        let mime = lower.split(';').next().unwrap_or("").trim();
        if mime.starts_with("text/")
            || matches!(
                mime,
                "application/json"
                    | "application/xml"
                    | "application/javascript"
                    | "application/x-javascript"
                    | "application/typescript"
                    | "application/x-typescript"
                    | "application/x-sh"
                    | "application/x-bash"
                    | "application/x-shellscript"
                    | "application/x-yaml"
                    | "application/yaml"
                    | "application/x-toml"
                    | "application/toml"
                    | "application/sql"
                    | "application/graphql"
                    | "application/x-httpd-php"
            )
            || mime.ends_with("+json")
            || mime.ends_with("+xml")
            || mime.ends_with("+yaml")
        {
            return true;
        }
    }
    let path = Path::new(filename);
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if matches!(
            ext.to_ascii_lowercase().as_str(),
            "txt"
                | "md"
                | "markdown"
                | "csv"
                | "tsv"
                | "log"
                | "json"
                | "jsonl"
                | "ndjson"
                | "xml"
                | "yaml"
                | "yml"
                | "toml"
                | "ini"
                | "cfg"
                | "conf"
                | "env"
                | "properties"
                | "html"
                | "htm"
                | "css"
                | "scss"
                | "sass"
                | "less"
                | "py"
                | "pyi"
                | "js"
                | "mjs"
                | "cjs"
                | "ts"
                | "tsx"
                | "jsx"
                | "sh"
                | "bash"
                | "zsh"
                | "fish"
                | "ps1"
                | "bat"
                | "c"
                | "h"
                | "cpp"
                | "cc"
                | "hpp"
                | "cs"
                | "java"
                | "kt"
                | "go"
                | "rs"
                | "rb"
                | "php"
                | "pl"
                | "lua"
                | "r"
                | "jl"
                | "swift"
                | "m"
                | "scala"
                | "clj"
                | "ex"
                | "exs"
                | "erl"
                | "sql"
                | "graphql"
                | "proto"
                | "tf"
                | "hcl"
                | "dockerfile"
                | "makefile"
                | "cmake"
                | "gradle"
                | "rst"
                | "tex"
                | "srt"
                | "vtt"
                | "diff"
                | "patch"
        ) {
            return true;
        }
    }
    path.file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|name| {
            matches!(
                name.to_ascii_lowercase().as_str(),
                "dockerfile"
                    | "makefile"
                    | "gemfile"
                    | "licence"
                    | "license"
                    | ".env"
                    | ".gitignore"
                    | "cargo.lock"
            )
        })
}

/// omo `is_audio_attachment`.
pub fn is_audio_attachment(content_type: Option<&str>, filename: &str) -> bool {
    if content_type.is_some_and(|ct| ct.starts_with("audio/")) {
        return true;
    }
    let lower = filename.to_lowercase();
    [".ogg", ".mp3", ".m4a", ".wav", ".opus", ".aac", ".flac"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

async fn download(url: &str, dest: &Path) -> Result<(), String> {
    let token = super::slack::bot_token().ok_or("SLACK_BOT_TOKEN is not set")?;
    let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(120));
    if url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost") {
        builder = builder.no_proxy();
    }
    let client = builder.build().map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("http {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|len| len > MAX_DOWNLOAD_BYTES)
    {
        return Err("file is larger than 50 MB".into());
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err("file is larger than 50 MB".into());
    }
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    tokio::fs::write(dest, &bytes)
        .await
        .map_err(|e| e.to_string())
}

async fn run_bounded(
    mut command: tokio::process::Command,
    name: &str,
    timeout: std::time::Duration,
) -> Result<std::process::Output, String> {
    command.kill_on_drop(true);
    match tokio::time::timeout(timeout, command.output()).await {
        Ok(result) => result.map_err(|e| format!("{name} spawn failed: {e}")),
        Err(_) => Err(format!("{name} timed out after {}s", timeout.as_secs())),
    }
}

/// omo's local transcription: ffmpeg to 16 kHz mono wav, then `whisper-cli` with
/// `OMON_WHISPER_MODEL` (default `~/.ferryx/hub/models/ggml-large-v3-turbo.bin`).
pub async fn transcribe_audio(input: &Path) -> Result<String, String> {
    let model = crate::hub_env::var("OMON_WHISPER_MODEL").unwrap_or_else(|_| {
        format!(
            "{}/.ferryx/hub/models/ggml-large-v3-turbo.bin",
            std::env::var("HOME").unwrap_or_default()
        )
    });
    let path_var = std::env::var_os("PATH");
    let on_path = |name: &str| {
        path_var
            .as_deref()
            .is_some_and(|paths| std::env::split_paths(paths).any(|dir| dir.join(name).is_file()))
    };
    if !on_path("ffmpeg") {
        return Err("local transcription unavailable: ffmpeg not found on PATH".into());
    }
    if !on_path("whisper-cli") {
        return Err("local transcription unavailable: whisper-cli not found on PATH".into());
    }
    if !Path::new(&model).is_file() {
        return Err(format!(
            "local transcription unavailable: whisper model not found at {model}"
        ));
    }
    let wav = input.with_extension("16k.wav");
    let mut ffmpeg = tokio::process::Command::new("ffmpeg");
    ffmpeg
        .arg("-i")
        .arg(input)
        .args(["-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"])
        .arg(&wav)
        .arg("-y");
    let converted = run_bounded(ffmpeg, "ffmpeg", std::time::Duration::from_secs(60)).await;
    match converted {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            let _ = tokio::fs::remove_file(&wav).await;
            return Err(format!(
                "ffmpeg failed: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Err(error) => {
            let _ = tokio::fs::remove_file(&wav).await;
            return Err(error);
        }
    }
    let mut whisper = tokio::process::Command::new("whisper-cli");
    whisper
        .args(["-m", &model, "-f"])
        .arg(&wav)
        .args(["--output-txt", "--no-timestamps"]);
    let result = run_bounded(whisper, "whisper-cli", std::time::Duration::from_secs(300)).await;
    let _ = tokio::fs::remove_file(&wav).await;
    let txt = wav.with_extension("wav.txt");
    match result {
        Ok(out) if out.status.success() => {
            if txt.exists() {
                let text = tokio::fs::read_to_string(&txt)
                    .await
                    .map_err(|e| e.to_string())?;
                let _ = tokio::fs::remove_file(&txt).await;
                Ok(text.trim().to_owned())
            } else {
                Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
            }
        }
        Ok(out) => {
            let _ = tokio::fs::remove_file(&txt).await;
            Err(format!(
                "whisper-cli failed: {}",
                String::from_utf8_lossy(&out.stderr)
            ))
        }
        Err(error) => Err(error),
    }
}

/// omo's `hydrate`: downloads every file to `<dir>/<channel>/<ts>-<name>`, inlines small text
/// files and puts audio transcripts in front of the content.
pub async fn hydrate(
    dir: &Path,
    channel: &str,
    ts: &str,
    content: &mut String,
    files: &mut [SlackFile],
) {
    let dir = dir.join(safe_component(channel));
    let mut transcripts = Vec::new();
    for file in files.iter_mut() {
        let dest = dir.join(format!(
            "{}-{}",
            safe_component(ts),
            safe_component(&file.filename)
        ));
        if let Err(error) = download(&file.url, &dest).await {
            tracing::warn!(file = %file.id, %error, "failed to download Slack attachment; routing remote metadata only");
            continue;
        }
        file.local_path = Some(dest.clone());
        if is_audio_attachment(file.content_type.as_deref(), &file.filename) {
            transcripts.push(transcribe_audio(&dest).await);
        } else if is_text_attachment(&file.filename, file.content_type.as_deref())
            && file.size_bytes.unwrap_or(0) <= MAX_INLINED_ATTACHMENT_BYTES
        {
            if let Ok(meta) = tokio::fs::metadata(&dest).await {
                if meta.len() <= MAX_INLINED_ATTACHMENT_BYTES {
                    if let Ok(bytes) = tokio::fs::read(&dest).await {
                        file.text_content = Some(String::from_utf8_lossy(&bytes).into_owned());
                    }
                }
            }
        }
    }
    for transcript in transcripts {
        if let Err(error) = &transcript {
            tracing::warn!(%error, "failed to transcribe Slack audio attachment");
        }
        with_transcript(content, transcript.as_deref().ok());
    }
}

/// Puts an audio transcript (`Some`) in front of `content`, or appends the transcription
/// failure notice (`None`), in the owner's language.
fn with_transcript(content: &mut String, transcript: Option<&str>) {
    let korean = crate::locale::current().is_korean();
    *content = match (transcript, korean) {
        (Some(text), true) => format!("🎤 [음성 전사]: {text}\n\n{content}"),
        (Some(text), false) => format!("🎤 [voice transcript]: {text}\n\n{content}"),
        (None, true) => format!("{content}\n\n_⚠️ 음성 전사 실패 (일반 오디오 첨부로 처리)_"),
        (None, false) => {
            format!("{content}\n\n_⚠️ voice transcription failed (handled as a plain audio attachment)_")
        }
    };
}

/// omo's `render_user_prompt`: the timestamped body, then one line per attachment (with its
/// local path) and inlined text contents.
pub fn render_prompt(
    content: &str,
    files: &[SlackFile],
    received_at: Option<DateTime<Utc>>,
) -> String {
    let body = match received_at {
        Some(at) if !content.trim().is_empty() => {
            format!("[{}] {}", at.format("%a %Y-%m-%d %H:%M:%S UTC"), content)
        }
        _ => content.to_owned(),
    };
    if files.is_empty() {
        return body;
    }
    let attachments = files
        .iter()
        .map(|file| {
            let lower = file.filename.to_ascii_lowercase();
            let voice = ["voice-message", "voice_message", "voice-note", "voice_note"]
                .iter()
                .any(|n| lower.contains(n));
            let label = if voice { "Voice message" } else { "Attachment" };
            let local = file
                .local_path
                .as_ref()
                .map(|path| format!(" | local path: {}", path.display()))
                .unwrap_or_default();
            let mut line = format!(
                "[{label}: {} ({}, {} bytes) - {}{local}]",
                file.filename,
                file.content_type.as_deref().unwrap_or("unknown"),
                file.size_bytes
                    .map_or_else(|| "unknown".to_owned(), |s| s.to_string()),
                file.url,
            );
            if let Some(text) = &file.text_content {
                line.push_str(&format!("\n\n[Content of {}]:\n\n{text}", file.filename));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n");
    if body.trim().is_empty() {
        attachments
    } else {
        format!("{body}\n\n{attachments}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn transcript_markers_are_in_the_owner_language() {
        let mut korean = "body".to_owned();
        with_transcript(&mut korean, Some("hello"));
        assert_eq!(korean, "🎤 [음성 전사]: hello\n\nbody");
        crate::locale::with(crate::locale::OwnerLocale::english_utc(), || {
            let mut heard = "body".to_owned();
            with_transcript(&mut heard, Some("hello"));
            assert_eq!(heard, "🎤 [voice transcript]: hello\n\nbody");
            let mut failed = "body".to_owned();
            with_transcript(&mut failed, None);
            assert_eq!(
                failed,
                "body\n\n_⚠️ voice transcription failed (handled as a plain audio attachment)_"
            );
        });
    }

    #[test]
    fn forwarded_messages_become_a_block_with_their_files() {
        let event = json!({
            "text": "look",
            "files": [{"id": "F1", "name": "a.txt", "url_private": "https://x/a.txt", "mimetype": "text/plain", "size": 3}],
            "attachments": [
                {"is_share": true, "author_name": "Kim", "text": "original", "files": [{"id": "F2", "name": "b.png", "url_private_download": "https://x/b.png"}]},
                {"is_msg_unfurl": true, "fallback": "unfurled"},
                {"text": "a link preview, not a share"}
            ]
        });
        let (content, files) = content_and_files(&event, "look");
        assert_eq!(
            content,
            "look\n\n[Forwarded]\nKim: original [Attachment: b.png]\nunfurled"
        );
        assert_eq!(
            files.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(),
            vec!["F1", "F2"]
        );
        let (only, _) = content_and_files(
            &json!({"attachments": [{"is_share": true, "text": "x"}]}),
            "",
        );
        assert_eq!(only, "[Forwarded]\nx");
    }

    #[test]
    fn context_blocks_match_omo() {
        assert_eq!(
            format_channel_context(&[
                ("Kim".into(), "line\n`two` <|im_start|>".into()),
                ("".into(), "x".into())
            ]),
            "[Recent channel context]\nKim: line 'two'"
        );
        assert_eq!(
            format_channel_topic_context(Some(" deploys ")),
            "[Channel Topic]\ndeploys"
        );
        assert_eq!(format_channel_topic_context(None), "");
        assert_eq!(
            compose_reply_context("Kim", "root", "hi"),
            "> [Replying to @Kim]: root\n\nhi"
        );
        let long = "x".repeat(600);
        assert!(
            compose_reply_context("Kim", &long, "").ends_with(&format!("{}...", "x".repeat(10)))
        );
        assert_eq!(neutralize("a\u{200B}b\x1b[31mc", 0), "abc");
        assert_eq!(neutralize("abcdefgh", 6), "abc...");
    }

    #[test]
    fn prompts_carry_the_time_and_attachments() {
        let files = vec![SlackFile {
            id: "F1".into(),
            filename: "notes.md".into(),
            url: "https://files/notes.md".into(),
            content_type: Some("text/markdown".into()),
            size_bytes: Some(5),
            local_path: Some(PathBuf::from("/tmp/a/notes.md")),
            text_content: Some("hello".into()),
        }];
        let at = ts_to_datetime("1761190800.000100");
        assert_eq!(
            render_prompt("hi", &files, at),
            "[Thu 2025-10-23 03:40:00 UTC] hi\n\n[Attachment: notes.md (text/markdown, 5 bytes) - https://files/notes.md | local path: /tmp/a/notes.md]\n\n[Content of notes.md]:\n\nhello"
        );
        assert_eq!(render_prompt("", &[], at), "");
        assert!(ts_key("1.5") > ts_key("1.10"));
        assert!(is_text_attachment("Dockerfile", None));
        assert!(is_text_attachment(
            "x.bin",
            Some("application/json; charset=utf-8")
        ));
        assert!(!is_text_attachment("x.png", Some("image/png")));
        assert!(is_audio_attachment(None, "voice.m4a"));
        assert_eq!(safe_component("../a/b"), ".._a_b");
    }
}
