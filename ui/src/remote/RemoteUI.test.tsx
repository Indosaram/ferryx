import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { MobileKeyDock } from "../components/MobileKeyDock";
import { PairingPage } from "./PairingPage";
import { RemoteSessionList } from "./RemoteSessionList";

describe("Remote UI Components", () => {
  it("PairingPage renders 6-digit PIN input and handles submission", () => {
    const handlePaired = vi.fn();
    render(<PairingPage onPaired={handlePaired} />);

    const input = screen.getByPlaceholderText(/6-digit PIN/i);
    expect(input).toBeInTheDocument();
    expect(input).toHaveAttribute("maxLength", "6");
  });

  it("RemoteSessionList displays active terminal sessions and attach buttons", () => {
    const handleSelect = vi.fn();
    const handleRefresh = vi.fn();
    const sessions = [
      {
        sessionId: "test-session-123456",
        title: "Main Shell",
        worktreeLabel: "feature/remote-test",
        running: true,
      },
    ];

    render(
      <RemoteSessionList
        sessions={sessions}
        onSelect={handleSelect}
        onRefresh={handleRefresh}
      />
    );

    expect(screen.getByText(/Main Shell/i)).toBeInTheDocument();
    expect(screen.getByText(/feature\/remote-test/i)).toBeInTheDocument();

    fireEvent.click(screen.getByText(/Attach/i));
    expect(handleSelect).toHaveBeenCalledWith("test-session-123456");
  });

  it("MobileKeyDock dispatches primary key actions and latches Ctrl/Alt modifiers", () => {
    const handleSendKey = vi.fn();
    render(<MobileKeyDock onSendKey={handleSendKey} />);

    fireEvent.click(screen.getByText("Ctrl-C"));
    expect(handleSendKey).toHaveBeenCalledWith("ctrl-c");

    fireEvent.click(screen.getByText("Tab"));
    expect(handleSendKey).toHaveBeenCalledWith("tab");

    // Latch Ctrl + C
    fireEvent.click(screen.getByText("Ctrl"));
    fireEvent.click(screen.getByText("Tab"));
    expect(handleSendKey).toHaveBeenCalledWith("ctrl-tab");
  });
});
