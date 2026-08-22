import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { WorkspaceTab } from "../lib/types";
import { TabBar } from "./TabBar";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ startDragging: vi.fn() }),
}));

afterEach(cleanup);

function terminalTab(id: string, label: string): WorkspaceTab {
  return { id, label, sessionId: `session-${id}` };
}

describe("TabBar appearance", () => {
  it("keeps the active tab on application chrome instead of the terminal surface", () => {
    render(
      <TabBar
        groupId="group-a"
        tabs={[terminalTab("tab-a", "main"), terminalTab("tab-b", "feature")]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
      />,
    );

    const activeTab = screen.getByText("main").closest('[role="tab"]');
    const inactiveTab = screen.getByText("feature").closest('[role="tab"]');

    expect(activeTab).toHaveClass("bg-accent", "text-foreground");
    expect(activeTab).not.toHaveClass("bg-terminal");
    expect(inactiveTab).toHaveClass("bg-card", "text-muted-foreground");
  });
});
