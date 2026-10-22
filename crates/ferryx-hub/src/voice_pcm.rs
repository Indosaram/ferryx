//! Groups decoded Discord voice PCM into utterances separated by silence.
//! Opus payloads are ignored until a decoder is attached; PCM, silence, speaker, and leave match omo.
use std::collections::HashMap;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoicePacket {
    Pcm { ssrc: u32, samples: Vec<i16> },
    Silent { ssrc: u32 },
    Speaker { ssrc: u32, user_id: u64 },
    Left { user_id: u64 },
    Opus { ssrc: u32, payload: Vec<u8> },
    Label { ssrc: u32, name: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Utterance {
    pub ssrc: u32,
    pub user_id: Option<u64>,
    pub name: Option<String>,
    pub samples: Vec<i16>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl Utterance {
    pub fn speaker_label(&self) -> Option<String> {
        if let Some(name) = self
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            return Some(name.to_owned());
        }
        self.user_id.map(|user_id| format!("<@{user_id}>"))
    }
}

struct Speaker {
    samples: Vec<i16>,
    trailing_silence: usize,
    user_id: Option<u64>,
    name: Option<String>,
}

pub struct UtteranceAssembler {
    sample_rate: u32,
    channels: u16,
    samples_per_tick: usize,
    silence_samples: usize,
    min_samples: usize,
    max_samples: usize,
    speakers: HashMap<u32, Speaker>,
    users: HashMap<u32, u64>,
    names: HashMap<u32, String>,
}

impl UtteranceAssembler {
    pub fn new(
        sample_rate: u32,
        channels: u16,
        silence: Duration,
        min_speech: Duration,
        max_speech: Duration,
    ) -> Self {
        let channels = channels.max(1);
        let sample_rate = sample_rate.max(1);
        let samples_per_tick =
            ((u64::from(sample_rate) * u64::from(channels) * 20) / 1000).max(1) as usize;
        Self {
            sample_rate,
            channels,
            samples_per_tick,
            silence_samples: samples_for(sample_rate, channels, silence).max(samples_per_tick),
            min_samples: samples_for(sample_rate, channels, min_speech),
            max_samples: samples_for(sample_rate, channels, max_speech).max(1),
            speakers: HashMap::new(),
            users: HashMap::new(),
            names: HashMap::new(),
        }
    }

    pub fn push(&mut self, packet: VoicePacket) -> Option<Utterance> {
        match packet {
            VoicePacket::Pcm { ssrc, samples } => self.push_samples(ssrc, samples),
            VoicePacket::Silent { ssrc } => self.push_silence(ssrc),
            VoicePacket::Speaker { ssrc, user_id } => {
                self.users.insert(ssrc, user_id);
                if let Some(speaker) = self.speakers.get_mut(&ssrc) {
                    speaker.user_id = Some(user_id);
                }
                None
            }
            VoicePacket::Label { ssrc, name } => {
                let name = name.trim().to_owned();
                if name.is_empty() {
                    return None;
                }
                self.names.insert(ssrc, name.clone());
                if let Some(speaker) = self.speakers.get_mut(&ssrc) {
                    speaker.name = Some(name);
                }
                None
            }
            VoicePacket::Left { user_id } => self.finish_user(user_id),
            VoicePacket::Opus { .. } => None,
        }
    }

    fn push_samples(&mut self, ssrc: u32, samples: Vec<i16>) -> Option<Utterance> {
        if samples.is_empty() {
            return None;
        }
        let user_id = self.users.get(&ssrc).copied();
        let name = self.names.get(&ssrc).cloned();
        let speaker = self.speakers.entry(ssrc).or_insert_with(|| Speaker {
            samples: Vec::new(),
            trailing_silence: 0,
            user_id,
            name,
        });
        speaker.trailing_silence = 0;
        speaker.user_id = speaker.user_id.or(user_id);
        if speaker.name.is_none() {
            speaker.name = self.names.get(&ssrc).cloned();
        }
        speaker.samples.extend(samples);
        if speaker.samples.len() >= self.max_samples {
            return self.take(ssrc);
        }
        None
    }

    fn push_silence(&mut self, ssrc: u32) -> Option<Utterance> {
        let speaker = self.speakers.get_mut(&ssrc)?;
        speaker.trailing_silence = speaker
            .trailing_silence
            .saturating_add(self.samples_per_tick);
        if speaker.trailing_silence < self.silence_samples {
            return None;
        }
        if speaker.samples.len() < self.min_samples {
            self.speakers.remove(&ssrc);
            return None;
        }
        self.take(ssrc)
    }

    fn finish_user(&mut self, user_id: u64) -> Option<Utterance> {
        let ssrc = self
            .users
            .iter()
            .find(|(_, id)| **id == user_id)
            .map(|(ssrc, _)| *ssrc)?;
        self.users.remove(&ssrc);
        let long_enough = self
            .speakers
            .get(&ssrc)
            .is_some_and(|speaker| speaker.samples.len() >= self.min_samples);
        if long_enough {
            self.take(ssrc)
        } else {
            self.speakers.remove(&ssrc);
            None
        }
    }

    fn take(&mut self, ssrc: u32) -> Option<Utterance> {
        let speaker = self.speakers.remove(&ssrc)?;
        if speaker.samples.is_empty() {
            return None;
        }
        Some(Utterance {
            ssrc,
            user_id: speaker.user_id.or_else(|| self.users.get(&ssrc).copied()),
            name: speaker.name.or_else(|| self.names.get(&ssrc).cloned()),
            samples: speaker.samples,
            sample_rate: self.sample_rate,
            channels: self.channels,
        })
    }
}

fn samples_for(sample_rate: u32, channels: u16, duration: Duration) -> usize {
    let millis = duration.as_millis().min(u128::from(u64::MAX)) as u64;
    ((u64::from(sample_rate) * u64::from(channels) * millis) / 1000) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assembler() -> UtteranceAssembler {
        UtteranceAssembler::new(
            16_000,
            1,
            Duration::from_millis(40),
            Duration::from_millis(40),
            Duration::from_millis(100),
        )
    }

    fn pcm(ssrc: u32, n: usize) -> VoicePacket {
        VoicePacket::Pcm {
            ssrc,
            samples: vec![1; n],
        }
    }

    #[test]
    fn silence_after_speech_emits_one_utterance_and_drops_noise() {
        let mut assembler = assembler();
        assert!(assembler.push(pcm(1, 640)).is_none());
        assert!(assembler.push(VoicePacket::Silent { ssrc: 1 }).is_none());
        let utterance = assembler
            .push(VoicePacket::Silent { ssrc: 1 })
            .expect("silence threshold ends the utterance");
        assert_eq!(utterance.samples.len(), 640);
        assert!(assembler.push(pcm(2, 10)).is_none());
        assert!(assembler.push(VoicePacket::Silent { ssrc: 2 }).is_none());
        assert!(assembler.push(VoicePacket::Silent { ssrc: 2 }).is_none());
    }

    #[test]
    fn a_display_name_labels_the_utterance_and_long_speech_flushes() {
        let mut named = assembler();
        assert!(named
            .push(VoicePacket::Label {
                ssrc: 3,
                name: "Ada".into(),
            })
            .is_none());
        assert!(named.push(pcm(3, 640)).is_none());
        assert!(named.push(VoicePacket::Silent { ssrc: 3 }).is_none());
        let utterance = named
            .push(VoicePacket::Silent { ssrc: 3 })
            .expect("named speaker ended");
        assert_eq!(utterance.speaker_label().as_deref(), Some("Ada"));
        let mut fresh = assembler();
        let flushed = fresh.push(pcm(1, 1600)).expect("max speech length flushes");
        assert_eq!(flushed.samples.len(), 1600);
        assert_eq!(flushed.sample_rate, 16_000);
    }
}
