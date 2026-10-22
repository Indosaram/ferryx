//! Join a Discord voice channel with Songbird, group decoded PCM, and speak the reply.
//! Opus is decoded by Songbird (`Decode`, 16 kHz, mono), the same receive format omo uses.
use std::collections::HashMap;
#[cfg(feature = "hub-voice")]
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;

#[cfg(feature = "hub-voice")]
use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::{broadcast, mpsc, watch, Mutex};

#[cfg(feature = "hub-voice")]
use crate::discord_surface::{interaction_channel, interaction_user, voice_channel_option};
use crate::discord_surface::{
    interaction_guild, voice_gateway_update, voice_state_payload, voice_subcommand,
    VoiceGatewayUpdate,
};
#[cfg(feature = "hub-voice")]
use crate::lanes::kibitzer::ModelTurn;
use crate::voice::VoiceSettings;
#[cfg(feature = "hub-voice")]
use crate::voice::{
    format_voice_transcript, pcm_bytes_to_samples, pcm_to_wav, spoken_text, OPENAI_PCM_CHANNELS,
    OPENAI_PCM_RATE,
};
#[cfg(feature = "hub-voice")]
use crate::voice_pcm::{Utterance, UtteranceAssembler, VoicePacket};

#[cfg(feature = "hub-voice")]
const RECEIVE_SAMPLE_RATE: u32 = 16_000;
#[cfg(feature = "hub-voice")]
const RECEIVE_CHANNELS: u16 = 1;

struct LiveCall {
    stop: watch::Sender<bool>,
    generation: Arc<AtomicU64>,
    driver: Arc<Mutex<songbird::Driver>>,
}

fn outbound() -> &'static StdMutex<Option<mpsc::Sender<String>>> {
    static SLOT: OnceLock<StdMutex<Option<mpsc::Sender<String>>>> = OnceLock::new();
    SLOT.get_or_init(|| StdMutex::new(None))
}

fn notices() -> &'static broadcast::Sender<VoiceGatewayUpdate> {
    static TX: OnceLock<broadcast::Sender<VoiceGatewayUpdate>> = OnceLock::new();
    TX.get_or_init(|| broadcast::channel(64).0)
}

struct VoiceStates {
    channels: HashMap<u64, HashMap<u64, u64>>,
}

fn states() -> &'static StdMutex<VoiceStates> {
    static STATES: OnceLock<StdMutex<VoiceStates>> = OnceLock::new();
    STATES.get_or_init(|| {
        StdMutex::new(VoiceStates {
            channels: HashMap::new(),
        })
    })
}

fn calls() -> &'static StdMutex<HashMap<u64, LiveCall>> {
    static CALLS: OnceLock<StdMutex<HashMap<u64, LiveCall>>> = OnceLock::new();
    CALLS.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn bot_user() -> &'static AtomicU64 {
    static BOT: AtomicU64 = AtomicU64::new(0);
    &BOT
}

pub struct GatewayAttachment;

impl Drop for GatewayAttachment {
    fn drop(&mut self) {
        if let Ok(mut slot) = outbound().lock() {
            *slot = None;
        }
    }
}

pub fn attach(sender: mpsc::Sender<String>) -> GatewayAttachment {
    if let Ok(mut slot) = outbound().lock() {
        *slot = Some(sender);
    }
    GatewayAttachment
}

pub fn observe(frame: &Value) {
    let Some(update) = voice_gateway_update(frame) else {
        return;
    };
    match &update {
        VoiceGatewayUpdate::Ready { user_id } => {
            bot_user().store(*user_id, Ordering::Release);
        }
        VoiceGatewayUpdate::State {
            guild_id,
            user_id,
            channel_id,
            ..
        } => {
            if let Ok(mut cache) = states().lock() {
                let guild = cache.channels.entry(*guild_id).or_default();
                match channel_id {
                    Some(channel) => {
                        guild.insert(*user_id, *channel);
                    }
                    None => {
                        guild.remove(user_id);
                    }
                }
            }
        }
        VoiceGatewayUpdate::Server { .. } => {}
    }
    let _ = notices().send(update);
}

pub async fn answer(data: &Value) -> Result<String, String> {
    let settings = VoiceSettings::from_lookup(&|name| std::env::var(name).ok());
    match voice_subcommand(data).unwrap_or("") {
        "leave" => leave(&settings, interaction_guild(data)).await,
        "join" => join(&settings, data).await,
        _ => Err("Usage: /voice join or /voice leave.".into()),
    }
}

async fn join(settings: &VoiceSettings, data: &Value) -> Result<String, String> {
    if let Some(reason) = settings.unavailable_reason() {
        return Err(reason.to_owned());
    }
    #[cfg(not(feature = "hub-voice"))]
    {
        let _ = data;
        Err("Discord voice playback is compiled with hub-voice.".into())
    }
    #[cfg(feature = "hub-voice")]
    {
        if !crate::chat::discord::voice_receive_from_env() {
            return Err(
                "Discord voice needs DISCORD_VOICE_RECEIVE=on so the gateway requests GUILD_VOICE_STATES."
                    .into(),
            );
        }
        let guild_id = interaction_guild(data)
            .ok_or_else(|| "Voice channels are only available in a server.".to_owned())?;
        let user_id = interaction_user(data).unwrap_or(0);
        let voice_channel = match voice_channel_option(data) {
            Some(channel) => channel,
            None => states()
                .lock()
                .ok()
                .and_then(|cache| cache.channels.get(&guild_id)?.get(&user_id).copied())
                .ok_or_else(|| {
                    "Join a voice channel first, or pass one to `/voice join`.".to_owned()
                })?,
        };
        let _ = nonzero(guild_id)?;
        let _ = nonzero(voice_channel)?;
        let bot = bot_user().load(Ordering::Acquire);
        if bot == 0 {
            return Err("Discord gateway has not identified yet.".into());
        }
        let text_channel = interaction_channel(data).unwrap_or_default();
        let (generation, epoch) = retire(guild_id).await;
        let mut notices = notices().subscribe();
        send_state(guild_id, Some(voice_channel)).await?;
        let (session_id, endpoint, token) =
            collect_voice_connection(&mut notices, guild_id, bot, Duration::from_secs(15)).await?;
        let info = songbird::ConnectionInfo {
            channel_id: Some(songbird::id::ChannelId(nonzero(voice_channel)?)),
            endpoint,
            guild_id: songbird::id::GuildId(nonzero(guild_id)?),
            session_id,
            token,
            user_id: songbird::id::UserId(nonzero(bot)?),
        };
        let mut driver = songbird::Driver::new(driver_config());
        driver.mute(false);
        let (packet_tx, packet_rx) = mpsc::channel(256);
        let listener = SongbirdAudioEventListener {
            sender: packet_tx,
            generation: generation.clone(),
            epoch,
        };
        driver.add_global_event(
            songbird::Event::Core(songbird::CoreEvent::VoiceTick),
            listener.clone(),
        );
        driver.add_global_event(
            songbird::Event::Core(songbird::CoreEvent::SpeakingStateUpdate),
            listener.clone(),
        );
        driver.add_global_event(
            songbird::Event::Core(songbird::CoreEvent::ClientDisconnect),
            listener,
        );
        if let Err(error) = driver.connect(info).await {
            let _ = send_state(guild_id, None).await;
            return Err(format!("Could not join the voice channel: {error}"));
        }
        let (stop_tx, stop_rx) = watch::channel(false);
        let driver = Arc::new(Mutex::new(driver));
        if let Ok(mut live) = calls().lock() {
            live.insert(
                guild_id,
                LiveCall {
                    stop: stop_tx,
                    generation: generation.clone(),
                    driver: driver.clone(),
                },
            );
        }
        let silence = settings.silence();
        let min_speech = settings.min_speech();
        let max_speech = settings.max_speech();
        let owned = settings.clone();
        tokio::spawn(listen(
            packet_rx,
            driver,
            owned,
            text_channel,
            voice_channel,
            bot,
            stop_rx,
            generation,
            epoch,
            silence,
            min_speech,
            max_speech,
        ));
        Ok(format!(
            "Joined <#{voice_channel}>. I will answer in this text channel and play the audio in voice."
        ))
    }
}

async fn leave(settings: &VoiceSettings, guild_id: Option<u64>) -> Result<String, String> {
    if let Some(reason) = settings.unavailable_reason() {
        return Err(reason.to_owned());
    }
    let Some(guild_id) = guild_id else {
        return Err("Voice channels are only available in a server.".into());
    };
    let previous = calls()
        .lock()
        .ok()
        .and_then(|mut live| live.remove(&guild_id));
    let Some(previous) = previous else {
        return Ok("I am not in a voice channel in this server.".into());
    };
    let _ = previous.stop.send(true);
    previous.generation.fetch_add(1, Ordering::AcqRel);
    previous.driver.lock().await.leave();
    send_state(guild_id, None).await?;
    Ok("Left the voice channel.".into())
}

#[cfg(feature = "hub-voice")]
async fn retire(guild_id: u64) -> (Arc<AtomicU64>, u64) {
    let previous = calls()
        .lock()
        .ok()
        .and_then(|mut live| live.remove(&guild_id));
    if let Some(previous) = previous {
        let _ = previous.stop.send(true);
        let epoch = previous.generation.fetch_add(1, Ordering::AcqRel) + 1;
        previous.driver.lock().await.leave();
        (previous.generation, epoch)
    } else {
        (Arc::new(AtomicU64::new(1)), 1)
    }
}

async fn send_state(guild_id: u64, channel_id: Option<u64>) -> Result<(), String> {
    let sender = outbound()
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
        .ok_or_else(|| "Discord gateway is not connected.".to_owned())?;
    sender
        .send(voice_state_payload(guild_id, channel_id).to_string())
        .await
        .map_err(|_| "Discord gateway is not connected.".to_owned())
}

async fn collect_voice_connection(
    notices: &mut broadcast::Receiver<VoiceGatewayUpdate>,
    guild_id: u64,
    bot_user_id: u64,
    wait: Duration,
) -> Result<(String, String, String), String> {
    let deadline = tokio::time::Instant::now() + wait;
    let mut session_id: Option<String> = None;
    let mut endpoint: Option<String> = None;
    let mut token: Option<String> = None;
    loop {
        if let (Some(session_id), Some(endpoint), Some(token)) = (&session_id, &endpoint, &token) {
            return Ok((session_id.clone(), endpoint.clone(), token.clone()));
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err("Timed out waiting for the Discord voice server.".into());
        }
        let update = match tokio::time::timeout(remaining, notices.recv()).await {
            Ok(Ok(update)) => update,
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            _ => return Err("Timed out waiting for the Discord voice server.".into()),
        };
        match update {
            VoiceGatewayUpdate::State {
                guild_id: guild,
                user_id,
                session_id: session,
                channel_id,
            } if guild == guild_id && user_id == bot_user_id && channel_id.is_some() => {
                if !session.is_empty() {
                    session_id = Some(session);
                }
            }
            VoiceGatewayUpdate::Server {
                guild_id: guild,
                endpoint: next_endpoint,
                token: next_token,
            } if guild == guild_id => {
                endpoint = Some(next_endpoint);
                token = Some(next_token);
            }
            _ => {}
        }
    }
}

#[cfg(feature = "hub-voice")]
fn driver_config() -> songbird::Config {
    songbird::Config::default()
        .decode_mode(songbird::driver::DecodeMode::Decode)
        .decode_channels(songbird::driver::Channels::Mono)
        .decode_sample_rate(songbird::driver::SampleRate::Hz16000)
}

#[cfg(feature = "hub-voice")]
fn nonzero(id: u64) -> Result<NonZeroU64, String> {
    NonZeroU64::new(id).ok_or_else(|| "Discord ids must be non-zero.".to_owned())
}

#[cfg(feature = "hub-voice")]
#[derive(Clone)]
struct SongbirdAudioEventListener {
    sender: mpsc::Sender<VoicePacket>,
    generation: Arc<AtomicU64>,
    epoch: u64,
}

#[cfg(feature = "hub-voice")]
impl SongbirdAudioEventListener {
    fn current(&self) -> bool {
        self.generation.load(Ordering::Acquire) == self.epoch
    }

    fn forward(&self, packet: VoicePacket) -> Option<songbird::Event> {
        match self.sender.try_send(packet) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => None,
            Err(mpsc::error::TrySendError::Closed(_)) => Some(songbird::Event::Cancel),
        }
    }
}

#[cfg(feature = "hub-voice")]
#[async_trait]
impl songbird::EventHandler for SongbirdAudioEventListener {
    async fn act(&self, context: &songbird::EventContext<'_>) -> Option<songbird::Event> {
        if !self.current() {
            return Some(songbird::Event::Cancel);
        }
        match context {
            songbird::EventContext::VoiceTick(tick) => {
                for (ssrc, voice) in &tick.speaking {
                    if let Some(samples) = &voice.decoded_voice {
                        if let Some(cancel) = self.forward(VoicePacket::Pcm {
                            ssrc: *ssrc,
                            samples: samples.clone(),
                        }) {
                            return Some(cancel);
                        }
                    }
                }
                for ssrc in &tick.silent {
                    if let Some(cancel) = self.forward(VoicePacket::Silent { ssrc: *ssrc }) {
                        return Some(cancel);
                    }
                }
            }
            songbird::EventContext::SpeakingStateUpdate(update) => {
                if let Some(user_id) = update.user_id {
                    return self.forward(VoicePacket::Speaker {
                        ssrc: update.ssrc,
                        user_id: user_id.0,
                    });
                }
            }
            songbird::EventContext::ClientDisconnect(update) => {
                return self.forward(VoicePacket::Left {
                    user_id: update.user_id.0,
                });
            }
            _ => {}
        }
        None
    }
}

#[cfg(feature = "hub-voice")]
#[allow(clippy::too_many_arguments)]
async fn listen(
    mut receiver: mpsc::Receiver<VoicePacket>,
    driver: Arc<Mutex<songbird::Driver>>,
    settings: VoiceSettings,
    text_channel: String,
    voice_channel_id: u64,
    bot_user_id: u64,
    mut stop: watch::Receiver<bool>,
    generation: Arc<AtomicU64>,
    epoch: u64,
    silence: Duration,
    min_speech: Duration,
    max_speech: Duration,
) {
    let mut assembler = UtteranceAssembler::new(
        RECEIVE_SAMPLE_RATE,
        RECEIVE_CHANNELS,
        silence,
        min_speech,
        max_speech,
    );
    let turn = Arc::new(AtomicU64::new(0));
    let mut worker: Option<tokio::task::JoinHandle<()>> = None;
    loop {
        let packet = tokio::select! {
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    break;
                }
                continue;
            }
            packet = receiver.recv() => match packet {
                Some(packet) => packet,
                None => break,
            },
        };
        if generation.load(Ordering::Acquire) != epoch {
            break;
        }
        let speaking = matches!(packet, VoicePacket::Pcm { .. });
        if speaking {
            if let Some(handle) = worker.take() {
                turn.fetch_add(1, Ordering::AcqRel);
                handle.abort();
                driver.lock().await.stop();
            }
        }
        let Some(utterance) = assembler.push(packet) else {
            continue;
        };
        if utterance.user_id == Some(bot_user_id) {
            continue;
        }
        if let Some(handle) = worker.take() {
            handle.abort();
            driver.lock().await.stop();
        }
        let mine = turn.fetch_add(1, Ordering::AcqRel) + 1;
        let settings = settings.clone();
        let driver = driver.clone();
        let text_channel = text_channel.clone();
        let turn = turn.clone();
        let generation = generation.clone();
        worker = Some(tokio::spawn(speak_turn(
            driver,
            text_channel,
            settings,
            utterance,
            voice_channel_id,
            generation,
            epoch,
            turn,
            mine,
        )));
    }
    if let Some(handle) = worker.take() {
        handle.abort();
    }
    driver.lock().await.stop();
    let _ = voice_channel_id;
}

#[cfg(feature = "hub-voice")]
#[allow(clippy::too_many_arguments)]
async fn speak_turn(
    driver: Arc<Mutex<songbird::Driver>>,
    text_channel: String,
    settings: VoiceSettings,
    utterance: Utterance,
    _voice_channel_id: u64,
    generation: Arc<AtomicU64>,
    epoch: u64,
    turn: Arc<AtomicU64>,
    mine: u64,
) {
    if generation.load(Ordering::Acquire) != epoch || turn.load(Ordering::Acquire) != mine {
        return;
    }
    let (Some(stt), Some(tts)) = (settings.stt().cloned(), settings.tts().cloned()) else {
        return;
    };
    let wav = match pcm_to_wav(
        utterance.sample_rate,
        utterance.channels,
        &utterance.samples,
    ) {
        Ok(wav) => wav,
        Err(error) => {
            tracing::warn!(%error, "voice wav failed");
            return;
        }
    };
    let raw = match crate::voice::transcribe(&stt, &wav).await {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(%error, "voice transcription failed");
            return;
        }
    };
    let transcript = format_voice_transcript(utterance.speaker_label().as_deref(), &raw);
    if transcript.is_empty()
        || generation.load(Ordering::Acquire) != epoch
        || turn.load(Ordering::Acquire) != mine
    {
        return;
    }
    let response = match ask_model(&transcript).await {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(%error, "voice turn failed");
            return;
        }
    };
    if spoken_text(&response).is_empty()
        || generation.load(Ordering::Acquire) != epoch
        || turn.load(Ordering::Acquire) != mine
    {
        return;
    }
    if !text_channel.is_empty() {
        let _ = crate::chat::discord::deliver(&text_channel, &response).await;
    }
    let voice = settings.tts_voice().to_owned();
    let pcm = match crate::voice::synthesize_pcm(&tts, &voice, &response).await {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::warn!(%error, "failed to play the voice reply");
            return;
        }
    };
    if pcm.is_empty()
        || generation.load(Ordering::Acquire) != epoch
        || turn.load(Ordering::Acquire) != mine
    {
        return;
    }
    let samples = pcm_bytes_to_samples(&pcm);
    let input = pcm_input(OPENAI_PCM_RATE, u32::from(OPENAI_PCM_CHANNELS), &samples);
    driver.lock().await.play_input(input);
}

#[cfg(feature = "hub-voice")]
async fn ask_model(prompt: &str) -> Result<String, String> {
    let url =
        std::env::var("FERRYX_HUB_OMO_URL").unwrap_or_else(|_| "ws://127.0.0.1:19742".to_owned());
    crate::meta::backend::UrlModel::new(url)
        .complete_with(prompt, None, &[])
        .await
        .map_err(|error| error.to_string())
}

#[cfg(feature = "hub-voice")]
fn pcm_input(sample_rate: u32, channels: u32, samples: &[i16]) -> songbird::input::Input {
    let adapter = songbird::input::RawAdapter::new(
        std::io::Cursor::new(pcm_f32_bytes(samples)),
        sample_rate,
        channels,
    );
    adapter.into()
}

fn pcm_f32_bytes(samples: &[i16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 4);
    for sample in samples {
        let unit = f32::from(*sample) / 32768.0;
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_frames_become_interleaved_f32() {
        let bytes = pcm_f32_bytes(&[0, 16384, -16384]);
        assert_eq!(bytes.len(), 12);
        assert_eq!(f32::from_le_bytes(bytes[4..8].try_into().unwrap()), 0.5);
        assert_eq!(f32::from_le_bytes(bytes[8..12].try_into().unwrap()), -0.5);
    }

    #[tokio::test]
    async fn a_voice_server_update_completes_the_connection() {
        let mut notices = notices().subscribe();
        let guild = 900_000_000 + (std::process::id() as u64);
        let bot = 7u64;
        observe(&serde_json::json!({
            "op": 0,
            "t": "VOICE_STATE_UPDATE",
            "d": {
                "guild_id": guild.to_string(),
                "user_id": bot.to_string(),
                "session_id": "sess",
                "channel_id": "42"
            }
        }));
        observe(&serde_json::json!({
            "op": 0,
            "t": "VOICE_SERVER_UPDATE",
            "d": {
                "guild_id": guild.to_string(),
                "token": "tok",
                "endpoint": "example.discord.media:443"
            }
        }));
        let (session, endpoint, token) =
            collect_voice_connection(&mut notices, guild, bot, Duration::from_secs(2))
                .await
                .unwrap();
        assert_eq!(session, "sess");
        assert_eq!(endpoint, "example.discord.media:443");
        assert_eq!(token, "tok");
    }
}
