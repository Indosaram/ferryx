import { describe, expect, it, vi } from "vitest";
import { createReplyWaiter } from "./replyWaiter";

describe("createReplyWaiter", () => {
  it("resolves with the assistant message that appears after the send", async () => {
    const waiter = createReplyWaiter(5_000);
    const reply = waiter.arm("m1");
    waiter.observe("m1", "the earlier answer");
    waiter.observe("m2", "the new answer");

    await expect(reply).resolves.toBe("the new answer");
  });

  it("ignores the baseline message it armed on", async () => {
    const waiter = createReplyWaiter(5_000);
    const reply = waiter.arm("m2");
    waiter.observe("m2", "still the old answer");
    waiter.observe("m3", "  the real answer  ");

    await expect(reply).resolves.toBe("the real answer");
  });

  it("settles empty when the surface never answers", async () => {
    vi.useFakeTimers();
    try {
      const waiter = createReplyWaiter(1_000);
      const reply = waiter.arm("m1");
      vi.advanceTimersByTime(1_001);
      await expect(reply).resolves.toBe("");
    } finally {
      vi.useRealTimers();
    }
  });

  it("settles empty when the send was refused", async () => {
    const waiter = createReplyWaiter(5_000);
    const reply = waiter.arm("m1");
    waiter.disarm();
    waiter.observe("m2", "this must not resurrect a refused turn");

    await expect(reply).resolves.toBe("");
  });

  it("does not leak an answer observed while unarmed", async () => {
    const waiter = createReplyWaiter(5_000);
    waiter.observe("m9", "arrived before any turn");

    const reply = waiter.arm("m1");
    waiter.observe("m2", "the answer to this turn");

    await expect(reply).resolves.toBe("the answer to this turn");
  });
});
