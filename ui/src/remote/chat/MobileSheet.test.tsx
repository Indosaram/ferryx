import "@testing-library/jest-dom/vitest";
import { useState } from "react";
import { Server } from "lucide-react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { MobileSheet, MobileSheetItem } from "./MobileSheet";

beforeEach(cleanup);
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

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

function ControlledSheetHarness({ initialOpen = true }: { initialOpen?: boolean }) {
  const [open, setOpen] = useState(initialOpen);
  return (
    <>
      <button type="button" onClick={() => setOpen(false)} data-testid="external-close">
        Close
      </button>
      <button type="button" onClick={() => setOpen(true)} data-testid="external-open">
        Open
      </button>
      <MobileSheet open={open} onOpenChange={setOpen} title="Actions" ariaLabel="Actions" testId="sheet">
        <MobileSheetItem label="Item 1" />
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
    // the scrim is a pointer target only: one named Close control, not two
    expect(backdrop).toHaveAttribute("aria-hidden", "true");
    expect(backdrop.tagName).not.toBe("BUTTON");
    expect(backdrop).toHaveClass("bg-chat-screen/80");
    expect(screen.getAllByRole("button", { name: "Close Actions" })).toHaveLength(1);

    const dialog = screen.getByRole("dialog", { name: "Actions" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog).toHaveClass(
      "rounded-t-[1.25rem]",
      "bg-chat-surface",
      "border-t",
      "border-chat-border",
      "max-h-[80dvh]",
      "animate-sheet-in",
    );

    // no grabber: the sheet has no drag-to-dismiss, so it does not draw the affordance
    expect(dialog.querySelector("span.h-1.w-9")).not.toBeInTheDocument();

    const title = dialog.querySelector("h2");
    expect(title).toHaveTextContent("Actions");
    expect(title).toHaveClass("text-[0.9375rem]", "font-semibold");
    expect(dialog.querySelector('[aria-label="Close Actions"] svg')).toBeInTheDocument();

    // the footer sits outside the scroller and carries the safe-area padding itself
    const footer = screen.getByRole("button", { name: "New terminal" }).parentElement as HTMLElement;
    expect(footer).toHaveClass("pb-safe-sheet");
    expect(footer.closest(".overflow-y-auto")).toBeNull();

    expect(screen.getByText("Machines")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "New terminal" })).toBeInTheDocument();
  });

  it("closes when Escape is pressed", () => {
    const onOpenChange = vi.fn();
    render(<MobileSheet open onOpenChange={onOpenChange} title="Actions" ariaLabel="Actions" />);

    fireEvent.keyDown(window, { key: "Escape" });

    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("closes only the topmost of two stacked sheets on Escape", () => {
    const closeLower = vi.fn();
    const closeUpper = vi.fn();
    render(
      <>
        <MobileSheet open onOpenChange={closeLower} title="Actions" ariaLabel="Actions" />
        <MobileSheet open onOpenChange={closeUpper} title="Options" ariaLabel="Options" />
      </>,
    );
    fireEvent.keyDown(window, { key: "Escape" });
    expect(closeUpper).toHaveBeenCalledWith(false);
    expect(closeLower).not.toHaveBeenCalled();
  });

  it("makes the page behind it inert while open and restores it on close", () => {
    render(
      <div>
        <button type="button" data-testid="behind">Behind</button>
        <ControlledSheetHarness initialOpen />
      </div>,
    );
    expect(screen.getByTestId("behind")).toHaveAttribute("inert");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByTestId("behind")).not.toHaveAttribute("inert");
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
    fireEvent.animationEnd(dialog);

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(document.activeElement).toBe(trigger);
  });

  it("ui-shell-sheets-2: traps Tab and Shift+Tab inside the dialog, wrapping at both edges", () => {
    render(
      <>
        <button type="button" data-testid="outside">Outside</button>
        <MobileSheet open onOpenChange={vi.fn()} title="Actions" ariaLabel="Actions">
          <MobileSheetItem testId="item-1" label="First" onSelect={vi.fn()} />
          <MobileSheetItem testId="item-2" label="Last" onSelect={vi.fn()} />
        </MobileSheet>
      </>,
    );

    const dialog = screen.getByRole("dialog", { name: "Actions" });
    const closeButton = dialog.querySelector('[aria-label="Close Actions"]') as HTMLElement;
    const last = screen.getByTestId("item-2");

    last.focus();
    const forward = fireEvent.keyDown(last, { key: "Tab" });
    expect(forward).toBe(false);
    expect(document.activeElement).toBe(closeButton);

    const backward = fireEvent.keyDown(closeButton, { key: "Tab", shiftKey: true });
    expect(backward).toBe(false);
    expect(document.activeElement).toBe(last);

    // Tab between interior elements keeps native order (not intercepted).
    const first = screen.getByTestId("item-1");
    first.focus();
    expect(fireEvent.keyDown(first, { key: "Tab" })).toBe(true);
  });

  it("close plays the exit and unmounts only after animationend", () => {
    render(<ControlledSheetHarness initialOpen />);
    const dialog = screen.getByRole("dialog", { name: "Actions" });
    expect(dialog).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("external-close"));

    expect(screen.getByRole("dialog", { name: "Actions" })).toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Actions" })).toHaveClass("animate-sheet-out");

    fireEvent.animationEnd(screen.getByRole("dialog", { name: "Actions" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("re-opening during the exit keeps the sheet open and mounted", () => {
    render(<ControlledSheetHarness initialOpen />);
    const dialog = screen.getByRole("dialog", { name: "Actions" });
    expect(dialog).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("external-close"));
    expect(screen.getByRole("dialog", { name: "Actions" })).toHaveClass("animate-sheet-out");

    fireEvent.click(screen.getByTestId("external-open"));
    expect(screen.getByRole("dialog", { name: "Actions" })).toHaveClass("animate-sheet-in");

    fireEvent.animationEnd(screen.getByRole("dialog", { name: "Actions" }));
    expect(screen.getByRole("dialog", { name: "Actions" })).toBeInTheDocument();
  });

  it("close button title equals its accessible name", () => {
    render(<MobileSheet open onOpenChange={vi.fn()} title="Actions" ariaLabel="Actions" />);
    const dialog = screen.getByRole("dialog", { name: "Actions" });
    const closeBtn = dialog.querySelector('button[aria-label="Close Actions"]');
    expect(closeBtn).toHaveAttribute("title", "Close Actions");
  });

  it("with animation:none the sheet unmounts without any animationend", () => {
    const origGetComputedStyle = window.getComputedStyle;
    vi.spyOn(window, "getComputedStyle").mockImplementation((el: Element, pseudo?: string | null) => {
      const style = origGetComputedStyle(el, pseudo);
      return new Proxy(style, {
        get(target, prop) {
          if (prop === "animationName") return "none";
          if (prop === "animationDuration") return "0s";
          const val = Reflect.get(target, prop);
          return typeof val === "function" ? val.bind(target) : val;
        },
      });
    });

    render(<ControlledSheetHarness initialOpen />);
    expect(screen.getByRole("dialog", { name: "Actions" })).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("external-close"));

    // Sheet unmounts immediately without any animationend fired
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("with animationDuration 0s the sheet unmounts without any animationend", () => {
    const origGetComputedStyle = window.getComputedStyle;
    vi.spyOn(window, "getComputedStyle").mockImplementation((el: Element, pseudo?: string | null) => {
      const style = origGetComputedStyle(el, pseudo);
      return new Proxy(style, {
        get(target, prop) {
          if (prop === "animationName") return "sheet-out";
          if (prop === "animationDuration") return "0s";
          const val = Reflect.get(target, prop);
          return typeof val === "function" ? val.bind(target) : val;
        },
      });
    });

    render(<ControlledSheetHarness initialOpen />);
    expect(screen.getByRole("dialog", { name: "Actions" })).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("external-close"));

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("with a running animation and animationend never fired, it unmounts after the fallback time", () => {
    vi.useFakeTimers();
    const origGetComputedStyle = window.getComputedStyle;
    vi.spyOn(window, "getComputedStyle").mockImplementation((el: Element, pseudo?: string | null) => {
      const style = origGetComputedStyle(el, pseudo);
      return new Proxy(style, {
        get(target, prop) {
          if (prop === "animationName") return "sheet-out";
          if (prop === "animationDuration") return "200ms";
          if (prop === "animationDelay") return "0s";
          const val = Reflect.get(target, prop);
          return typeof val === "function" ? val.bind(target) : val;
        },
      });
    });

    render(<ControlledSheetHarness initialOpen />);
    expect(screen.getByRole("dialog", { name: "Actions" })).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("external-close"));

    // Immediately after close, dialog is still in exit transition
    expect(screen.getByRole("dialog", { name: "Actions" })).toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Actions" })).toHaveClass("animate-sheet-out");

    // Advance timers before fallback deadline (200ms duration + 0ms delay + 100ms margin = 300ms)
    act(() => {
      vi.advanceTimersByTime(250);
    });
    expect(screen.getByRole("dialog", { name: "Actions" })).toBeInTheDocument();

    // Advance past fallback deadline
    act(() => {
      vi.advanceTimersByTime(60);
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("re-open during closing cancels the fallback and stays mounted", () => {
    vi.useFakeTimers();
    const origGetComputedStyle = window.getComputedStyle;
    vi.spyOn(window, "getComputedStyle").mockImplementation((el: Element, pseudo?: string | null) => {
      const style = origGetComputedStyle(el, pseudo);
      return new Proxy(style, {
        get(target, prop) {
          if (prop === "animationName") return "sheet-out";
          if (prop === "animationDuration") return "200ms";
          if (prop === "animationDelay") return "0s";
          const val = Reflect.get(target, prop);
          return typeof val === "function" ? val.bind(target) : val;
        },
      });
    });

    render(<ControlledSheetHarness initialOpen />);
    expect(screen.getByRole("dialog", { name: "Actions" })).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("external-close"));
    expect(screen.getByRole("dialog", { name: "Actions" })).toHaveClass("animate-sheet-out");

    // Advance 100ms into the exit animation
    act(() => {
      vi.advanceTimersByTime(100);
    });

    // Re-open while closing
    fireEvent.click(screen.getByTestId("external-open"));
    expect(screen.getByRole("dialog", { name: "Actions" })).toHaveClass("animate-sheet-in");

    // Advance past the original fallback deadline
    act(() => {
      vi.advanceTimersByTime(300);
    });

    // The sheet must STILL be mounted because fallback was cancelled
    expect(screen.getByRole("dialog", { name: "Actions" })).toBeInTheDocument();
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
    expect(active.querySelector("svg")).toHaveClass("size-[1.125rem]");
    expect(screen.getByText("build box")).toHaveClass("text-[0.9375rem]");
    expect(screen.getByText("orca/ws/main")).toHaveClass("text-[0.8125rem]", "text-chat-foreground-secondary");
    expect(active).toHaveAttribute("aria-current", "true");
    expect(active).not.toHaveAttribute("aria-selected");
    expect(screen.getByTestId("row-idle")).not.toHaveAttribute("aria-current");
    expect(screen.getByTestId("row-idle")).not.toHaveAttribute("aria-selected");
  });
});
