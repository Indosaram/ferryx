import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, renderHook } from "@testing-library/react";

const tauri = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauri.listen }));

import { buildSidebarModel, type SidebarModel } from "../components/sidebar/sidebarModel";
import type { RemoteHostState } from "../state/remoteHostStore";
import { createDesktopSidebarSync, useDesktopSidebarPublisher, type DesktopSidebarInput } from "./desktopSidebarSync";
import type { RegisteredProject } from "./tauri";

const remoteState: RemoteHostState = {
  hosts: {}, activeHostId: null, discovering: false, nativeStatus: "ready", machineFeaturesEnabled: true,
};

function model(projects: RegisteredProject[], generatedAt = "2026-10-24T00:00:00.000Z"): SidebarModel {
  return buildSidebarModel({
    source: "desktop", generatedAt, projects, activeProjectId: projects[0]?.workspaceId, worktrees: [],
    hosts: [], remoteState, reachability: () => ({ supported: false, entry: null }),
    worktreeOrder: {}, collapsedGroupIds: new Set(), advancedFeatures: false, listingFailures: {},
  });
}

const project = (id: string): RegisteredProject => ({ workspaceId: id, repoRoot: `/repo/${id}`, gitRoot: `/repo/${id}` });
const sentGroupIds = () => tauri.invoke.mock.calls.map(([command, args]) => {
  expect(command).toBe("cmd_sync_desktop_sidebar");
  return (args as { model: SidebarModel }).model.projects.map((node) => node.groupId);
});

function deferredInvoke() {
  const started = Promise.withResolvers<void>();
  const response = Promise.withResolvers<number>();
  tauri.invoke.mockImplementationOnce(() => {
    started.resolve();
    return response.promise;
  });
  return { started: started.promise, ...response };
}

beforeEach(() => {
  tauri.invoke.mockReset();
  tauri.invoke.mockResolvedValue(1);
  tauri.listen.mockReset();
  tauri.listen.mockResolvedValue(() => undefined);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("createDesktopSidebarSync", () => {
  it("sends a model once when only its timestamp changes", async () => {
    const send = createDesktopSidebarSync();

    await send(model([project("a")], "2026-10-24T00:00:00.000Z"));
    await send(model([project("a")], "2026-10-24T00:00:05.000Z"));

    expect(sentGroupIds()).toEqual([["a"]]);
  });

  it("sends every distinct model in order", async () => {
    const send = createDesktopSidebarSync();

    await send(model([project("a")]));
    await send(model([project("a"), project("b")]));

    expect(sentGroupIds()).toEqual([["a"], ["a", "b"]]);
  });

  it("keeps one call in flight and collapses queued models to the latest", async () => {
    const response = deferredInvoke();
    const send = createDesktopSidebarSync();

    const first = send(model([project("first")]));
    await response.started;
    void send(model([project("obsolete")]));
    const last = send(model([project("latest")]));
    expect(tauri.invoke).toHaveBeenCalledTimes(1);
    response.resolve(1);
    await Promise.all([first, last]);

    expect(sentGroupIds()).toEqual([["first"], ["latest"]]);
  });

  it("resends an unchanged model when asked to after a reconnect", async () => {
    const send = createDesktopSidebarSync();

    await send(model([project("a")]));
    await send(model([project("a")]), true);

    expect(sentGroupIds()).toEqual([["a"], ["a"]]);
  });

  it("resends an unchanged model when requested during an in-flight publish", async () => {
    const response = deferredInvoke();
    const send = createDesktopSidebarSync();
    const snapshot = model([project("a")]);
    const first = send(snapshot);
    await response.started;

    const resend = send(snapshot, true);
    expect(tauri.invoke).toHaveBeenCalledTimes(1);
    response.resolve(1);
    await Promise.all([first, resend]);

    expect(sentGroupIds()).toEqual([["a"], ["a"]]);
  });

  it("resends the latest model when it changes during an in-flight publish", async () => {
    const response = deferredInvoke();
    const send = createDesktopSidebarSync();
    const first = send(model([project("a")]));
    await response.started;

    void send(model([project("obsolete")]), true);
    const latest = send(model([project("b")]));
    expect(tauri.invoke).toHaveBeenCalledTimes(1);
    response.resolve(1);
    await Promise.all([first, latest]);

    expect(sentGroupIds()).toEqual([["a"], ["b"]]);
  });

  it("sends a pending resend when the in-flight publish rejects", async () => {
    const response = deferredInvoke();
    const send = createDesktopSidebarSync();
    const snapshot = model([project("a")]);
    const first = send(snapshot);
    await response.started;

    const resend = send(snapshot, true);
    expect(tauri.invoke).toHaveBeenCalledTimes(1);
    response.reject({ code: "STALE_EPOCH", message: "stale sidebar lifetime" });
    await Promise.all([first, resend]);

    expect(sentGroupIds()).toEqual([["a"], ["a"]]);
  });

  it("deduplicates an unchanged non-forced model after a deferred publish succeeds", async () => {
    const response = deferredInvoke();
    const send = createDesktopSidebarSync();
    const snapshot = model([project("a")]);
    const first = send(snapshot);
    await response.started;
    response.resolve(1);
    await first;

    await send(snapshot);

    expect(sentGroupIds()).toEqual([["a"]]);
  });

  it.each(["INTERNAL_ERROR", "STALE_EPOCH"])("retries a model rejected with %s instead of treating it as sent", async (code) => {
    vi.useFakeTimers();
    tauri.invoke.mockRejectedValueOnce({ code, message: "publish rejected" });
    const send = createDesktopSidebarSync();

    await expect(send(model([project("a")]))).rejects.toMatchObject({ code });
    await send(model([project("a")]));

    expect(sentGroupIds()).toEqual([["a"], ["a"]]);
  });

  it.each([false, true])("drains after idle dedupe with final forced=%s", async (forced) => {
    const send = createDesktopSidebarSync();
    const snapshot = model([project("a")]);
    await send(snapshot);
    await send(snapshot);
    await send(forced ? snapshot : model([project("b")]), forced);
    expect(sentGroupIds()).toEqual([["a"], [forced ? "a" : "b"]]);
  });

  it.each(["MACHINE_SERVICE_UNAVAILABLE", "STALE_EPOCH"])("retries failed forced publication autonomously for %s", async (code) => {
    vi.useFakeTimers();
    const send = createDesktopSidebarSync();
    const snapshot = model([project("a")]);
    await send(snapshot);
    tauri.invoke.mockRejectedValueOnce({ code, message: "transient" });
    await expect(send(snapshot, true)).rejects.toMatchObject({ code });
    await vi.advanceTimersByTimeAsync(999);
    expect(tauri.invoke).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(sentGroupIds()).toEqual([["a"], ["a"], ["a"]]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it.each(["INVALID_REQUEST", "PAYLOAD_TOO_LARGE", "CAPACITY_EXCEEDED"])("does not retry permanent %s", async (code) => {
    vi.useFakeTimers();
    tauri.invoke.mockRejectedValueOnce({ code, message: "STALE_EPOCH is only prose here" });
    const send = createDesktopSidebarSync();
    await expect(send(model([project("a")]), true)).rejects.toMatchObject({ code });
    await vi.advanceTimersByTimeAsync(60_000);
    expect(tauri.invoke).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("supersedes a retry with the newest model", async () => {
    vi.useFakeTimers();
    tauri.invoke.mockRejectedValueOnce({ code: "INTERNAL_ERROR", message: "transient" });
    const send = createDesktopSidebarSync();
    await expect(send(model([project("a")]))).rejects.toMatchObject({ code: "INTERNAL_ERROR" });
    await send(model([project("b")]));
    await vi.advanceTimersByTimeAsync(60_000);
    expect(sentGroupIds()).toEqual([["a"], ["b"]]);
  });

  it("caps retry backoff and cancels outstanding obligations", async () => {
    vi.useFakeTimers();
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    tauri.invoke.mockRejectedValue({ code: "INTERNAL_ERROR", message: "transient" });
    const send = createDesktopSidebarSync();
    await expect(send(model([project("a")]))).rejects.toMatchObject({ code: "INTERNAL_ERROR" });
    for (const delay of [1000, 2000, 4000, 8000, 16000, 30000, 30000]) {
      const count = tauri.invoke.mock.calls.length;
      await vi.advanceTimersByTimeAsync(delay - 1);
      expect(tauri.invoke).toHaveBeenCalledTimes(count);
      await vi.advanceTimersByTimeAsync(1);
      expect(tauri.invoke).toHaveBeenCalledTimes(count + 1);
    }
    send.cancel();
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("useDesktopSidebarPublisher", () => {
  const input: DesktopSidebarInput = {
    enabled: true, projects: [project("hook")], activeProjectId: "hook", worktrees: [],
    inactiveProjectWorktrees: {}, emptyWorkspaceIds: [], activityByWorktreePath: {},
    unreadWorktreePaths: {}, agents: [], attentionRows: [],
  };

  it.each([null, "STALE_EPOCH"])("forces publication after a missed resync and initial result %s", async (code) => {
    const attached = Promise.withResolvers<() => void>();
    tauri.listen.mockReturnValue(attached.promise);
    if (code) tauri.invoke.mockRejectedValueOnce({ code, message: "old epoch" });
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    await act(async () => { renderHook(() => useDesktopSidebarPublisher(input)); });
    expect(tauri.invoke).toHaveBeenCalledTimes(1);
    await act(async () => { attached.resolve(() => undefined); await attached.promise; });
    expect(sentGroupIds()).toEqual([["hook"], ["hook"]]);
  });

  it("forces unchanged publication on re-enable before listener readiness", async () => {
    const { rerender } = renderHook(({ enabled }) => useDesktopSidebarPublisher({ ...input, enabled }), {
      initialProps: { enabled: true },
    });
    await act(async () => undefined);
    const count = tauri.invoke.mock.calls.length;
    rerender({ enabled: false });
    const attached = Promise.withResolvers<() => void>();
    tauri.listen.mockReturnValue(attached.promise);
    await act(async () => { rerender({ enabled: true }); });
    expect(tauri.invoke).toHaveBeenCalledTimes(count + 1);
    await act(async () => { attached.resolve(() => undefined); await attached.promise; });
  });
});
