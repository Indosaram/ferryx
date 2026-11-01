import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { BrowserEngineFallbackNotice as Notice } from "../lib/browserEngineFallback";
import { BrowserEngineFallbackNotice } from "./BrowserEngineFallbackNotice";

const fallback = vi.hoisted(() => ({
  listener: null as ((notice: Notice) => void) | null,
  onBrowserEngineFallback: vi.fn(),
  takeBrowserEngineFallback: vi.fn(),
}));

vi.mock("../lib/browserEngineFallback", () => ({
  onBrowserEngineFallback: fallback.onBrowserEngineFallback,
  takeBrowserEngineFallback: fallback.takeBrowserEngineFallback,
}));

describe("BrowserEngineFallbackNotice", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    fallback.listener = null;
    fallback.onBrowserEngineFallback.mockImplementation(async (listener: (notice: Notice) => void) => {
      fallback.listener = listener;
      return () => undefined;
    });
  });
  afterEach(cleanup);

  it("shows a notice stored before the pane mounted", async () => {
    fallback.takeBrowserEngineFallback.mockResolvedValue({ browserId: "b1", message: "engine did not start" });
    render(<BrowserEngineFallbackNotice browserId="b1" />);
    expect(await screen.findByTestId("browser-engine-fallback")).toHaveTextContent("engine did not start");
    expect(fallback.takeBrowserEngineFallback).toHaveBeenCalledWith("b1");
  });

  it("shows a live notice for its own tab only and can be dismissed", async () => {
    fallback.takeBrowserEngineFallback.mockResolvedValue(null);
    render(<BrowserEngineFallbackNotice browserId="b1" />);
    await vi.waitFor(() => expect(fallback.listener).not.toBeNull());
    act(() => fallback.listener?.({ browserId: "b2", message: "other tab" }));
    expect(screen.queryByTestId("browser-engine-fallback")).toBeNull();
    act(() => fallback.listener?.({ browserId: "b1", message: "engine crashed" }));
    expect(screen.getByTestId("browser-engine-fallback")).toHaveTextContent("engine crashed");
    fireEvent.click(screen.getByRole("button", { name: "Dismiss engine notice" }));
    expect(screen.queryByTestId("browser-engine-fallback")).toBeNull();
  });

  it("renders nothing when the backend cannot be asked", async () => {
    fallback.takeBrowserEngineFallback.mockRejectedValue(new Error("no ipc"));
    const { container } = render(<BrowserEngineFallbackNotice browserId="b1" />);
    await vi.waitFor(() => expect(fallback.takeBrowserEngineFallback).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });
});
