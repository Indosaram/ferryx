import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { panelSnapshotFixture } from "../../lib/supervisorPanel/fixtures";
import { createSupervisorFrameBus } from "../home/machineSupervisorSources";
import type { SupervisorFetch, SupervisorHttpResponse } from "../remoteSupervisor";
import { usePhonePanel } from "./usePhonePanel";

afterEach(() => cleanup());

// Deleting this test permits the new sibling frame to invalidate an unrelated initial fleet
// snapshot request, leaving the phone stuck loading even though the sidebar consumed the frame.
it("does not cancel the initial fleet-panel read when a lifecycle frame arrives", async () => {
  const bus = createSupervisorFrameBus();
  let answer!: (response: SupervisorHttpResponse) => void;
  const panel = new Promise<SupervisorHttpResponse>((resolve) => { answer = resolve; });
  const fetchLike = vi.fn<SupervisorFetch>((path) => {
    if (path === "/api/v1/supervisor/panel") return panel;
    if (path === "/api/v1/supervisor/prs") return Promise.resolve({ status: 200, body: JSON.stringify({ prs: [], outage: null, refreshedAtMs: 1 }) });
    if (path.startsWith("/api/v1/supervisor/transcript")) return Promise.resolve({ status: 200, body: JSON.stringify({ messages: [], nextCursor: 0, hasMore: false }) });
    throw new Error(`Unexpected route ${path}`);
  });
  const fetchFor = () => fetchLike;
  const { result } = renderHook(() => usePhonePanel({ hostId: "owner", fetchFor, bus, enabled: true }));
  act(() => bus.emit("owner", { type: "panel.lanes", model: { sections: [], revision: 0 } }));
  await act(async () => { answer({ status: 200, body: JSON.stringify(panelSnapshotFixture) }); await panel; });
  expect(result.current.state.kind).toBe("ready");
  expect(fetchLike.mock.calls.filter(([path]) => path === "/api/v1/supervisor/panel")).toHaveLength(1);
});
