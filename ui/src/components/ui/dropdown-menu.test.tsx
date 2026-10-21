import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "./dropdown-menu";

describe("DropdownMenu", () => {
  afterEach(() => cleanup());

  it("opens with Enter on the trigger and fires onSelect for the chosen item", async () => {
    const onRename = vi.fn();
    const onForget = vi.fn();
    render(
      <DropdownMenu>
        <DropdownMenuTrigger>Machine actions</DropdownMenuTrigger>
        <DropdownMenuContent>
          <DropdownMenuItem onSelect={onRename}>Rename</DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="destructive" onSelect={onForget}>
            Forget
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>,
    );
    const trigger = screen.getByRole("button", { name: "Machine actions" });

    trigger.focus();
    fireEvent.keyDown(trigger, { key: "Enter" });
    await screen.findByRole("menu");
    const forget = screen.getByRole("menuitem", { name: "Forget" });
    expect(forget).toHaveClass("text-destructive");

    fireEvent.click(forget);
    expect(onForget).toHaveBeenCalledTimes(1);
    expect(onRename).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  });
});
