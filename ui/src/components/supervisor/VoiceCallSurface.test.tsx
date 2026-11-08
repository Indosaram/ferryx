import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { UnifiedGoal, UnifiedSupervisorClient, UnifiedSupervisorView } from "../../lib/unifiedSupervisor";
import type { SpokenAudio, VoiceCallPort, VoiceCallStatus } from "../../lib/voiceCall";
import { VoiceCallSurface } from "./VoiceCallSurface";

afterEach(cleanup);

const available: VoiceCallStatus = {
  available: true,
  reason: null,
  sttModel: "whisper-1",
  ttsModel: "tts-1",
  ttsVoice: "alloy",
  sampleRate: 24_000,
  channels: 1,
  maxUtteranceMs: 30_000,
  minSpeechMs: 400,
  silenceMs: 700,
};

const silent: VoiceCallStatus = { ...available, available: false, reason: "Voice needs OMON_STT_API_KEY.", sttModel: null, ttsModel: null };

const goal: UnifiedGoal = {
  goalId: "g-7", supervisorId: "s-1", projectId: "p-1", laneKey: "sv-7", title: "Write result.txt",
  runState: "running", stateDetail: null, revision: 5, pendingWaits: [], nextTrigger: null,
  updatedAt: "", mode: "full", turnLimit: 20, turnsUsed: 2, grant: ["edit"],
};

const view: UnifiedSupervisorView = {
  identity: { supervisorId: "s-1", ownerPrincipal: "desktop:qa-owner" },
  projects: [{ projectId: "p-1", name: "qa-supervisor" }],
  goals: [goal],
  health: null,
  error: null,
};

function spoken(text: string): SpokenAudio {
  return { audioBase64: btoa(text), mimeType: "audio/wav", characters: text.length, durationMs: 500 };
}

function fakePort(overrides: Partial<VoiceCallPort> = {}) {
  const played: SpokenAudio[] = [];
  const port: VoiceCallPort = {
    status: vi.fn(async () => available),
    record: vi.fn(async () => "UklGRg=="),
    stopRecording: vi.fn(),
    transcribe: vi.fn(async () => "status"),
    speak: vi.fn(async (text: string) => spoken(text)),
    play: vi.fn(async (audio: SpokenAudio) => {
      played.push(audio);
    }),
    stopPlayback: vi.fn(),
    ...overrides,
  };
  return { port, played };
}

function fakeClient(answer: Record<string, unknown>) {
  return {
    request: vi.fn(async () => ({ kind: "ok" as const, value: answer })),
    conversation: vi.fn(async () => ({ kind: "ok" as const, value: { messages: [], next: 0 } })),
    memory: vi.fn(async () => ({ kind: "ok" as const, value: {} })),
    remember: vi.fn(async () => ({ kind: "ok" as const, value: {} })),
    refresh: vi.fn(),
  } satisfies UnifiedSupervisorClient;
}

function renderSurface(port: VoiceCallPort, client: UnifiedSupervisorClient = fakeClient({})) {
  render(<VoiceCallSurface view={view} client={client} projectId="p-1" port={port} onClose={() => undefined} />);
}

describe("VoiceCallSurface", () => {
  it("opens ready when the backend has both speech models", async () => {
    const { port } = fakePort();
    renderSurface(port);
    await waitFor(() => expect(screen.getByTestId("voice-call").dataset.phase).toBe("idle"));
    expect(screen.getByTestId("voice-call-hint").textContent).toContain("Tap and speak");
    expect(screen.getByTestId("voice-call-orb").getAttribute("aria-label")).toBe("Start talking");
  });

  it("states the backend's own reason and keeps typing available when speech is off", async () => {
    const { port } = fakePort({ status: vi.fn(async () => silent) });
    renderSurface(port);
    await waitFor(() => expect(screen.getByTestId("voice-call").dataset.phase).toBe("unavailable"));
    expect(screen.getByTestId("voice-call-error").textContent).toContain("OMON_STT_API_KEY");
    expect((screen.getByTestId("voice-call-orb") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId("voice-call-input") as HTMLInputElement).disabled).toBe(false);
  });

  it("records, transcribes, asks the hub and speaks its answer in one tap", async () => {
    const { port, played } = fakePort();
    const client = fakeClient({ outcome: "admitted", receipt: { goal_id: "g-8" }, revision: 1, acknowledgment: "Goal g-8 is queued." });
    renderSurface(port, client);
    await waitFor(() => expect(screen.getByTestId("voice-call").dataset.phase).toBe("idle"));

    await act(async () => {
      fireEvent.click(screen.getByTestId("voice-call-orb"));
    });

    await waitFor(() => expect(screen.getByTestId("voice-call").dataset.phase).toBe("idle"));
    expect(port.record).toHaveBeenCalledWith(expect.objectContaining({ sampleRate: 24_000, silenceMs: 700 }));
    expect(port.transcribe).toHaveBeenCalledWith("UklGRg==");
    expect(client.request).not.toHaveBeenCalled(); // "status" is answered from the snapshot
    expect(played.map((audio) => atob(audio.audioBase64)).join(" ")).toContain("Write result.txt");
    expect(screen.getByTestId("voice-call-transcript").textContent).toContain("status");
  });

  it("submits a spoken task and refreshes the snapshot", async () => {
    const { port } = fakePort({ transcribe: vi.fn(async () => "Run the tests") });
    const client = fakeClient({ outcome: "conversational", reason: "a question is answered, not launched" });
    renderSurface(port, client);
    await waitFor(() => expect(screen.getByTestId("voice-call").dataset.phase).toBe("idle"));

    await act(async () => {
      fireEvent.click(screen.getByTestId("voice-call-orb"));
    });

    await waitFor(() => expect(client.request).toHaveBeenCalled());
    expect(client.request).toHaveBeenCalledWith(expect.objectContaining({ action: "submit" }));
    expect(client.refresh).not.toHaveBeenCalled();
    expect(screen.getByTestId("voice-call-transcript").textContent).toContain("Nothing was launched");
  });

  it("ignores a take with no speech instead of showing an error", async () => {
    const { port } = fakePort({ record: vi.fn(async () => { throw new Error("nothing-recorded"); }) });
    renderSurface(port);
    await waitFor(() => expect(screen.getByTestId("voice-call").dataset.phase).toBe("idle"));

    await act(async () => {
      fireEvent.click(screen.getByTestId("voice-call-orb"));
    });

    await waitFor(() => expect(screen.getByTestId("voice-call").dataset.phase).toBe("idle"));
    expect(screen.queryByTestId("voice-call-error")).toBeNull();
  });

  it("shows a refused microphone as the error it is", async () => {
    const { port } = fakePort({
      record: vi.fn(async () => {
        throw new Error("The microphone was refused. Allow it in system settings, or type the request.");
      }),
    });
    renderSurface(port);
    await waitFor(() => expect(screen.getByTestId("voice-call").dataset.phase).toBe("idle"));

    await act(async () => {
      fireEvent.click(screen.getByTestId("voice-call-orb"));
    });

    await waitFor(() => expect(screen.getByTestId("voice-call").dataset.phase).toBe("error"));
    expect(screen.getByTestId("voice-call-error").textContent).toContain("microphone was refused");
  });

  it("lets the owner stop a take while it is listening", async () => {
    let settle: (() => void) | null = null;
    const { port } = fakePort({
      record: vi.fn(
        () =>
          new Promise<string>((resolve) => {
            settle = () => resolve("UklGRg==");
          }),
      ),
    });
    renderSurface(port);
    await waitFor(() => expect(screen.getByTestId("voice-call").dataset.phase).toBe("idle"));

    act(() => {
      fireEvent.click(screen.getByTestId("voice-call-orb"));
    });
    await waitFor(() => expect(screen.getByTestId("voice-call").dataset.phase).toBe("listening"));
    expect(screen.getByTestId("voice-call-orb").getAttribute("aria-label")).toBe("Stop listening");

    act(() => {
      fireEvent.click(screen.getByTestId("voice-call-orb"));
    });
    expect(port.stopRecording).toHaveBeenCalled();
    await act(async () => {
      settle?.();
    });
  });

  it("sends a typed request without opening the microphone", async () => {
    const { port } = fakePort();
    const client = fakeClient({ outcome: "admitted", receipt: { goal_id: "g-9" }, revision: 2, acknowledgment: "Goal g-9 is queued." });
    renderSurface(port, client);
    await waitFor(() => expect(screen.getByTestId("voice-call").dataset.phase).toBe("idle"));

    fireEvent.change(screen.getByTestId("voice-call-input"), { target: { value: "Run the tests" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("voice-call-send"));
    });

    await waitFor(() => expect(client.request).toHaveBeenCalled());
    expect(port.record).not.toHaveBeenCalled();
    expect(port.speak).not.toHaveBeenCalled();
    expect(client.refresh).toHaveBeenCalled();
    expect(screen.getByTestId("voice-call-transcript").textContent).toContain("Goal g-9 is queued.");
  });

  it("tears the microphone and playback down when the call closes", async () => {
    const { port } = fakePort();
    const { unmount } = render(
      <VoiceCallSurface view={view} client={fakeClient({})} projectId="p-1" port={port} onClose={() => undefined} />,
    );
    await waitFor(() => expect(screen.getByTestId("voice-call").dataset.phase).toBe("idle"));
    unmount();
    expect(port.stopRecording).toHaveBeenCalled();
    expect(port.stopPlayback).toHaveBeenCalled();
  });
});

