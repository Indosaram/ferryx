import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import { describe, it, expect, vi, afterEach } from "vitest";
import { NewTabPopover } from "./NewTabPopover";

afterEach(cleanup);

describe("NewTabPopover", () => {
  it("renders search input and actions when open", () => {
    render(
      <NewTabPopover
        open={true}
        onClose={vi.fn()}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
      />
    );

    expect(
      screen.getByPlaceholderText("Search open tabs, files, URLs, agents...")
    ).toBeInTheDocument();
    expect(screen.getByText("New Terminal")).toBeInTheDocument();
    expect(screen.getByText("New Browser Tab")).toBeInTheDocument();
  });

  it("calls onNewTerminal when New Terminal is clicked", () => {
    const onNewTerminal = vi.fn();
    const onClose = vi.fn();
    render(
      <NewTabPopover
        open={true}
        onClose={onClose}
        onNewTerminal={onNewTerminal}
        onNewBrowser={vi.fn()}
      />
    );

    fireEvent.click(screen.getByText("New Terminal"));
    expect(onNewTerminal).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it("opens browser tab with url when URL is entered", () => {
    const onNewBrowser = vi.fn();
    const onClose = vi.fn();
    render(
      <NewTabPopover
        open={true}
        onClose={onClose}
        onNewTerminal={vi.fn()}
        onNewBrowser={onNewBrowser}
      />
    );

    const input = screen.getByPlaceholderText("Search open tabs, files, URLs, agents...");
    fireEvent.change(input, { target: { value: "github.com" } });
    fireEvent.submit(input.closest("form")!);

    expect(onNewBrowser).toHaveBeenCalledWith("https://github.com");
    expect(onClose).toHaveBeenCalled();
  });

  it("closes on Escape key", () => {
    const onClose = vi.fn();
    render(
      <NewTabPopover
        open={true}
        onClose={onClose}
        onNewTerminal={vi.fn()}
        onNewBrowser={vi.fn()}
      />
    );

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });
});
