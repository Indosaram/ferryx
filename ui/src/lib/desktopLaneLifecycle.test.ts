import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { installDesktopLaneLifecycleTransport } from "./desktopLaneLifecycle";
import { laneLifecycleTransportOrDefault, setLaneLifecycleTransport } from "./sidebarPersistence";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(() => { setLaneLifecycleTransport(null); vi.clearAllMocks(); vi.unstubAllGlobals(); });

describe("desktop lifecycle channel", () => {
  it("installs IPC before use and unwraps the hub apply envelope", async () => {
    const row = { worktreeKey: "p:ferryx/code/ferryx", effectiveState: "pinned" };
    vi.mocked(invoke).mockResolvedValue({ status: 200, value: { row, stale: false } });
    installDesktopLaneLifecycleTransport();
    const body = { worktreeKey: row.worktreeKey, workspaceId: "ferryx", path: "/code/ferryx", action: "pin" as const, pinned: true };
    expect(await laneLifecycleTransportOrDefault().write(body)).toEqual(row);
    expect(invoke).toHaveBeenCalledWith("cmd_lane_lifecycle", { operation: "apply", body });
  });

  it("sends listing identities over IPC before the sidebar consumes the model", async () => {
    const model = { sections: [], revision: 0 };
    const identities = [{ worktreeKey: "w:ferryx/managed", workspaceId: "ferryx", path: "/code/managed" }];
    vi.mocked(invoke).mockResolvedValue({ status: 200, value: model });
    installDesktopLaneLifecycleTransport();
    expect(await laneLifecycleTransportOrDefault().reconcileIdentities!(identities)).toEqual(model);
    expect(invoke).toHaveBeenCalledWith("cmd_lane_lifecycle", { operation: "identities", body: identities });
  });

  it("uses the predecessor HTTP transport for every operation when IPC is unsupported", async () => {
    vi.mocked(invoke).mockResolvedValue(null);
    const row = { worktreeKey: "p:ferryx/code/ferryx", effectiveState: "pinned" };
    const model = { sections: [], revision: 0 };
    const fetcher = vi.fn()
      .mockResolvedValueOnce({ ok: true, json: async () => model })
      .mockResolvedValueOnce({ ok: true, json: async () => ({ row }) })
      .mockResolvedValueOnce({ ok: true, json: async () => row })
      .mockResolvedValueOnce({ ok: true, json: async () => ({ imported: true }) })
      .mockResolvedValueOnce({ ok: true, json: async () => ({ sections: [] }) })
      .mockResolvedValueOnce({ ok: true, json: async () => model });
    vi.stubGlobal("fetch", fetcher);
    installDesktopLaneLifecycleTransport();
    const transport = laneLifecycleTransportOrDefault();
    const body = { worktreeKey: row.worktreeKey, workspaceId: "ferryx", path: "/code/ferryx", action: "pin" as const, pinned: true };
    expect(await transport.read()).toEqual(model);
    expect(await transport.write(body)).toEqual(row);
    expect(await transport.writePinTabs({ ...body, pinnedTabs: ["tab-1"] })).toEqual(row);
    expect(await transport.importOrder({ ferryx: [body.path] })).toBe(true);
    await transport.reorder("ferryx", [body.path]);
    expect(await transport.reconcileIdentities!([{ worktreeKey: body.worktreeKey, workspaceId: "ferryx", path: body.path }])).toEqual(model);
    expect(fetcher.mock.calls.map(([url]) => url)).toEqual([
      "/api/lanes/lifecycle", "/api/lanes/lifecycle/apply", "/api/lanes/lifecycle/pin-tabs",
      "/api/lanes/lifecycle/import", "/api/lanes/lifecycle/order", "/api/lanes/lifecycle/identities",
    ]);
    expect(JSON.parse(fetcher.mock.calls[1][1].body)).toEqual(body);
  });

  it("does not retry a typed standby refusal through HTTP", async () => {
    const fetcher = vi.fn();
    vi.stubGlobal("fetch", fetcher);
    const refusal = { code: "SUPERVISOR_STANDBY", message: "standby", details: { holderMachineId: "mach-b" } };
    vi.mocked(invoke).mockRejectedValue(refusal);
    installDesktopLaneLifecycleTransport();
    await expect(laneLifecycleTransportOrDefault().read()).rejects.toEqual(refusal);
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("rejects a hub conflict rather than treating it as a saved row", async () => {
    vi.mocked(invoke).mockResolvedValue({ status: 409, value: { stale: true, error: "the row moved on" } });
    installDesktopLaneLifecycleTransport();
    await expect(laneLifecycleTransportOrDefault().read()).rejects.toThrow("the row moved on");
  });
});
