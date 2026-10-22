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
    }
}
