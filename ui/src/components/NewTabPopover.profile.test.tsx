import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { NewTabPopover } from "./NewTabPopover";

afterEach(cleanup);

beforeEach(() => {
  localStorage.clear();
});

describe("NewTabPopover browser profiles", () => {
  it("passes an explicitly selected private profile to browser creation", () => {
    const onNewBrowser = vi.fn();
    render(
      <NewTabPopover
        open
        onClose={vi.fn()}
        onNewTerminal={vi.fn()}
        onNewBrowser={onNewBrowser}
      />,
    );

    fireEvent.change(screen.getByLabelText("Browser profile"), { target: { value: "private" } });
    fireEvent.click(screen.getByRole("button", { name: /New Browser Tab/i }));

    expect(onNewBrowser).toHaveBeenCalledWith("about:blank", "private");
  });
});
