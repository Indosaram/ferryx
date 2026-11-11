import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { VoiceSection } from "./VoiceSection";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

afterEach(cleanup);

/**
 * The section asks two commands, so answers are keyed by command rather than by call order:
 * an order-based mock silently feeds the status answer to whichever read happens first.
 */
function answers(overrides: {
  key?: { saved: boolean; source: "file" | "environment" | null };
  status?: { available: boolean; sttSource: "key" | "codex" | null; speakAvailable: boolean; speakReason: string | null };
  save?: () => Promise<unknown>;
}) {
  const key = overrides.key ?? { saved: false, source: null };
  const status = overrides.status ?? {
    available: true,
    sttSource: "codex" as const,
    speakAvailable: false,
    speakReason: "Spoken replies need a speech key.",
  };
  invoke.mockImplementation(async (command: string) => {
    if (command === "cmd_voice_call_key_state") return key;
    if (command === "cmd_voice_call_status") return status;
    if (command === "cmd_voice_call_set_key") return overrides.save ? overrides.save() : { saved: true, source: "file" };
    throw new Error(`unexpected command ${command}`);
  });
}

describe("VoiceSection", () => {
  it("explains that voice is off and never claims a key it does not have", async () => {
    answers({});
    render(<VoiceSection />);

    await waitFor(() => expect(screen.getByLabelText("Speech provider API key")).toBeTruthy());
    expect(screen.getByText(/Dictation works without a key/)).toBeTruthy();
    expect(screen.queryByText("Saved")).toBeNull();
  });

  it("says dictation is already free when a Codex sign-in covers it", async () => {
    answers({});
    render(<VoiceSection />);

    await waitFor(() => expect(screen.getByText("Text replies only")).toBeTruthy());
    expect(screen.getByText(/Dictation is free through your Codex sign-in/)).toBeTruthy();
  });

  it("offers spoken replies once a key pays for them", async () => {
    answers({
      key: { saved: true, source: "file" },
      status: { available: true, sttSource: "key", speakAvailable: true, speakReason: null },
    });
    render(<VoiceSection />);

    await waitFor(() => expect(screen.getByText("Your account")).toBeTruthy());
    expect(screen.getByText(/Spoken replies use your own provider account/)).toBeTruthy();
  });

  it("saves the typed key and then says it is saved", async () => {
    answers({});
    render(<VoiceSection />);

    const field = await screen.findByLabelText("Speech provider API key");
    await act(async () => {
      fireEvent.change(field, { target: { value: "  sk-test  " } });
      fireEvent.submit(field.closest("form") as HTMLFormElement);
    });

    await waitFor(() => expect(screen.getByText("Saved")).toBeTruthy());
    expect(invoke).toHaveBeenCalledWith("cmd_voice_call_set_key", { apiKey: "  sk-test  " });
  });

  it("does not offer removal for a key that came from the environment", async () => {
    answers({ key: { saved: true, source: "environment" } });
    render(<VoiceSection />);

    await waitFor(() => expect(screen.getByText(/process's environment/)).toBeTruthy());
    expect(screen.queryByLabelText("Remove the saved key")).toBeNull();
  });

  it("shows a save failure instead of pretending the key was saved", async () => {
    answers({
      save: async () => {
        throw new Error("the hub env file is not writable");
      },
    });
    render(<VoiceSection />);

    const field = await screen.findByLabelText("Speech provider API key");
    await act(async () => {
      fireEvent.change(field, { target: { value: "sk-test" } });
      fireEvent.submit(field.closest("form") as HTMLFormElement);
    });

    await waitFor(() => expect(screen.getByRole("alert").textContent).toBe("the hub env file is not writable"));
    expect(screen.queryByText("Saved")).toBeNull();
  });
});
