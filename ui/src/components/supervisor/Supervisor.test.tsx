import { act, cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { desktopUnifiedClient, setSupervisorSnapshotForTest, useSupervisor } from "../../lib/supervisor";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => false, invoke: ipc.invoke }));

const lane = { key: "ledger", title: "Ledger", status: "working", sessionId: "s1", cwd: "/repo/main", goal: "Get tests green", paused: false, lastActivityAt: "" };

afterEach(cleanup);
beforeEach(() => {
  ipc.invoke.mockReset();
  ipc.invoke.mockResolvedValue(undefined);
});

describe("useSupervisor", () => {
  it("settles after a snapshot instead of re-rendering itself", () => {
    let renders = 0;
    function Probe() {
      renders += 1;
      useSupervisor();
      return null;
    }
    render(<Probe />);
    act(() => {
      setSupervisorSnapshotForTest({
        hub: { available: true, running: true },
        lanes: [lane],
        watched: [],
        actions: [{ deviceId: "owner", op: "send", sessionId: "s1", requestId: "r", code: "OK", atMs: Date.now(), text: "x" }],
      });
    });
    expect(renders).toBeLessThanOrEqual(4);
  });
});

describe("desktopUnifiedClient", () => {
  it("sends envelopes and reads through the typed IPC commands and keeps refusal codes", async () => {
    ipc.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_supervisor_request") throw { code: "REVISION_CONFLICT", message: "goal g changed", details: { code: "REVISION_CONFLICT", status: 409 } };
      if (command === "cmd_supervisor_read") return { projectId: "p", messages: [], next: 0 };
      return undefined;
    });
    const envelope = { requestId: "r", goalId: "g", expectedRevision: 1, action: "pause", payload: {} };
    await expect(desktopUnifiedClient.request(envelope)).resolves.toEqual({ kind: "error", code: "REVISION_CONFLICT", message: "goal g changed", status: 409 });
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_supervisor_request", { envelope });
    await expect(desktopUnifiedClient.conversation("p", 2)).resolves.toMatchObject({ kind: "ok" });
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_supervisor_read", { request: { view: "conversation", projectId: "p", after: 2 } });
    await desktopUnifiedClient.memory();
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_supervisor_read", { request: { view: "memory" } });
  });
});
