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

  it("retries a model whose publish failed instead of treating it as sent", async () => {
    tauri.invoke.mockRejectedValueOnce({ code: "INTERNAL_ERROR", message: "daemon unavailable" });
    const send = createDesktopSidebarSync();

    await expect(send(model([project("a")]))).rejects.toMatchObject({ code: "INTERNAL_ERROR" });
    await send(model([project("a")]));

    expect(sentGroupIds()).toEqual([["a"], ["a"]]);
  });
});
