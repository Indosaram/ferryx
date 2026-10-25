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

function renderSections(sections: readonly RemoteSidebarSection[]) {
  render(<RemoteSidebar sections={sections} density="touch" pending={null} onSelect={() => undefined} view="worktrees" />);
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
