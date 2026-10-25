import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: tauri.invoke }));

import { buildSidebarModel, type SidebarModel } from "../components/sidebar/sidebarModel";
import type { RemoteHostState } from "../state/remoteHostStore";
import { createDesktopSidebarSync } from "./desktopSidebarSync";
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

beforeEach(() => {
  tauri.invoke.mockReset();
  tauri.invoke.mockResolvedValue(1);
});

afterEach(() => {
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
    let release: (revision: number) => void = () => undefined;
    tauri.invoke.mockImplementationOnce(() => new Promise((resolve) => { release = resolve; }));
    const send = createDesktopSidebarSync();

    const first = send(model([project("first")]));
    void send(model([project("obsolete")]));
    const last = send(model([project("latest")]));
    expect(tauri.invoke).toHaveBeenCalledTimes(1);
    release(1);
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
    let release: (revision: number) => void = () => undefined;
    tauri.invoke.mockImplementationOnce(() => new Promise<number>((resolve) => { release = resolve; }));
    const send = createDesktopSidebarSync();
    const snapshot = model([project("a")]);
    const first = send(snapshot);

    const resend = send(snapshot, true);
    expect(tauri.invoke).toHaveBeenCalledTimes(1);
    release(1);
    await Promise.all([first, resend]);

    expect(sentGroupIds()).toEqual([["a"], ["a"]]);
  });

  it("resends the latest model when it changes during an in-flight publish", async () => {
    let release: (revision: number) => void = () => undefined;
    tauri.invoke.mockImplementationOnce(() => new Promise<number>((resolve) => { release = resolve; }));
    const send = createDesktopSidebarSync();
    const first = send(model([project("a")]));

    void send(model([project("obsolete")]), true);
    const latest = send(model([project("b")]));
    expect(tauri.invoke).toHaveBeenCalledTimes(1);
    release(1);
    await Promise.all([first, latest]);

    expect(sentGroupIds()).toEqual([["a"], ["b"]]);
  });

  it("sends a pending resend when the in-flight publish rejects", async () => {
    let reject: (reason: unknown) => void = () => undefined;
    tauri.invoke.mockImplementationOnce(() => new Promise<number>((_resolve, fail) => { reject = fail; }));
    const send = createDesktopSidebarSync();
    const snapshot = model([project("a")]);
    const first = send(snapshot);

    const resend = send(snapshot, true);
    expect(tauri.invoke).toHaveBeenCalledTimes(1);
    reject({ code: "STALE_EPOCH", message: "stale sidebar lifetime" });
    await Promise.all([first, resend]);

    expect(sentGroupIds()).toEqual([["a"], ["a"]]);
  });

  it("deduplicates an unchanged non-forced model after a deferred publish succeeds", async () => {
    let release: (revision: number) => void = () => undefined;
    tauri.invoke.mockImplementationOnce(() => new Promise<number>((resolve) => { release = resolve; }));
    const send = createDesktopSidebarSync();
    const snapshot = model([project("a")]);
    const first = send(snapshot);
    release(1);
    await first;

    await send(snapshot);

    expect(sentGroupIds()).toEqual([["a"]]);
  });

  it.each(["INTERNAL_ERROR", "STALE_EPOCH"])("retries a model rejected with %s instead of treating it as sent", async (code) => {
    tauri.invoke.mockRejectedValueOnce({ code, message: "publish rejected" });
    const send = createDesktopSidebarSync();

    await expect(send(model([project("a")]))).rejects.toMatchObject({ code });
    await send(model([project("a")]));

    expect(sentGroupIds()).toEqual([["a"], ["a"]]);
  });
});
