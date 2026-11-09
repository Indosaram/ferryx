//! Speech in and out for the Supervisor voice call: the microphone recording becomes text, and
//! the supervisor's own receipt becomes audio. Both use the hub's speech settings and the same
//! provider calls the Discord huddle uses (`ferryx_hub::voice`), so one key configures both.
//!
//! Nothing here touches the daemon. A transcript goes back to the UI, which sends it through
//! `cmd_supervisor_request` exactly like a typed message, and the spoken reply is the receipt of
//! that request. The surface never invents an answer the hub did not give.

use super::error::{IpcError, IpcErrorCode};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;

/// A recording larger than this is refused before it is decoded.
const MAX_WAV_BYTES: usize = 8 * 1024 * 1024;

/// Spoken replies are clipped well below the provider limit: a receipt is a sentence or two, and
/// a four-minute answer would cross the IPC boundary as a multi-megabyte WAV.
const SPEECH_LIMIT: usize = 600;

const UNAVAILABLE: &str = "VOICE_UNAVAILABLE";
const INPUT_INVALID: &str = "VOICE_INPUT_INVALID";
const PROVIDER_FAILED: &str = "VOICE_PROVIDER_FAILED";

fn refusal(code: &str, message: impl Into<String>) -> IpcError {
    IpcError::new(IpcErrorCode::Custom(code.to_owned()), message)
}

/// Why the call cannot start, in this surface's words. `None` only when speech works.
///
/// Kept as a pure function of the settings' flags so the three reasons are testable without a
/// provider, and so the Discord wording in `VoiceSettings::unavailable_reason` is never shown here.
fn unavailability(built: bool, enabled: bool, has_stt: bool, has_codex: bool) -> Option<&'static str> {
    if !built {
        return Some("This build has no speech support. Type the request instead.");
    }
    if !enabled {
        return Some("Voice is switched off. Set OMON_VOICE_ENABLED=1 to talk to the supervisor.");
    }
    if !has_stt && !has_codex {
        return Some(
            "Voice needs a speech key or a Codex sign-in. Set OPENAI_API_KEY, or run `codex login` to dictate free.",
        );
    }
    None
}

/// Spoken replies are the only part that needs a paid key: the Codex sign-in covers dictation but
/// exposes no speech synthesis, so a keyless machine can hear nothing back yet.
fn speak_unavailability(built: bool, enabled: bool, has_tts: bool) -> Option<&'static str> {
    if !built {
        return Some("This build has no speech support.");
    }
    if !enabled {
        return Some("Voice is switched off. Set OMON_VOICE_ENABLED=1 to talk to the supervisor.");
    }
    if !has_tts {
        return Some("Spoken replies need a speech key. Set OPENAI_API_KEY or OMON_TTS_API_KEY.");
    }
    None
}

/// Clips spoken text to `limit` characters at a sentence end, else a word end, so a receipt is
/// never cut mid-word.
fn clip_for_speech(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let head: String = text.chars().take(limit).collect();
    let sentence = head
        .char_indices()
        .rev()
        .find(|(_, ch)| matches!(ch, '.' | '!' | '?'))
        .map(|(index, ch)| index + ch.len_utf8());
    if let Some(end) = sentence {
        return head[..end].trim_end().to_owned();
    }
    match head.rfind(char::is_whitespace) {
        Some(end) if end > 0 => head[..end].trim_end().to_owned(),
        _ => head,
    }
}

/// The recording as bytes, refused unless it is a RIFF/WAVE file of sane size.
fn decode_wav(wav_base64: &str) -> Result<Vec<u8>, IpcError> {
    // Base64 is 4 bytes of text per 3 bytes of data; refuse before allocating for a huge upload.
    if wav_base64.len() > MAX_WAV_BYTES / 3 * 4 + 4 {
        return Err(refusal(
            INPUT_INVALID,
            format!("The recording is larger than {} MiB.", MAX_WAV_BYTES / (1024 * 1024)),
        ));
    }
    let bytes = STANDARD
        .decode(wav_base64.as_bytes())
        .map_err(|_| refusal(INPUT_INVALID, "The recording was not valid base64."))?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(refusal(
            INPUT_INVALID,
            "The recording was not a WAV file.",
        ));
    }
    Ok(bytes)
}

/// The call's capability as the surface needs it before it asks for the microphone.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceCallStatus {
    pub available: bool,
    pub reason: Option<&'static str>,
    /// Which credential pays for dictation, so the surface can say "free" without guessing.
    pub stt_source: Option<&'static str>,
    /// Dictation can work while spoken replies cannot: the Codex sign-in has no synthesis.
    pub speak_available: bool,
    pub speak_reason: Option<&'static str>,
    pub stt_model: Option<String>,
    pub tts_model: Option<String>,
    pub tts_voice: String,
    /// The rate and channel count the surface records and the provider returns.
    pub sample_rate: u32,
    pub channels: u16,
    pub max_utterance_ms: u64,
    pub min_speech_ms: u64,
    pub silence_ms: u64,
}

/// One spoken reply as a playable WAV, base64 for the IPC boundary.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpokenAudio {
    /// Empty when the text had nothing speakable; the surface then plays nothing.
    pub audio_base64: String,
    pub mime_type: &'static str,
    pub characters: usize,
    pub duration_ms: u64,
}

fn settings() -> ferryx_hub::voice::VoiceSettings {
    // The hub env file is read fresh, not from the daemon's cached copy: a key the owner just
    // saved has to work on the next call without restarting the daemon.
    let file = ferryx_hub::hub_env::read_file_env();
    ferryx_hub::voice::VoiceSettings::from_lookup(&|key| {
        std::env::var(key).ok().or_else(|| file.get(key).cloned())
    })
}

#[cfg(all(test, feature = "hub-voice"))]
mod live {
    use super::*;

    /// The whole call in one hop: the provider speaks a sentence, and the same sentence comes
    /// back as text through the two commands the surface actually invokes. Ignored by default
    /// because it spends provider credit and needs real credentials; run it with
    /// `cargo test --lib voice_call::live -- --ignored`.
    #[tokio::test]
    #[ignore = "live: needs real STT/TTS credentials"]
    async fn a_spoken_sentence_transcribes_back_to_itself() {
        let sentence = "The supervisor voice call is working correctly.";
        let spoken = cmd_voice_call_speak(sentence.to_owned())
            .await
            .expect("the provider should synthesize the sentence");
        assert!(
            spoken.audio_base64.len() > 1_000,
            "a real clip is expected, got {} base64 bytes",
            spoken.audio_base64.len()
        );
        assert!(spoken.duration_ms > 500, "clip was {} ms", spoken.duration_ms);

        let heard = cmd_voice_call_transcribe(spoken.audio_base64.clone())
            .await
            .expect("the provider should transcribe the clip it just spoke");
        let text = heard["text"].as_str().unwrap_or_default().to_lowercase();
        assert!(
            text.contains("supervisor") && text.contains("working"),
            "transcript was {text:?}"
        );
    }
}

#[tauri::command]
pub fn cmd_voice_call_status() -> VoiceCallStatus {
    let settings = settings();
    let codex = codex_dictation_available();
    let speak_reason = speak_unavailability(
        cfg!(feature = "hub-voice"),
        voice_enabled(&settings),
        settings.tts().is_some(),
    );
    let reason = unavailability(
        cfg!(feature = "hub-voice"),
        voice_enabled(&settings),
        settings.stt().is_some(),
        codex,
    );
    VoiceCallStatus {
        available: reason.is_none(),
        reason,
        stt_source: if settings.stt().is_some() {
            Some("key")
        } else if codex {
            Some("codex")
        } else {
            None
        },
        speak_available: speak_reason.is_none(),
        speak_reason,
        stt_model: settings.stt().map(|endpoint| endpoint.model.clone()),
        tts_model: settings.tts().map(|endpoint| endpoint.model.clone()),
        tts_voice: settings.tts_voice().to_owned(),
        sample_rate: ferryx_hub::voice::OPENAI_PCM_RATE,
        channels: ferryx_hub::voice::OPENAI_PCM_CHANNELS,
        max_utterance_ms: settings.max_speech().as_millis() as u64,
        min_speech_ms: settings.min_speech().as_millis() as u64,
        silence_ms: settings.silence().as_millis() as u64,
    }
}

/// Where the owner's speech key lives: the hub env file, never an IPC response.
const SPEECH_KEY_VAR: &str = "OPENAI_API_KEY";

/// Whether a speech key is saved, without reading or returning it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechKeyState {
    pub saved: bool,
    /// `file` when the owner saved one, `environment` when only the process has one.
    pub source: Option<&'static str>,
}

#[tauri::command]
pub fn cmd_voice_call_key_state() -> SpeechKeyState {
    let file = ferryx_hub::hub_env::read_file_env();
    if file.get(SPEECH_KEY_VAR).is_some_and(|value| !value.trim().is_empty()) {
        return SpeechKeyState { saved: true, source: Some("file") };
    }
    if std::env::var(SPEECH_KEY_VAR).is_ok_and(|value| !value.trim().is_empty()) {
        return SpeechKeyState { saved: true, source: Some("environment") };
    }
    SpeechKeyState { saved: false, source: None }
}

/// Saves the owner's speech key for the next call. An empty key removes it.
#[tauri::command]
pub fn cmd_voice_call_set_key(api_key: String) -> Result<SpeechKeyState, IpcError> {
    let trimmed = api_key.trim();
    let mut entries = std::collections::BTreeMap::new();
    entries.insert(SPEECH_KEY_VAR.to_owned(), trimmed.to_owned());
    ferryx_hub::hub_env::set_file_entries(&entries)
        .map_err(|error| IpcError::internal(error.to_string()))?;
    Ok(cmd_voice_call_key_state())
}

/// One utterance as text. The caller records at `VoiceCallStatus::sample_rate`.
#[tauri::command]
pub async fn cmd_voice_call_transcribe(wav_base64: String) -> Result<serde_json::Value, IpcError> {
    let settings = settings();
    if let Some(reason) = unavailability(
        cfg!(feature = "hub-voice"),
        voice_enabled(&settings),
        settings.stt().is_some(),
        codex_dictation_available(),
    ) {
        return Err(refusal(UNAVAILABLE, reason));
    }
    let wav = decode_wav(&wav_base64)?;
    // A saved key wins: the owner paid for it and expects its model. The Codex sign-in is the
    // fallback that makes dictation work on a machine with no key at all.
    let (text, source) = if settings.stt().is_some() {
        (transcribe_wav(&settings, &wav).await?, "key")
    } else {
        (transcribe_with_codex(&wav).await?, "codex")
    };
    Ok(serde_json::json!({ "text": text.trim(), "source": source }))
}

/// Whether this machine's Codex sign-in can dictate, without reading the token out.
fn codex_dictation_available() -> bool {
    cfg!(feature = "hub-voice") && ferryx_hub::codex_speech::available()
}

/// Enabled when a key is configured, or when a Codex sign-in can dictate instead. The hub
/// derives `enabled` from the presence of a key, so a keyless machine with a Codex login would
/// otherwise be told voice is switched off; an explicit `OMON_VOICE_ENABLED=0` still wins.
fn voice_enabled(settings: &ferryx_hub::voice::VoiceSettings) -> bool {
    if settings.enabled() {
        return true;
    }
    if !codex_dictation_available() {
        return false;
    }
    let file = ferryx_hub::hub_env::read_file_env();
    let explicit = std::env::var("OMON_VOICE_ENABLED")
        .ok()
        .or_else(|| file.get("OMON_VOICE_ENABLED").cloned());
    !matches!(
        explicit.as_deref().map(str::to_ascii_lowercase).as_deref(),
        Some("0") | Some("false") | Some("no") | Some("off")
    )
}

#[cfg(feature = "hub-voice")]
async fn transcribe_with_codex(wav: &[u8]) -> Result<String, IpcError> {
    ferryx_hub::codex_speech::transcribe(wav)
        .await
        .map_err(|error| refusal(PROVIDER_FAILED, format!("Speech to text failed: {error}")))
}

#[cfg(not(feature = "hub-voice"))]
async fn transcribe_with_codex(_wav: &[u8]) -> Result<String, IpcError> {
    Err(refusal(UNAVAILABLE, "This build has no speech support."))
}

#[cfg(feature = "hub-voice")]
async fn transcribe_wav(
    settings: &ferryx_hub::voice::VoiceSettings,
    wav: &[u8],
) -> Result<String, IpcError> {
    let endpoint = settings
        .stt()
        .ok_or_else(|| refusal(UNAVAILABLE, "No speech-to-text endpoint is configured."))?;
    ferryx_hub::voice::transcribe(endpoint, wav)
        .await
        .map_err(|error| refusal(PROVIDER_FAILED, format!("Speech to text failed: {error}")))
}

#[cfg(not(feature = "hub-voice"))]
async fn transcribe_wav(
    _settings: &ferryx_hub::voice::VoiceSettings,
    _wav: &[u8],
) -> Result<String, IpcError> {
    Err(refusal(UNAVAILABLE, "This build has no speech support."))
}

/// The reply to speak, as a WAV the surface plays from a blob URL.
#[tauri::command]
pub async fn cmd_voice_call_speak(text: String) -> Result<SpokenAudio, IpcError> {
    let settings = settings();
    if let Some(reason) = speak_unavailability(
        cfg!(feature = "hub-voice"),
        voice_enabled(&settings),
        settings.tts().is_some(),
    ) {
        return Err(refusal(UNAVAILABLE, reason));
    }
    let spoken = ferryx_hub::voice::spoken_text(&text);
    if spoken.is_empty() {
        return Ok(SpokenAudio {
            audio_base64: String::new(),
            mime_type: "audio/wav",
            characters: 0,
            duration_ms: 0,
        });
    }
    let clipped = clip_for_speech(&spoken, SPEECH_LIMIT);
    let pcm = speak_pcm(&settings, &clipped).await?;
    let samples = ferryx_hub::voice::pcm_bytes_to_samples(&pcm);
    let rate = ferryx_hub::voice::OPENAI_PCM_RATE;
    let wav = ferryx_hub::voice::pcm_to_wav(rate, ferryx_hub::voice::OPENAI_PCM_CHANNELS, &samples)
        .map_err(|error| refusal(PROVIDER_FAILED, error))?;
    let duration_ms = if rate == 0 {
        0
    } else {
        (samples.len() as u64 * 1_000) / u64::from(rate)
    };
    Ok(SpokenAudio {
        audio_base64: STANDARD.encode(&wav),
        mime_type: "audio/wav",
        characters: clipped.chars().count(),
        duration_ms,
    })
}

#[cfg(feature = "hub-voice")]
async fn speak_pcm(
    settings: &ferryx_hub::voice::VoiceSettings,
    text: &str,
) -> Result<Vec<u8>, IpcError> {
    let endpoint = settings
        .tts()
        .ok_or_else(|| refusal(UNAVAILABLE, "No text-to-speech endpoint is configured."))?;
    ferryx_hub::voice::synthesize_pcm(endpoint, settings.tts_voice(), text)
        .await
        .map_err(|error| refusal(PROVIDER_FAILED, format!("Text to speech failed: {error}")))
}

#[cfg(not(feature = "hub-voice"))]
async fn speak_pcm(
    _settings: &ferryx_hub::voice::VoiceSettings,
    _text: &str,
) -> Result<Vec<u8>, IpcError> {
    Err(refusal(UNAVAILABLE, "This build has no speech support."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ready_surface_has_no_reason() {
        assert_eq!(unavailability(true, true, true, true), None);
    }

    #[test]
    fn each_unready_state_names_its_own_fix() {
        let missing_build = unavailability(false, true, true, true).unwrap();
        assert!(missing_build.contains("no speech support"));

        let switched_off = unavailability(true, false, true, true).unwrap();
        assert!(switched_off.contains("OMON_VOICE_ENABLED"));

        let no_key = unavailability(true, true, false, false).unwrap();
        assert!(no_key.contains("OPENAI_API_KEY"));

        // The Discord wording must never reach this surface.
        for reason in [missing_build, switched_off, no_key] {
            assert!(!reason.contains("Discord"), "{reason}");
        }
    }

    #[test]
    fn a_half_configured_endpoint_still_reports_the_key() {
        assert!(unavailability(true, true, false, false).is_some());
    }

    #[test]
    fn a_codex_sign_in_alone_makes_dictation_available() {
        assert_eq!(unavailability(true, true, false, true), None);
        assert_eq!(unavailability(true, true, true, false), None);
    }

    /// The env var is a race with any other test that reads the hub env file, so this test only
    /// asserts the precedence rule it owns: an explicit off switches a keyless machine off too.
    #[test]
    fn an_explicit_off_switches_voice_off_without_a_key() {
        let settings = ferryx_hub::voice::VoiceSettings::from_lookup(&|name| match name {
            "OMON_VOICE_ENABLED" => Some("0".into()),
            _ => None,
        });
        assert!(!settings.enabled());
        // With no key and no sign-in the reason is the key, not a stale "switched off" line.
        assert!(unavailability(true, false, false, false).unwrap().contains("OMON_VOICE_ENABLED"));
    }

    #[test]
    fn spoken_replies_still_need_a_paid_key() {
        let no_tts = speak_unavailability(true, true, false).unwrap();
        assert!(no_tts.contains("OPENAI_API_KEY"));
        assert_eq!(speak_unavailability(true, true, true), None);
        assert!(speak_unavailability(false, true, true).is_some());
        assert!(speak_unavailability(true, false, true).is_some());
    }

    #[test]
    fn short_speech_is_unchanged() {
        assert_eq!(clip_for_speech("Done.", 600), "Done.");
    }

    #[test]
    fn long_speech_clips_at_a_sentence_end() {
        let text = "The worker finished the first requirement. The second one is still running.";
        let clipped = clip_for_speech(text, 45);
        assert_eq!(clipped, "The worker finished the first requirement.");
        assert!(!clipped.ends_with(' '));
    }

    #[test]
    fn speech_without_a_sentence_end_clips_at_a_word() {
        let clipped = clip_for_speech("alpha beta gamma delta", 12);
        assert_eq!(clipped, "alpha beta");
    }

    #[test]
    fn a_single_long_word_is_hard_cut() {
        assert_eq!(clip_for_speech("abcdefghijklmnop", 5), "abcde");
    }

    #[test]
    fn clipping_counts_characters_not_bytes() {
        let clipped = clip_for_speech("한국어 문장입니다. 두 번째 문장입니다.", 12);
        assert!(clipped.chars().count() <= 12);
        assert_eq!(clipped, "한국어 문장입니다.");
    }

    #[test]
    fn a_non_wav_upload_is_refused() {
        let error = decode_wav(&STANDARD.encode(b"not a wave file at all, definitely not")).unwrap_err();
        assert_eq!(error.code, IpcErrorCode::Custom(INPUT_INVALID.to_owned()));
    }

    #[test]
    fn a_truncated_wav_header_is_refused() {
        let error = decode_wav(&STANDARD.encode(b"RIFF")).unwrap_err();
        assert_eq!(error.code, IpcErrorCode::Custom(INPUT_INVALID.to_owned()));
    }

    #[test]
    fn invalid_base64_is_refused() {
        let error = decode_wav("!!!not base64!!!").unwrap_err();
        assert_eq!(error.code, IpcErrorCode::Custom(INPUT_INVALID.to_owned()));
    }

    #[test]
    fn an_oversize_upload_is_refused_before_decoding() {
        let huge = "A".repeat(MAX_WAV_BYTES / 3 * 4 + 8);
        let error = decode_wav(&huge).unwrap_err();
        assert_eq!(error.code, IpcErrorCode::Custom(INPUT_INVALID.to_owned()));
        assert!(error.message.contains("larger than"));
    }

    #[test]
    fn a_real_wav_header_passes() {
        let wav = ferryx_hub::voice::pcm_to_wav(24_000, 1, &[0, 1, -1, 2]).unwrap();
        let decoded = decode_wav(&STANDARD.encode(&wav)).unwrap();
        assert_eq!(decoded, wav);
    }
}

