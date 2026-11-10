// The device side of the supervisor voice call: microphone capture, the three Tauri commands, and
// playback of the returned WAV. Everything decision-shaped lives in `voiceCall.ts`; this file is
// the one seam a test cannot drive, so it stays thin and defensive.
import { invoke } from "@tauri-apps/api/core";

import {
  UtteranceGate,
  encodeWav,
  resample,
  toInt16,
  type SpokenAudio,
  type VoiceCallPort,
  type VoiceCallStatus,
} from "./voiceCall";

/** The capture frame a `ScriptProcessorNode` gives at 44.1/48 kHz: about 85-93 ms. */
const FRAME_SIZE = 4096;

function base64(bytes: Uint8Array): string {
  let binary = "";
  const chunk = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunk) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunk));
  }
  return btoa(binary);
}

function audioContextCtor(): typeof AudioContext | null {
  const scope = globalThis as { AudioContext?: typeof AudioContext; webkitAudioContext?: typeof AudioContext };
  return scope.AudioContext ?? scope.webkitAudioContext ?? null;
}

/** What one take needs from the status the surface already fetched. */
export type TakeOptions = Pick<VoiceCallStatus, "sampleRate" | "minSpeechMs" | "silenceMs" | "maxUtteranceMs">;

/** The two provider hops, kept apart from capture so a phone can reach them over HTTP. */
export type VoiceTransport = {
  status: () => Promise<VoiceCallStatus>;
  transcribe: (wavBase64: string) => Promise<string>;
  speak: (text: string) => Promise<SpokenAudio>;
};

/** How the phone reaches the gateway: the same shape `SupervisorFetch` has. */
export type VoiceFetch = (
  path: string,
  init?: { method?: string; headers?: Record<string, string>; body?: string },
) => Promise<{ status: number; body: Uint8Array | string }>;

function decodeJson(body: Uint8Array | string): unknown {
  const text = typeof body === "string" ? body : new TextDecoder().decode(body);
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

function failureOf(body: Uint8Array | string): string {
  const data = decodeJson(body) as { error?: { message?: unknown }; message?: unknown } | null;
  const message = data?.error?.message ?? data?.message;
  return typeof message === "string" && message.trim() ? message : "The supervisor's voice is unreachable.";
}

/**
 * The capture and playback both surfaces share, over one transport. Capture uses
 * `ScriptProcessorNode` rather than `AudioWorklet`: the worklet
 * needs a module URL and a blob URL is outside this app's CSP, while the processor node is
 * available in every WebView the app ships on. The node is routed through a muted gain, so the
 * microphone is never played back into the speakers.
 */
function createVoicePort(transport: VoiceTransport): VoiceCallPort {
  let stream: MediaStream | null = null;
  let context: AudioContext | null = null;
  let settleTake: (() => void) | null = null;
  let audio: HTMLAudioElement | null = null;
  let objectUrl: string | null = null;

  const releaseCapture = () => {
    settleTake?.();
    settleTake = null;
    stream?.getTracks().forEach((track) => track.stop());
    stream = null;
    void context?.close().catch(() => undefined);
    context = null;
  };

  const releasePlayback = () => {
    if (audio) {
      audio.pause();
      audio.src = "";
      audio = null;
    }
    if (objectUrl) {
      URL.revokeObjectURL(objectUrl);
      objectUrl = null;
    }
  };

  return {
    status: () => transport.status(),

    record: async (take: TakeOptions) => {
      const Ctor = audioContextCtor();
      if (!Ctor) throw new Error("This WebView has no audio capture support.");
      if (!navigator.mediaDevices?.getUserMedia) throw new Error("This WebView has no microphone API.");
      let opened: MediaStream;
      try {
        opened = await navigator.mediaDevices.getUserMedia({
          audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true },
        });
      } catch {
        throw new Error("The microphone was refused. Allow it in system settings, or type the request.");
      }
      stream = opened;
      const ctx = new Ctor();
      context = ctx;
      if (ctx.state === "suspended") await ctx.resume().catch(() => undefined);
      const rate = ctx.sampleRate;
      const gate = new UtteranceGate({
        sampleRate: rate,
        minSpeechMs: take.minSpeechMs,
        silenceMs: take.silenceMs,
        maxSpeechMs: take.maxUtteranceMs,
        waitForSpeechMs: 6_000,
      });
      const source = ctx.createMediaStreamSource(opened);
      // Deprecated but universal; it fires on the audio thread's schedule without a module URL.
      const processor = ctx.createScriptProcessor(FRAME_SIZE, 1, 1);
      const muted = ctx.createGain();
      muted.gain.value = 0;
      const done = new Promise<void>((resolve) => {
        settleTake = resolve;
      });

      processor.onaudioprocess = (event) => {
        const decision = gate.push(new Float32Array(event.inputBuffer.getChannelData(0)));
        if (decision === "collect") return;
        processor.onaudioprocess = null;
        releaseCapture();
      };
      source.connect(processor);
      processor.connect(muted);
      muted.connect(ctx.destination);
      await done;

      if (!gate.heard) throw new Error("nothing-recorded");
      const mono = resample(gate.samples(), rate, take.sampleRate);
      return base64(encodeWav(toInt16(mono), take.sampleRate));
    },

    stopRecording: () => releaseCapture(),

    transcribe: async (wavBase64: string) => {
      return transport.transcribe(wavBase64);
    },

    speak: (text: string) => transport.speak(text),

    play: async (spoken: SpokenAudio) => {
      releasePlayback();
      if (!spoken.audioBase64) return;
      const binary = atob(spoken.audioBase64);
      const bytes = new Uint8Array(binary.length);
      for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
      objectUrl = URL.createObjectURL(new Blob([bytes], { type: spoken.mimeType }));
      const element = new Audio(objectUrl);
      audio = element;
      await new Promise<void>((resolve, reject) => {
        element.onended = () => resolve();
        element.onerror = () => reject(new Error("The spoken reply could not be played."));
        void element.play().catch(reject);
      });
      releasePlayback();
    },

    stopPlayback: () => releasePlayback(),
  };
}

/** The desktop port: the machine's own IPC commands. */
export function desktopVoicePort(): VoiceCallPort {
  return createVoicePort({
    status: () => invoke<VoiceCallStatus>("cmd_voice_call_status"),
    transcribe: async (wavBase64) => {
      const answer = await invoke<{ text: string }>("cmd_voice_call_transcribe", { wavBase64 });
      return answer.text;
    },
    speak: (text) => invoke<SpokenAudio>("cmd_voice_call_speak", { text }),
  });
}

/**
 * The phone port: the same commands behind the gateway's `/api/v1/voice/*` routes, carried by the
 * per-machine fetch the home already uses. Capture and playback stay on the device.
 */
export function remoteVoicePort(fetchLike: VoiceFetch): VoiceCallPort {
  const call = async (path: string, init?: { method?: string; body?: string }) => {
    const headers = init?.body === undefined ? undefined : { "Content-Type": "application/json" };
    const res = await fetchLike(path, init?.body === undefined ? undefined : { method: init.method, headers, body: init.body });
    if (res.status < 200 || res.status >= 300) throw new Error(failureOf(res.body));
    return decodeJson(res.body);
  };
  return createVoicePort({
    status: async () => {
      const data = await call("/api/v1/voice/status");
      if (!data || typeof data !== "object") throw new Error("The computer's voice status was not readable.");
      return data as VoiceCallStatus;
    },
    transcribe: async (wavBase64) => {
      const data = await call("/api/v1/voice/transcribe", { method: "POST", body: JSON.stringify({ wavBase64 }) });
      const text = (data as { text?: unknown } | null)?.text;
      return typeof text === "string" ? text : "";
    },
    speak: async (text) => {
      const data = await call("/api/v1/voice/speak", { method: "POST", body: JSON.stringify({ text }) });
      if (!data || typeof data !== "object") throw new Error("The spoken reply was not readable.");
      return data as SpokenAudio;
    },
  });
}

