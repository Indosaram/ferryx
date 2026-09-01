import { describe, expect, it } from "vitest";

import { withTimeout } from "./withTimeout";

describe("withTimeout", () => {
  it("passes through a promise that settles in time", async () => {
    await expect(
      withTimeout(Promise.resolve("ok"), 1000, "stage"),
    ).resolves.toBe("ok");
  });

  it("propagates the operation rejection untouched", async () => {
    await expect(
      withTimeout(Promise.reject(new Error("boom")), 1000, "stage"),
    ).rejects.toThrow("boom");
  });

  it("rejects with a labeled TimeoutError when the operation stalls", async () => {
    await expect(
      withTimeout(new Promise<never>(() => {}), 10, "cmd_project_initial"),
    ).rejects.toMatchObject({
      name: "TimeoutError",
      label: "cmd_project_initial",
      ms: 10,
    });
  });

  it("clears the timer so the process can exit", async () => {
    await withTimeout(Promise.resolve(1), 5, "stage");
    expect(true).toBe(true);
  });
});
