import { describe, expect, it, vi } from "vitest";
import { enqueueStrictPersistence } from "./persistenceQueue";

describe("enqueueStrictPersistence", () => {
  it("rejects the transactional caller while keeping the shared save chain usable", async () => {
    const chain = { current: Promise.resolve() };
    const failed = enqueueStrictPersistence(chain, async () => { throw new Error("disk full"); });
    await expect(failed).rejects.toThrow("disk full");

    const next = vi.fn(async () => undefined);
    await enqueueStrictPersistence(chain, next);
    expect(next).toHaveBeenCalledOnce();
  });
});
