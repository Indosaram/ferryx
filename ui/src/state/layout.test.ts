import { describe, expect, it } from "vitest";

import type { TerminalTab } from "../lib/types";
import { createLayoutState, layoutReducer } from "./layout";

function tab(id: string, sessionId: string): TerminalTab {
  return { id, label: id, sessionId };
}

describe("layoutReducer", () => {
  it("enables a single-tab mirror split without adding another logical tab", () => {
    const primary = tab("tab-1", "session-1");
    const state = createLayoutState([primary], primary.id);

    const next = layoutReducer(state, {
      type: "ENABLE_SPLIT",
      orientation: "horizontal",
      secondaryTabId: primary.id,
    });

    expect(next.tabs).toEqual([primary]);
    expect(next.primaryTabId).toBe(primary.id);
    expect(next.secondaryTabId).toBe(primary.id);
    expect(next.split).toBe("horizontal");
  });

  it("uses an existing other tab as the secondary pane when available", () => {
    const primary = tab("tab-1", "session-1");
    const secondary = tab("tab-2", "session-2");
    const state = createLayoutState([primary, secondary], primary.id);

    const next = layoutReducer(state, {
      type: "ENABLE_SPLIT",
      orientation: "horizontal",
      secondaryTabId: secondary.id,
    });

    expect(next.tabs).toEqual([primary, secondary]);
    expect(next.primaryTabId).toBe(primary.id);
    expect(next.secondaryTabId).toBe(secondary.id);
    expect(next.split).toBe("horizontal");
  });

  it("rotates split orientation without changing tab identity", () => {
    const primary = tab("tab-1", "session-1");
    const split = layoutReducer(createLayoutState([primary], primary.id), {
      type: "ENABLE_SPLIT",
      orientation: "horizontal",
      secondaryTabId: primary.id,
    });

    const rotated = layoutReducer(split, { type: "ROTATE_SPLIT" });

    expect(rotated.split).toBe("vertical");
    expect(rotated.tabs).toEqual(split.tabs);
    expect(rotated.primaryTabId).toBe(split.primaryTabId);
    expect(rotated.secondaryTabId).toBe(split.secondaryTabId);
  });

  it("applies the last-tab-close replacement policy atomically", () => {
    const primary = tab("tab-1", "session-1");
    const replacement = tab("tab-2", "session-2");
    const state = createLayoutState([primary], primary.id);

    const next = layoutReducer(state, {
      type: "CLOSE_TAB",
      tabId: primary.id,
      replacementTab: replacement,
    });

    expect(next.tabs).toEqual([replacement]);
    expect(next.primaryTabId).toBe(replacement.id);
    expect(next.secondaryTabId).toBeNull();
    expect(next.split).toBe("none");
  });

  it("never leaves stale split ids when the secondary tab closes", () => {
    const primary = tab("tab-1", "session-1");
    const secondary = tab("tab-2", "session-2");
    const split = layoutReducer(createLayoutState([primary, secondary], primary.id), {
      type: "ENABLE_SPLIT",
      orientation: "horizontal",
      secondaryTabId: secondary.id,
    });

    const next = layoutReducer(split, { type: "CLOSE_TAB", tabId: secondary.id });

    expect(next.tabs).toEqual([primary]);
    expect(next.primaryTabId).toBe(primary.id);
    expect(next.secondaryTabId).toBeNull();
    expect(next.split).toBe("none");
  });
});
