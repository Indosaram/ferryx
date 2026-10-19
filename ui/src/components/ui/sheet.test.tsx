import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { Sheet, SheetContent, SheetDescription, SheetTitle, SheetTrigger } from "./sheet";

function Harness() {
  return (
    <Sheet>
      <SheetTrigger>Details for laptop</SheetTrigger>
      <SheetContent>
        <SheetTitle>laptop</SheetTitle>
        <SheetDescription>Paired machine details.</SheetDescription>
      </SheetContent>
    </Sheet>
  );
}

describe("Sheet", () => {
  afterEach(() => cleanup());

  it("opens via trigger as a right-anchored named dialog and closes via the Close button", async () => {
    render(<Harness />);
    const trigger = screen.getByRole("button", { name: "Details for laptop" });

    fireEvent.click(trigger);
    const sheet = await screen.findByRole("dialog", { name: "laptop" });
    expect(sheet).toHaveAccessibleDescription("Paired machine details.");
    expect(sheet).toHaveClass("fixed", "inset-y-0", "right-0", "w-full", "max-w-md");

    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(trigger).toHaveFocus());
  });

  it("closes on Escape and returns focus to the trigger", async () => {
    render(<Harness />);
    const trigger = screen.getByRole("button", { name: "Details for laptop" });

    trigger.focus();
    fireEvent.click(trigger);
    const sheet = await screen.findByRole("dialog", { name: "laptop" });

    fireEvent.keyDown(sheet, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(trigger).toHaveFocus());
  });
});
