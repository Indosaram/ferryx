import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { Dialog, DialogContent, DialogDescription, DialogTitle, DialogTrigger } from "./dialog";

function Harness() {
  return (
    <Dialog>
      <DialogTrigger>Open dialog</DialogTrigger>
      <DialogContent>
        <DialogTitle>Machine details</DialogTitle>
        <DialogDescription>Inspect this machine.</DialogDescription>
      </DialogContent>
    </Dialog>
  );
}

describe("Dialog", () => {
  afterEach(() => cleanup());

  it("opens via trigger, names the dialog from its title, and closes via the Close button", async () => {
    render(<Harness />);
    const trigger = screen.getByRole("button", { name: "Open dialog" });

    fireEvent.click(trigger);
    const dialog = await screen.findByRole("dialog", { name: "Machine details" });
    expect(dialog).toHaveAccessibleDescription("Inspect this machine.");

    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(trigger).toHaveFocus());
  });

  it("closes on Escape and returns focus to the trigger", async () => {
    render(<Harness />);
    const trigger = screen.getByRole("button", { name: "Open dialog" });

    trigger.focus();
    fireEvent.click(trigger);
    const dialog = await screen.findByRole("dialog", { name: "Machine details" });

    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(trigger).toHaveFocus());
  });
});
