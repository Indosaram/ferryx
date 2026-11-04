import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { PanelEvent, PanelSnapshot } from "./contract";
import { emptyPanelSnapshot } from "./emptySnapshot";
import { panelSnapshotFixture } from "./fixtures";
import { buildPanelView } from "./model";
import { panelDecisionCount, usePanelDecisionCount } from "./usePanelDecisionCount";

const NOW = Date.UTC(2026, 10, 2, 14, 17, 0);

describe("panelDecisionCount", () => {
  it("counts exactly the decision cards the panel renders", () => {
    const [first] = panelSnapshotFixture.decisions;
    const snapshot: PanelSnapshot = {
      ...panelSnapshotFixture,
      decisions: [
        ...panelSnapshotFixture.decisions,
        // A repeat for the same session is one card; an escalation without a reason is none.
        { ...first, createdAtMs: first.createdAtMs + 1 },
        { ...first, sessionRef: "mac:other", reason: " " },
      ],
    };
    expect(panelDecisionCount(snapshot)).toBe(buildPanelView(snapshot, { nowMs: NOW }).decisions.length);
    expect(panelDecisionCount(snapshot)).toBe(2);
  });

  it("is zero while setup is required", () => {
    expect(panelDecisionCount({ ...panelSnapshotFixture, setupRequired: true })).toBe(0);
    expect(panelDecisionCount(emptyPanelSnapshot())).toBe(0);
  });
});

describe("usePanelDecisionCount", () => {
  it("replays an event that raced the first load", async () => {
    const handlers: Array<(event: PanelEvent) => void> = [];
    let resolveLoad!: (snapshot: PanelSnapshot) => void;
    const load = vi.fn(() => new Promise<PanelSnapshot>((resolve) => { resolveLoad = resolve; }));
    const subscribe = vi.fn(async (handler: (event: PanelEvent) => void) => {
      handlers.push(handler);
      return () => undefined;
    });
    const { result } = renderHook(() => usePanelDecisionCount({ load, subscribe }));
    expect(result.current).toBe(0);
    expect(handlers).toHaveLength(1);

    const [first] = panelSnapshotFixture.decisions;
    act(() => handlers[0]({ type: "panel.decision", sessionRef: first.sessionRef, decision: null }));
    await act(async () => { resolveLoad(panelSnapshotFixture); });
    expect(result.current).toBe(panelSnapshotFixture.decisions.length - 1);

    act(() => handlers[0]({ type: "panel.decision", sessionRef: first.sessionRef, decision: first }));
    expect(result.current).toBe(panelSnapshotFixture.decisions.length);
  });

  it("stays zero when the build has no panel commands", async () => {
    const load = vi.fn(() => Promise.reject(new Error("unknown command")));
    const subscribe = vi.fn(() => Promise.reject(new Error("unknown event")));
    const { result } = renderHook(() => usePanelDecisionCount({ load, subscribe }));
    await act(async () => { await load.mock.results[0]!.value.catch(() => undefined); });
    expect(result.current).toBe(0);
  });
});
