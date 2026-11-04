import { describe, expect, it } from "vitest";
import {
  advanceFollowUpIdleClock,
  followUpHistoryAnswerKey,
  type FollowUpIdleClock,
} from "./referenceFollowUpIdle";
import type { ReferenceTurn } from "./referenceTypes";

const clock = (over: Partial<FollowUpIdleClock> = {}): FollowUpIdleClock => ({
  seq: 0,
  wasIdle: false,
  targetKey: null,
  answeredKey: null,
  ...over,
});

const user = (text: string): ReferenceTurn => ({
  id: "u1",
  role: "user",
  parts: [{ kind: "text", text }],
});

const openAssistant = (text: string): ReferenceTurn => ({
  id: "a-open",
  role: "assistant",
  parts: [
    { kind: "text", text },
    { kind: "tool", name: "read", summary: text, input: "", output: "", callId: "c-open", status: "running" },
  ],
});

const textAnswer = (id: string, text: string): ReferenceTurn => ({
  id,
  role: "assistant",
  parts: [{ kind: "text", text }],
});

const closedAssistant = (id: string, output: string): ReferenceTurn => ({
  id,
  role: "assistant",
  parts: [{
    kind: "tool",
    name: "read",
    summary: output,
    input: "",
    output,
    callId: id,
    status: "completed",
  }],
});

describe("follow-up idle clock", () => {
  it("bumps once when the pane enters idle and not again while that observation holds", () => {
    const entered = advanceFollowUpIdleClock(clock(), true, "pane", null);
    expect(entered.seq).toBe(1);
    const held = advanceFollowUpIdleClock(entered, true, "pane", null);
    expect(held.seq).toBe(1);
    expect(held.wasIdle).toBe(true);
  });

  it("bumps while idle when history shows a new closed assistant tail, once per key", () => {
    const idle = advanceFollowUpIdleClock(clock(), true, "pane", null);
    const answered = advanceFollowUpIdleClock(idle, true, "pane", "answer-1");
    expect(answered.seq).toBe(2);
    expect(answered.answeredKey).toBe("answer-1");
    const same = advanceFollowUpIdleClock(answered, true, "pane", "answer-1");
    expect(same.seq).toBe(2);
    const next = advanceFollowUpIdleClock(same, true, "pane", "answer-2");
    expect(next.seq).toBe(3);
  });

  it("does not bump for a null answer key, including a submitted user tail", () => {
    const idle = advanceFollowUpIdleClock(clock({ seq: 4, wasIdle: true, targetKey: "pane", answeredKey: "answer-1" }), true, "pane", null);
    expect(idle.seq).toBe(4);
    expect(idle.answeredKey).toBe("answer-1");
    expect(followUpHistoryAnswerKey([user("submitted")])).toBeNull();
    expect(followUpHistoryAnswerKey([user("hi"), openAssistant("still growing")])).toBeNull();
  });

  it("does not treat an answer that arrives while the pane is not idle as a fresh idle observation", () => {
    const idle = advanceFollowUpIdleClock(clock(), true, "pane", null);
    const working = advanceFollowUpIdleClock(idle, false, "pane", "answer-1");
    expect(working.seq).toBe(1);
    expect(working.wasIdle).toBe(false);
    expect(working.answeredKey).toBe("answer-1");
    const back = advanceFollowUpIdleClock(working, true, "pane", "answer-1");
    expect(back.seq).toBe(2);
  });

  it("counts a target change into idle as a new observation", () => {
    const idle = advanceFollowUpIdleClock(clock(), true, "pane-a", "answer-1");
    const switched = advanceFollowUpIdleClock(idle, true, "pane-b", "answer-1");
    expect(switched.seq).toBeGreaterThan(idle.seq);
    expect(switched.targetKey).toBe("pane-b");
  });

  it("keys a closed assistant tail and ignores an open one", () => {
    const first = textAnswer("a1", "done");
    const second = textAnswer("a2", "done later");
    expect(followUpHistoryAnswerKey([user("q"), first])).toBeTruthy();
    expect(followUpHistoryAnswerKey([user("q"), first])).not.toBe(followUpHistoryAnswerKey([user("q"), second]));
    expect(followUpHistoryAnswerKey([user("q"), textAnswer("a3", "plain answer")])).toBeTruthy();
    expect(followUpHistoryAnswerKey([first, user("submitted")])).toBeNull();
    expect(followUpHistoryAnswerKey([])).toBeNull();
    expect(followUpHistoryAnswerKey(null)).toBeNull();
  });

  it("does not key a tail that sits between two model calls", () => {
    expect(followUpHistoryAnswerKey([user("q"), closedAssistant("a1", "tool result")])).toBeNull();
    const thinkingLast: ReferenceTurn = {
      id: "a-think",
      role: "assistant",
      parts: [{ kind: "text", text: "looking" }, { kind: "thinking", text: "next step" }],
    };
    expect(followUpHistoryAnswerKey([user("q"), thinkingLast])).toBeNull();
    const commentaryLast: ReferenceTurn = {
      id: "a-comm",
      role: "assistant",
      parts: [
        { kind: "text", text: "answer", phase: "finalAnswer" },
        { kind: "text", text: "checking more", phase: "commentary" },
      ],
    };
    expect(followUpHistoryAnswerKey([user("q"), commentaryLast])).toBeNull();
    const toolThenAnswer: ReferenceTurn = {
      id: "a-final",
      role: "assistant",
      parts: [
        ...closedAssistant("a-final", "tool result").parts,
        { kind: "text", text: "the answer", phase: "finalAnswer" },
      ],
    };
    expect(followUpHistoryAnswerKey([user("q"), toolThenAnswer])).toBeTruthy();
  });
});
