import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SidebarStateSlot } from "./SidebarStateSlot";

afterEach(cleanup);

describe("SidebarStateSlot", () => {
  it("labels waiting as needs input and working as running, each with a status dot", () => {
    const { container, rerender } = render(<SidebarStateSlot state="waiting" data-testid="slot" />);
    expect(screen.getByTestId("slot")).toHaveTextContent("needs input");
    expect(container.querySelector('[data-status-state="waiting"]')).toBeInTheDocument();

    rerender(<SidebarStateSlot state="working" label="3 running" data-testid="slot" />);
    expect(screen.getByTestId("slot")).toHaveTextContent("3 running");
    expect(container.querySelector('[data-status-state="working"]')).toBeInTheDocument();
  });

  it.each(["unread", "done"] as const)("renders only a dot for %s", (state) => {
    const { container } = render(<SidebarStateSlot state={state} label="ignored" data-testid="slot" />);
    expect(screen.getByTestId("slot")).toHaveTextContent("");
    expect(container.querySelector(`[data-status-state="${state}"]`)).toBeInTheDocument();
  });

  it("renders a truncated host with a server icon for a remote slot", () => {
    render(<SidebarStateSlot state={null} host="omarchy" data-testid="slot" />);
    const slot = screen.getByTestId("slot");
    expect(slot).toHaveTextContent("omarchy");
    expect(screen.getByText("omarchy")).toHaveClass("max-w-[88px]", "truncate");
    expect(slot.querySelector("svg")).toBeInTheDocument();
  });

  it("renders nothing when idle", () => {
    const { container } = render(<SidebarStateSlot state={null} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("is a span without onActivate and a button with it", () => {
    const { rerender } = render(<SidebarStateSlot state={null} label="Connected" data-testid="slot" />);
    expect(screen.getByTestId("slot").tagName).toBe("SPAN");
    expect(screen.queryByRole("button")).not.toBeInTheDocument();

    const onActivate = vi.fn();
    rerender(<SidebarStateSlot state={null} label="Offline" ariaLabel="Open remote settings" onActivate={onActivate} data-testid="slot" />);
    fireEvent.click(screen.getByRole("button", { name: "Open remote settings" }));
    expect(onActivate).toHaveBeenCalledOnce();
  });
});
