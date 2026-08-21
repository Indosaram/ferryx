import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { TabBar } from "./TabBar";

afterEach(cleanup);

describe("TabBar", () => {
  it("routes activate, close, and new-tab actions through real callbacks", () => {
    const onActivate = vi.fn();
    const onClose = vi.fn();
    const onAdd = vi.fn();
    render(
      <TabBar
        tabs={[
          { id: "tab-a", label: "main", sessionId: "session-a" },
          { id: "tab-b", label: "feature", sessionId: "session-b" },
        ]}
        activeTabId="tab-a"
        onActivate={onActivate}
        onClose={onClose}
        onAdd={onAdd}
      />,
    );

    const featureLabel = screen.getByText("feature");
    fireEvent.click(featureLabel.closest("button")!);
    expect(onActivate).toHaveBeenCalledWith("tab-b");

    fireEvent.click(screen.getByRole("button", { name: "Close main" }));
    expect(onClose).toHaveBeenCalledWith("tab-a");

    fireEvent.click(screen.getByRole("button", { name: "New terminal" }));
    expect(onAdd).toHaveBeenCalledOnce();
  });

  it("uses low-chrome 32px geometry and a one-pixel active indicator", () => {
    render(
      <TabBar
        tabs={[{ id: "tab-a", label: "main", sessionId: "session-a" }]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
      />,
    );

    expect(screen.getByTestId("tab-strip")).toHaveClass("h-tabbar");
    expect(screen.getByTestId("tab-active-indicator")).toHaveClass("h-px");
  });

  it("renders a card-colored tab strip with terminal background limited to the active tab", () => {
    const { rerender } = render(
      <TabBar
        tabs={[]}
        activeTabId=""
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
      />,
    );

    const emptyStrip = screen.getByTestId("tab-strip");
    expect(emptyStrip).toHaveClass("bg-card");
    expect(emptyStrip).not.toHaveClass("bg-terminal");

    rerender(
      <TabBar
        tabs={[
          { id: "tab-a", label: "main", sessionId: "session-a" },
          { id: "tab-b", label: "feature", sessionId: "session-b" },
        ]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
      />,
    );

    const activeTab = screen.getByText("main").closest("button")!;
    const inactiveTab = screen.getByText("feature").closest("button")!;

    expect(activeTab).toHaveClass("bg-terminal");
    expect(inactiveTab).not.toHaveClass("bg-terminal");
    expect(inactiveTab).toHaveClass("bg-card");
  });
});