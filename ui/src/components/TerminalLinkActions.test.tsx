import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TERMINAL_LINK_ACTION_EVENT } from "../lib/linkRouting";
import { TerminalLinkActions } from "./TerminalLinkActions";

const mockRouteHttpLink = vi.fn().mockResolvedValue("external");
vi.mock("../lib/linkRouting", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/linkRouting")>();
  return {
    ...actual,
    routeHttpLink: (...args: any[]) => mockRouteHttpLink(...args),
  };
});

let customToastRender: ((id: string) => React.ReactNode) | null = null;
const mockToast = {
  custom: vi.fn((renderFn: (id: string) => React.ReactNode, _options?: any) => {
    customToastRender = renderFn;
    return "custom-toast-id";
  }),
  dismiss: vi.fn(),
};

vi.mock("./ui/sonner", () => ({
  toast: {
    custom: (renderFn: (id: string) => React.ReactNode, options?: any) => mockToast.custom(renderFn, options),
    dismiss: (id?: any) => mockToast.dismiss(id),
  },
}));

describe("TerminalLinkActions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    customToastRender = null;
  });

  afterEach(() => {
    cleanup();
  });

  it("shows a custom toast on TERMINAL_LINK_ACTION_EVENT", () => {
    render(<TerminalLinkActions />);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_LINK_ACTION_EVENT, {
        detail: { url: "https://example.com/docs" },
      }),
    );

    expect(mockToast.custom).toHaveBeenCalledTimes(1);
    expect(customToastRender).toBeTypeOf("function");
  });

  it("routes to web browser and dismisses toast on click", async () => {
    render(<TerminalLinkActions />);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_LINK_ACTION_EVENT, {
        detail: { url: "https://example.com/docs" },
      }),
    );

    const toastElement = customToastRender!("toast-123");
    const { getByText } = render(toastElement as React.ReactElement);

    const webBtn = getByText("Web Browser");
    fireEvent.click(webBtn);

    expect(mockToast.dismiss).toHaveBeenCalledWith("toast-123");
    expect(mockRouteHttpLink).toHaveBeenCalledWith("https://example.com/docs", {
      source: "terminal",
      destination: "external",
    });
  });

  it("routes to builtin browser and dismisses toast on click", async () => {
    render(<TerminalLinkActions />);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_LINK_ACTION_EVENT, {
        detail: { url: "https://example.com/docs" },
      }),
    );

    const toastElement = customToastRender!("toast-456");
    const { getByText } = render(toastElement as React.ReactElement);

    const builtinBtn = getByText("Built-in Browser");
    fireEvent.click(builtinBtn);

    expect(mockToast.dismiss).toHaveBeenCalledWith("toast-456");
    expect(mockRouteHttpLink).toHaveBeenCalledWith("https://example.com/docs", {
      source: "terminal",
      destination: "builtin",
    });
  });

  it("dismisses toast when close button is clicked", () => {
    render(<TerminalLinkActions />);

    window.dispatchEvent(
      new CustomEvent(TERMINAL_LINK_ACTION_EVENT, {
        detail: { url: "https://example.com/docs" },
      }),
    );

    const toastElement = customToastRender!("toast-789");
    const { getByLabelText } = render(toastElement as React.ReactElement);

    const closeBtn = getByLabelText("Close link actions");
    fireEvent.click(closeBtn);

    expect(mockToast.dismiss).toHaveBeenCalledWith("toast-789");
  });
});
