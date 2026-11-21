import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useDesktopContinuity, type CommittedDesktopSelection } from "./useDesktopContinuity";
import {
  createDesktopContinuityPublisher,
  type DesktopContinuityAccount,
  type DesktopContinuityDependencies,
  type DesktopContinuityTarget,
} from "./desktopContinuityPublisher";

const mocks = vi.hoisted(() => ({
  select: vi.fn(), invalidate: vi.fn(), begin: vi.fn(),
  create: null as null | ((dependencies: DesktopContinuityDependencies) =>
    ReturnType<typeof createDesktopContinuityPublisher>),
  actualCreate: null as null | ((dependencies: DesktopContinuityDependencies) =>
    ReturnType<typeof createDesktopContinuityPublisher>),
  token: "token-a" as string | null,
  accountId: "account-a" as string | null,
  dependencies: null as DesktopContinuityDependencies | null,
  read: vi.fn(), write: vi.fn(),
  resolveIdentity: vi.fn(),
  remoteStatus: vi.fn(),
}));
vi.mock("./desktopContinuityPublisher", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./desktopContinuityPublisher")>();
  mocks.actualCreate = actual.createDesktopContinuityPublisher;
  return {
    ...actual,
    createDesktopContinuityPublisher: (dependencies: DesktopContinuityDependencies) => {
      mocks.dependencies = dependencies;
      return mocks.create?.(dependencies) ??
        { select: mocks.select, invalidate: mocks.invalidate, begin: mocks.begin };
    },
  };
});
vi.mock("./tauri", () => ({ getRemoteStatus: mocks.remoteStatus }));
vi.mock("../remote/accountSession", () => ({
  ACCOUNT_SESSION_CHANGED_EVENT: "account-session-changed",
  getConfiguredAccountOrigin: () => "https://account.test",
  getStoredAccountSessionToken: () => mocks.token,
  getStoredAccountId: () => mocks.accountId,
  resolveStoredAccountId: mocks.resolveIdentity,
  getAccountContinuity: mocks.read, putAccountContinuity: mocks.write,
}));

const selection: CommittedDesktopSelection = {
  workspaceId: "workspace", tabId: "tab", leafId: "leaf", worktreePath: "/repo",
  worktreeSlug: null, sessionId: "backend-session", local: true,
};

beforeEach(() => {
  vi.clearAllMocks();
  mocks.create = null;
  mocks.begin.mockReturnValue({ captured: "intent" });
  mocks.token = "token-a";
  mocks.accountId = "account-a";
  mocks.resolveIdentity.mockReset();
  mocks.resolveIdentity.mockResolvedValue(null);
  mocks.remoteStatus.mockReset();
  mocks.remoteStatus.mockResolvedValue({ machineId: "local-machine" });
});

async function setup(onError = vi.fn()) {
  const hook = renderHook(({ selected = selection, leader = true, enabled = true }: {
    selected?: CommittedDesktopSelection; leader?: boolean; enabled?: boolean;
  }) => useDesktopContinuity({
    enabled, leader, selection: selected, onError,
  }), { initialProps: { selected: selection, leader: true, enabled: true } });
  await act(async () => {});
  return hook;
}

/** Installs the real publisher so CAS baselines and generations are the shipped ones, with the
 * debounce made explicit and the pointer revision simulating another device. */
function installRealPublisher() {
  let revision = 7;
  const scheduled: Array<() => Promise<void>> = [];
  const read = vi.fn(async (_credentials: unknown) => ({ revision }));
  const write = vi.fn(async (_account: DesktopContinuityAccount, _target: DesktopContinuityTarget,
    expectedRevision: number) => {
    if (expectedRevision !== revision) throw { code: "CONTINUITY_CONFLICT", details: { current: { revision } } };
    revision += 1;
    return { revision };
  });
  mocks.create = (dependencies) => {
    const actual = mocks.actualCreate;
    if (!actual) throw new Error("expected the real publisher factory");
    return actual({
      ...dependencies,
      read: (credentials, signal) => { void signal; return read(credentials); },
      write: (account, target, expectedRevision, signal) => {
        void signal;
        return write(account, target, expectedRevision);
      },
      schedule(callback) {
        scheduled.push(callback);
        return () => {
          const index = scheduled.indexOf(callback);
          if (index >= 0) scheduled.splice(index, 1);
        };
      },
    });
  };
  return {
    read, write,
    publishFromPhone() { revision += 1; },
    revision: () => revision,
    async flush() {
      while (scheduled.length > 0) await scheduled.shift()!();
    },
  };
}

describe("desktop explicit selection boundary", () => {
  it("forwards captured origin, bearer, and the exact abort signal to both HTTP helpers", async () => {
    await setup();
    const captured = { accountOrigin: "https://original.test", accountId: "original-account", sessionToken: "original-token" };
    const controller = new AbortController();
    const target = { machineId: "machine", workspaceId: "workspace", worktreeSlug: null, sessionId: "backend" };
    mocks.token = "replacement-token";
    mocks.accountId = "replacement-account";
    await mocks.dependencies!.read(captured, controller.signal);
    await mocks.dependencies!.write(captured, target, 7, controller.signal);
    expect(mocks.read).toHaveBeenCalledWith(captured.accountOrigin, captured.sessionToken, controller.signal);
    expect(mocks.write).toHaveBeenCalledWith(captured.accountOrigin, captured.sessionToken, 7, target, controller.signal);
  });

  it("does not publish mount or unrelated rerenders", async () => {
    const hook = await setup();
    hook.rerender({ selected: { ...selection }, leader: true });
    expect(mocks.select).not.toHaveBeenCalled();
    expect(mocks.remoteStatus).not.toHaveBeenCalled();
  });

  it.each(["failure", "missing"])("retries machine identity only on fresh explicit intent after %s", async (outcome) => {
    const onError = vi.fn();
    const failure = new Error("status unavailable");
    if (outcome === "failure") mocks.remoteStatus.mockRejectedValueOnce(failure);
    else mocks.remoteStatus.mockResolvedValueOnce({ machineId: null });
    const hook = await setup(onError);
    await act(async () => hook.result.current({ workspaceId: "workspace" }));
    expect(mocks.select).not.toHaveBeenCalled();
    expect(onError.mock.calls).toEqual(outcome === "failure" ? [[failure]] : []);
    hook.rerender({ selected: { ...selection } });
    expect(mocks.remoteStatus).toHaveBeenCalledTimes(1);
    await act(async () => hook.result.current({ workspaceId: "workspace" }));
    expect(mocks.remoteStatus).toHaveBeenCalledTimes(2);
    expect(mocks.select).toHaveBeenCalledExactlyOnceWith({ machineId: "local-machine",
      workspaceId: "workspace", worktreeSlug: null, sessionId: "backend-session" }, expect.anything());
  });

  it.each(["supersede", "credentials", "identity", "leadership", "disable", "unmount"].flatMap(
    (boundary) => ["success", "failure"].map((outcome) => ({ boundary, outcome })),
  ))(
    "ignores stale machine lookup $outcome after $boundary", async ({ boundary, outcome }) => {
      const onError = vi.fn();
      const hook = await setup(onError);
      let resolve!: (value: { machineId: string }) => void;
      let reject!: (error: unknown) => void;
      const stale = new Promise<{ machineId: string }>((yes, no) => { resolve = yes; reject = no; });
      mocks.remoteStatus.mockReturnValueOnce(stale);
      act(() => hook.result.current({ workspaceId: "workspace", tabId: "next" }));
      if (boundary === "supersede") {
        await act(async () => hook.result.current({ workspaceId: "workspace", tabId: "tab" }));
      } else if (boundary === "credentials" || boundary === "identity") {
        act(() => {
          if (boundary === "credentials") mocks.token = "token-b";
          else mocks.accountId = "account-b";
          window.dispatchEvent(new Event("account-session-changed"));
        });
      } else if (boundary === "leadership") hook.rerender({ leader: false });
      else if (boundary === "disable") hook.rerender({ enabled: false });
      else hook.unmount();
      await act(async () => {
        if (outcome === "success") resolve({ machineId: "stale-machine" });
        else reject(new Error("stale status failure"));
        await stale.catch(() => undefined);
      });
      expect(mocks.select).toHaveBeenCalledTimes(boundary === "supersede" ? 1 : 0);
      expect(mocks.select.mock.calls.some(([target]) => target.machineId === "stale-machine")).toBe(false);
      expect(onError).not.toHaveBeenCalled();
    },
  );

  it("publishes only after the requested pane is committed", async () => {
    const hook = await setup();
    await act(async () => hook.result.current({ workspaceId: "workspace", tabId: "next", leafId: "next-leaf" }));
    expect(mocks.select).not.toHaveBeenCalled();
    hook.rerender({ selected: { ...selection, tabId: "next", leafId: "next-leaf", sessionId: "next-backend" }, leader: true });
    expect(mocks.select).toHaveBeenCalledExactlyOnceWith({ machineId: "local-machine",
      workspaceId: "workspace", worktreeSlug: null, sessionId: "next-backend" }, expect.anything());
  });

  it("accepts a fresh explicit reselection of the same pane", async () => {
    const hook = await setup();
    await act(async () => hook.result.current({ workspaceId: "workspace", tabId: "tab" }));
    expect(mocks.select).toHaveBeenCalledTimes(1);
  });

  it("invalidates pending intent on account changes without clearing the pointer", async () => {
    const hook = await setup();
    await act(async () => hook.result.current({ workspaceId: "workspace", tabId: "next" }));
    mocks.token = "different-token";
    act(() => window.dispatchEvent(new Event("account-session-changed")));
    hook.rerender({ selected: { ...selection, tabId: "next" }, leader: true });
    expect(mocks.select).not.toHaveBeenCalled();
    expect(mocks.invalidate).toHaveBeenCalled();
  });

  it("resolves an existing sign-in and preserves explicit intent across its identity event", async () => {
    const hook = await setup();
    mocks.accountId = null;
    let resolve!: (value: string | null) => void;
    const identity = new Promise<string | null>((done) => { resolve = done; });
    mocks.resolveIdentity.mockReturnValueOnce(identity);
    await act(async () => hook.result.current({ workspaceId: "workspace", tabId: "next" }));
    expect(mocks.resolveIdentity).toHaveBeenCalledWith("https://account.test", "token-a", expect.any(AbortSignal));
    expect(mocks.select).not.toHaveBeenCalled();
    await act(async () => {
      mocks.accountId = "resolved-account";
      window.dispatchEvent(new Event("account-session-changed"));
      resolve("resolved-account");
      await identity;
    });
    expect(mocks.select).not.toHaveBeenCalled();
    hook.rerender({ selected: { ...selection, tabId: "next" }, leader: true });
    expect(mocks.select).toHaveBeenCalledTimes(1);
  });

  it("resolves account identity on fresh explicit intent after an initial identity failure", async () => {
    const onError = vi.fn();
    const hook = await setup(onError);
    mocks.accountId = null;
    const failure = new Error("identity unavailable");
    mocks.resolveIdentity.mockRejectedValueOnce(failure);
    await act(async () => hook.result.current({ workspaceId: "workspace", tabId: "next" }));
    expect(onError).toHaveBeenCalledExactlyOnceWith(failure);
    expect(mocks.select).not.toHaveBeenCalled();
    hook.rerender({ selected: { ...selection, tabId: "next" } });
    expect(mocks.resolveIdentity).toHaveBeenCalledTimes(1);
    expect(mocks.select).not.toHaveBeenCalled();

    let resolve!: (value: string | null) => void;
    const identity = new Promise<string | null>((done) => { resolve = done; });
    mocks.resolveIdentity.mockReturnValueOnce(identity);
    act(() => hook.result.current({ workspaceId: "workspace", tabId: "tab" }));
    expect(mocks.resolveIdentity).toHaveBeenCalledTimes(2);
    await act(async () => {
      mocks.accountId = "resolved-account";
      window.dispatchEvent(new Event("account-session-changed"));
      resolve("resolved-account");
      await identity;
    });
    expect(mocks.select).not.toHaveBeenCalled();
    hook.rerender({ selected: selection });
    expect(mocks.select).toHaveBeenCalledExactlyOnceWith({ machineId: "local-machine",
      workspaceId: "workspace", worktreeSlug: null, sessionId: "backend-session" }, expect.anything());
    expect(onError).toHaveBeenCalledTimes(1);
  });

  it("aborts identity lookup on credential change and drops its late result", async () => {
    const hook = await setup();
    mocks.accountId = null;
    let resolve!: (value: string | null) => void;
    const identity = new Promise<string | null>((done) => { resolve = done; });
    mocks.resolveIdentity.mockReturnValueOnce(identity);
    await act(async () => hook.result.current({ workspaceId: "workspace", tabId: "next" }));
    const signal = mocks.resolveIdentity.mock.calls[0]?.[2] as AbortSignal;
    act(() => {
      mocks.token = "token-b";
      window.dispatchEvent(new Event("account-session-changed"));
    });
    expect(signal.aborted).toBe(true);
    await act(async () => {
      mocks.accountId = "account-b";
      resolve("account-a");
      await identity;
    });
    hook.rerender({ selected: { ...selection, tabId: "next" }, leader: true });
    expect(mocks.select).not.toHaveBeenCalled();
  });

  it("does not publish signed-out, unidentified-account, nonleader, or nonlocal selections", async () => {
    const hook = await setup();
    mocks.accountId = null;
    await act(async () => hook.result.current({ workspaceId: "workspace" }));
    mocks.accountId = "account-a";
    mocks.token = null;
    await act(async () => hook.result.current({ workspaceId: "workspace" }));
    mocks.token = "token-a";
    hook.rerender({ selected: selection, leader: false });
    await act(async () => hook.result.current({ workspaceId: "workspace" }));
    hook.rerender({ selected: { ...selection, local: false }, leader: true });
    await act(async () => hook.result.current({ workspaceId: "workspace" }));
    expect(mocks.select).not.toHaveBeenCalled();
  });

  it("never begins a publication for a signed-out selection", async () => {
    const hook = await setup();
    expect(mocks.begin).not.toHaveBeenCalled();
    mocks.token = null;
    await act(async () => hook.result.current({ workspaceId: "workspace", tabId: "tab" }));
    expect(mocks.begin).not.toHaveBeenCalled();
    expect(mocks.select).not.toHaveBeenCalled();
    expect(mocks.remoteStatus).not.toHaveBeenCalled();
    hook.rerender({ selected: { ...selection } });
    expect(mocks.select).not.toHaveBeenCalled();
    expect(mocks.begin).not.toHaveBeenCalled();
  });

  it("never publishes the outgoing workspace session while a project switch is still in flight", async () => {
    const stillRenderingOutgoingProject: CommittedDesktopSelection = {
      workspaceId: "project-a", tabId: null, leafId: null, worktreePath: null,
      worktreeSlug: null, sessionId: null, local: false,
    };
    const incoming: CommittedDesktopSelection = {
      ...selection, workspaceId: "project-b", tabId: "tab-b", leafId: "leaf-b",
      worktreePath: "/repo/b", sessionId: "backend-b",
    };
    const hook = await setup();
    hook.rerender({ selected: stillRenderingOutgoingProject });
    await act(async () => hook.result.current({ workspaceId: "project-b" }));
    hook.rerender({ selected: { ...stillRenderingOutgoingProject } });
    expect(mocks.select).not.toHaveBeenCalled();

    hook.rerender({ selected: incoming });
    expect(mocks.select).toHaveBeenCalledExactlyOnceWith({
      machineId: "local-machine", workspaceId: "project-b", worktreeSlug: null, sessionId: "backend-b",
    }, expect.anything());
    expect(mocks.select.mock.calls.some(([target]) => target.sessionId === "backend-session")).toBe(false);
  });

  it("keeps a switch intent pending through an empty project until its first terminal commits", async () => {
    const emptyProject: CommittedDesktopSelection = {
      workspaceId: "project-b", tabId: null, leafId: null, worktreePath: null,
      worktreeSlug: null, sessionId: null, local: false,
    };
    const firstTerminal: CommittedDesktopSelection = {
      ...emptyProject, tabId: "tab-b", leafId: "leaf-b", worktreePath: "/repo/b",
      sessionId: "backend-b", local: true,
    };
    const hook = await setup();
    await act(async () => hook.result.current({ workspaceId: "project-b" }));
    hook.rerender({ selected: emptyProject });
    expect(mocks.select).not.toHaveBeenCalled();

    hook.rerender({ selected: firstTerminal });
    expect(mocks.select).toHaveBeenCalledExactlyOnceWith({
      machineId: "local-machine", workspaceId: "project-b", worktreeSlug: null, sessionId: "backend-b",
    }, expect.anything());
  });

  it("keeps the selection-time baseline so a phone publication made while a project is still empty wins", async () => {
    const pointer = installRealPublisher();
    const onError = vi.fn();
    const emptyProject: CommittedDesktopSelection = {
      workspaceId: "project-b", tabId: null, leafId: null, worktreePath: null,
      worktreeSlug: null, sessionId: null, local: false,
    };
    const firstTerminal: CommittedDesktopSelection = {
      ...emptyProject, tabId: "tab-b", leafId: "leaf-b", worktreePath: "/repo/b",
      sessionId: "backend-b", local: true,
    };
    const hook = await setup(onError);
    await act(async () => hook.result.current({ workspaceId: "project-b" }));
    expect(pointer.read).toHaveBeenCalledTimes(1);
    pointer.publishFromPhone();
    expect(pointer.revision()).toBe(8);

    hook.rerender({ selected: emptyProject });
    expect(pointer.write).not.toHaveBeenCalled();
    hook.rerender({ selected: firstTerminal });
    await act(async () => { await pointer.flush(); });

    expect(pointer.write).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ accountId: "account-a" }),
      expect.objectContaining({ workspaceId: "project-b", sessionId: "backend-b" }), 7);
    expect(pointer.revision()).toBe(8);
    expect(onError).not.toHaveBeenCalled();
    expect(pointer.read).toHaveBeenCalledTimes(1);
  });
});
