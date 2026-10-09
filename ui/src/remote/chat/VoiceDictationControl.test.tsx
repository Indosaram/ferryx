import { useState } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { VoiceDictationControl } from "./VoiceDictationControl";
import {
  VOICE_STATUS,
  type SpeechRecognitionEventLike,
  type SpeechRecognitionLike,
} from "./voiceToDraft";

interface ResultChunk {
  readonly text: string;
  readonly final: boolean;
}

class FakeRecognition implements SpeechRecognitionLike {
  public continuous = false;
  public interimResults = false;
  public lang = "";
  public onresult: ((event: SpeechRecognitionEventLike) => void) | null = null;
  public onerror: ((event: { error: string }) => void) | null = null;
  public onend: (() => void) | null = null;
  public startCalls = 0;
  public abortCalls = 0;

  public start(): void {
    this.startCalls += 1;
  }

  public stop(): void {}

  public abort(): void {
    this.abortCalls += 1;
  }

  public emit(chunks: readonly ResultChunk[]): void {
    this.onresult?.({
      resultIndex: 0,
      results: chunks.map((chunk) => ({
        isFinal: chunk.final,
        transcript: chunk.text,
      })),
    });
  }

  public emitFinal(text: string): void {
    act(() => {
      this.emit([{ text, final: true }]);
    });
  }

  public emitInterim(text: string): void {
    act(() => {
      this.emit([{ text, final: false }]);
    });
  }

  public emitError(code: string): void {
    act(() => {
      this.onerror?.({ error: code });
    });
  }
}

interface HarnessProps {
  readonly targetKey?: string;
  readonly createRecognition: () => FakeRecognition | null;
  readonly onSend: (text: string) => void;
  readonly initialText?: string;
  readonly disabled?: boolean;
}

function HarnessComposer({
  targetKey = "target-a",
  createRecognition,
  onSend,
  initialText = "",
  disabled,
}: HarnessProps) {
  const [text, setText] = useState(initialText);
  const [revision, setRevision] = useState(0);
  return (
    <div>
      <textarea
        data-testid="draft"
        value={text}
        onChange={(event) => setText(event.target.value)}
      />
      <VoiceDictationControl
        targetKey={targetKey}
        draftRevision={revision}
        value={text}
        onChange={setText}
        createRecognition={createRecognition}
        disabled={disabled}
      />
      <button
        type="button"
        data-testid="send"
        onClick={() => {
          onSend(text);
          setText("");
          setRevision((current) => current + 1);
        }}
      >
        Send
      </button>
    </div>
  );
}

function draft() {
  return screen.getByTestId("draft") as HTMLTextAreaElement;
}

function mic() {
  return screen.getByTestId("voice-dictation-button") as HTMLButtonElement;
}

afterEach(cleanup);

describe("VoiceDictationControl unsupported surface", () => {
  it("tells the truth when the browser has no speech service", () => {
    const onSend = vi.fn();
    render(<HarnessComposer createRecognition={() => null} onSend={onSend} />);

    expect(mic()).toBeDisabled();
    expect(mic()).toHaveAccessibleName(
      "Voice input is not supported in this browser"
    );
    expect(screen.getByTestId("voice-dictation-status")).toHaveTextContent(
      VOICE_STATUS.unsupported
    );
    expect(screen.queryByTestId("voice-dictation-interim")).toBeNull();

    fireEvent.click(mic());
    expect(draft()).toHaveValue("");
    expect(onSend).not.toHaveBeenCalled();
    expect(screen.queryByTestId("voice-dictation-status")).toHaveTextContent(
      VOICE_STATUS.unsupported
    );
  });
});

describe("VoiceDictationControl dictation flow", () => {
  it("starts only on user activation and discloses vendor processing", () => {
    const recognition = new FakeRecognition();
    const onSend = vi.fn();
    render(
      <HarnessComposer createRecognition={() => recognition} onSend={onSend} />
    );

    expect(recognition.startCalls).toBe(0);
    expect(screen.queryByTestId("voice-dictation-status")).toBeNull();

    fireEvent.click(mic());
    expect(recognition.startCalls).toBe(1);
    expect(mic()).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByTestId("voice-dictation-status")).toHaveTextContent(
      VOICE_STATUS.listening
    );
    expect(onSend).not.toHaveBeenCalled();
  });

  it("keeps interim speech as a preview instead of committing it", () => {
    const recognition = new FakeRecognition();
    render(<HarnessComposer createRecognition={() => recognition} onSend={vi.fn()} />);

    fireEvent.click(mic());
    recognition.emitInterim("half a sen");
    expect(screen.getByTestId("voice-dictation-interim")).toHaveTextContent(
      "half a sen"
    );
    expect(draft()).toHaveValue("");

    recognition.emitFinal("half a sentence.");
    expect(draft()).toHaveValue("half a sentence.");
    expect(screen.queryByTestId("voice-dictation-interim")).toBeNull();
  });

  it("commits final text into an editable draft that the user can keep editing", () => {
    const recognition = new FakeRecognition();
    render(<HarnessComposer createRecognition={() => recognition} onSend={vi.fn()} />);

    fireEvent.click(mic());
    recognition.emitFinal("dictated");
    expect(draft()).toHaveValue("dictated");

    fireEvent.change(draft(), { target: { value: "dictated by hand" } });
    expect(draft()).toHaveValue("dictated by hand");

    recognition.emitFinal("more");
    expect(draft()).toHaveValue("dictated by hand more");
  });

  it("never auto-sends: sending stays an explicit user action", () => {
    const recognition = new FakeRecognition();
    const onSend = vi.fn();
    render(<HarnessComposer createRecognition={() => recognition} onSend={onSend} />);

    fireEvent.click(mic());
    recognition.emitInterim("do not send me");
    recognition.emitFinal("send me explicitly");
    expect(onSend).not.toHaveBeenCalled();
    expect(draft()).toHaveValue("send me explicitly");

    fireEvent.click(screen.getByTestId("send"));
    expect(onSend).toHaveBeenCalledTimes(1);
    expect(onSend).toHaveBeenCalledWith("send me explicitly");
    expect(draft()).toHaveValue("");
  });

  it("stops on the second press and fences the late result", () => {
    const recognition = new FakeRecognition();
    render(<HarnessComposer createRecognition={() => recognition} onSend={vi.fn()} />);

    fireEvent.click(mic());
    const lateResult = recognition.onresult;
    expect(lateResult).not.toBeNull();

    fireEvent.click(mic());
    expect(recognition.abortCalls).toBe(1);
    expect(mic()).toHaveAttribute("aria-pressed", "false");
    expect(screen.queryByTestId("voice-dictation-status")).toBeNull();

    act(() => {
      lateResult?.({
        resultIndex: 0,
        results: [{ isFinal: true, transcript: "too late" }],
      });
    });
    expect(draft()).toHaveValue("");
  });

  it("shows a denied microphone honestly and leaves the draft untouched", () => {
    const recognition = new FakeRecognition();
    const onSend = vi.fn();
    render(<HarnessComposer createRecognition={() => recognition} onSend={onSend} />);

    fireEvent.click(mic());
    recognition.emitError("not-allowed");

    expect(screen.getByTestId("voice-dictation-status")).toHaveTextContent(
      VOICE_STATUS.deniedPermission
    );
    expect(mic()).toHaveAttribute("aria-pressed", "false");
    expect(draft()).toHaveValue("");
    expect(onSend).not.toHaveBeenCalled();
  });
});

describe("VoiceDictationControl target and draft fencing", () => {
  it("cancels on target change and drops the stale result", () => {
    const recognition = new FakeRecognition();
    const onSend = vi.fn();
    const props = { createRecognition: () => recognition, onSend };
    const { rerender } = render(<HarnessComposer {...props} />);

    fireEvent.click(mic());
    const staleResult = recognition.onresult;
    expect(staleResult).not.toBeNull();

    rerender(<HarnessComposer {...props} targetKey="target-b" />);
    expect(recognition.abortCalls).toBe(1);
    expect(screen.queryByTestId("voice-dictation-status")).toBeNull();

    act(() => {
      staleResult?.({
        resultIndex: 0,
        results: [{ isFinal: true, transcript: "belongs to target-a" }],
      });
    });
    expect(draft()).toHaveValue("");
    expect(onSend).not.toHaveBeenCalled();
  });

  it("cancels when the draft is sent mid-dictation and fences the stale result", () => {
    const recognition = new FakeRecognition();
    const onSend = vi.fn();
    render(<HarnessComposer createRecognition={() => recognition} onSend={onSend} />);

    fireEvent.click(mic());
    const staleResult = recognition.onresult;
    recognition.emitFinal("committed before send");

    fireEvent.click(screen.getByTestId("send"));
    expect(onSend).toHaveBeenCalledWith("committed before send");
    expect(recognition.abortCalls).toBe(1);
    expect(draft()).toHaveValue("");

    act(() => {
      staleResult?.({
        resultIndex: 0,
        results: [{ isFinal: true, transcript: "after the send" }],
      });
    });
    expect(draft()).toHaveValue("");
  });

  it("cancels dictation when the control is disabled mid-session", () => {
    const recognition = new FakeRecognition();
    const props = { createRecognition: () => recognition, onSend: vi.fn() };
    const { rerender } = render(<HarnessComposer {...props} />);

    fireEvent.click(mic());
    expect(recognition.startCalls).toBe(1);
    rerender(<HarnessComposer {...props} disabled />);
    expect(recognition.abortCalls).toBe(1);
    expect(mic()).toBeDisabled();
  });

  it("does nothing when the control is disabled from the start", () => {
    const recognition = new FakeRecognition();
    render(
      <HarnessComposer
        createRecognition={() => recognition}
        onSend={vi.fn()}
        disabled
      />
    );
    fireEvent.click(mic());
    expect(recognition.startCalls).toBe(0);
    expect(draft()).toHaveValue("");
  });
});
