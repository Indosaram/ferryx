import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { RemoteTerminalTabStrip, type RemoteStripTab } from "./RemoteTerminalTabStrip";
import type { RemoteTerminalTabInfo } from "./RemoteSessionList";

function tab(key: string, overrides: Partial<RemoteStripTab> = {}): RemoteStripTab {
  const pane: RemoteTerminalTabInfo = { id: key, label: key };
  return {
    key,
    label: key,
    ariaLabel: key,
    sessionId: null,
    agentType: null,
    activity: null,
    active: false,
    option: null,
    pane,
    ...overrides,
  };
}

function renderStrip(tabs: readonly RemoteStripTab[]) {
  return render(
    <RemoteTerminalTabStrip
      tabs={tabs}
      paneCount={tabs.length}
      locked={false}
      busyKey={null}
      onSelect={vi.fn()}
      createDisabled={false}
    />,
  );
}

describe("RemoteTerminalTabStrip", () => {
  afterEach(cleanup);

  it("shows waiting and done as distinct shapes with text instead of a ringed or outlined dot", () => {
    // Given: one waiting and one done tab
    renderStrip([tab("build", { activity: "waiting" }), tab("tests", { activity: "done" })]);

    // Then: waiting is a filled dot with text, done is a check glyph with text
    const waiting = screen.getByTestId("tab-waiting-indicator");
    const done = screen.getByTestId("tab-done-indicator");
    expect(waiting.innerHTML).not.toMatch(/ring-|border-/);
    expect(waiting).toHaveTextContent("Needs you");
    expect(done).toHaveTextContent("Done");
    expect(done.querySelector("svg")).not.toBeNull();
  });

  it("moves focus between tabs with arrow keys and keeps one tab in the tab order", () => {
    // Given: three tabs with the middle one active
    renderStrip([tab("a"), tab("b", { active: true }), tab("c")]);
    const tablist = screen.getByRole("tablist", { name: "Terminal tabs" });
    const [a, b, c] = within(tablist).getAllByRole("tab");
    expect([a, b, c].map((el) => el?.getAttribute("tabindex"))).toEqual(["-1", "0", "-1"]);
    expect(b).not.toHaveAttribute("aria-current");

    // When: ArrowRight from the active tab, then End, then ArrowRight wraps
    b?.focus();
    fireEvent.keyDown(tablist, { key: "ArrowRight" });
    expect(document.activeElement).toBe(c);
    fireEvent.keyDown(tablist, { key: "ArrowRight" });

    // Then: focus wraps to the first tab
    expect(document.activeElement).toBe(a);
  });

  it("hides the ordinal when no pane tab is active", () => {
    // Given: no active tab
    renderStrip([tab("a"), tab("b")]);

    // Then: no "? / N" placeholder is rendered
    expect(screen.queryByText(/\?\s*\/\s*2/)).toBeNull();
    expect(screen.queryByLabelText(/Terminal position/)).toBeNull();
  });
});
