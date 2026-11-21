import { describe, expect, it, vi } from "vitest";
import { createPhoneContinuityPublisher } from "./phoneContinuityPublication";
import type { AccountContinuityPointer } from "./accountSession";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

const target: Pick<AccountContinuityPointer, "machineId" | "workspaceId" | "worktreeSlug" | "sessionId"> = {
  machineId: "machine", workspaceId: "workspace", worktreeSlug: null, sessionId: "phone",
};

function fixture() {
  let revision = 7;
  const publisher = createPhoneContinuityPublisher();
  let selected: string | null = null;
  let account = { origin: "https://relay.test", token: "token-a", id: "account-a" };
  const read = vi.fn(async () => ({ revision }));
  const write = vi.fn(async (_target: typeof target, expectedRevision: number) => {
    if (expectedRevision !== revision) throw { status: 409, details: { current: { revision } } };
    selected = _target.sessionId;
    return { revision: ++revision };
  });
  const begin = () => {
    const selectedAccount = { ...account };
    return publisher.begin({ origin: account.origin, token: account.token, accountId: account.id }, {
      isCurrent: () => account.origin === selectedAccount.origin &&
        account.token === selectedAccount.token && account.id === selectedAccount.id,
      read, write: (selection, revision) => write(selection, revision),
    });
  };
  return { begin, read, write, selected: () => selected, desktopSelect() { revision += 1; selected = "desktop"; },
    changeAccount(change: Partial<typeof account>) { account = { ...account, ...change }; },
    invalidate: publisher.invalidate };
}

describe("explicit phone continuity publication", () => {
  it("reads the desktop's current revision for each fresh phone pick, not the page-load revision", async () => {
    const f = fixture();
    expect(await f.read()).toEqual({ revision: 7 });
    f.desktopSelect();
    const first = f.begin();
    expect(f.read).toHaveBeenCalledTimes(2);
    expect(await first.publish(target)).toBe(true);
    expect(f.write).toHaveBeenLastCalledWith(target, 8);
    f.desktopSelect();
    expect(await f.begin().publish({ ...target, sessionId: "fresh" })).toBe(true);
    expect(f.write).toHaveBeenLastCalledWith({ ...target, sessionId: "fresh" }, 10);
  });

  it("does not rebase an intent when desktop changes during machine discovery or retry its conflict", async () => {
    const f = fixture();
    const intent = f.begin();
    f.desktopSelect();
    await expect(intent.publish(target)).rejects.toMatchObject({ status: 409 });
    expect(await intent.publish(target)).toBe(false);
    expect(f.read).toHaveBeenCalledTimes(1);
    expect(f.write).toHaveBeenCalledTimes(1);
    expect(f.write).toHaveBeenCalledWith(target, 7);
    expect(await f.begin().publish(target)).toBe(true);
    expect(f.write).toHaveBeenLastCalledWith(target, 8);
  });

  it("drops an older read completion after a newer explicit pick", async () => {
    const f = fixture();
    const delayed = deferred<{ revision: number }>();
    f.read.mockReturnValueOnce(delayed.promise);
    const old = f.begin();
    const oldDone = old.publish(target);
    const fresh = { ...target, sessionId: "newer" };
    expect(await f.begin().publish(fresh)).toBe(true);
    delayed.resolve({ revision: 7 });
    expect(await oldDone).toBe(false);
    expect(f.write).toHaveBeenCalledTimes(1);
    expect(f.write).toHaveBeenCalledWith(fresh, 7);
  });

  it.each([{ origin: "https://other.test" }, { token: "token-b" }, { id: "account-b" }])(
    "fences a delayed baseline after credential changes: %j", async (change) => {
      const f = fixture();
      const delayed = deferred<{ revision: number }>();
      f.read.mockReturnValueOnce(delayed.promise);
      const intent = f.begin();
      const done = intent.publish(target);
      f.changeAccount(change);
      delayed.resolve({ revision: 7 });
      expect(await done).toBe(false);
      expect(f.write).not.toHaveBeenCalled();
    },
  );

  it("does not publish automatically or after invalidation", async () => {
    const f = fixture();
    expect(f.read).not.toHaveBeenCalled();
    expect(f.write).not.toHaveBeenCalled();
    const intent = f.begin();
    f.invalidate();
    expect(await intent.publish(target)).toBe(false);
    expect(f.write).not.toHaveBeenCalled();
  });

  it("does not write after a failed baseline read", async () => {
    const f = fixture();
    const failure = new Error("offline");
    f.read.mockRejectedValueOnce(failure);
    await expect(f.begin().publish(target)).rejects.toBe(failure);
    expect(f.write).not.toHaveBeenCalled();
  });

  it.each(["machineId", "workspaceId", "sessionId"] as const)(
    "rejects a missing or empty %s at the publication boundary", async (field) => {
      const f = fixture();
      for (const value of [null, "", " "]) {
        const intent = f.begin();
        expect(await intent.publish({ ...target, [field]: value })).toBe(false);
        expect(await intent.publish(target)).toBe(false);
      }
      expect(f.write).not.toHaveBeenCalled();
    },
  );

  it.each(["none", "before", "after"] as const)("orders local picks without overwriting a desktop write (%s)", async (foreign) => {
    const f = fixture();
    const entered = deferred<void>();
    const release = deferred<void>();
    const commit = f.write.getMockImplementation()!;
    f.write.mockImplementationOnce(async (selection, revision) => {
      entered.resolve();
      await release.promise;
      if (foreign === "before") f.desktopSelect();
      const result = await commit(selection, revision);
      if (foreign === "after") f.desktopSelect();
      return result;
    });
    const first = f.begin();
    const settled = first.publish(target).catch((error: unknown) => {
      if (first.isCurrent()) throw error;
      return false;
    });
    await entered.promise;
    const fresh = { ...target, sessionId: "second-phone-pick" };
    const pending = f.begin().publish(fresh);
    const result = foreign === "none" ? expect(pending).resolves.toBe(true)
      : expect(pending).rejects.toMatchObject({ status: 409 });
    release.resolve();
    await result;
    expect(await settled).toBe(false);
    expect(f.read).toHaveBeenCalledTimes(2);
    expect(f.write).toHaveBeenCalledTimes(2);
    expect(f.write).toHaveBeenLastCalledWith(fresh, foreign === "before" ? 7 : 8);
    expect(f.selected()).toBe(foreign === "none" ? fresh.sessionId : "desktop");
  });
});
