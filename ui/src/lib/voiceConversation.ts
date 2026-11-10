// The free-form voice conversation model: speak, transcribe, send the words to whichever
// conversation surface owns the turn, then speak its reply. Unlike the supervisor call (which maps
// a transcript onto four control verbs), this keeps the whole sentence and hands it to the same
// send path the keyboard uses, so a voice turn is the same turn a typed one would be.
import type { SpokenAudio, VoiceCallPort, VoiceCallStatus } from "./voiceCall";

export type ConversationTurn = {
  readonly transcript: string;
  readonly reply: string;
  readonly audio: SpokenAudio | null;
};

export type ConversationReply = string | { readonly text: string } | void | undefined | null;

export type ConversationHost = {
  readonly send: (text: string) => ConversationReply | Promise<ConversationReply>;
};

export function spokenReply(value: ConversationReply): string {
  if (typeof value === "string") return value.trim();
  if (value && typeof value === "object" && "text" in value) {
    const text = (value as { readonly text?: unknown }).text;
    return typeof text === "string" ? text.trim() : "";
  }
  return "";
}

/**
 * Clip a reply to what a spoken turn should carry. A voice surface answers with a sentence or two;
 * a whole transcript read aloud is unusable, and the provider bills by the character.
 */
export function clipForSpeech(text: string, limit = 600): string {
  const trimmed = text.trim();
  if (trimmed.length <= limit) return trimmed;
  const window = trimmed.slice(0, limit);
  const boundary = Math.max(window.lastIndexOf(". "), window.lastIndexOf("? "), window.lastIndexOf("! "));
  if (boundary > limit * 0.5) return `${window.slice(0, boundary + 1)} …`;
  return `${window} …`;
}

export function conversationAvailability(status: VoiceCallStatus | null): string | null {
  if (!status) return "Checking whether this device can record…";
  if (!status.available) return status.reason ?? "Voice is not configured on this hub.";
  return null;
}

export async function runConversationTurn(input: {
  readonly port: VoiceCallPort;
  readonly status: VoiceCallStatus;
  readonly host: ConversationHost;
  readonly speak: boolean;
}): Promise<{ readonly turn: ConversationTurn | null; readonly error: string | null }> {
  const wav = await input.port.record({
    sampleRate: input.status.sampleRate,
    minSpeechMs: input.status.minSpeechMs,
    silenceMs: input.status.silenceMs,
    maxUtteranceMs: input.status.maxUtteranceMs,
  });
  if (!wav) return { turn: null, error: "Nothing was recorded." };

  const transcript = (await input.port.transcribe(wav)).trim();
  if (!transcript) return { turn: null, error: "Could not make out any words." };

  const reply = spokenReply(await input.host.send(transcript));
  // Without a paid key there is no synthesis: the words still arrive, and the reply stays on
  // screen. Dictation alone is the free path.
  if (!input.speak || !reply || !input.status.speakAvailable) {
    return { turn: { transcript, reply, audio: null }, error: null };
  }

  // A failed synthesis must not swallow the turn: the words still arrived.
  try {
    const audio = await input.port.speak(clipForSpeech(reply));
    if (audio.audioBase64) await input.port.play(audio);
    return { turn: { transcript, reply, audio: audio.audioBase64 ? audio : null }, error: null };
  } catch (error) {
    return { turn: { transcript, reply, audio: null }, error: null };
  }
}
