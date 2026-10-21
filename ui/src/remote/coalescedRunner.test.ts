import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createCoalescedRunner } from "./coalescedRunner";
import { contextName } from "./RemoteSessionList";

describe("contextName", () => {
  it("names an SSH-host project by its folder, never its opaque id", () => {
    expect(contextName({ workspaceId: "ssh:6016f094", worktreeLabel: "mhc", worktreeSlug: null })).toBe("mhc");
    expect(contextName({ workspaceId: "noveling", worktreeLabel: "main", worktreeSlug: null })).toBe("noveling / main");
  });
});

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => { resolve = r; });
  return { promise, resolve };
}

describe("createCoalescedRunner", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("runs a burst of triggers landing during a slow run as one trailing run", async () => {
    const runs: ReturnType<typeof deferred>[] = [];
    const settled = vi.fn();
    const runner = createCoalescedRunner(() => {
      const run = deferred();
      runs.push(run);
      return run.promise;
    }, 150, settled);

    runner.trigger();
    await vi.advanceTimersByTimeAsync(150);
    expect(runs).toHaveLength(1);

    // A busy desktop republishes its inventory every ~400ms while the first read is slow.
    for (let i = 0; i < 20; i += 1) {
      runner.trigger();
      await vi.advanceTimersByTimeAsync(400);
    }
    expect(runs).toHaveLength(1);

    runs[0].resolve();
    await vi.advanceTimersByTimeAsync(150);
    expect(settled).toHaveBeenCalledTimes(1);
    expect(runs).toHaveLength(2);

    runs[1].resolve();
    await vi.advanceTimersByTimeAsync(1000);
    expect(runs).toHaveLength(2);
    expect(settled).toHaveBeenCalledTimes(2);
  });

  it("keeps running after a failed run and stops after cancel", async () => {
    let calls = 0;
    const runner = createCoalescedRunner(() => {
      calls += 1;
      return Promise.reject(new Error("tunnel closed"));
    }, 150);

    runner.trigger();
    await vi.advanceTimersByTimeAsync(150);
    runner.trigger();
    await vi.advanceTimersByTimeAsync(150);
    expect(calls).toBe(2);

    runner.trigger();
    runner.cancel();
    await vi.advanceTimersByTimeAsync(1000);
    expect(calls).toBe(2);
  });
});
