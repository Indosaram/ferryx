import { describe, expect, it } from "vitest";

import { parseChatPrompt } from "./RemoteApp";

const wire = { promptId: "p1", agent: "claude", kind: "question", title: "Q", question: "Which?", options: [{ label: "A" }] };

describe("parseChatPrompt", () => {
  it("propagates an unknown answer outcome so the card can lock its choices", () => {
    expect(parseChatPrompt({ prompt: { ...wire, answerStatus: "outcomeUnknown" } })?.answerStatus).toBe("outcomeUnknown");
  });

  it("omits answerStatus for any other value", () => {
    expect(parseChatPrompt({ prompt: wire })).not.toHaveProperty("answerStatus");
    expect(parseChatPrompt({ prompt: { ...wire, answerStatus: "ok" } })).not.toHaveProperty("answerStatus");
  });
});
