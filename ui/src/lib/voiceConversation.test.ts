import { describe, expect, it, vi } from "vitest";
import { clipForSpeech, conversationAvailability, runConversationTurn, spokenReply } from "./voiceConversation";
import type { VoiceCallPort, VoiceCallStatus } from "./voiceCall";

const status: VoiceCallStatus = {
  available: true,
  reason: null,
  sttModel: "whisper-1",
  ttsModel: "tts-1",
  sttSource: "key",
  speakAvailable: true,
  speakReason: null,
  ttsVoice: "alloy",
  sampleRate: 16000,
  channels: 1,
  maxUtteranceMs: 30000,
  minSpeechMs: 400,
  silenceMs: 700,
};

function port(overrides: Partial<VoiceCallPort> = {}): VoiceCallPort {
  return {
    status: async () => status,
    record: async () => "WAV",
    transcribe: async () => "what is left to do",
    speak: async (text) => ({ audioBase64: "AUDIO", mimeType: "audio/wav", characters: text.length, durationMs: 100 }),
    play: async () => undefined,
    stopRecording: () => undefined,
    stopPlayback: () => undefined,
    ...overrides,
  };
}

describe("spokenReply", () => {
  it("keeps a string answer and a text receipt and refuses to invent one", () => {
    expect(spokenReply("done")).toBe("done");
    expect(spokenReply({ text: "  admitted " })).toBe("admitted");
    expect(spokenReply(undefined)).toBe("");
  });
});

describe("clipForSpeech", () => {
  it("leaves a short reply alone and cuts a long one at a sentence", () => {
    expect(clipForSpeech("All clear.")).toBe("All clear.");
    const long = `${"One sentence here. ".repeat(60)}`;
    const clipped = clipForSpeech(long, 100);
    expect(clipped.length).toBeLessThanOrEqual(104);
    expect(clipped.endsWith("…")).toBe(true);
    expect(clipped.slice(0, -2).endsWith(". ") || clipped.slice(0, -1).trimEnd().endsWith(".")).toBe(true);
    expect(clipped).not.toContain("One sentence her…");
  });
});

describe("conversationAvailability", () => {
  it("reports the hub's own reason instead of a generic one", () => {
    expect(conversationAvailability(status)).toBeNull();
    expect(conversationAvailability({ ...status, available: false, reason: "No speech key." })).toBe("No speech key.");
    expect(conversationAvailability({ ...status, available: false, reason: null })).toContain("not configured");
    expect(conversationAvailability(null)).toContain("Checking");
  });
});

describe("runConversationTurn", () => {
  it("sends the transcript through the surface's own send and speaks its reply", async () => {
    const send = vi.fn(() => "Paused the goal.");
    const play = vi.fn(async () => undefined);
    const result = await runConversationTurn({ port: port({ play }), status, host: { send }, speak: true });

    expect(send).toHaveBeenCalledWith("what is left to do");
    expect(play).toHaveBeenCalledOnce();
    expect(result.error).toBeNull();
    expect(result.turn?.transcript).toBe("what is left to do");
    expect(result.turn?.reply).toBe("Paused the goal.");
  });

  it("keeps the turn when synthesis fails, because the words still arrived", async () => {
    const send = vi.fn(() => "admitted");
    const result = await runConversationTurn({
      port: port({ speak: async () => { throw new Error("provider down"); } }),
      status,
      host: { send },
      speak: true,
    });

    expect(result.error).toBeNull();
    expect(result.turn?.reply).toBe("admitted");
    expect(result.turn?.audio).toBeNull();
  });

  it("keeps dictating for free and answers in text when no key pays for speech", async () => {
    const speak = vi.fn();
    const send = vi.fn(() => "The second requirement is still running.");
    const result = await runConversationTurn({
      port: port({ speak }),
      status: { ...status, sttSource: "codex", speakAvailable: false, speakReason: "Spoken replies need a speech key.", ttsModel: null },
      host: { send },
      speak: true,
    });

    expect(send).toHaveBeenCalledWith("what is left to do");
    expect(speak).not.toHaveBeenCalled();
    expect(result.turn?.reply).toBe("The second requirement is still running.");
    expect(result.turn?.audio).toBeNull();
    expect(result.error).toBeNull();
  });

  it("does not synthesize when the owner asked for text only", async () => {
    const speak = vi.fn();
    const result = await runConversationTurn({ port: port({ speak }), status, host: { send: () => "ok" }, speak: false });

    expect(speak).not.toHaveBeenCalled();
    expect(result.turn?.audio).toBeNull();
  });

  it("reports an empty take and an empty transcript without calling send", async () => {
    const send = vi.fn();
    const silent = await runConversationTurn({ port: port({ record: async () => "" }), status, host: { send }, speak: true });
    expect(silent.error).toBe("Nothing was recorded.");
    expect(send).not.toHaveBeenCalled();

    const mumble = await runConversationTurn({ port: port({ transcribe: async () => "   " }), status, host: { send }, speak: true });
    expect(mumble.error).toBe("Could not make out any words.");
    expect(send).not.toHaveBeenCalled();
  });

  it("awaits a surface that answers asynchronously", async () => {
    const send = vi.fn(async () => "The goal is running.");
    const result = await runConversationTurn({ port: port(), status, host: { send }, speak: true });

    expect(result.turn?.reply).toBe("The goal is running.");
  });
});
