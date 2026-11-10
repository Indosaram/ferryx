import { describe, expect, it, vi } from "vitest";

import {
  UtteranceGate,
  answerVoiceTurn,
  encodeWav,
  openGoals,
  parseVoiceIntent,
  resample,
  spokenReceipt,
  statusSpeech,
  toInt16,
  type VoiceCallStatus,
} from "./voiceCall";
import type { UnifiedGoal, UnifiedSupervisorClient, UnifiedSupervisorView } from "./unifiedSupervisor";
import type { SupervisorCallResult, SupervisorEnvelope } from "./unifiedSupervisor";

function goal(overrides: Partial<UnifiedGoal> = {}): UnifiedGoal {
  return {
    goalId: "g1",
    supervisorId: "s1",
    projectId: "p1",
    laneKey: "sv-1",
    title: "Get tests green",
    runState: "running",
    stateDetail: null,
    revision: 3,
    pendingWaits: [],
    nextTrigger: null,
    updatedAt: "2026-11-08T00:00:00Z",
    mode: "supervised",
    turnLimit: 12,
    turnsUsed: 1,
    grant: ["edit"],
    ...overrides,
  };
}

function view(goals: UnifiedGoal[]): UnifiedSupervisorView {
  return {
    identity: { supervisorId: "s1", ownerPrincipal: "owner" },
    projects: [{ projectId: "p1", name: "Ferryx" }],
    goals,
    health: null,
    error: null,
  };
}

function client(answers: SupervisorCallResult[]) {
  const request = vi.fn(async (_envelope: SupervisorEnvelope): Promise<SupervisorCallResult> =>
    answers.shift() ?? { kind: "error", code: "NO_ANSWER", message: "no answer queued", status: null },
  );
  const stub: UnifiedSupervisorClient = {
    request,
    conversation: vi.fn(async () => ({ kind: "error" as const, code: "UNUSED", message: "", status: null })),
    memory: vi.fn(async () => ({ kind: "error" as const, code: "UNUSED", message: "", status: null })),
    remember: vi.fn(async () => ({ kind: "error" as const, code: "UNUSED", message: "", status: null })),
    refresh: vi.fn(),
  };
  return { stub, request };
}

describe("encodeWav", () => {
  it("writes a canonical 44-byte RIFF header for mono 16-bit", () => {
    const wav = encodeWav(new Int16Array([0, 1, -1]), 24_000);
    expect(wav.length).toBe(44 + 6);
    const view = new DataView(wav.buffer);
    const ascii = (offset: number, length: number) => String.fromCharCode(...wav.slice(offset, offset + length));
    expect(ascii(0, 4)).toBe("RIFF");
    expect(ascii(8, 4)).toBe("WAVE");
    expect(ascii(12, 4)).toBe("fmt ");
    expect(view.getUint32(4, true)).toBe(36 + 6);
    expect(view.getUint16(20, true)).toBe(1);
    expect(view.getUint16(22, true)).toBe(1);
    expect(view.getUint32(24, true)).toBe(24_000);
    expect(view.getUint32(28, true)).toBe(48_000);
    expect(view.getUint16(32, true)).toBe(2);
    expect(view.getUint16(34, true)).toBe(16);
    expect(ascii(36, 4)).toBe("data");
    expect(view.getUint32(40, true)).toBe(6);
    expect(view.getInt16(44, true)).toBe(0);
    expect(view.getInt16(46, true)).toBe(1);
    expect(view.getInt16(48, true)).toBe(-1);
  });

  it("refuses a non-positive rate instead of writing a broken header", () => {
    expect(() => encodeWav(new Int16Array([0]), 0)).toThrow();
  });
});

describe("resample and toInt16", () => {
  it("returns the input unchanged at the same rate", () => {
    const input = new Float32Array([0.1, 0.2]);
    // A Float32Array stores the nearest float32, so the two constants never compare equal as
    // float64 literals; the values must match the stored precision instead.
    const out = resample(input, 48_000, 48_000);
    expect(out.length).toBe(2);
    expect(out[0]).toBe(input[0]);
    expect(out[1]).toBe(input[1]);
  });

  it("halves the sample count from 48 kHz to 24 kHz", () => {
    const input = new Float32Array(960);
    expect(resample(input, 48_000, 24_000).length).toBe(480);
  });

  it("interpolates between neighbours rather than dropping samples", () => {
    const out = resample(new Float32Array([0, 1, 0, 1]), 4, 2);
    expect(Array.from(out)).toEqual([0, 0]);
  });

  it("clamps instead of wrapping", () => {
    const out = toInt16(new Float32Array([2, -2, 0.5]));
    expect(Array.from(out)).toEqual([32767, -32767, 16384]);
  });
});

describe("UtteranceGate", () => {
  const sampleRate = 16_000;
  const frameMs = 20;
  const frameLength = (sampleRate * frameMs) / 1_000;
  const loud = () => new Float32Array(frameLength).fill(0.2);
  const quiet = () => new Float32Array(frameLength);
  const options = { sampleRate, minSpeechMs: 100, silenceMs: 200, waitForSpeechMs: 1_000, maxSpeechMs: 4_000 };

  function feed(gate: UtteranceGate, frames: Float32Array[]): string[] {
    return frames.map((frame) => gate.push(frame));
  }

  it("keeps listening until speech arrives, then closes after the silence", () => {
    const gate = new UtteranceGate(options);
    expect(feed(gate, Array.from({ length: 10 }, quiet)).every((decision) => decision === "collect")).toBe(true);
    expect(gate.heard).toBe(false);
    feed(gate, Array.from({ length: 6 }, loud));
    const closing = feed(gate, Array.from({ length: 10 }, quiet));
    expect(closing[closing.length - 1]).toBe("close");
    expect(gate.heard).toBe(true);
    expect(gate.samples().length).toBeGreaterThan(3 * frameLength);
  });

  it("discards a take that never heard speech", () => {
    const gate = new UtteranceGate(options);
    const decisions = feed(gate, Array.from({ length: 60 }, quiet));
    expect(decisions[decisions.length - 1]).toBe("discard");
    expect(gate.samples().length).toBe(0);
  });

  it("discards a blip shorter than the minimum speech", () => {
    const gate = new UtteranceGate({ ...options, minSpeechMs: 200 });
    gate.push(loud());
    const closing = feed(gate, Array.from({ length: 12 }, quiet));
    expect(closing[closing.length - 1]).toBe("discard");
  });

  it("closes at the maximum take length even while the speaker continues", () => {
    const gate = new UtteranceGate({ ...options, maxSpeechMs: 200 });
    const decisions = feed(gate, Array.from({ length: 40 }, loud));
    expect(decisions.slice(0, 9).every((decision) => decision === "collect")).toBe(true);
    expect(decisions[9]).toBe("close");
  });

  it("ignores frames after it settled", () => {
    const gate = new UtteranceGate({ ...options, maxSpeechMs: 100 });
    feed(gate, Array.from({ length: 10 }, loud));
    expect(gate.push(loud())).toBe("close");
  });

  it("does not count the pre-roll as speech, so silence alone never opens a take", () => {
    const gate = new UtteranceGate({ ...options, waitForSpeechMs: 60 });
    expect(feed(gate, [quiet(), quiet(), quiet()])).toEqual(["collect", "collect", "discard"]);
  });
});

describe("parseVoiceIntent", () => {
  const goals = [goal(), goal({ goalId: "g2", title: "Ship the docs" })];

  it("treats a status word as a report", () => {
    expect(parseVoiceIntent("status", goals)).toEqual({ kind: "status" });
    expect(parseVoiceIntent("Progress please.", goals)).toEqual({ kind: "status" });
  });

  it("treats anything else as a task", () => {
    expect(parseVoiceIntent("Run the test suite", goals)).toEqual({ kind: "submit", text: "Run the test suite" });
  });

  it("does not read a control word inside a sentence as a command", () => {
    expect(parseVoiceIntent("please pause everything", goals).kind).toBe("submit");
  });

  it("acts on the only open goal without a name", () => {
    const only = [goal()];
    expect(parseVoiceIntent("pause", only)).toMatchObject({ kind: "goal", action: "pause" });
  });

  it("asks which goal when several are open and none is named", () => {
    expect(parseVoiceIntent("pause", goals)).toMatchObject({ kind: "choose-goal", action: "pause" });
  });

  it("acts on the goal whose title contains the spoken name", () => {
    expect(parseVoiceIntent("pause the docs", goals)).toMatchObject({ kind: "goal", action: "pause", goal: { goalId: "g2" } });
  });

  it("never picks a goal for an unmatched name", () => {
    expect(parseVoiceIntent("pause the migration", goals)).toMatchObject({ kind: "choose-goal" });
  });

  it("reports no goal when everything is closed", () => {
    expect(parseVoiceIntent("pause", [goal({ runState: "completed" })])).toEqual({ kind: "no-goal", action: "pause" });
  });

  it("needs the word cancel before cancelling", () => {
    expect(parseVoiceIntent("cancel", [goal()])).toMatchObject({ kind: "goal", action: "cancel" });
    expect(parseVoiceIntent("stop", [goal()])).toMatchObject({ kind: "goal", action: "pause" });
  });
});

describe("spokenReceipt", () => {
  it("speaks the hub's acknowledgment verbatim", () => {
    expect(
      spokenReceipt({ outcome: "admitted", goalId: "g9", revision: 2, runState: "ready", acknowledgment: "Goal g9 is queued.", duplicate: false }),
    ).toBe("Goal g9 is queued.");
  });

  it("falls back to a sentence the receipt supports when there is no acknowledgment", () => {
    expect(
      spokenReceipt({ outcome: "awaiting_owner", goalId: "g9", revision: 4, runState: "awaiting_owner", acknowledgment: "", duplicate: false }),
    ).toBe("Goal g9 registered at revision 4.");
  });

  it("says a question launched nothing", () => {
    expect(spokenReceipt({ outcome: "conversational", reason: "the owner asked a question; a question is answered, not launched" })).toBe(
      "Nothing was launched: the owner asked a question; a question is answered, not launched.",
    );
  });

  it("asks for the missing choice", () => {
    expect(spokenReceipt({ outcome: "needs_choice", missing: "repo", detail: "two repositories match" })).toBe(
      "The request needs repo first: two repositories match",
    );
  });

  it("admits an unreadable receipt instead of guessing", () => {
    expect(spokenReceipt({ outcome: "unknown" })).toContain("cannot read");
  });
});

describe("statusSpeech", () => {
  it("says nothing is open when every goal closed", () => {
    expect(statusSpeech(view([goal({ runState: "completed" })]), "p1")).toBe("Nothing is open. Every goal has finished.");
  });

  it("names each open goal with its wait line", () => {
    const speech = statusSpeech(view([goal(), goal({ goalId: "g2", title: "Ship the docs", runState: "awaiting_owner" })]), "p1");
    expect(speech).toContain("Get tests green: The worker is running");
    expect(speech).toContain("Ship the docs");
  });

  it("caps a long queue and says how many are left", () => {
    const many = Array.from({ length: 5 }, (_, index) => goal({ goalId: `g${index}`, title: `Task ${index}` }));
    expect(statusSpeech(view(many), "p1", 2)).toContain("And 3 more.");
  });

  it("says the view has not loaded rather than inventing a status", () => {
    expect(statusSpeech(null, null)).toContain("not loaded");
  });
});

describe("openGoals", () => {
  it("drops closed goals and other projects", () => {
    const goals = [goal(), goal({ goalId: "g2", runState: "cancelled" }), goal({ goalId: "g3", projectId: "p2" })];
    expect(openGoals(view(goals), "p1").map((entry) => entry.goalId)).toEqual(["g1"]);
  });
});

describe("answerVoiceTurn", () => {
  it("submits a spoken task and speaks the acknowledgment", async () => {
    const { stub, request } = client([
      { kind: "ok", value: { outcome: "admitted", receipt: { goal_id: "g7" }, revision: 1, runState: "ready", acknowledgment: "Goal g7 is queued.", duplicate: false } },
    ]);
    const result = await answerVoiceTurn({ transcript: "Run the tests", client: stub, view: view([]), projectId: "p1" });
    expect(result.speech).toBe("Goal g7 is queued.");
    expect(result.mutated).toBe(true);
    expect(request.mock.calls[0]?.[0]).toMatchObject({ action: "submit", projectId: "p1", payload: { request: "Run the tests" } });
  });

  it("reports that a question launched nothing", async () => {
    const { stub } = client([{ kind: "ok", value: { outcome: "conversational", reason: "a question is answered, not launched" } }]);
    const result = await answerVoiceTurn({ transcript: "what is running", client: stub, view: view([]), projectId: null });
    expect(result.speech).toContain("Nothing was launched");
    expect(result.mutated).toBe(false);
  });

  it("speaks a refusal code rather than pretending the work started", async () => {
    const { stub } = client([{ kind: "error", code: "ADMISSION_REFUSED", message: "no grant", status: 403 }]);
    const result = await answerVoiceTurn({ transcript: "Delete the repo", client: stub, view: view([]), projectId: null });
    expect(result.speech).toContain("ADMISSION_REFUSED");
    expect(result.mutated).toBe(false);
  });

  it("pauses the named goal through the goal envelope", async () => {
    const { stub, request } = client([{ kind: "ok", value: { goalId: "g2" } }]);
    const result = await answerVoiceTurn({
      transcript: "pause the docs",
      client: stub,
      view: view([goal(), goal({ goalId: "g2", title: "Ship the docs" })]),
      projectId: "p1",
    });
    expect(result.speech).toBe("Paused Ship the docs.");
    expect(request.mock.calls[0]?.[0]).toMatchObject({ action: "pause", goalId: "g2", expectedRevision: 3, payload: { reason: "by voice" } });
  });

  it("asks which goal instead of acting when the name matches several", async () => {
    const { stub, request } = client([]);
    const result = await answerVoiceTurn({
      transcript: "pause the tests",
      client: stub,
      view: view([goal({ title: "Run the tests" }), goal({ goalId: "g2", title: "Fix the tests" })]),
      projectId: "p1",
    });
    expect(result.speech).toContain("Which goal");
    expect(request).not.toHaveBeenCalled();
  });

  it("refetches when a control is refused for a stale revision", async () => {
    const { stub } = client([{ kind: "error", code: "REVISION_CONFLICT", message: "moved", status: 409 }]);
    const result = await answerVoiceTurn({ transcript: "resume", client: stub, view: view([goal()]), projectId: "p1" });
    expect(result.mutated).toBe(true);
    expect(result.speech).toContain("REVISION_CONFLICT");
  });

  it("says nothing for an empty transcript", async () => {
    const { stub, request } = client([]);
    const result = await answerVoiceTurn({ transcript: "   ", client: stub, view: view([]), projectId: null });
    expect(result).toEqual({ speech: "", mutated: false, transcript: "" });
    expect(request).not.toHaveBeenCalled();
  });
});

describe("VoiceCallStatus shape", () => {
  it("is what the component reads before recording", () => {
    const status: VoiceCallStatus = {
      available: false,
      reason: "Voice needs a speech key.",
      sttModel: null,
      ttsModel: null,
      sttSource: null,
      speakAvailable: false,
      speakReason: "Spoken replies need a speech key.",
      ttsVoice: "alloy",
      sampleRate: 24_000,
      channels: 1,
      maxUtteranceMs: 30_000,
      minSpeechMs: 400,
      silenceMs: 700,
    };
    expect(status.reason).toContain("speech key");
  });
});

