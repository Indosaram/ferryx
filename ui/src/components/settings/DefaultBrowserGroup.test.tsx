import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { DefaultBrowserGroup } from "./DefaultBrowserGroup";
import * as browserTauri from "../../lib/browserTauri";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

vi.mock("../../lib/browserTauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/browserTauri")>()),
  getDefaultBrowserStatus: vi.fn(),
  makeDefaultBrowser: vi.fn(),
}));

const getStatus = vi.mocked(browserTauri.getDefaultBrowserStatus);
const makeDefault = vi.mocked(browserTauri.makeDefaultBrowser);

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("DefaultBrowserGroup", () => {
  it("shows the current handler and makes Ferryx the default", async () => {
    getStatus
      .mockResolvedValueOnce({ state: "notDefault", current: "Safari" })
      .mockResolvedValueOnce({ state: "default" });
    makeDefault.mockResolvedValueOnce({ outcome: "requested" });
    render(<DefaultBrowserGroup />);

    await waitFor(() => expect(screen.getByTestId("default-browser-status").textContent).toBe("Not default"));
    expect(screen.getByText(/open in Safari\./)).toBeTruthy();

    fireEvent.click(screen.getByTestId("default-browser-action"));

    await waitFor(() => expect(screen.getByTestId("default-browser-status").textContent).toBe("Default"));
    expect(makeDefault).toHaveBeenCalledOnce();
    expect(screen.queryByTestId("default-browser-action")).toBeNull();
  });

  it("names an unrecognized handler as another app", async () => {
    getStatus.mockResolvedValue({ state: "notDefault", current: null });
    render(<DefaultBrowserGroup />);
    await waitFor(() => expect(screen.getByText(/Links from other apps open in another app\./)).toBeTruthy());
  });

  it("tells the user to finish in system settings when the OS needs it", async () => {
    getStatus.mockResolvedValue({ state: "notDefault", current: null });
    makeDefault.mockResolvedValueOnce({ outcome: "openedSystemSettings" });
    render(<DefaultBrowserGroup />);

    await waitFor(() => expect(screen.getByTestId("default-browser-action")).toBeTruthy());
    fireEvent.click(screen.getByTestId("default-browser-action"));

    await waitFor(() => expect(screen.getByText(/system settings that just opened/)).toBeTruthy());
  });

  it("reports a failed request", async () => {
    getStatus.mockResolvedValue({ state: "notDefault", current: null });
    makeDefault.mockRejectedValueOnce({ code: "INTERNAL_ERROR", message: "reg.exe failed" });
    render(<DefaultBrowserGroup />);

    await waitFor(() => expect(screen.getByTestId("default-browser-action")).toBeTruthy());
    fireEvent.click(screen.getByTestId("default-browser-action"));

    await waitFor(() => expect(screen.getByRole("alert").textContent).toBe("INTERNAL_ERROR"));
  });

  it("renders nothing where the platform cannot register a browser", async () => {
    getStatus.mockResolvedValue({ state: "unsupported" });
    const { container } = render(<DefaultBrowserGroup />);
    await waitFor(() => expect(getStatus).toHaveBeenCalled());
    await waitFor(() => expect(container.textContent).toBe(""));
  });
});
