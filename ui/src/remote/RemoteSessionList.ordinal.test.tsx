import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  RemoteWorkspaceMirror,
  mirrorInventory,
  renderedPaneOrder,
  type RemoteContextOption,
  type RemoteWorkspaceModel,
} from "./RemoteSessionList";

const MACHINE_A = "mach-macos-primary-01";

function sessionOption(sessionId: string, sessionLabel: string): RemoteContextOption {
  return {
    workspaceId: "ws-ferryx-core",
    worktreeSlug: "main",
    worktreeLabel: "main",
    machineId: MACHINE_A,
    sessionId,
    sessionLabel,
    daemonEpoch: 101,
  };
}

/* The desktop strip order is first, second, third, but the option list orders sessions
   third, first, second — the order the tablist actually renders. The active tab is
   tab-third: strip order would number it 3, rendered order numbers it 1. */
function ordinalModel(): RemoteWorkspaceModel {
  return {
    context: {
      workspaceId: "ws-ferryx-core",
      worktreeSlug: "main",
      worktreeLabel: "main",
      activeTerminal: { sessionId: "s-third", running: true, daemonEpoch: 101 },
      activeTabId: "tab-third",
      terminalTabs: [
        { id: "tab-first", label: "first", worktreeSlug: "main", sessionId: "s-first", daemonEpoch: 101 },
        { id: "tab-second", label: "second", worktreeSlug: "main", sessionId: "s-second", daemonEpoch: 101 },
        { id: "tab-third", label: "third", worktreeSlug: "main", sessionId: "s-third", daemonEpoch: 101 },
      ],
    },
    options: [
      sessionOption("s-third", "third (active)"),
      sessionOption("s-first", "first"),
      sessionOption("s-second", "second"),
    ],
  };
}

function renderOrdinalMirror(model: RemoteWorkspaceModel) {
  const onSelect = vi.fn();
  render(
    <RemoteWorkspaceMirror
      model={model}
      pending={null}
      selectorOpen={true}
      onSelectorOpenChange={vi.fn()}
      onSelect={onSelect}
      activeMachineId={MACHINE_A}
    />,
  );
  return { onSelect };
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("remote terminal ordinal matches the rendered selection", () => {
  it("numbers the active tab by rendered row order, not the desktop strip order", () => {
    const { onSelect } = renderOrdinalMirror(ordinalModel());

    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((tab) => tab.textContent)).toEqual(["third", "first", "second"]);
    expect(tabs[0]).toHaveAttribute("aria-selected", "true");

    expect(screen.getByLabelText("Terminal position: Tab 1 of 3")).toBeInTheDocument();
    expect(screen.getByText("1 / 3")).toBeInTheDocument();
    expect(screen.queryByLabelText("Terminal position: Tab 3 of 3")).toBeNull();

    expect(screen.getByRole("button", { name: "Previous terminal tab" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Next terminal tab" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenCalledWith(
      expect.objectContaining({ tabId: "tab-first", sessionId: "s-first" }),
    );
  });

  it("claims no position when no rendered row holds the selection", () => {
    const model = ordinalModel();
    model.context.activeTabId = null;
    model.context.activeTerminal = null;
    renderOrdinalMirror(model);

    expect(screen.getByLabelText("Terminal position: unknown of 3")).toBeInTheDocument();
    expect(screen.getByText("? / 3")).toBeInTheDocument();
    expect(
      screen.getAllByRole("tab").some((tab) => tab.getAttribute("aria-selected") === "true"),
    ).toBe(false);

    expect(screen.getByRole("button", { name: "Previous terminal tab" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Next terminal tab" })).toBeDisabled();
  });

  it("exposes the shared rendered order the header and swipe navigation consume", () => {
    expect(
      renderedPaneOrder(mirrorInventory(ordinalModel(), MACHINE_A)).map((tab) => tab.id),
    ).toEqual(["tab-third", "tab-first", "tab-second"]);
  });
});
