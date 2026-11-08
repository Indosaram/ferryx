// The supervisor voice call's pure model: recording a take, deciding what the owner meant, and
// turning the hub's receipt into the sentence that gets spoken.
//
// Nothing here touches a device or the network. The component owns a `VoiceCallPort` for that, so
// the whole decision path runs in tests without a microphone, and the one part that cannot be
// tested in jsdom (capture) is a single named seam.
import {
  CLOSED_RUN_STATES,
  goalEnvelope,
  goalWaitLine,
  newRequestId,
  parseSubmitReceipt,
  submitEnvelope,
  type SubmitReceipt,
  type UnifiedGoal,
  type UnifiedSupervisorClient,
  type UnifiedSupervisorView,
} from "./unifiedSupervisor";

/** The subset of goal actions a spoken control word may produce. */
export type VoiceControl = "pause" | "resume" | "cancel" | "approve";

/** What the surface needs to know before it asks for the microphone (`VoiceCallStatus`). */
export type VoiceCallStatus = {
  available: boolean;
  reason: string | null;
  sttModel: string | null;
  ttsModel: string | null;
  ttsVoice: string;
  sampleRate: number;
  channels: number;
  maxUtteranceMs: number;
  minSpeechMs: number;
  silenceMs: number;
};

/** One spoken reply (`SpokenAudio`); `audioBase64` empty means the text was not speakable. */
export type SpokenAudio = {
  audioBase64: string;
  mimeType: string;
  characters: number;
  durationMs: number;
};

/**
 * The device and transport seam. The desktop implementation records with `getUserMedia` and calls
 * the three Tauri commands; tests pass a fake. `record` resolves once the take has ended.
 */
export type VoiceCallPort = {
  readonly status: () => Promise<VoiceCallStatus>;
  /** Record one take under the status's own speech rules; resolves with a base64 WAV. */
  readonly record: (take: Pick<VoiceCallStatus, "sampleRate" | "minSpeechMs" | "silenceMs" | "maxUtteranceMs">) => Promise<string>;
  /** Stop the take in progress and make `record` settle with what it has. */
  readonly stopRecording: () => void;
  readonly transcribe: (wavBase64: string) => Promise<string>;
  readonly speak: (text: string) => Promise<SpokenAudio>;
  readonly play: (audio: SpokenAudio) => Promise<void>;
  readonly stopPlayback: () => void;
};

/** A canonical 44-byte RIFF/WAVE header around mono 16-bit samples, mirroring `voice::pcm_to_wav`. */
export function encodeWav(samples: Int16Array, sampleRate: number): Uint8Array {
  if (!Number.isFinite(sampleRate) || sampleRate <= 0) throw new Error("sampleRate must be positive");
  const dataBytes = samples.length * 2;
  const out = new Uint8Array(44 + dataBytes);
  const view = new DataView(out.buffer);
  const ascii = (offset: number, value: string) => {
    for (let index = 0; index < value.length; index += 1) view.setUint8(offset + index, value.charCodeAt(index));
  };
  ascii(0, "RIFF");
  view.setUint32(4, 36 + dataBytes, true);
  ascii(8, "WAVE");
  ascii(12, "fmt ");
  view.setUint32(16, 16, true); // PCM chunk size
  view.setUint16(20, 1, true); // format 1, PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true); // byte rate: rate * channels * bytes per sample
  view.setUint16(32, 2, true); // block align
  view.setUint16(34, 16, true); // bits per sample
  ascii(36, "data");
  view.setUint32(40, dataBytes, true);
  for (let index = 0; index < samples.length; index += 1) view.setInt16(44 + index * 2, samples[index] ?? 0, true);
  return out;
}

/** Linear-interpolation resample. The device rate (44.1/48 kHz) is not the provider's 24 kHz. */
export function resample(input: Float32Array, from: number, to: number): Float32Array {
  if (input.length === 0 || from === to || !(from > 0) || !(to > 0)) return input.slice();
  const length = Math.max(1, Math.round((input.length * to) / from));
  const step = from / to;
  const out = new Float32Array(length);
  for (let index = 0; index < length; index += 1) {
    const position = index * step;
    const left = Math.floor(position);
    const right = Math.min(left + 1, input.length - 1);
    const weight = position - left;
    out[index] = (input[left] ?? 0) * (1 - weight) + (input[right] ?? 0) * weight;
  }
  return out;
}

/** Float samples to 16-bit, clamped rather than wrapped. */
export function toInt16(samples: Float32Array): Int16Array {
  const out = new Int16Array(samples.length);
  for (let index = 0; index < samples.length; index += 1) {
    const clamped = Math.max(-1, Math.min(1, samples[index] ?? 0));
    out[index] = Math.round(clamped * 32767);
  }
  return out;
}

function rmsOf(frame: Float32Array): number {
  if (frame.length === 0) return 0;
  let sum = 0;
  for (let index = 0; index < frame.length; index += 1) {
    const value = frame[index] ?? 0;
    sum += value * value;
  }
  return Math.sqrt(sum / frame.length);
}

export type GateDecision = "collect" | "close" | "discard";

export type UtteranceGateOptions = {
  sampleRate: number;
  /** Voiced time below this is a blip, not an utterance. */
  minSpeechMs: number;
  /** Quiet time that ends a take. */
  silenceMs: number;
  /** A take with no speech at all ends this long after it started. */
  waitForSpeechMs?: number;
  maxSpeechMs: number;
  /** RMS at or above this is speech; the default is about -40 dBFS. */
  threshold?: number;
  /** Audio kept from before the first voiced frame so the first syllable is not clipped. */
  prerollMs?: number;
};

/**
 * Frame-driven voice activity gate, the same silence rules as `voice_pcm::UtteranceAssembler` but
 * measured per frame on the render thread: the first take closes on silence alone, with no timer
 * and no wall clock, so a test drives it with synthetic frames.
 */
export class UtteranceGate {
  private readonly options: Required<UtteranceGateOptions>;
  private collected: Float32Array[] = [];
  private preroll: Float32Array[] = [];
  private voicedMs = 0;
  private quietMs = 0;
  private collectedMs = 0;
  private heardSpeech = false;
  private settled: GateDecision | null = null;

  constructor(options: UtteranceGateOptions) {
    this.options = {
      waitForSpeechMs: 6_000,
      threshold: 0.008,
      prerollMs: 200,
      ...options,
    };
  }

  /** Add one frame; the decision says whether the take continues, closes, or is dropped. */
  push(frame: Float32Array): GateDecision {
    if (this.settled) return this.settled;
    const { sampleRate, threshold, prerollMs, waitForSpeechMs, maxSpeechMs, minSpeechMs, silenceMs } = this.options;
    const frameMs = sampleRate > 0 ? (frame.length / sampleRate) * 1_000 : 0;
    const voiced = rmsOf(frame) >= threshold;

    if (!this.heardSpeech) {
      if (!voiced) {
        this.preroll.push(frame);
        this.quietMs += frameMs;
        const keepMs = prerollMs;
        while (this.preroll.length > 1 && (this.preroll.length * frameMs) > keepMs) this.preroll.shift();
        if (this.quietMs >= waitForSpeechMs) return this.settle("discard");
        return "collect";
      }
      this.heardSpeech = true;
      this.quietMs = 0;
      for (const earlier of this.preroll) this.append(earlier, frameMs);
      this.preroll = [];
      this.append(frame, frameMs);
      this.voicedMs += frameMs;
      return this.collectedMs >= maxSpeechMs ? this.settle("close") : "collect";
    }

    this.append(frame, frameMs);
    if (voiced) {
      this.voicedMs += frameMs;
      this.quietMs = 0;
    } else {
      this.quietMs += frameMs;
    }
    if (this.collectedMs >= maxSpeechMs) return this.settle("close");
    if (this.quietMs >= silenceMs) {
      return this.settle(this.voicedMs >= minSpeechMs ? "close" : "discard");
    }
    return "collect";
  }

  private append(frame: Float32Array, frameMs: number) {
    this.collected.push(frame);
    this.collectedMs += frameMs;
  }

  private settle(decision: GateDecision): GateDecision {
    this.settled = decision;
    if (decision === "discard") this.collected = [];
    return decision;
  }

  /** Whether any speech was heard, which is what a discarded take never had. */
  get heard(): boolean {
    return this.heardSpeech;
  }

  /** The collected audio, joined. Empty after a discard. */
  samples(): Float32Array {
    const total = this.collected.reduce((sum, frame) => sum + frame.length, 0);
    const out = new Float32Array(total);
    let offset = 0;
    for (const frame of this.collected) {
      out.set(frame, offset);
      offset += frame.length;
    }
    return out;
  }
}

export type VoiceIntent =
  | { kind: "status" }
  | { kind: "goal"; action: VoiceControl; goal: UnifiedGoal }
  | { kind: "choose-goal"; action: VoiceControl; goals: UnifiedGoal[] }
  | { kind: "no-goal"; action: VoiceControl }
  | { kind: "submit"; text: string };

/** Words that ask for a report rather than an action. */
const STATUS_WORDS: readonly string[] = ["status", "progress", "report", "update"];

/**
 * The control vocabulary. `stop`/`halt` pause, which is reversible; cancelling a goal needs the
 * word "cancel", because a misheard "stop" must never destroy work.
 */
const CONTROLS: ReadonlyArray<{ action: VoiceControl; words: readonly string[] }> = [
  { action: "pause", words: ["pause", "hold", "stop", "halt"] },
  { action: "resume", words: ["resume", "continue", "unpause"] },
  { action: "cancel", words: ["cancel", "abort"] },
  { action: "approve", words: ["approve", "approved"] },
];

const CONTROL_VERB: Record<VoiceControl, string> = {
  pause: "pause",
  resume: "resume",
  cancel: "cancel",
  approve: "approve",
};

const CONTROL_PAST: Record<VoiceControl, string> = {
  pause: "Paused",
  resume: "Resumed",
  cancel: "Cancelled",
  approve: "Approved",
};

function normalize(text: string): string {
  return text.toLowerCase().replace(/[.,!?;:]+$/u, "").trim();
}

/** Open goals the surface may act on, narrowed to one project when the surface has one. */
export function openGoals(view: UnifiedSupervisorView | null, projectId: string | null): UnifiedGoal[] {
  if (!view) return [];
  return view.goals.filter((goal) => !CLOSED_RUN_STATES.has(goal.runState) && (!projectId || goal.projectId === projectId));
}

/**
 * What a transcript asks for. Deterministic and model-free: a control word is the first word, a
 * status word alone asks for a report, and everything else is a task for the hub to admit.
 */
export function parseVoiceIntent(transcript: string, goals: readonly UnifiedGoal[]): VoiceIntent {
  const text = transcript.trim();
  const normalized = normalize(text);
  if (!normalized) return { kind: "submit", text };
  const words = normalized.split(/\s+/u);
  const head = words[0] ?? "";
  if (STATUS_WORDS.includes(head)) return { kind: "status" };
  const control = CONTROLS.find((entry) => entry.words.includes(head));
  if (!control) return { kind: "submit", text };
  const open = goals.filter((goal) => !CLOSED_RUN_STATES.has(goal.runState));
  if (open.length === 0) return { kind: "no-goal", action: control.action };
  const named = words
    .slice(1)
    .join(" ")
    .replace(/^(?:the|a|that|goal|task|this)\s+/u, "")
    .trim();
  if (!named) {
    return open.length === 1
      ? { kind: "goal", action: control.action, goal: open[0]! }
      : { kind: "choose-goal", action: control.action, goals: open };
  }
  const matches = open.filter((goal) => goal.title.toLowerCase().includes(named));
  if (matches.length === 1) return { kind: "goal", action: control.action, goal: matches[0]! };
  // More than one match, or none: never act on a goal the owner did not name.
  return { kind: "choose-goal", action: control.action, goals: matches.length > 1 ? matches : open };
}

/** The receipt as one spoken sentence. */
export function spokenReceipt(receipt: SubmitReceipt): string {
  switch (receipt.outcome) {
    case "admitted":
    case "awaiting_owner":
      return receipt.acknowledgment || `Goal ${receipt.goalId} registered at revision ${receipt.revision}.`;
    case "conversational":
      return `Nothing was launched: ${receipt.reason || "the hub treated it as conversation"}.`;
    case "needs_choice":
      return `The request needs ${receipt.missing} first: ${receipt.detail}`;
    case "unknown":
      return "The hub answered, but I cannot read that receipt. Check the supervisor panel.";
  }
}

/** The open goals as a spoken report; `limit` keeps a long queue from becoming a monologue. */
export function statusSpeech(view: UnifiedSupervisorView | null, projectId: string | null, limit = 3): string {
  if (!view) return "The supervisor view has not loaded yet. Try again in a moment.";
  const open = openGoals(view, projectId);
  if (open.length === 0) return "Nothing is open. Every goal has finished.";
  const shown = open.slice(0, limit);
  const rest = open.length - shown.length;
  const lines = shown.map((goal) => `${goal.title}: ${goalWaitLine(goal)}`).join(" ");
  return rest > 0 ? `${lines} And ${rest} more.` : lines;
}

export type VoiceTurnResult = {
  /** What to speak; empty when there is nothing honest to say. */
  speech: string;
  /** Whether the hub state changed, so the caller refreshes the snapshot. */
  mutated: boolean;
  transcript: string;
};

/**
 * One voice turn: decide what the transcript asks for, make the request, and return the sentence
 * to speak. Every answer here is the hub's own receipt or the live snapshot; nothing is invented.
 */
export async function answerVoiceTurn(input: {
  transcript: string;
  client: UnifiedSupervisorClient;
  view: UnifiedSupervisorView | null;
  projectId: string | null;
}): Promise<VoiceTurnResult> {
  const transcript = input.transcript.trim();
  if (!transcript) return { speech: "", mutated: false, transcript };
  const intent = parseVoiceIntent(transcript, openGoals(input.view, input.projectId));

  if (intent.kind === "status") {
    return { speech: statusSpeech(input.view, input.projectId), mutated: false, transcript };
  }
  if (intent.kind === "no-goal") {
    return { speech: `There is no open goal to ${CONTROL_VERB[intent.action]}.`, mutated: false, transcript };
  }
  if (intent.kind === "choose-goal") {
    const titles = intent.goals.map((goal) => goal.title).join(", ");
    return {
      speech: titles
        ? `Which goal should I ${CONTROL_VERB[intent.action]}? ${titles}.`
        : `I did not find a goal named that. Nothing was ${CONTROL_PAST[intent.action].toLowerCase()}.`,
      mutated: false,
      transcript,
    };
  }
  if (intent.kind === "goal") {
    // Pause and cancel carry a reason so the panel shows the owner spoke it.
    const payload = intent.action === "pause" || intent.action === "cancel" ? { reason: "by voice" } : {};
    const result = await input.client.request(goalEnvelope("desktop", intent.goal, intent.action, payload));
    if (result.kind === "error") {
      return {
        speech: `The ${CONTROL_VERB[intent.action]} was refused: ${result.code}. ${result.message}`,
        // A stale revision means the goal moved; the caller refetches.
        mutated: result.code === "REVISION_CONFLICT",
        transcript,
      };
    }
    return { speech: `${CONTROL_PAST[intent.action]} ${intent.goal.title}.`, mutated: true, transcript };
  }

  const result = await input.client.request(
    submitEnvelope(newRequestId("desktop", "submit"), input.projectId, { text: transcript, doneWhen: "", verifier: "" }),
  );
  if (result.kind === "error") {
    return { speech: `The supervisor refused it: ${result.code}. ${result.message}`, mutated: false, transcript };
  }
  const receipt = parseSubmitReceipt(result.value);
  const admitted = receipt.outcome === "admitted" || receipt.outcome === "awaiting_owner";
  return { speech: spokenReceipt(receipt), mutated: admitted, transcript };
}

