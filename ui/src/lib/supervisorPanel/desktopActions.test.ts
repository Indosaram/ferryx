import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ answer: vi.fn(), enable: vi.fn(), start: vi.fn() }));

vi.mock("../tauri", () => ({
  converseSupervisor: vi.fn(),
  enableAgentControlOwner: mocks.enable,
  isStructuredIpcError: () => false,
  setSupervisorPanelPaused: vi.fn(),
  startSupervisor: mocks.start,
  stopSupervisedSession: vi.fn(),
  supervisorAnswer: mocks.answer,
  undoSupervisorAction: vi.fn(),
}));

vi.mock("../supervisor", () => ({ setSupervisorLanePaused: vi.fn() }));

import { desktopPanelActions } from "./desktopActions";
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
});
