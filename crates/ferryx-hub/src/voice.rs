//! Speech settings and the Slack huddle sidecar. Network calls compile with `hub-voice`.
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_OPENAI_BASE: &str = "https://api.openai.com/v1";
const DEFAULT_STT_MODEL: &str = "whisper-1";
const DEFAULT_TTS_MODEL: &str = "tts-1";
const DEFAULT_TTS_VOICE: &str = "alloy";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpeechEndpoint {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceSettings {
    enabled: bool,
    stt: Option<SpeechEndpoint>,
    tts: Option<SpeechEndpoint>,
    tts_voice: String,
    slack_voice_reply: bool,
    silence: Duration,
    min_speech: Duration,
    max_speech: Duration,
}

impl VoiceSettings {
    pub fn from_lookup(lookup: &dyn Fn(&str) -> Option<String>) -> Self {
        let speech_key = first(
            lookup,
            &["OMON_STT_API_KEY", "OMON_TTS_API_KEY", "OPENAI_API_KEY"],
        );
        let speech_base = first(
            lookup,
            &["OMON_STT_BASE_URL", "OMON_TTS_BASE_URL", "OPENAI_API_BASE"],
        )
        .unwrap_or_else(|| DEFAULT_OPENAI_BASE.to_owned());
        let stt_key = first(lookup, &["OMON_STT_API_KEY"]).or_else(|| speech_key.clone());
        let tts_key = first(lookup, &["OMON_TTS_API_KEY"]).or(speech_key);
        let stt_base = first(lookup, &["OMON_STT_BASE_URL"]).unwrap_or_else(|| speech_base.clone());
        let tts_base = first(lookup, &["OMON_TTS_BASE_URL"]).unwrap_or(speech_base);
        let explicit = first(lookup, &["OMON_VOICE_ENABLED"]);
        let enabled = match explicit.as_deref().map(|value| value.to_ascii_lowercase()) {
            Some(value) if matches!(value.as_str(), "1" | "true" | "yes" | "on") => true,
            Some(value) if matches!(value.as_str(), "0" | "false" | "no" | "off") => false,
            _ => stt_key.is_some() && tts_key.is_some(),
        };
        Self {
            enabled,
            stt: stt_key.map(|api_key| SpeechEndpoint {
                api_key,
                base_url: stt_base,
                model: first(lookup, &["OMON_STT_MODEL"])
                    .unwrap_or_else(|| DEFAULT_STT_MODEL.to_owned()),
            }),
            tts: tts_key.map(|api_key| SpeechEndpoint {
                api_key,
                base_url: tts_base,
                model: first(lookup, &["OMON_TTS_MODEL"])
                    .unwrap_or_else(|| DEFAULT_TTS_MODEL.to_owned()),
            }),
            tts_voice: first(lookup, &["OMON_TTS_VOICE"])
                .unwrap_or_else(|| DEFAULT_TTS_VOICE.to_owned()),
            slack_voice_reply: flag(lookup, "SLACK_VOICE_REPLY"),
            silence: duration_ms(lookup, "OMON_VOICE_SILENCE_MS", 700),
            min_speech: duration_ms(lookup, "OMON_VOICE_MIN_SPEECH_MS", 400),
            max_speech: duration_ms(lookup, "OMON_VOICE_MAX_SPEECH_MS", 30_000),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn ready(&self) -> bool {
        self.enabled && self.stt.is_some() && self.tts.is_some()
    }

    pub fn stt(&self) -> Option<&SpeechEndpoint> {
        self.stt.as_ref()
    }

    pub fn tts(&self) -> Option<&SpeechEndpoint> {
        self.tts.as_ref()
    }

    pub fn tts_voice(&self) -> &str {
        &self.tts_voice
    }

    pub fn slack_voice_reply(&self) -> bool {
        self.slack_voice_reply && self.tts.is_some()
    }

    pub fn silence(&self) -> Duration {
        self.silence
    }

    pub fn min_speech(&self) -> Duration {
        self.min_speech
    }

    pub fn max_speech(&self) -> Duration {
        self.max_speech
    }

    /// Why `/voice join` cannot start, when [`Self::ready`] is false.
    pub fn unavailable_reason(&self) -> Option<&'static str> {
        if self.ready() {
            return None;
        }
        if !self.enabled {
            return Some("Discord voice is disabled (OMON_VOICE_ENABLED=false).");
        }
        if self.stt.is_none() || self.tts.is_none() {
            return Some(
                "Discord voice needs an API key. Set OPENAI_API_KEY, or OMON_STT_API_KEY and OMON_TTS_API_KEY.",
            );
        }
        None
    }
}

/// Prefixes a transcript so the agent session can tell speech from typed text.
pub fn format_voice_transcript(speaker: Option<&str>, transcript: &str) -> String {
    let transcript = transcript.trim();
    if transcript.is_empty() {
        return String::new();
    }
    match speaker.map(str::trim).filter(|speaker| !speaker.is_empty()) {
        Some(speaker) => format!("[voice] {speaker}: {transcript}"),
        None => format!("[voice] {transcript}"),
    }
}

pub fn stt_url(endpoint: &SpeechEndpoint) -> String {
    format!(
        "{}/audio/transcriptions",
        endpoint.base_url.trim_end_matches('/')
    )
}

pub fn tts_url(endpoint: &SpeechEndpoint) -> String {
    format!("{}/audio/speech", endpoint.base_url.trim_end_matches('/'))
}

pub fn sidecar_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sidecar/slack-huddle/index.mjs")
}

/// The sidecar is a bun program. It reads `OMO_HUDDLE_JOIN_TIMEOUT_MS` and `OMO_HUDDLE_MIC_LABEL`.
pub fn huddle_argv(sidecar: &Path) -> Vec<String> {
    vec!["bun".to_owned(), sidecar.display().to_string()]
}

pub const OPENAI_PCM_RATE: u32 = 24_000;
pub const OPENAI_PCM_CHANNELS: u16 = 1;
pub const SPEECH_CHAR_LIMIT: usize = 4_000;

/// Writes a canonical PCM WAV. OpenAI `pcm` is 24 kHz, 16-bit, mono, little-endian.
pub fn pcm_to_wav(sample_rate: u32, channels: u16, samples: &[i16]) -> Result<Vec<u8>, String> {
    if channels == 0 || sample_rate == 0 {
        return Err("WAV sample rate and channel count must be non-zero".into());
    }
    let data_bytes = samples
        .len()
        .checked_mul(2)
        .ok_or_else(|| "PCM sample buffer is too large for a WAV header".to_owned())?;
    let data_size = u32::try_from(data_bytes)
        .map_err(|_| "PCM sample buffer is too large for a WAV header".to_owned())?;
    let mut wav = Vec::with_capacity(44 + data_bytes);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_size).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * u32::from(channels) * 2).to_le_bytes());
    wav.extend_from_slice(&(channels * 2).to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    Ok(wav)
}

pub fn pcm_bytes_to_samples(bytes: &[u8]) -> Vec<i16> {
    let mut samples = Vec::with_capacity(bytes.len() / 2);
    let mut rest = bytes;
    while rest.len() >= 2 {
        samples.push(i16::from_le_bytes([rest[0], rest[1]]));
        rest = &rest[2..];
    }
    samples
}

/// Strips fences, links, and markdown chrome, then clips to the speech limit.
pub fn spoken_text(raw: &str) -> String {
    let without_think = strip_tag(raw, "think");
    if without_think.trim().is_empty() || is_silence(without_think.trim()) {
        return String::new();
    }
    let without_fences = strip_fenced_code(&without_think);
    let without_links = strip_markdown_links(&without_fences);
    let plain: String = without_links
        .chars()
        .filter(|ch| !matches!(ch, '*' | '_' | '`' | '#' | '>'))
        .collect();
    let collapsed = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() || is_silence(&collapsed) {
        return String::new();
    }
    truncate_chars(&collapsed, SPEECH_CHAR_LIMIT)
}

fn is_silence(text: &str) -> bool {
    matches!(text, "NO_REPLY" | "[silence]" | "…")
}

fn strip_tag(input: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut rest = input;
    let mut out = String::new();
    while let Some(start) = rest.find(&open) {
        out.push_str(&rest[..start]);
        let after = &rest[start + open.len()..];
        match after.find(&close) {
            Some(end) => rest = &after[end + close.len()..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

fn strip_fenced_code(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("```") {
        out.push_str(&rest[..start]);
        out.push(' ');
        let after = &rest[start + 3..];
        match after.find("```") {
            Some(end) => rest = &after[end + 3..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

fn strip_markdown_links(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('[') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(label_end) = after.find("](") else {
            out.push('[');
            rest = after;
            continue;
        };
        let label = &after[..label_end];
        let after_label = &after[label_end + 2..];
        let Some(url_end) = after_label.find(')') else {
            out.push('[');
            rest = after;
            continue;
        };
        out.push_str(label);
        rest = &after_label[url_end + 1..];
    }
    out.push_str(rest);
    out
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(limit.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// Audio message timestamps (`channel:ts`) whose agent reply should be spoken back.
#[derive(Clone, Default)]
pub struct SlackVoiceReplies {
    pending: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
}

impl SlackVoiceReplies {
    pub fn note(&self, channel: &str, ts: &str) {
        if channel.is_empty() || ts.is_empty() {
            return;
        }
        self.pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(format!("{channel}:{ts}"));
    }

    /// Claims one pending audio message. A later duplicate reply does not upload again.
    pub fn take(&self, channel: &str, reply_to: Option<&str>) -> bool {
        let Some(ts) = reply_to.filter(|ts| !ts.is_empty()) else {
            return false;
        };
        self.pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&format!("{channel}:{ts}"))
    }
}

pub fn pending_voice_replies() -> &'static SlackVoiceReplies {
    static REPLIES: std::sync::OnceLock<SlackVoiceReplies> = std::sync::OnceLock::new();
    REPLIES.get_or_init(SlackVoiceReplies::default)
}

/// `POST {base}/audio/speech` with `response_format: pcm`. Empty spoken text does not call out.
#[cfg(feature = "hub-voice")]
pub async fn synthesize_pcm(
    endpoint: &SpeechEndpoint,
    voice: &str,
    text: &str,
) -> Result<Vec<u8>, String> {
    let text = spoken_text(text);
    if text.is_empty() {
        return Ok(Vec::new());
    }
    #[derive(serde::Serialize)]
    struct Request<'a> {
        model: &'a str,
        input: &'a str,
        voice: &'a str,
        response_format: &'static str,
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .post(tts_url(endpoint))
        .bearer_auth(&endpoint.api_key)
        .json(&Request {
            model: &endpoint.model,
            input: &text,
            voice,
            response_format: "pcm",
        })
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("tts {status}: {body}"));
    }
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| error.to_string())
}

fn first(lookup: &dyn Fn(&str) -> Option<String>, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        lookup(name)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

fn flag(lookup: &dyn Fn(&str) -> Option<String>, name: &str) -> bool {
    matches!(
        lookup(name).as_deref().map(str::trim).map(|value| value.to_ascii_lowercase()),
        Some(value) if matches!(value.as_str(), "1" | "true" | "yes" | "on")
    )
}

fn duration_ms(lookup: &dyn Fn(&str) -> Option<String>, name: &str, default_ms: u64) -> Duration {
    let ms = lookup(name)
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(default_ms);
    Duration::from_millis(ms)
}

#[cfg(feature = "hub-voice")]
pub async fn transcribe(endpoint: &SpeechEndpoint, wav: &[u8]) -> Result<String, String> {
    let client = reqwest::Client::new();
    let part = reqwest::multipart::Part::bytes(wav.to_vec())
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|error| error.to_string())?;
    let form = reqwest::multipart::Form::new()
        .text("model", endpoint.model.clone())
        .part("file", part);
    let response = client
        .post(stt_url(endpoint))
        .bearer_auth(&endpoint.api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let body: serde_json::Value = response.json().await.map_err(|error| error.to_string())?;
    body.get("text")
        .and_then(|value| value.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "stt response has no text".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_endpoints_follow_the_gateway_env_and_slack_voice_needs_tts() {
        let settings = VoiceSettings::from_lookup(&|name| match name {
            "OMON_STT_API_KEY" => Some("stt-key".into()),
            "OMON_TTS_API_KEY" => Some("tts-key".into()),
            "OMON_STT_BASE_URL" => Some("https://stt.example/v1".into()),
            "OMON_TTS_BASE_URL" => Some("https://speech.example/v1".into()),
            "SLACK_VOICE_REPLY" => Some("true".into()),
            "OMON_VOICE_SILENCE_MS" => Some("900".into()),
            _ => None,
        });
        assert!(settings.ready());
        assert!(settings.slack_voice_reply());
        assert_eq!(settings.silence(), Duration::from_millis(900));
        assert_eq!(
            stt_url(settings.stt().unwrap()),
            "https://stt.example/v1/audio/transcriptions"
        );
        assert_eq!(
            tts_url(settings.tts().unwrap()),
            "https://speech.example/v1/audio/speech"
        );
        assert_eq!(settings.tts_voice(), "alloy");
        let off = VoiceSettings::from_lookup(&|name| match name {
            "OPENAI_API_KEY" => Some("key".into()),
            "OMON_VOICE_ENABLED" => Some("off".into()),
            _ => None,
        });
        assert!(!off.enabled());
        let argv = huddle_argv(Path::new("sidecar/slack-huddle/index.mjs"));
        assert_eq!(argv[0], "bun");
        assert!(argv[1].ends_with("index.mjs"));
        let replies = SlackVoiceReplies::default();
        replies.note("C1", "1700.0001");
        assert!(replies.take("C1", Some("1700.0001")));
        assert!(!replies.take("C1", Some("1700.0001")));
        assert!(!replies.take("C1", None));
        replies.note("", "1");
        assert!(!replies.take("", Some("1")));
        assert_eq!(
            spoken_text("See [docs](https://example.com).\n```rust\npanic!()\n```\n**Done**"),
            "See docs. Done"
        );
        assert!(spoken_text("<think>hidden</think>").is_empty());
        let wav = pcm_to_wav(16_000, 1, &[1, -2]).unwrap();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(i16::from_le_bytes([wav[44], wav[45]]), 1);
        assert_eq!(
            format_voice_transcript(Some("<@7>"), " hello "),
            "[voice] <@7>: hello"
        );
        assert!(format_voice_transcript(None, "  ").is_empty());
        assert_eq!(
            off.unavailable_reason(),
            Some("Discord voice is disabled (OMON_VOICE_ENABLED=false).")
        );
        let missing =
            VoiceSettings::from_lookup(&|name| (name == "OMON_VOICE_ENABLED").then(|| "on".into()));
        assert_eq!(
            missing.unavailable_reason(),
            Some(
                "Discord voice needs an API key. Set OPENAI_API_KEY, or OMON_STT_API_KEY and OMON_TTS_API_KEY."
            )
        );
    }

    #[cfg(feature = "hub-voice")]
    #[tokio::test]
    async fn synthesize_requests_pcm_and_wraps_a_wav() {
        let body = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::<u8>::new()));
        let recorded = body.clone();
        let app = axum::Router::new().route(
            "/audio/speech",
            axum::routing::post(|bytes: axum::body::Bytes| async move {
                *recorded.lock().await = bytes.to_vec();
                axum::body::Bytes::from(vec![0x01, 0x00, 0xff, 0xff])
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let endpoint = SpeechEndpoint {
            api_key: "tts-secret".into(),
            base_url: format!("http://{address}"),
            model: "tts-1".into(),
        };
        let pcm = synthesize_pcm(&endpoint, "nova", "Hello **there**")
            .await
            .unwrap();
        let request: serde_json::Value = serde_json::from_slice(&body.lock().await).unwrap();
        assert_eq!(request["response_format"], "pcm");
        assert_eq!(request["input"], "Hello there");
        assert_eq!(request["voice"], "nova");
        let samples = pcm_bytes_to_samples(&pcm);
        assert_eq!(samples, vec![1, -1]);
        let wav = pcm_to_wav(OPENAI_PCM_RATE, OPENAI_PCM_CHANNELS, &samples).unwrap();
        assert_eq!(&wav[8..12], b"WAVE");
        assert!(synthesize_pcm(&endpoint, "nova", "   ")
            .await
            .unwrap()
            .is_empty());
    }
}
