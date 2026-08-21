import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { BrowserTab } from "../lib/types";
import { BrowserPane } from "./BrowserPane";

const browserMocks = vi.hoisted(() => ({
  setBrowserBounds: vi.fn(async () => undefined),
  setBrowserVisible: vi.fn(async () => undefined),
}));

vi.mock("../lib/browserTauri", () => browserMocks);
vi.mock("./BrowserToolbar", () => ({ BrowserToolbar: () => <div data-testid="browser-toolbar" /> }));

const tab: BrowserTab = {
  kind: "browser",
  id: "tab-browser",
  label: "Browser",
  browserId: "browser-1",
  url: "http://localhost:3000",
  loading: false,
  canGoBack: false,
  canGoForward: false,
};

describe("BrowserPane native webview lifecycle", () => {
  beforeEach(() => {
    browserMocks.setBrowserBounds.mockClear();
    browserMocks.setBrowserVisible.mockClear();
  });

  it("shows the child webview while mounted and hides it during cleanup", async () => {
    const { unmount } = render(
      <BrowserPane tab={tab} onNavigate={() => undefined} onReload={() => undefined} />,
    );

    await waitFor(() => {
      expect(browserMocks.setBrowserBounds).toHaveBeenCalledWith("browser-1", {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
      });
      expect(browserMocks.setBrowserVisible).toHaveBeenCalledWith("browser-1", true);
    });

    unmount();

    await waitFor(() => {
      expect(browserMocks.setBrowserVisible).toHaveBeenLastCalledWith("browser-1", false);
    });
  });

  it("keeps an explicitly hidden child webview hidden", async () => {
    render(
      <BrowserPane tab={tab} visible={false} onNavigate={() => undefined} onReload={() => undefined} />,
    );

    await waitFor(() => {
      expect(browserMocks.setBrowserVisible).toHaveBeenCalledWith("browser-1", false);
    });
    expect(browserMocks.setBrowserBounds).not.toHaveBeenCalled();
  });
});