import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import { describe, it, expect, vi, afterEach } from "vitest";
import { BrowserToolbar } from "./BrowserToolbar";
import type { BrowserTab } from "../lib/types";

afterEach(cleanup);

describe("BrowserToolbar", () => {
  const mockTab: BrowserTab = {
    kind: "browser",
    id: "tab-1",
    label: "Localhost",
    browserId: "b-1",
    url: "http://localhost:3000",
    loading: false,
    canGoBack: true,
    canGoForward: false,
  };

  it("renders address input with tab url", () => {
    render(
      <BrowserToolbar
        tab={mockTab}
        onNavigate={vi.fn()}
        onReload={vi.fn()}
      />
    );

    const input = screen.getByLabelText("URL address bar");
    expect(input).toHaveValue("http://localhost:3000");
  });

  it("submits navigated url on enter", () => {
    const onNavigate = vi.fn();
    render(
      <BrowserToolbar
        tab={mockTab}
        onNavigate={onNavigate}
        onReload={vi.fn()}
      />
    );

    const input = screen.getByLabelText("URL address bar");
    fireEvent.change(input, { target: { value: "github.com" } });
    fireEvent.submit(input);

    expect(onNavigate).toHaveBeenCalledWith("https://github.com");
  });

  it("triggers reload on reload click", () => {
    const onReload = vi.fn();
    render(
      <BrowserToolbar
        tab={mockTab}
        onNavigate={vi.fn()}
        onReload={onReload}
      />
    );

    const reloadBtn = screen.getByLabelText("Reload");
    fireEvent.click(reloadBtn);
    expect(onReload).toHaveBeenCalled();
  });
});
