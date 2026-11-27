import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import type { ComponentProps } from "react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ActivityMode } from "../../lib/sidebarPersistence";
import {
  DesktopActivityBar,
  ACTIVITY_BAR_WIDTH,
  MAC_TRAFFIC_LIGHT_CLEARANCE,
  MAC_TRAFFIC_LIGHT_OVERHANG,
  MAC_TRAFFIC_LIGHT_WIDTH,
} from "./DesktopActivityBar";

afterEach(cleanup);

/** The rail is controlled: this harness owns the mode like the desktop shell does. */
function Harness(props: Partial<ComponentProps<typeof DesktopActivityBar>> & { initialMode?: ActivityMode }) {
  const { initialMode = "worktrees", ...rest } = props;
  const [mode, setMode] = useState<ActivityMode>(initialMode);
  return (
    <>
      <DesktopActivityBar mode={mode} onModeChange={setMode} {...rest} />
      <output data-testid="mode">{mode}</output>
    </>
  );
}

function activityTabs() {
  return within(screen.getByRole("tablist", { name: "Activity" })).getAllByRole("tab");
}

describe("DesktopActivityBar", () => {
  it("is a fixed 48px vertical Activity tablist with the four modes in rail order", () => {
    render(<Harness />);

    const tablist = screen.getByRole("tablist", { name: "Activity" });
    expect(tablist).toHaveAttribute("aria-orientation", "vertical");
    expect(activityTabs().map((tab) => tab.getAttribute("aria-label"))).toEqual([
      "Supervisor",
      "Worktrees",
      "Browser",
      "Remote Desktop",
    ]);
    expect(screen.getByTestId("desktop-activity-bar")).toHaveStyle({ width: `${ACTIVITY_BAR_WIDTH}px` });
  });

  it("selects a mode on click and marks exactly one tab selected", () => {
    render(<Harness />);

    fireEvent.click(screen.getByRole("tab", { name: "Browser" }));

    expect(screen.getByTestId("mode")).toHaveTextContent("browser");
    const selected = activityTabs().filter((tab) => tab.getAttribute("aria-selected") === "true");
    expect(selected.map((tab) => tab.getAttribute("aria-label"))).toEqual(["Browser"]);
  });

  it("moves and selects with the arrow, Home and End keys without leaving the tablist", () => {
    render(<Harness initialMode="supervisor" />);
    const [supervisor] = activityTabs();
    supervisor!.focus();

    fireEvent.keyDown(supervisor!, { key: "ArrowDown" });
    expect(screen.getByTestId("mode")).toHaveTextContent("worktrees");
    expect(document.activeElement).toBe(screen.getByRole("tab", { name: "Worktrees" }));

    // Wraps from the last mode back to the first.
    fireEvent.keyDown(document.activeElement!, { key: "ArrowUp" });
    expect(screen.getByTestId("mode")).toHaveTextContent("supervisor");
    fireEvent.keyDown(document.activeElement!, { key: "ArrowUp" });
    expect(screen.getByTestId("mode")).toHaveTextContent("remoteDesktop");

    fireEvent.keyDown(document.activeElement!, { key: "Home" });
    expect(screen.getByTestId("mode")).toHaveTextContent("supervisor");
    fireEvent.keyDown(document.activeElement!, { key: "End" });
    expect(screen.getByTestId("mode")).toHaveTextContent("remoteDesktop");
  });

  it("selects with Enter and Space, and keeps one roving tab stop", () => {
    render(<Harness />);

    fireEvent.keyDown(screen.getByRole("tab", { name: "Browser" }), { key: "Enter" });
    expect(screen.getByTestId("mode")).toHaveTextContent("browser");

    fireEvent.keyDown(screen.getByRole("tab", { name: "Remote Desktop" }), { key: " " });
    expect(screen.getByTestId("mode")).toHaveTextContent("remoteDesktop");

    const stops = activityTabs().filter((tab) => tab.getAttribute("tabindex") === "0");
    expect(stops.map((tab) => tab.getAttribute("aria-label"))).toEqual(["Remote Desktop"]);
  });

  it("shows a numeric or dot badge per mode and folds the count into the accessible name", () => {
    const { rerender } = render(
      <Harness supervisorBadge={3} worktreesBadge="dot" browserBadge={0} remoteDesktopBadge={120} />,
    );

    expect(screen.getByTestId("activity-mode-supervisor-badge")).toHaveTextContent("3");
    expect(screen.getByTestId("activity-mode-worktrees-badge")).toBeEmptyDOMElement();
    expect(screen.queryByTestId("activity-mode-browser-badge")).toBeNull();
    expect(screen.getByTestId("activity-mode-remoteDesktop-badge")).toHaveTextContent("99+");
    // The name stays the mode's own; the count rides along as a description.
    const supervisorTab = screen.getByRole("tab", { name: "Supervisor" });
    const describedBy = supervisorTab.getAttribute("aria-describedby");
    expect(describedBy).toBeTruthy();
    expect(document.getElementById(describedBy!)).toHaveTextContent("3 waiting");
    const worktreesTab = screen.getByRole("tab", { name: "Worktrees" });
    expect(document.getElementById(worktreesTab.getAttribute("aria-describedby")!)).toHaveTextContent("Attention needed");
    expect(screen.getByRole("tab", { name: "Browser" })).not.toHaveAttribute("aria-describedby");

    rerender(<Harness supervisorBadge={null} worktreesBadge={null} />);
    expect(screen.queryByTestId("activity-mode-supervisor-badge")).toBeNull();
    expect(screen.queryByTestId("activity-mode-worktrees-badge")).toBeNull();
  });

  it("renders the macOS traffic-light pad at titlebar height (36px) and exposes the 24px overhang", () => {
    expect(MAC_TRAFFIC_LIGHT_CLEARANCE).toBe(36);
    expect(MAC_TRAFFIC_LIGHT_WIDTH).toBe(72);
    expect(MAC_TRAFFIC_LIGHT_OVERHANG).toBe(24);

    const { rerender } = render(<Harness isMac />);
    const pad = screen.getByTestId("titlebar-traffic-light-pad");
    expect(pad).toHaveStyle({ height: "36px" });
    expect(screen.getByTestId("desktop-activity-bar")).toHaveAttribute("data-tauri-drag-region");

    rerender(<Harness isMac={false} />);
    expect(screen.queryByTestId("titlebar-traffic-light-pad")).toBeNull();
  });

  it("wires data-shortcut and tooltip labels for every mode, Settings, and the drawer toggle", () => {
    const { rerender } = render(
      <Harness
        isMac
        sidebarOpen
        supervisorBadge={3}
        onOpenSettings={() => undefined}
        onToggleSidebar={() => undefined}
      />,
    );

    expect(screen.getByTestId("activity-mode-supervisor")).toHaveAttribute("data-shortcut", "activity.mode1");
    expect(screen.getByTestId("activity-mode-worktrees")).toHaveAttribute("data-shortcut", "activity.mode2");
    expect(screen.getByTestId("activity-mode-browser")).toHaveAttribute("data-shortcut", "activity.mode3");
    expect(screen.getByTestId("activity-mode-remoteDesktop")).toHaveAttribute("data-shortcut", "activity.mode4");
    expect(screen.getByTestId("activity-settings")).toHaveAttribute("data-shortcut", "settings.toggle");
    expect(screen.getByTestId("activity-drawer-toggle")).toHaveAttribute("data-shortcut", "sidebar.left.toggle");

    expect(screen.getByTestId("activity-mode-supervisor")).toHaveAttribute("title", "Supervisor (⌘⌃1) - 3 waiting");
    expect(screen.getByTestId("activity-mode-worktrees")).toHaveAttribute("title", "Worktrees (⌘⌃2)");
    expect(screen.getByTestId("activity-mode-browser")).toHaveAttribute("title", "Browser (⌘⌃3)");
    expect(screen.getByTestId("activity-mode-remoteDesktop")).toHaveAttribute("title", "Remote Desktop (⌘⌃4)");
    expect(screen.getByTestId("activity-settings")).toHaveAttribute("title", "Settings (⌘,)");
    expect(screen.getByTestId("activity-drawer-toggle")).toHaveAttribute("title", "Collapse sidebar (⌘B)");

    rerender(
      <Harness
        isMac={false}
        sidebarOpen={false}
        onOpenSettings={() => undefined}
        onToggleSidebar={() => undefined}
      />,
    );
    expect(screen.getByTestId("activity-mode-supervisor")).toHaveAttribute("title", "Supervisor (Ctrl+Alt+Shift+1)");
    expect(screen.getByTestId("activity-settings")).toHaveAttribute("title", "Settings (Ctrl+,)");
    expect(screen.getByTestId("activity-drawer-toggle")).toHaveAttribute("title", "Show sidebar (Ctrl+B)");
  });

  it("toggles only the drawer from the rail and keeps the mode selection", () => {
    const onToggleSidebar = vi.fn();
    const { rerender } = render(<Harness sidebarOpen onToggleSidebar={() => onToggleSidebar()} />);

    fireEvent.click(screen.getByTestId("activity-drawer-toggle"));
    expect(onToggleSidebar).toHaveBeenCalledOnce();
    expect(screen.getByTestId("mode")).toHaveTextContent("worktrees");

    rerender(<Harness sidebarOpen={false} onToggleSidebar={() => onToggleSidebar()} />);
    expect(screen.getByRole("button", { name: "Show sidebar" })).toHaveAttribute("aria-expanded", "false");
  });

  it("offers Resources and Settings at the bottom, and omits the actions a host does not pass", () => {
    const onOpenResources = vi.fn();
    const onOpenSettings = vi.fn();
    const { rerender } = render(<Harness onOpenResources={onOpenResources} onOpenSettings={onOpenSettings} />);

    fireEvent.click(screen.getByRole("button", { name: "System resources" }));
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(onOpenResources).toHaveBeenCalledOnce();
    expect(onOpenSettings).toHaveBeenCalledOnce();

    rerender(<Harness />);
    expect(screen.queryByRole("button", { name: "System resources" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Settings" })).toBeNull();
    expect(screen.queryByTestId("activity-drawer-toggle")).toBeNull();
  });

  it("keeps the rail's buttons out of the drag region", () => {
    render(<Harness sidebarOpen onToggleSidebar={() => undefined} />);
    for (const tab of activityTabs()) expect(tab).toHaveClass("no-drag");
    expect(screen.getByTestId("activity-drawer-toggle")).toHaveClass("no-drag");
  });
});
