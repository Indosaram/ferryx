import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { VoiceConversationButton } from "./VoiceConversationButton";
import type { VoiceCallPort, VoiceCallStatus } from "../../lib/voiceCall";

afterEach(cleanup);

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
    transcribe: async () => "what is running",
    speak: async (text) => ({ audioBase64: "AUDIO", mimeType: "audio/wav", characters: text.length, durationMs: 100 }),
    play: async () => undefined,
    stopRecording: () => undefined,
    stopPlayback: () => undefined,
    ...overrides,
  };
}

describe("VoiceConversationButton", () => {
  it("records one sentence, sends it, and speaks the reply the surface gave", async () => {
    const send = vi.fn(() => "Two goals are running.");
    const play = vi.fn(async () => undefined);
    const onTurn = vi.fn();

    render(<VoiceConversationButton port={port({ play })} status={status} host={{ send }} onTurn={onTurn} />);
    await act(async () => {
      fireEvent.click(screen.getByTestId("voice-conversation-button"));
    });

    await waitFor(() => expect(onTurn).toHaveBeenCalledOnce());
    expect(send).toHaveBeenCalledWith("what is running");
    expect(play).toHaveBeenCalledOnce();
    expect(onTurn.mock.calls[0]?.[0]).toMatchObject({ transcript: "what is running", reply: "Two goals are running." });
  });

  it("says why voice is off instead of pretending it listened", () => {
    const send = vi.fn();
    render(
      <VoiceConversationButton
        port={port()}
        status={{ ...status, available: false, reason: "No speech provider is configured." }}
        host={{ send }}
      />,
    );

    const button = screen.getByTestId<HTMLButtonElement>("voice-conversation-button");
    expect(button.disabled).toBe(true);
    expect(button.getAttribute("title")).toBe("No speech provider is configured.");
    expect(send).not.toHaveBeenCalled();
  });

  it("shows a failed turn without losing the surface", async () => {
    render(<VoiceConversationButton port={port({ record: async () => "" })} status={status} host={{ send: vi.fn() }} />);
    await act(async () => {
      fireEvent.click(screen.getByTestId("voice-conversation-button"));
    });

    await waitFor(() => expect(screen.getByTestId("voice-conversation-error")).toBeTruthy());
    expect(screen.getByTestId("voice-conversation-error").textContent).toBe("Nothing was recorded.");
  });
});
