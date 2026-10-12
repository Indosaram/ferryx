import "@testing-library/jest-dom/vitest";
import { useState } from "react";
import { Server } from "lucide-react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { MobileSheet, MobileSheetItem } from "./MobileSheet";

beforeEach(cleanup);
afterEach(cleanup);

/** Hosts a sheet behind a real trigger so focus capture and restore run end to end. */
function FocusHarness() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" data-testid="trigger" onClick={() => setOpen(true)}>
        Open sheet
      </button>
      <MobileSheet open={open} onOpenChange={setOpen} title="Actions" ariaLabel="Actions" testId="sheet">
        <MobileSheetItem label="New terminal" onSelect={() => {}} />
      </MobileSheet>
    </>
  );
}

describe("MobileSheet", () => {
  it("renders nothing while closed", () => {
    render(
      <MobileSheet open={false} onOpenChange={vi.fn()} title="Actions" ariaLabel="Actions" testId="sheet">
        <MobileSheetItem label="New terminal" />
      </MobileSheet>,
    );

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.queryByTestId("sheet")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Close Actions")).not.toBeInTheDocument();
    expect(screen.queryByText("New terminal")).not.toBeInTheDocument();
  });

  it("renders the fixed layer, backdrop and dialog panel when open", () => {
    render(
      <MobileSheet
        open
        onOpenChange={vi.fn()}
        title="Actions"
        ariaLabel="Actions"
        testId="sheet"
        footer={<button type="button">New terminal</button>}
      >
        <MobileSheetItem label="Machines" />
      </MobileSheet>,
    );

    const layer = screen.getByTestId("sheet");
    expect(layer).toHaveClass("fixed", "inset-0", "z-50");

    const backdrop = screen.getByTestId("sheet-backdrop");
    expect(backdrop).toHaveAttribute("aria-label", "Close Actions");
    expect(backdrop).toHaveClass("bg-black/50");

    const dialog = screen.getByRole("dialog", { name: "Actions" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog).toHaveClass(
      "rounded-t-[20px]",
      "bg-chat-surface",
      "border-t",
      "border-chat-border",
      "max-h-[80dvh]",
      "animate-sheet-in",
    );

    const handle = dialog.querySelector("span.h-1.w-9");
    expect(handle).toBeInTheDocument();
    expect(handle).toHaveClass("rounded-full", "bg-chat-foreground-secondary/40");

    const title = dialog.querySelector("h2");
    expect(title).toHaveTextContent("Actions");
    expect(title).toHaveClass("text-[15px]", "font-semibold");
    expect(dialog.querySelector('[aria-label="Close Actions"] svg')).toBeInTheDocument();

    const scroller = dialog.querySelector(".pb-safe-sheet");
    expect(scroller).toBeInTheDocument();
    expect(scroller).toHaveClass("overflow-y-auto");

    expect(screen.getByText("Machines")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "New terminal" })).toBeInTheDocument();
  });

  it("closes when Escape is pressed", () => {
    const onOpenChange = vi.fn();
    render(<MobileSheet open onOpenChange={onOpenChange} title="Actions" ariaLabel="Actions" />);

    fireEvent.keyDown(window, { key: "Escape" });

    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("closes when the backdrop is tapped", () => {
    const onOpenChange = vi.fn();
    render(<MobileSheet open onOpenChange={onOpenChange} title="Actions" ariaLabel="Actions" testId="sheet" />);

    fireEvent.click(screen.getByTestId("sheet-backdrop"));

    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("closes when the title row close button is tapped", () => {
    const onOpenChange = vi.fn();
    render(<MobileSheet open onOpenChange={onOpenChange} title="Actions" ariaLabel="Actions" />);

    const dialog = screen.getByRole("dialog", { name: "Actions" });
    expect(dialog.querySelector('[aria-label="Close Actions"]')).toHaveClass("size-11");
    fireEvent.click(dialog.querySelector('[aria-label="Close Actions"]') as HTMLElement);

    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("moves focus into the sheet on open and restores it to the trigger on close", () => {
    render(<FocusHarness />);

    const trigger = screen.getByTestId("trigger");
    trigger.focus();
    expect(document.activeElement).toBe(trigger);

    fireEvent.click(trigger);

    const dialog = screen.getByRole("dialog", { name: "Actions" });
    expect(dialog.contains(document.activeElement)).toBe(true);
    // The first focusable element inside the panel is the title row close button.
    expect(document.activeElement).toBe(dialog.querySelector('[aria-label="Close Actions"]'));

    fireEvent.click(screen.getByTestId("sheet-backdrop"));

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(document.activeElement).toBe(trigger);
  });
});

describe("MobileSheetItem", () => {
  it("ignores clicks while disabled", () => {
    const onSelect = vi.fn();
    render(
      <MobileSheet open onOpenChange={vi.fn()} title="Actions" ariaLabel="Actions">
        <MobileSheetItem testId="sheet-item" label="New terminal" onSelect={onSelect} disabled />
      </MobileSheet>,
    );

    const item = screen.getByTestId("sheet-item");
    expect(item).toBeDisabled();
    fireEvent.click(item);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("runs onSelect when enabled", () => {
    const onSelect = vi.fn();
    render(
      <MobileSheet open onOpenChange={vi.fn()} title="Actions" ariaLabel="Actions">
        <MobileSheetItem testId="sheet-item" label="New terminal" onSelect={onSelect} />
      </MobileSheet>,
    );

    fireEvent.click(screen.getByTestId("sheet-item"));
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("renders touch sizing, an 18px icon, a 15px label, a 13px description and aria-current", () => {
    render(
      <MobileSheet open onOpenChange={vi.fn()} title="Sessions" ariaLabel="Sessions">
        <MobileSheetItem
          testId="row-active"
          icon={Server}
          label="build box"
          description="orca/ws/main"
          selected
          onSelect={vi.fn()}
        />
        <MobileSheetItem testId="row-idle" icon={Server} label="local" selected={false} onSelect={vi.fn()} />
      </MobileSheet>,
    );

    const active = screen.getByTestId("row-active");
    expect(active).toHaveClass("min-h-12");
    expect(active.querySelector("svg")).toHaveClass("size-[18px]");
    expect(screen.getByText("build box")).toHaveClass("text-[15px]");
    expect(screen.getByText("orca/ws/main")).toHaveClass("text-[13px]", "text-chat-foreground-secondary");
    expect(active).toHaveAttribute("aria-current", "true");
    expect(active).not.toHaveAttribute("aria-selected");
    expect(screen.getByTestId("row-idle")).not.toHaveAttribute("aria-current");
    expect(screen.getByTestId("row-idle")).not.toHaveAttribute("aria-selected");
  });
});
