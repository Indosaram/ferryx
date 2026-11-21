import { describe, expect, it, vi } from "vitest";
import {
  createDesktopContinuityPublisher,
  type DesktopContinuityAccount,
  type DesktopContinuityIntent,
  type DesktopContinuityRevision,
  type DesktopContinuityTarget,
} from "./desktopContinuityPublisher";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

const account: DesktopContinuityAccount = {
  accountOrigin: "https://account.test", accountId: "user-a", sessionToken: "token-a",
};
const target: DesktopContinuityTarget = {
  machineId: "machine", workspaceId: "workspace", worktreeSlug: null, sessionId: "backend-pty",
};

function fixture() {
  let currentAccount: DesktopContinuityAccount | null = account;
  let revision = 7;
  const scheduled: Array<() => Promise<void>> = [];
  const read = vi.fn(async (_credentials: { accountOrigin: string; sessionToken: string },
    _signal: AbortSignal): Promise<DesktopContinuityRevision | null> => ({ revision }));
  const write = vi.fn(async (_account: DesktopContinuityAccount, _target: DesktopContinuityTarget,
    expectedRevision: number, _signal: AbortSignal) => {
    if (expectedRevision !== revision) throw { code: "CONTINUITY_CONFLICT", details: { current: { revision } } };
    revision += 1;
    return { revision };
  });
  const onError = vi.fn();
  const publisher = createDesktopContinuityPublisher({
    currentAccount: () => currentAccount,
    read,
    write,
    onError,
    schedule(callback) {
      scheduled.push(callback);
      return () => {
        const index = scheduled.indexOf(callback);
        if (index >= 0) scheduled.splice(index, 1);
      };
    },
  });
  return {
    publisher, read, write, onError,
    setAccount(value: DesktopContinuityAccount | null) { currentAccount = value; },
    publishFromAnotherDevice() { revision += 1; },
    pointerRevision() { return revision; },
    intent(): DesktopContinuityIntent { return publisher.begin(account); },
    async flush() { const callback = scheduled.shift(); await callback?.(); },
  };
}

describe("desktop continuity publication", () => {
  it("does not publish without a selection or while signed out", async () => {
    const f = fixture();
    await f.flush();
    const intent = f.intent();
    f.setAccount(null);
    f.publisher.select(target, intent);
    await f.flush();
    expect(f.write).not.toHaveBeenCalled();
  });

  it("publishes the backend session and nullable root slug with its baseline revision", async () => {
    const f = fixture();
    f.publisher.select(target, f.intent());
    expect(f.write).not.toHaveBeenCalled();
    await f.flush();
    expect(f.write).toHaveBeenCalledWith(account, target, 7, expect.any(AbortSignal));
  });

  it("reads the baseline once per intent, at the moment of the selection", async () => {
    const f = fixture();
    const intent = f.intent();
    expect(f.read).toHaveBeenCalledExactlyOnceWith(
      { accountOrigin: account.accountOrigin, sessionToken: account.sessionToken }, expect.any(AbortSignal));
    f.publisher.select(target, intent);
    expect(f.read).toHaveBeenCalledTimes(1);
    await f.flush();
    expect(f.write).toHaveBeenCalledWith(account, target, 7, expect.any(AbortSignal));
    expect(f.read).toHaveBeenCalledTimes(1);
  });

  it("loses to a newer publication instead of adopting its revision as the baseline", async () => {
    const f = fixture();
    const intent = f.intent();
    f.publishFromAnotherDevice();
    f.publisher.select(target, intent);
    await f.flush();
    expect(f.write).toHaveBeenCalledExactlyOnceWith(account, target, 7, expect.any(AbortSignal));
    expect(f.onError).not.toHaveBeenCalled();
    expect(f.pointerRevision()).toBe(8);
    f.publisher.select({ ...target, sessionId: "next-backend" }, f.intent());
    await f.flush();
    expect(f.write).toHaveBeenLastCalledWith(account, { ...target, sessionId: "next-backend" }, 8,
      expect.any(AbortSignal));
  });

  it("keeps the intent-time baseline through a slow baseline read", async () => {
    const f = fixture();
    const pending = deferred<DesktopContinuityRevision | null>();
    f.read.mockReturnValueOnce(pending.promise);
    const intent = f.intent();
    f.publishFromAnotherDevice();
    f.publisher.select(target, intent);
    pending.resolve({ revision: 7 });
    await f.flush();
    expect(f.write).toHaveBeenCalledExactlyOnceWith(account, target, 7, expect.any(AbortSignal));
    expect(f.onError).not.toHaveBeenCalled();
    expect(f.pointerRevision()).toBe(8);
  });

  it("publishes with the intent-time baseline after account identity resolves late", async () => {
    const f = fixture();
    f.setAccount({ accountOrigin: account.accountOrigin, accountId: "", sessionToken: account.sessionToken });
    const intent = f.intent();
    f.setAccount(account);
    f.publisher.select(target, intent);
    await f.flush();
    expect(f.write).toHaveBeenCalledExactlyOnceWith(account, target, 7, expect.any(AbortSignal));
  });

  it("debounces superseded intent using a dedicated generation", async () => {
    const f = fixture();
    const first = f.intent();
    f.publisher.select(target, first);
    const firstSignal = f.read.mock.calls[0]?.[1];
    const next = { ...target, sessionId: "backend-next" };
    f.publisher.select(next, f.intent());
    await f.flush();
    expect(firstSignal?.aborted).toBe(true);
    expect(f.write).toHaveBeenCalledTimes(1);
    expect(f.write).toHaveBeenCalledWith(account, next, 7, expect.any(AbortSignal));
  });

  it("drops an older intent selected after a newer one", async () => {
    const f = fixture();
    const older = f.intent();
    const newer = f.intent();
    f.publisher.select({ ...target, sessionId: "newer" }, newer);
    f.publisher.select(target, older);
    await f.flush();
    expect(f.write).toHaveBeenCalledExactlyOnceWith(account, { ...target, sessionId: "newer" }, 7,
      expect.any(AbortSignal));
  });

  it("does not retry a conflict; a fresh explicit selection may publish", async () => {
    const f = fixture();
    const intent = f.intent();
    f.publishFromAnotherDevice();
    f.publisher.select(target, intent);
    await f.flush();
    expect(f.onError).not.toHaveBeenCalled();
    await f.flush();
    expect(f.write).toHaveBeenCalledExactlyOnceWith(account, target, 7, expect.any(AbortSignal));
    await f.flush();
    expect(f.write).toHaveBeenCalledTimes(1);
    f.publisher.select(target, f.intent());
    await f.flush();
    expect(f.write).toHaveBeenLastCalledWith(account, target, 8, expect.any(AbortSignal));
  });

  it("reports non-conflict write failures without retrying", async () => {
    const f = fixture();
    const failure = { code: "CONTINUITY_WRITE_FAILED" };
    f.publisher.select(target, f.intent());
    f.write.mockRejectedValueOnce(failure);
    await f.flush();
    expect(f.onError).toHaveBeenCalledExactlyOnceWith(failure);
    expect(f.write).toHaveBeenCalledTimes(1);
  });

  it.each([null, undefined])(
    "reports a baseline read rejection carrying %s instead of publishing it as a baseline",
    async (reason) => {
      const f = fixture();
      const pending = deferred<DesktopContinuityRevision | null>();
      f.read.mockReturnValueOnce(pending.promise);
      f.publisher.select(target, f.intent());
      const finished = f.flush();
      pending.reject(reason);
      await finished;
      expect(f.onError).toHaveBeenCalledExactlyOnceWith(reason);
      expect(f.write).not.toHaveBeenCalled();
    },
  );

  it("publishes the first-ever selection with revision 0 when no pointer exists", async () => {
    const f = fixture();
    f.read.mockReturnValueOnce(Promise.resolve(null));
    f.publisher.select(target, f.intent());
    await f.flush();
    expect(f.onError).not.toHaveBeenCalled();
    expect(f.write).toHaveBeenCalledExactlyOnceWith(account, target, 0, expect.any(AbortSignal));
  });

  it.each([false, true])("orders two local selections without rebasing over a later phone write (%s)", async (phoneWins) => {
    const f = fixture();
    const started = deferred<void>();
    const finish = deferred<void>();
    const commit = f.write.getMockImplementation()!;
    f.write.mockImplementationOnce(async (...args) => {
      started.resolve();
      await finish.promise;
      expect(args[3].aborted).toBe(false);
      const result = await commit(...args);
      if (phoneWins) f.publishFromAnotherDevice();
      return result;
    });
    f.publisher.select(target, f.intent());
    const first = f.flush();
    await started.promise;
    const newer = { ...target, sessionId: "second-session" };
    f.publisher.select(newer, f.intent());
    const second = f.flush();
    finish.resolve();
    await Promise.all([first, second]);
    expect(f.write).toHaveBeenCalledTimes(2);
    expect(f.write).toHaveBeenLastCalledWith(account, newer, 8, expect.any(AbortSignal));
    expect(f.pointerRevision()).toBe(9);
  });

  it("does not let a late conflict cancel a newer explicit selection", async () => {
    const f = fixture();
    const started = deferred<void>();
    const response = deferred<DesktopContinuityRevision>();
    f.write.mockImplementationOnce(() => {
      started.resolve();
      return response.promise;
    });
    f.publisher.select(target, f.intent());
    const firstFinished = f.flush();
    await started.promise;
    const next = { ...target, sessionId: "newer-session" };
    f.publisher.select(next, f.intent());
    response.reject({ code: "CONTINUITY_CONFLICT" });
    await firstFinished;
    await f.flush();
    expect(f.onError).not.toHaveBeenCalled();
    expect(f.write).toHaveBeenCalledTimes(2);
    expect(f.write).toHaveBeenLastCalledWith(account, next, 7, expect.any(AbortSignal));
  });

  it.each([null, { ...account, accountId: "user-b", sessionToken: "token-b" },
    { ...account, sessionToken: "replacement-token" },
    { ...account, accountOrigin: "https://other.test" }])(
    "drops queued intent when credentials change without replaying it", async (replacement) => {
      const f = fixture();
      const pending = deferred<DesktopContinuityRevision | null>();
      f.read.mockReturnValueOnce(pending.promise);
      const intent = f.intent();
      f.publisher.select(target, intent);
      f.setAccount(replacement);
      pending.resolve({ revision: 7 });
      await pending.promise;
      await f.flush();
      expect(f.write).not.toHaveBeenCalled();
      expect(f.onError).not.toHaveBeenCalled();
    },
  );

  it.each(["before-select", "after-select"] as const)(
    "drops an intent whose credentials changed %s", async (boundary) => {
      const f = fixture();
      const intent = f.intent();
      if (boundary === "before-select") {
        f.setAccount({ ...account, sessionToken: "replacement-token" });
        f.publisher.select(target, intent);
      } else {
        f.publisher.select(target, intent);
        f.setAccount({ ...account, sessionToken: "replacement-token" });
      }
      await f.flush();
      expect(f.write).not.toHaveBeenCalled();
      expect(f.onError).not.toHaveBeenCalled();
    },
  );

  it("invalidation cancels pending work and never sends a tombstone", async () => {
    const f = fixture();
    const intent = f.intent();
    f.publisher.select(target, intent);
    f.publisher.invalidate();
    await f.flush();
    expect(f.write).not.toHaveBeenCalled();
  });

  it("drops an intent invalidated before it is selected", async () => {
    const f = fixture();
    const intent = f.intent();
    f.publisher.invalidate();
    f.publisher.select(target, intent);
    await f.flush();
    expect(f.write).not.toHaveBeenCalled();
  });

  it.each(["resolve", "reject"] as const)(
    "aborts an in-flight PUT and ignores its late %s after switching accounts", async (outcome) => {
      const f = fixture();
      const started = deferred<AbortSignal>();
      const response = deferred<DesktopContinuityRevision>();
      f.write.mockImplementationOnce(async (captured, _target, _revision, signal) => {
        expect(captured).toEqual(account);
        started.resolve(signal);
        return response.promise;
      });
      f.publisher.select(target, f.intent());
      const finished = f.flush();
      const signal = await started.promise;
      const nextAccount = { accountOrigin: "https://other.test", accountId: "user-b", sessionToken: "token-b" };
      f.setAccount(nextAccount);
      f.publisher.invalidate();
      expect(signal.aborted).toBe(true);
      if (outcome === "resolve") response.resolve({ revision: 999 });
      else response.reject({ code: "CONTINUITY_CONFLICT" });
      await finished;
      expect(f.onError).not.toHaveBeenCalled();
      expect(f.write).toHaveBeenCalledTimes(1);
      f.read.mockResolvedValueOnce({ revision: 3 });
      f.publisher.select({ ...target, sessionId: "fresh-session" }, f.publisher.begin(nextAccount));
      await f.flush();
      expect(f.write).toHaveBeenLastCalledWith(nextAccount,
        { ...target, sessionId: "fresh-session" }, 3, expect.any(AbortSignal));
    },
  );

  it("snapshots credentials instead of retaining a mutable account object", async () => {
    const f = fixture();
    const mutable = { ...account };
    f.setAccount(mutable);
    const intent = f.intent();
    mutable.sessionToken = "different-token";
    mutable.accountOrigin = "https://different.test";
    f.publisher.select(target, intent);
    await f.flush();
    expect(f.read.mock.calls[0]?.[0]).toEqual({
      accountOrigin: account.accountOrigin, sessionToken: account.sessionToken,
    });
    expect(f.write).not.toHaveBeenCalled();
  });
});
