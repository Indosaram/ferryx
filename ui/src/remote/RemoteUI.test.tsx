import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MobileKeyDock } from "../components/MobileKeyDock";
import { PairingPage } from "./PairingPage";
import { RemoteApp } from "./RemoteApp";
import { RemoteSessionList } from "./RemoteSessionList";

afterEach(cleanup);

describe("Remote UI Components", () => {
  it("PairingPage renders 6-digit PIN input and handles submission", () => {
    const handlePaired = vi.fn();
    render(<PairingPage onPaired={handlePaired} />);

    const input = screen.getByPlaceholderText(/6-digit PIN/i);
    expect(input).toBeInTheDocument();
    expect(input).toHaveAttribute("maxLength", "6");
  });

  it("PairingPage renders Ferryx Desktop branding helper text", () => {
    const handlePaired = vi.fn();
    render(<PairingPage onPaired={handlePaired} />);

    expect(
      screen.getByText(/Enter the 6-digit PIN from your Ferryx Desktop settings/i)
    ).toBeInTheDocument();
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

  it("RemoteSessionList renders Ferryx branding in header and empty state", () => {
    const handleSelect = vi.fn();
    const handleRefresh = vi.fn();

    render(
      <RemoteSessionList
        sessions={[]}
        onSelect={handleSelect}
        onRefresh={handleRefresh}
      />
    );

    expect(screen.getByText("Connected to Ferryx native engine")).toBeInTheDocument();
    expect(
      screen.getByText(/Open a terminal in your desktop Ferryx app to attach/i)
    ).toBeInTheDocument();
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

  it("RemoteApp renders Ferryx Remote in header and does not contain rorca", () => {
    localStorage.setItem("ferryx_remote_token", "test-token-xyz");
    render(<RemoteApp />);

    const header = screen.getByRole("banner");
    expect(header).toHaveTextContent(/Ferryx Remote/i);
    expect(header.textContent).not.toContain("rorca");
    localStorage.clear();
  });

  it("RemoteApp supports legacy rorca_remote_token with Ferryx Remote header", () => {
    localStorage.setItem("rorca_remote_token", "legacy-token-123");
    render(<RemoteApp />);

    const header = screen.getByRole("banner");
    expect(header).toHaveTextContent(/Ferryx Remote/i);
    expect(header.textContent).not.toContain("rorca");
    localStorage.clear();
  });
});
