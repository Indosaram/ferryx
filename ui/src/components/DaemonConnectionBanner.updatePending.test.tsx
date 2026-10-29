import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import { DaemonConnectionBanner } from "./DaemonConnectionBanner";

const pendingError = (liveSessions: number | null) => ({
  code: "DAEMON_UPDATE_PENDING",
  message: "Update installed; terminals are still running on the previous version",
  details: { runningVersion: "2026.1013.2", liveSessions, retireRequested: true },
});

describe("DaemonConnectionBanner update pending", () => {
  beforeEach(() => invoke.mockReset());
  afterEach(() => cleanup());

  it("unknown count: button disabled, explains, sends nothing", () => {
    render(<DaemonConnectionBanner error={pendingError(null)} onRetry={vi.fn()} />);
    const button = screen.getByTestId("daemon-restart-terminals");
    expect(button).toBeDisabled();
    expect(screen.getByTestId("daemon-update-pending-text")).toHaveTextContent(
      "cannot be managed from this version and keeps running until its terminals close",
    );
    fireEvent.click(button);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("known count: confirm names n and sends Shutdown once", async () => {
    invoke.mockResolvedValueOnce({ outcome: "accepted", live: 2 });
    const onRetry = vi.fn();
    render(<DaemonConnectionBanner error={pendingError(2)} onRetry={onRetry} />);
    expect(screen.getByTestId("daemon-update-pending-text")).toHaveTextContent(
      "Update installed; 2 terminals still running on the previous version",
    );
    fireEvent.click(screen.getByTestId("daemon-restart-terminals"));
    expect(invoke).not.toHaveBeenCalled();
    expect(screen.getByTestId("daemon-restart-confirm-text")).toHaveTextContent("2 running terminals");
    fireEvent.click(screen.getByTestId("daemon-restart-confirm"));
    await waitFor(() => expect(onRetry).toHaveBeenCalledTimes(1));
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("cmd_daemon_restart_previous_terminals", { confirmLiveSessions: 2 });
  });

  it("count changed: nothing exits, the count is re-read", async () => {
    invoke
      .mockResolvedValueOnce({ outcome: "countChanged", live: 3 })
      .mockResolvedValueOnce({ runningVersion: "2026.1013.2", liveSessions: 3, retireRequested: true });
    const onRetry = vi.fn();
    render(<DaemonConnectionBanner error={pendingError(2)} onRetry={onRetry} />);
    fireEvent.click(screen.getByTestId("daemon-restart-terminals"));
    fireEvent.click(screen.getByTestId("daemon-restart-confirm"));
    await waitFor(() =>
      expect(screen.getByTestId("daemon-update-pending-text")).toHaveTextContent("3 terminals still running"),
    );
    expect(onRetry).not.toHaveBeenCalled();
    expect(invoke.mock.calls.map((call) => call[0])).toEqual([
      "cmd_daemon_restart_previous_terminals",
      "cmd_daemon_update_pending_status",
    ]);
  });
  it("restart required on the same protocol: offers the confirmed restart and sends Shutdown once", async () => {
    invoke.mockResolvedValueOnce({ outcome: "accepted", live: 1 });
    const onRetry = vi.fn();
    const error = pendingError(1);
    render(
      <DaemonConnectionBanner
        error={{ ...error, details: { ...error.details, retireRequested: false, restartRequired: true } }}
        onRetry={onRetry}
      />,
    );
    expect(screen.getByTestId("daemon-restart-terminals")).toBeEnabled();
    fireEvent.click(screen.getByTestId("daemon-restart-terminals"));
    fireEvent.click(screen.getByTestId("daemon-restart-confirm"));
    await waitFor(() => expect(onRetry).toHaveBeenCalledTimes(1));
    expect(invoke.mock.calls).toEqual([["cmd_daemon_restart_previous_terminals", { confirmLiveSessions: 1 }]]);
  });
});
