import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { Attention, AttentionGroup, FleetSession, PanelEvent, PanelSnapshot } from "./contract";
import { emptyPanelSnapshot } from "./emptySnapshot";
import { sameAttentionMap, usePanelAttention } from "./usePanelAttention";

afterEach(cleanup);

function attentionFor(group: AttentionGroup): Attention {
  return {
    group,
    axes: {
      demand: group === "needsYou" ? "question" : "none",
      activity: group === "working" ? "working" : "stopped",
      completion: group === "unread" ? "reported" : "none",
      read: group === "unread" ? "unread" : "read",
      ownership: "owner",
    },
  };
}

function sessionWith(group: AttentionGroup, updatedAtMs: number): FleetSession {
  return {
    sessionRef: "local:sess-1",
    hostId: "local",
    hostLabel: "Local",
    title: "Pane",
    cwd: "/repo",
    agent: "codex",
    state: group === "working" ? "working" : "blocked",
    stateSource: "daemon",
    stateSinceMs: 1000,
    screenChangedAtMs: 1000,
    pendingPrompt: null,
    lastAssistantLine: null,
    updatedAtMs,
    stale: false,
    attention: attentionFor(group),
  };
}

function snapshotWith(group: AttentionGroup): PanelSnapshot {
  return {
    ...emptyPanelSnapshot(),
    sessions: [sessionWith(group, 1000)],
  };
}

describe("usePanelAttention", () => {
  it("shallow-compares attention maps by session group", () => {
    expect(sameAttentionMap({ a: { group: "working" } }, { a: { group: "working" } })).toBe(true);
    expect(sameAttentionMap({ a: { group: "working" } }, { a: { group: "needsYou" } })).toBe(false);
    expect(sameAttentionMap({ a: { group: "working" } }, {})).toBe(false);
  });

  it("keeps the same attention map reference when panel.session leaves groups unchanged", async () => {
    const handlers: Array<(event: PanelEvent) => void> = [];
    const initial = snapshotWith("working");
    const load = async () => initial;
    const subscribe = async (handler: (event: PanelEvent) => void) => {
      handlers.push(handler);
      return () => undefined;
    };
    const { result } = renderHook(() => usePanelAttention({ load, subscribe }));

    await waitFor(() => expect(result.current["sess-1"]?.group).toBe("working"));
    const firstRef = result.current;

    act(() => {
      handlers[0]?.({
        type: "panel.session",
        session: sessionWith("working", 2000),
      });
    });
    expect(result.current).toBe(firstRef);

    act(() => {
      handlers[0]?.({
        type: "panel.session",
        session: sessionWith("needsYou", 3000),
      });
    });
    expect(result.current).not.toBe(firstRef);
    expect(result.current["sess-1"]?.group).toBe("needsYou");
  });
});
