import { describe, expect, it } from "vitest";
import {
  VOICE_STATUS,
  VoiceToDraftAdapter,
  appendTranscript,
  detectSpeechRecognition,
  type SpeechRecognitionEventLike,
  type SpeechRecognitionLike,
  type VoicePhaseState,
  type VoiceToDraftOptions,
  type VoiceTranscriptEvent,
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
  public onerror:
    | ((event: { error: string; message?: string }) => void)
    | null = null;
  public onend: (() => void) | null = null;
  public startCalls = 0;
  public abortCalls = 0;
  public stopCalls = 0;
  public throwOnStart = false;

  public start(): void {
    this.startCalls += 1;
    if (this.throwOnStart) throw new Error("InvalidStateError");
  }

  public stop(): void {
    this.stopCalls += 1;
  }

  public abort(): void {
    this.abortCalls += 1;
  }

  public emitResults(
    chunks: readonly ResultChunk[],
    resultIndex = 0
  ): void {
    this.onresult?.({
      resultIndex,
      results: chunks.map((chunk) => ({
        isFinal: chunk.final,
        transcript: chunk.text,
      })),
    });
  }

  public emitError(code: string): void {
    this.onerror?.({ error: code });
  }

  public emitEnd(): void {
    this.onend?.();
  }
}

interface Harness {
  readonly adapter: VoiceToDraftAdapter;
  readonly recognition: FakeRecognition | null;
  readonly finals: VoiceTranscriptEvent[];
  readonly interims: VoiceTranscriptEvent[];
  readonly phases: VoicePhaseState[];
}

function makeAdapter(
  overrides: {
    recognition?: FakeRecognition | null;
    options?: Partial<VoiceToDraftOptions>;
  } = {}
): Harness {
  const recognition =
    overrides.recognition === undefined ? new FakeRecognition() : overrides.recognition;
  const finals: VoiceTranscriptEvent[] = [];
  const interims: VoiceTranscriptEvent[] = [];
  const phases: VoicePhaseState[] = [];
  const adapter = new VoiceToDraftAdapter({
    createRecognition: () => recognition,
    onPhase: (state) => phases.push(state),
    onInterim: (event) => interims.push(event),
    onFinal: (event) => finals.push(event),
    targetKey: "target-a",
    draftRevision: 1,
    ...overrides.options,
  });
  return { adapter, recognition, finals, interims, phases };
}

describe("detectSpeechRecognition", () => {
  it("reports unsupported when neither constructor exists", () => {
    expect(detectSpeechRecognition({})).toBeNull();
  });

  it("reports unsupported on this test runtime, which has no speech service", () => {
    expect(detectSpeechRecognition()).toBeNull();
  });

  it("prefers the unprefixed constructor and falls back to webkit", () => {
    class Prefixed implements SpeechRecognitionLike {
      continuous = false;
      interimResults = false;
      lang = "";
      onresult = null;
      onerror = null;
      onend = null;
      start(): void {}
      stop(): void {}
      abort(): void {}
    }
    class Unprefixed extends Prefixed {}
    expect(detectSpeechRecognition({ SpeechRecognition: Unprefixed })).toBe(
      Unprefixed
    );
    expect(detectSpeechRecognition({ webkitSpeechRecognition: Prefixed })).toBe(
      Prefixed
    );
    expect(
      detectSpeechRecognition({
        SpeechRecognition: Unprefixed,
        webkitSpeechRecognition: Prefixed,
      })
    ).toBe(Unprefixed);
  });
});

describe("appendTranscript", () => {
  it("writes into an empty draft", () => {
    expect(appendTranscript("", "hello")).toBe("hello");
  });

  it("appends after existing user text without clobbering it", () => {
    expect(appendTranscript("existing draft", "and more")).toBe(
      "existing draft and more"
    );
  });

  it("does not double the separator when the draft already ends in whitespace", () => {
    expect(appendTranscript("draft ", "more")).toBe("draft more");
    expect(appendTranscript("draft\n", "more")).toBe("draft\nmore");
  });

  it("ignores an empty commit", () => {
    expect(appendTranscript("draft", "   ")).toBe("draft");
  });
});

describe("VoiceToDraftAdapter support and start", () => {
  it("is honestly unsupported when the factory yields no service", () => {
    const harness = makeAdapter({ recognition: null });
    expect(harness.adapter.supported).toBe(false);
    expect(harness.adapter.state.phase).toBe("unsupported");
    expect(harness.adapter.state.message).toBe(VOICE_STATUS.unsupported);
    expect(harness.adapter.start()).toBe(false);
    expect(harness.adapter.state.phase).toBe("unsupported");
  });

  it("treats a throwing factory as unsupported instead of crashing", () => {
    const adapter = new VoiceToDraftAdapter({
      createRecognition: () => {
        throw new Error("blocked by policy");
      },
      onPhase: () => undefined,
      onInterim: () => undefined,
      onFinal: () => undefined,
      targetKey: "target-a",
    });
    expect(adapter.state.phase).toBe("unsupported");
  });

  it("starts a user-activated session with interim results enabled", () => {
    const harness = makeAdapter();
    expect(harness.adapter.state.phase).toBe("idle");
    expect(harness.adapter.start()).toBe(true);
    expect(harness.recognition?.startCalls).toBe(1);
    expect(harness.recognition?.continuous).toBe(true);
    expect(harness.recognition?.interimResults).toBe(true);
    expect(harness.adapter.state.phase).toBe("listening");
    expect(harness.adapter.currentSessionId).toBe(1);
  });

  it("does not start a second overlapping session", () => {
    const harness = makeAdapter();
    expect(harness.adapter.start()).toBe(true);
    expect(harness.adapter.start()).toBe(false);
    expect(harness.recognition?.startCalls).toBe(1);
  });

  it("reports a failed start without pretending to be recording", () => {
    const recognition = new FakeRecognition();
    const harness = makeAdapter({ recognition });
    expect(harness.adapter.start()).toBe(true);
    harness.adapter.cancel();
    recognition.throwOnStart = true;
    expect(harness.adapter.start()).toBe(false);
    expect(harness.adapter.state.phase).toBe("failed");
    expect(harness.adapter.state.message).toBe(VOICE_STATUS.startFailed);
    expect(harness.finals).toEqual([]);
    expect(harness.interims).toEqual([]);
  });
});

describe("VoiceToDraftAdapter final-only delivery", () => {
  it("routes interim results to preview and never to the draft commit", () => {
    const harness = makeAdapter();
    harness.adapter.start();
    harness.recognition?.emitResults([{ text: "  hello wor ", final: false }]);
    expect(harness.interims).toHaveLength(1);
    expect(harness.interims[0]?.text).toBe("hello wor");
    expect(harness.finals).toHaveLength(0);
  });

  it("commits only final results, fenced by session, target and draft revision", () => {
    const harness = makeAdapter();
    harness.adapter.start();
    harness.recognition?.emitResults([
      { text: "draft preview", final: false },
      { text: " finished.", final: true },
    ]);
    expect(harness.interims).toHaveLength(1);
    expect(harness.finals).toHaveLength(1);
    expect(harness.finals[0]).toEqual({
      sessionId: 1,
      targetKey: "target-a",
      draftRevision: 1,
      text: "finished.",
    });
  });

  it("skips results before resultIndex", () => {
    const harness = makeAdapter();
    harness.adapter.start();
    harness.recognition?.emitResults(
      [
        { text: "already handled", final: true },
        { text: "new", final: true },
      ],
      1
    );
    expect(harness.finals).toHaveLength(1);
    expect(harness.finals[0]?.text).toBe("new");
  });
});

describe("VoiceToDraftAdapter fencing of late results", () => {
  it("drops results that arrive after cancel", () => {
    const harness = makeAdapter();
    harness.adapter.start();
    harness.adapter.cancel();
    expect(harness.recognition?.abortCalls).toBe(1);
    expect(harness.adapter.state.phase).toBe("idle");
    harness.recognition?.emitResults([{ text: "late", final: true }]);
    expect(harness.finals).toHaveLength(0);
    expect(harness.interims).toHaveLength(0);
  });

  it("drops results that arrive after the session ended", () => {
    const harness = makeAdapter();
    harness.adapter.start();
    harness.recognition?.emitEnd();
    expect(harness.adapter.state.phase).toBe("idle");
    harness.recognition?.emitResults([{ text: "after end", final: true }]);
    expect(harness.finals).toHaveLength(0);
  });

  it("cancels and fences on target change so text cannot cross targets", () => {
    const harness = makeAdapter();
    harness.adapter.start();
    harness.adapter.setTargetKey("target-b");
    expect(harness.recognition?.abortCalls).toBe(1);
    harness.recognition?.emitResults([{ text: "belongs to old target", final: true }]);
    expect(harness.finals).toHaveLength(0);
  });

  it("cancels and fences on draft revision change so a late result cannot fill a newer draft", () => {
    const harness = makeAdapter();
    harness.adapter.start();
    harness.adapter.setDraftRevision(2);
    expect(harness.recognition?.abortCalls).toBe(1);
    harness.recognition?.emitResults([{ text: "stale draft text", final: true }]);
    expect(harness.finals).toHaveLength(0);
  });

  it("keeps a session running when the fence does not move", () => {
    const harness = makeAdapter();
    harness.adapter.start();
    harness.adapter.setTargetKey("target-a");
    harness.adapter.setDraftRevision(1);
    expect(harness.recognition?.abortCalls).toBe(0);
    expect(harness.adapter.listening).toBe(true);
    harness.recognition?.emitResults([{ text: "kept", final: true }]);
    expect(harness.finals).toHaveLength(1);
  });

  it("drops results carried by a stale handler from a previous session", () => {
    const harness = makeAdapter();
    harness.adapter.start();
    const staleHandler = harness.recognition?.onresult ?? null;
    expect(staleHandler).not.toBeNull();
    harness.adapter.cancel();
    harness.adapter.start();
    expect(harness.adapter.currentSessionId).toBe(2);
    staleHandler?.({
      resultIndex: 0,
      results: [{ isFinal: true, transcript: "from session one" }],
    });
    expect(harness.finals).toHaveLength(0);
    harness.recognition?.emitResults([{ text: "session two", final: true }]);
    expect(harness.finals).toHaveLength(1);
    expect(harness.finals[0]?.sessionId).toBe(2);
  });

  it("drops results after dispose and detaches every handler", () => {
    const harness = makeAdapter();
    harness.adapter.start();
    harness.adapter.dispose();
    expect(harness.recognition?.abortCalls).toBe(1);
    expect(harness.recognition?.onresult).toBeNull();
    expect(harness.recognition?.onerror).toBeNull();
    expect(harness.recognition?.onend).toBeNull();
  });
});

describe("VoiceToDraftAdapter truthful failure states", () => {
  it("reports denied permission without emitting any text", () => {
    const harness = makeAdapter();
    harness.adapter.start();
    harness.recognition?.emitError("not-allowed");
    expect(harness.adapter.state.phase).toBe("denied");
    expect(harness.adapter.state.message).toBe(VOICE_STATUS.deniedPermission);
    expect(harness.adapter.listening).toBe(false);
    expect(harness.finals).toEqual([]);
    expect(harness.interims).toEqual([]);
  });

  it("reports a blocked service distinctly from a denied microphone", () => {
    const harness = makeAdapter();
    harness.adapter.start();
    harness.recognition?.emitError("service-not-allowed");
    expect(harness.adapter.state.phase).toBe("denied");
    expect(harness.adapter.state.message).toBe(VOICE_STATUS.deniedService);
  });

  it("treats an aborted session as an ordinary stop", () => {
    const harness = makeAdapter();
    harness.adapter.start();
    harness.recognition?.emitError("aborted");
    expect(harness.adapter.state.phase).toBe("idle");
    expect(harness.adapter.state.message).toBeNull();
  });

  it("reports no-speech and network failures without touching the draft", () => {
    const noSpeech = makeAdapter();
    noSpeech.adapter.start();
    noSpeech.recognition?.emitError("no-speech");
    expect(noSpeech.adapter.state.phase).toBe("failed");
    expect(noSpeech.adapter.state.message).toBe(VOICE_STATUS.noSpeech);

    const network = makeAdapter();
    network.adapter.start();
    network.recognition?.emitError("network");
    expect(network.adapter.state.phase).toBe("failed");
    expect(network.adapter.state.message).toBe(VOICE_STATUS.network);

    expect(noSpeech.finals).toEqual([]);
    expect(network.finals).toEqual([]);
  });

  it("ignores errors from a stale session", () => {
    const harness = makeAdapter();
    harness.adapter.start();
    const staleError = harness.recognition?.onerror ?? null;
    harness.adapter.cancel();
    staleError?.({ error: "not-allowed" });
    expect(harness.adapter.state.phase).toBe("idle");
  });

  it("allows an explicit retry after a failure", () => {
    const harness = makeAdapter();
    harness.adapter.start();
    harness.recognition?.emitError("no-speech");
    expect(harness.adapter.start()).toBe(true);
    expect(harness.adapter.state.phase).toBe("listening");
  });
});

describe("VoiceToDraftAdapter cannot send", () => {
  it("exposes no send, submit or dispatch surface", () => {
    const harness = makeAdapter();
    const surface = [
      ...Object.getOwnPropertyNames(harness.adapter),
      ...Object.getOwnPropertyNames(VoiceToDraftAdapter.prototype),
    ].filter((name) => /send|submit|dispatch|publish/i.test(name));
    expect(surface).toEqual([]);
  });
});
