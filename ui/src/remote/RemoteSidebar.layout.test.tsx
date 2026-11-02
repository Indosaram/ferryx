// Phone widths stack one section per machine; an offline machine must not take an equal share of
// the height from the machine the user can open.
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { RemoteSidebar } from "./RemoteSidebar";
import { sidebarModelFixture } from "./remoteSidebarTestUtils";
import type { RemoteSidebarSection } from "./useRemoteSidebarModel";

const ROWS = [{ workspaceId: "ws-ferryx", slug: "main", label: "main", projectLabel: "ferryx" }];

function section(machineId: string, state: RemoteSidebarSection["state"], withModel: boolean): RemoteSidebarSection {
  return {
    machineId,
    label: machineId,
    state,
    model: withModel ? sidebarModelFixture(ROWS) : null,
    signals: withModel ? {} : null,
    error: state === "offline" ? { code: "OFFLINE", message: "This computer is not reachable" } : null,
  };
}

function renderSections(sections: readonly RemoteSidebarSection[], density: "compact" | "touch" = "compact") {
  render(<RemoteSidebar sections={sections} density={density} pending={null} onSelect={() => undefined} view="worktrees" />);
  return Object.fromEntries(screen.getAllByTestId("remote-sidebar-section").map((el) => [el.dataset.machineId, el.className.split(/\s+/)]));
}

afterEach(cleanup);

describe("remote sidebar section height", () => {
  it("lets the online machine fill the drawer while an offline machine with its last list sizes to content", () => {
    const classes = renderSections([section("offline-mac", "offline", true), section("online-mac", "ok", true)]);

    expect(classes["online-mac"]).toContain("flex-1");
    expect(classes["offline-mac"]).not.toContain("flex-1");
    expect(classes["offline-mac"]).toContain("flex-none");
  });

  it("keeps a still-loading machine to its status line", () => {
    const classes = renderSections([section("loading-mac", "loading", false), section("online-mac", "ok", true)]);

    expect(classes["loading-mac"]).not.toContain("flex-1");
    expect(classes["online-mac"]).toContain("flex-1");
  });
});

describe("remote sidebar on a phone", () => {
  it("scrolls the whole page once, with one filter for every machine", () => {
    const classes = renderSections([section("offline-mac", "offline", true), section("online-mac", "ok", true)], "touch");
    expect(screen.getAllByRole("searchbox", { name: "Filter worktrees" })).toHaveLength(1);
    expect(screen.getAllByTestId("remote-sidebar-scroller")).toHaveLength(1);
    expect(classes["offline-mac"]).not.toContain("max-h-[40%]");
    for (const section of screen.getAllByTestId("remote-sidebar-section")) {
      expect(section.querySelector(".overflow-y-auto")).toBeNull();
    }
  });
});
