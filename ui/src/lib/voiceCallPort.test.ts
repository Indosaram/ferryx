import { describe, expect, it } from "vitest";

import { remoteVoicePort, type VoiceFetch } from "./voiceCallPort";

function transport(routes: Record<string, { status: number; body: unknown }>) {
  const calls: { path: string; init?: { method?: string; headers?: Record<string, string>; body?: string } }[] = [];
  const fetchLike: VoiceFetch = async (path, init) => {
    calls.push(init ? { path, init } : { path });
    const route = routes[path];
    if (!route) return { status: 404, body: JSON.stringify({ error: { code: "NOT_FOUND", message: "no route" } }) };
    return { status: route.status, body: JSON.stringify(route.body) };
  };
  return { fetchLike, calls };
}

const status = {
  available: true,
  reason: null,
  sttModel: "whisper-1",
  ttsModel: "gpt-4o-mini-tts",
  ttsVoice: "alloy",
  sampleRate: 24000,
  channels: 1,
  maxUtteranceMs: 60000,
  minSpeechMs: 250,
  silenceMs: 900,
};

describe("remoteVoicePort", () => {
  it("reads the same status the desktop call reads, over the gateway", async () => {
    const { fetchLike, calls } = transport({ "/api/v1/voice/status": { status: 200, body: status } });
    await expect(remoteVoicePort(fetchLike).status()).resolves.toEqual(status);
    expect(calls).toEqual([{ path: "/api/v1/voice/status" }]);
  });

  it("posts the recording and returns the transcript text", async () => {
    const { fetchLike, calls } = transport({ "/api/v1/voice/transcribe": { status: 200, body: { text: "  what is running?  " } } });
    await expect(remoteVoicePort(fetchLike).transcribe("UklGRg==")).resolves.toBe("  what is running?  ");
    expect(calls[0].init?.method).toBe("POST");
    expect(calls[0].init?.headers).toEqual({ "Content-Type": "application/json" });
    expect(JSON.parse(String(calls[0].init?.body))).toEqual({ wavBase64: "UklGRg==" });
  });

  it("posts the receipt and returns the playable clip", async () => {
    const spoken = { audioBase64: "AAAA", mimeType: "audio/wav", characters: 5, durationMs: 900 };
    const { fetchLike } = transport({ "/api/v1/voice/speak": { status: 200, body: spoken } });
    await expect(remoteVoicePort(fetchLike).speak("Done.")).resolves.toEqual(spoken);
  });

  it("surfaces the machine's own refusal message instead of a bare status", async () => {
    const { fetchLike } = transport({
      "/api/v1/voice/status": { status: 503, body: { error: { code: "VOICE_UNAVAILABLE", message: "Voice is switched off." } } },
    });
    await expect(remoteVoicePort(fetchLike).status()).rejects.toThrow("Voice is switched off.");
  });

  it("refuses a spoken reply that is not an object", async () => {
    const { fetchLike } = transport({ "/api/v1/voice/speak": { status: 200, body: null } });
    await expect(remoteVoicePort(fetchLike).speak("Done.")).rejects.toThrow(/not readable/);
  });
});

