import { describe, expect, it } from "vitest";

import { isTerminalTab, type RemoteDesktopTab, type TerminalTab } from "../lib/types";
import { createLayoutState, defaultContentForTab, layoutReducer, normalizeLayout, toPaneContent } from "./layout";
import { moveTabIntoPaneSplit } from "./tabPaneDrop";

const rdTab: RemoteDesktopTab = { kind: "remoteDesktop", id: "rd", label: "Studio Mac", hostId: "host-1", sessionId: "" };
const terminal: TerminalTab = { id: "term", label: "shell", sessionId: "sess-1" };

describe("remoteDesktop pane content", () => {
  it("is not a terminal tab and owns no session", () => {
    expect(isTerminalTab(rdTab)).toBe(false);
    const layout = layoutReducer(createLayoutState([terminal], terminal.id), { type: "ADD_TAB", tab: rdTab });
    const paneLayout = layout.layoutsByTabId[rdTab.id];
    const leafId = paneLayout.activeLeafId!;
    expect(paneLayout.contentsByLeafId?.[leafId]).toEqual({ kind: "remoteDesktop", hostId: "host-1" });
    expect(paneLayout.sessionIdsByLeafId[leafId]).toBe("");
    expect(layout.activeTabId).toBe(rdTab.id);
    expect(layout.returnTabIdByTabId?.[rdTab.id]).toBe(terminal.id);
  });

  it("restores from raw persisted content and survives normalization", () => {
    expect(toPaneContent({ kind: "remoteDesktop", hostId: "host-2" })).toEqual({ kind: "remoteDesktop", hostId: "host-2" });
    expect(toPaneContent({ kind: "remoteDesktop" }, "fallback")).toEqual({ kind: "terminal", sessionId: "fallback" });
    expect(defaultContentForTab(rdTab)).toEqual({ kind: "remoteDesktop", hostId: "host-1" });
    const normalized = normalizeLayout(JSON.parse(JSON.stringify(createLayoutState([rdTab], rdTab.id))));
    const paneLayout = normalized.layoutsByTabId[rdTab.id];
    expect(paneLayout.contentsByLeafId?.[paneLayout.activeLeafId!]).toEqual({ kind: "remoteDesktop", hostId: "host-1" });
  });

  it("refuses to split into or out of a remote desktop tab", () => {
    const layout = createLayoutState([terminal, rdTab], terminal.id);
    const terminalLeaf = layout.layoutsByTabId[terminal.id].activeLeafId!;
    const rdLeaf = layout.layoutsByTabId[rdTab.id].activeLeafId!;
    expect(moveTabIntoPaneSplit(layout, rdTab.id, terminal.id, terminalLeaf, "horizontal").tabs).toHaveLength(2);
    expect(moveTabIntoPaneSplit(layout, terminal.id, rdTab.id, rdLeaf, "horizontal").tabs).toHaveLength(2);
  });
});
