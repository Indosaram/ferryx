import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

const { invokeMock, isTauriMock, isDesktopShellMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  isTauriMock: vi.fn(() => true),
  isDesktopShellMock: vi.fn((tauri: boolean) => tauri),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  isTauri: () => isTauriMock(),
}));

vi.mock("../lib/clientRuntime", () => ({
  isDesktopShell: (tauri: boolean) => isDesktopShellMock(tauri),
}));

import { DaemonSupervisionBanner } from "./DaemonSupervisionBanner";

describe("DaemonSupervisionBanner", () => {
  afterEach(() => {
    cleanup();
    invokeMock.mockReset();
    isTauriMock.mockReset().mockReturnValue(true);
    isDesktopShellMock.mockReset().mockImplementation((tauri: boolean) => tauri);
  });

  it("does not query outside the desktop shell", () => {
    isTauriMock.mockReturnValue(false);
    const { container } = render(<DaemonSupervisionBanner />);
    expect(container.firstChild).toBeNull();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("renders the supervisor refusal banner when reported", async () => {
    invokeMock.mockResolvedValueOnce({ banner: "Daemon stopped restarting after crash budget." });
    render(<DaemonSupervisionBanner />);
    const alert = await screen.findByTestId("daemon-supervision-banner");
    expect(alert.getAttribute("role")).toBe("alert");
    expect(alert.textContent).toContain("Daemon stopped restarting after crash budget.");
    expect(invokeMock).toHaveBeenCalledWith("cmd_daemon_supervision_status");
  });

  it("re-queries when refreshKey changes or window regains focus", async () => {
    invokeMock
      .mockResolvedValueOnce({ banner: null })
      .mockResolvedValueOnce({ banner: "Supervision off: restart refused." })
      .mockResolvedValueOnce({ banner: "Restart budget exhausted." });

    const { rerender } = render(<DaemonSupervisionBanner refreshKey="ok:0" />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));
    expect(screen.queryByTestId("daemon-supervision-banner")).toBeNull();

    rerender(<DaemonSupervisionBanner refreshKey="HOST_UNAVAILABLE:1" />);
    expect((await screen.findByTestId("daemon-supervision-banner")).textContent).toContain(
      "Supervision off: restart refused.",
    );

    fireEvent(window, new Event("focus"));
    await waitFor(() => {
      expect(screen.getByTestId("daemon-supervision-banner").textContent).toContain(
        "Restart budget exhausted.",
      );
    });
  });

  it("stays hidden when status invocation rejects", async () => {
    invokeMock.mockRejectedValueOnce(new Error("ipc unavailable"));
    render(<DaemonSupervisionBanner />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));
    expect(screen.queryByTestId("daemon-supervision-banner")).toBeNull();
  });
});
