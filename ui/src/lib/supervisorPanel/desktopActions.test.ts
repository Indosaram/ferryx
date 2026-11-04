import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ answer: vi.fn(), enable: vi.fn(), start: vi.fn() }));

vi.mock("../tauri", () => ({
  converseSupervisor: vi.fn(),
  enableAgentControlOwner: mocks.enable,
  isStructuredIpcError: (error: unknown) =>
    !!error && typeof error === "object" && typeof (error as { code?: unknown }).code === "string",
  setSupervisorPanelPaused: vi.fn(),
  startSupervisor: mocks.start,
  stopSupervisedSession: vi.fn(),
  supervisorAnswer: mocks.answer,
  undoSupervisorAction: vi.fn(),
}));

vi.mock("../supervisor", () => ({ setSupervisorLanePaused: vi.fn() }));

import { AGENT_CONTROL_NOT_ENABLED_NOTICE, desktopPanelActions } from "./desktopActions";
import type { PanelDecisionTarget } from "./actions";

function actions() {
  return desktopPanelActions({ openSession: vi.fn(), onChanged: vi.fn() });
}

describe("desktop supervisor panel IPC", () => {
  beforeEach(() => vi.clearAllMocks());

  it("answers through the host-qualified supervisor route", async () => {
    mocks.answer.mockResolvedValue({ status: "sent" });
    const decision: PanelDecisionTarget = { sessionRef: "omarchy:s-1", promptId: "p-1", screenRevision: "r-7" };
    await expect(actions().answer(decision, 2)).resolves.toEqual({ ok: true });
    expect(mocks.answer).toHaveBeenCalledWith({
      sessionRef: "omarchy:s-1",
      promptId: "p-1",
      screenRevision: "r-7",
      optionIndex: 2,
    });
  });

  it("enables only the owner before starting the hub", async () => {
    mocks.enable.mockResolvedValue({ enabled: true, excludedDeviceIds: [], audit: [] });
    mocks.start.mockResolvedValue(true);
    await expect(actions().turnOn()).resolves.toEqual({ ok: true });
    expect(mocks.enable).toHaveBeenCalledOnce();
    expect(mocks.start).toHaveBeenCalledOnce();
    expect(mocks.enable.mock.invocationCallOrder[0]).toBeLessThan(mocks.start.mock.invocationCallOrder[0]);
  });

  it("still starts the hub when this computer refuses agent control to a token-only peer", async () => {
    const forbidden = { code: "FORBIDDEN", message: "this connection proved only the transport token", details: {} };
    mocks.enable.mockRejectedValue(forbidden);
    mocks.start.mockResolvedValue(true);
    const onChanged = vi.fn();
    const result = await desktopPanelActions({ openSession: vi.fn(), onChanged }).turnOn();
    expect(result).toEqual({ ok: true, notice: AGENT_CONTROL_NOT_ENABLED_NOTICE });
    expect(mocks.start).toHaveBeenCalledOnce();
    expect(onChanged).toHaveBeenCalledOnce();
  });

  it("stops at any other agent control failure without starting the hub", async () => {
    mocks.enable.mockRejectedValue({ code: "IO_ERROR", message: "disk full", details: {} });
    await expect(actions().turnOn()).resolves.toEqual({ ok: false, stale: false, message: "disk full" });
    expect(mocks.start).not.toHaveBeenCalled();
  });

  it("reports a refused hub start even after agent control was refused", async () => {
    mocks.enable.mockRejectedValue({ code: "FORBIDDEN", message: "token only", details: {} });
    mocks.start.mockRejectedValue({ code: "SUPERVISOR_UNAVAILABLE", message: "the supervisor hub is not open", details: {} });
    await expect(actions().turnOn()).resolves.toEqual({
      ok: false,
      stale: false,
      message: "the supervisor hub is not open",
    });
  });
});
